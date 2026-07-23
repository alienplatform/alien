use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alien_azure_clients::authorization::MockAuthorizationApi;
use alien_azure_clients::container_apps::MockContainerAppsApi;
use alien_azure_clients::event_grid::{
    EventSubscription, EventSubscriptionProperties, MockEventGridApi,
};
use alien_azure_clients::long_running_operation::OperationResult;
use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::models::managed_environments_dapr_components::DaprComponent;
use alien_azure_clients::service_bus::MockServiceBusManagementApi;
use alien_azure_clients::AzureClientConfigExt;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Platform, ServiceAccount, Worker, WorkerTrigger};
use alien_error::AlienError;

use crate::core::controller_test::{
    test_azure_service_bus_namespace, test_storage_1, test_storage_2, SingleControllerExecutor,
};
use crate::core::MockPlatformServiceProvider;
use crate::infra_requirements::AzureServiceBusNamespaceController;
use crate::service_account::AzureServiceAccountController;
use crate::storage::AzureStorageController;
use crate::worker::azure::AzureStorageTriggerInfrastructure;
use crate::worker::azure_dapr_components::service_bus_dapr_component;
use crate::worker::azure_names::{
    get_azure_blob_trigger_dapr_component_name, get_azure_storage_event_subscription_name,
    get_legacy_azure_blob_trigger_dapr_component_names, storage_trigger_queue_name,
    storage_trigger_receiver_role_assignment_name,
};
use crate::worker::AzureWorkerController;

const SUBSCRIPTION_ID: &str = "12345678-1234-1234-1234-123456789012";

fn record(actions: &Arc<Mutex<Vec<String>>>, action: impl Into<String>) {
    actions.lock().expect("action log lock").push(action.into());
}

fn action_index(actions: &[String], expected: &str) -> usize {
    actions
        .iter()
        .position(|action| action == expected)
        .unwrap_or_else(|| panic!("missing action {expected:?} in {actions:#?}"))
}

fn remote_not_found(resource_type: &str, resource_name: &str) -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
        resource_type: resource_type.to_string(),
        resource_name: resource_name.to_string(),
    })
}

fn receiver_role_assignment(target: &StorageTarget, role_definition_id: &str) -> RoleAssignment {
    RoleAssignment {
        id: Some(target.receiver_assignment_id.clone()),
        name: Some(
            target
                .receiver_assignment_id
                .rsplit('/')
                .next()
                .expect("role assignment name")
                .to_string(),
        ),
        properties: Some(RoleAssignmentProperties {
            principal_id: target.execution_principal_id.clone(),
            role_definition_id: role_definition_id.to_string(),
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
    }
}

#[derive(Clone)]
struct StorageTarget {
    storage_id: String,
    source_resource_id: String,
    source_container_name: String,
    event_subscription_name: String,
    resource_group: String,
    namespace: String,
    queue: String,
    receiver_assignment_id: String,
    execution_principal_id: String,
}

struct StorageProviderExpectations {
    app_name: String,
    existing_components: Vec<DaprComponent>,
    old: StorageTarget,
    desired: StorageTarget,
    expected_execution_client_id: String,
    expected_execution_principal_id: String,
    expected_container_identity_id: Option<String>,
    expect_container_app_update: bool,
    deletes_are_missing: bool,
}

fn assert_container_app_identity(
    app: &alien_azure_clients::models::container_apps::ContainerApp,
    expected_identity_id: &str,
    expected_client_id: &str,
) {
    let serialized = serde_json::to_value(app).expect("serialize desired Container App");
    let identities = serialized
        .pointer("/identity/userAssignedIdentities")
        .and_then(serde_json::Value::as_object)
        .expect("desired Container App user-assigned identities");
    assert_eq!(
        identities.keys().cloned().collect::<Vec<_>>(),
        vec![expected_identity_id.to_string()]
    );
    let environment = serialized
        .pointer("/properties/template/containers/0/env")
        .and_then(serde_json::Value::as_array)
        .expect("desired Container App environment");
    let azure_client_id = environment
        .iter()
        .find(|item| {
            item.get("name").and_then(serde_json::Value::as_str) == Some("AZURE_CLIENT_ID")
        })
        .and_then(|item| item.get("value"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(azure_client_id, Some(expected_client_id));
}

fn storage_provider(
    expected: StorageProviderExpectations,
    actions: Arc<Mutex<Vec<String>>>,
) -> Arc<MockPlatformServiceProvider> {
    let components = Arc::new(Mutex::new(
        expected
            .existing_components
            .into_iter()
            .map(|component| {
                (
                    component.name.clone().expect("existing component name"),
                    component,
                )
            })
            .collect::<HashMap<_, _>>(),
    ));

    let mut container_apps = MockContainerAppsApi::new();
    let update_actions = actions.clone();
    let app_name_for_update = expected.app_name.clone();
    let expected_identity_id = expected.expected_container_identity_id.clone();
    let expected_client_id = expected.expected_execution_client_id.clone();
    container_apps
        .expect_update_container_app()
        .times(usize::from(expected.expect_container_app_update))
        .returning(move |resource_group, app_name, app| {
            assert_eq!(resource_group, "default-resource-group");
            assert_eq!(app_name, app_name_for_update);
            if let Some(identity_id) = expected_identity_id.as_deref() {
                assert_container_app_identity(app, identity_id, &expected_client_id);
            }
            record(&update_actions, "update-app");
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_update, false),
            ))
        });
    let app_name_for_get = expected.app_name.clone();
    container_apps
        .expect_get_container_app()
        .times(usize::from(expected.expect_container_app_update))
        .returning(move |resource_group, app_name| {
            assert_eq!(resource_group, "default-resource-group");
            assert_eq!(app_name, app_name_for_get);
            Ok(create_successful_container_app_response(
                &app_name_for_get,
                false,
            ))
        });
    let get_components = components.clone();
    container_apps
        .expect_get_dapr_component()
        .times(1..)
        .returning(move |_, _, component_name| {
            get_components
                .lock()
                .expect("component map lock")
                .get(component_name)
                .cloned()
                .ok_or_else(|| remote_not_found("Dapr component", component_name))
        });
    let delete_components = components.clone();
    let delete_actions = actions.clone();
    let missing_dapr_delete = expected.deletes_are_missing;
    container_apps
        .expect_delete_dapr_component()
        .times(1..)
        .returning(move |_, _, component_name| {
            assert!(
                delete_components
                    .lock()
                    .expect("component map lock")
                    .remove(component_name)
                    .is_some(),
                "controller must establish ownership before deleting {component_name}"
            );
            record(&delete_actions, format!("delete-dapr:{component_name}"));
            if missing_dapr_delete {
                Err(remote_not_found("Dapr component", component_name))
            } else {
                Ok(OperationResult::Completed(()))
            }
        });
    let put_components = components;
    let put_actions = actions.clone();
    let desired_client_id = expected.expected_execution_client_id.clone();
    container_apps
        .expect_create_or_update_dapr_component()
        .times(1)
        .returning(move |_, _, component_name, component| {
            let metadata = component
                .properties
                .as_ref()
                .expect("Dapr component properties")
                .metadata
                .iter()
                .filter_map(|item| Some((item.name.as_deref()?, item.value.as_deref()?)))
                .collect::<HashMap<_, _>>();
            assert_eq!(
                metadata.get("azureClientId").copied(),
                Some(desired_client_id.as_str())
            );
            record(&put_actions, format!("put-dapr:{component_name}"));
            put_components
                .lock()
                .expect("component map lock")
                .insert(component_name.to_string(), component.clone());
            Ok(OperationResult::Completed(component.clone()))
        });
    let container_apps = Arc::new(container_apps);

    let mut event_grid = MockEventGridApi::new();
    let old_for_delete = expected.old.clone();
    let event_delete_actions = actions.clone();
    let missing_event_delete = expected.deletes_are_missing;
    event_grid
        .expect_delete_event_subscription()
        .times(1)
        .returning(move |source_resource_id, subscription_name| {
            assert_eq!(source_resource_id, old_for_delete.source_resource_id);
            assert_eq!(subscription_name, old_for_delete.event_subscription_name);
            record(
                &event_delete_actions,
                format!("delete-event:{source_resource_id}/{subscription_name}"),
            );
            if missing_event_delete {
                Err(remote_not_found(
                    "Event Grid subscription",
                    &subscription_name,
                ))
            } else {
                Ok(())
            }
        });
    let desired_for_create = expected.desired.clone();
    let event_create_actions = actions.clone();
    event_grid
        .expect_create_or_update_event_subscription()
        .times(1)
        .returning(move |source_resource_id, subscription_name, request| {
            assert_eq!(source_resource_id, desired_for_create.source_resource_id);
            assert_eq!(subscription_name, desired_for_create.event_subscription_name);
            assert_eq!(
                request.properties.destination.properties.resource_id,
                format!(
                    "/subscriptions/{SUBSCRIPTION_ID}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}/queues/{}",
                    desired_for_create.resource_group,
                    desired_for_create.namespace,
                    desired_for_create.queue
                )
            );
            assert_eq!(
                request.properties.filter.subject_begins_with,
                format!(
                    "/blobServices/default/containers/{}/blobs/",
                    desired_for_create.source_container_name
                )
            );
            record(
                &event_create_actions,
                format!("create-event:{source_resource_id}/{subscription_name}"),
            );
            Ok(EventSubscription {
                id: None,
                name: Some(subscription_name),
                properties: Some(EventSubscriptionProperties {
                    provisioning_state: Some("Succeeded".to_string()),
                }),
            })
        });
    let event_grid = Arc::new(event_grid);

    let mut service_bus = MockServiceBusManagementApi::new();
    let old_for_queue_delete = expected.old.clone();
    let queue_delete_actions = actions.clone();
    let missing_queue_delete = expected.deletes_are_missing;
    service_bus.expect_delete_queue().times(1).returning(
        move |resource_group, namespace, queue| {
            assert_eq!(resource_group, old_for_queue_delete.resource_group);
            assert_eq!(namespace, old_for_queue_delete.namespace);
            assert_eq!(queue, old_for_queue_delete.queue);
            record(
                &queue_delete_actions,
                format!("delete-queue:{resource_group}/{namespace}/{queue}"),
            );
            if missing_queue_delete {
                Err(remote_not_found("Service Bus queue", &queue))
            } else {
                Ok(())
            }
        },
    );
    let desired_for_queue_create = expected.desired.clone();
    let queue_create_actions = actions.clone();
    service_bus
        .expect_create_or_update_queue()
        .times(1..)
        .returning(move |resource_group, namespace, queue, _| {
            assert_eq!(resource_group, desired_for_queue_create.resource_group);
            assert_eq!(namespace, desired_for_queue_create.namespace);
            assert_eq!(queue, desired_for_queue_create.queue);
            record(
                &queue_create_actions,
                format!("create-queue:{resource_group}/{namespace}/{queue}"),
            );
            Ok(alien_azure_clients::models::queue::SbQueue::default())
        });
    let service_bus = Arc::new(service_bus);

    let mut authorization = MockAuthorizationApi::new();
    authorization
        .expect_build_role_assignment_id()
        .returning(|scope, assignment_name| {
            format!(
                "/{}/providers/Microsoft.Authorization/roleAssignments/{assignment_name}",
                scope.to_scope_string(&alien_azure_clients::AzureClientConfig::mock())
            )
        });
    let role_definition_id = format!(
        "/subscriptions/{SUBSCRIPTION_ID}/providers/Microsoft.Authorization/roleDefinitions/4f6d3b9b-027b-4f4c-9142-0e5a2a2247e0"
    );
    let old_for_list = expected.old.clone();
    let desired_for_list = expected.desired.clone();
    let assignment_availability = Arc::new(Mutex::new((true, false)));
    let list_availability = assignment_availability.clone();
    let role_definition_for_list = role_definition_id.clone();
    authorization
        .expect_list_role_assignments()
        .times(1..)
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
            let old_scope = old_for_list
                .receiver_assignment_id
                .split("/providers/Microsoft.Authorization")
                .next()
                .expect("old assignment scope");
            let desired_scope = desired_for_list
                .receiver_assignment_id
                .split("/providers/Microsoft.Authorization")
                .next()
                .expect("desired assignment scope");
            let availability = list_availability.lock().expect("assignment availability");
            if scope.eq_ignore_ascii_case(old_scope) && availability.0 {
                return Ok(vec![receiver_role_assignment(
                    &old_for_list,
                    &role_definition_for_list,
                )]);
            }
            if scope.eq_ignore_ascii_case(desired_scope) && availability.1 {
                return Ok(vec![receiver_role_assignment(
                    &desired_for_list,
                    &role_definition_for_list,
                )]);
            }
            Ok(Vec::new())
        });
    let old_assignment_id = expected.old.receiver_assignment_id.clone();
    let role_delete_actions = actions.clone();
    let missing_role_delete = expected.deletes_are_missing;
    let delete_availability = assignment_availability.clone();
    authorization
        .expect_delete_role_assignment_by_id()
        .times(1)
        .returning(move |assignment_id| {
            assert_eq!(assignment_id, old_assignment_id);
            delete_availability
                .lock()
                .expect("assignment availability")
                .0 = false;
            record(&role_delete_actions, format!("delete-rbac:{assignment_id}"));
            if missing_role_delete {
                Err(remote_not_found("role assignment", &assignment_id))
            } else {
                Ok(None)
            }
        });
    let desired_assignment_id = expected.desired.receiver_assignment_id;
    let desired_role_principal = expected.expected_execution_principal_id;
    let role_put_actions = actions;
    let put_availability = assignment_availability;
    authorization
        .expect_create_or_update_role_assignment_by_id()
        .times(1)
        .returning(move |assignment_id, assignment| {
            assert_eq!(assignment_id, desired_assignment_id);
            let principal_id = &assignment
                .properties
                .as_ref()
                .expect("role assignment properties")
                .principal_id;
            assert_eq!(principal_id, &desired_role_principal);
            put_availability.lock().expect("assignment availability").1 = true;
            record(
                &role_put_actions,
                format!("put-rbac:{assignment_id}:{principal_id}"),
            );
            Ok(assignment.clone())
        });
    let authorization = Arc::new(authorization);

    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_container_apps_client()
        .returning(move |_| Ok(container_apps.clone()));
    provider
        .expect_get_azure_event_grid_client()
        .returning(move |_| Ok(event_grid.clone()));
    provider
        .expect_get_azure_service_bus_management_client()
        .returning(move |_| Ok(service_bus.clone()));
    provider
        .expect_get_azure_authorization_client()
        .returning(move |_| Ok(authorization.clone()));
    Arc::new(provider)
}
