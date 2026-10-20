//! The manager's AWS handoff against a real CloudFormation management role.
//!
//! Drives `RemoteAccessResolver` exactly as the manager does: the manager's
//! credentials assume its own management role, which then assumes the
//! customer stack's management role, sending the deployment's ExternalId.
//! Run it against a stack created from a template without the ExternalId
//! condition (`ALIEN_EXTERNAL_ID_LIVE_EXPECT=open`), then again after updating
//! that stack to a template that sets `ManagementExternalId`
//! (`ALIEN_EXTERNAL_ID_LIVE_EXPECT=protected`):
//!
//! ```text
//! export ALIEN_EXTERNAL_ID_LIVE_TEST=1 \
//!        AWS_MANAGEMENT_ACCESS_KEY_ID=... AWS_MANAGEMENT_SECRET_ACCESS_KEY=... \
//!        AWS_MANAGEMENT_ROLE_ARN=... AWS_TARGET_ACCOUNT_ID=... AWS_TARGET_REGION=... \
//!        ALIEN_EXTERNAL_ID_LIVE_ROLE_ARN=<the stack's management role> \
//!        ALIEN_EXTERNAL_ID_LIVE_VALUE=<the ManagementExternalId value> \
//!        ALIEN_EXTERNAL_ID_LIVE_EXPECT=open|protected
//! cargo test -p alien-infra --features aws --test aws_management_external_id_live -- --ignored --nocapture
//! ```
//!
//! It creates nothing; the stack is the caller's to create and delete.

#![cfg(feature = "aws")]

use std::collections::HashMap;

use alien_aws_clients::{AwsClientConfigExt, StsApi, StsClient};
use alien_core::{
    AwsClientConfig, AwsCredentials, AwsEnvironmentInfo, AwsImpersonationConfig, ClientConfig,
    EnvironmentInfo, ManagementExternalId, Platform, RemoteStackManagement,
    RemoteStackManagementOutputs, Resource, ResourceLifecycle, ResourceOutputs, ResourceStatus,
    StackResourceState, StackState,
};
use alien_infra::RemoteAccessResolver;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn stack_state(role_arn: &str) -> StackState {
    let config = RemoteStackManagement::new("management".to_string()).build();
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert(
        config.id.clone(),
        StackResourceState::builder()
            .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(config))
            .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: role_arn.to_string(),
                access_configuration: role_arn.to_string(),
                legacy_remote_bindings_access: None,
            }))
            .lifecycle(ResourceLifecycle::Frozen)
            .dependencies(vec![])
            .build(),
    );
    state
}

#[tokio::test]
#[ignore = "assumes roles in real AWS accounts; see the module docs"]
async fn manager_handoff_sends_the_external_id_to_a_real_management_role() {
    assert_eq!(
        std::env::var("ALIEN_EXTERNAL_ID_LIVE_TEST").as_deref(),
        Ok("1"),
        "set ALIEN_EXTERNAL_ID_LIVE_TEST=1 to run against real AWS"
    );
    let region = env("AWS_TARGET_REGION");
    let target_account = env("AWS_TARGET_ACCOUNT_ID");
    let role_arn = env("ALIEN_EXTERNAL_ID_LIVE_ROLE_ARN");
    let value = ManagementExternalId::new(env("ALIEN_EXTERNAL_ID_LIVE_VALUE")).unwrap();
    let protected = match env("ALIEN_EXTERNAL_ID_LIVE_EXPECT").as_str() {
        "open" => false,
        "protected" => true,
        other => panic!("ALIEN_EXTERNAL_ID_LIVE_EXPECT must be open or protected, got {other}"),
    };

    // First hop: the manager's own management role, as `impersonate_management_sa` does.
    let manager = AwsClientConfig {
        account_id: env("AWS_MANAGEMENT_ACCOUNT_ID"),
        region: region.clone(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: env("AWS_MANAGEMENT_ACCESS_KEY_ID"),
            secret_access_key: env("AWS_MANAGEMENT_SECRET_ACCESS_KEY"),
            session_token: None,
        },
        service_overrides: None,
    }
    .impersonate(AwsImpersonationConfig {
        role_arn: env("AWS_MANAGEMENT_ROLE_ARN"),
        session_name: Some("external-id-live".to_string()),
        duration_seconds: Some(900),
        external_id: None,
        target_region: None,
    })
    .await
    .expect("the manager can assume its own management role");
    let base = ClientConfig::Aws(Box::new(manager));
    let state = stack_state(&role_arn);
    let target = EnvironmentInfo::Aws(AwsEnvironmentInfo {
        account_id: target_account.clone(),
        region,
    });
    let resolve = |external_id: Option<ManagementExternalId>| {
        let base = base.clone();
        let state = state.clone();
        let target = target.clone();
        async move {
            RemoteAccessResolver::new(HashMap::new())
                .resolve(base, &state, Some(&target), external_id.as_ref())
                .await
        }
    };

    let resolved = resolve(Some(value.clone()))
        .await
        .expect("the deployment's ExternalId opens the stack's management role");
    let ClientConfig::Aws(customer) = resolved else {
        panic!("expected AWS credentials")
    };
    let identity = StsClient::new(reqwest::Client::new(), *customer)
        .get_caller_identity()
        .await
        .expect("the handed-off credentials work")
        .get_caller_identity_result;
    assert_eq!(identity.account.as_deref(), Some(target_account.as_str()));
    let role_name = role_arn.rsplit('/').next().unwrap();
    assert!(identity
        .arn
        .unwrap()
        .contains(&format!("assumed-role/{role_name}/")));
    println!("with the ExternalId: allowed");

    let wrong = ManagementExternalId::new("0".repeat(64)).unwrap();
    for (label, external_id) in [
        ("without an ExternalId", None),
        ("with a wrong ExternalId", Some(wrong)),
    ] {
        let outcome = resolve(external_id).await;
        if protected {
            let error = outcome.expect_err("a protected role must refuse this request");
            assert_eq!(error.code, "AUTHENTICATION_FAILED");
            assert!(
                format!("{error:?}").contains("AccessDenied")
                    || serde_json::to_string(&error)
                        .unwrap()
                        .contains("REMOTE_ACCESS_DENIED"),
                "{label}: expected AccessDenied, got {error:?}"
            );
            println!("{label}: denied");
        } else {
            outcome.unwrap_or_else(|error| {
                panic!("{label}: a role without the condition must still allow it: {error:?}")
            });
            println!("{label}: allowed (role has no ExternalId condition)");
        }
    }
}
