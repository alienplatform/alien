use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use alien_azure_clients::{
    authorization::MockAuthorizationApi,
    container_apps::MockContainerAppsApi,
    long_running_operation::OperationResult,
    models::authorization_role_assignments::{
        RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
    },
    models::managed_environments_dapr_components::DaprComponent,
    service_bus::MockServiceBusManagementApi,
    AzureClientConfigExt,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Platform, ResourceStatus};
use alien_error::AlienError;

use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
use crate::worker::{
    azure_dapr_components::service_bus_dapr_component,
    azure_names::{
        commands_queue_name, commands_sender_role_assignment_name,
        get_azure_internal_commands_dapr_component_name,
    },
    AzureWorkerController,
};

fn record(actions: &Arc<Mutex<Vec<String>>>, action: impl Into<String>) {
    actions.lock().expect("action log lock").push(action.into());
}

fn action_index(actions: &[String], expected: &str) -> usize {
    actions
        .iter()
        .position(|action| action == expected)
        .unwrap_or_else(|| panic!("missing action {expected:?} in {actions:#?}"))
}

fn remote_not_found(resource_name: &str) -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
        resource_type: "Dapr component".to_string(),
        resource_name: resource_name.to_string(),
    })
}

fn sender_assignment_id(
    resource_group: &str,
    namespace: &str,
    queue: &str,
    assignment_name: &str,
) -> String {
    format!(
        "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/{resource_group}/providers/Microsoft.ServiceBus/namespaces/{namespace}/queues/{queue}/providers/Microsoft.Authorization/roleAssignments/{assignment_name}"
    )
}

fn provider(
    app_name: &str,
    old_component: DaprComponent,
    actions: Arc<Mutex<Vec<String>>>,
) -> Arc<MockPlatformServiceProvider> {
    let components = Arc::new(Mutex::new(HashMap::from([(
        old_component.name.clone().expect("old component name"),
        old_component,
    )])));
    let mut container_apps = MockContainerAppsApi::new();
    let update_actions = actions.clone();
    let update_app_name = app_name.to_string();
    container_apps
        .expect_update_container_app()
        .times(1)
        .returning(move |resource_group, app_name, _| {
            assert_eq!(resource_group, "default-resource-group");
            assert_eq!(app_name, update_app_name);
            record(&update_actions, "update-app");
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&update_app_name, false),
            ))
        });
    let get_app_name = app_name.to_string();
    container_apps
        .expect_get_container_app()
        .times(1)
        .returning(move |resource_group, app_name| {
            assert_eq!(resource_group, "default-resource-group");
            assert_eq!(app_name, get_app_name);
            Ok(create_successful_container_app_response(
                &get_app_name,
                false,
            ))
        });
    let get_components = components.clone();
    container_apps
        .expect_get_dapr_component()
        .times(1..)
        .returning(move |_, _, name| {
            get_components
                .lock()
                .expect("component map lock")
                .get(name)
                .cloned()
                .ok_or_else(|| remote_not_found(name))
        });
    let delete_components = components.clone();
    let delete_actions = actions.clone();
    container_apps
        .expect_delete_dapr_component()
        .times(1)
        .returning(move |_, _, name| {
            assert!(
                delete_components
                    .lock()
                    .expect("component map lock")
                    .remove(name)
                    .is_some(),
                "delete requires an owned component"
            );
            record(&delete_actions, format!("delete-dapr:{name}"));
            Ok(OperationResult::Completed(()))
        });
    let put_components = components;
    let put_actions = actions.clone();
    container_apps
        .expect_create_or_update_dapr_component()
        .times(1)
        .returning(move |_, _, name, component| {
            let metadata = component
                .properties
                .as_ref()
                .expect("Dapr properties")
                .metadata
                .iter()
                .filter_map(|item| Some((item.name.as_deref()?, item.value.as_deref()?)))
                .collect::<HashMap<_, _>>();
            assert_eq!(
                metadata.get("namespaceName").copied(),
                Some("default-service-bus-namespace")
            );
            record(&put_actions, format!("put-dapr:{name}"));
            put_components
                .lock()
                .expect("component map lock")
                .insert(name.to_string(), component.clone());
            Ok(OperationResult::Completed(component.clone()))
        });
    let container_apps = Arc::new(container_apps);

    let mut service_bus = MockServiceBusManagementApi::new();
    let delete_queue_actions = actions.clone();
    service_bus.expect_delete_queue().times(1).returning(
        move |resource_group, namespace, queue| {
            record(
                &delete_queue_actions,
                format!("delete-queue:{resource_group}/{namespace}/{queue}"),
            );
            Ok(())
        },
    );
    let create_queue_actions = actions.clone();
    service_bus
        .expect_create_or_update_queue()
        .times(1)
        .returning(move |resource_group, namespace, queue, _| {
            record(
                &create_queue_actions,
                format!("create-queue:{resource_group}/{namespace}/{queue}"),
            );
            Ok(alien_azure_clients::models::queue::SbQueue::default())
        });
    let service_bus = Arc::new(service_bus);

    let mut authorization = MockAuthorizationApi::new();
    authorization
        .expect_build_role_assignment_id()
        .returning(|scope, name| {
            format!(
                "/{}/providers/Microsoft.Authorization/roleAssignments/{name}",
                scope.to_scope_string(&alien_azure_clients::AzureClientConfig::mock())
            )
        });
    let old_assignment_name = commands_sender_role_assignment_name(
        "test",
        "commands-target-worker",
        "old-manager-principal",
        "old-namespace",
        &commands_queue_name(app_name),
    );
    let old_assignment_id = sender_assignment_id(
        "old-resource-group",
        "old-namespace",
        &commands_queue_name(app_name),
        &old_assignment_name,
    );
    let old_assignment = RoleAssignment {
        id: Some(old_assignment_id),
        name: Some(old_assignment_name),
        properties: Some(RoleAssignmentProperties {
            principal_id: "old-manager-principal".to_string(),
            role_definition_id: super::super::command_sender::commands_sender_role_definition_id(
                "12345678-1234-1234-1234-123456789012",
            ),
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
    };
    let list_responses = Arc::new(Mutex::new(VecDeque::from([
        vec![old_assignment],
        Vec::new(),
        Vec::new(),
    ])));
    authorization
        .expect_list_role_assignments()
        .times(1..)
        .returning(move |_, _| {
            Ok(list_responses
                .lock()
                .expect("list responses lock")
                .pop_front()
                .unwrap_or_default())
        });
    let delete_role_actions = actions.clone();
    authorization
        .expect_delete_role_assignment_by_id()
        .times(1)
        .returning(move |id| {
            record(&delete_role_actions, format!("delete-rbac:{id}"));
            Ok(None)
        });
    let put_role_actions = actions;
    authorization
        .expect_create_or_update_role_assignment_by_id()
        .times(1)
        .returning(move |id, assignment| {
            let principal = &assignment
                .properties
                .as_ref()
                .expect("role assignment properties")
                .principal_id;
            record(&put_role_actions, format!("put-rbac:{id}:{principal}"));
            Ok(assignment.clone())
        });
    let authorization = Arc::new(authorization);

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
        .expect_get_azure_caller_principal_id()
        .times(0..)
        .returning(|_| Ok("new-manager-principal".to_string()));
    Arc::new(provider)
}

#[tokio::test]
async fn commands_dependency_target_move_checkpoints_and_deletes_old_target_before_create() {
    let app_name = "test-commands-target-worker";
    let component_name = get_azure_internal_commands_dapr_component_name(app_name);
    let queue_name = commands_queue_name(app_name);
    let old_component = service_bus_dapr_component(
        component_name.clone(),
        app_name,
        "old-namespace",
        queue_name.clone(),
        "old-execution-client",
    );
    let actions = Arc::new(Mutex::new(Vec::new()));
    let provider = provider(app_name, old_component, actions.clone());

    let mut worker = basic_function();
    worker.id = "commands-target-worker".to_string();
    worker.commands_enabled = true;
    let mut controller = AzureWorkerController::mock_ready(app_name);
    controller.commands_resource_group_name = Some("old-resource-group".to_string());
    controller.commands_namespace_name = Some("old-namespace".to_string());
    controller.commands_queue_name = Some(queue_name.clone());
    controller.commands_dapr_component = Some(component_name.clone());
    controller.commands_sender_role_assignment_id = None;
    controller.commands_sender_role_assignment_discovery_complete = false;

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker.clone())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .build()
        .await
        .expect("executor should build");
    executor.update(worker).expect("equal update should start");

    for step in 0..10 {
        if executor
            .internal_state::<AzureWorkerController>()
            .expect("Azure worker controller")
            .commands_update_teardown_candidates_initialized
        {
            break;
        }
        executor
            .step()
            .await
            .unwrap_or_else(|error| panic!("commands checkpoint failed at step {step}: {error}"));
        assert!(
            actions
                .lock()
                .expect("action log lock")
                .iter()
                .all(|action| action == "update-app"),
            "no cleanup mutation is allowed before the durable teardown checkpoint"
        );
    }
    {
        let controller = executor
            .internal_state::<AzureWorkerController>()
            .expect("Azure worker controller");
        assert!(controller.commands_update_teardown_candidates_initialized);
        assert_eq!(
            controller.commands_resource_group_name.as_deref(),
            Some("old-resource-group")
        );
        assert_eq!(
            controller.commands_namespace_name.as_deref(),
            Some("old-namespace")
        );
        assert_eq!(
            controller.commands_queue_name.as_deref(),
            Some(queue_name.as_str())
        );
        assert_eq!(
            controller.commands_dapr_component.as_deref(),
            Some(component_name.as_str())
        );
    }
    assert_eq!(
        actions.lock().expect("action log lock").as_slice(),
        ["update-app"],
        "cleanup cursors must be durable before remote deletion"
    );

    let old_queue_action = format!("delete-queue:old-resource-group/old-namespace/{queue_name}");
    for step in 0..40 {
        if actions
            .lock()
            .expect("action log lock")
            .contains(&old_queue_action)
        {
            break;
        }
        executor
            .step()
            .await
            .unwrap_or_else(|error| panic!("old target cleanup failed at step {step}: {error}"));
    }
    assert!(!actions
        .lock()
        .expect("action log lock")
        .iter()
        .any(|action| action.starts_with("create-queue:")));
    let controller = executor
        .internal_state::<AzureWorkerController>()
        .expect("Azure worker controller");
    assert!(controller.commands_resource_group_name.is_none());
    assert!(controller.commands_namespace_name.is_none());
    assert!(controller.commands_queue_name.is_none());

    executor
        .step()
        .await
        .expect("checkpoint completed teardown");
    executor
        .step()
        .await
        .expect("checkpoint exact desired target");
    let controller = executor
        .internal_state::<AzureWorkerController>()
        .expect("Azure worker controller");
    assert_eq!(
        controller.commands_resource_group_name.as_deref(),
        Some("mock-rg")
    );
    assert_eq!(
        controller.commands_namespace_name.as_deref(),
        Some("default-service-bus-namespace")
    );
    assert_eq!(
        controller.commands_queue_name.as_deref(),
        Some(queue_name.as_str())
    );
    assert!(!controller.commands_sender_role_assignment_discovery_complete);
    assert!(
        !actions
            .lock()
            .expect("action log lock")
            .iter()
            .any(|action| action.starts_with("create-queue:")),
        "desired target must be durable before queue creation"
    );

    for step in 0..40 {
        if actions
            .lock()
            .expect("action log lock")
            .iter()
            .any(|action| {
                action.starts_with("put-rbac:") && action.ends_with(":new-manager-principal")
            })
        {
            break;
        }
        executor
            .step()
            .await
            .unwrap_or_else(|error| panic!("new target creation failed at step {step}: {error}"));
    }

    let actions = actions.lock().expect("action log lock");
    let delete_dapr = action_index(&actions, &format!("delete-dapr:{component_name}"));
    let old_assignment_name = commands_sender_role_assignment_name(
        "test",
        "commands-target-worker",
        "old-manager-principal",
        "old-namespace",
        &queue_name,
    );
    let delete_sender = action_index(
        &actions,
        &format!(
            "delete-rbac:{}",
            sender_assignment_id(
                "old-resource-group",
                "old-namespace",
                &queue_name,
                &old_assignment_name,
            )
        ),
    );
    let delete_queue = action_index(&actions, &old_queue_action);
    let create_queue = action_index(
        &actions,
        &format!("create-queue:mock-rg/default-service-bus-namespace/{queue_name}"),
    );
    let put_dapr = action_index(&actions, &format!("put-dapr:{component_name}"));
    let put_sender = actions
        .iter()
        .position(|action| {
            action.starts_with("put-rbac:") && action.ends_with(":new-manager-principal")
        })
        .expect("new direct sender assignment");
    assert!(
        delete_dapr < delete_sender
            && delete_sender < delete_queue
            && delete_queue < create_queue
            && create_queue < put_dapr
            && put_dapr < put_sender,
        "old target must be gone before new target creation: {actions:#?}"
    );
    assert_eq!(executor.status(), ResourceStatus::Updating);
}

fn legacy_commands_provider(
    app_name: &str,
    queue_deleted: Arc<Mutex<Option<String>>>,
) -> Arc<MockPlatformServiceProvider> {
    let mut container_apps = MockContainerAppsApi::new();
    let update_name = app_name.to_string();
    container_apps
        .expect_update_container_app()
        .times(1)
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&update_name, false),
            ))
        });
    let get_name = app_name.to_string();
    container_apps
        .expect_get_container_app()
        .times(1)
        .returning(move |_, _| Ok(create_successful_container_app_response(&get_name, false)));
    container_apps
        .expect_get_dapr_component()
        .times(1..)
        .returning(|_, _, name| Err(remote_not_found(name)));
    let container_apps = Arc::new(container_apps);

    let mut authorization = MockAuthorizationApi::new();
    authorization
        .expect_list_role_assignments()
        .times(1)
        .returning(|_, _| Ok(Vec::new()));
    let authorization = Arc::new(authorization);

    let mut service_bus = MockServiceBusManagementApi::new();
    service_bus.expect_delete_queue().times(1).returning(
        move |resource_group, namespace, queue| {
            assert_eq!(resource_group, "rotated-resource-group");
            assert_eq!(namespace, "legacy-namespace");
            *queue_deleted.lock().expect("queue deletion lock") =
                Some(format!("{resource_group}/{namespace}/{queue}"));
            Ok(())
        },
    );
    let service_bus = Arc::new(service_bus);

    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_container_apps_client()
        .returning(move |_| Ok(container_apps.clone()));
    provider
        .expect_get_azure_authorization_client()
        .returning(move |_| Ok(authorization.clone()));
    provider
        .expect_get_azure_service_bus_management_client()
        .returning(move |_| Ok(service_bus.clone()));
    Arc::new(provider)
}

#[tokio::test]
async fn legacy_commands_state_without_resource_group_checkpoints_rotated_cleanup_target() {
    let app_name = "test-commands-target-worker";
    let queue_name = commands_queue_name(app_name);
    let mut previous = basic_function();
    previous.id = "commands-target-worker".to_string();
    previous.commands_enabled = true;
    let mut desired = previous.clone();
    desired.commands_enabled = false;

    let mut legacy = AzureWorkerController::mock_ready(app_name);
    legacy.commands_resource_group_name = None;
    legacy.commands_namespace_name = Some("legacy-namespace".to_string());
    legacy.commands_queue_name = Some(queue_name.clone());
    let mut serialized = serde_json::to_value(legacy).expect("serialize legacy controller");
    serialized
        .as_object_mut()
        .expect("controller object")
        .remove("commandsResourceGroupName");
    let legacy: AzureWorkerController =
        serde_json::from_value(serialized).expect("deserialize pre-resource-group state");

    let queue_deleted = Arc::new(Mutex::new(None));
    let mut namespace = crate::infra_requirements::AzureServiceBusNamespaceController::mock_ready(
        "rotated-namespace",
    );
    namespace.resource_group_name = Some("rotated-resource-group".to_string());
    let mut executor = SingleControllerExecutor::builder()
        .resource(previous)
        .controller(legacy)
        .platform(Platform::Azure)
        .service_provider(legacy_commands_provider(app_name, queue_deleted.clone()))
        .with_test_dependencies()
        .with_dependency(
            crate::core::controller_test::test_azure_service_bus_namespace(),
            namespace,
        )
        .build()
        .await
        .expect("legacy executor");
    executor.update(desired).expect("disable commands update");

    for step in 0..40 {
        if queue_deleted.lock().expect("queue deletion lock").is_some() {
            break;
        }
        executor
            .step()
            .await
            .unwrap_or_else(|error| panic!("legacy cleanup failed at step {step}: {error}"));
        let controller = executor
            .internal_state::<AzureWorkerController>()
            .expect("Azure worker controller");
        if controller.commands_update_teardown_candidates_initialized {
            assert_eq!(
                controller.commands_resource_group_name.as_deref(),
                Some("rotated-resource-group")
            );
            assert_eq!(
                controller.commands_namespace_name.as_deref(),
                Some("legacy-namespace")
            );
            assert_eq!(
                controller.commands_queue_name.as_deref(),
                Some(queue_name.as_str())
            );
        }
    }
    assert_eq!(
        queue_deleted
            .lock()
            .expect("queue deletion lock")
            .as_deref(),
        Some(format!("rotated-resource-group/legacy-namespace/{queue_name}").as_str())
    );
}
