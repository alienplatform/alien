//! A release that only changes a resource's stack dependencies.
//!
//! Ordering edges belong to the executor: it orders creates and deletes by them, and no controller
//! reads them. When setup owns a resource and only its edges changed, the executor records them
//! and does not run the controller, which under template setup may not write the resource.

use super::helpers::default_deployment_config;
use crate::{
    artifact_registry::AwsArtifactRegistryController,
    core::{MockPlatformServiceProvider, StackExecutor, StackResourceStateExt},
    kv::AwsKvController,
    service_account::AwsServiceAccountController,
    storage::AwsStorageController,
};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt};
use alien_core::{
    ArtifactRegistry, ClientConfig, InitialSetupAuthority, Kv, Platform, Resource, ResourceEntry,
    ResourceLifecycle, ResourceRef, ResourceStatus, ServiceAccount, Stack, StackResourceState,
    StackState, Storage,
};
use std::sync::Arc;

/// A Running, imported resource as an earlier release recorded it.
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

fn entry(resource: Resource, dependencies: Vec<ResourceRef>) -> ResourceEntry {
    let mut entry = ResourceEntry::new(resource, ResourceLifecycle::Frozen);
    entry.dependencies = dependencies;
    entry
}

fn cache_edge() -> Vec<ResourceRef> {
    vec![ResourceRef::new(Kv::RESOURCE_TYPE, "cache")]
}

/// A stack whose setup-owned resources gain an edge to `cache`, and the state of an install
/// that recorded none. `cache` itself is unchanged.
fn release_adding_an_edge(
    subjects: Vec<(Resource, Box<dyn crate::core::ResourceController>)>,
) -> (Stack, StackState) {
    let cache = Kv::new("cache".to_string()).build();
    let mut stack = Stack::new("edges".to_string())
        .add(cache.clone(), ResourceLifecycle::Frozen)
        .build();
    let mut state = StackState::new(Platform::Aws);
    state.resource_prefix = "edges-test".to_string();
    state.resources.insert(
        "cache".to_string(),
        installed(
            Resource::new(cache),
            ResourceLifecycle::Frozen,
            vec![],
            Box::new(AwsKvController::mock_ready("edges-test-cache")),
        ),
    );
    for (resource, controller) in subjects {
        let id = resource.id().to_string();
        stack
            .resources
            .insert(id.clone(), entry(resource.clone(), cache_edge()));
        state.resources.insert(
            id,
            installed(resource, ResourceLifecycle::Frozen, vec![], controller),
        );
    }
    (stack, state)
}

/// The provider has no expectations: a controller that asks for any cloud client fails the test.
fn executor(stack: &Stack, authority: InitialSetupAuthority) -> StackExecutor {
    StackExecutor::builder(stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
        .deployment_config(&default_deployment_config())
        .initial_setup_authority(authority)
        .step_running_resources(false)
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .build()
        .unwrap()
}

fn setup_owned_subjects() -> Vec<(Resource, Box<dyn crate::core::ResourceController>)> {
    vec![
        (
            Resource::new(ServiceAccount::new("execution-sa".to_string()).build()),
            Box::new(AwsServiceAccountController::mock_ready("edges-test-execution-sa")),
        ),
        (
            Resource::new(Storage::new("objects".to_string()).build()),
            Box::new(AwsStorageController::mock_ready("edges-test-objects")),
        ),
    ]
}

/// Under template setup, a service account and a Frozen bucket that only gain an edge have it
/// recorded in one step. Neither controller runs: status, config, previous config and the
/// controller's saved state are exactly what the install left.
#[tokio::test]
async fn template_setup_records_new_edges_of_setup_owned_resources_without_their_controllers() {
    let (stack, state) = release_adding_an_edge(setup_owned_subjects());
    let executor = executor(&stack, InitialSetupAuthority::ImportedHandoff);
    assert!(
        !executor.is_synced(&state),
        "an unrecorded edge is outstanding work"
    );

    let plan = executor.plan(&state).unwrap();
    assert!(plan.updates.is_empty(), "{:?}", plan.updates.keys());
    let mut recorded = plan.dependency_records.clone();
    recorded.sort();
    assert_eq!(recorded, ["execution-sa", "objects"]);

    let next = executor.step(state.clone()).await.unwrap().next_state;
    for id in ["execution-sa", "objects"] {
        let (before, after) = (&state.resources[id], &next.resources[id]);
        assert_eq!(after.dependencies, cache_edge(), "{id}");
        assert_eq!(after.status, ResourceStatus::Running, "{id}: {:?}", after.error);
        assert_eq!(after.config, before.config, "{id}");
        assert_eq!(after.previous_config, before.previous_config, "{id}");
        assert_eq!(after.internal_state, before.internal_state, "{id}");
        assert!(after.error.is_none(), "{id}: {:?}", after.error);
    }
    assert!(executor.is_synced(&next));
    let replanned = executor.plan(&next).unwrap();
    assert!(replanned.updates.is_empty() && replanned.dependency_records.is_empty());
}

/// Setup itself reconciles what it owns, so under direct setup an edge-only change still updates.
#[test]
fn direct_setup_still_updates_on_an_edge_only_change() {
    let (stack, state) = release_adding_an_edge(setup_owned_subjects());
    let plan = executor(&stack, InitialSetupAuthority::DirectSetup)
        .plan(&state)
        .unwrap();
    assert!(plan.dependency_records.is_empty());
    let mut updated = plan.updates.keys().cloned().collect::<Vec<_>>();
    updated.sort();
    assert_eq!(updated, ["execution-sa", "objects"]);
}

fn registry_gaining_an_edge() -> (Stack, StackState) {
    release_adding_an_edge(vec![(
        Resource::new(ArtifactRegistry::new("images".to_string()).build()),
        Box::new(AwsArtifactRegistryController::mock_ready(
            "123456789012",
            "us-east-1",
        )),
    )])
}

/// A Frozen registry has management operations, so runtime may update it. An edge to a resource
/// that is already there and unchanged is still only an edge: it is recorded, and the registry's
/// setup-created roles are not rewritten.
#[tokio::test]
async fn a_managed_frozen_resource_records_an_edge_to_an_existing_dependency() {
    let (stack, state) = registry_gaining_an_edge();
    let executor = executor(&stack, InitialSetupAuthority::ImportedHandoff);
    let plan = executor.plan(&state).unwrap();
    assert!(plan.updates.is_empty(), "{:?}", plan.updates.keys());
    assert_eq!(plan.dependency_records, ["images"]);

    let next = executor.step(state.clone()).await.unwrap().next_state;
    let (before, after) = (&state.resources["images"], &next.resources["images"]);
    assert_eq!(after.dependencies, cache_edge());
    assert_eq!(after.status, ResourceStatus::Running, "{:?}", after.error);
    assert_eq!(after.internal_state, before.internal_state);
    assert!(executor.is_synced(&next));
}

/// When the new edge reaches a dependency that is not settled, the registry may need to read it
/// once it is, so the change waits for an update as before and nothing is recorded early.
#[test]
fn a_managed_frozen_resource_waits_to_update_for_an_unsettled_new_dependency() {
    let (stack, mut state) = registry_gaining_an_edge();
    state.resources.get_mut("cache").unwrap().status = ResourceStatus::Updating;
    let executor = executor(&stack, InitialSetupAuthority::ImportedHandoff);
    let plan = executor.plan(&state).unwrap();
    assert!(plan.dependency_records.is_empty());
    assert!(plan.updates.is_empty(), "deferred until cache settles");
    assert!(!executor.is_synced(&state));

    let (stack, mut state) = registry_gaining_an_edge();
    state.resources.remove("cache");
    let plan = executor(&stack, InitialSetupAuthority::ImportedHandoff)
        .plan(&state)
        .unwrap();
    assert_eq!(plan.creates, ["cache"]);
    assert!(plan.dependency_records.is_empty());
}

/// A dependency that changes in the same plan is a reason to update, not just a new edge.
#[test]
fn a_dependency_updated_in_the_same_plan_still_updates_a_managed_frozen_resource() {
    let (mut stack, mut state) = registry_gaining_an_edge();
    // A Live bucket the registry already depends on gets a real config change.
    let uploads = Storage::new("uploads".to_string()).build();
    state.resources.insert(
        "uploads".to_string(),
        installed(
            Resource::new(uploads),
            ResourceLifecycle::Live,
            vec![],
            Box::new(AwsStorageController::mock_ready("edges-test-uploads")),
        ),
    );
    stack.resources.insert(
        "uploads".to_string(),
        ResourceEntry::new(
            Resource::new(Storage::new("uploads".to_string()).versioning(true).build()),
            ResourceLifecycle::Live,
        ),
    );
    let uploads_edge = ResourceRef::new(Storage::RESOURCE_TYPE, "uploads");
    state.resources.get_mut("images").unwrap().dependencies = vec![uploads_edge.clone()];
    stack.resources.get_mut("images").unwrap().dependencies =
        [cache_edge(), vec![uploads_edge]].concat();

    let plan = executor(&stack, InitialSetupAuthority::ImportedHandoff)
        .plan(&state)
        .unwrap();
    assert!(plan.updates.contains_key("uploads"), "{:?}", plan.updates.keys());
    assert!(plan.updates.contains_key("images"), "{:?}", plan.updates.keys());
    assert!(plan.dependency_records.is_empty());
}

/// A config change that comes with new edges is a real update, even for a setup-owned resource.
#[test]
fn a_config_change_with_new_edges_is_still_an_update() {
    let (mut stack, state) = release_adding_an_edge(setup_owned_subjects());
    let changed = Storage::new("objects".to_string()).versioning(true).build();
    stack.resources.insert(
        "objects".to_string(),
        entry(Resource::new(changed), cache_edge()),
    );
    let plan = executor(&stack, InitialSetupAuthority::ImportedHandoff)
        .plan(&state)
        .unwrap();
    assert_eq!(plan.dependency_records, ["execution-sa"]);
    assert_eq!(plan.updates.keys().collect::<Vec<_>>(), ["objects"]);
}

/// Deletion ordering reads recorded edges. When a release drops a setup-owned resource's edge
/// along with the resource it pointed to, recording the removal is what lets that delete start.
#[tokio::test]
async fn recording_a_removed_edge_releases_the_old_dependency_for_deletion() {
    let (stack, mut state) = release_adding_an_edge(setup_owned_subjects());
    // The install recorded the edges; the next release drops `cache` and the edges with it.
    for id in ["execution-sa", "objects"] {
        state.resources.get_mut(id).unwrap().dependencies = cache_edge();
    }
    let mut stack = stack;
    stack.resources.shift_remove("cache");
    for id in ["execution-sa", "objects"] {
        stack.resources.get_mut(id).unwrap().dependencies = vec![];
    }
    let mut cache = state.resources["cache"].clone();
    cache.lifecycle = Some(ResourceLifecycle::Live);
    state.resources.insert("cache".to_string(), cache);

    let executor = executor(&stack, InitialSetupAuthority::ImportedHandoff);
    let plan = executor.plan(&state).unwrap();
    assert!(
        plan.deletes.is_empty(),
        "cache is still recorded as a dependency"
    );
    let mut recorded = plan.dependency_records.clone();
    recorded.sort();
    assert_eq!(recorded, ["execution-sa", "objects"]);

    // This step only records: the deletes were planned before it, and there were none.
    let next = executor.step(state).await.unwrap().next_state;
    for id in ["execution-sa", "objects"] {
        assert!(next.resources[id].dependencies.is_empty(), "{id}");
    }
    assert_eq!(
        executor.plan(&next).unwrap().deletes,
        ["cache"],
        "with the removal recorded, nothing holds cache"
    );
}

/// A setup-owned resource already exists, so its new edge waits for nothing: it is recorded even
/// while the dependency it points to is still updating.
#[test]
fn a_new_edge_is_recorded_while_its_dependency_is_still_settling() {
    let (stack, mut state) = release_adding_an_edge(setup_owned_subjects());
    state.resources.get_mut("cache").unwrap().status = ResourceStatus::Updating;
    let plan = executor(&stack, InitialSetupAuthority::ImportedHandoff)
        .plan(&state)
        .unwrap();
    assert!(plan.updates.is_empty(), "{:?}", plan.updates.keys());
    let mut recorded = plan.dependency_records.clone();
    recorded.sort();
    assert_eq!(recorded, ["execution-sa", "objects"]);
}
