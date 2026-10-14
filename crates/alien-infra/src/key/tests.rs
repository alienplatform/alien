//! A key check that fails without an answer about the key (throttling, 5xx, a
//! dropped connection) must keep dependents running. Only the cloud's answer
//! that the key is disabled, deleted or denied to us pauses them.

use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

use alien_aws_clients::{
    kms::{DescribeKeyResponse, KeyMetadata, MockKmsApi},
    AwsClientConfig, AwsClientConfigExt,
};
use alien_azure_clients::{
    keyvault::{JsonWebKey, KeyAttributes, KeyBundle, MockKeyVaultKeysApi},
    AzureClientConfig, AzureClientConfigExt,
};
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{
    import::{
        data::{AwsKeyImportData, AzureKeyImportData, GcpKeyImportData},
        ImportContext,
    },
    ClientConfig, DeploymentConfig, Key, KeyOutputs, Platform, ResourceLifecycle, ResourceRef,
    ResourceStatus, Stack, StackResourceState, StackSettings, StackState, Storage,
};
use alien_error::{AlienError, Context};
use alien_gcp_clients::{
    cloud_kms::{CryptoKey, CryptoKeyVersion, MockCloudKmsApi},
    GcpClientConfig, GcpClientConfigExt,
};

use super::{AwsKeyImporter, AzureKeyImporter, GcpKeyImporter};
use crate::{
    core::MockPlatformServiceProvider, import::ResourceImporter, storage::TestStorageController,
    ResourceRegistry, StackExecutor, StackResourceStateExt,
};

const HEALTHY: u8 = 0;
const DISABLED: u8 = 1;
const REVOKED: u8 = 2;
const TRANSIENT: u8 = 3;
const TRANSPORT: u8 = 4;

const KEY_ID: &str = "customer-key";
const STORAGE_ID: &str = "key-transient-storage";

const AWS_KEY_ARN: &str =
    "arn:aws:kms:us-east-1:123456789012:key/11111111-2222-3333-4444-555555555555";
const GCP_KEY: &str = "projects/p/locations/us-east4/keyRings/r/cryptoKeys/key";
const AZURE_KEY: &str = "https://vault.vault.azure.net/keys/key";

/// An HTTP error answer as the client returns it, optionally under the
/// service-specific error the client maps it to.
fn http_error(
    status: u16,
    body: Option<&str>,
    mapped: Option<ClientErrorData>,
) -> alien_client_core::Error {
    let error = AlienError::new(ClientErrorData::HttpResponseError {
        message: format!("HTTP {status}"),
        url: "https://key.test".to_string(),
        http_status: status,
        http_request_text: None,
        http_response_text: body.map(str::to_string),
    });
    match mapped {
        Some(mapped) => error.context(mapped),
        None => error,
    }
}

fn transport_error() -> alien_client_core::Error {
    AlienError::new(ClientErrorData::HttpRequestFailed {
        message: "connection reset".to_string(),
    })
}

fn aws_error(code: &str, status: u16) -> alien_client_core::Error {
    http_error(
        status,
        Some(&serde_json::json!({ "__type": code }).to_string()),
        Some(ClientErrorData::GenericError {
            message: "AWS KMS DescribeKey failed".to_string(),
        }),
    )
}

fn denied(status: u16) -> alien_client_core::Error {
    http_error(
        status,
        None,
        Some(ClientErrorData::RemoteAccessDenied {
            resource_type: "key".to_string(),
            resource_name: "key".to_string(),
        }),
    )
}

fn unavailable(status: u16) -> alien_client_core::Error {
    http_error(
        status,
        None,
        Some(ClientErrorData::RemoteServiceUnavailable {
            message: "unavailable".to_string(),
        }),
    )
}

fn throttled() -> alien_client_core::Error {
    http_error(
        429,
        None,
        Some(ClientErrorData::RateLimitExceeded {
            message: "slow down".to_string(),
        }),
    )
}

fn aws_provider(mode: Arc<AtomicU8>, transient: alien_client_core::Error, revoked: alien_client_core::Error) -> MockPlatformServiceProvider {
    let mut kms = MockKmsApi::new();
    kms.expect_describe_key().returning(move |_| {
        let (enabled, key_state) = match mode.load(Ordering::SeqCst) {
            HEALTHY => (true, "Enabled"),
            DISABLED => (false, "Disabled"),
            REVOKED => return Err(revoked.clone()),
            TRANSIENT => return Err(transient.clone()),
            _ => return Err(transport_error()),
        };
        Ok(DescribeKeyResponse {
            key_metadata: KeyMetadata {
                origin: Some("AWS_KMS".to_string()),
                arn: AWS_KEY_ARN.to_string(),
                key_id: "key-id".to_string(),
                enabled,
                key_state: key_state.to_string(),
                key_usage: "ENCRYPT_DECRYPT".to_string(),
                key_spec: "SYMMETRIC_DEFAULT".to_string(),
                deletion_date: None,
            },
        })
    });
    let kms = Arc::new(kms);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_kms_client()
        .returning(move |_| Ok(kms.clone()));
    provider
}

fn gcp_provider(mode: Arc<AtomicU8>, transient: alien_client_core::Error, revoked: alien_client_core::Error) -> MockPlatformServiceProvider {
    let mut kms = MockCloudKmsApi::new();
    kms.expect_get_crypto_key().returning(move |_| {
        let state = match mode.load(Ordering::SeqCst) {
            HEALTHY => "ENABLED",
            DISABLED => "DISABLED",
            REVOKED => return Err(revoked.clone()),
            TRANSIENT => return Err(transient.clone()),
            _ => return Err(transport_error()),
        };
        Ok(CryptoKey {
            name: GCP_KEY.to_string(),
            purpose: "ENCRYPT_DECRYPT".to_string(),
            primary: Some(CryptoKeyVersion {
                name: format!("{GCP_KEY}/cryptoKeyVersions/1"),
                state: state.to_string(),
                algorithm: "GOOGLE_SYMMETRIC_ENCRYPTION".to_string(),
            }),
        })
    });
    let kms = Arc::new(kms);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_gcp_cloud_kms_client()
        .returning(move |_| Ok(kms.clone()));
    provider
}

fn azure_provider(mode: Arc<AtomicU8>, transient: alien_client_core::Error, revoked: alien_client_core::Error) -> MockPlatformServiceProvider {
    let mut keys = MockKeyVaultKeysApi::new();
    keys.expect_get_key().returning(move |_| {
        let enabled = match mode.load(Ordering::SeqCst) {
            HEALTHY => true,
            DISABLED => false,
            REVOKED => return Err(revoked.clone()),
            TRANSIENT => return Err(transient.clone()),
            _ => return Err(transport_error()),
        };
        Ok(KeyBundle {
            key: JsonWebKey {
                kid: format!("{AZURE_KEY}/v1"),
                kty: "RSA-HSM".to_string(),
                key_ops: vec!["wrapKey".to_string(), "unwrapKey".to_string()],
            },
            attributes: KeyAttributes {
                enabled: Some(enabled),
                recovery_level: Some("Recoverable".to_string()),
            },
        })
    });
    let keys = Arc::new(keys);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_key_vault_keys_client()
        .returning(move |_| Ok(keys.clone()));
    provider
}

fn import_key(platform: Platform, stack: &Stack) -> StackResourceState {
    let settings = StackSettings::default();
    let ctx = ImportContext {
        resource_id: KEY_ID,
        platform,
        region: "us-east-1",
        stack_settings: &settings,
        management_config: None,
        resource: &stack.resources[KEY_ID],
    };
    match platform {
        Platform::Aws => AwsKeyImporter.import(
            AwsKeyImportData {
                key_arn: AWS_KEY_ARN.to_string(),
                availability_probe: true,
            },
            &ctx,
        ),
        Platform::Gcp => GcpKeyImporter.import(
            GcpKeyImportData {
                crypto_key_name: GCP_KEY.to_string(),
                primary_version: format!("{GCP_KEY}/cryptoKeyVersions/1"),
            },
            &ctx,
        ),
        Platform::Azure => AzureKeyImporter.import(
            AzureKeyImportData {
                vault_resource_id: "/subscriptions/s/resourceGroups/g/providers/Microsoft.KeyVault/vaults/vault".to_string(),
                key_name: "key".to_string(),
                lineage_version_id: "v1".to_string(),
                key_id: format!("{AZURE_KEY}/v1"),
            },
            &ctx,
        ),
        other => panic!("no key importer for {other:?}"),
    }
    .unwrap()
}

fn key_available(state: &StackState) -> Option<bool> {
    state.resources[KEY_ID]
        .outputs
        .as_ref()
        .unwrap()
        .downcast_ref::<KeyOutputs>()
        .unwrap()
        .available
}

fn storage_ready_checks(state: &StackState) -> u32 {
    state.resources[STORAGE_ID]
        .get_internal_controller_typed::<TestStorageController>()
        .unwrap()
        .ready_checks
}

/// Drives a Key with a dependent Storage from Running through two kinds of
/// failed check. A check without an answer about the key (`TRANSIENT`, a
/// transport failure) must keep the Storage running through every retry and the
/// final RefreshFailed. A disabled key or a revocation must pause it until the
/// key is healthy again.
async fn check(platform: Platform, mode: Arc<AtomicU8>, provider: MockPlatformServiceProvider) {
    let mut registry = ResourceRegistry::with_built_ins();
    registry
        .register::<Storage>(Storage::RESOURCE_TYPE)
        .with_controller::<TestStorageController>(platform);
    let key = Key::new(KEY_ID.to_string()).build();
    let stack = Stack::new("key-transient".to_string())
        .add(key.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            Storage::new(STORAGE_ID.to_string()).build(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Key::RESOURCE_TYPE, &key.id)],
        )
        .build();
    let deployment_config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let config = match platform {
        Platform::Aws => ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
        Platform::Gcp => ClientConfig::Gcp(Box::new(GcpClientConfig::mock())),
        Platform::Azure => ClientConfig::Azure(Box::new(AzureClientConfig::mock())),
        other => panic!("no client config for {other:?}"),
    };
    let executor = StackExecutor::builder(&stack, config)
        .deployment_config(&deployment_config)
        .resource_registry(Arc::new(registry))
        .service_provider(Arc::new(provider))
        .build()
        .unwrap();

    mode.store(HEALTHY, Ordering::SeqCst);
    let mut running = StackState::new(platform);
    running
        .resources
        .insert(KEY_ID.to_string(), import_key(platform, &stack));
    for _ in 0..8 {
        running = executor.step(running).await.unwrap().next_state;
    }
    assert!(running
        .resources
        .values()
        .all(|resource| resource.status == ResourceStatus::Running));
    assert_eq!(key_available(&running), Some(true));

    for failure in [TRANSIENT, TRANSPORT] {
        mode.store(failure, Ordering::SeqCst);
        let mut state = running.clone();
        // MAX_RETRIES (10) failed checks end in RefreshFailed.
        for step in 1..=12 {
            state = executor.step(state).await.unwrap().next_state;
            assert_eq!(
                key_available(&state),
                Some(true),
                "failure mode {failure}, step {step}: a failed check must keep the last observation"
            );
            assert!(state.resources[KEY_ID].error.is_some());
        }
        assert_eq!(state.resources[KEY_ID].status, ResourceStatus::RefreshFailed);
        assert_eq!(
            storage_ready_checks(&state),
            storage_ready_checks(&running) + 12,
            "failure mode {failure}: the dependent must keep being reconciled"
        );
    }

    for pause in [DISABLED, REVOKED] {
        mode.store(pause, Ordering::SeqCst);
        let mut state = executor.step(running.clone()).await.unwrap().next_state;
        assert_eq!(key_available(&state), Some(false), "pause mode {pause}");
        assert_eq!(state.resources[KEY_ID].error.is_some(), pause == REVOKED);
        let paused_checks = storage_ready_checks(&state);
        for _ in 0..3 {
            state = executor.step(state).await.unwrap().next_state;
        }
        assert_eq!(key_available(&state), Some(false), "pause mode {pause}");
        assert_eq!(
            storage_ready_checks(&state),
            paused_checks,
            "pause mode {pause}: the dependent must be paused"
        );

        mode.store(HEALTHY, Ordering::SeqCst);
        for _ in 0..2 {
            state = executor.step(state).await.unwrap().next_state;
        }
        assert_eq!(key_available(&state), Some(true));
        assert_eq!(state.resources[KEY_ID].status, ResourceStatus::Running);
        assert!(storage_ready_checks(&state) > paused_checks);
    }
}

#[tokio::test]
async fn aws_key_pauses_dependents_only_on_a_confirmed_key_state() {
    for (transient, revoked) in [
        (
            aws_error("ThrottlingException", 400),
            aws_error("AccessDeniedException", 400),
        ),
        (
            aws_error("KMSInternalException", 500),
            aws_error("NotFoundException", 400),
        ),
        (
            // An expired or rejected credential is ours, not the customer's key.
            aws_error("UnrecognizedClientException", 400),
            aws_error("AccessDeniedException", 400),
        ),
    ] {
        let mode = Arc::new(AtomicU8::new(HEALTHY));
        let provider = aws_provider(mode.clone(), transient, revoked);
        check(Platform::Aws, mode, provider).await;
    }
}

#[tokio::test]
async fn gcp_key_pauses_dependents_only_on_a_confirmed_key_state() {
    for (transient, revoked) in [(throttled(), denied(403)), (unavailable(503), denied(404))] {
        let mode = Arc::new(AtomicU8::new(HEALTHY));
        let provider = gcp_provider(mode.clone(), transient, revoked);
        check(Platform::Gcp, mode, provider).await;
    }
}

#[tokio::test]
async fn azure_key_pauses_dependents_only_on_a_confirmed_key_state() {
    for (transient, revoked) in [
        (throttled(), denied(403)),
        (unavailable(503), denied(404)),
        // A 401 that survives the token refresh is our credential, not the key.
        (denied(401), denied(403)),
    ] {
        let mode = Arc::new(AtomicU8::new(HEALTHY));
        let provider = azure_provider(mode.clone(), transient, revoked);
        check(Platform::Azure, mode, provider).await;
    }
}
