//! Tests for lifecycle filtering (Frozen vs Live resources).

use super::helpers::*;
use crate::error::Result;
use alien_core::{
    Function, Resource, ResourceLifecycle, ResourceRef, ResourceStatus, Stack, Storage,
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
            vec![ResourceRef::new(Function::RESOURCE_TYPE, "resource-b")],
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
                ResourceRef::new(Function::RESOURCE_TYPE, "resource-b"),
                ResourceRef::new(Function::RESOURCE_TYPE, "resource-c"),
            ],
        )
        .add_with_dependencies(
            resource_e.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Function::RESOURCE_TYPE, "resource-d")],
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

    // Delete Live first
    let live_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;
    let state = run_to_synced(&live_deletion, state).await?;

    assert_eq!(
        get_status(&state, "resource-e"),
        Some(ResourceStatus::Deleted)
    );
    assert_eq!(
        get_status(&state, "resource-b"),
        Some(ResourceStatus::Deleted)
    );
    assert_eq!(
        get_status(&state, "resource-c"),
        Some(ResourceStatus::Deleted)
    );

    // Delete Frozen - D should be deleted, A should be deleted (no more dependents)
    let frozen_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Frozen])?;
    let state = run_to_synced(&frozen_deletion, state).await?;

    assert_eq!(
        get_status(&state, "resource-a"),
        Some(ResourceStatus::Deleted),
        "Resource A should be deleted since Live dependents are gone"
    );
    assert_eq!(
        get_status(&state, "resource-d"),
        Some(ResourceStatus::Deleted)
    );

    Ok(())
}
