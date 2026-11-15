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
        // The dependency has not rolled to this release yet, so none of its dependents is
        // planned: a dependent whose config did not change is not rewritten at all, and one
        // that changed waits for the dependency, whatever its lifecycle.
        for (resource, _, _) in &dependents {
            assert!(
                !plan.updates.contains_key(resource.id()),
                "{} must wait for the dependency's update with {authority:?}",
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
    /// while `cluster` is still updating: `daemon`'s own config is the same, and it waits for
    /// `config` to roll first, with or without the opt-ins on `cluster`.
    #[tokio::test]
    async fn dependent_following_a_changing_dependency_waits_for_it_whatever_the_opt_ins(
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

            // `daemon` follows `config`, which has not rolled to v2 yet: it waits for it, and
            // `cluster`'s opt-in does not change that.
            let plan = new_executor(&stack("v2"))?.plan(&state)?;
            assert!(plan.updates.contains_key("config"));
            assert!(!plan.updates.contains_key("cluster"));
            assert!(
                !plan.updates.contains_key("daemon"),
                "dependency opts in: {dependents_may_update}, dependent opts in: \
                 {updates_during_dependency_change}, both: {both}"
            );
        }
        Ok(())
    }
}

/// Setup renders the AWS management policy after the network it names. The edge only orders
/// setup: management provisions once its dependency is Running, and when the dependency fails,
/// management stays Pending and the failure is the dependency's own.
#[tokio::test]
async fn setup_management_waits_for_the_network_and_never_hangs_on_its_failure() -> Result<()> {
    use alien_core::RemoteStackManagement;
    let stack = |network: Worker| {
        Stack::new("setup-order".to_owned())
            .add(network, ResourceLifecycle::Frozen)
            .add_with_dependencies(
                RemoteStackManagement::new("management".to_string()).build(),
                ResourceLifecycle::Frozen,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "network")],
            )
            .build()
    };

    // The network comes up: management follows it.
    let executor = new_executor_with_filter(
        &stack(test_function("network")),
        vec![ResourceLifecycle::Frozen],
    )?;
    let state = run_to_synced(&executor, new_test_state()).await?;
    assert_all_running(&state, &["network", "management"]);

    // An install from before the edge reruns setup with it: both stay Running.
    let before_edge = Stack::new("setup-order".to_owned())
        .add(test_function("network"), ResourceLifecycle::Frozen)
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let installed = run_to_synced(
        &new_executor_with_filter(&before_edge, vec![ResourceLifecycle::Frozen])?,
        new_test_state(),
    )
    .await?;
    let rerun = run_to_synced(&executor, installed).await?;
    assert_all_running(&rerun, &["network", "management"]);

    // The network fails: management never starts, and the run ends on the network's error.
    let failing = Worker::new("network".to_string())
        .code(alien_core::WorkerCode::Image {
            image: "image-network".to_string(),
        })
        .memory_mb(5120) // the test controller rejects more than 4 GiB
        .permissions("execution".to_string())
        .build();
    let executor = new_executor_with_filter(&stack(failing), vec![ResourceLifecycle::Frozen])?;
    let state = run_steps(&executor, new_test_state(), 50).await?;
    assert_eq!(
        get_status(&state, "network"),
        Some(ResourceStatus::ProvisionFailed)
    );
    assert!(state.resources["network"].error.is_some());
    assert_eq!(
        get_status(&state, "management"),
        Some(ResourceStatus::Pending)
    );
    assert!(state.resources["management"].error.is_none());
    assert!(
        executor
            .run_until_synced(state)
            .await
            .into_result()
            .is_err(),
        "the run ends on the failed network instead of waiting"
    );
    Ok(())
}

/// A release rolls through a chain of dependents in order: a dependent updates only after the
/// resource it links has rolled to the same release, so a failed step leaves everything
/// downstream on the previous one.
mod rollout_order {
    use super::*;
    use alien_core::StackState;

    fn worker(id: &str, release: &str, fail_update: bool) -> Worker {
        let environment = if fail_update {
            [("SIMULATE_UPDATE_FAILURE".to_string(), "true".to_string())]
                .into_iter()
                .collect()
        } else {
            Default::default()
        };
        Worker::new(id.to_string())
            .code(alien_core::WorkerCode::Image {
                image: format!("{id}-{release}"),
            })
            .environment(environment)
            .permissions("execution".to_string())
            .build()
    }

    /// `sandbox` <- `api` (links sandbox) <- `runner` (links api), all Live.
    fn stack(release: &str, fail_sandbox_update: bool) -> Stack {
        Stack::new("rollout-order".to_owned())
            .add(
                worker("sandbox", release, fail_sandbox_update),
                ResourceLifecycle::Live,
            )
            .add_with_dependencies(
                worker("api", release, false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "sandbox")],
            )
            .add_with_dependencies(
                worker("runner", release, false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "api")],
            )
            .build()
    }

    fn image(state: &StackState, id: &str) -> String {
        match &state.resources[id]
            .config
            .downcast_ref::<Worker>()
            .expect("a worker")
            .code
        {
            alien_core::WorkerCode::Image { image } => image.clone(),
            other => panic!("expected an image worker, got {other:?}"),
        }
    }

    async fn running_on_release_one() -> Result<StackState> {
        let state = run_to_synced(&new_executor(&stack("r1", false))?, new_test_state()).await?;
        assert_all_running(&state, &["sandbox", "api", "runner"]);
        Ok(state)
    }

    /// A release whose first step fails. The resources linked to it
    /// stay on the previous release, though their own config changed.
    #[tokio::test]
    async fn failed_dependency_update_keeps_dependents_on_the_previous_release() -> Result<()> {
        let mut state = running_on_release_one().await?;
        let executor = new_executor(&stack("r2", true))?;

        for _ in 0..60 {
            state = executor.step(state).await?.next_state;
        }

        assert_eq!(
            get_status(&state, "sandbox"),
            Some(ResourceStatus::UpdateFailed)
        );
        for id in ["api", "runner"] {
            assert_eq!(
                get_status(&state, id),
                Some(ResourceStatus::Running),
                "{id}"
            );
            assert_eq!(image(&state, id), format!("{id}-r1"), "{id} must not roll");
        }
        Ok(())
    }

    /// Without a failure the release rolls in dependency order: a resource starts its update
    /// only once everything it links is Running on the new release.
    #[tokio::test]
    async fn release_rolls_dependency_first() -> Result<()> {
        let mut state = running_on_release_one().await?;
        let executor = new_update_executor(&stack("r2", false))?;

        let mut started: Vec<String> = Vec::new();
        for _ in 0..80 {
            let before = state.clone();
            state = executor.step(state).await?.next_state;
            for (id, upstream) in [
                ("sandbox", None),
                ("api", Some("sandbox")),
                ("runner", Some("api")),
            ] {
                if image(&before, id) != image(&state, id) {
                    started.push(id.to_string());
                    if let Some(upstream) = upstream {
                        assert_eq!(
                            get_status(&before, upstream),
                            Some(ResourceStatus::Running),
                            "{id} started while {upstream} was not Running"
                        );
                        assert_eq!(
                            image(&before, upstream),
                            format!("{upstream}-r2"),
                            "{id} started before {upstream} rolled"
                        );
                    }
                }
            }
        }

        assert_eq!(started, ["sandbox", "api", "runner"]);
        assert_all_running(&state, &["sandbox", "api", "runner"]);
        Ok(())
    }

    /// `api`'s own config does not change in the release; it only follows `sandbox`. The
    /// `runner` linking it still waits for `sandbox`, not just for `api`'s recorded config.
    #[tokio::test]
    async fn dependent_waits_for_a_dependency_that_only_follows_another() -> Result<()> {
        let mut state = running_on_release_one().await?;
        let desired = Stack::new("rollout-order".to_owned())
            .add(worker("sandbox", "r2", true), ResourceLifecycle::Live)
            .add_with_dependencies(
                worker("api", "r1", false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "sandbox")],
            )
            .add_with_dependencies(
                worker("runner", "r2", false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "api")],
            )
            .build();
        let executor = new_executor(&desired)?;

        for _ in 0..60 {
            state = executor.step(state).await?.next_state;
        }

        assert_eq!(
            get_status(&state, "sandbox"),
            Some(ResourceStatus::UpdateFailed)
        );
        assert_eq!(image(&state, "runner"), "runner-r1");
        assert_eq!(get_status(&state, "runner"), Some(ResourceStatus::Running));
        Ok(())
    }

    /// A resource of another type that depends on `func` and whose own config does not change
    /// is still updated when `func` changes, but only once `func` has rolled: the owed update
    /// is remembered in the state, so it survives plan boundaries and a restart in between.
    #[tokio::test]
    async fn an_owed_update_waits_for_the_dependency_across_plans_and_restarts() -> Result<()> {
        let stack = |image: &str| {
            Stack::new("owed-update".to_owned())
                .add(
                    test_function_with_image("func", image),
                    ResourceLifecycle::Live,
                )
                .add_with_dependencies(
                    test_storage("store"),
                    ResourceLifecycle::Live,
                    vec![ResourceRef::new(Worker::RESOURCE_TYPE, "func")],
                )
                .build()
        };
        let mut state = run_to_synced(&new_executor(&stack("v1"))?, new_test_state()).await?;
        assert_all_running(&state, &["func", "store"]);
        let executor = new_executor(&stack("v2"))?;

        // The first step starts `func` only; `store` owes an update and records it.
        state = executor.step(state).await?.next_state;
        assert_eq!(get_status(&state, "func"), Some(ResourceStatus::Updating));
        assert_eq!(get_status(&state, "store"), Some(ResourceStatus::Running));
        assert!(state.resources["store"].dependency_update_pending);

        let mut store_updated = false;
        for _ in 0..80 {
            // A restart between steps: the marker is all the new executor has to go on.
            state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
            let before = state.clone();
            state = executor.step(state).await?.next_state;
            if get_status(&state, "store") == Some(ResourceStatus::Updating) {
                store_updated = true;
                assert_eq!(
                    get_status(&before, "func"),
                    Some(ResourceStatus::Running),
                    "store started updating while func was still changing"
                );
                assert_eq!(image(&before, "func"), "v2");
            }
        }

        assert!(store_updated, "the owed update never ran");
        assert_all_running(&state, &["func", "store"]);
        assert!(!state.resources["store"].dependency_update_pending);
        Ok(())
    }

    /// A dependency whose controller asks for an update with its config unchanged (a rotated
    /// secret) has not rolled yet, so a linked dependent whose own config changed waits for
    /// that update instead of starting in the same plan.
    #[tokio::test]
    async fn a_dependency_with_a_requested_update_holds_its_dependent_back() -> Result<()> {
        let stack = |release: &str| {
            Stack::new("rollout-requested".to_owned())
                .add(
                    worker("dep-requested", "r1", false),
                    ResourceLifecycle::Live,
                )
                .add_with_dependencies(
                    worker("app-requested", release, false),
                    ResourceLifecycle::Live,
                    vec![ResourceRef::new(Worker::RESOURCE_TYPE, "dep-requested")],
                )
                .build()
        };
        let mut state = run_to_synced(&new_executor(&stack("r1"))?, new_test_state()).await?;
        assert_all_running(&state, &["dep-requested", "app-requested"]);

        crate::worker::request_test_worker_update("dep-requested", true);
        let executor = new_executor(&stack("r2"))?;
        state = executor.step(state).await?.next_state;

        assert_eq!(
            get_status(&state, "dep-requested"),
            Some(ResourceStatus::Updating)
        );
        assert_eq!(
            image(&state, "app-requested"),
            "app-requested-r1",
            "the dependent started in the same plan as its dependency's update"
        );
        crate::worker::request_test_worker_update("dep-requested", false);
        for _ in 0..40 {
            state = executor.step(state).await?.next_state;
        }
        assert_eq!(image(&state, "app-requested"), "app-requested-r2");
        assert_all_running(&state, &["dep-requested", "app-requested"]);
        Ok(())
    }

    /// A dependency this executor does not update (a Frozen resource under a Live-only run)
    /// never holds its dependents back, though its recorded config differs from the desired.
    #[tokio::test]
    async fn frozen_dependency_with_drift_does_not_block_a_live_dependent() -> Result<()> {
        let stack = |public: bool, image: &str| {
            Stack::new("rollout-frozen".to_owned())
                .add(
                    test_storage_with_public_read("store", public),
                    ResourceLifecycle::Frozen,
                )
                .add_with_dependencies(
                    test_function_with_image("func", image),
                    ResourceLifecycle::Live,
                    vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store")],
                )
                .build()
        };
        let mut state =
            run_to_synced(&new_executor(&stack(false, "v1"))?, new_test_state()).await?;
        assert_all_running(&state, &["store", "func"]);

        let live = new_executor_with_filter(&stack(true, "v2"), vec![ResourceLifecycle::Live])?;
        assert!(live.plan(&state)?.updates.contains_key("func"));
        for _ in 0..20 {
            state = live.step(state).await?.next_state;
        }
        assert_eq!(image(&state, "func"), "v2");
        assert_eq!(get_status(&state, "func"), Some(ResourceStatus::Running));
        Ok(())
    }

    /// An adopted external binding that owed an update to a dependency takes the controller-less
    /// update path. That path must clear the marker, or the binding is planned again and its
    /// own dependents wait for it forever.
    #[tokio::test]
    async fn an_external_binding_clears_its_owed_update_once_it_runs() -> Result<()> {
        use alien_core::{
            DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBinding, ExternalBindings,
            StackSettings, StorageBinding,
        };
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "uploads",
            ExternalBinding::Storage(StorageBinding::s3("external-test-bucket")),
        );
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(bindings)
            .allow_frozen_changes(false)
            .build();
        let stack = |image: &str| {
            Stack::new("owed-external".to_owned())
                .add(
                    test_function_with_image("func", image),
                    ResourceLifecycle::Live,
                )
                .add_with_dependencies(
                    test_storage("uploads"),
                    ResourceLifecycle::Live,
                    vec![ResourceRef::new(Worker::RESOURCE_TYPE, "func")],
                )
                .build()
        };
        let build = |image: &str| {
            crate::core::StackExecutor::builder(&stack(image), alien_core::ClientConfig::Test)
                .deployment_config(&config)
                .build()
        };
        let mut state = run_to_synced(&build("v1")?, new_test_state()).await?;
        let executor = build("v2")?;

        state = executor.step(state).await?.next_state;
        assert!(state.resources["uploads"].dependency_update_pending);

        for _ in 0..40 {
            state = executor.step(state).await?.next_state;
        }

        assert_all_running(&state, &["func", "uploads"]);
        assert_eq!(image(&state, "func"), "v2");
        assert!(
            !state.resources["uploads"].dependency_update_pending,
            "the external binding ran its owed update and must forget it"
        );
        assert!(executor.is_synced(&state));
        Ok(())
    }

    /// Every resource depends on the two before it, so the number of paths to the first one
    /// grows like the Fibonacci numbers. Planning an update of a resource at the end must not
    /// walk each path.
    #[tokio::test]
    async fn rollout_check_stays_fast_on_a_dense_dependency_graph() -> Result<()> {
        const LAYERS: usize = 36;
        let stack = |leaf_image: &str| {
            let mut builder = Stack::new("dense".to_owned());
            for i in 0..LAYERS {
                let dependencies = (1..=2)
                    .filter(|back| i >= *back)
                    .map(|back| ResourceRef::new(Worker::RESOURCE_TYPE, format!("w{}", i - back)))
                    .collect::<Vec<_>>();
                builder = builder.add_with_dependencies(
                    test_function_with_image(&format!("w{i}"), "base"),
                    ResourceLifecycle::Live,
                    dependencies,
                );
            }
            builder
                .add_with_dependencies(
                    test_function_with_image("leaf", leaf_image),
                    ResourceLifecycle::Live,
                    vec![
                        ResourceRef::new(Worker::RESOURCE_TYPE, format!("w{}", LAYERS - 1)),
                        ResourceRef::new(Worker::RESOURCE_TYPE, format!("w{}", LAYERS - 2)),
                    ],
                )
                .build()
        };
        let state = run_to_synced(&new_executor(&stack("v1"))?, new_test_state()).await?;

        let executor = new_executor(&stack("v2"))?;
        let started = std::time::Instant::now();
        let plan = executor.plan(&state)?;
        assert!(plan.updates.contains_key("leaf"));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "planning took {:?}",
            started.elapsed()
        );
        Ok(())
    }

    /// `net` (Frozen, setup-owned) <- `mid` (unchanged) <- `app` (new release). Setup owns
    /// `net`; the executor cannot change it, so its failure never holds a dependent back.
    fn chain(app_release: &str) -> Stack {
        Stack::new("rollout-setup-owned".to_owned())
            .add(test_storage("net"), ResourceLifecycle::Frozen)
            .add_with_dependencies(
                worker("mid", "r1", false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Storage::RESOURCE_TYPE, "net")],
            )
            .add_with_dependencies(
                worker("app", app_release, false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "mid")],
            )
            .build()
    }

    /// A setup-owned resource that cannot be refreshed (an old template, say) does not block
    /// the resources above it, directly or further down the chain, when it is not changing.
    #[tokio::test]
    async fn a_failed_setup_owned_dependency_does_not_block_unchanged_dependents() -> Result<()> {
        let mut state = run_to_synced(&new_executor(&chain("r1"))?, new_test_state()).await?;
        assert_all_running(&state, &["net", "mid", "app"]);
        state
            .resources
            .get_mut("net")
            .expect("net is deployed")
            .status = ResourceStatus::RefreshFailed;

        // `app` is two steps above `net`; `mid` is directly above it.
        let executor = new_executor(&chain("r2"))?;
        assert!(executor.plan(&state)?.updates.contains_key("app"));

        let direct = Stack::new("rollout-setup-owned".to_owned())
            .add(test_storage("net"), ResourceLifecycle::Frozen)
            .add_with_dependencies(
                worker("mid", "r2", false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Storage::RESOURCE_TYPE, "net")],
            )
            .add_with_dependencies(
                worker("app", "r1", false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "mid")],
            )
            .build();
        assert!(new_executor(&direct)?
            .plan(&state)?
            .updates
            .contains_key("mid"));
        Ok(())
    }

    /// Further up the chain a failed resource that is not changing holds nothing back, but one
    /// whose update is still to come does.
    #[tokio::test]
    async fn a_failed_dependency_with_a_change_still_to_come_holds_dependents_back() -> Result<()> {
        let mut state = running_on_release_one().await?;
        state
            .resources
            .get_mut("sandbox")
            .expect("sandbox is deployed")
            .status = ResourceStatus::UpdateFailed;
        // `api` follows `sandbox`; `runner` changes. `sandbox` still has its change to come.
        let desired = Stack::new("rollout-order".to_owned())
            .add(worker("sandbox", "r2", false), ResourceLifecycle::Live)
            .add_with_dependencies(
                worker("api", "r1", false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "sandbox")],
            )
            .add_with_dependencies(
                worker("runner", "r2", false),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "api")],
            )
            .build();
        assert!(!new_executor(&desired)?
            .plan(&state)?
            .updates
            .contains_key("runner"));
        Ok(())
    }

    /// A failed setup run leaves the marker on a setup-owned resource that the runtime
    /// executor may not update. It must forget the marker, or the deployment never completes.
    #[tokio::test]
    async fn a_marker_left_by_a_failed_setup_run_is_cleared() -> Result<()> {
        let stack = |release: &str, fail: bool| {
            Stack::new("rollout-marker".to_owned())
                .add(worker("func", release, fail), ResourceLifecycle::Live)
                .add_with_dependencies(
                    test_storage("store"),
                    ResourceLifecycle::Frozen,
                    vec![ResourceRef::new(Worker::RESOURCE_TYPE, "func")],
                )
                .build()
        };
        let setup = |stack: &Stack| {
            crate::core::StackExecutor::builder(stack, alien_core::ClientConfig::Test)
                .deployment_config(&default_deployment_config())
                .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
                .build()
        };
        let mut state = run_to_synced(&setup(&stack("r1", false))?, new_test_state()).await?;
        let failing = setup(&stack("r2", true))?;
        for _ in 0..40 {
            state = failing.step(state).await?.next_state;
        }
        assert_eq!(
            get_status(&state, "func"),
            Some(ResourceStatus::UpdateFailed)
        );
        assert!(
            state.resources["store"].dependency_update_pending,
            "the failed setup run should have left the marker"
        );

        let runtime = new_executor(&stack("r2", false))?;
        let state = run_to_synced(&runtime, state).await?;
        assert_all_running(&state, &["func", "store"]);
        assert!(!state.resources["store"].dependency_update_pending);
        Ok(())
    }

    /// A dependency cycle that planning meets is an error, not a stack overflow.
    #[tokio::test]
    async fn a_dependency_cycle_is_an_error_not_a_stack_overflow() -> Result<()> {
        let stack = |release: &str, cyclic: bool| {
            let a_deps = if cyclic {
                vec![ResourceRef::new(Worker::RESOURCE_TYPE, "a")]
            } else {
                vec![]
            };
            Stack::new("rollout-cycle".to_owned())
                .add_with_dependencies(worker("a", "r1", false), ResourceLifecycle::Live, a_deps)
                .add_with_dependencies(
                    worker("b", release, false),
                    ResourceLifecycle::Live,
                    vec![ResourceRef::new(Worker::RESOURCE_TYPE, "a")],
                )
                .build()
        };
        let mut state =
            run_to_synced(&new_executor(&stack("r1", false))?, new_test_state()).await?;
        // The recorded edges already match the cyclic desired ones, so `a` looks unchanged.
        state
            .resources
            .get_mut("a")
            .expect("a is deployed")
            .dependencies = vec![ResourceRef::new(Worker::RESOURCE_TYPE, "a")];

        let error = new_executor(&stack("r2", true))?
            .plan(&state)
            .expect_err("a cycle must be reported");
        assert_eq!(error.code, "CIRCULAR_DEPENDENCY_DETECTED");
        Ok(())
    }
}
