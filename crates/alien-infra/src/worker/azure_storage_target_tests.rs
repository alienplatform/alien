include!("azure_storage_target_test_support.rs");

use crate::core::controller_test::test_storage_2;

async fn run_until_storage_subscription_created(
    executor: &mut SingleControllerExecutor,
    actions: &Arc<Mutex<Vec<String>>>,
) {
    for step in 0..80 {
        let actions = actions.lock().expect("action log lock");
        if actions
            .iter()
            .any(|action| action.starts_with("create-event:"))
            && actions.iter().any(|action| action.starts_with("put-dapr:"))
        {
            return;
        }
        drop(actions);
        executor.step().await.unwrap_or_else(|error| {
            panic!(
                "storage target reconciliation failed at step {step}, state {:?}: {error}",
                executor
                    .internal_state::<AzureWorkerController>()
                    .expect("Azure worker controller")
                    .state
            )
        });
    }
    panic!(
        "storage subscription was not created; actions: {:#?}",
        actions.lock().expect("action log lock")
    );
}

fn assert_old_storage_removed_before_new_target(
    actions: &[String],
    old: &StorageTarget,
    desired: &StorageTarget,
    old_component_names: &[String],
    desired_component_name: &str,
) {
    let delete_event = action_index(
        actions,
        &format!(
            "delete-event:{}/{}",
            old.source_resource_id, old.event_subscription_name
        ),
    );
    let delete_receiver = action_index(
        actions,
        &format!("delete-rbac:{}", old.receiver_assignment_id),
    );
    let delete_queue = action_index(
        actions,
        &format!(
            "delete-queue:{}/{}/{}",
            old.resource_group, old.namespace, old.queue
        ),
    );
    let deleted_components = old_component_names
        .iter()
        .filter(|name| name.as_str() != desired_component_name)
        .map(|name| action_index(actions, &format!("delete-dapr:{name}")))
        .collect::<Vec<_>>();
    let create_queue = action_index(
        actions,
        &format!(
            "create-queue:{}/{}/{}",
            desired.resource_group, desired.namespace, desired.queue
        ),
    );
    let create_receiver = actions
        .iter()
        .position(|action| {
            action.starts_with(&format!("put-rbac:{}:", desired.receiver_assignment_id))
        })
        .expect("new storage receiver assignment");
    let put_dapr = action_index(actions, &format!("put-dapr:{desired_component_name}"));
    let create_event = action_index(
        actions,
        &format!(
            "create-event:{}/{}",
            desired.source_resource_id, desired.event_subscription_name
        ),
    );
    assert!(
        delete_event < delete_receiver && delete_receiver < delete_queue,
        "storage delivery resources must be torn down in durable order: {actions:#?}"
    );
    assert!(
        deleted_components
            .iter()
            .all(|delete_component| *delete_component < put_dapr),
        "every historical Dapr component must be removed before applying the desired component: {actions:#?}"
    );
    assert!(
        delete_queue < create_queue
            && create_queue < create_receiver
            && create_receiver < create_event
            && create_event < put_dapr,
        "the old target must be fully absent before the new target is constructed: {actions:#?}"
    );
}

#[tokio::test]
async fn imported_storage_trigger_update_checkpoints_exact_old_target_before_remote_cleanup() {
    let app_name = "test-storage-target-worker";
    let old_storage = test_storage_1();
    let new_storage = test_storage_2();
    let previous_worker = storage_trigger_worker(&old_storage);
    let desired_worker = storage_trigger_worker(&new_storage);
    let execution_client_id = "12345678-1234-1234-1234-123456789012";
    let execution_principal_id = "87654321-4321-4321-4321-210987654321";
    let old = storage_target(
        &previous_worker.id,
        &old_storage.id,
        "old-storage-account",
        "old-container",
        "mock-rg",
        "default-service-bus-namespace",
        execution_principal_id,
    );
    let desired = storage_target(
        &desired_worker.id,
        &new_storage.id,
        "new-storage-account",
        "new-container",
        "mock-rg",
        "default-service-bus-namespace",
        execution_principal_id,
    );
    let old_components = storage_components(
        app_name,
        &old_storage.id,
        &old.namespace,
        &old.queue,
        execution_client_id,
    );
    let old_component_names = old_components
        .iter()
        .map(|component| component.name.clone().expect("component name"))
        .collect::<Vec<_>>();
    let desired_component_name =
        get_azure_blob_trigger_dapr_component_name(app_name, &new_storage.id);
    let actions = Arc::new(Mutex::new(Vec::new()));
    let provider = storage_provider(
        StorageProviderExpectations {
            app_name: app_name.to_string(),
            existing_components: old_components,
            old: old.clone(),
            desired: desired.clone(),
            expected_execution_client_id: execution_client_id.to_string(),
            expected_execution_principal_id: execution_principal_id.to_string(),
            expected_container_identity_id: None,
            expect_container_app_update: true,
            deletes_are_missing: false,
        },
        actions.clone(),
    );

    let mut executor = SingleControllerExecutor::builder()
        .resource(previous_worker)
        .controller(AzureWorkerController::mock_ready(app_name))
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .with_dependency(
            old_storage.clone(),
            storage_controller(&old_storage.id, "old-storage-account", "old-container"),
        )
        .with_dependency(
            new_storage.clone(),
            storage_controller(&new_storage.id, "new-storage-account", "new-container"),
        )
        .build()
        .await
        .expect("executor should build");
    executor
        .update(desired_worker)
        .expect("storage trigger update should start");

    for step in 0..10 {
        if executor
            .internal_state::<AzureWorkerController>()
            .expect("Azure worker controller")
            .trigger_update_teardown_candidates_initialized
        {
            break;
        }
        executor
            .step()
            .await
            .unwrap_or_else(|error| panic!("storage checkpoint failed at step {step}: {error}"));
        assert!(
            actions
                .lock()
                .expect("action log lock")
                .iter()
                .all(|action| action == "update-app"),
            "no cleanup mutation is allowed before imported cursors are durable"
        );
    }

    {
        let controller = executor
            .internal_state::<AzureWorkerController>()
            .expect("Azure worker controller");
        assert!(controller.trigger_update_teardown_candidates_initialized);
        assert_eq!(controller.storage_trigger_infrastructure.len(), 1);
        let reconstructed = &controller.storage_trigger_infrastructure[0];
        assert_eq!(
            reconstructed.storage_id.as_deref(),
            Some(old.storage_id.as_str())
        );
        assert_eq!(reconstructed.source_resource_id, old.source_resource_id);
        assert_eq!(
            reconstructed.source_container_name.as_deref(),
            Some(old.source_container_name.as_str())
        );
        assert_eq!(
            reconstructed.event_subscription_name,
            old.event_subscription_name
        );
        assert_eq!(reconstructed.service_bus_resource_group, old.resource_group);
        assert_eq!(reconstructed.namespace_name, old.namespace);
        assert_eq!(reconstructed.queue_name, old.queue);
        assert_eq!(
            reconstructed.receiver_role_assignment_id.as_deref(),
            Some(old.receiver_assignment_id.as_str())
        );
        assert!(!reconstructed.queue_applied);
        assert!(!reconstructed.delivery_reconciled);
        assert_eq!(controller.dapr_components, old_component_names);
    }
    assert_eq!(
        actions.lock().expect("action log lock").as_slice(),
        ["update-app"],
        "import reconstruction must be durable before deleting any remote resource"
    );

    run_until_storage_subscription_created(&mut executor, &actions).await;

    let actions = actions.lock().expect("action log lock");
    assert_old_storage_removed_before_new_target(
        &actions,
        &old,
        &desired,
        &old_component_names,
        &desired_component_name,
    );
    let controller = executor
        .internal_state::<AzureWorkerController>()
        .expect("Azure worker controller");
    assert_eq!(controller.storage_trigger_infrastructure.len(), 1);
    let tracked = &controller.storage_trigger_infrastructure[0];
    assert_eq!(
        tracked.storage_id.as_deref(),
        Some(desired.storage_id.as_str())
    );
    assert_eq!(tracked.source_resource_id, desired.source_resource_id);
    assert_eq!(
        tracked.source_container_name.as_deref(),
        Some(desired.source_container_name.as_str())
    );
    assert_eq!(
        tracked.receiver_role_assignment_id.as_deref(),
        Some(desired.receiver_assignment_id.as_str())
    );
    assert!(tracked.queue_applied);
    assert!(tracked.delivery_reconciled);
}

#[tokio::test]
async fn equal_worker_update_reconciles_rotated_execution_identity_and_storage_delivery() {
    let app_name = "test-storage-target-worker";
    let storage = test_storage_1();
    let worker = storage_trigger_worker(&storage);
    let old_client_id = "old-execution-client";
    let old_principal_id = "old-execution-principal";
    let new_client_id = "new-execution-client";
    let new_principal_id = "new-execution-principal";
    let new_identity_id = format!(
        "/subscriptions/{SUBSCRIPTION_ID}/resourceGroups/default-resource-group/providers/Microsoft.ManagedIdentity/userAssignedIdentities/new-execution-identity"
    );
    let old = storage_target(
        &worker.id,
        &storage.id,
        "old-storage-account",
        "old-container",
        "old-service-bus-rg",
        "old-service-bus-namespace",
        old_principal_id,
    );
    let desired = storage_target(
        &worker.id,
        &storage.id,
        "new-storage-account",
        "new-container",
        "new-service-bus-rg",
        "new-service-bus-namespace",
        new_principal_id,
    );
    let existing_components = storage_components(
        app_name,
        &storage.id,
        &old.namespace,
        &old.queue,
        old_client_id,
    );
    let old_component_names = existing_components
        .iter()
        .map(|component| component.name.clone().expect("component name"))
        .collect::<Vec<_>>();
    let desired_component_name = get_azure_blob_trigger_dapr_component_name(app_name, &storage.id);
    let actions = Arc::new(Mutex::new(Vec::new()));
    let provider = storage_provider(
        StorageProviderExpectations {
            app_name: app_name.to_string(),
            existing_components,
            old: old.clone(),
            desired: desired.clone(),
            expected_execution_client_id: new_client_id.to_string(),
            expected_execution_principal_id: new_principal_id.to_string(),
            expected_container_identity_id: Some(new_identity_id.clone()),
            expect_container_app_update: true,
            deletes_are_missing: false,
        },
        actions.clone(),
    );

    let mut controller = AzureWorkerController::mock_ready(app_name);
    controller.storage_trigger_infrastructure = vec![AzureStorageTriggerInfrastructure {
        storage_id: Some(old.storage_id.clone()),
        source_resource_id: old.source_resource_id.clone(),
        source_container_name: Some(old.source_container_name.clone()),
        event_subscription_name: old.event_subscription_name.clone(),
        service_bus_resource_group: old.resource_group.clone(),
        namespace_name: old.namespace.clone(),
        queue_name: old.queue.clone(),
        receiver_role_assignment_id: Some(old.receiver_assignment_id.clone()),
        queue_applied: true,
        delivery_reconciled: true,
    }];
    controller.dapr_components = vec![desired_component_name.clone()];
    let service_account = ServiceAccount::new("default-profile-sa".to_string()).build();
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker.clone())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .with_dependency(
            storage.clone(),
            storage_controller(&storage.id, "new-storage-account", "new-container"),
        )
        .with_dependency(
            test_azure_service_bus_namespace(),
            namespace_controller("new-service-bus-namespace", "new-service-bus-rg"),
        )
        .with_dependency(
            service_account,
            rotated_service_account(&new_identity_id, new_client_id, new_principal_id),
        )
        .build()
        .await
        .expect("executor should build");
    executor
        .update(worker)
        .expect("equal worker update should start");

    executor
        .step()
        .await
        .expect("update identity on Container App");
    executor
        .step()
        .await
        .expect("observe updated Container App");
    executor
        .step()
        .await
        .expect("checkpoint identity-dependent cleanup targets");
    assert_eq!(
        actions.lock().expect("action log lock").as_slice(),
        ["update-app"],
        "identity rotation cleanup must be checkpointed before remote mutation"
    );

    run_until_storage_subscription_created(&mut executor, &actions).await;

    let actions = actions.lock().expect("action log lock");
    assert_old_storage_removed_before_new_target(
        &actions,
        &old,
        &desired,
        &old_component_names,
        &desired_component_name,
    );
    let new_receiver = actions
        .iter()
        .find(|action| action.starts_with(&format!("put-rbac:{}:", desired.receiver_assignment_id)))
        .expect("new receiver role assignment");
    assert!(new_receiver.ends_with(&format!(":{new_principal_id}")));
    let controller = executor
        .internal_state::<AzureWorkerController>()
        .expect("Azure worker controller");
    assert_eq!(controller.storage_trigger_infrastructure.len(), 1);
    assert_eq!(
        controller.storage_trigger_infrastructure[0]
            .receiver_role_assignment_id
            .as_deref(),
        Some(desired.receiver_assignment_id.as_str())
    );
    assert_eq!(
        controller.storage_trigger_infrastructure[0]
            .storage_id
            .as_deref(),
        Some(desired.storage_id.as_str())
    );
    assert!(controller.storage_trigger_infrastructure[0].queue_applied);
    assert!(controller.storage_trigger_infrastructure[0].delivery_reconciled);
    assert_eq!(
        controller.storage_trigger_infrastructure[0]
            .source_container_name
            .as_deref(),
        Some("new-container")
    );
}

fn storage_checkpoint_provider() -> Arc<MockPlatformServiceProvider> {
    let mut authorization = MockAuthorizationApi::new();
    authorization
        .expect_build_role_assignment_id()
        .returning(|scope, name| {
            format!(
                "/{}/providers/Microsoft.Authorization/roleAssignments/{name}",
                scope.to_scope_string(&alien_azure_clients::AzureClientConfig::mock())
            )
        });
    let authorization = Arc::new(authorization);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_authorization_client()
        .returning(move |_| Ok(authorization.clone()));
    Arc::new(provider)
}

fn imported_ready_storage_provider(
    stale: StorageTarget,
    desired: StorageTarget,
    actions: Arc<Mutex<Vec<String>>>,
) -> Arc<MockPlatformServiceProvider> {
    let mut service_bus = MockServiceBusManagementApi::new();
    let desired_queue = desired.clone();
    let queue_actions = actions.clone();
    service_bus
        .expect_create_or_update_queue()
        .times(1)
        .returning(move |resource_group, namespace, queue, _| {
            assert_eq!(resource_group, desired_queue.resource_group);
            assert_eq!(namespace, desired_queue.namespace);
            assert_eq!(queue, desired_queue.queue);
            record(&queue_actions, format!("create-queue:{queue}"));
            Ok(alien_azure_clients::models::queue::SbQueue::default())
        });
    let service_bus = Arc::new(service_bus);

    let role_definition_id = format!(
        "/subscriptions/{SUBSCRIPTION_ID}/providers/Microsoft.Authorization/roleDefinitions/4f6d3b9b-027b-4f4c-9142-0e5a2a2247e0"
    );
    let list_responses = Arc::new(Mutex::new(std::collections::VecDeque::from([
        vec![receiver_role_assignment(&stale, &role_definition_id)],
        Vec::new(),
        vec![receiver_role_assignment(&desired, &role_definition_id)],
    ])));
    let mut authorization = MockAuthorizationApi::new();
    authorization
        .expect_build_role_assignment_id()
        .returning(|scope, name| {
            format!(
                "/{}/providers/Microsoft.Authorization/roleAssignments/{name}",
                scope.to_scope_string(&alien_azure_clients::AzureClientConfig::mock())
            )
        });
    let desired_scope = desired
        .receiver_assignment_id
        .split("/providers/Microsoft.Authorization")
        .next()
        .expect("desired assignment scope")
        .to_string();
    let role_definition_for_list = role_definition_id.clone();
    authorization
        .expect_list_role_assignments()
        .times(3)
        .returning(move |scope, requested_role_definition| {
            assert_eq!(
                requested_role_definition.as_deref(),
                Some(role_definition_for_list.as_str())
            );
            let scope = format!(
                "/{}",
                scope
                    .to_scope_string(&alien_azure_clients::AzureClientConfig::mock())
                    .trim_start_matches('/')
            );
            assert_eq!(scope, desired_scope);
            Ok(list_responses
                .lock()
                .expect("list responses")
                .pop_front()
                .expect("expected receiver discovery response"))
        });
    let stale_assignment_id = stale.receiver_assignment_id.clone();
    let delete_actions = actions.clone();
    authorization
        .expect_delete_role_assignment_by_id()
        .times(1)
        .returning(move |assignment_id| {
            assert_eq!(assignment_id, stale_assignment_id);
            record(&delete_actions, format!("delete-rbac:{assignment_id}"));
            Ok(None)
        });
    let desired_assignment_id = desired.receiver_assignment_id.clone();
    let desired_principal_id = desired.execution_principal_id.clone();
    let put_actions = actions.clone();
    authorization
        .expect_create_or_update_role_assignment_by_id()
        .times(1)
        .returning(move |assignment_id, assignment| {
            assert_eq!(assignment_id, desired_assignment_id);
            assert_eq!(
                assignment
                    .properties
                    .as_ref()
                    .expect("role assignment properties")
                    .principal_id,
                desired_principal_id
            );
            record(&put_actions, format!("put-rbac:{assignment_id}"));
            Ok(assignment.clone())
        });
    let authorization = Arc::new(authorization);

    let mut event_grid = MockEventGridApi::new();
    let desired_event = desired;
    let event_actions = actions;
    event_grid
        .expect_create_or_update_event_subscription()
        .times(1)
        .returning(move |source_resource_id, subscription_name, request| {
            assert_eq!(source_resource_id, desired_event.source_resource_id);
            assert_eq!(subscription_name, desired_event.event_subscription_name);
            assert_eq!(
                request.properties.filter.subject_begins_with,
                format!(
                    "/blobServices/default/containers/{}/blobs/",
                    desired_event.source_container_name
                )
            );
            record(&event_actions, format!("put-event:{subscription_name}"));
            Ok(EventSubscription {
                id: None,
                name: Some(subscription_name),
                properties: Some(EventSubscriptionProperties {
                    provisioning_state: Some("Succeeded".to_string()),
                }),
            })
        });
    let event_grid = Arc::new(event_grid);

    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_authorization_client()
        .returning(move |_| Ok(authorization.clone()));
    provider
        .expect_get_azure_service_bus_management_client()
        .returning(move |_| Ok(service_bus.clone()));
    provider
        .expect_get_azure_event_grid_client()
        .returning(move |_| Ok(event_grid.clone()));
    Arc::new(provider)
}

#[tokio::test]
async fn imported_ready_worker_checkpoints_then_repairs_storage_delivery() {
    let app_name = "test-storage-target-worker";
    let storage = test_storage_1();
    let worker = storage_trigger_worker(&storage);
    let stale = storage_target(
        &worker.id,
        &storage.id,
        "storage-account",
        "storage-container",
        "service-bus-rg",
        "service-bus-namespace",
        "stale-principal",
    );
    let desired = storage_target(
        &worker.id,
        &storage.id,
        "storage-account",
        "storage-container",
        "service-bus-rg",
        "service-bus-namespace",
        "desired-principal",
    );
    let actions = Arc::new(Mutex::new(Vec::new()));
    let provider = imported_ready_storage_provider(stale.clone(), desired.clone(), actions.clone());
    let service_account = ServiceAccount::new("default-profile-sa".to_string()).build();
    let mut controller = AzureWorkerController::mock_ready(app_name);
    controller.resource_id = None;
    controller.storage_trigger_infrastructure.clear();
    controller.auxiliary_teardown_candidates_initialized = false;
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .with_dependency(
            storage.clone(),
            storage_controller(&storage.id, "storage-account", "storage-container"),
        )
        .with_dependency(
            test_azure_service_bus_namespace(),
            namespace_controller("service-bus-namespace", "service-bus-rg"),
        )
        .with_dependency(
            service_account,
            rotated_service_account("desired-identity", "desired-client", "desired-principal"),
        )
        .build()
        .await
        .expect("executor should build");

    executor.step().await.expect("checkpoint imported target");
    assert!(
        actions.lock().expect("action log lock").is_empty(),
        "the first Ready pass must not mutate remote delivery resources"
    );
    let checkpoint = executor
        .internal_state::<AzureWorkerController>()
        .expect("Azure worker controller");
    assert_eq!(checkpoint.storage_trigger_infrastructure.len(), 1);
    assert_eq!(
        checkpoint.storage_trigger_infrastructure[0]
            .receiver_role_assignment_id
            .as_deref(),
        Some(desired.receiver_assignment_id.as_str())
    );
    assert!(!checkpoint.storage_trigger_infrastructure[0].delivery_reconciled);

    for step in 0..12 {
        let before = actions.lock().expect("action log lock").len();
        executor
            .step()
            .await
            .unwrap_or_else(|error| panic!("Ready repair failed at step {step}: {error}"));
        let after = actions.lock().expect("action log lock").len();
        assert!(after <= before + 1, "at most one durable mutation per step");
        if executor
            .internal_state::<AzureWorkerController>()
            .expect("Azure worker controller")
            .storage_trigger_infrastructure[0]
            .delivery_reconciled
        {
            break;
        }
    }

    assert!(
        executor
            .internal_state::<AzureWorkerController>()
            .expect("Azure worker controller")
            .storage_trigger_infrastructure[0]
            .delivery_reconciled,
        "Ready repair must reach a proven receiver grant and exact Event Grid subscription"
    );
    assert_eq!(
        actions.lock().expect("action log lock").as_slice(),
        [
            format!("create-queue:{}", desired.queue),
            format!("delete-rbac:{}", stale.receiver_assignment_id),
            format!("put-rbac:{}", desired.receiver_assignment_id),
            format!("put-event:{}", desired.event_subscription_name),
        ]
    );
}

#[tokio::test]
async fn create_crash_then_dependency_rotation_drains_checkpointed_target_before_new_target() {
    let app_name = "test-storage-target-worker";
    let storage = test_storage_1();
    let worker = storage_trigger_worker(&storage);
    let target_a = storage_target(
        &worker.id,
        &storage.id,
        "storage-a",
        "container-a",
        "service-bus-rg-a",
        "namespace-a",
        "principal-a",
    );
    let target_b = storage_target(
        &worker.id,
        &storage.id,
        "storage-b",
        "container-b",
        "service-bus-rg-b",
        "namespace-b",
        "principal-b",
    );
    let service_account = ServiceAccount::new("default-profile-sa".to_string()).build();
    let mut controller = AzureWorkerController::mock_ready(app_name);
    controller.state = AzureWorkerState::ConfiguringDaprComponents;
    let mut first = SingleControllerExecutor::builder()
        .resource(worker.clone())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(storage_checkpoint_provider())
        .with_test_dependencies()
        .with_dependency(
            storage.clone(),
            storage_controller(&storage.id, "storage-a", "container-a"),
        )
        .with_dependency(
            test_azure_service_bus_namespace(),
            namespace_controller("namespace-a", "service-bus-rg-a"),
        )
        .with_dependency(
            service_account.clone(),
            rotated_service_account("identity-a", "client-a", "principal-a"),
        )
        .build()
        .await
        .expect("first executor");

    first.step().await.expect("checkpoint target A");
    let persisted = first
        .internal_state::<AzureWorkerController>()
        .expect("Azure worker controller")
        .clone();
    assert_eq!(persisted.storage_trigger_infrastructure.len(), 1);
    assert_eq!(
        persisted.storage_trigger_infrastructure[0].source_resource_id,
        target_a.source_resource_id
    );
    assert_eq!(
        persisted.storage_trigger_infrastructure[0]
            .storage_id
            .as_deref(),
        Some(target_a.storage_id.as_str())
    );
    assert_eq!(
        persisted.storage_trigger_infrastructure[0]
            .receiver_role_assignment_id
            .as_deref(),
        Some(target_a.receiver_assignment_id.as_str())
    );
    assert!(!persisted.storage_trigger_infrastructure[0].queue_applied);
    assert!(!persisted.storage_trigger_infrastructure[0].delivery_reconciled);

    let old_components = storage_components(
        app_name,
        &storage.id,
        &target_a.namespace,
        &target_a.queue,
        "client-a",
    );
    let old_names = old_components
        .iter()
        .map(|component| component.name.clone().expect("component name"))
        .collect::<Vec<_>>();
    let desired_component = get_azure_blob_trigger_dapr_component_name(app_name, &storage.id);
    let actions = Arc::new(Mutex::new(Vec::new()));
    let provider = storage_provider(
        StorageProviderExpectations {
            app_name: app_name.to_string(),
            existing_components: old_components,
            old: target_a.clone(),
            desired: target_b.clone(),
            expected_execution_client_id: "client-b".to_string(),
            expected_execution_principal_id: "principal-b".to_string(),
            expected_container_identity_id: None,
            expect_container_app_update: false,
            deletes_are_missing: true,
        },
        actions.clone(),
    );
    let mut resumed = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(persisted)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .with_dependency(
            storage.clone(),
            storage_controller(&storage.id, "storage-b", "container-b"),
        )
        .with_dependency(
            test_azure_service_bus_namespace(),
            namespace_controller("namespace-b", "service-bus-rg-b"),
        )
        .with_dependency(
            service_account,
            rotated_service_account("identity-b", "client-b", "principal-b"),
        )
        .build()
        .await
        .expect("resumed executor");

    for step in 0..80 {
        let before = actions.lock().expect("action log lock").len();
        resumed
            .step()
            .await
            .unwrap_or_else(|error| panic!("rotated create failed at step {step}: {error}"));
        let after = actions.lock().expect("action log lock").len();
        assert!(after <= before + 1, "at most one durable mutation per step");
        let controller = resumed
            .internal_state::<AzureWorkerController>()
            .expect("Azure worker controller");
        if controller.storage_trigger_infrastructure.len() == 1
            && controller.storage_trigger_infrastructure[0].source_resource_id
                == target_b.source_resource_id
        {
            assert!(
                !actions
                    .lock()
                    .expect("action log lock")
                    .iter()
                    .any(|action| action.starts_with("create-queue:")),
                "target B must checkpoint before B mutation"
            );
            break;
        }
    }
    run_until_storage_subscription_created(&mut resumed, &actions).await;
    assert_old_storage_removed_before_new_target(
        &actions.lock().expect("action log lock"),
        &target_a,
        &target_b,
        &old_names,
        &desired_component,
    );
}
