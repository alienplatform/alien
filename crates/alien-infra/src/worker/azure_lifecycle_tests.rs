#[tokio::test]
async fn test_pre_container_app_rbac_wait_holds_state_when_woken_early() {
    let deadline = current_unix_timestamp_secs().saturating_add(60);
    let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
    controller.state = AzureWorkerState::WaitingBeforeContainerAppCreation;
    controller.pre_container_app_rbac_wait_until_epoch_secs = Some(deadline);

    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Provisioning);
    assert_eq!(
        controller.state,
        AzureWorkerState::WaitingBeforeContainerAppCreation
    );
    assert_eq!(
        step_result.suggested_delay,
        Some(Duration::from_secs(AZURE_RBAC_WAIT_POLL_SECS))
    );
    assert_eq!(
        controller.pre_container_app_rbac_wait_until_epoch_secs,
        Some(deadline)
    );
}

#[tokio::test]
async fn test_ready_rbac_wait_holds_state_when_woken_early() {
    let deadline = current_unix_timestamp_secs().saturating_add(60);
    let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
    controller.state = AzureWorkerState::WaitingForRbacPropagation;
    controller.ready_rbac_wait_until_epoch_secs = Some(deadline);

    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Provisioning);
    assert_eq!(
        controller.state,
        AzureWorkerState::WaitingForRbacPropagation
    );
    assert_eq!(
        step_result.suggested_delay,
        Some(Duration::from_secs(AZURE_RBAC_WAIT_POLL_SECS))
    );
    assert_eq!(controller.ready_rbac_wait_until_epoch_secs, Some(deadline));
}

#[tokio::test]
async fn test_ready_rbac_wait_advances_after_deadline() {
    let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
    controller.state = AzureWorkerState::WaitingForRbacPropagation;
    controller.ready_rbac_wait_until_epoch_secs =
        Some(current_unix_timestamp_secs().saturating_sub(1));

    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Provisioning);
    assert_eq!(controller.state, AzureWorkerState::RunningReadinessProbe);
    assert_eq!(step_result.suggested_delay, None);
    assert_eq!(controller.ready_rbac_wait_until_epoch_secs, None);

    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Running);
    assert_eq!(controller.state, AzureWorkerState::Ready);
    assert_eq!(step_result.suggested_delay, None);
}

#[tokio::test]
async fn test_update_rbac_wait_holds_and_clears() {
    let deadline = current_unix_timestamp_secs().saturating_add(60);
    let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
    controller.state = AzureWorkerState::UpdateWaitingForRbacPropagation;
    controller.ready_rbac_wait_until_epoch_secs = Some(deadline);
    controller.update_rbac_wait_required = true;

    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Updating);
    assert_eq!(
        controller.state,
        AzureWorkerState::UpdateWaitingForRbacPropagation
    );
    assert_eq!(
        step_result.suggested_delay,
        Some(Duration::from_secs(AZURE_RBAC_WAIT_POLL_SECS))
    );
    assert_eq!(controller.ready_rbac_wait_until_epoch_secs, Some(deadline));
    assert!(controller.update_rbac_wait_required);

    let mut controller = controller.clone();
    controller.ready_rbac_wait_until_epoch_secs =
        Some(current_unix_timestamp_secs().saturating_sub(1));
    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Updating);
    assert_eq!(
        controller.state,
        AzureWorkerState::UpdateRunningReadinessProbe
    );
    assert_eq!(step_result.suggested_delay, None);
    assert_eq!(controller.ready_rbac_wait_until_epoch_secs, None);
    assert!(!controller.update_rbac_wait_required);

    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Running);
    assert_eq!(controller.state, AzureWorkerState::Ready);
    assert_eq!(step_result.suggested_delay, None);
}

// ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

#[rstest]
#[case::basic(basic_function())]
#[case::env_vars(function_with_env_vars())]
#[case::storage_link(function_with_storage_link())]
#[case::env_and_storage(function_with_env_and_storage())]
#[case::multiple_storages(function_with_multiple_storages())]
#[case::public_ingress(function_public_ingress())]
#[case::private_ingress(function_private_ingress())]
#[case::concurrency(function_with_concurrency())]
#[case::custom_config(function_custom_config())]
#[case::readiness_probe(function_with_readiness_probe())]
#[case::complete_test(function_complete_test())]
#[tokio::test]
async fn test_create_and_delete_flow_succeeds(#[case] worker: Worker) {
    let app_name = format!("test-{}", worker.id);
    let (mock_provider, _mock_server) = setup_mocks_for_function(&worker, &app_name, true);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Run create flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify outputs are available
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    assert!(function_outputs.identifier.is_some());
    assert!(function_outputs.worker_name.starts_with("test-"));

    // Delete the worker
    executor.delete().unwrap();

    // Run delete flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);

    // Verify outputs are no longer available
    assert!(executor.outputs().is_none());
}

// ─────────────── UPDATE FLOW TESTS ────────────────────────────────

#[rstest]
#[case::basic_to_env(basic_function(), function_with_env_vars())]
#[case::env_to_storage(function_with_env_vars(), function_with_storage_link())]
#[case::storage_to_custom(function_with_storage_link(), function_custom_config())]
#[case::custom_to_public(function_custom_config(), function_public_ingress())]
#[case::public_to_complete(function_public_ingress(), function_complete_test())]
#[case::complete_to_basic(function_complete_test(), basic_function())]
#[tokio::test]
async fn test_update_flow_succeeds(#[case] from_function: Worker, #[case] to_function: Worker) {
    // Ensure both workers have the same ID for valid updates
    let worker_id = "test-update-worker".to_string();
    let mut from_function = from_function;
    from_function.id = worker_id.clone();

    let mut to_function = to_function;
    to_function.id = worker_id.clone();

    let app_name = format!("test-{}", worker_id);
    let (mock_provider, mock_server) = setup_mocks_for_function(&to_function, &app_name, false);

    // Start with the "from" worker in Ready state
    let mut ready_controller = AzureWorkerController::mock_ready(&app_name);

    // If the target worker has a readiness probe, update the controller URL to point to mock server
    if to_function.readiness_probe.is_some() && !to_function.public_endpoints.is_empty() {
        if let Some(ref server) = mock_server {
            ready_controller.url = Some(server.base_url());
        }
    } else if !to_function.public_endpoints.is_empty() {
        // Ensure the controller has a URL for public workers
        ready_controller.url = Some(format!("https://{}.azurecontainerapps.io", app_name));
    }

    let mut executor = SingleControllerExecutor::builder()
        .resource(from_function)
        .controller(ready_controller)
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Ensure we start in Running state
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Update to the new worker
    executor.update(to_function).unwrap();

    // Run the update flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}

#[tokio::test]
async fn update_enables_commands_and_reconciles_partial_tracking() {
    let mut from_worker = basic_function();
    from_worker.id = "commands-toggle-worker".to_string();
    let mut to_worker = from_worker.clone();
    to_worker.commands_enabled = true;
    let app_name = "test-commands-toggle-worker";
    let component_name = get_azure_internal_commands_dapr_component_name(app_name);
    let desired_component = service_bus_dapr_component(
        component_name.clone(),
        app_name,
        "default-service-bus-namespace",
        commands_queue_name(app_name),
        "12345678-1234-1234-1234-123456789012",
    );
    let component_created = Arc::new(AtomicBool::new(false));

    let mut container_apps = MockContainerAppsApi::new();
    let app_name_for_update = app_name.to_string();
    container_apps
        .expect_update_container_app()
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_update, false),
            ))
        });
    let app_name_for_get = app_name.to_string();
    container_apps
        .expect_get_container_app()
        .returning(move |_, _| {
            Ok(create_successful_container_app_response(
                &app_name_for_get,
                false,
            ))
        })
        .times(0..);
    let component_name_for_get = component_name.clone();
    let desired_for_get = desired_component.clone();
    let created_for_get = component_created.clone();
    container_apps
        .expect_get_dapr_component()
        .returning(move |_, _, name| {
            if name == component_name_for_get && created_for_get.load(Ordering::SeqCst) {
                Ok(desired_for_get.clone())
            } else {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Dapr component".to_string(),
                        resource_name: name.to_string(),
                    },
                ))
            }
        })
        .times(1..);
    let created_by_put = component_created.clone();
    container_apps
        .expect_create_or_update_dapr_component()
        .times(1)
        .returning(move |_, _, _, component| {
            created_by_put.store(true, Ordering::SeqCst);
            Ok(OperationResult::Completed(component.clone()))
        });

    let mut service_bus = MockServiceBusManagementApi::new();
    service_bus
        .expect_create_or_update_queue()
        .times(1..)
        .returning(|_, _, _, _| Ok(alien_azure_clients::models::queue::SbQueue::default()));
    let provider =
        setup_commands_toggle_provider(Arc::new(container_apps), Arc::new(service_bus), None);
    let mut controller = AzureWorkerController::mock_ready(app_name);
    controller.commands_namespace_name = Some("default-service-bus-namespace".to_string());
    let mut executor = SingleControllerExecutor::builder()
        .resource(from_worker)
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.update(to_worker).unwrap();
    for step in 0..30 {
        if executor.status() == ResourceStatus::Running {
            break;
        }
        executor.step().await.unwrap_or_else(|error| {
            let state = executor
                .internal_state::<AzureWorkerController>()
                .map(|controller| format!("{:?}", controller.state))
                .unwrap_or_else(|| "unavailable".to_string());
            panic!("commands-enable update failed at step {step}, state {state}: {error}");
        });
    }

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
    assert_eq!(
        controller.commands_namespace_name.as_deref(),
        Some("default-service-bus-namespace")
    );
    assert_eq!(
        controller.commands_queue_name.as_deref(),
        Some("test-commands-toggle-worker-rq")
    );
    assert_eq!(
        controller.commands_dapr_component.as_deref(),
        Some(component_name.as_str())
    );
}

#[tokio::test]
async fn update_disables_imported_commands_without_touching_storage() {
    let mut from_worker = basic_function();
    from_worker.id = "commands-toggle-worker".to_string();
    from_worker.commands_enabled = true;
    let enabled_worker = from_worker.clone();
    let mut to_worker = from_worker.clone();
    to_worker.commands_enabled = false;
    let app_name = "test-commands-toggle-worker";
    let component_name = get_azure_internal_commands_dapr_component_name(app_name);
    let existing_component = service_bus_dapr_component(
        component_name.clone(),
        app_name,
        "default-service-bus-namespace",
        commands_queue_name(app_name),
        "12345678-1234-1234-1234-123456789012",
    );

    let mut container_apps = MockContainerAppsApi::new();
    let app_name_for_update = app_name.to_string();
    container_apps
        .expect_update_container_app()
        .times(2)
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_update, false),
            ))
        });
    let app_name_for_get = app_name.to_string();
    container_apps
        .expect_get_container_app()
        .returning(move |_, _| {
            Ok(create_successful_container_app_response(
                &app_name_for_get,
                false,
            ))
        })
        .times(0..);
    let component_name_for_get = component_name.clone();
    let existing_component_for_get = existing_component.clone();
    container_apps
        .expect_get_dapr_component()
        .times(2..)
        .returning(move |_, _, name| {
            if name == component_name_for_get {
                Ok(existing_component_for_get.clone())
            } else {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Dapr component".to_string(),
                        resource_name: name.to_string(),
                    },
                ))
            }
        });
    container_apps
        .expect_delete_dapr_component()
        .times(1)
        .returning(|_, _, _| Ok(OperationResult::Completed(())));
    container_apps
        .expect_create_or_update_dapr_component()
        .times(0);

    let mut service_bus = MockServiceBusManagementApi::new();
    service_bus
        .expect_delete_queue()
        .times(1)
        .returning(|_, _, _| Ok(()));
    service_bus
        .expect_create_or_update_queue()
        .times(1..)
        .returning(|_, _, _, _| Ok(alien_azure_clients::models::queue::SbQueue::default()));
    let role_assignment_created = Arc::new(AtomicBool::new(false));
    let provider = setup_commands_toggle_provider(
        Arc::new(container_apps),
        Arc::new(service_bus),
        Some(role_assignment_created.clone()),
    );
    let controller = AzureWorkerController::mock_ready(app_name);
    let mut executor = SingleControllerExecutor::builder()
        .resource(from_worker)
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.update(to_worker).unwrap();
    for step in 0..30 {
        if executor.status() == ResourceStatus::Running {
            break;
        }
        executor.step().await.unwrap_or_else(|error| {
            let state = executor
                .internal_state::<AzureWorkerController>()
                .map(|controller| format!("{:?}", controller.state))
                .unwrap_or_else(|| "unavailable".to_string());
            panic!("commands-disable update failed at step {step}, state {state}: {error}");
        });
    }

    assert_eq!(executor.status(), ResourceStatus::Running);
    {
        let controller = executor.internal_state::<AzureWorkerController>().unwrap();
        assert!(controller.commands_namespace_name.is_none());
        assert!(controller.commands_queue_name.is_none());
        assert!(controller.commands_dapr_component.is_none());
        assert!(controller.storage_trigger_infrastructure.is_empty());
    }

    role_assignment_created.store(false, Ordering::SeqCst);
    executor.update(enabled_worker).unwrap();
    for step in 0..30 {
        if executor.status() == ResourceStatus::Running {
            break;
        }
        executor.step().await.unwrap_or_else(|error| {
            let state = executor
                .internal_state::<AzureWorkerController>()
                .map(|controller| format!("{:?}", controller.state))
                .unwrap_or_else(|| "unavailable".to_string());
            panic!("commands-reenable update failed at step {step}, state {state}: {error}");
        });
    }

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
    assert!(role_assignment_created.load(Ordering::SeqCst));
    assert!(controller.commands_sender_role_assignment_id.is_some());
    assert_eq!(
        controller.commands_dapr_component.as_deref(),
        Some(component_name.as_str())
    );
}

// ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

#[rstest]
#[case::basic(basic_function(), false)]
#[case::public_with_missing_app(function_public_ingress(), true)]
#[case::private_with_missing_app(function_private_ingress(), true)]
#[tokio::test]
async fn test_best_effort_deletion_when_resources_missing(
    #[case] worker: Worker,
    #[case] app_missing: bool,
) {
    let app_name = format!("test-{}", worker.id);
    let mock_container_apps = setup_mock_client_for_best_effort_deletion(&app_name, app_missing);
    let mock_provider = setup_mock_service_provider(mock_container_apps, None);

    // Start with a ready controller
    let mut ready_controller = AzureWorkerController::mock_ready(&app_name);
    if !worker.public_endpoints.is_empty() {
        ready_controller.url = Some(format!("https://{}.azurecontainerapps.io", app_name));
    }

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(ready_controller)
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Ensure we start in Running state
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Delete the worker
    executor.delete().unwrap();

    // Run the delete flow - it should succeed even when resources are missing
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);

    // Verify outputs are no longer available
    assert!(executor.outputs().is_none());
}

// ─────────────── LONG RUNNING OPERATION TESTS ──────────────────────

#[tokio::test]
async fn test_long_running_creation_operation() {
    let worker = basic_function();
    let app_name = format!("test-{}", worker.id);
    let (mock_container_apps, mock_lro) =
        setup_mock_client_for_long_running_creation(&app_name, false);
    let mock_provider = setup_mock_service_provider(mock_container_apps, Some(mock_lro));

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Run create flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify the controller went through LRO states
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert!(controller.container_app_name.is_some());
    assert!(controller.resource_id.is_some());
}

// ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

/// Test that verifies public workers get URL in outputs
#[tokio::test]
async fn test_public_function_gets_url_in_outputs() {
    let worker = function_public_ingress();
    let app_name = format!("test-{}", worker.id);

    let mut mock_container_apps = MockContainerAppsApi::new();

    // Mock creation with URL
    mock_container_apps
        .expect_create_or_update_container_app()
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name, true),
            ))
        });

    mock_container_apps
        .expect_delete_container_app()
        .returning(|_, _| Ok(OperationResult::Completed(())));

    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        });

    let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify URL is in outputs
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    let endpoint = function_outputs
        .public_endpoints
        .get("default")
        .expect("default public endpoint");
    assert!(endpoint.url.contains("azurecontainerapps.io"));
}

/// Test that verifies private workers don't get URL in outputs
#[tokio::test]
async fn test_private_function_has_no_url_in_outputs() {
    let worker = function_private_ingress();
    let app_name = format!("test-{}", worker.id);

    let mut mock_container_apps = MockContainerAppsApi::new();

    // Mock creation without URL
    mock_container_apps
        .expect_create_or_update_container_app()
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name, false),
            ))
        });

    mock_container_apps
        .expect_delete_container_app()
        .returning(|_, _| Ok(OperationResult::Completed(())));

    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        });

    let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify no URL in outputs
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    assert!(function_outputs.public_endpoints.is_empty());
}

/// Test that verifies correct container app configuration parameters
#[tokio::test]
async fn test_container_app_configuration_validation() {
    let worker = function_custom_config();
    let app_name = format!("test-{}", worker.id);

    let mut mock_container_apps = MockContainerAppsApi::new();

    // Validate container app creation request has correct parameters
    let app_name_for_response = app_name.clone();
    mock_container_apps
        .expect_create_or_update_container_app()
        .withf(|_rg, _name, container_app| {
            // Check that the container has correct resource configuration
            if let Some(properties) = &container_app.properties {
                if let Some(template) = &properties.template {
                    if let Some(container) = template.containers.first() {
                        if let Some(resources) = &container.resources {
                            // function_custom_config has 512MB memory
                            let expected_memory = format!("{}Gi", 512.0 / 1024.0);
                            return resources.memory.as_ref() == Some(&expected_memory)
                                && resources.cpu == Some(0.25);
                        }
                    }
                }
            }
            false
        })
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_response, false),
            ))
        });

    mock_container_apps
        .expect_delete_container_app()
        .returning(|_, _| Ok(OperationResult::Completed(())));

    // Allow get_container_app calls during creation (may be called 0 or more times)
    mock_container_apps
        .expect_get_container_app()
        .returning(move |_, _| Ok(create_successful_container_app_response(&app_name, false)))
        .times(0..);

    // Mock get operation failure for deletion verification
    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        })
        .times(0..);

    let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}

/// Test that verifies environment variables are correctly passed
#[tokio::test]
async fn test_environment_variable_handling() {
    let worker = function_with_env_vars();
    let app_name = format!("test-{}", worker.id);

    let mut mock_container_apps = MockContainerAppsApi::new();

    // Validate container app creation request has environment variables
    let app_name_for_response = app_name.clone();
    mock_container_apps
        .expect_create_or_update_container_app()
        .withf(|_rg, _name, container_app| {
            if let Some(properties) = &container_app.properties {
                if let Some(template) = &properties.template {
                    if let Some(container) = template.containers.first() {
                        // Check that environment variables are present
                        let has_app_env = container.env.iter().any(|env_var| {
                            env_var.name.as_deref() == Some("APP_ENV")
                                && env_var.value.as_deref() == Some("production")
                        });
                        let has_log_level = container.env.iter().any(|env_var| {
                            env_var.name.as_deref() == Some("LOG_LEVEL")
                                && env_var.value.as_deref() == Some("debug")
                        });
                        let has_db_name = container.env.iter().any(|env_var| {
                            env_var.name.as_deref() == Some("DB_NAME")
                                && env_var.value.as_deref() == Some("myapp")
                        });
                        return has_app_env && has_log_level && has_db_name;
                    }
                }
            }
            false
        })
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_response, false),
            ))
        });

    mock_container_apps
        .expect_delete_container_app()
        .returning(|_, _| Ok(OperationResult::Completed(())));

    // Allow get_container_app calls during creation (may be called 0 or more times)
    mock_container_apps
        .expect_get_container_app()
        .returning(move |_, _| Ok(create_successful_container_app_response(&app_name, false)))
        .times(0..);

    // Mock get operation failure for deletion verification
    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        })
        .times(0..);

    let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}

/// Test that verifies deletion works when container_app_name is not set (early creation failure)
#[tokio::test]
async fn test_delete_with_no_container_app_name_succeeds() {
    let worker = basic_function();

    // Create a controller with no container app name set (simulating early creation failure)
    let controller = AzureWorkerController {
        state: AzureWorkerState::CreateFailed,
        container_app_name: None, // This is the key - no container app name set
        resource_id: None,
        url: None,
        container_app_url: None,
        pending_operation_url: None,
        pending_operation_retry_after: None,
        dapr_components: Vec::new(),
        storage_trigger_infrastructure: Vec::new(),
        storage_trigger_teardown_progress: AzureStorageTriggerTeardownProgress::default(),
        fqdn: None,
        certificate_id: None,
        keyvault_cert_id: None,
        container_apps_certificate_id: None,
        uses_custom_domain: false,
        certificate_issued_at: None,
        commands_resource_group_name: None,
        commands_namespace_name: None,
        commands_queue_name: None,
        commands_queue_applied: false,
        commands_dapr_component: None,
        commands_dapr_component_deletion_candidates: Vec::new(),
        commands_sender_role_assignment_id: None,
        commands_sender_role_assignment_intent: None,
        commands_sender_role_assignment_discovery_complete: false,
        commands_receiver_role_assignment_id: None,
        commands_infrastructure_auth_wait_until_epoch_secs: None,
        container_apps_environment_wake_wait_until_epoch_secs: None,
        container_apps_environment_wake_retry_after_epoch_secs: None,
        pre_container_app_rbac_wait_until_epoch_secs: None,
        ready_rbac_wait_until_epoch_secs: None,
        update_rbac_wait_required: false,
        update_dapr_components_deleted: false,
        dapr_component_naming_version: CURRENT_DAPR_COMPONENT_NAMING_VERSION,
        pending_dapr_component_deletion_name: None,
        dapr_component_deletion_candidates_initialized: false,
        auxiliary_teardown_candidates_initialized: false,
        commands_update_teardown_candidates_initialized: false,
        trigger_update_teardown_candidates_initialized: false,
        storage_delivery_update_reconciliation_initialized: false,
        _internal_stay_count: None,
    };

    // Mock provider - no expectations since no API calls should be made
    let mock_provider = Arc::new(MockPlatformServiceProvider::new());

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Start in CreateFailed state
    assert_eq!(executor.status(), ResourceStatus::ProvisionFailed);

    // Delete the worker
    executor.delete().unwrap();

    // Run the delete flow - should succeed without making any API calls
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);

    // Verify outputs are no longer available
    assert!(executor.outputs().is_none());
}
