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
        .contains("is created by initial setup"));

    // Setup teardown, as `alien destroy` runs it.
    let prepared = state
        .prepare_for_teardown_with_lifecycle_filter(&[ResourceLifecycle::Frozen])
        .expect("a key that was never created must not block teardown");
    assert_eq!(prepared, vec![key.id.clone()]);
    // The delete flow sees there is no key and finishes; the loop below steps it.
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
        disk_encryption_set_id: None,
    }));
    imported_key
        .set_internal_controller(Some(Box::new(AwsKeyController {
            state: AwsKeyState::RefreshFailed,
            key_arn: Some(KEY_ARN.to_string()),
            ..Default::default()
        })))
        .unwrap();
    imported.resources.insert(key.id.clone(), imported_key);
    imported
        .prepare_for_teardown_with_lifecycle_filter(&[ResourceLifecycle::Frozen])
        .expect("an imported key enters the delete flow, which refuses it");
    let mut refused = None;
    for _ in 0..4 {
        imported = teardown.step(imported).await.unwrap().next_state;
        if imported.resources[&key.id].status == ResourceStatus::DeleteFailed {
            refused = imported.resources[&key.id].error.clone();
            break;
        }
    }
    let refused = refused.expect("deleting an imported key must fail");
    assert!(
        refused
            .message
            .contains("owned by the setup that created it"),
        "{refused:?}"
    );
}

mod created_keys {
    use super::*;
    use crate::key::KeyOrigin;
    use crate::{
        core::controller_test::SingleControllerExecutor,
        remote_stack_management::AwsRemoteStackManagementController,
    };
    use alien_aws_clients::iam::MockIamApi;
    use alien_aws_clients::{
        kms::KmsApi,
        resourcegroupstagging::{
            GetResourcesResponse, MockResourceGroupsTaggingApi, ResourceTagMapping,
        },
    };
    use alien_core::{InitialSetupAuthority, Platform};
    use std::sync::Mutex;

    const NEW_ARN: &str =
        "arn:aws:kms:us-east-1:123456789012:key/aaaaaaaa-0000-0000-0000-000000000001";
    const OTHER_ARN: &str =
        "arn:aws:kms:us-east-1:123456789012:key/aaaaaaaa-0000-0000-0000-000000000002";

    fn metadata(arn: &str, spec: &str, origin: &str, state: &str) -> DescribeKeyResponse {
        DescribeKeyResponse {
            key_metadata: KeyMetadata {
                origin: Some(origin.to_string()),
                arn: arn.to_string(),
                key_id: arn.rsplit('/').next().unwrap().to_string(),
                enabled: state == "Enabled",
                key_state: state.to_string(),
                key_usage: "ENCRYPT_DECRYPT".to_string(),
                key_spec: spec.to_string(),
                deletion_date: None,
            },
        }
    }

    async fn executor(
        provider: MockPlatformServiceProvider,
        controller: AwsKeyController,
        setup_run: bool,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(Key::new("data-key".to_string()).build())
            .resource_prefix("test")
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .controller(controller)
            .platform(Platform::Aws)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .setup_run(setup_run)
            .with_dependency(
                alien_core::RemoteStackManagement::new("management".into()).build(),
                AwsRemoteStackManagementController::mock_ready("test-management"),
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap()
    }

    fn provider(
        kms: MockKmsApi,
        tagging: MockResourceGroupsTaggingApi,
        iam: MockIamApi,
    ) -> MockPlatformServiceProvider {
        let (kms, tagging, iam) = (Arc::new(kms), Arc::new(tagging), Arc::new(iam));
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_kms_client()
            .returning(move |_| Ok(kms.clone() as Arc<dyn KmsApi>));
        provider
            .expect_get_aws_resource_groups_tagging_client()
            .returning(move |_| Ok(tagging.clone()));
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider
    }

    #[tokio::test]
    async fn a_direct_setup_creates_the_key_with_the_plans_policy_and_tags() {
        let created = Arc::new(Mutex::new(Vec::new()));
        let mut kms = MockKmsApi::new();
        let seen = created.clone();
        kms.expect_create_key().times(1).returning(move |request| {
            seen.lock().unwrap().push(request);
            Ok(metadata(NEW_ARN, "SYMMETRIC_DEFAULT", "AWS_KMS", "Enabled"))
        });
        kms.expect_enable_key_rotation()
            .times(1)
            .withf(|key| key == NEW_ARN)
            .returning(|_| Ok(()));
        kms.expect_describe_key()
            .returning(|arn| Ok(metadata(arn, "SYMMETRIC_DEFAULT", "AWS_KMS", "Enabled")));
        let mut iam = MockIamApi::new();
        iam.expect_put_role_policy().returning(|_, _, _| Ok(()));
        let mut executor = executor(
            provider(kms, tagged(&[]), iam),
            AwsKeyController::default(),
            true,
        )
        .await;
        for _ in 0..4 {
            executor.step().await.unwrap();
        }
        let requests = created.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(request
            .tags
            .iter()
            .any(|tag| tag.tag_key == "alien-create-marker" && !tag.tag_value.is_empty()));
        assert_eq!(request.key_spec, "SYMMETRIC_DEFAULT");
        assert_eq!(request.key_usage, "ENCRYPT_DECRYPT");
        assert_eq!(request.description, "Encryption key 'data-key'");
        let mut tags: Vec<_> = request
            .tags
            .iter()
            .filter(|tag| tag.tag_key != "alien-create-marker")
            .map(|tag| (tag.tag_key.as_str(), tag.tag_value.as_str()))
            .collect();
        tags.sort();
        let mut expected: Vec<_> = alien_core::standard_resource_tags("test", "data-key")
            .into_iter()
            .collect();
        expected.sort();
        assert_eq!(
            tags,
            expected
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect::<Vec<_>>()
        );
        let policy: serde_json::Value = serde_json::from_str(&request.policy).unwrap();
        let denies = policy["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .find(|statement| statement["Sid"] == "DenyManagementKeyAdministration")
            .expect("the management role is denied key administration");
        assert_eq!(
            denies["Condition"]["ArnEquals"]["aws:PrincipalArn"],
            "arn:aws:iam::123456789012:role/test-management"
        );
        assert_eq!(executor.status(), ResourceStatus::Running);
        let controller = executor.internal_state::<AwsKeyController>().unwrap();
        assert_eq!(controller.origin, KeyOrigin::Created);
        assert_eq!(controller.key_arn.as_deref(), Some(NEW_ARN));
        assert!(controller.access_revision.is_some());
    }

    #[tokio::test]
    async fn outside_a_setup_run_create_refuses_and_calls_no_cloud() {
        let mut provider = MockPlatformServiceProvider::new();
        provider.expect_get_aws_kms_client().never();
        provider.expect_get_aws_iam_client().never();
        provider
            .expect_get_aws_resource_groups_tagging_client()
            .never();
        let mut executor = executor(provider, AwsKeyController::default(), false).await;
        refused(&mut executor).await;
    }

    /// Steps until the controller would have created or adopted a key, and checks it did not.
    async fn refused(executor: &mut SingleControllerExecutor) {
        for _ in 0..4 {
            let _ = executor.step().await;
        }
        assert_ne!(executor.status(), ResourceStatus::Running);
        let controller = executor.internal_state::<AwsKeyController>().unwrap();
        assert!(controller.key_arn.is_none(), "the key must not be adopted");
    }

    fn crashed_controller() -> AwsKeyController {
        AwsKeyController {
            state: AwsKeyState::CreatingKey,
            create_requested: true,
            create_marker: Some("marker-from-the-lost-attempt".to_string()),
            ..Default::default()
        }
    }

    fn tagged(arns: &[&str]) -> MockResourceGroupsTaggingApi {
        let arns: Vec<String> = arns.iter().map(|arn| arn.to_string()).collect();
        let mut tagging = MockResourceGroupsTaggingApi::new();
        tagging.expect_get_resources().returning(move |request| {
            assert_eq!(request.resource_type_filters, vec!["kms:key"]);
            // The deployment's three tags and the creation marker: a key that only carries the
            // predictable tags is never listed, so it is never adopted.
            assert_eq!(request.tag_filters.len(), 4);
            assert!(request
                .tag_filters
                .iter()
                .any(|filter| filter.key == "alien-create-marker" && !filter.values[0].is_empty()));
            Ok(GetResourcesResponse {
                pagination_token: None,
                resource_tag_mapping_list: arns
                    .iter()
                    .map(|arn| ResourceTagMapping {
                        resource_arn: arn.clone(),
                        tags: vec![],
                        compliance_details: None,
                    })
                    .collect(),
            })
        });
        tagging
    }

    #[tokio::test]
    async fn a_crash_after_create_key_adopts_the_tagged_key_and_rewrites_its_policy() {
        let mut kms = MockKmsApi::new();
        kms.expect_create_key().never();
        kms.expect_describe_key()
            .returning(|arn| Ok(metadata(arn, "SYMMETRIC_DEFAULT", "AWS_KMS", "Enabled")));
        kms.expect_put_key_policy()
            .times(1)
            .withf(|key, policy| key == NEW_ARN && policy.contains("EnableAccountAdministration"))
            .returning(|_, _| Ok(()));
        kms.expect_enable_key_rotation().returning(|_| Ok(()));
        let mut iam = MockIamApi::new();
        iam.expect_put_role_policy().returning(|_, _, _| Ok(()));
        let mut executor = executor(
            provider(kms, tagged(&[NEW_ARN]), iam),
            crashed_controller(),
            true,
        )
        .await;
        for _ in 0..3 {
            executor.step().await.unwrap();
        }
        let controller = executor.internal_state::<AwsKeyController>().unwrap();
        assert_eq!(controller.key_arn.as_deref(), Some(NEW_ARN));
        assert_eq!(controller.origin, KeyOrigin::Created);
    }

    #[tokio::test]
    async fn the_create_flag_is_saved_before_create_key_is_ever_sent() {
        let mut kms = MockKmsApi::new();
        kms.expect_create_key().never();
        let mut first = executor(
            provider(kms, MockResourceGroupsTaggingApi::new(), MockIamApi::new()),
            AwsKeyController::default(),
            true,
        )
        .await;
        // One step: CreateStart. Nothing has called the cloud yet, and the state a crash
        // here would leave behind already has the flag.
        first.step().await.unwrap();
        let saved = first.internal_state::<AwsKeyController>().unwrap();
        assert_eq!(saved.state, AwsKeyState::CreatingKey);
        assert!(saved.create_requested);
        assert!(saved.create_marker.is_some());
        assert!(saved.key_arn.is_none());

        // The rerun starts from that saved state and finds the key the lost call made.
        let mut kms = MockKmsApi::new();
        kms.expect_create_key().never();
        kms.expect_describe_key()
            .returning(|arn| Ok(metadata(arn, "SYMMETRIC_DEFAULT", "AWS_KMS", "Enabled")));
        kms.expect_put_key_policy().returning(|_, _| Ok(()));
        let mut second = executor(
            provider(kms, tagged(&[NEW_ARN]), MockIamApi::new()),
            saved.clone(),
            true,
        )
        .await;
        second.step().await.unwrap();
        let adopted = second.internal_state::<AwsKeyController>().unwrap();
        assert_eq!(adopted.key_arn.as_deref(), Some(NEW_ARN));
    }

    #[tokio::test]
    async fn a_planted_key_with_our_tags_but_the_wrong_spec_is_not_adopted() {
        for (spec, origin) in [
            ("RSA_2048", "AWS_KMS"),
            ("SYMMETRIC_DEFAULT", "EXTERNAL"),
            ("SYMMETRIC_DEFAULT", "EXTERNAL_KEY_STORE"),
        ] {
            let mut kms = MockKmsApi::new();
            kms.expect_create_key().never();
            kms.expect_put_key_policy().never();
            kms.expect_describe_key()
                .returning(move |arn| Ok(metadata(arn, spec, origin, "Enabled")));
            let mut executor = executor(
                provider(kms, tagged(&[OTHER_ARN]), MockIamApi::new()),
                crashed_controller(),
                true,
            )
            .await;
            refused(&mut executor).await;
        }
    }

    #[tokio::test]
    async fn a_tagged_key_in_another_account_is_not_adopted() {
        let foreign = "arn:aws:kms:us-east-1:999999999999:key/aaaaaaaa-0000-0000-0000-000000000009";
        let mut kms = MockKmsApi::new();
        kms.expect_create_key().never();
        kms.expect_put_key_policy().never();
        kms.expect_describe_key()
            .returning(|arn| Ok(metadata(arn, "SYMMETRIC_DEFAULT", "AWS_KMS", "Enabled")));
        let mut executor = executor(
            provider(kms, tagged(&[foreign]), MockIamApi::new()),
            crashed_controller(),
            true,
        )
        .await;
        refused(&mut executor).await;
    }

    #[tokio::test]
    async fn two_tagged_keys_fail_and_name_both() {
        let mut kms = MockKmsApi::new();
        kms.expect_create_key().never();
        kms.expect_put_key_policy().never();
        kms.expect_describe_key()
            .returning(|arn| Ok(metadata(arn, "SYMMETRIC_DEFAULT", "AWS_KMS", "Enabled")));
        let mut executor = executor(
            provider(kms, tagged(&[NEW_ARN, OTHER_ARN]), MockIamApi::new()),
            crashed_controller(),
            true,
        )
        .await;
        refused(&mut executor).await;
    }

    #[tokio::test]
    async fn deleting_a_created_key_removes_its_grants_and_never_touches_the_key() {
        let mut kms = MockKmsApi::new();
        kms.expect_create_key().never();
        kms.expect_put_key_policy().never();
        let mut iam = MockIamApi::new();
        iam.expect_delete_role_policy()
            .times(1)
            .withf(|role, policy| role == "test-access" && policy == "data-key-key-access")
            .returning(|_, _| Ok(()));
        let controller = AwsKeyController {
            state: AwsKeyState::Ready,
            key_arn: Some(NEW_ARN.to_string()),
            origin: KeyOrigin::Created,
            grants: vec![("test-access".to_string(), "data-key-key-access".to_string())],
            ..Default::default()
        };
        let mut executor = executor(
            provider(kms, MockResourceGroupsTaggingApi::new(), iam),
            controller,
            true,
        )
        .await;
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }
}
