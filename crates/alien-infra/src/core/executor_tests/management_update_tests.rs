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
    executor_with_provider(
        stack,
        authority,
        setup_run,
        resume_failed,
        MockPlatformServiceProvider::new(),
    )
}

fn executor_with_provider(
    stack: &Stack,
    authority: InitialSetupAuthority,
    setup_run: bool,
    resume_failed: bool,
    provider: MockPlatformServiceProvider,
) -> StackExecutor {
    let mut config = default_deployment_config();
    config.setup_run = setup_run;
    StackExecutor::builder(stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
        .deployment_config(&config)
        .initial_setup_authority(authority)
        .step_running_resources(false)
        .resume_unchanged_failed_resources(resume_failed)
        .service_provider(Arc::new(provider))
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

/// The stack of an old automatic-setup install after the new release: management, the network
/// it now waits for, a Frozen service account, bucket and vault behind management, and a Live
/// resource behind them. Every Frozen type here is one setup owns.
fn old_automatic_setup_install() -> (Stack, StackState, ResourceRef) {
    use crate::service_account::AwsServiceAccountController;
    let (mut stack, mut state) = management_gaining_the_network_edge();
    let management_ref = ResourceRef::new(RemoteStackManagement::RESOURCE_TYPE, "management");
    let sa = alien_core::ServiceAccount::new("execution-sa".to_string()).build();
    let objects = Storage::new("objects".to_string()).build();
    let uploads = Storage::new("uploads".to_string()).build();
    let vault = alien_core::Vault::new("vault".to_string()).build();
    for (resource, lifecycle) in [
        (Resource::new(sa), ResourceLifecycle::Frozen),
        (Resource::new(objects), ResourceLifecycle::Frozen),
        (Resource::new(vault), ResourceLifecycle::Frozen),
        (Resource::new(uploads), ResourceLifecycle::Live),
    ] {
        let id = resource.id().to_string();
        let mut entry =
            ResourceEntry::new(Storage::new("placeholder".to_string()).build(), lifecycle);
        entry.config = resource.clone();
        entry.dependencies = vec![management_ref.clone()];
        stack.resources.insert(id.clone(), entry);
        let controller: Box<dyn crate::core::ResourceController> = match id.as_str() {
            "execution-sa" => Box::new(AwsServiceAccountController::mock_ready("mgmt-test-sa")),
            "objects" => Box::new(AwsStorageController::mock_ready("mgmt-test-objects")),
            "vault" => Box::new(crate::vault::AwsVaultController {
                state: crate::vault::AwsVaultState::Ready,
                account_id: Some("123456789012".to_string()),
                region: Some("us-east-1".to_string()),
                vault_prefix: Some("mgmt-test-vault".to_string()),
                ..Default::default()
            }),
            _ => Box::new(AwsStorageController::mock_ready("mgmt-test-uploads")),
        };
        // The earlier release recorded these resources without the edge to management.
        state
            .resources
            .insert(id, installed(resource, lifecycle, vec![], controller));
    }
    (stack, state, management_ref)
}

/// The install without the resources whose own drift updates (storage permissions, vault
/// grants) would call the cloud: management, its network, the service account and the Live
/// bucket.
fn small_install() -> (Stack, StackState, ResourceRef) {
    let (mut stack, mut state, management_ref) = old_automatic_setup_install();
    for id in ["objects", "vault"] {
        stack.resources.remove(id);
        state.resources.remove(id);
    }
    (stack, state, management_ref)
}

/// A provider that allows only the Live bucket's own policy write.
fn live_bucket_provider() -> MockPlatformServiceProvider {
    let mut provider = MockPlatformServiceProvider::new();
    let mut s3 = alien_aws_clients::s3::MockS3Api::new();
    s3.expect_put_bucket_policy().returning(|bucket, _| {
        assert!(
            bucket.ends_with("-uploads"),
            "only the Live bucket: {bucket}"
        );
        Ok(())
    });
    let s3 = Arc::new(s3);
    provider
        .expect_get_aws_s3_client()
        .returning(move |_| Ok(s3.clone()));
    // The Live bucket's own role policies are listed to find stale grants (read only).
    let mut iam = alien_aws_clients::iam::MockIamApi::new();
    iam.expect_list_role_policies().returning(|_| {
        Ok(alien_aws_clients::iam::ListRolePoliciesResponse {
            list_role_policies_result: alien_aws_clients::iam::ListRolePoliciesResult {
                policy_names: None,
                is_truncated: Some(false),
                marker: None,
            },
        })
    });
    let iam = Arc::new(iam);
    provider
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(iam.clone()));
    provider
}

fn plan_json(executor: &StackExecutor, state: &StackState) -> serde_json::Value {
    serde_json::to_value(executor.plan(state).unwrap()).unwrap()
}

/// An ordinary run under direct setup records the edges that only order work (management, the
/// service account) and plans no update for them.
#[test]
fn an_ordinary_run_under_direct_setup_records_ordering_edges_and_updates_nothing_for_them() {
    let (stack, state, _) = old_automatic_setup_install();
    let ordinary = plan_json(
        &executor_for_run(&stack, InitialSetupAuthority::DirectSetup, false, false),
        &state,
    );
    let recorded: Vec<_> = ordinary["dependencyRecords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_string())
        .collect();
    for id in ["management", "execution-sa"] {
        assert!(
            recorded.contains(&id.to_string()),
            "{id} recorded: {ordinary}"
        );
        assert!(
            ordinary["updates"].get(id).is_none(),
            "{id} not updated: {ordinary}"
        );
    }
    assert!(ordinary["dependencyUpdatePending"]
        .as_array()
        .unwrap()
        .is_empty());
}

/// The same state, a setup run: management and what follows it update, as they did.
#[test]
fn a_setup_run_still_updates_management_and_writes() {
    let (stack, state, _) = old_automatic_setup_install();
    let setup = plan_json(
        &executor_for_run(&stack, InitialSetupAuthority::DirectSetup, true, false),
        &state,
    );
    assert!(
        !setup["updates"].as_object().unwrap().is_empty(),
        "a setup run updates: {setup}"
    );
}

/// An ordinary run records management's new edge and updates nothing: no controller runs,
/// the recorded controller state is untouched, and the stack is synced.
#[tokio::test]
async fn an_ordinary_run_records_managements_edge_without_running_its_controller() {
    let (stack, state) = management_gaining_the_network_edge();
    let executor = executor_for_run(&stack, InitialSetupAuthority::DirectSetup, false, false);
    let plan = executor.plan(&state).unwrap();
    assert!(plan.updates.is_empty(), "{:?}", plan.updates.keys());
    assert!(plan.dependency_records.contains(&"management".to_string()));
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

/// The stuck state the failed build left: management failed at its first update state and the
/// marker is set on its dependents. Once management resumes, its dependents are planned for
/// their ordinary equal-config updates exactly as before: the plan lists them.
#[test]
fn the_stuck_state_plans_its_dependents_for_their_ordinary_updates_after_the_retry() {
    use crate::core::StackResourceStateExt;
    let (stack, mut state, management_ref) = old_automatic_setup_install();
    management_failed_at_update_start(&mut state);
    for id in ["execution-sa", "objects", "vault", "uploads"] {
        let record = state.resources.get_mut(id).unwrap();
        record.dependencies = vec![management_ref.clone()];
        record.dependency_update_pending = true;
    }
    assert!(state
        .resources
        .get_mut("management")
        .unwrap()
        .retry_failed()
        .unwrap());
    let executor = executor_for_run(&stack, InitialSetupAuthority::DirectSetup, false, true);
    let plan = executor.plan(&state).unwrap();
    let mut updates: Vec<_> = plan.updates.keys().cloned().collect();
    updates.sort();
    println!("planned for the stuck state after the retry: {updates:?}");
    // Management is already updating (resumed). Every dependent that carries the marker is
    // planned or deferred behind it, none is skipped as setup's.
    let mut owed: Vec<_> = plan.updates.keys().cloned().collect();
    owed.extend(plan.dependency_update_pending.iter().cloned());
    for id in ["execution-sa", "objects", "vault", "uploads"] {
        assert!(
            owed.contains(&id.to_string()),
            "{id} owes its update: {updates:?}"
        );
    }
}

/// The stuck record with a Live resource waiting behind it, resumed by a retry request: the
/// record settles and the Live resource proceeds in the same pass.
#[tokio::test]
async fn a_failed_management_update_recovers_on_retry_and_unblocks_the_live_resource() {
    let (mut stack, mut state, _) = small_install();
    stack.resources.remove("execution-sa");
    state.resources.remove("execution-sa");
    let settled = state.resources["management"].internal_state.clone();
    management_failed_at_update_start(&mut state);
    let uploads = state.resources.get_mut("uploads").unwrap();
    uploads
        .set_last_failed_controller(Some(Box::new(AwsStorageController::mock_ready(
            "mgmt-test-uploads",
        ))))
        .unwrap();
    uploads.status = ResourceStatus::UpdateFailed;
    uploads.retry_attempt = 10;
    let provider = live_bucket_provider();
    let executor = executor_with_provider(
        &stack,
        InitialSetupAuthority::DirectSetup,
        false,
        true,
        provider,
    );
    let mut current = state;
    for _ in 0..12 {
        current = executor.step(current).await.unwrap().next_state;
        if executor.is_synced(&current) {
            break;
        }
    }
    for (id, resource) in &current.resources {
        assert_eq!(
            resource.status,
            ResourceStatus::Running,
            "{id}: {:?}",
            resource.error
        );
    }
    assert_eq!(current.resources["management"].internal_state, settled);
    assert!(executor.is_synced(&current));
}

/// Test (iv): plan a stored stack state. Point `ALIEN_STACK_STATE_FIXTURE` at a JSON file of a
/// `StackState`; the desired stack is the recorded resources plus management's edge to the
/// network. The files hold account data, so none is committed.
#[test]
#[ignore = "needs ALIEN_STACK_STATE_FIXTURE"]
fn a_stored_state_plans_as_an_ordinary_run_under_direct_setup() {
    let path = std::env::var("ALIEN_STACK_STATE_FIXTURE").expect("ALIEN_STACK_STATE_FIXTURE");
    let mut state: StackState =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    // The controllers of the compute cluster and the daemon are not in this repository, so
    // an executor cannot be built for them. Each is replaced by a resource of this repository
    // with the same lifecycle and the same need for management permissions (a registry for
    // the cluster, a bucket for the daemon), same id and edges, and no saved controller:
    // only the plan for its id is meaningful.
    for record in state.resources.values_mut() {
        let id = record.config.id().to_string();
        let stand_in = match record.resource_type.as_str() {
            "compute-cluster" => Some(Resource::new(alien_core::ArtifactRegistry::new(id).build())),
            "daemon" => Some(Resource::new(Storage::new(id).build())),
            _ => None,
        };
        if let Some(stand_in) = stand_in {
            record.resource_type = stand_in.resource_type().to_string();
            record.config = stand_in;
            record.internal_state = None;
        }
    }
    let mut stack = Stack::new("stored".to_string()).build();
    let network_id = state
        .resources
        .iter()
        .find(|(_, r)| r.resource_type == "network")
        .map(|(id, _)| id.clone());
    for (id, record) in &state.resources {
        let lifecycle = record.lifecycle.unwrap_or(ResourceLifecycle::Frozen);
        let mut entry =
            ResourceEntry::new(Storage::new("placeholder".to_string()).build(), lifecycle);
        entry.config = record.config.clone();
        entry.dependencies = record.dependencies.clone();
        if record.resource_type == "remote-stack-management" {
            if let Some(network) = &network_id {
                entry.dependencies = vec![ResourceRef::new(Network::RESOURCE_TYPE, network)];
            }
        }
        stack.resources.insert(id.clone(), entry);
    }
    let executor = executor_for_run(&stack, InitialSetupAuthority::DirectSetup, false, false);
    let plan = executor.plan(&state).unwrap();
    let summary = |plan: &crate::core::PlanResult| {
        serde_json::json!({
            "updates": plan.updates.keys().collect::<Vec<_>>(),
            "dependencyRecords": &plan.dependency_records,
            "dependencyUpdatePending": &plan.dependency_update_pending,
            "setupRequired": &plan.setup_required,
        })
    };
    println!("with markers: {}", summary(&plan));
    let markers: Vec<_> = state
        .resources
        .iter()
        .filter(|(_, r)| r.dependency_update_pending)
        .map(|(id, _)| id.clone())
        .collect();
    println!("markers on: {markers:?}");
    assert!(!plan.updates.contains_key("management"));
}
