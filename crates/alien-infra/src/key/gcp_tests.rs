use super::*;
use crate::key::KeyOrigin;
use crate::{core::controller_test::SingleControllerExecutor, core::MockPlatformServiceProvider};
use alien_core::{
    permissions::PermissionProfile, InitialSetupAuthority, ResourceLifecycle, ServiceAccount,
};
use alien_error::AlienError;
use alien_gcp_clients::cloud_kms::{CryptoKey, CryptoKeyVersion, KeyRing, MockCloudKmsApi};
use std::sync::{Arc, Mutex};

const RING: &str = "projects/test-project-123/locations/us-central1/keyRings";
const RUNTIME_MEMBER: &str =
    "serviceAccount:test-runtime-sa@test-project-123.iam.gserviceaccount.com";
const CRYPTO_ROLE: &str = "roles/cloudkms.cryptoKeyEncrypterDecrypter";

fn crypto_key(name: &str, algorithm: &str, level: &str, labelled: bool) -> CryptoKey {
    CryptoKey {
        name: name.to_string(),
        purpose: "ENCRYPT_DECRYPT".to_string(),
        primary: Some(CryptoKeyVersion {
            name: format!("{name}/cryptoKeyVersions/1"),
            state: "ENABLED".to_string(),
            algorithm: algorithm.to_string(),
        }),
        labels: if labelled {
            key_labels("test", "data-key")
        } else {
            Default::default()
        },
        version_template: Some(VersionTemplate {
            algorithm: algorithm.to_string(),
            protection_level: Some(level.to_string()),
        }),
        rotation_period: Some("7776000s".to_string()),
    }
}

fn conflict() -> alien_client_core::Error {
    AlienError::new(alien_client_core::ErrorData::RemoteResourceConflict {
        resource_type: "key".to_string(),
        resource_name: "key".to_string(),
        message: "ALREADY_EXISTS".to_string(),
    })
}

fn provider(kms: MockCloudKmsApi) -> MockPlatformServiceProvider {
    let kms = Arc::new(kms);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_gcp_cloud_kms_client()
        .returning(move |_| Ok(kms.clone() as Arc<dyn CloudKmsApi>));
    provider
}

async fn executor(
    provider: MockPlatformServiceProvider,
    controller: GcpKeyController,
    setup_run: bool,
) -> SingleControllerExecutor {
    SingleControllerExecutor::builder()
        .resource(Key::new("data-key".to_string()).build())
        .resource_prefix("test")
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(controller)
        .platform(Platform::Gcp)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .setup_run(setup_run)
        .with_stack_resource(
            ServiceAccount::new("runtime-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .permission_profile(
            "runtime",
            PermissionProfile::new().resource("data-key", ["key/remote-cryptography"]),
        )
        .service_provider(Arc::new(provider))
        .build()
        .await
        .unwrap()
}

async fn refused(executor: &mut SingleControllerExecutor) {
    for _ in 0..5 {
        let _ = executor.step().await;
    }
    assert_ne!(executor.status(), ResourceStatus::Running);
    let controller = executor.internal_state::<GcpKeyController>().unwrap();
    assert!(
        controller.crypto_key_name.is_none(),
        "the key must not be adopted"
    );
}

#[tokio::test]
async fn a_direct_setup_creates_ring_key_and_bindings_from_the_plan() {
    let created = Arc::new(Mutex::new(None));
    let policy = Arc::new(Mutex::new(None));
    let mut kms = MockCloudKmsApi::new();
    kms.expect_create_key_ring()
        .times(1)
        .returning(|location, id| {
            assert_eq!(location, "projects/test-project-123/locations/us-central1");
            Ok(KeyRing {
                name: format!("{location}/keyRings/{id}"),
            })
        });
    let seen = created.clone();
    kms.expect_create_crypto_key()
        .times(1)
        .returning(move |ring, id, request| {
            assert!(ring.starts_with(RING));
            assert_eq!(id, "key");
            let name = format!("{ring}/cryptoKeys/{id}");
            *seen.lock().unwrap() = Some(request);
            Ok(crypto_key(
                &name,
                "GOOGLE_SYMMETRIC_ENCRYPTION",
                "SOFTWARE",
                true,
            ))
        });
    kms.expect_get_crypto_key_iam_policy().returning(|_| {
        Ok(IamPolicy {
            etag: Some("etag-1".into()),
            ..Default::default()
        })
    });
    let written = policy.clone();
    kms.expect_set_crypto_key_iam_policy()
        .times(1)
        .returning(move |_, policy| {
            *written.lock().unwrap() = Some(policy.clone());
            Ok(policy)
        });
    kms.expect_get_crypto_key().returning(|name| {
        Ok(crypto_key(
            name,
            "GOOGLE_SYMMETRIC_ENCRYPTION",
            "SOFTWARE",
            true,
        ))
    });
    let mut executor = executor(provider(kms), GcpKeyController::default(), true).await;
    for _ in 0..5 {
        executor.step().await.unwrap();
    }
    let request = created.lock().unwrap().take().unwrap();
    assert_eq!(request.purpose, "ENCRYPT_DECRYPT");
    assert_eq!(request.rotation_period, "7776000s");
    assert_eq!(
        request.version_template.protection_level.as_deref(),
        Some("SOFTWARE")
    );
    assert_eq!(request.labels["managed-by"], "setup");
    assert_eq!(request.labels["deployment"], "test");
    let policy = policy.lock().unwrap().take().unwrap();
    assert_eq!(policy.etag.as_deref(), Some("etag-1"));
    assert_eq!(policy.bindings.len(), 1);
    assert_eq!(policy.bindings[0].role, CRYPTO_ROLE);
    assert_eq!(policy.bindings[0].members, [RUNTIME_MEMBER]);
    assert_eq!(executor.status(), ResourceStatus::Running);
    let controller = executor.internal_state::<GcpKeyController>().unwrap();
    assert_eq!(controller.origin, KeyOrigin::Created);
    assert!(controller.access_revision.is_some());
}

#[tokio::test]
async fn outside_a_setup_run_create_refuses_and_calls_no_cloud() {
    let mut provider = MockPlatformServiceProvider::new();
    provider.expect_get_gcp_cloud_kms_client().never();
    let mut executor = executor(provider, GcpKeyController::default(), false).await;
    refused(&mut executor).await;
}

#[tokio::test]
async fn an_existing_key_that_is_ours_is_adopted_and_its_policy_replaced() {
    let policy = Arc::new(Mutex::new(None));
    let mut kms = MockCloudKmsApi::new();
    kms.expect_create_key_ring()
        .returning(|_, _| Err(conflict()));
    kms.expect_create_crypto_key()
        .returning(|_, _, _| Err(conflict()));
    kms.expect_get_crypto_key().returning(|name| {
        Ok(crypto_key(
            name,
            "GOOGLE_SYMMETRIC_ENCRYPTION",
            "SOFTWARE",
            true,
        ))
    });
    // The key carries a member nobody on this deployment added.
    kms.expect_get_crypto_key_iam_policy().returning(|_| {
        Ok(IamPolicy {
            etag: Some("e".into()),
            bindings: vec![Binding {
                role: CRYPTO_ROLE.to_string(),
                members: vec!["user:planted@example.com".to_string()],
                condition: None,
            }],
            ..Default::default()
        })
    });
    let written = policy.clone();
    kms.expect_set_crypto_key_iam_policy()
        .returning(move |_, policy| {
            *written.lock().unwrap() = Some(policy.clone());
            Ok(policy)
        });
    let mut executor = executor(provider(kms), GcpKeyController::default(), true).await;
    for _ in 0..5 {
        executor.step().await.unwrap();
    }
    let policy = policy.lock().unwrap().take().unwrap();
    assert_eq!(policy.bindings.len(), 1);
    assert_eq!(policy.bindings[0].members, [RUNTIME_MEMBER]);
}

#[tokio::test]
async fn a_planted_key_with_the_wrong_metadata_or_labels_is_not_adopted() {
    for (algorithm, level, labelled) in [
        ("GOOGLE_SYMMETRIC_ENCRYPTION", "HSM", true),
        ("EXTERNAL_SYMMETRIC_ENCRYPTION", "EXTERNAL", true),
        ("RSA_DECRYPT_OAEP_2048_SHA256", "SOFTWARE", true),
        ("GOOGLE_SYMMETRIC_ENCRYPTION", "SOFTWARE", false),
    ] {
        let mut kms = MockCloudKmsApi::new();
        kms.expect_create_key_ring()
            .returning(|_, _| Err(conflict()));
        kms.expect_create_crypto_key()
            .returning(|_, _, _| Err(conflict()));
        kms.expect_get_crypto_key()
            .returning(move |name| Ok(crypto_key(name, algorithm, level, labelled)));
        kms.expect_set_crypto_key_iam_policy().never();
        let mut executor = executor(provider(kms), GcpKeyController::default(), true).await;
        refused(&mut executor).await;
    }
}

fn created_controller() -> GcpKeyController {
    GcpKeyController {
        state: GcpKeyState::Ready,
        crypto_key_name: Some(format!("{RING}/test-data-key-aaaaaa/cryptoKeys/key")),
        primary_version: Some("v1".to_string()),
        origin: KeyOrigin::Created,
        bindings: vec![(CRYPTO_ROLE.to_string(), RUNTIME_MEMBER.to_string())],
        ..Default::default()
    }
}

#[tokio::test]
async fn deleting_a_created_key_removes_our_members_and_never_touches_the_key() {
    let policy = Arc::new(Mutex::new(None));
    let mut kms = MockCloudKmsApi::new();
    kms.expect_get_crypto_key_iam_policy().returning(|_| {
        Ok(IamPolicy {
            etag: Some("e".into()),
            bindings: vec![Binding {
                role: CRYPTO_ROLE.to_string(),
                members: vec![
                    RUNTIME_MEMBER.to_string(),
                    "user:admin@example.com".to_string(),
                ],
                condition: None,
            }],
            ..Default::default()
        })
    });
    let written = policy.clone();
    kms.expect_set_crypto_key_iam_policy()
        .times(1)
        .returning(move |_, policy| {
            *written.lock().unwrap() = Some(policy.clone());
            Ok(policy)
        });
    kms.expect_create_crypto_key().never();
    let mut executor = executor(provider(kms), created_controller(), true).await;
    executor.delete().unwrap();
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);
    let policy = policy.lock().unwrap().take().unwrap();
    assert_eq!(policy.bindings[0].members, ["user:admin@example.com"]);
}

#[tokio::test]
async fn deleting_without_setup_credentials_leaves_the_policy_alone() {
    let mut kms = MockCloudKmsApi::new();
    kms.expect_set_crypto_key_iam_policy().never();
    kms.expect_get_crypto_key_iam_policy().never();
    let mut executor = executor(provider(kms), created_controller(), false).await;
    executor.delete().unwrap();
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);
}

#[test]
fn ring_names_fit_cloud_kms_and_are_stable() {
    let id = key_ring_id(
        "a_Very_long_prefix_that_goes_on_and_on_and_on",
        "data-key-with-a-long-id-too",
    );
    assert!(id.len() <= 63, "{id}");
    assert_eq!(
        id,
        key_ring_id(
            "a_Very_long_prefix_that_goes_on_and_on_and_on",
            "data-key-with-a-long-id-too"
        )
    );
    assert!(id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
}
