use alien_azure_clients::long_running_operation::MockLongRunningOperationApi;
use alien_azure_clients::AzureClientConfigExt;
use std::sync::Mutex;

fn stale_lro_error() -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
        resource_type: "Azure operation".to_string(),
        resource_name: "stale-operation".to_string(),
    })
}

#[tokio::test]
async fn pre_create_queue_legacy_delete_uses_delete_waiter() {
    let app_name = "test-basic-func";
    let mut container_apps = MockContainerAppsApi::new();
    container_apps
        .expect_get_dapr_component()
        .times(1)
        .returning(move |_, _, component_name| {
            Ok(service_bus_dapr_component(
                component_name.to_string(),
                app_name,
                "default-service-bus-namespace",
                "test-trigger-queue".to_string(),
                "12345678-1234-1234-1234-123456789012",
            ))
        });
    container_apps
        .expect_delete_dapr_component()
        .times(1)
        .returning(|_, _, _| {
            Ok(OperationResult::LongRunning(LongRunningOperation {
                url: "https://management.azure.com/operations/delete-queue-legacy".to_string(),
                retry_after: Some(Duration::from_secs(7)),
                location_url: None,
            }))
        });
    container_apps
        .expect_create_or_update_dapr_component()
        .times(0);
    let provider = setup_mock_service_provider(Arc::new(container_apps), None);
    let queue = alien_core::Queue::new("trigger-queue".to_string()).build();
    let queue_controller = crate::queue::azure::AzureQueueController {
        state: crate::queue::azure::AzureQueueState::Ready,
        namespace_name: Some("default-service-bus-namespace".to_string()),
        queue_name: Some("test-trigger-queue".to_string()),
        _internal_stay_count: None,
    };
    let mut worker = basic_function();
    worker.triggers.push(WorkerTrigger::queue(&queue));
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_dependency(queue, queue_controller)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.step().await.unwrap();

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(
        controller.state,
        AzureWorkerState::WaitingForPreCreateDaprComponentDeletion
    );
    assert_eq!(
        controller.pending_operation_url.as_deref(),
        Some("https://management.azure.com/operations/delete-queue-legacy")
    );
    assert_eq!(controller.pending_operation_retry_after, Some(7));
}

#[tokio::test]
async fn pre_create_storage_legacy_delete_uses_delete_waiter() {
    let app_name = "test-basic-func";
    let mut container_apps = MockContainerAppsApi::new();
    container_apps
        .expect_get_dapr_component()
        .times(1)
        .returning(move |_, _, component_name| {
            Ok(service_bus_dapr_component(
                component_name.to_string(),
                app_name,
                "default-service-bus-namespace",
                "test-basic-func-test-storage-1-storage".to_string(),
                "12345678-1234-1234-1234-123456789012",
            ))
        });
    container_apps
        .expect_delete_dapr_component()
        .times(1)
        .returning(|_, _, _| {
            Ok(OperationResult::LongRunning(LongRunningOperation {
                url: "https://management.azure.com/operations/delete-storage-legacy".to_string(),
                retry_after: Some(Duration::from_secs(11)),
                location_url: None,
            }))
        });
    container_apps
        .expect_create_or_update_dapr_component()
        .times(0);

    let mut service_bus = MockServiceBusManagementApi::new();
    service_bus
        .expect_create_or_update_queue()
        .times(1)
        .returning(|_, _, _, _| Ok(alien_azure_clients::models::queue::SbQueue::default()));

    let mut authorization = MockAuthorizationApi::new();
    let created_assignment = Arc::new(Mutex::new(None));
    authorization
        .expect_build_role_assignment_id()
        .times(1..)
        .returning(|scope, assignment_name| {
            format!(
                "/{}/providers/Microsoft.Authorization/roleAssignments/{assignment_name}",
                scope.to_scope_string(&alien_azure_clients::AzureClientConfig::mock())
            )
        });
    let assignment_for_list = created_assignment.clone();
    authorization
        .expect_list_role_assignments()
        .times(1..)
        .returning(move |_, _| {
            Ok(assignment_for_list
                .lock()
                .expect("created role assignment lock")
                .clone()
                .into_iter()
                .collect())
        });
    let assignment_for_create = created_assignment.clone();
    authorization
        .expect_create_or_update_role_assignment_by_id()
        .times(1)
        .returning(move |assignment_id, assignment| {
            let mut created = assignment.clone();
            created.id = Some(assignment_id.clone());
            created.name = assignment_id.rsplit('/').next().map(ToString::to_string);
            *assignment_for_create
                .lock()
                .expect("created role assignment lock") = Some(created.clone());
            Ok(created)
        });

    let mut event_grid = MockEventGridApi::new();
    event_grid
        .expect_create_or_update_event_subscription()
        .times(1)
        .returning(|_, subscription_name, _| {
            Ok(alien_azure_clients::event_grid::EventSubscription {
                id: None,
                name: Some(subscription_name),
                properties: Some(
                    alien_azure_clients::event_grid::EventSubscriptionProperties {
                        provisioning_state: Some("Succeeded".to_string()),
                    },
                ),
            })
        });

    let container_apps = Arc::new(container_apps);
    let service_bus = Arc::new(service_bus);
    let authorization = Arc::new(authorization);
    let event_grid = Arc::new(event_grid);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_container_apps_client()
        .returning(move |_| Ok(container_apps.clone()));
    provider
        .expect_get_azure_service_bus_management_client()
        .returning(move |_| Ok(service_bus.clone()));
    provider
        .expect_get_azure_authorization_client()
        .returning(move |_| Ok(authorization.clone()));
    provider
        .expect_get_azure_event_grid_client()
        .returning(move |_| Ok(event_grid.clone()));

    let storage = test_storage_1();
    let mut worker = basic_function();
    worker.triggers.push(WorkerTrigger::storage(
        &storage,
        vec!["created".to_string()],
    ));
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(Arc::new(provider))
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    for _ in 0..10 {
        executor.step().await.unwrap();
        if executor
            .internal_state::<AzureWorkerController>()
            .unwrap()
            .state
            == AzureWorkerState::WaitingForPreCreateDaprComponentDeletion
        {
            break;
        }
    }

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(
        controller.state,
        AzureWorkerState::WaitingForPreCreateDaprComponentDeletion
    );
    assert_eq!(
        controller.pending_operation_url.as_deref(),
        Some("https://management.azure.com/operations/delete-storage-legacy")
    );
    assert_eq!(controller.pending_operation_retry_after, Some(11));
}

fn reconciliation_cursor(controller: &mut AzureWorkerController) {
    controller.pending_dapr_component_deletion_name = Some("tracked-trigger".to_string());
    controller.commands_dapr_component = Some("tracked-commands".to_string());
}

fn assert_reconciliation_cursor(controller: &AzureWorkerController) {
    assert_eq!(
        controller.pending_dapr_component_deletion_name.as_deref(),
        Some("tracked-trigger")
    );
    assert_eq!(
        controller.commands_dapr_component.as_deref(),
        Some("tracked-commands")
    );
}

#[tokio::test]
async fn stale_pre_create_legacy_delete_reenters_reconciliation() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, operation, _| {
            assert_eq!(operation, "DeleteDaprComponent");
            Err(stale_lro_error())
        });
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForPreCreateDaprComponentDeletion;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale".to_string());
    controller.pending_operation_retry_after = Some(30);
    reconciliation_cursor(&mut controller);
    let mut executor = SingleControllerExecutor::builder()
        .resource(basic_function())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.step().await.unwrap();

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(controller.state, AzureWorkerState::CreateStart);
    assert!(controller.pending_operation_url.is_none());
    assert!(controller.pending_operation_retry_after.is_none());
    assert_reconciliation_cursor(controller);
}

#[tokio::test]
async fn completed_fallback_commands_legacy_delete_reenters_reconciliation() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, operation, _| {
            assert_eq!(operation, "DeleteDaprComponent");
            Ok(Some("completed".to_string()))
        });
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForLegacyCommandsDaprComponentDeletionDuringCreate;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/delete-commands".to_string());
    controller.pending_operation_retry_after = Some(30);
    reconciliation_cursor(&mut controller);
    let mut executor = SingleControllerExecutor::builder()
        .resource(basic_function())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.step().await.unwrap();

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(
        controller.state,
        AzureWorkerState::CreatingCommandsInfrastructure
    );
    assert!(controller.pending_operation_url.is_none());
    assert!(controller.pending_operation_retry_after.is_none());
    assert_reconciliation_cursor(controller);
}

#[tokio::test]
async fn completed_post_create_legacy_delete_reenters_reconciliation() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, operation, _| {
            assert_eq!(operation, "DeleteDaprComponent");
            Ok(Some("completed".to_string()))
        });
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForLegacyDaprComponentDeletionDuringCreate;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/delete-trigger".to_string());
    reconciliation_cursor(&mut controller);
    let mut executor = SingleControllerExecutor::builder()
        .resource(basic_function())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.step().await.unwrap();

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(
        controller.state,
        AzureWorkerState::ConfiguringDaprComponents
    );
    assert!(controller.pending_operation_url.is_none());
    assert_reconciliation_cursor(controller);
}

#[tokio::test]
async fn stale_update_legacy_delete_reenters_reconciliation() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, operation, _| {
            assert_eq!(operation, "DeleteDaprComponent");
            Err(stale_lro_error())
        });
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::UpdateWaitingForLegacyDaprComponentDeletion;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale-update".to_string());
    reconciliation_cursor(&mut controller);
    let mut executor = SingleControllerExecutor::builder()
        .resource(basic_function())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.step().await.unwrap();

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(controller.state, AzureWorkerState::UpdateDaprComponents);
    assert!(controller.pending_operation_url.is_none());
    assert_reconciliation_cursor(controller);
}
