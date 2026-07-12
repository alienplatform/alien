//! Tests for lifecycle filtering (Frozen vs Live resources).

use super::helpers::*;
use crate::core::state_utils::StackResourceStateExt;
use crate::core::StackExecutor;
use crate::error::Result;
use crate::storage::TestStorageController;
use alien_core::{
    Resource, ResourceLifecycle, ResourceOutputs, ResourceRef, ResourceStatus, Stack, Storage,
    StorageOutputs, Worker,
};

/// Tests that filtering to Frozen only processes Frozen resources.
#[tokio::test]
async fn test_frozen_filter_only_processes_frozen() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("lifecycle-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Frozen])?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&final_state, "store1"),
        Some(ResourceStatus::Running)
    );
    assert_not_in_state(&final_state, &["func1"]);

    Ok(())
}

/// Tests that callers can skip Ready handlers for already-running resources.
#[tokio::test]
async fn test_filtered_executor_can_skip_running_ready_handlers() -> Result<()> {
    let store1 = test_storage("store1");

    let stack = Stack::new("skip-running-ready-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .build();

    let executor = StackExecutor::builder(&stack, alien_core::ClientConfig::Test)
        .deployment_config(
            &alien_core::DeploymentConfig::builder()
                .stack_settings(alien_core::StackSettings::default())
                .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .external_bindings(alien_core::ExternalBindings::default())
                .allow_frozen_changes(false)
                .build(),
        )
        .lifecycle_filter(vec![ResourceLifecycle::Frozen])
        .step_running_resources(false)
        .build()?;

    let mut resource_state = alien_core::StackResourceState::new_pending(
        Storage::RESOURCE_TYPE.to_string(),
        Resource::new(store1),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    resource_state.status = ResourceStatus::Running;
    resource_state.outputs = Some(ResourceOutputs::new(StorageOutputs {
        bucket_name: "imported-store1".to_string(),
    }));
    let mut controller = TestStorageController::default();
    controller.bucket_name = Some("imported-store1".to_string());
    resource_state.set_internal_controller(Some(Box::new(controller)))?;

    let mut state = new_test_state();
    state
        .resources
        .insert("store1".to_string(), resource_state.clone());

    let step_result = executor.step(state).await?;

    assert_eq!(
        get_status(&step_result.next_state, "store1"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        step_result.suggested_delay_ms, None,
        "Running resources should not run Ready handlers when disabled"
    );

    Ok(())
}

/// Tests that filtered executors still run Ready handlers for already-running
/// managed resources outside the filter.
#[tokio::test]
async fn test_filtered_executor_steps_out_of_scope_running_resources() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("out-of-scope-ready-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func1,
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    let state_after_frozen = run_to_synced(
        &new_executor_with_filter(&stack, vec![ResourceLifecycle::Frozen])?,
        new_test_state(),
    )
    .await?;

    let executor = StackExecutor::builder(&stack, alien_core::ClientConfig::Test)
        .deployment_config(
            &alien_core::DeploymentConfig::builder()
                .stack_settings(alien_core::StackSettings::default())
                .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .external_bindings(alien_core::ExternalBindings::default())
                .allow_frozen_changes(false)
                .build(),
        )
        .lifecycle_filter(vec![ResourceLifecycle::Live])
        .build()?;

    let step_result = executor.step(state_after_frozen).await?;

    let storage_controller: TestStorageController = step_result
        .next_state
        .resources
        .get("store1")
        .and_then(|resource| resource.get_internal_controller_typed().ok())
        .expect("storage controller should remain available");

    assert_eq!(
        get_status(&step_result.next_state, "store1"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        storage_controller.ready_checks, 1,
        "Out-of-scope running resources should run Ready handlers when enabled"
    );

    Ok(())
}

/// Tests that filtered executors can finish already-started management work
/// for out-of-scope resources before provisioning dependents.
#[tokio::test]
async fn test_filtered_executor_steps_out_of_scope_updating_dependency() -> Result<()> {
    let store1 = test_storage_with_public_read("store1", false);
    let func1 = test_function("func1");

    let stack = Stack::new("out-of-scope-updating-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func1.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    let state_after_frozen = run_to_synced(
        &new_executor_with_filter(&stack, vec![ResourceLifecycle::Frozen])?,
        new_test_state(),
    )
    .await?;

    let updated_store1 = test_storage_with_public_read("store1", true);
    let updated_stack = Stack::new("out-of-scope-updating-test".to_owned())
        .add(updated_store1, ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func1,
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    let frozen_executor =
        new_executor_with_filter(&updated_stack, vec![ResourceLifecycle::Frozen])?;
    let state_with_updating_frozen = frozen_executor.step(state_after_frozen).await?.next_state;

    assert_eq!(
        get_status(&state_with_updating_frozen, "store1"),
        Some(ResourceStatus::Updating)
    );

    let live_executor = new_executor_with_filter(&updated_stack, vec![ResourceLifecycle::Live])?;
    let final_state = run_steps(&live_executor, state_with_updating_frozen, 8).await?;

    assert_all_running(&final_state, &["store1", "func1"]);

    Ok(())
}

/// Tests incremental deployment: Frozen first, then Live.
#[tokio::test]
async fn test_incremental_deployment_frozen_then_live() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("incremental-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    // Phase 1: Deploy Frozen
    let frozen_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Frozen])?;
    let state = new_test_state();
    let state_after_frozen = run_to_synced(&frozen_executor, state).await?;

    assert_eq!(
        get_status(&state_after_frozen, "store1"),
        Some(ResourceStatus::Running)
    );
    assert_not_in_state(&state_after_frozen, &["func1"]);

    // Phase 2: Deploy Live
    let live_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;
    let final_state = run_to_synced(&live_executor, state_after_frozen).await?;

    assert_all_running(&final_state, &["store1", "func1"]);
    Ok(())
}

/// Tests Live resource depending on Frozen resource works with incremental deployment.
#[tokio::test]
async fn test_live_depends_on_frozen_incremental() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("cross-lifecycle-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func1.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    // Phase 1: Deploy Frozen
    let frozen_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Frozen])?;
    let state = new_test_state();
    let state_after_frozen = run_to_synced(&frozen_executor, state).await?;

    assert_eq!(
        get_status(&state_after_frozen, "store1"),
        Some(ResourceStatus::Running)
    );

    // Phase 2: Deploy Live
    let live_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;
    let final_state = run_to_synced(&live_executor, state_after_frozen).await?;

    assert_all_running(&final_state, &["store1", "func1"]);
    Ok(())
}

/// Tests deploying both lifecycles at once (no filter).
#[tokio::test]
async fn test_no_filter_deploys_all() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("all-lifecycle-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["store1", "func1"]);
    Ok(())
}

/// Tests filtering with multiple lifecycle types (Frozen + Live = all).
#[tokio::test]
async fn test_multi_lifecycle_filtering() -> Result<()> {
    let frozen_store = test_storage("frozen-store");
    let live_func = test_function("live-func");

    let stack = Stack::new("multi-filter-stack".to_owned())
        .add(frozen_store.clone(), ResourceLifecycle::Frozen)
        .add(live_func.clone(), ResourceLifecycle::Live)
        .build();

    // Create executor with filter for both Frozen and Live resources (all)
    let executor = new_executor_with_filter(
        &stack,
        vec![ResourceLifecycle::Frozen, ResourceLifecycle::Live],
    )?;

    let state = new_test_state();
    let final_state = run_to_synced(&executor, state).await?;

    // Verify both resources were processed
    assert_eq!(
        get_status(&final_state, "frozen-store"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&final_state, "live-func"),
        Some(ResourceStatus::Running)
    );

    Ok(())
}

/// Tests is_synced with filtered resources.
#[tokio::test]
async fn test_is_synced_with_filtered_resources() -> Result<()> {
    let frozen_store = test_storage("frozen-store");
    let live_store = test_storage("live-store");
    let live_func = test_function("live-func");

    let stack = Stack::new("terminal-test-stack".to_owned())
        .add(frozen_store.clone(), ResourceLifecycle::Frozen)
        .add(live_store.clone(), ResourceLifecycle::Live)
        .add(live_func.clone(), ResourceLifecycle::Live)
        .build();

    // Create executor filtered to Frozen resources only
    let frozen_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Frozen])?;

    let state = new_test_state();

    // Run one step to get the frozen store created
    let step_result = frozen_executor.step(state).await?;
    let mut state = step_result.next_state;

    // After one step, frozen_store should be provisioning but not synced
    assert_eq!(
        get_status(&state, "frozen-store"),
        Some(ResourceStatus::Provisioning)
    );
    assert!(
        !frozen_executor.is_synced(&state),
        "Stack should not be synced after initial step"
    );

    // Run to completion
    state = run_to_synced(&frozen_executor, state).await?;

    // Now, stack should be synced with only frozen_store running
    assert_eq!(
        get_status(&state, "frozen-store"),
        Some(ResourceStatus::Running)
    );
    assert_not_in_state(&state, &["live-store", "live-func"]);

    // The stack should be synced for the frozen executor
    assert!(
        frozen_executor.is_synced(&state),
        "Stack should be synced after filtered resources reach Running state"
    );

    // Now create a new executor with different filter and same state
    let live_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;

    // The stack should NOT be synced for this executor since Live resources aren't processed yet
    assert!(
        !live_executor.is_synced(&state),
        "Stack should not be synced for live_executor since Live resources not yet processed"
    );

    // Run the live executor to completion
    state = run_to_synced(&live_executor, state).await?;

    // Now all resources (from both filters) should be running
    assert_all_running(&state, &["frozen-store", "live-store", "live-func"]);

    // And the stack should be synced for both executors
    assert!(
        frozen_executor.is_synced(&state),
        "Stack should be synced for frozen executor"
    );
    assert!(
        live_executor.is_synced(&state),
        "Stack should be synced for live executor"
    );

    Ok(())
}

/// Tests transitive dependencies across different lifecycles.
#[tokio::test]
async fn test_transitive_dependencies_across_lifecycles() -> Result<()> {
    // A (Frozen) -> B (Live) -> C (Frozen)
    let resource_a = test_storage("resource-a");
    let resource_b = test_function("resource-b");
    let resource_c = test_function("resource-c");

    let stack = Stack::new("transitive-deps-stack".to_owned())
        .add(resource_a.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            resource_b.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "resource-a")],
        )
        .add_with_dependencies(
            resource_c.clone(),
            ResourceLifecycle::Frozen,
            vec![ResourceRef::new(Worker::RESOURCE_TYPE, "resource-b")],
        )
        .build();

    // Deploy all resources
    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state = run_to_synced(&executor, state).await?;

    assert_all_running(&state, &["resource-a", "resource-b", "resource-c"]);

    // Now try to delete only Frozen resources (A and C)
    let deletion_executor = new_deletion_executor_with_filter(vec![ResourceLifecycle::Frozen])?;

    let final_state = run_to_synced(&deletion_executor, state).await?;

    // Resource C should be deleted, but A should remain because B depends on it
    assert_eq!(
        get_status(&final_state, "resource-a"),
        Some(ResourceStatus::Running),
        "Resource A should remain due to Live dependent"
    );
    assert_eq!(
        get_status(&final_state, "resource-b"),
        Some(ResourceStatus::Running),
        "Resource B (Live) should remain"
    );
    assert_eq!(
        get_status(&final_state, "resource-c"),
        Some(ResourceStatus::Deleted),
        "Resource C should be deleted"
    );

    Ok(())
}

/// Tests resource with dependents of mixed lifecycles.
#[tokio::test]
async fn test_resource_with_mixed_lifecycle_dependents() -> Result<()> {
    let base_resource = test_storage("base-resource");
    let frozen_dependent = test_function("frozen-dependent");
    let live_dependent = test_function("live-dependent");

    let stack = Stack::new("mixed-dependents-stack".to_owned())
        .add(base_resource.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            frozen_dependent.clone(),
            ResourceLifecycle::Frozen,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "base-resource")],
        )
        .add_with_dependencies(
            live_dependent.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "base-resource")],
        )
        .build();

    // Set up initial state with all resources
    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state = run_to_synced(&executor, state).await?;

    assert_all_running(
        &state,
        &["base-resource", "frozen-dependent", "live-dependent"],
    );

    // Try to delete only Frozen resources
    let frozen_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Frozen])?;

    let state_after_frozen = run_to_synced(&frozen_deletion, state).await?;

    // Base resource should not be deleted because it has a Live dependent
    assert_eq!(
        get_status(&state_after_frozen, "base-resource"),
        Some(ResourceStatus::Running),
        "Base resource should not be deleted due to Live dependent"
    );
    // Frozen dependent should be deleted
    assert_eq!(
        get_status(&state_after_frozen, "frozen-dependent"),
        Some(ResourceStatus::Deleted),
        "Frozen dependent should be deleted"
    );

    // Delete Live resources
    let live_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;
    let state_after_live = run_to_synced(&live_deletion, state_after_frozen).await?;

    assert_eq!(
        get_status(&state_after_live, "base-resource"),
        Some(ResourceStatus::Running),
        "Base resource should remain"
    );
    assert_eq!(
        get_status(&state_after_live, "live-dependent"),
        Some(ResourceStatus::Deleted),
        "Live dependent should be deleted"
    );

    // Now base resource can be deleted (no more dependents)
    let final_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Frozen])?;
    let final_state = run_to_synced(&final_deletion, state_after_live).await?;

    assert_eq!(
        get_status(&final_state, "base-resource"),
        Some(ResourceStatus::Deleted),
        "Base resource should now be deleted"
    );

    Ok(())
}

/// Tests changing lifecycle filters mid-operation.
#[tokio::test]
async fn test_changing_lifecycle_filters() -> Result<()> {
    let frozen_res = test_storage("frozen-res");
    let live_res = test_storage("live-res");

    let stack = Stack::new("changing-filters-stack".to_owned())
        .add(frozen_res.clone(), ResourceLifecycle::Frozen)
        .add(live_res.clone(), ResourceLifecycle::Live)
        .build();

    // Start with only Frozen resources
    let frozen_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Frozen])?;
    let state = new_test_state();

    // Run one step only
    let step_result = frozen_executor.step(state).await?;
    let mut state = step_result.next_state;

    // frozen_res should be Pending or Provisioning after one step
    assert!(
        matches!(
            get_status(&state, "frozen-res"),
            Some(ResourceStatus::Pending) | Some(ResourceStatus::Provisioning)
        ),
        "Frozen resource should be Pending or Provisioning after one step"
    );
    assert_not_in_state(&state, &["live-res"]);

    // Switch to Live resources (without completing Frozen)
    let live_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;
    state = run_to_synced(&live_executor, state).await?;

    assert_eq!(
        get_status(&state, "live-res"),
        Some(ResourceStatus::Running),
        "Live resource should be Running"
    );

    // Complete everything
    let completion_executor = new_executor(&stack)?;
    state = run_to_synced(&completion_executor, state).await?;

    // All should be Running
    assert_all_running(&state, &["frozen-res", "live-res"]);

    Ok(())
}

/// Tests complex dependency graph with resources of different lifecycles.
#[tokio::test]
async fn test_complex_dependency_graph_with_lifecycles() -> Result<()> {
    // Graph:
    //   A (Frozen) <- C (Live)
    //   |              |
    //   v              v
    //   B (Live) ---> D (Frozen)
    //                  |
    //                  v
    //                  E (Live)

    let resource_a = test_storage("resource-a");
    let resource_b = test_function("resource-b");
    let resource_c = test_function("resource-c");
    let resource_d = test_function("resource-d");
    let resource_e = test_function("resource-e");

    let stack = Stack::new("complex-graph-stack".to_owned())
        .add(resource_a.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            resource_b.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "resource-a")],
        )
        .add_with_dependencies(
            resource_c.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "resource-a")],
        )
        .add_with_dependencies(
            resource_d.clone(),
            ResourceLifecycle::Frozen,
            vec![
                ResourceRef::new(Worker::RESOURCE_TYPE, "resource-b"),
                ResourceRef::new(Worker::RESOURCE_TYPE, "resource-c"),
            ],
        )
        .add_with_dependencies(
            resource_e.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Worker::RESOURCE_TYPE, "resource-d")],
        )
        .build();

    // Deploy all resources
    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state = run_to_synced(&executor, state).await?;

    assert_all_running(
        &state,
        &[
            "resource-a",
            "resource-b",
            "resource-c",
            "resource-d",
            "resource-e",
        ],
    );

    // Phase 1: Delete Live — only E can be deleted.
    // B and C cannot be deleted because D (Frozen) still depends on them.
    let live_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;
    let state = run_to_synced(&live_deletion, state).await?;

    assert_eq!(
        get_status(&state, "resource-e"),
        Some(ResourceStatus::Deleted)
    );
    assert_eq!(
        get_status(&state, "resource-b"),
        Some(ResourceStatus::Running),
        "B should not be deleted — D (Frozen) still depends on it"
    );
    assert_eq!(
        get_status(&state, "resource-c"),
        Some(ResourceStatus::Running),
        "C should not be deleted — D (Frozen) still depends on it"
    );

    // Phase 2: Delete Frozen — D can be deleted (E is gone). A stays (B, C depend on it).
    let frozen_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Frozen])?;
    let state = run_to_synced(&frozen_deletion, state).await?;

    assert_eq!(
        get_status(&state, "resource-d"),
        Some(ResourceStatus::Deleted)
    );
    assert_eq!(
        get_status(&state, "resource-a"),
        Some(ResourceStatus::Running),
        "A should not be deleted — B, C (Live) still depend on it"
    );

    // Phase 3: Delete Live again — B and C can now be deleted (D is gone).
    let live_deletion2 = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;
    let state = run_to_synced(&live_deletion2, state).await?;

    assert_eq!(
        get_status(&state, "resource-b"),
        Some(ResourceStatus::Deleted)
    );
    assert_eq!(
        get_status(&state, "resource-c"),
        Some(ResourceStatus::Deleted)
    );

    // Phase 4: Delete Frozen again — A can now be deleted (no more dependents).
    let frozen_deletion2 = new_deletion_executor_with_filter(vec![ResourceLifecycle::Frozen])?;
    let state = run_to_synced(&frozen_deletion2, state).await?;

    assert_eq!(
        get_status(&state, "resource-a"),
        Some(ResourceStatus::Deleted),
        "A should be deleted since all dependents are gone"
    );

    Ok(())
}

/// Regression for the machines waiting-for-machines deadlock: a frozen
/// resource whose Ready handler observes external reality (e.g. machine
/// inventory) must keep refreshing its outputs while a Live-filtered
/// executor steps the deployment. Frozen restricts mutation, never
/// observation.
#[tokio::test]
async fn test_live_filtered_executor_refreshes_frozen_observed_outputs() -> Result<()> {
    let observed = {
        let mut worker = test_function("observed-cluster");
        worker.environment.insert(
            "SIMULATE_OBSERVED_URL_REFRESH".to_string(),
            "true".to_string(),
        );
        worker
    };
    let live_func = test_function("live-func");

    let stack = Stack::new("frozen-observation-test".to_owned())
        .add(observed.clone(), ResourceLifecycle::Frozen)
        .add(live_func.clone(), ResourceLifecycle::Live)
        .build();

    // Phase 1: provision everything (no filter) so the frozen resource is Running.
    let full_executor = new_executor(&stack)?;
    let state = run_to_synced(&full_executor, new_test_state()).await?;
    assert_all_running(&state, &["observed-cluster", "live-func"]);

    let url_of = |state: &alien_core::StackState| -> Option<String> {
        state
            .resources
            .get("observed-cluster")
            .and_then(|r| r.outputs.as_ref())
            .and_then(|o| o.downcast_ref::<alien_core::WorkerOutputs>())
            .and_then(|o| o.public_endpoints.get("default"))
            .map(|e| e.url.clone())
    };
    let url_before = url_of(&state).expect("frozen resource should expose outputs");

    // Phase 2: step with a Live lifecycle filter, as deployment provisioning does.
    let live_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;
    let mut state = state;
    for _ in 0..3 {
        state = live_executor.step(state).await?.next_state;
    }

    let url_after = url_of(&state).expect("frozen resource outputs must survive live steps");
    assert_ne!(
        url_before, url_after,
        "Live-filtered steps must refresh the frozen resource's observed outputs"
    );
    assert_eq!(
        get_status(&state, "observed-cluster"),
        Some(ResourceStatus::Running),
        "observation must not mutate the frozen resource's status"
    );

    Ok(())
}

/// A transient observation failure (RefreshFailed) on a frozen resource must
/// not park it: out-of-scope stepping keeps running it, the controller
/// recovers, and observed outputs keep refreshing. One transient error must
/// never permanently freeze observed state.
#[tokio::test]
async fn test_frozen_resource_recovers_from_failed_observation() -> Result<()> {
    let observed = {
        let mut worker = test_function("observed-cluster");
        worker.environment.insert(
            "SIMULATE_OBSERVED_URL_REFRESH".to_string(),
            "true".to_string(),
        );
        worker.environment.insert(
            "SIMULATE_OBSERVED_REFRESH_FAIL_ONCE".to_string(),
            "true".to_string(),
        );
        worker
    };
    let live_func = test_function("live-func");

    let stack = Stack::new("frozen-observation-recovery-test".to_owned())
        .add(observed.clone(), ResourceLifecycle::Frozen)
        .add(live_func.clone(), ResourceLifecycle::Live)
        .build();

    let full_executor = new_executor(&stack)?;
    let state = run_to_synced(&full_executor, new_test_state()).await?;

    let url_of = |state: &alien_core::StackState| -> Option<String> {
        state
            .resources
            .get("observed-cluster")
            .and_then(|r| r.outputs.as_ref())
            .and_then(|o| o.downcast_ref::<alien_core::WorkerOutputs>())
            .and_then(|o| o.public_endpoints.get("default"))
            .map(|e| e.url.clone())
    };
    let url_before = url_of(&state).expect("frozen resource should expose outputs");

    // Step under the Live filter; the first observation dips into
    // RefreshFailed, subsequent steps must recover it.
    let live_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Live])?;
    let mut state = state;
    let mut seen_statuses = Vec::new();
    for _ in 0..4 {
        state = live_executor.step(state).await?.next_state;
        seen_statuses.push(get_status(&state, "observed-cluster"));
    }

    assert!(
        seen_statuses.contains(&Some(ResourceStatus::RefreshFailed)),
        "the simulated observation failure should surface as RefreshFailed, saw: {seen_statuses:?}"
    );
    assert_eq!(
        get_status(&state, "observed-cluster"),
        Some(ResourceStatus::Running),
        "the frozen resource must recover to Running after the transient failure"
    );
    let url_after = url_of(&state).expect("outputs must survive the failure");
    assert_ne!(
        url_before, url_after,
        "observation must keep refreshing outputs after recovery"
    );

    Ok(())
}
