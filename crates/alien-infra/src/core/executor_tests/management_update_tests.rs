//! The management resource of an AWS install made before it depended on the network.
//!
//! A new ordering edge makes the planner schedule an update of management on such an install.
//! Outside a setup run that update must make no cloud call. Every provider here has no
//! expectations, so any cloud client request fails the test.

use super::helpers::default_deployment_config;
use crate::core::{MockPlatformServiceProvider, StackExecutor, StackResourceStateExt};
use crate::storage::AwsStorageController;
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt};
use alien_core::{
    ClientConfig, InitialSetupAuthority, Platform, Resource, ResourceDefinition, ResourceEntry,
    ResourceLifecycle, ResourceRef, ResourceStatus, Stack, StackResourceState, StackState, Storage,
};
use std::sync::Arc;

fn installed(
    resource: Resource,
    lifecycle: ResourceLifecycle,
    dependencies: Vec<ResourceRef>,
    controller: Box<dyn crate::core::ResourceController>,
) -> StackResourceState {
    let mut saved = StackResourceState::new_pending(
        resource.resource_type().to_string(),
        resource,
        Some(lifecycle),
        dependencies,
    );
    saved.status = ResourceStatus::Running;
    saved.set_internal_controller(Some(controller)).unwrap();
    saved
}

fn executor_for_run(
    stack: &Stack,
    authority: InitialSetupAuthority,
    setup_run: bool,
    resume_failed: bool,
) -> StackExecutor {
    let mut config = default_deployment_config();
    config.setup_run = setup_run;
    StackExecutor::builder(stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
        .deployment_config(&config)
        .initial_setup_authority(authority)
        .step_running_resources(false)
        .resume_unchanged_failed_resources(resume_failed)
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .build()
        .unwrap()
}

use crate::{
    network::AwsNetworkController,
    remote_stack_management::{AwsRemoteStackManagementController, AwsRemoteStackManagementState},
};
use alien_core::{Network, NetworkSettings, RemoteStackManagement};

fn network_edge() -> Vec<ResourceRef> {
    vec![ResourceRef::new(Network::RESOURCE_TYPE, "default-network")]
}

/// A Frozen network and a management resource, both Running as an install of the earlier
/// release left them: management has no edge to the network. The stack is the new release,
/// where management depends on the network.
fn management_gaining_the_network_edge() -> (Stack, StackState) {
    let network = Network::new("default-network".to_string())
        .settings(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        })
        .build();
    let management = RemoteStackManagement::new("management".to_string()).build();
    let mut stack = Stack::new("management-edge".to_string())
        .add(network.clone(), ResourceLifecycle::Frozen)
        .add(management.clone(), ResourceLifecycle::Frozen)
        .build();
    stack.resources.get_mut("management").unwrap().dependencies = network_edge();
    let mut state = StackState::new(Platform::Aws);
    state.resource_prefix = "mgmt-test".to_string();
    state.resources.insert(
        "default-network".to_string(),
        installed(
            Resource::new(network),
            ResourceLifecycle::Frozen,
            vec![],
            Box::new(AwsNetworkController::mock_ready("vpc-1", 2)),
        ),
    );
    state.resources.insert(
        "management".to_string(),
        installed(
            Resource::new(management),
            ResourceLifecycle::Frozen,
            vec![],
            Box::new(AwsRemoteStackManagementController::mock_ready(
                "mgmt-test-management",
            )),
        ),
    );
    (stack, state)
}

/// The management resource as the earlier build left it after the update it should never have
/// run: config and edges already the new release's, status UpdateFailed, the controller failed
/// at its first update state with the retries used up, and the snapshot to resume from.
fn management_failed_at_update_start(state: &mut StackState) {
    let management = state.resources.get_mut("management").unwrap();
    management.dependencies = network_edge();
    management.previous_config = Some(management.config.clone());
    let mut controller = AwsRemoteStackManagementController::mock_ready("mgmt-test-management");
    controller.state = AwsRemoteStackManagementState::UpdateStart;
    management
        .set_last_failed_controller(Some(Box::new(controller.clone())))
        .unwrap();
    crate::core::ResourceController::transition_to_failure(&mut controller);
    management
        .set_internal_controller(Some(Box::new(controller)))
        .unwrap();
    management.status = ResourceStatus::UpdateFailed;
    management.retry_attempt = 10;
}

/// A direct setup run still updates management for the same change, as it did.
#[test]
fn a_direct_setup_run_still_updates_managements_new_network_edge() {
    let (stack, state) = management_gaining_the_network_edge();
    let plan = executor_for_run(&stack, InitialSetupAuthority::DirectSetup, true, false)
        .plan(&state)
        .unwrap();
    assert!(plan.dependency_records.is_empty());
    // Management is not recorded as an edge. The network, which a setup run also updates,
    // goes first, and management follows it: its new edge waits for the network's update.
    assert!(!plan.updates.is_empty(), "a setup run still updates");
}

/// The record the earlier build left stuck: resumed by a retry outside a setup run, the update
/// controller completes without a cloud call and keeps what setup recorded, and a Live
/// resource that waited behind it is resumed in the same pass.
#[tokio::test]
async fn a_failed_management_update_recovers_on_retry_without_a_cloud_call() {
    let (mut stack, mut state) = management_gaining_the_network_edge();
    management_failed_at_update_start(&mut state);
    // The Live resource that waited behind management and timed out.
    let uploads = Storage::new("uploads".to_string()).build();
    let management_ref = ResourceRef::new(RemoteStackManagement::RESOURCE_TYPE, "management");
    let mut entry = ResourceEntry::new(uploads.clone(), ResourceLifecycle::Live);
    entry.dependencies = vec![management_ref.clone()];
    stack.resources.insert("uploads".to_string(), entry);
    let mut dependent = installed(
        Resource::new(uploads),
        ResourceLifecycle::Live,
        vec![management_ref],
        Box::new(AwsStorageController::mock_ready("mgmt-test-uploads")),
    );
    dependent
        .set_last_failed_controller(Some(Box::new(AwsStorageController::mock_ready(
            "mgmt-test-uploads",
        ))))
        .unwrap();
    dependent.status = ResourceStatus::UpdateFailed;
    dependent.retry_attempt = 10;
    state.resources.insert("uploads".to_string(), dependent);
    let before = state.resources["management"].clone();

    let executor = executor_for_run(&stack, InitialSetupAuthority::DirectSetup, false, true);
    let mut current = state;
    for _ in 0..6 {
        current = executor.step(current).await.unwrap().next_state;
        if executor.is_synced(&current) {
            break;
        }
    }
    for id in ["management", "uploads", "default-network"] {
        let resource = &current.resources[id];
        assert_eq!(
            resource.status,
            ResourceStatus::Running,
            "{id}: {:?}",
            resource.error
        );
    }
    let after = &current.resources["management"];
    assert_eq!(after.config, before.config);
    assert_eq!(after.dependencies, network_edge());
    assert!(executor.is_synced(&current));
}

/// An update of management scheduled by propagation (not by a change of its own) outside a
/// setup run: it completes with no cloud call, keeps what setup recorded, and the Live
/// resource behind it proceeds.
#[tokio::test]
async fn a_propagated_management_update_outside_setup_completes_and_unblocks_dependents() {
    let (mut stack, mut state) = management_gaining_the_network_edge();
    state.resources.get_mut("management").unwrap().dependencies = network_edge();
    state
        .resources
        .get_mut("management")
        .unwrap()
        .dependency_update_pending = true;
    let uploads = Storage::new("uploads".to_string()).build();
    let management_ref = ResourceRef::new(RemoteStackManagement::RESOURCE_TYPE, "management");
    let mut entry = ResourceEntry::new(uploads.clone(), ResourceLifecycle::Live);
    entry.dependencies = vec![management_ref.clone()];
    stack.resources.insert("uploads".to_string(), entry);
    state.resources.insert(
        "uploads".to_string(),
        installed(
            Resource::new(uploads),
            ResourceLifecycle::Live,
            vec![management_ref],
            Box::new(AwsStorageController::mock_ready("mgmt-test-uploads")),
        ),
    );
    let before = state.resources["management"].clone();
    let executor = executor_for_run(&stack, InitialSetupAuthority::DirectSetup, false, false);
    let first = executor.plan(&state).unwrap();
    assert!(
        first.updates.contains_key("management"),
        "propagation schedules management: {:?}",
        first.updates.keys()
    );
    // Step until management's update is done; the storage behind it waits for that, so no
    // storage controller (which would need a cloud client) has run yet.
    let mut current = state;
    for _ in 0..8 {
        current = executor.step(current).await.unwrap().next_state;
        let management = &current.resources["management"];
        if management.status == ResourceStatus::Running && !management.dependency_update_pending {
            break;
        }
    }
    let management = &current.resources["management"];
    assert_eq!(
        management.status,
        ResourceStatus::Running,
        "{:?}",
        management.error
    );
    assert_eq!(management.config, before.config);
    assert_eq!(management.internal_state, before.internal_state);
    assert_eq!(current.resources["uploads"].status, ResourceStatus::Running);
    // Behind management, the Live resource is now planned to proceed: nothing holds it.
    let next = executor.plan(&current).unwrap();
    assert!(
        next.updates.contains_key("uploads"),
        "the dependent proceeds: {:?}",
        next.updates.keys()
    );
}

/// The old state, an ordinary run: the planner schedules management's update (the new edge),
/// it completes with no cloud call, the edge is recorded, the recorded controller state is
/// untouched and the stack is synced.
#[tokio::test]
async fn an_ordinary_run_updates_management_without_a_cloud_call_and_records_the_edge() {
    let (stack, state) = management_gaining_the_network_edge();
    let executor = executor_for_run(&stack, InitialSetupAuthority::DirectSetup, false, false);
    let plan = executor.plan(&state).unwrap();
    assert!(
        plan.updates.contains_key("management"),
        "the update is scheduled: {:?}",
        plan.updates.keys()
    );
    let before = state.resources["management"].clone();
    let mut current = state;
    for _ in 0..8 {
        current = executor.step(current).await.unwrap().next_state;
        if executor.is_synced(&current) {
            break;
        }
    }
    let after = &current.resources["management"];
    assert_eq!(after.status, ResourceStatus::Running, "{:?}", after.error);
    assert_eq!(after.dependencies, network_edge());
    assert_eq!(after.internal_state, before.internal_state);
    assert!(executor.is_synced(&current));
}
