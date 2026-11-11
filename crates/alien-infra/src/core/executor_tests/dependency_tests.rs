//! Tests for dependency ordering during creation and updates.

use std::time::Duration;

use super::helpers::*;
#[cfg(feature = "aws")]
use crate::core::StackExecutor;
use crate::error::Result;
#[cfg(feature = "aws")]
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt};
#[cfg(feature = "aws")]
use alien_core::{
    ClientConfig, InitialSetupAuthority, Key, Platform, ServiceAccount, StackResourceState,
    StackState, Vault,
};
use alien_core::{
    Resource, ResourceLifecycle, ResourceRef, ResourceStatus, Stack, Storage, Worker,
};

/// Tests that a function waits for its storage dependency to be Running.
#[tokio::test]
async fn test_function_waits_for_storage_dependency() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("dep-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func1.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["store1", "func1"]);
    Ok(())
}

/// Tests transitive dependencies: A → B → C
#[tokio::test]
async fn test_transitive_dependencies() -> Result<()> {
    let store_a = test_storage("store-a");
    let func_b = test_function("func-b");
    let func_c = test_function("func-c");

    let stack = Stack::new("transitive-test".to_owned())
        .add(store_a.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func_b.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store-a")],
        )
        .add_with_dependencies(
            func_c.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Worker::RESOURCE_TYPE, "func-b")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["store-a", "func-b", "func-c"]);
    Ok(())
}

/// Tests that circular dependencies are detected at executor creation time.
#[tokio::test]
async fn test_circular_dependency_detection() {
    let func_a = test_function("func-a");
    let func_b = test_function("func-b");

    let stack = Stack::new("circular-test".to_owned())
        .add_with_dependencies(
            func_a,
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Worker::RESOURCE_TYPE, "func-b")],
        )
        .add_with_dependencies(
            func_b,
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Worker::RESOURCE_TYPE, "func-a")],
        )
        .build();

    let result = new_executor(&stack);

    assert!(result.is_err(), "Circular dependency should be detected");
}

/// Tests diamond dependency pattern: D depends on both B and C, which both depend on A.
#[tokio::test]
async fn test_diamond_dependencies() -> Result<()> {
    let store_a = test_storage("store-a");
    let func_b = test_function("func-b");
    let func_c = test_function("func-c");
    let func_d = test_function("func-d");

    let stack = Stack::new("diamond-test".to_owned())
        .add(store_a.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func_b.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store-a")],
        )
        .add_with_dependencies(
            func_c.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store-a")],
        )
        .add_with_dependencies(
            func_d.clone(),
            ResourceLifecycle::Live,
            vec![
                ResourceRef::new(Worker::RESOURCE_TYPE, "func-b"),
                ResourceRef::new(Worker::RESOURCE_TYPE, "func-c"),
            ],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["store-a", "func-b", "func-c", "func-d"]);
    Ok(())
}

/// Tests independent branches in dependency tree run in parallel.
#[tokio::test]
async fn test_independent_branches_run_in_parallel() -> Result<()> {
    let store_a = test_storage("store-a");
    let store_c = test_storage("store-c");
    let func_b = test_function("func-b");
    let func_d = test_function("func-d");

    let stack = Stack::new("parallel-branches-test".to_owned())
        .add(store_a.clone(), ResourceLifecycle::Frozen)
        .add(store_c.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func_b.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store-a")],
        )
        .add_with_dependencies(
            func_d.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store-c")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["store-a", "store-c", "func-b", "func-d"]);
    Ok(())
}

/// Tests a mix of independent and dependent resources.
/// - An independent Worker.
/// - A Storage resource.
/// - A Worker dependent on the Storage resource.
#[tokio::test]
async fn test_mixed_dependencies() -> Result<()> {
    let store1 = test_storage("store1");
    let func_indep = test_function("func-indep");
    let func_dep = test_function("func-dep");

    let stack = Stack::new("mixed-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add(func_indep.clone(), ResourceLifecycle::Live)
        .add_with_dependencies(
            func_dep.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["store1", "func-indep", "func-dep"]);
    Ok(())
}

/// Tests Azure infrastructure dependency ordering.
/// Resource group → Storage account → User storage.
#[tokio::test]
async fn test_infrastructure_dependency_ordering() -> Result<()> {
    // Simulates Azure infrastructure pattern
    let resource_group = test_storage("resource-group");
    let storage_account = test_storage("storage-account");
    let user_storage = test_storage("user-storage");

    let stack = build_stack_with_deps(
        "infra-test",
        vec![
            (
                Resource::new(resource_group.clone()),
                ResourceLifecycle::Frozen,
                vec![],
            ),
            (
                Resource::new(storage_account.clone()),
                ResourceLifecycle::Frozen,
                vec![ResourceRef::new(Storage::RESOURCE_TYPE, "resource-group")],
            ),
            (
                Resource::new(user_storage.clone()),
                ResourceLifecycle::Frozen,
                vec![
                    ResourceRef::new(Storage::RESOURCE_TYPE, "storage-account"),
                    ResourceRef::new(Storage::RESOURCE_TYPE, "resource-group"),
                ],
            ),
        ],
    );

    let executor = new_executor(&stack)?;
    let mut state = new_test_state();

    // Step through manually to verify ordering
    let step1 = executor.step(state).await?;
    state = step1.next_state;

    // Only resource-group should start (no deps)
    assert_eq!(
        get_status(&state, "resource-group"),
        Some(ResourceStatus::Provisioning),
        "Resource group should be Provisioning"
    );
    assert_eq!(
        get_status(&state, "storage-account"),
        Some(ResourceStatus::Pending),
        "Storage account should be Pending"
    );
    assert_eq!(
        get_status(&state, "user-storage"),
        Some(ResourceStatus::Pending),
        "User storage should be Pending"
    );

    // Run to completion
    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(
        &final_state,
        &["resource-group", "storage-account", "user-storage"],
    );
    Ok(())
}

/// Tests that function updates when storage dependency config changes.
#[tokio::test]
async fn test_function_updates_when_dependency_changes() -> Result<()> {
    let storage_v1 = test_storage_with_public_read("storage", false);
    let func = test_function_linked("func", &storage_v1);

    let stack_v1 = Stack::new("dep-update-test".to_owned())
        .add(storage_v1.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "storage")],
        )
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_v1 = run_to_synced(&executor_v1, state).await?;

    assert_all_running(&state_v1, &["storage", "func"]);

    // Update storage config
    let storage_v2 = test_storage_with_public_read("storage", true);
    let func_v2 = test_function_linked("func", &storage_v2);

    let stack_v2 = Stack::new("dep-update-test".to_owned())
        .add(storage_v2.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func_v2.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "storage")],
        )
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let plan = executor_v2.plan(&state_v1)?;

    // Storage should be marked for update
    assert!(
        plan.updates.contains_key("storage"),
        "Storage should be marked for update"
    );

    // Run to completion
    let final_state = run_to_synced(&executor_v2, state_v1).await?;
    assert_all_running(&final_state, &["storage", "func"]);

    Ok(())
}

/// Tests resource rename scenario: old resource deleted, new created.
#[tokio::test]
async fn test_resource_rename_and_reapply() -> Result<()> {
    let original_storage = test_storage("original-storage");

    let stack_v1 = Stack::new("rename-test".to_owned())
        .add(original_storage.clone(), ResourceLifecycle::Frozen)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_v1 = run_to_synced(&executor_v1, state).await?;

    assert_all_running(&state_v1, &["original-storage"]);

    // Rename storage (remove original, add renamed)
    let renamed_storage = test_storage("renamed-storage");

    let stack_v2 = Stack::new("rename-test".to_owned())
        .add(renamed_storage.clone(), ResourceLifecycle::Frozen)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let plan = executor_v2.plan(&state_v1)?;

    // Original storage should be deleted, renamed created
    assert!(
        plan.creates.contains(&"renamed-storage".to_string()),
        "Should create renamed storage"
    );
    assert!(
        plan.deletes.contains(&"original-storage".to_string()),
        "Should delete original storage"
    );

    // Run to completion
    let final_state = run_to_synced(&executor_v2, state_v1).await?;

    assert_eq!(
        get_status(&final_state, "original-storage"),
        Some(ResourceStatus::Deleted)
    );
    assert_eq!(
        get_status(&final_state, "renamed-storage"),
        Some(ResourceStatus::Running)
    );

    Ok(())
}

/// Tests dependency error when Live depends on missing Frozen.
#[tokio::test]
async fn test_dependency_not_ready_error() -> Result<()> {
    let frozen_store = test_storage("frozen-store");
    let live_func = test_function("live-func");

    let stack = Stack::new("dep-error-test".to_owned())
        .add(frozen_store.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            live_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "frozen-store")],
        )
        .build();

    // Create executor for only Live (skipping Frozen)
    let executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;
    let state = new_test_state();

    // Step should fail - dependency not ready
    let step_result = executor.step(state).await;
    assert!(
        step_result.is_err(),
        "Should fail when dependency is not ready"
    );

    Ok(())
}

/// Dependency propagation must preserve setup ownership after an import while
/// continuing to reconcile Live resources and Frozen resources with management.
#[cfg(feature = "aws")]
#[test]
fn dependency_updates_respect_setup_authority() -> Result<()> {
    for authority in [
        InitialSetupAuthority::ImportedHandoff,
        InitialSetupAuthority::DirectSetup,
    ] {
        let dependency = test_function_with_image("dependency", "new-image");
        let dependents = [
            (
                Resource::new(ServiceAccount::new("identity".to_string()).build()),
                ResourceLifecycle::Frozen,
                false,
            ),
            (
                Resource::new(Vault::new("secrets".to_string()).build()),
                ResourceLifecycle::Frozen,
                false,
            ),
            (
                Resource::new(test_storage("frozen-storage")),
                ResourceLifecycle::Frozen,
                false,
            ),
            (
                Resource::new(test_storage("live-storage")),
                ResourceLifecycle::Live,
                true,
            ),
            (
                Resource::new(Key::new("key".to_string()).build()),
                ResourceLifecycle::Frozen,
                true,
            ),
        ];
        let mut resources = vec![(Resource::new(dependency), ResourceLifecycle::Live, vec![])];
        let mut state = StackState::new(Platform::Aws);
        state.resources.insert(
            "dependency".to_string(),
            create_running_function_state("dependency", "old-image"),
        );
        for (resource, lifecycle, _) in &dependents {
            let dependencies = vec![ResourceRef::new(Worker::RESOURCE_TYPE, "dependency")];
            resources.push((resource.clone(), *lifecycle, dependencies.clone()));
            let mut saved = StackResourceState::new_pending(
                resource.resource_type().to_string(),
                resource.clone(),
                Some(*lifecycle),
                dependencies,
            );
            saved.status = ResourceStatus::Running;
            state.resources.insert(resource.id().to_string(), saved);
        }
        let stack = build_stack_with_deps("dependency-handoff", resources);
        let executor =
            StackExecutor::builder(&stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
                .deployment_config(&default_deployment_config())
                .initial_setup_authority(authority)
                .build()?;
        let plan = executor.plan(&state)?;
        assert!(plan.updates.contains_key("dependency"));
        assert!(plan.creates.is_empty());
        assert!(plan.deletes.is_empty());
        assert!(plan.replaces.is_empty());
        for (resource, _, runtime_management) in &dependents {
            assert_eq!(
                plan.updates.contains_key(resource.id()),
                authority == InitialSetupAuthority::DirectSetup || *runtime_management,
                "dependency update for {} with {authority:?}",
                resource.id(),
            );
        }
    }
    Ok(())
}

/// A stack where `daemon` depends on `cluster`, both Running at v1, then a desired stack that
/// changes both to v2 while `cluster` is still in the middle of its own update (or failed it).
mod update_during_dependency_change {
    use super::*;
    use crate::core::state_utils::StackResourceStateExt;
    use crate::worker::{TestWorkerController, TestWorkerState};
    use alien_core::StackState;

    fn stack(version: &str) -> Stack {
        Stack::new("update-during-dependency-change".to_owned())
            .add(
                test_function_with_image("cluster", &format!("cluster-{version}")),
                ResourceLifecycle::Live,
            )
            .add_with_dependencies(
                test_function_with_image("daemon", &format!("daemon-{version}")),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "cluster")],
            )
            .build()
    }

    /// `cluster` is updating to v2 (or failed that update); `daemon` still runs v1.
    async fn cluster_changing(
        cluster_status: ResourceStatus,
        dependents_may_update: bool,
        updates_during_dependency_change: bool,
    ) -> Result<StackState> {
        let mut state = run_to_synced(&new_executor(&stack("v1"))?, new_test_state()).await?;
        assert_all_running(&state, &["cluster", "daemon"]);

        let cluster = state.resources.get_mut("cluster").expect("cluster state");
        let mut controller = cluster.get_internal_controller_typed::<TestWorkerController>()?;
        controller.dependents_may_update = dependents_may_update;
        controller.state = TestWorkerState::UpdateCodePolling;
        cluster.config = Resource::new(test_function_with_image("cluster", "cluster-v2"));
        cluster.status = ResourceStatus::Updating;
        if cluster_status == ResourceStatus::UpdateFailed {
            let polling = controller.clone();
            controller.state = TestWorkerState::UpdateFailed;
            cluster.status = ResourceStatus::UpdateFailed;
            cluster.set_last_failed_controller(Some(Box::new(polling)))?;
        }
        cluster.set_internal_controller(Some(Box::new(controller)))?;
        assert_eq!(cluster.status, cluster_status);

        let daemon = state.resources.get_mut("daemon").expect("daemon state");
        let mut controller = daemon.get_internal_controller_typed::<TestWorkerController>()?;
        controller.updates_during_dependency_change = updates_during_dependency_change;
        daemon.set_internal_controller(Some(Box::new(controller)))?;
        Ok(state)
    }

    fn daemon_image(state: &StackState) -> String {
        match &state.resources["daemon"]
            .config
            .downcast_ref::<Worker>()
            .expect("daemon is a worker")
            .code
        {
            alien_core::WorkerCode::Image { image } => image.clone(),
            other => panic!("expected an image worker, got {other:?}"),
        }
    }

    /// Only both controllers together open the edge: the dependency says its dependents may
    /// update now, and the dependent says it can update against it. Then the dependent's
    /// update is planned and stepped while the dependency is still Updating. Either side
    /// alone keeps today's order, where the dependent waits for its dependency to be Running.
    #[tokio::test]
    async fn dependent_updates_during_dependency_update_only_when_both_opt_in() -> Result<()> {
        for (dependents_may_update, updates_during_dependency_change) in
            [(true, true), (true, false), (false, true), (false, false)]
        {
            let both = dependents_may_update && updates_during_dependency_change;
            let state = cluster_changing(
                ResourceStatus::Updating,
                dependents_may_update,
                updates_during_dependency_change,
            )
            .await?;
            let executor = new_executor(&stack("v2"))?;

            let plan = executor.plan(&state)?;
            assert_eq!(
                plan.updates.contains_key("daemon"),
                both,
                "dependency opts in: {dependents_may_update}, dependent opts in: \
                 {updates_during_dependency_change}"
            );

            let next = executor.step(state).await?.next_state;
            assert_eq!(get_status(&next, "cluster"), Some(ResourceStatus::Updating));
            if both {
                assert_eq!(get_status(&next, "daemon"), Some(ResourceStatus::Updating));
                assert_eq!(daemon_image(&next), "daemon-v2");
            } else {
                assert_eq!(get_status(&next, "daemon"), Some(ResourceStatus::Running));
                assert_eq!(daemon_image(&next), "daemon-v1");
            }
        }

        // Without both opt-ins the dependent still updates once its dependency is Running.
        let mut state = cluster_changing(ResourceStatus::Updating, true, false).await?;
        let cluster = state.resources.get_mut("cluster").expect("cluster state");
        let mut controller = cluster.get_internal_controller_typed::<TestWorkerController>()?;
        controller.state = TestWorkerState::Ready;
        cluster.status = ResourceStatus::Running;
        cluster.set_internal_controller(Some(Box::new(controller)))?;
        assert!(new_executor(&stack("v2"))?
            .plan(&state)?
            .updates
            .contains_key("daemon"));
        Ok(())
    }

    /// The dependency failed its update and is open; so is a Retry of it. The dependent's
    /// update is planned in both, also when its own earlier attempt failed.
    #[tokio::test]
    async fn dependent_updates_against_a_failed_dependency_and_its_retry() -> Result<()> {
        let mut state = cluster_changing(ResourceStatus::UpdateFailed, true, true).await?;
        let daemon = state.resources.get_mut("daemon").expect("daemon state");
        let mut controller = daemon.get_internal_controller_typed::<TestWorkerController>()?;
        controller.state = TestWorkerState::UpdateFailed;
        daemon.status = ResourceStatus::UpdateFailed;
        daemon.set_internal_controller(Some(Box::new(controller)))?;
        let executor = new_executor(&stack("v2"))?;

        assert!(
            executor.plan(&state)?.updates.contains_key("daemon"),
            "an open, failed dependency must not block the dependent"
        );

        let cluster = state.resources.get_mut("cluster").expect("cluster state");
        assert!(
            cluster.retry_failed()?,
            "the failed cluster update is retried"
        );
        assert_eq!(cluster.status, ResourceStatus::Updating);
        assert!(
            executor.plan(&state)?.updates.contains_key("daemon"),
            "the retried, open dependency must not block the dependent"
        );
        let next = executor.step(state).await?.next_state;
        assert_eq!(get_status(&next, "cluster"), Some(ResourceStatus::Updating));
        assert_eq!(get_status(&next, "daemon"), Some(ResourceStatus::Updating));
        assert_eq!(daemon_image(&next), "daemon-v2");
        Ok(())
    }

    /// `daemon` also depends on `config`. A desired stack that changes only `config` to v2
    /// while `cluster` is still updating: `daemon`'s own config is the same, so its update
    /// comes from `config` changing, and only both opt-ins on `cluster` let it through.
    #[tokio::test]
    async fn dependency_change_updates_the_dependent_during_another_dependency_update_only_when_both_opt_in(
    ) -> Result<()> {
        fn stack(config_version: &str) -> Stack {
            Stack::new("update-during-dependency-change".to_owned())
                .add(
                    test_function_with_image("cluster", "cluster-v2"),
                    ResourceLifecycle::Live,
                )
                .add(
                    test_function_with_image("config", &format!("config-{config_version}")),
                    ResourceLifecycle::Live,
                )
                .add_with_dependencies(
                    test_function_with_image("daemon", "daemon-v1"),
                    ResourceLifecycle::Live,
                    vec![
                        ResourceRef::new(Worker::RESOURCE_TYPE, "cluster"),
                        ResourceRef::new(Worker::RESOURCE_TYPE, "config"),
                    ],
                )
                .build()
        }

        for (dependents_may_update, updates_during_dependency_change) in
            [(true, true), (true, false), (false, true), (false, false)]
        {
            let both = dependents_may_update && updates_during_dependency_change;
            let mut state = run_to_synced(&new_executor(&stack("v1"))?, new_test_state()).await?;
            assert_all_running(&state, &["cluster", "config", "daemon"]);

            let cluster = state.resources.get_mut("cluster").expect("cluster state");
            let mut controller = cluster.get_internal_controller_typed::<TestWorkerController>()?;
            controller.dependents_may_update = dependents_may_update;
            controller.state = TestWorkerState::UpdateCodePolling;
            cluster.status = ResourceStatus::Updating;
            cluster.set_internal_controller(Some(Box::new(controller)))?;

            let daemon = state.resources.get_mut("daemon").expect("daemon state");
            let mut controller = daemon.get_internal_controller_typed::<TestWorkerController>()?;
            controller.updates_during_dependency_change = updates_during_dependency_change;
            daemon.set_internal_controller(Some(Box::new(controller)))?;

            let plan = new_executor(&stack("v2"))?.plan(&state)?;
            assert!(plan.updates.contains_key("config"));
            assert!(!plan.updates.contains_key("cluster"));
            assert_eq!(
                plan.updates.contains_key("daemon"),
                both,
                "dependency opts in: {dependents_may_update}, dependent opts in: \
                 {updates_during_dependency_change}"
            );
        }
        Ok(())
    }
}

/// Reading a link's binding follows the same two-sided opt-in as the planner: a dependent
/// whose link is still Updating may read it only when the dependency lets dependents update and
/// the dependent says it can. Without both, it waits (`DependencyNotReady`), as before.
#[test]
fn linked_resource_read_during_update_needs_both_opt_ins() {
    use crate::core::kubernetes_manifest_test_support::KubernetesManifestTestHarness;
    use crate::worker::{TestWorkerController, TestWorkerState};

    for (dependents_may_update, updates_during_dependency_change) in
        [(true, true), (true, false), (false, true), (false, false)]
    {
        let both = dependents_may_update && updates_during_dependency_change;
        let dependency = TestWorkerController {
            state: TestWorkerState::UpdateCodePolling,
            dependents_may_update,
            ..Default::default()
        };
        let dependent = TestWorkerController {
            state: TestWorkerState::Ready,
            updates_during_dependency_change,
            ..Default::default()
        };
        let harness =
            KubernetesManifestTestHarness::new(Resource::new(test_function("daemon")), Vec::new())
                .with_linked_resource(test_function("cluster"), dependency)
                .with_linked_resource(test_function("daemon"), dependent);
        let ctx = harness.ctx();
        let links = [ResourceRef::new(Worker::RESOURCE_TYPE, "cluster")];

        let read = crate::core::ensure_linked_resources_running(&links, &ctx, "daemon");
        assert_eq!(
            read.is_ok(),
            both,
            "dependency opts in: {dependents_may_update}, dependent opts in: \
             {updates_during_dependency_change}"
        );
    }
}

/// A controller that opts in for config-only updates, as a container controller can: it reads
/// its links in UpdateStart and records there whether the update kept the image. These tests
/// run the whole executor over a dependency and a dependent that both change in one plan.
mod config_only_updates_in_parallel {
    use super::*;
    use crate::core::state_utils::StackResourceStateExt;
    use crate::worker::{TestWorkerController, TestWorkerState};
    use alien_core::StackState;

    fn worker(id: &str, image: &str, timeout_seconds: u32, link: Option<&Worker>) -> Worker {
        let mut worker = test_function_with_image(id, image);
        worker.timeout_seconds = timeout_seconds;
        if let Some(link) = link {
            worker.links = vec![ResourceRef::new(Worker::RESOURCE_TYPE, link.id.clone())];
        }
        worker
    }

    fn stack(cluster_image: &str, timeout_seconds: u32) -> Stack {
        let cluster = worker("cluster", cluster_image, timeout_seconds, None);
        let daemon = worker("daemon", "daemon-v1", timeout_seconds, Some(&cluster));
        Stack::new("config-only-updates".to_owned())
            .add(cluster, ResourceLifecycle::Live)
            .add_with_dependencies(
                daemon,
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "cluster")],
            )
            .build()
    }

    /// Both resources Running on v1, and both controllers opted in.
    async fn running_and_opted_in() -> Result<StackState> {
        let mut state =
            run_to_synced(&new_executor(&stack("cluster-v1", 30))?, new_test_state()).await?;
        for id in ["cluster", "daemon"] {
            let resource = state.resources.get_mut(id).expect("resource state");
            let mut controller =
                resource.get_internal_controller_typed::<TestWorkerController>()?;
            controller.config_only_opt_in = true;
            resource.set_internal_controller(Some(Box::new(controller)))?;
        }
        Ok(state)
    }

    fn daemon_controller_state(state: &StackState) -> TestWorkerState {
        state.resources["daemon"]
            .get_internal_controller_typed::<TestWorkerController>()
            .expect("daemon controller")
            .state
    }

    /// An env-style change to both, with the image untouched: after the first step the
    /// dependent has started its update next to the dependency's, not behind it.
    #[tokio::test]
    async fn a_config_only_change_updates_dependency_and_dependent_in_the_same_step() -> Result<()>
    {
        let state = running_and_opted_in().await?;
        let next = new_executor(&stack("cluster-v1", 60))?
            .step(state)
            .await?
            .next_state;

        assert_eq!(get_status(&next, "cluster"), Some(ResourceStatus::Updating));
        assert_eq!(get_status(&next, "daemon"), Some(ResourceStatus::Updating));
        assert_ne!(
            daemon_controller_state(&next),
            TestWorkerState::UpdateStart,
            "the dependent must have sent its update while the dependency was updating"
        );
        Ok(())
    }

    /// An image change to the dependency keeps the order: the dependent does not start its
    /// update in any step until the dependency is Running.
    #[tokio::test]
    async fn an_image_change_to_the_dependency_still_makes_the_dependent_wait() -> Result<()> {
        let mut state = running_and_opted_in().await?;
        let executor = new_executor(&stack("cluster-v2", 60))?;
        let mut cluster_was_updating = false;
        for _ in 0..200 {
            state = executor.step(state).await?.next_state;
            if get_status(&state, "cluster") != Some(ResourceStatus::Running) {
                cluster_was_updating = true;
                assert_eq!(
                    daemon_controller_state(&state),
                    TestWorkerState::UpdateStart,
                    "the dependent must not start while an image change is rolling"
                );
            } else if cluster_was_updating
                && get_status(&state, "daemon") == Some(ResourceStatus::Running)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(cluster_was_updating, "the dependency's update ran");
        assert_eq!(get_status(&state, "cluster"), Some(ResourceStatus::Running));
        assert_eq!(get_status(&state, "daemon"), Some(ResourceStatus::Running));
        Ok(())
    }
}
