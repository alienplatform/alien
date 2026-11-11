//! Opt-in GCP test for the key controller in a direct setup, on real Cloud KMS.
//!
//! Set `ALIEN_GCP_KEY_LIVE_TEST=1`, `GOOGLE_SERVICE_ACCOUNT_KEY` (a service account key JSON of a
//! disposable test project) and `GCP_REGION`. Run with
//! `cargo nextest run -p alien-infra --all-features --test gcp_key_live --run-ignored ignored-only --no-capture`.
//! It creates a service account, a key ring and keys. Key rings and keys cannot be deleted in
//! Cloud KMS, and the client has no call to destroy a key version, so the test deletes only the
//! service accounts. The key versions stay enabled (billed per active version) until someone
//! schedules them for destruction; the test prints the rings and keys to clean up by hand.
#![cfg(feature = "gcp")]

use std::sync::Arc;

use alien_core::{
    ClientConfig, GcpClientConfig, InitialSetupAuthority, Key, KeyFingerprint, KeyOutputs,
    PermissionProfile, Platform, ResourceLifecycle, ResourceStatus, ServiceAccount,
};
use alien_gcp_clients::{
    cloud_kms::{CloudKmsApi, CloudKmsClient, CreateCryptoKeyRequest, VersionTemplate},
    iam::{CreateServiceAccountRequest, IamApi, IamClient},
    GcpClientConfigExt,
};
use alien_infra::{
    controller_test::SingleControllerExecutor, DefaultPlatformServiceProvider, GcpKeyController,
};
use futures_util::FutureExt;
use reqwest::Client;

async fn executor(config: &GcpClientConfig, prefix: &str) -> SingleControllerExecutor {
    SingleControllerExecutor::builder()
        .resource(Key::new("data-key".to_string()).build())
        .resource_prefix(prefix)
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(GcpKeyController::default())
        .platform(Platform::Gcp)
        .client_config(ClientConfig::Gcp(Box::new(config.clone())))
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .setup_run(true)
        .with_stack_resource(
            ServiceAccount::new("runtime-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .permission_profile(
            "runtime",
            PermissionProfile::new().resource("data-key", ["key/remote-cryptography"]),
        )
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .build()
        .await
        .unwrap()
}

fn name(run: &SingleControllerExecutor) -> String {
    match run
        .outputs()
        .unwrap()
        .downcast_ref::<KeyOutputs>()
        .unwrap()
        .fingerprint
        .clone()
    {
        KeyFingerprint::Gcp { crypto_key_name } => crypto_key_name,
        other => panic!("not a GCP key: {other:?}"),
    }
}

/// The ring name the controller computes (`<prefix>-<id>` plus a digest suffix).
fn key_ring_id(prefix: &str, key_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let base: String = format!("{prefix}-{key_id}")
        .to_lowercase()
        .replace('_', "-")
        .chars()
        .take(56)
        .collect();
    let digest = Sha256::digest(format!("{prefix}/{key_id}").as_bytes());
    let suffix: String = digest
        .iter()
        .take(3)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{}-{suffix}", base.trim_matches('-'))
}

async fn service_account(iam: &IamClient, id: &str) {
    iam.create_service_account(
        id.to_string(),
        CreateServiceAccountRequest {
            service_account: alien_gcp_clients::iam::ServiceAccount {
                display_name: Some("alien key live test, safe to delete".into()),
                ..Default::default()
            },
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "creates real Cloud KMS keys and service accounts; see module docs"]
async fn a_direct_setup_creates_adopts_refuses_a_planted_key_and_keeps_the_key_on_delete() {
    assert_eq!(std::env::var("ALIEN_GCP_KEY_LIVE_TEST").as_deref(), Ok("1"));
    let config = GcpClientConfig::from_std_env().await.unwrap();
    let http = Client::new();
    let kms = CloudKmsClient::new(http.clone(), config.clone());
    let iam = IamClient::new(http.clone(), config.clone());
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let prefix = format!("alienk{}", &suffix[..6]);
    let planted_prefix = format!("alienp{}", &suffix[..6]);
    let accounts = [
        format!("{prefix}-runtime-sa"),
        format!("{planted_prefix}-runtime-sa"),
    ];
    for account in &accounts {
        service_account(&iam, account).await;
    }
    // New service accounts take a moment to be visible to IAM policy writes.
    tokio::time::sleep(std::time::Duration::from_secs(20)).await;

    let result = std::panic::AssertUnwindSafe(async {
        // 1. Create under the gate.
        let mut run = executor(&config, &prefix).await;
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        let first = name(&run);
        let key = kms.get_crypto_key(&first).await.unwrap();
        let policy = kms.get_crypto_key_iam_policy(&first).await.unwrap();
        eprintln!("CREATED {first}\nKEY {key:?}\nIAM POLICY {policy:?}");
        assert_eq!(key.purpose, "ENCRYPT_DECRYPT");
        assert!(
            !policy.bindings.is_empty(),
            "the runtime account must be bound"
        );

        // 2. A rerun with no state finds the same key and adopts it.
        let mut again = executor(&config, &prefix).await;
        again.run_until_terminal().await.unwrap();
        assert_eq!(name(&again), first, "re-run must adopt the same key");
        eprintln!("ADOPTED {}", name(&again));

        // 3. A planted key in the place a deployment would use, without its labels and with the
        // wrong purpose, is refused.
        let ring_id = key_ring_id(&planted_prefix, "data-key");
        let ring = kms
            .create_key_ring(
                &format!("projects/{}/locations/{}", config.project_id, config.region),
                &ring_id,
            )
            .await
            .unwrap();
        let planted = kms
            .create_crypto_key(
                &ring.name,
                "key",
                CreateCryptoKeyRequest {
                    // Same purpose and algorithm as ours, but without this deployment's labels.
                    purpose: "ENCRYPT_DECRYPT".into(),
                    version_template: VersionTemplate {
                        algorithm: "GOOGLE_SYMMETRIC_ENCRYPTION".into(),
                        protection_level: Some("SOFTWARE".into()),
                    },
                    labels: Default::default(),
                    rotation_period: "7776000s".into(),
                    next_rotation_time: "2027-06-01T00:00:00Z".into(),
                },
            )
            .await
            .unwrap();
        eprintln!("PLANTED {ring_id} {planted:?}");
        let mut refused = executor(&config, &planted_prefix).await;
        let outcome = refused.run_until_terminal().await;
        eprintln!(
            "PLANTED RESULT status={:?} err={:?}",
            refused.status(),
            outcome.as_ref().err()
        );
        assert_ne!(
            refused.status(),
            ResourceStatus::Running,
            "a planted key must not be adopted"
        );

        // 4. Delete removes our members and keeps the key.
        run.delete().unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Deleted);
        let after = kms.get_crypto_key(&first).await.unwrap();
        let after_policy = kms.get_crypto_key_iam_policy(&first).await.unwrap();
        eprintln!(
            "AFTER DELETE key {} still exists; IAM POLICY {after_policy:?}",
            after.name
        );
        assert!(after_policy.bindings.iter().all(|b| b.members.is_empty()));
    })
    .catch_unwind()
    .await;

    let leftovers = std::env::var("ALIEN_GCP_KEY_LIVE_LEFTOVER_ACCOUNTS").unwrap_or_default();
    let leftovers: Vec<String> = leftovers
        .split(',')
        .filter(|a| !a.is_empty())
        .map(str::to_string)
        .collect();
    for account in accounts.iter().chain(leftovers.iter()) {
        eprintln!(
            "delete service account {account}: {:?}",
            iam.delete_service_account(format!(
                "{account}@{}.iam.gserviceaccount.com",
                config.project_id
            ))
            .await
            .is_ok()
        );
    }
    eprintln!("LEFT BEHIND, schedule the key versions for destruction by hand (Cloud KMS cannot delete rings or keys): key rings with prefixes {prefix}-data-key and {planted_prefix}-data-key");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
