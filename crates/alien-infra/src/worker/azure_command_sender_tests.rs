use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use alien_azure_clients::authorization::{MockAuthorizationApi, Scope};
use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::service_bus::MockServiceBusManagementApi;
use alien_core::{Platform, ResourceStatus, Worker};

use super::{commands_sender_role_definition_id, AzureCommandsSenderRoleAssignmentIntent};
use crate::core::controller_test::SingleControllerExecutor;
use crate::core::MockPlatformServiceProvider;
use crate::worker::azure_names::commands_sender_role_assignment_name;
use crate::worker::fixtures::basic_function;
use crate::worker::{AzureWorkerController, AzureWorkerState};

const SUBSCRIPTION_ID: &str = "12345678-1234-1234-1234-123456789012";
const RESOURCE_GROUP: &str = "mock-rg";
const NAMESPACE: &str = "default-service-bus-namespace";
const QUEUE: &str = "test-sender-worker-rq";
const WORKER_ID: &str = "sender-worker";

fn worker(commands_enabled: bool) -> Worker {
    let mut worker = basic_function();
    worker.id = WORKER_ID.to_string();
    worker.commands_enabled = commands_enabled;
    worker
}

fn assignment_id(name: &str) -> String {
    format!(
        "/subscriptions/{SUBSCRIPTION_ID}/resourceGroups/{RESOURCE_GROUP}/providers/Microsoft.ServiceBus/namespaces/{NAMESPACE}/queues/{QUEUE}/providers/Microsoft.Authorization/roleAssignments/{name}"
    )
}

fn direct_assignment(principal_id: &str) -> RoleAssignment {
    let name =
        commands_sender_role_assignment_name("test", WORKER_ID, principal_id, NAMESPACE, QUEUE);
    RoleAssignment {
        id: Some(assignment_id(&name)),
        name: Some(name),
        properties: Some(RoleAssignmentProperties {
            principal_id: principal_id.to_string(),
            role_definition_id: commands_sender_role_definition_id(SUBSCRIPTION_ID),
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

fn setup_owned_assignment(principal_id: &str) -> RoleAssignment {
    RoleAssignment {
        id: Some(assignment_id("setup-owned")),
        name: Some("setup-owned".to_string()),
        properties: Some(RoleAssignmentProperties {
            principal_id: principal_id.to_string(),
            role_definition_id: commands_sender_role_definition_id(SUBSCRIPTION_ID),
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

fn provider(
    principal_id: &str,
    list_responses: Vec<Vec<RoleAssignment>>,
    actions: Arc<Mutex<Vec<String>>>,
) -> Arc<MockPlatformServiceProvider> {
    provider_with_queue_cleanup(principal_id, list_responses, actions, false)
}

fn provider_with_queue_cleanup(
    principal_id: &str,
    list_responses: Vec<Vec<RoleAssignment>>,
    actions: Arc<Mutex<Vec<String>>>,
    delete_queue: bool,
) -> Arc<MockPlatformServiceProvider> {
    let mut authorization = MockAuthorizationApi::new();
    authorization
        .expect_build_role_assignment_id()
        .returning(|_, name| assignment_id(&name));
    let responses = Arc::new(Mutex::new(VecDeque::from(list_responses)));
    authorization
        .expect_list_role_assignments()
        .returning(move |scope, role_definition_id| {
            assert!(matches!(
                scope,
                Scope::Resource {
                    resource_group_name,
                    resource_provider,
                    parent_resource_path: Some(parent),
                    resource_type,
                    resource_name,
                } if resource_group_name == RESOURCE_GROUP
                    && resource_provider == "Microsoft.ServiceBus"
                    && parent == &format!("namespaces/{NAMESPACE}")
                    && resource_type == "queues"
                    && resource_name == QUEUE
            ));
            assert_eq!(
                role_definition_id.as_deref(),
                Some(commands_sender_role_definition_id(SUBSCRIPTION_ID).as_str())
            );
            Ok(responses
                .lock()
                .expect("list response lock")
                .pop_front()
                .unwrap_or_default())
        });
    let delete_actions = actions.clone();
    authorization
        .expect_delete_role_assignment_by_id()
        .returning(move |id| {
            delete_actions
                .lock()
                .expect("action lock")
                .push(format!("delete:{id}"));
            Ok(None)
        });
    let put_actions = actions.clone();
    authorization
        .expect_create_or_update_role_assignment_by_id()
        .returning(move |id, assignment| {
            let principal = assignment
                .properties
                .as_ref()
                .expect("role assignment properties")
                .principal_id
                .clone();
            put_actions
                .lock()
                .expect("action lock")
                .push(format!("put:{id}:{principal}"));
            Ok(assignment.clone())
        });
    let authorization = Arc::new(authorization);

    let principal_id = principal_id.to_string();
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_caller_principal_id()
        .times(0..)
        .returning(move |_| Ok(principal_id.clone()));
    provider
        .expect_get_azure_authorization_client()
        .returning(move |_| Ok(authorization.clone()));
    if delete_queue {
        let queue_actions = actions.clone();
        let mut service_bus = MockServiceBusManagementApi::new();
        service_bus.expect_delete_queue().times(1).returning(
            move |resource_group, namespace, queue| {
                assert_eq!(resource_group, RESOURCE_GROUP);
                assert_eq!(namespace, NAMESPACE);
                assert_eq!(queue, QUEUE);
                queue_actions
                    .lock()
                    .expect("action lock")
                    .push("delete-queue".to_string());
                Ok(())
            },
        );
        let service_bus = Arc::new(service_bus);
        provider
            .expect_get_azure_service_bus_management_client()
            .times(1)
            .returning(move |_| Ok(service_bus.clone()));
    }
    Arc::new(provider)
}

async fn build_executor(
    worker: Worker,
    mut controller: AzureWorkerController,
    provider: Arc<MockPlatformServiceProvider>,
) -> SingleControllerExecutor {
    controller.commands_resource_group_name = Some(RESOURCE_GROUP.to_string());
    controller.commands_namespace_name = Some(NAMESPACE.to_string());
    controller.commands_queue_name = Some(QUEUE.to_string());
    SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(provider)
        .with_test_dependencies()
        .build()
        .await
        .expect("executor should build")
}

#[tokio::test]
async fn sender_intent_is_checkpointed_before_put_and_crash_retry_reuses_exact_id() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let provider = provider("principal-b", vec![vec![]], actions.clone());
    let mut executor = build_executor(
        worker(true),
        AzureWorkerController::mock_ready("test-sender-worker"),
        provider.clone(),
    )
    .await;

    executor.step().await.expect("discovery checkpoint");
    assert!(actions.lock().expect("action lock").is_empty());
    executor.step().await.expect("intent checkpoint");
    let before_put = executor
        .internal_state::<AzureWorkerController>()
        .expect("controller")
        .clone();
    let planned_id = before_put
        .commands_sender_role_assignment_intent
        .as_ref()
        .expect("planned sender")
        .assignment_id
        .clone();
    assert!(actions.lock().expect("action lock").is_empty());

    executor.step().await.expect("first idempotent put");
    let mut retry_executor = build_executor(worker(true), before_put, provider).await;
    retry_executor
        .step()
        .await
        .expect("retry the same idempotent put");

    let puts: Vec<String> = actions
        .lock()
        .expect("action lock")
        .iter()
        .filter(|action| action.starts_with("put:"))
        .cloned()
        .collect();
    assert_eq!(puts.len(), 2);
    assert!(puts
        .iter()
        .all(|action| action.contains(&planned_id) && action.ends_with(":principal-b")));
}

#[tokio::test]
async fn applied_principal_is_deleted_before_new_principal_is_planned_and_put() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let mut controller = AzureWorkerController::mock_ready("test-sender-worker");
    let old = direct_assignment("principal-a");
    let provider = provider(
        "principal-b",
        vec![vec![old.clone()], vec![]],
        actions.clone(),
    );
    controller.commands_sender_role_assignment_id = old.id.clone();
    controller.commands_sender_role_assignment_discovery_complete = true;
    let mut executor = build_executor(worker(true), controller, provider).await;

    for _ in 0..5 {
        executor.step().await.expect("sender reconciliation step");
    }

    let actions = actions.lock().expect("action lock");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0], format!("delete:{}", old.id.as_deref().unwrap()));
    assert!(actions[1].starts_with("put:"));
    assert!(actions[1].ends_with(":principal-b"));
}

#[tokio::test]
async fn missing_applied_grant_is_recreated_only_after_discovery_checkpoint() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let provider = provider("principal-b", vec![vec![]], actions.clone());
    let mut controller = AzureWorkerController::mock_ready("test-sender-worker");
    controller.commands_sender_role_assignment_id = direct_assignment("principal-b").id;
    controller.commands_sender_role_assignment_discovery_complete = false;
    let mut executor = build_executor(worker(true), controller, provider).await;

    executor.step().await.expect("missing grant discovery");
    assert!(actions.lock().expect("action lock").is_empty());
    executor.step().await.expect("intent checkpoint");
    assert!(actions.lock().expect("action lock").is_empty());
    executor.step().await.expect("grant recreation");

    let actions = actions.lock().expect("action lock");
    assert_eq!(actions.len(), 1);
    assert!(actions[0].starts_with("put:"));
    assert!(actions[0].ends_with(":principal-b"));
}

#[tokio::test]
async fn equal_update_revalidates_missing_and_stale_sender_grants() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let desired = direct_assignment("principal-b");
    let stale = direct_assignment("principal-a");
    let mut controller = AzureWorkerController::mock_ready("test-sender-worker");
    controller.commands_sender_role_assignment_id = desired.id.clone();
    controller.commands_sender_role_assignment_discovery_complete = true;
    let mut checkpoint_executor = build_executor(
        worker(true),
        controller,
        Arc::new(MockPlatformServiceProvider::new()),
    )
    .await;
    checkpoint_executor
        .update(worker(true))
        .expect("equal update should start");
    checkpoint_executor
        .step()
        .await
        .expect("enter equal update");
    checkpoint_executor
        .step()
        .await
        .expect("equal update discovery checkpoint");
    let mut checkpointed = checkpoint_executor
        .internal_state::<AzureWorkerController>()
        .expect("checkpointed controller")
        .clone();
    assert_eq!(checkpointed.state, AzureWorkerState::UpdateStart);
    assert!(!checkpointed.commands_sender_role_assignment_discovery_complete);

    checkpointed.state = AzureWorkerState::Ready;
    let provider = provider(
        "principal-b",
        vec![vec![stale.clone()], vec![]],
        actions.clone(),
    );
    let mut executor = build_executor(worker(true), checkpointed, provider).await;
    for _ in 0..4 {
        executor
            .step()
            .await
            .expect("checkpointed sender drift reconciliation");
    }

    let actions = actions.lock().expect("action lock");
    assert_eq!(actions.len(), 2);
    assert_eq!(
        actions[0],
        format!(
            "delete:{}",
            stale.id.as_deref().expect("stale assignment ID")
        )
    );
    assert!(actions[1].starts_with("put:"));
    assert!(actions[1].ends_with(":principal-b"));
}

#[tokio::test]
async fn desired_grant_discovered_from_azure_is_adopted_without_put() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let desired = direct_assignment("principal-b");
    let provider = provider("principal-b", vec![vec![desired.clone()]], actions.clone());
    let mut executor = build_executor(
        worker(true),
        AzureWorkerController::mock_ready("test-sender-worker"),
        provider,
    )
    .await;

    executor.step().await.expect("discover desired grant");

    assert!(actions.lock().expect("action lock").is_empty());
    let controller = executor
        .internal_state::<AzureWorkerController>()
        .expect("controller");
    assert_eq!(controller.commands_sender_role_assignment_id, desired.id);
    assert!(controller.commands_sender_role_assignment_discovery_complete);
}

#[tokio::test]
async fn discovery_deletes_stale_direct_grant_and_preserves_setup_owned_assignment() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let old = direct_assignment("principal-a");
    let setup_owned = setup_owned_assignment("setup-principal");
    let provider = provider(
        "principal-b",
        vec![vec![old.clone(), setup_owned.clone()], vec![setup_owned]],
        actions.clone(),
    );
    let mut executor = build_executor(
        worker(true),
        AzureWorkerController::mock_ready("test-sender-worker"),
        provider,
    )
    .await;

    for _ in 0..4 {
        executor.step().await.expect("sender reconciliation step");
    }

    let actions = actions.lock().expect("action lock");
    let expected_delete = format!("delete:{}", old.id.as_deref().unwrap());
    assert_eq!(
        actions
            .iter()
            .filter(|action| action.starts_with("delete:"))
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected_delete]
    );
    assert!(actions
        .iter()
        .any(|action| action.ends_with(":principal-b")));
    assert!(!actions.iter().any(|action| action.contains("setup-owned")));
}

#[tokio::test]
async fn disabling_commands_deletes_applied_direct_grant_without_putting_a_new_one() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let old = direct_assignment("principal-a");
    let provider = provider("unused", vec![vec![old.clone()], vec![]], actions.clone());
    let mut controller = AzureWorkerController::mock_ready("test-sender-worker");
    controller.commands_sender_role_assignment_id = old.id.clone();
    controller.commands_sender_role_assignment_discovery_complete = true;
    let mut executor = build_executor(worker(false), controller, provider).await;

    for _ in 0..3 {
        executor.step().await.expect("sender revoke");
    }

    let actions = actions.lock().expect("action lock");
    assert_eq!(
        actions.as_slice(),
        [format!("delete:{}", old.id.as_deref().unwrap())]
    );
    assert!(!actions.iter().any(|action| action.starts_with("put:")));
    assert_eq!(executor.status(), ResourceStatus::Running);
}

#[tokio::test]
async fn persisted_unproven_ids_are_cleared_without_delete_and_setup_owned_remote_is_preserved() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let setup_owned = setup_owned_assignment("setup-principal");
    let provider = provider("unused", vec![vec![setup_owned]], actions.clone());
    let mut controller = AzureWorkerController::mock_ready("test-sender-worker");
    controller.commands_sender_role_assignment_id = Some("unproven-applied-id".to_string());
    controller.commands_sender_role_assignment_intent =
        Some(AzureCommandsSenderRoleAssignmentIntent {
            assignment_id: "unproven-intent-id".to_string(),
            assignment_name: "unproven-intent".to_string(),
            principal_id: "unproven-principal".to_string(),
            resource_group_name: RESOURCE_GROUP.to_string(),
            namespace_name: NAMESPACE.to_string(),
            queue_name: QUEUE.to_string(),
        });
    controller.commands_sender_role_assignment_discovery_complete = true;
    let mut executor = build_executor(worker(false), controller, provider).await;

    for _ in 0..3 {
        executor.step().await.expect("proof-boundary cleanup");
    }

    assert!(actions.lock().expect("action lock").is_empty());
    let controller = executor
        .internal_state::<AzureWorkerController>()
        .expect("controller");
    assert!(controller.commands_sender_role_assignment_id.is_none());
    assert!(controller.commands_sender_role_assignment_intent.is_none());
    assert!(controller.commands_sender_role_assignment_discovery_complete);
}

#[tokio::test]
async fn cleanup_discovers_all_direct_grants_before_deleting_queue_and_preserves_setup_owned() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let first = direct_assignment("principal-a");
    let second = direct_assignment("principal-b");
    let setup_owned = setup_owned_assignment("setup-principal");
    let provider = provider_with_queue_cleanup(
        "unused",
        vec![
            vec![first.clone(), second.clone(), setup_owned.clone()],
            vec![second.clone(), setup_owned.clone()],
            vec![setup_owned],
        ],
        actions.clone(),
        true,
    );
    let mut controller = AzureWorkerController::mock_ready("test-sender-worker");
    controller.state = AzureWorkerState::DeletingCommandsInfrastructure;
    controller.commands_resource_group_name = Some(RESOURCE_GROUP.to_string());
    controller.commands_namespace_name = Some(NAMESPACE.to_string());
    controller.commands_queue_name = Some(QUEUE.to_string());
    controller.commands_sender_role_assignment_discovery_complete = false;
    let mut executor = build_executor(worker(false), controller, provider).await;

    for _ in 0..4 {
        executor.step().await.expect("commands cleanup step");
    }

    assert_eq!(
        actions.lock().expect("action lock").as_slice(),
        [
            format!("delete:{}", first.id.as_deref().unwrap()),
            format!("delete:{}", second.id.as_deref().unwrap()),
            "delete-queue".to_string(),
        ]
    );
    let controller = executor
        .internal_state::<AzureWorkerController>()
        .expect("controller");
    assert!(controller.commands_resource_group_name.is_none());
    assert!(controller.commands_namespace_name.is_none());
    assert!(controller.commands_queue_name.is_none());
}
