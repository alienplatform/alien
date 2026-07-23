fn equal_update_drift_provider(
    app_name: &str,
    desired: StorageTarget,
    actions: Arc<Mutex<Vec<String>>>,
) -> Arc<MockPlatformServiceProvider> {
    let mut container_apps = MockContainerAppsApi::new();
    let update_app_name = app_name.to_string();
    let update_actions = actions.clone();
    container_apps
        .expect_update_container_app()
        .times(1)
        .returning(move |_, app_name, _| {
            assert_eq!(app_name, update_app_name);
            record(&update_actions, "update-app");
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&update_app_name, false),
            ))
        });
    let get_app_name = app_name.to_string();
    let get_actions = actions.clone();
    container_apps
        .expect_get_container_app()
        .times(1)
        .returning(move |_, app_name| {
            assert_eq!(app_name, get_app_name);
            record(&get_actions, "get-app");
            Ok(create_successful_container_app_response(
                &get_app_name,
                false,
            ))
        });
    let container_apps = Arc::new(container_apps);

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
    let mut authorization = MockAuthorizationApi::new();
    authorization
        .expect_build_role_assignment_id()
        .returning(|scope, name| {
            format!(
                "/{}/providers/Microsoft.Authorization/roleAssignments/{name}",
                scope.to_scope_string(&alien_azure_clients::AzureClientConfig::mock())
            )
        });
    let assignment = receiver_role_assignment(&desired, &role_definition_id);
    let desired_scope = desired
        .receiver_assignment_id
        .split("/providers/Microsoft.Authorization")
        .next()
        .expect("desired assignment scope")
        .to_string();
    authorization
        .expect_list_role_assignments()
        .times(1)
        .returning(move |scope, requested_role_definition| {
            assert_eq!(
                requested_role_definition.as_deref(),
                Some(role_definition_id.as_str())
            );
            let scope = format!(
                "/{}",
                scope
                    .to_scope_string(&alien_azure_clients::AzureClientConfig::mock())
                    .trim_start_matches('/')
            );
            assert_eq!(scope, desired_scope);
            Ok(vec![assignment.clone()])
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
                request.properties.destination.properties.resource_id,
                format!(
                    "/subscriptions/{SUBSCRIPTION_ID}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}/queues/{}",
                    desired_event.resource_group,
                    desired_event.namespace,
                    desired_event.queue
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
    Arc::new(provider)
}

#[tokio::test]
async fn equal_update_revalidates_tracked_storage_delivery_once() {
    let app_name = "test-storage-target-worker";
    let storage = test_storage_1();
    let worker = storage_trigger_worker(&storage);
    let desired = storage_target(
        &worker.id,
        &storage.id,
        "storage-account",
        "storage-container",
        "service-bus-rg",
        "service-bus-namespace",
        "execution-principal",
    );
    let actions = Arc::new(Mutex::new(Vec::new()));
    let provider = equal_update_drift_provider(app_name, desired.clone(), actions.clone());
    let mut controller = AzureWorkerController::mock_ready(app_name);
    controller.storage_trigger_infrastructure = vec![AzureStorageTriggerInfrastructure {
        storage_id: Some(desired.storage_id.clone()),
        source_resource_id: desired.source_resource_id.clone(),
        source_container_name: Some(desired.source_container_name.clone()),
        event_subscription_name: desired.event_subscription_name.clone(),
        service_bus_resource_group: desired.resource_group.clone(),
        namespace_name: desired.namespace.clone(),
        queue_name: desired.queue.clone(),
        queue_applied: true,
        receiver_role_assignment_id: Some(desired.receiver_assignment_id.clone()),
        delivery_reconciled: true,
    }];
    let service_account = ServiceAccount::new("default-profile-sa".to_string()).build();
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker.clone())
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
            rotated_service_account(
                "execution-identity",
                "execution-client",
                "execution-principal",
            ),
        )
        .build()
        .await
        .expect("executor should build");
    executor.update(worker).expect("equal update should start");

    executor.step().await.expect("enter UpdateStart");
    executor
        .step()
        .await
        .expect("checkpoint delivery revalidation");
    assert!(
        actions.lock().expect("action log lock").is_empty(),
        "delivery latches must be checkpointed before any remote update"
    );
    let checkpoint = executor
        .internal_state::<AzureWorkerController>()
        .expect("Azure worker controller");
    assert!(checkpoint.storage_delivery_update_reconciliation_initialized);
    assert!(!checkpoint.storage_trigger_infrastructure[0].queue_applied);
    assert!(!checkpoint.storage_trigger_infrastructure[0].delivery_reconciled);

    for step in 0..16 {
        executor
            .step()
            .await
            .unwrap_or_else(|error| panic!("equal-update drift repair failed at {step}: {error}"));
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
            .delivery_reconciled
    );
    assert_eq!(
        actions.lock().expect("action log lock").as_slice(),
        [
            "update-app".to_string(),
            "get-app".to_string(),
            format!("create-queue:{}", desired.queue),
            format!("put-event:{}", desired.event_subscription_name),
        ]
    );
}
