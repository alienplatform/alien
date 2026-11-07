//! Direct setup's management role against real IAM, with automatic management.
//!
//! Automatic management grants every resource type's management permission sets, so the role's
//! numbered managed policies depend on the whole permission registry. AWS attaches at most 20
//! managed policies to a role by default. Two scenarios, each under its own resource prefix:
//!
//! - `fresh`: setup creates the role and attaches its policies. Prints the attached count.
//! - `converge`: a role in the state a setup that hit the quota leaves behind (policies 0-19
//!   attached, 20 and 21 created but not attached) is updated. Afterwards exactly the packed
//!   policies are attached, and the rest are deleted.
//!
//! Each scenario ends by deleting, by exact name, the role, its numbered policies and its
//! permissions boundary, even when an assertion failed, and then checks that none is left.
//!
//! Ignored by default: it writes IAM in a real account. It refuses to run unless
//! `ALIEN_AWS_POLICY_QUOTA_LIVE_TEST=1`. The management role ARN is only named in the role's
//! trust policy; nothing assumes it.
//!
//! ```text
//! export ALIEN_AWS_POLICY_QUOTA_LIVE_TEST=1 AWS_TARGET_ACCOUNT_ID=... AWS_TARGET_REGION=... \
//!        AWS_TARGET_ACCESS_KEY_ID=... AWS_TARGET_SECRET_ACCESS_KEY=... AWS_MANAGEMENT_ROLE_ARN=...
//! cargo test -p alien-infra --all-features --test aws_management_policy_quota_live \
//!     -- --ignored --nocapture --test-threads=1
//! ```

#![cfg(all(feature = "aws", feature = "test-utils"))]

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use alien_aws_clients::iam::{IamApi, IamClient};
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{
    AwsClientConfig, AwsCredentials, AwsManagementConfig, ClientConfig, ManagementConfig, Platform,
    RemoteStackManagement, ResourceLifecycle, ResourceStatus, SetupManagement, StackSettings,
};
use alien_error::AlienError;
use alien_infra::controller_test::SingleControllerExecutor;
use alien_infra::{AwsRemoteStackManagementController, MockPlatformServiceProvider};
use futures::FutureExt;

/// AWS's default quota of managed policies attached to one role.
const AWS_ATTACHED_POLICIES_PER_ROLE: usize = 20;
/// The most numbered policies the controller may attach, leaving room for the account owner's.
const MAX_MANAGEMENT_POLICIES: usize = 18;
/// Numbered policies the sweep looks for past the first missing one, in case a run stopped
/// part way through deleting them.
const SWEEP_SPAN: usize = 40;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn aws_config() -> AwsClientConfig {
    assert_eq!(
        std::env::var("ALIEN_AWS_POLICY_QUOTA_LIVE_TEST").as_deref(),
        Ok("1"),
        "set ALIEN_AWS_POLICY_QUOTA_LIVE_TEST=1 to write IAM in a real account"
    );
    AwsClientConfig {
        account_id: env("AWS_TARGET_ACCOUNT_ID"),
        region: env("AWS_TARGET_REGION"),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: env("AWS_TARGET_ACCESS_KEY_ID"),
            secret_access_key: env("AWS_TARGET_SECRET_ACCESS_KEY"),
            session_token: std::env::var("AWS_TARGET_SESSION_TOKEN").ok(),
        },
        service_overrides: None,
    }
}

/// Trust an explicitly selected test-runner CA without disabling peer verification.
fn live_http_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder();
    if let Ok(path) = std::env::var("ALIEN_LIVE_TEST_CA_CERT") {
        let pem = std::fs::read(path).expect("read test-runner CA");
        builder = builder.add_root_certificate(
            reqwest::Certificate::from_pem(&pem).expect("parse test-runner CA"),
        );
    }
    builder.build().expect("live HTTP client")
}

fn live_iam(config: &AwsClientConfig) -> Arc<dyn IamApi> {
    Arc::new(IamClient::new(
        live_http_client(),
        alien_aws_clients::AwsCredentialProvider::from_config_sync(config.clone()),
    ))
}

fn prefix(scenario: &str) -> String {
    let base = std::env::var("ALIEN_AWS_POLICY_QUOTA_LIVE_PREFIX").unwrap_or("alpq".to_string());
    format!(
        "{base}{scenario}{}",
        chrono::Utc::now().format("%m%d%H%M%S")
    )
}

fn role_name(prefix: &str) -> String {
    format!("{prefix}-management")
}

fn policy_arn(config: &AwsClientConfig, name: &str) -> String {
    format!("arn:aws:iam::{}:policy/{name}", config.account_id)
}

fn numbered(prefix: &str, index: usize) -> String {
    format!("{prefix}-deployment-management-{index}")
}

fn is_not_found(error: &AlienError<ClientErrorData>) -> bool {
    matches!(
        error.error,
        Some(ClientErrorData::RemoteResourceNotFound { .. })
    )
}

async fn executor(config: &AwsClientConfig, prefix: &str) -> SingleControllerExecutor {
    let iam = live_iam(config);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(iam.clone()));
    SingleControllerExecutor::builder()
        .resource(RemoteStackManagement::new("management".to_string()).build())
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(AwsRemoteStackManagementController::default())
        .platform(Platform::Aws)
        .client_config(ClientConfig::Aws(Box::new(config.clone())))
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: env("AWS_MANAGEMENT_ROLE_ARN"),
        }))
        .stack_settings(StackSettings {
            setup_management: SetupManagement::Automatic,
            ..StackSettings::default()
        })
        .with_stack_resource(
            alien_infra::controller_test::test_function_1(),
            ResourceLifecycle::Live,
        )
        .service_provider(Arc::new(provider))
        .resource_prefix(prefix.to_string())
        .build()
        .await
        .expect("executor builds")
}

/// Names of the managed policies attached to the role, sorted.
async fn attached(iam: &dyn IamApi, role: &str) -> Vec<String> {
    let mut names: Vec<String> = iam
        .list_attached_role_policies(role)
        .await
        .expect("list attached policies")
        .list_attached_role_policies_result
        .attached_policies
        .map(|attached| attached.member)
        .unwrap_or_default()
        .into_iter()
        .map(|policy| policy.policy_name)
        .collect();
    names.sort();
    names
}

/// Numbered policies that exist, by index, searching `SWEEP_SPAN` past the last one found.
async fn existing_numbered(iam: &dyn IamApi, config: &AwsClientConfig, prefix: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < found.last().map_or(0, |last| last + 1) + SWEEP_SPAN {
        match iam
            .get_policy(&policy_arn(config, &numbered(prefix, index)))
            .await
        {
            Ok(_) => found.push(index),
            Err(error) if is_not_found(&error) => {}
            Err(error) => panic!("get policy {index}: {error}"),
        }
        index += 1;
    }
    found
}

async fn delete_policy(iam: &dyn IamApi, arn: &str) {
    let versions = match iam.list_policy_versions(arn).await {
        Ok(response) => response
            .list_policy_versions_result
            .versions
            .map(|versions| versions.member)
            .unwrap_or_default(),
        Err(error) if is_not_found(&error) => return,
        Err(error) => panic!("list versions of {arn}: {error}"),
    };
    for version in versions
        .iter()
        .filter(|version| !version.is_default_version)
    {
        iam.delete_policy_version(arn, &version.version_id)
            .await
            .unwrap_or_else(|error| panic!("delete version of {arn}: {error}"));
    }
    match iam.delete_policy(arn).await {
        Ok(()) => println!("deleted {arn}"),
        Err(error) if is_not_found(&error) => {}
        Err(error) => panic!("delete {arn}: {error}"),
    }
}

/// Deletes the role, its numbered policies and its boundary, by exact name, and checks that
/// nothing with those names is left.
async fn sweep(config: &AwsClientConfig, prefix: &str) {
    let iam = live_iam(config);
    let role = role_name(prefix);
    match iam.get_role(&role).await {
        Ok(_) => {
            for name in attached(iam.as_ref(), &role).await {
                assert!(
                    name.starts_with(prefix),
                    "{role} has a foreign policy {name}"
                );
                iam.detach_role_policy(&role, &policy_arn(config, &name))
                    .await
                    .unwrap_or_else(|error| panic!("detach {name}: {error}"));
            }
            let inline = iam
                .list_role_policies(&role)
                .await
                .expect("list inline policies")
                .list_role_policies_result
                .policy_names
                .map(|names| names.member)
                .unwrap_or_default();
            for name in inline {
                iam.delete_role_policy(&role, &name)
                    .await
                    .unwrap_or_else(|error| panic!("delete inline {name}: {error}"));
            }
            iam.delete_role(&role)
                .await
                .unwrap_or_else(|error| panic!("delete role {role}: {error}"));
            println!("deleted role {role}");
        }
        Err(error) if is_not_found(&error) => {}
        Err(error) => panic!("get role {role}: {error}"),
    }
    for index in existing_numbered(iam.as_ref(), config, prefix).await {
        delete_policy(iam.as_ref(), &policy_arn(config, &numbered(prefix, index))).await;
    }
    let boundary = policy_arn(config, &format!("{prefix}-deployment-management-boundary"));
    delete_policy(iam.as_ref(), &boundary).await;

    assert!(
        matches!(iam.get_role(&role).await, Err(ref error) if is_not_found(error)),
        "{role} is left"
    );
    assert_eq!(
        existing_numbered(iam.as_ref(), config, prefix).await,
        Vec::<usize>::new()
    );
    assert!(
        matches!(iam.get_policy(&boundary).await, Err(ref error) if is_not_found(error)),
        "{boundary} is left"
    );
    println!("sweep of {prefix}: nothing left");
}

/// Runs `scenario`, then sweeps whatever it created, and fails if either failed.
async fn with_sweep<F>(config: &AwsClientConfig, prefix: &str, scenario: F)
where
    F: Future<Output = ()>,
{
    let outcome = AssertUnwindSafe(scenario).catch_unwind().await;
    sweep(config, prefix).await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

fn numbered_names(prefix: &str, count: usize) -> Vec<String> {
    let mut names: Vec<String> = (0..count).map(|index| numbered(prefix, index)).collect();
    names.sort();
    names
}

#[tokio::test]
#[ignore = "writes IAM in a real AWS account; see the module docs"]
async fn fresh_automatic_setup_fits_the_role_quota() {
    let config = aws_config();
    let prefix = prefix("f");
    println!("prefix {prefix}");
    with_sweep(&config, &prefix, async {
        let mut executor = executor(&config, &prefix).await;
        let outcome = executor.run_until_terminal().await;
        let iam = live_iam(&config);
        let attached = attached(iam.as_ref(), &role_name(&prefix)).await;
        let created = existing_numbered(iam.as_ref(), &config, &prefix).await;
        println!(
            "setup: {:?}; status {:?}; {} numbered policies created, {} attached",
            outcome.as_ref().map_err(|error| error.to_string()),
            executor.status(),
            created.len(),
            attached.len()
        );
        outcome.expect("setup succeeds");
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert!(attached.len() <= MAX_MANAGEMENT_POLICIES, "{attached:?}");
        assert_eq!(attached, numbered_names(&prefix, attached.len()));
        assert_eq!(created, (0..attached.len()).collect::<Vec<_>>());
    })
    .await;
}

#[tokio::test]
#[ignore = "writes IAM in a real AWS account; see the module docs"]
async fn update_converges_a_role_left_at_the_quota() {
    let config = aws_config();
    let prefix = prefix("c");
    println!("prefix {prefix}");
    with_sweep(&config, &prefix, async {
        let mut executor = executor(&config, &prefix).await;
        executor
            .run_until_terminal()
            .await
            .expect("setup succeeds");
        let iam = live_iam(&config);
        let role = role_name(&prefix);
        let packed = attached(iam.as_ref(), &role).await.len();

        // The state a setup that hit the quota leaves: 22 numbered policies, 0-19 attached.
        let filler = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"iam:GetRole","Resource":"*"}]}"#;
        for index in packed..22 {
            let name = numbered(&prefix, index);
            iam.create_policy(&name, filler, None, None)
                .await
                .unwrap_or_else(|error| panic!("create {name}: {error}"));
            if index < AWS_ATTACHED_POLICIES_PER_ROLE {
                iam.attach_role_policy(&role, &policy_arn(&config, &name))
                    .await
                    .unwrap_or_else(|error| panic!("attach {name}: {error}"));
            }
        }
        assert_eq!(
            attached(iam.as_ref(), &role).await,
            numbered_names(&prefix, AWS_ATTACHED_POLICIES_PER_ROLE)
        );
        println!("seeded: 22 numbered policies, 20 attached; packed grant is {packed}");

        executor
            .update(RemoteStackManagement::new("management".to_string()).build())
            .expect("update starts");
        executor
            .run_until_terminal()
            .await
            .expect("the update converges");
        assert_eq!(executor.status(), ResourceStatus::Running);
        let after = attached(iam.as_ref(), &role).await;
        println!("after update: {} attached", after.len());
        assert_eq!(after, numbered_names(&prefix, packed));
        assert_eq!(
            existing_numbered(iam.as_ref(), &config, &prefix).await,
            (0..packed).collect::<Vec<_>>()
        );
    })
    .await;
}
