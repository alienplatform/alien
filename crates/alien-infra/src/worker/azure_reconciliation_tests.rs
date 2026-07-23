#[test]
fn azure_storage_trigger_maps_only_supported_event_types() {
    assert_eq!(
        azure_storage_event_types(
            &[
                "created".to_string(),
                "deleted".to_string(),
                "tierChanged".to_string(),
            ],
            "worker",
        )
        .unwrap(),
        vec![
            "Microsoft.Storage.BlobCreated",
            "Microsoft.Storage.BlobDeleted",
            "Microsoft.Storage.BlobTierChanged",
        ]
    );
    assert!(azure_storage_event_types(&["metadataUpdated".to_string()], "worker").is_err());
}

#[test]
fn legacy_controller_state_defaults_dapr_naming_version_to_zero() {
    let mut serialized = serde_json::to_value(AzureWorkerController::mock_ready("worker"))
        .expect("controller state should serialize");
    serialized
        .as_object_mut()
        .expect("controller state should be an object")
        .remove("daprComponentNamingVersion");

    let controller: AzureWorkerController =
        serde_json::from_value(serialized).expect("legacy controller state should deserialize");

    assert_eq!(controller.dapr_component_naming_version, 0);
}

#[tokio::test]
async fn ready_legacy_controller_enters_dapr_name_migration() {
    let mut controller = AzureWorkerController::mock_ready("worker");
    controller.dapr_component_naming_version = 0;
    let mut executor = SingleControllerExecutor::builder()
        .resource(basic_function())
        .controller(controller)
        .platform(Platform::Azure)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.step().await.unwrap();

    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert_eq!(controller.state, AzureWorkerState::Ready);
    assert!(controller.auxiliary_teardown_candidates_initialized);

    executor.step().await.unwrap();

    assert_eq!(
        executor
            .internal_state::<AzureWorkerController>()
            .unwrap()
            .state,
        AzureWorkerState::MigratingDaprComponentNames
    );
}

#[tokio::test]
async fn delete_polls_pending_migration_operation_before_component_cleanup() {
    let operation_polled = Arc::new(AtomicBool::new(false));
    let operation_polled_by_lro = operation_polled.clone();
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(move |_, _, _| {
            operation_polled_by_lro.store(true, Ordering::SeqCst);
            Ok(Some("completed".to_string()))
        });

    let operation_polled_by_cleanup = operation_polled.clone();
    let mut container_apps = MockContainerAppsApi::new();
    container_apps
        .expect_get_dapr_component()
        .times(1)
        .returning(move |_, _, component_name| {
            assert!(
                operation_polled_by_cleanup.load(Ordering::SeqCst),
                "pending migration must finish before component cleanup starts"
            );
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Dapr component".to_string(),
                    resource_name: component_name.to_string(),
                },
            ))
        });

    let provider = setup_mock_service_provider(Arc::new(container_apps), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForDaprComponentNameMigrationOperation;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/migrate-dapr".to_string());
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

    executor.delete().unwrap();
    executor.step().await.unwrap();
    assert_eq!(
        executor
            .internal_state::<AzureWorkerController>()
            .unwrap()
            .state,
        AzureWorkerState::WaitingForPendingOperationBeforeDelete
    );

    executor.step().await.unwrap();
    assert!(operation_polled.load(Ordering::SeqCst));
    assert_eq!(
        executor
            .internal_state::<AzureWorkerController>()
            .unwrap()
            .state,
        AzureWorkerState::DeleteStart
    );

    executor.step().await.unwrap();
    executor.step().await.unwrap();
    executor.step().await.unwrap();
    executor.step().await.unwrap();
}

#[tokio::test]
async fn delete_drains_pending_operation_before_no_app_fast_path() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Ok(Some("completed".to_string())));
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.container_app_name = None;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/create-app".to_string());
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
    assert_eq!(
        executor
            .internal_state::<AzureWorkerController>()
            .unwrap()
            .state,
        AzureWorkerState::WaitingForPendingOperationBeforeDelete
    );
    executor.step().await.unwrap();
    assert_eq!(
        executor
            .internal_state::<AzureWorkerController>()
            .unwrap()
            .state,
        AzureWorkerState::DeleteStart
    );
    executor.step().await.unwrap();
    assert_eq!(
        executor
            .internal_state::<AzureWorkerController>()
            .unwrap()
            .state,
        AzureWorkerState::Deleted
    );
}

#[tokio::test]
async fn imported_storage_teardown_orders_event_role_queue_before_dapr() {
    let order = Arc::new(AtomicUsize::new(0));
    let dapr_checked = Arc::new(AtomicBool::new(false));
    let storage = test_storage_1();
    let mut worker = basic_function();
    worker.commands_enabled = false;
    worker.triggers.push(WorkerTrigger::storage(
        &storage,
        vec!["created".to_string()],
    ));
    let execution_principal_id = "87654321-4321-4321-4321-210987654321";
    let receiver_assignment_name =
        crate::worker::azure_names::storage_trigger_receiver_role_assignment_name(
            "test",
            &worker.id,
            &storage.id,
            execution_principal_id,
        );
    let receiver_assignment_id = format!("/roleAssignments/{receiver_assignment_name}");

    let order_for_dapr = order.clone();
    let dapr_checked_by_get = dapr_checked.clone();
    let mut container_apps = MockContainerAppsApi::new();
    container_apps
        .expect_get_dapr_component()
        .times(1..)
        .returning(move |_, _, component_name| {
            assert_eq!(
                order_for_dapr.load(Ordering::SeqCst),
                3,
                "storage delivery infrastructure must be removed before Dapr"
            );
            dapr_checked_by_get.store(true, Ordering::SeqCst);
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Dapr component".to_string(),
                    resource_name: component_name.to_string(),
                },
            ))
        });

    let order_for_event = order.clone();
    let mut event_grid = MockEventGridApi::new();
    event_grid
        .expect_delete_event_subscription()
        .times(1)
        .returning(move |_, _| {
            assert_eq!(order_for_event.fetch_add(1, Ordering::SeqCst), 0);
            Ok(())
        });

    let order_for_role = order.clone();
    let mut authorization = MockAuthorizationApi::new();
    authorization
        .expect_build_role_assignment_id()
        .times(2)
        .returning(|_, name| format!("/roleAssignments/{name}"));
    let receiver_assignment_id_for_list = receiver_assignment_id.clone();
    let receiver_assignment_name_for_list = receiver_assignment_name.clone();
    let order_for_list = order.clone();
    authorization
        .expect_list_role_assignments()
        .times(2)
        .returning(move |_, role_definition_id| {
            let current_order = order_for_list.load(Ordering::SeqCst);
            if current_order == 2 {
                return Ok(Vec::new());
            }
            assert_eq!(
                current_order, 1,
                "receiver role discovery must follow Event Grid deletion"
            );
            Ok(vec![RoleAssignment {
                id: Some(receiver_assignment_id_for_list.clone()),
                name: Some(receiver_assignment_name_for_list.clone()),
                properties: Some(RoleAssignmentProperties {
                    principal_id: execution_principal_id.to_string(),
                    role_definition_id: role_definition_id
                        .expect("storage receiver role definition filter"),
                    scope: None,
                    principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                    condition: None,
                    condition_version: None,
                    delegated_managed_identity_resource_id: None,
                    description: None,
                    created_by: None,
                    created_on: None,
                    updated_by: None,
                    updated_on: None,
                }),
                type_: None,
            }])
        });
    authorization
        .expect_delete_role_assignment_by_id()
        .times(1)
        .returning(move |assignment_id| {
            assert_eq!(assignment_id, receiver_assignment_id);
            assert_eq!(order_for_role.fetch_add(1, Ordering::SeqCst), 1);
            Ok(None)
        });
    let authorization = Arc::new(authorization);

    let order_for_queue = order.clone();
    let mut service_bus = MockServiceBusManagementApi::new();
    service_bus
        .expect_delete_queue()
        .times(1)
        .returning(move |_, _, _| {
            assert_eq!(order_for_queue.fetch_add(1, Ordering::SeqCst), 2);
            Ok(())
        });

    let mut provider = MockPlatformServiceProvider::new();
    let container_apps = Arc::new(container_apps);
    provider
        .expect_get_azure_container_apps_client()
        .returning(move |_| Ok(container_apps.clone()));
    let event_grid = Arc::new(event_grid);
    provider
        .expect_get_azure_event_grid_client()
        .returning(move |_| Ok(event_grid.clone()));
    provider
        .expect_get_azure_authorization_client()
        .returning(move |_| Ok(authorization.clone()));
    let service_bus = Arc::new(service_bus);
    provider
        .expect_get_azure_service_bus_management_client()
        .returning(move |_| Ok(service_bus.clone()));

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::mock_ready("worker-app"))
        .platform(Platform::Azure)
        .service_provider(Arc::new(provider))
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.delete().unwrap();
    for _ in 0..12 {
        executor.step().await.unwrap();
        if dapr_checked.load(Ordering::SeqCst) {
            break;
        }
    }

    assert_eq!(order.load(Ordering::SeqCst), 3);
    assert!(
        dapr_checked.load(Ordering::SeqCst),
        "Dapr cleanup must run after storage delivery teardown"
    );
}

#[tokio::test]
async fn imported_custom_domain_deletes_deterministic_certificate_without_tracked_id() {
    let worker = function_public_ingress();
    let mut custom_domains = std::collections::HashMap::new();
    custom_domains.insert(
        worker.id.clone(),
        alien_core::CustomDomainConfig {
            domain: "worker.example.com".to_string(),
            certificate: alien_core::CustomCertificateConfig {
                azure: Some(alien_core::AzureCustomCertificateConfig {
                    key_vault_certificate_id: "https://vault.example/certificates/worker"
                        .to_string(),
                    key_vault_resource_id: None,
                }),
                ..Default::default()
            },
        },
    );
    let stack_settings = alien_core::StackSettings {
        domains: Some(alien_core::DomainSettings {
            custom_domains: Some(custom_domains),
            public_endpoint_target: None,
        }),
        ..Default::default()
    };
    let mut container_apps = MockContainerAppsApi::new();
    container_apps
        .expect_delete_managed_environment_certificate()
        .times(1)
        .returning(|_, _, _| Ok(OperationResult::Completed(())));
    let provider = setup_mock_service_provider(Arc::new(container_apps), None);
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::DeletingCertificate;
    controller.container_apps_certificate_id = None;
    controller.uses_custom_domain = false;
    controller.fqdn = Some("worker.example.com".to_string());
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Azure)
        .stack_settings(stack_settings)
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
        AzureWorkerState::Deleted
    );
}

#[tokio::test]
async fn completed_update_operation_clears_persisted_lro_cursor() {
    let mut lro = MockLongRunningOperationApi::new();
    lro.expect_check_status()
        .times(1)
        .returning(|_, _, _| Ok(Some("completed".to_string())));
    let provider =
        setup_mock_service_provider(Arc::new(MockContainerAppsApi::new()), Some(Arc::new(lro)));
    let mut controller = AzureWorkerController::mock_ready("worker-app");
    controller.state = AzureWorkerState::WaitingForUpdateOperation;
    controller.pending_operation_url =
        Some("https://management.azure.com/operations/update-app".to_string());
    controller.pending_operation_retry_after = Some(15);
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
    assert_eq!(controller.state, AzureWorkerState::UpdatingContainerApp);
    assert!(controller.pending_operation_url.is_none());
    assert!(controller.pending_operation_retry_after.is_none());
}

#[test]
fn strips_scheme_and_path_from_dns_endpoint_url() {
    assert_eq!(
        dns_name_from_url("https://app.example.azurecontainerapps.io/health"),
        "app.example.azurecontainerapps.io"
    );
    assert_eq!(
        dns_name_from_url("app.example.azurecontainerapps.io."),
        "app.example.azurecontainerapps.io"
    );
}

#[test]
fn platform_domain_outputs_target_container_app_host_not_public_fqdn() {
    let mut controller = AzureWorkerController::mock_ready("test-worker");
    controller.fqdn = Some("test-worker.public.example.com".to_string());
    controller.certificate_id = Some("cert_123".to_string());
    controller.url = Some("https://test-worker.azurecontainerapps.io".to_string());

    let outputs = controller.build_outputs().unwrap();
    let worker_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    let endpoint = worker_outputs
        .public_endpoints
        .get("default")
        .expect("default public endpoint");

    assert_eq!(
        endpoint.url.as_str(),
        "https://test-worker.azurecontainerapps.io"
    );
    assert_eq!(
        endpoint
            .load_balancer_endpoint
            .as_ref()
            .map(|endpoint| endpoint.dns_name.as_str()),
        Some("test-worker.azurecontainerapps.io")
    );
}

#[test]
fn dns_target_is_ingress_host_when_url_is_overridden_to_public_fqdn() {
    // Regression: when `url` is overridden to the public display FQDN (from `public_urls`), the
    // CNAME target must still be the Container App ingress host. Otherwise the record name (the
    // public FQDN) and the target collide into a self-referential CNAME, which the DNS provider
    // rejects — the bug that deadlocked the Azure worker in `waitingForDns`.
    let mut controller = AzureWorkerController::mock_ready("test-worker");
    controller.url = Some("https://test-worker.abc123.dev.vpc.direct".to_string());
    controller.container_app_url =
        Some("https://test-worker.kindsky.eastus2.azurecontainerapps.io".to_string());

    let outputs = controller.build_outputs().unwrap();
    let worker_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    let endpoint = worker_outputs
        .public_endpoints
        .get("default")
        .expect("default public endpoint");

    // Display URL stays the public FQDN.
    assert_eq!(
        endpoint.url.as_str(),
        "https://test-worker.abc123.dev.vpc.direct"
    );
    // The CNAME target is the ingress host — and crucially NOT the record's own public FQDN.
    let dns_name = endpoint
        .load_balancer_endpoint
        .as_ref()
        .map(|endpoint| endpoint.dns_name.as_str());
    assert_eq!(
        dns_name,
        Some("test-worker.kindsky.eastus2.azurecontainerapps.io")
    );
    assert_ne!(dns_name, Some("test-worker.abc123.dev.vpc.direct"));
}

#[tokio::test]
async fn imported_worker_heartbeat_rebuilds_ingress_host_for_dns() {
    // Regression for the create-path-only gap: an imported worker starts Ready with
    // `container_app_url = None` and `url` = the public display FQDN (the importer skips the
    // create flow). The heartbeat must rebuild `container_app_url` from the live Container App,
    // so the DNS CNAME targets the ingress host rather than the self-referential public FQDN.
    let app_name = "test-imported-worker";
    let mut mock = MockContainerAppsApi::new();
    mock.expect_get_container_app()
        .returning(move |_, _| Ok(create_successful_container_app_response(app_name, true)))
        .times(0..);
    let mock_provider = setup_mock_service_provider(Arc::new(mock), None);

    // Imported shape: ingress host unset, url is the public display FQDN.
    let mut controller = AzureWorkerController::mock_ready(app_name);
    controller.container_app_url = None;
    controller.url = Some("https://test-imported-worker.abc123.dev.vpc.direct".to_string());
    controller.commands_sender_role_assignment_discovery_complete = true;

    let mut worker = basic_function();
    worker.commands_enabled = false;
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    // The heartbeat rebuilt the ingress host…
    assert_eq!(
        controller.container_app_url.as_deref(),
        Some("https://test-imported-worker.azurecontainerapps.io")
    );
    // …so build_outputs targets it, NOT the public display FQDN.
    let outputs = controller.build_outputs().unwrap();
    let worker_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    let endpoint = worker_outputs
        .public_endpoints
        .get("default")
        .expect("default public endpoint");
    let dns_name = endpoint
        .load_balancer_endpoint
        .as_ref()
        .map(|endpoint| endpoint.dns_name.as_str());
    assert_eq!(dns_name, Some("test-imported-worker.azurecontainerapps.io"));
    assert_ne!(dns_name, Some("test-imported-worker.abc123.dev.vpc.direct"));
}

#[test]
fn detects_azure_authorization_propagation_error_from_http_context() {
    let http_error = AlienError::new(CloudClientErrorData::HttpResponseError {
        message: "Azure CreateOrUpdateDaprComponent failed: HTTP 403 Forbidden".to_string(),
        url: "https://management.azure.com/test".to_string(),
        http_status: 403,
        http_request_text: None,
        http_response_text: Some(
            "{\"error\":{\"code\":\"AuthorizationFailed\",\"message\":\"The client does not have authorization to perform action. If access was recently granted, please refresh your credentials.\"}}"
                .to_string(),
        ),
    });

    let error = http_error.context(ErrorData::CloudPlatformError {
        message: "Failed to create commands Dapr component".to_string(),
        resource_id: Some("alien-rs-fn".to_string()),
    });

    assert!(is_azure_authorization_propagation_error(&error));
}

#[test]
fn ignores_non_authorization_cloud_platform_errors() {
    let error = AlienError::new(ErrorData::CloudPlatformError {
        message: "Failed to create commands Dapr component".to_string(),
        resource_id: Some("alien-rs-fn".to_string()),
    });

    assert!(!is_azure_authorization_propagation_error(&error));
}
