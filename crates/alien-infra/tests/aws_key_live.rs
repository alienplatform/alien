//! Opt-in AWS test for the key controller in a direct setup, on real KMS.
//!
//! Set `ALIEN_AWS_KEY_LIVE_TEST=1`, `AWS_ACCOUNT_ID`, `AWS_REGION`, `AWS_ACCESS_KEY_ID` and
//! `AWS_SECRET_ACCESS_KEY`. Run with
//! `cargo nextest run -p alien-infra --all-features --test aws_key_live --run-ignored ignored-only --no-capture`.
//! It creates a throwaway management role and keys, then schedules every key for deletion with
//! the minimum pending window and deletes the role. Keys cannot be deleted at once.
#![cfg(feature = "aws")]

use std::sync::Arc;

use alien_aws_clients::{
    aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig},
    iam::{CreateRoleRequest, IamApi, IamClient},
    kms::{CreateKeyRequest, KmsApi, KmsClient, KmsTag},
    AwsClientConfigExt, AwsCredentialProvider,
};
use alien_core::{
    AwsClientConfig, ClientConfig, InitialSetupAuthority, Key, KeyFingerprint, KeyOutputs,
    Platform, ResourceLifecycle, ResourceStatus,
};
use alien_infra::{
    controller_test::SingleControllerExecutor, AwsKeyController, AwsRemoteStackManagementController,
    DefaultPlatformServiceProvider,
};
use futures_util::FutureExt;
use reqwest::{Client, Method};

/// A controller whose earlier run sent `CreateKey` and was lost before it recorded the key.
fn crashed_controller() -> AwsKeyController {
    serde_json::from_value(serde_json::json!({"state": "creatingKey", "createRequested": true}))
        .unwrap()
}

async fn executor(
    aws: &AwsClientConfig,
    prefix: &str,
    controller: AwsKeyController,
) -> SingleControllerExecutor {
    SingleControllerExecutor::builder()
        .resource(Key::new("data-key".to_string()).build())
        .resource_prefix(prefix)
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(controller)
        .platform(Platform::Aws)
        .client_config(ClientConfig::Aws(Box::new(aws.clone())))
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .setup_run(true)
        .with_dependency(
            alien_core::RemoteStackManagement::new("management".into()).build(),
            AwsRemoteStackManagementController::mock_ready(&format!("{prefix}-management")),
        )
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .await
        .unwrap()
}

fn arn(run: &SingleControllerExecutor) -> String {
    match run.outputs().unwrap().downcast_ref::<KeyOutputs>().unwrap().fingerprint.clone() {
        KeyFingerprint::Aws { key_arn } => key_arn,
        other => panic!("not an AWS key: {other:?}"),
    }
}

async fn schedule_deletion(http: &Client, aws: &AwsClientConfig, key_id: &str) {
    let body = serde_json::json!({"KeyId": key_id, "PendingWindowInDays": 7}).to_string();
    let region = &aws.region;
    let response = http
        .request(Method::POST, format!("https://kms.{region}.amazonaws.com"))
        .header("X-Amz-Target", "TrentService.ScheduleKeyDeletion")
        .header("Content-Type", "application/x-amz-json-1.1")
        .content_sha256(&body)
        .body(body);
    let response = alien_aws_clients::aws::aws_request_utils::sign_send_json::<serde_json::Value>(
        response,
        &AwsSignConfig {
            service_name: "kms".into(),
            region: region.clone(),
            credentials: aws.get_credentials(),
            signing_region: None,
        },
    )
    .await;
    eprintln!("ScheduleKeyDeletion {key_id}: {:?}", response.map(|_| "ok"));
}

#[tokio::test]
#[ignore = "creates real KMS keys and an IAM role; see module docs"]
async fn a_direct_setup_creates_adopts_refuses_a_planted_key_and_keeps_the_key_on_delete() {
    assert_eq!(std::env::var("ALIEN_AWS_KEY_LIVE_TEST").as_deref(), Ok("1"));
    let aws = AwsClientConfig::from_std_env().await.unwrap();
    let http = Client::new();
    let creds = AwsCredentialProvider::from_config(aws.clone()).await.unwrap();
    let iam = IamClient::new(http.clone(), creds.clone());
    let kms = KmsClient::new(http.clone(), creds);
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let prefix = format!("alienkey{}", &suffix[..8]);
    let planted_prefix = format!("alienplant{}", &suffix[..8]);
    // Keys left by an earlier failed run, to schedule for deletion too.
    let mut to_schedule: Vec<String> = std::env::var("ALIEN_AWS_KEY_LIVE_LEFTOVER_KEYS")
        .map(|keys| keys.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    let mut roles = vec![format!("{prefix}-management"), format!("{planted_prefix}-management")];

    let trust = serde_json::json!({"Version":"2012-10-17","Statement":[{"Effect":"Allow",
        "Principal":{"AWS": format!("arn:aws:iam::{}:root", aws.account_id)},"Action":"sts:AssumeRole"}]})
    .to_string();
    for role in &roles {
        iam.create_role(CreateRoleRequest {
            role_name: role.clone(),
            assume_role_policy_document: trust.clone(),
            permissions_boundary: None,
            path: None,
            description: Some("alien key live test, safe to delete".into()),
            max_session_duration: None,
            tags: None,
        })
        .await
        .unwrap();
    }
    // IAM is eventually consistent: a key policy naming a new role can be rejected for a while.
    tokio::time::sleep(std::time::Duration::from_secs(15)).await;

    let result = std::panic::AssertUnwindSafe(async {
        // 1. Create under the gate.
        let mut run = executor(&aws, &prefix, AwsKeyController::default()).await;
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        let first = arn(&run);
        to_schedule.push(first.clone());
        let policy = kms.get_key_policy(&first).await.unwrap();
        let tags = kms.list_resource_tags(&first).await.unwrap();
        eprintln!("CREATED {first}\nPOLICY {policy}\nTAGS {tags:?}");
        assert!(kms.get_key_rotation_status(&first).await.unwrap());

        // 2. A run that lost its state after sending CreateKey adopts the same key.
        let mut again = executor(&aws, &prefix, crashed_controller()).await;
        again.run_until_terminal().await.unwrap();
        assert_eq!(arn(&again), first, "re-run must adopt the same key");
        eprintln!("ADOPTED {}", arn(&again));

        // 3. A planted key with the deployment's tags but the wrong spec is refused.
        let planted = kms
            .create_key(CreateKeyRequest {
                description: "planted".into(),
                key_spec: "RSA_2048".into(),
                key_usage: "SIGN_VERIFY".into(),
                policy: serde_json::json!({"Version":"2012-10-17","Statement":[{"Sid":"root","Effect":"Allow",
                    "Principal":{"AWS": format!("arn:aws:iam::{}:root", aws.account_id)},"Action":"kms:*","Resource":"*"}]})
                .to_string(),
                tags: alien_core::standard_resource_tags(&planted_prefix, "data-key")
                    .into_iter()
                    .map(|(tag_key, tag_value)| KmsTag { tag_key, tag_value })
                    .collect(),
            })
            .await
            .unwrap();
        to_schedule.push(planted.key_metadata.key_id.clone());
        eprintln!("PLANTED {}", planted.key_metadata.key_id);
        let mut refused = executor(&aws, &planted_prefix, crashed_controller()).await;
        let outcome = refused.run_until_terminal().await;
        eprintln!("PLANTED RESULT status={:?} err={:?}", refused.status(), outcome.as_ref().err());
        assert_ne!(refused.status(), ResourceStatus::Running, "a planted key must not be adopted");

        // 4. Delete removes grants and keeps the key.
        run.delete().unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Deleted);
        let after = kms.describe_key(&first).await.unwrap();
        eprintln!("AFTER DELETE key state: {}", after.key_metadata.key_state);
        assert_eq!(after.key_metadata.key_state, "Enabled");
    })
    .catch_unwind()
    .await;

    for key in &to_schedule {
        schedule_deletion(&http, &aws, key).await;
    }
    for role in roles.drain(..) {
        if let Ok(list) = iam.list_role_policies(&role).await {
            for policy in list.list_role_policies_result.policy_names.map(|n| n.member).unwrap_or_default() {
                let _ = iam.delete_role_policy(&role, &policy).await;
            }
        }
        eprintln!("delete role {role}: {:?}", iam.delete_role(&role).await.is_ok());
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
