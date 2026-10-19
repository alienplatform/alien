use super::*;
use alien_error::ContextError;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

use crate::{
    core::MockPlatformServiceProvider,
    state_utils::StackStateExt,
    storage::{test_storage_deletes_issued, TestStorageController},
    ResourceRegistry, StackExecutor, StackResourceStateExt,
};
use alien_aws_clients::{
    kms::{DescribeKeyResponse, EncryptResponse, KeyMetadata, MockKmsApi},
    AwsClientConfig, AwsClientConfigExt,
};
use alien_core::{
    ClientConfig, DeploymentConfig, ResourceLifecycle, ResourceRef, Stack, StackState, Storage,
};

const KEY_ARN: &str = "arn:aws:kms:us-east-1:123456789012:key/11111111-2222-3333-4444-555555555555";

fn probe_error(code: &str) -> alien_error::AlienError<alien_client_core::ErrorData> {
    AlienError::new(alien_client_core::ErrorData::HttpResponseError {
        message: "KMS request failed".to_string(),
        url: "https://kms.us-east-1.amazonaws.com".to_string(),
        http_status: 400,
        http_request_text: None,
        http_response_text: Some(serde_json::json!({"__type": code}).to_string()),
    })
    .context(alien_client_core::ErrorData::GenericError {
        message: "AWS KMS Encrypt failed".to_string(),
    })
}

#[tokio::test]
async fn revoked_key_suspends_dependents_and_resumes_same_resources() {
    // 0 = enabled, 1 = revoked, 2 = an unexpected authorization failure.
    let availability = Arc::new(AtomicU8::new(0));
    let mut kms = MockKmsApi::new();
    kms.expect_describe_key()
        .withf(|key| key == KEY_ARN)
        .returning(|_| {
            Ok(DescribeKeyResponse {
                key_metadata: KeyMetadata {
                    origin: Some("EXTERNAL_KEY_STORE".to_string()),
                    arn: KEY_ARN.to_string(),
                    key_id: "key-id".to_string(),
                    enabled: true,
                    key_state: "Enabled".to_string(),
                    key_usage: "ENCRYPT_DECRYPT".to_string(),
                    key_spec: "SYMMETRIC_DEFAULT".to_string(),
                    deletion_date: None,
                },
            })
        });
    let observed = availability.clone();
    kms.expect_encrypt()
        .withf(|request| {
            request.key_id == KEY_ARN
                && request.plaintext == "YWxpZW4ta2V5LWhlYWx0aA=="
                && request.encryption_context.as_ref().unwrap()["deployment-purpose"]
                    == "key-health"
        })
        .returning(move |_| match observed.load(Ordering::SeqCst) {
            0 => Ok(EncryptResponse {
                ciphertext_blob: "probe".to_string(),
                key_id: KEY_ARN.to_string(),
            }),
            1 => Err(probe_error("com.amazonaws.kms#KeyUnavailableException")),
            _ => Err(probe_error("AccessDeniedException")),
        });
    let kms = Arc::new(kms);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_kms_client()
        .returning(move |_| Ok(kms.clone()));
    let mut registry = ResourceRegistry::with_built_ins();
    registry
        .register::<Storage>(Storage::RESOURCE_TYPE)
        .with_controller::<TestStorageController>(Platform::Aws);
    let key = Key::new("customer-key".to_string()).build();
    let storage = Storage::new("key-suspension-storage".to_string()).build();
    let dependent = Storage::new("key-suspension-transitive".to_string()).build();
    let stack = Stack::new("key-suspension".to_string())
        .add(key.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            storage,
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Key::RESOURCE_TYPE, &key.id)],
        )
        .add_with_dependencies(
            dependent,
            ResourceLifecycle::Live,
            vec![ResourceRef::new(
                Storage::RESOURCE_TYPE,
                "key-suspension-storage",
            )],
        )
        .build();
    let deployment_config = DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let config = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
    let registry = Arc::new(registry);
    let provider = Arc::new(provider);
    let executor = StackExecutor::builder(&stack, config.clone())
        .deployment_config(&deployment_config)
        .resource_registry(registry.clone())
        .service_provider(provider.clone())
        .build()
        .unwrap();
    let settings = alien_core::StackSettings::default();
    let ctx = ImportContext {
        resource_id: &key.id,
        platform: Platform::Aws,
        region: "us-east-1",
        stack_settings: &settings,
        management_config: None,
        resource: &stack.resources[&key.id],
    };
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert(
        key.id.clone(),
        AwsKeyImporter
            .import(
                AwsKeyImportData {
                    key_arn: KEY_ARN.to_string(),
                    availability_probe: true,
                },
                &ctx,
            )
            .unwrap(),
    );
    for _ in 0..8 {
        state = executor.step(state).await.unwrap().next_state;
    }
    assert!(state
        .resources
        .values()
        .all(|resource| resource.status == ResourceStatus::Running));
    let original_outputs = state.resources["key-suspension-storage"].outputs.clone();

    // An upgraded manager must accept both old import payloads and persisted
    // controllers without making Encrypt calls that their setup never granted.
    availability.store(2, Ordering::SeqCst);
    let legacy_import: AwsKeyImportData =
        serde_json::from_value(serde_json::json!({ "keyArn": KEY_ARN })).unwrap();
    let mut legacy_persisted = state.resources[&key.id].clone();
    legacy_persisted
        .internal_state
        .as_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("availabilityProbe");
    for legacy_key in [
        AwsKeyImporter.import(legacy_import, &ctx).unwrap(),
        legacy_persisted,
    ] {
        let mut legacy = state.clone();
        legacy.resources.insert(key.id.clone(), legacy_key);
        let refreshed = executor.step(legacy).await.unwrap().next_state;
        assert_eq!(refreshed.resources[&key.id].status, ResourceStatus::Running);
        assert_eq!(
            refreshed.resources[&key.id]
                .outputs
                .as_ref()
                .unwrap()
                .downcast_ref::<KeyOutputs>()
                .unwrap()
                .available,
            Some(true)
        );
    }

    availability.store(1, Ordering::SeqCst);
    state = executor.step(state).await.unwrap().next_state;
    assert_eq!(
        state.resources[&key.id]
            .outputs
            .as_ref()
            .unwrap()
            .downcast_ref::<KeyOutputs>()
            .unwrap()
            .available,
        Some(false)
    );
    let paused = state.clone();
    for _ in 0..3 {
        state = executor.step(state).await.unwrap().next_state;
    }
    for id in ["key-suspension-storage", "key-suspension-transitive"] {
        assert_eq!(
            state.resources[id].internal_state,
            paused.resources[id].internal_state
        );
        assert!(test_storage_deletes_issued(id).is_empty());
    }

    // Suspending a running resource is insufficient: an in-flight update or
    // replacement must not keep mutating after the dependency becomes unavailable.
    for replacing in [false, true] {
        let mut in_flight = paused.clone();
        let resource = in_flight
            .resources
            .get_mut("key-suspension-transitive")
            .unwrap();
        let mut controller = resource.get_internal_controller().unwrap().unwrap();
        if replacing {
            controller.transition_to_delete_start().unwrap();
        } else {
            controller.transition_to_update().unwrap();
        }
        resource.status = controller.get_status();
        resource.set_internal_controller(Some(controller)).unwrap();
        let before = resource.internal_state.clone();
        for _ in 0..3 {
            in_flight = executor.step(in_flight).await.unwrap().next_state;
        }
        assert_eq!(
            in_flight.resources["key-suspension-transitive"].internal_state,
            before
        );
        assert!(test_storage_deletes_issued("key-suspension-transitive").is_empty());
    }

    availability.store(0, Ordering::SeqCst);
    for _ in 0..2 {
        state = executor.step(state).await.unwrap().next_state;
    }
    assert_eq!(
        state.resources["key-suspension-storage"].outputs,
        original_outputs
    );
    assert!(
        state.resources["key-suspension-transitive"]
            .get_internal_controller_typed::<TestStorageController>()
            .unwrap()
            .ready_checks
            > paused.resources["key-suspension-transitive"]
                .get_internal_controller_typed::<TestStorageController>()
                .unwrap()
                .ready_checks
    );

    availability.store(2, Ordering::SeqCst);
    state = executor.step(state).await.unwrap().next_state;
    assert!(
        state.resources[&key.id].error.is_some(),
        "IAM failures must remain actionable errors"
    );
    let paused = state.resources["key-suspension-storage"]
        .internal_state
        .clone();
    state = executor.step(state).await.unwrap().next_state;
    assert_eq!(
        state.resources["key-suspension-storage"].internal_state,
        paused
    );

    // An explicit deletion remains possible during revocation.
    let delete_stack = Stack::new("key-suspension".to_string())
        .add(key, ResourceLifecycle::Frozen)
        .build();
    let delete = StackExecutor::builder(&delete_stack, config)
        .deployment_config(&deployment_config)
        .resource_registry(registry)
        .service_provider(provider)
        .build()
        .unwrap();
    for _ in 0..8 {
        state = delete.step(state).await.unwrap().next_state;
    }
    for id in ["key-suspension-storage", "key-suspension-transitive"] {
        assert_eq!(state.resources[id].status, ResourceStatus::Deleted);
        assert_eq!(test_storage_deletes_issued(id).len(), 1);
    }
}

/// A CLI setup that reaches a key fails it at create, after the setup-owned
/// resources before it were created. Tearing that deployment down must delete
/// those resources and drop the key, which never existed in the cloud.
#[tokio::test]
async fn teardown_after_a_key_failed_to_create_drops_the_key() {
    // No KMS expectations: tearing down a key that was never imported must not
    // reach AWS.
    let mut provider = MockPlatformServiceProvider::new();
    provider.expect_get_aws_kms_client().never();
    let mut registry = ResourceRegistry::with_built_ins();
    registry
        .register::<Storage>(Storage::RESOURCE_TYPE)
        .with_controller::<TestStorageController>(Platform::Aws);
    let registry = Arc::new(registry);
    let provider = Arc::new(provider);
    let key = Key::new("cli-setup-key".to_string()).build();
    let created_first = Storage::new("cli-setup-created-first".to_string()).build();
    let stack = Stack::new("cli-key-setup".to_string())
        .add(created_first, ResourceLifecycle::Frozen)
        .add_with_dependencies(
            key.clone(),
            ResourceLifecycle::Frozen,
            vec![ResourceRef::new(
                Storage::RESOURCE_TYPE,
                "cli-setup-created-first",
            )],
        )
        .build();
    let deployment_config = DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(true)
        .build();
    let config = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
    let setup = StackExecutor::builder(&stack, config.clone())
        .deployment_config(&deployment_config)
        .resource_registry(registry.clone())
        .service_provider(provider.clone())
        .build()
        .unwrap();
    let mut state = StackState::new(Platform::Aws);
    for _ in 0..8 {
        state = setup.step(state).await.unwrap().next_state;
    }
    assert_eq!(
        state.resources["cli-setup-created-first"].status,
        ResourceStatus::Running
    );
    let failed_key = &state.resources[&key.id];
    assert_eq!(failed_key.status, ResourceStatus::ProvisionFailed);
    assert!(failed_key
        .error
        .as_ref()
        .unwrap()
        .to_string()
        .contains("enter a deployment through stack import"));

    // Setup teardown, as `alien destroy` runs it.
    let prepared = state
        .prepare_for_teardown_with_lifecycle_filter(&[ResourceLifecycle::Frozen])
        .expect("a key that was never created must not block teardown");
    assert_eq!(prepared, vec![key.id.clone()]);
    assert_eq!(state.resources[&key.id].status, ResourceStatus::Deleted);
    let teardown = StackExecutor::builder(&Stack::new("teardown".to_string()).build(), config)
        .deployment_config(&deployment_config)
        .resource_registry(registry)
        .service_provider(provider)
        .step_out_of_scope_resources(false)
        .maybe_lifecycle_filter(Some(vec![ResourceLifecycle::Frozen]))
        .build()
        .unwrap();
    for _ in 0..8 {
        state = teardown.step(state).await.unwrap().next_state;
    }
    for (id, resource) in &state.resources {
        assert_eq!(resource.status, ResourceStatus::Deleted, "{id}");
    }
    assert_eq!(
        test_storage_deletes_issued("cli-setup-created-first").len(),
        1
    );

    // A key that was imported keeps refusing a teardown it cannot perform.
    let mut imported = StackState::new(Platform::Aws);
    let mut imported_key = state.resources[&key.id].clone();
    imported_key.status = ResourceStatus::RefreshFailed;
    imported_key.outputs = Some(ResourceOutputs::new(KeyOutputs {
        available: Some(true),
        fingerprint: KeyFingerprint::Aws {
            key_arn: KEY_ARN.to_string(),
        },
        wrapping_key_id: KEY_ARN.to_string(),
    }));
    imported_key
        .set_internal_controller(Some(Box::new(AwsKeyController {
            state: AwsKeyState::RefreshFailed,
            key_arn: Some(KEY_ARN.to_string()),
            ..Default::default()
        })))
        .unwrap();
    imported.resources.insert(key.id.clone(), imported_key);
    assert!(imported
        .prepare_for_teardown_with_lifecycle_filter(&[ResourceLifecycle::Frozen])
        .is_err());
}
