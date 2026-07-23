fn stale_lro_error() -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
        resource_type: "Azure operation".to_string(),
        resource_name: "stale-operation".to_string(),
    })
}

#[tokio::test]
async fn stale_legacy_migration_delete_reenters_authoritative_reconciliation() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Err(stale_lro_error()));
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForDaprComponentNameMigrationOperation;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale-migration-delete".to_string());
    controller.dapr_component_naming_version = 0;
    controller.dapr_components = vec!["still-tracked".to_string()];
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
        AzureWorkerState::MigratingDaprComponentNames
    );
    assert!(controller.pending_operation_url.is_none());
    assert_eq!(controller.dapr_components, ["still-tracked"]);
    assert_eq!(controller.dapr_component_naming_version, 0);
}

#[tokio::test]
async fn stale_commands_removal_migration_preserves_cursor_for_reconciliation() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Err(stale_lro_error()));
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForDaprComponentNameMigrationOperation;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale-commands-removal".to_string());
    controller.commands_dapr_component = Some("legacy-commands-component".to_string());
    controller.dapr_component_naming_version = 0;
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
        AzureWorkerState::MigratingDaprComponentNames
    );
    assert_eq!(
        controller.commands_dapr_component.as_deref(),
        Some("legacy-commands-component")
    );
    assert_eq!(controller.dapr_component_naming_version, 0);
}

#[tokio::test]
async fn stale_tracked_migration_delete_does_not_consume_component_cursor() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Err(stale_lro_error()));
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForDaprComponentNameMigrationOperation;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale-tracked-removal".to_string());
    controller.pending_dapr_component_deletion_name = Some("tracked-component".to_string());
    controller.dapr_components = vec!["tracked-component".to_string()];
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
        AzureWorkerState::MigratingDaprComponentNames
    );
    assert_eq!(
        controller.pending_dapr_component_deletion_name.as_deref(),
        Some("tracked-component")
    );
    assert_eq!(controller.dapr_components, ["tracked-component"]);
}

#[tokio::test]
async fn delete_treats_stale_legacy_operation_as_complete_without_consuming_dapr_cursor() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Err(stale_lro_error()));
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale".to_string());
    controller.pending_dapr_component_deletion_name = Some("still-pending".to_string());
    let mut executor = SingleControllerExecutor::builder()
        .resource(basic_function())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.delete().unwrap();
    executor.step().await.unwrap();
    executor.step().await.unwrap();

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(controller.state, AzureWorkerState::DeleteStart);
    assert!(controller.pending_operation_url.is_none());
    assert_eq!(
        controller.pending_dapr_component_deletion_name.as_deref(),
        Some("still-pending")
    );
}

#[tokio::test]
async fn stale_dapr_delete_operation_rechecks_target_before_clearing_cursor() {
    let component_name = "tracked-component".to_string();
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Err(stale_lro_error()));
    let mut container_apps = MockContainerAppsApi::new();
    container_apps
        .expect_get_dapr_component()
        .times(1)
        .returning(|_, _, component_name| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Dapr component".to_string(),
                    resource_name: component_name.to_string(),
                },
            ))
        });
    let provider = setup_mock_service_provider(Arc::new(container_apps), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForDaprComponentDeletion;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale".to_string());
    controller.pending_dapr_component_deletion_name = Some(component_name.clone());
    controller.dapr_components = vec![component_name.clone()];
    controller.dapr_component_deletion_candidates_initialized = true;
    controller.auxiliary_teardown_candidates_initialized = true;
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
    {
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();
        assert_eq!(controller.state, AzureWorkerState::DeletingDaprComponents);
        assert_eq!(
            controller.pending_dapr_component_deletion_name.as_deref(),
            Some(component_name.as_str())
        );
    }
    executor.step().await.unwrap();

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert!(controller.pending_dapr_component_deletion_name.is_none());
    assert!(!controller.dapr_components.contains(&component_name));
}

#[tokio::test]
async fn stale_commands_delete_operation_rechecks_target_before_advancing() {
    let component_name = "legacy-commands-component".to_string();
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Err(stale_lro_error()));
    let mut container_apps = MockContainerAppsApi::new();
    container_apps
        .expect_get_dapr_component()
        .times(1)
        .returning(|_, _, component_name| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Dapr component".to_string(),
                    resource_name: component_name.to_string(),
                },
            ))
        });
    let provider = setup_mock_service_provider(Arc::new(container_apps), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForCommandsDaprComponentDeletion;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale".to_string());
    controller.commands_dapr_component = Some(component_name.clone());
    controller.commands_dapr_component_deletion_candidates = vec![component_name.clone()];
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
    assert_eq!(
        executor
            .internal_state::<AzureWorkerController>()
            .unwrap()
            .commands_dapr_component
            .as_deref(),
        Some(component_name.as_str())
    );
    executor.step().await.unwrap();

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert!(controller.commands_dapr_component.is_none());
    assert!(controller
        .commands_dapr_component_deletion_candidates
        .is_empty());
}

#[tokio::test]
async fn stale_pre_create_commands_setup_delete_reenters_setup_without_consuming_target() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Err(stale_lro_error()));
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForPreCreateDaprComponentDeletion;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale".to_string());
    controller.commands_dapr_component = Some("tracked-commands-component".to_string());
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
    assert_eq!(
        controller.commands_dapr_component.as_deref(),
        Some("tracked-commands-component")
    );
}

#[tokio::test]
async fn completed_update_commands_setup_delete_reenters_setup_without_consuming_target() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Ok(Some("completed".to_string())));
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::UpdateWaitingForCommandsDaprComponentDeletionForSetup;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/delete-component".to_string());
    controller.commands_dapr_component = Some("tracked-commands-component".to_string());
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
    assert_eq!(
        controller.commands_dapr_component.as_deref(),
        Some("tracked-commands-component")
    );
}

#[tokio::test]
async fn stale_container_app_delete_operation_reissues_delete() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Err(stale_lro_error()));
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForDeleteOperation;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale".to_string());
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

    assert_eq!(
        executor
            .internal_state::<AzureWorkerController>()
            .unwrap()
            .state,
        AzureWorkerState::DeletingApp
    );
}

#[tokio::test]
async fn stale_certificate_delete_operation_reissues_delete_before_clearing_identity() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Err(stale_lro_error()));
    let mut container_apps = MockContainerAppsApi::new();
    container_apps
        .expect_delete_managed_environment_certificate()
        .times(1)
        .returning(|_, _, _| Ok(OperationResult::Completed(())));
    let provider = setup_mock_service_provider(Arc::new(container_apps), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForCertificateDeleteOperation;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/stale".to_string());
    controller.container_apps_certificate_id = Some("tracked-certificate-id".to_string());
    controller.uses_custom_domain = true;
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
    {
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();
        assert_eq!(controller.state, AzureWorkerState::DeletingCertificate);
        assert_eq!(
            controller.container_apps_certificate_id.as_deref(),
            Some("tracked-certificate-id")
        );
    }

    executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(controller.state, AzureWorkerState::Deleted);
    assert!(controller.container_apps_certificate_id.is_none());
}
