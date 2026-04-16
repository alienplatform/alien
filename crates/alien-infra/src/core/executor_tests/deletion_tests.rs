//! Tests for resource deletion flows and ordering.

use super::helpers::*;
use crate::error::Result;
use alien_core::{ResourceLifecycle, ResourceRef, ResourceStatus, Stack, Storage};

/// Tests basic deletion of a single resource.
#[tokio::test]
async fn test_delete_single_resource() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("delete-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&state_after_create, "func1"),
        Some(ResourceStatus::Running)
    );

    let deletion_executor = new_deletion_executor()?;
    let final_state = run_to_synced(&deletion_executor, state_after_create).await?;

    assert_deleted(&final_state, &["func1"]);
    Ok(())
}

/// Tests deletion order is reverse of creation order.
#[tokio::test]
async fn test_deletion_respects_reverse_dependency_order() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("reverse-delete-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            func1.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "store1")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["store1", "func1"]);

    let deletion_executor = new_deletion_executor()?;

    // Track deletion order
    let mut deletion_order = Vec::new();
    let mut current_state = state_after_create.clone();
    let max_steps = 20;

    for _ in 0..max_steps {
        if deletion_executor.is_synced(&current_state) {
            break;
        }
        let step_result = deletion_executor.step(current_state).await?;
        current_state = step_result.next_state;

        for (resource_id, resource_state) in &current_state.resources {
            if resource_state.status == ResourceStatus::Deleting
                && !deletion_order.contains(resource_id)
            {
                deletion_order.push(resource_id.clone());
            }
        }
    }

    // Finish deletion
    let final_state = run_to_synced(&deletion_executor, current_state).await?;
    assert_deleted(&final_state, &["store1", "func1"]);

    // Dependent (func1) should be deleted before dependency (store1)
    let func_index = deletion_order
        .iter()
        .position(|id| id == "func1")
        .expect("func1 should be in deletion order");
    let store_index = deletion_order
        .iter()
        .position(|id| id == "store1")
        .expect("store1 should be in deletion order");

    assert!(
        func_index < store_index,
        "func1 should be deleted before store1 (dependent before dependency)"
    );

    Ok(())
}

/// Tests partial deletion using lifecycle filter.
#[tokio::test]
async fn test_partial_deletion_with_lifecycle_filter() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("partial-delete-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["store1", "func1"]);

    // Delete only Live resources
    let live_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;
    let final_state = run_to_synced(&live_deletion, state_after_create).await?;

    assert_eq!(
        get_status(&final_state, "store1"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&final_state, "func1"),
        Some(ResourceStatus::Deleted)
    );
    Ok(())
}

/// Tests deleting multiple independent resources.
#[tokio::test]
async fn test_delete_multiple_independent_resources() -> Result<()> {
    let func1 = test_function("func1");
    let func2 = test_function("func2");

    let stack = Stack::new("multi-delete-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .add(func2.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["func1", "func2"]);

    let deletion_executor = new_deletion_executor()?;
    let final_state = run_to_synced(&deletion_executor, state_after_create).await?;

    assert_deleted(&final_state, &["func1", "func2"]);
    Ok(())
}

/// Tests deletion from empty state.
#[tokio::test]
async fn test_delete_empty_state() -> Result<()> {
    let deletion_executor = new_deletion_executor()?;
    let state = new_test_state();

    // Should complete immediately with no changes
    assert!(
        deletion_executor.is_synced(&state),
        "Empty state should be synced"
    );

    let final_state = run_to_synced(&deletion_executor, state).await?;
    assert!(final_state.resources.is_empty());

    Ok(())
}

/// Tests deletion of resources in mixed states.
#[tokio::test]
async fn test_delete_mixed_states() -> Result<()> {
    let running_func = test_function("running-func");
    let pending_func = test_function("pending-func");

    let stack = Stack::new("mixed-states-test".to_owned())
        .add(running_func.clone(), ResourceLifecycle::Live)
        .add(pending_func.clone(), ResourceLifecycle::Live)
        .build();

    // Create only one resource
    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_partial = run_to_synced(&executor, state).await?;

    // Both should be running after full sync
    assert_all_running(&state_partial, &["running-func", "pending-func"]);

    // Now delete everything
    let deletion_executor = new_deletion_executor()?;
    let final_state = run_to_synced(&deletion_executor, state_partial).await?;

    assert_deleted(&final_state, &["running-func", "pending-func"]);

    Ok(())
}

/// Tests deletion handles already deleted resources.
#[tokio::test]
async fn test_delete_already_deleted() -> Result<()> {
    // First create and run a resource
    let func1 = test_function("func1");
    let stack = Stack::new("delete-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    // Delete it
    let deletion_executor = new_deletion_executor()?;
    let state_after_delete = run_to_synced(&deletion_executor, state_after_create).await?;

    assert_deleted(&state_after_delete, &["func1"]);

    // Try to delete again - should be a no-op (already synced)
    assert!(
        deletion_executor.is_synced(&state_after_delete),
        "Already deleted state should be synced"
    );

    Ok(())
}

/// Tests deletion of dependent resources in correct order.
#[tokio::test]
async fn test_delete_dependent_running() -> Result<()> {
    let base_storage = test_storage("base-storage");
    let middle_func = test_function("middle-func");
    let leaf_func = test_function("leaf-func");

    let stack = Stack::new("dependent-delete-test".to_owned())
        .add(base_storage.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            middle_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "base-storage")],
        )
        .add_with_dependencies(
            leaf_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(
                alien_core::Function::RESOURCE_TYPE,
                "middle-func",
            )],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(
        &state_after_create,
        &["base-storage", "middle-func", "leaf-func"],
    );

    // Delete all
    let deletion_executor = new_deletion_executor()?;

    // Track deletion order
    let mut deletion_order = Vec::new();
    let mut current_state = state_after_create;
    let max_steps = 30;

    for _ in 0..max_steps {
        if deletion_executor.is_synced(&current_state) {
            break;
        }
        let step_result = deletion_executor.step(current_state).await?;
        current_state = step_result.next_state;

        for (resource_id, resource_state) in &current_state.resources {
            if resource_state.status == ResourceStatus::Deleting
                && !deletion_order.contains(resource_id)
            {
                deletion_order.push(resource_id.clone());
            }
        }
    }

    // Verify order: leaf first, then middle, then base
    let leaf_idx = deletion_order
        .iter()
        .position(|id| id == "leaf-func")
        .expect("leaf should be in order");
    let middle_idx = deletion_order
        .iter()
        .position(|id| id == "middle-func")
        .expect("middle should be in order");
    let base_idx = deletion_order
        .iter()
        .position(|id| id == "base-storage")
        .expect("base should be in order");

    assert!(
        leaf_idx < middle_idx,
        "leaf should be deleted before middle"
    );
    assert!(
        middle_idx < base_idx,
        "middle should be deleted before base"
    );

    Ok(())
}

/// Tests deletion terminal state tracking with filters.
#[tokio::test]
async fn test_deletion_terminal_state_with_filters() -> Result<()> {
    let frozen_store = test_storage("frozen-store");
    let live_store = test_storage("live-store");

    let stack = Stack::new("deletion-terminal-test".to_owned())
        .add(frozen_store.clone(), ResourceLifecycle::Frozen)
        .add(live_store.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["frozen-store", "live-store"]);

    // Delete only Live resources
    let live_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Live])?;

    // Run a single step
    let step_result = live_deletion.step(state_after_create.clone()).await?;
    let mut state = step_result.next_state;

    // After one step, live_store should be Deleting but not synced
    assert_eq!(
        get_status(&state, "frozen-store"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&state, "live-store"),
        Some(ResourceStatus::Deleting)
    );
    assert!(
        !live_deletion.is_synced(&state),
        "Stack should not be synced during deletion"
    );

    // Run to completion
    state = run_to_synced(&live_deletion, state).await?;

    // live_store deleted, frozen_store still Running
    assert_eq!(
        get_status(&state, "frozen-store"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&state, "live-store"),
        Some(ResourceStatus::Deleted)
    );

    // Stack should be synced for the deletion executor
    assert!(
        live_deletion.is_synced(&state),
        "Stack should be synced after filtered deletion completes"
    );

    Ok(())
}

/// Tests deletion with dependency in different lifecycle filter.
#[tokio::test]
async fn test_deletion_with_filtered_dependencies() -> Result<()> {
    let base_store = test_storage("base-store");
    let dependent_func = test_function("dependent-func");

    let stack = Stack::new("dep-deletion-test".to_owned())
        .add(base_store.clone(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            dependent_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "base-store")],
        )
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_all_running(&state_after_create, &["base-store", "dependent-func"]);

    // Try to delete only Frozen (base store) while keeping dependent
    // This should not delete base_store because dependent_func depends on it
    let frozen_deletion = new_deletion_executor_with_filter(vec![ResourceLifecycle::Frozen])?;
    let final_state = run_to_synced(&frozen_deletion, state_after_create).await?;

    // base_store should not be deleted because dependent_func depends on it
    assert_eq!(
        get_status(&final_state, "base-store"),
        Some(ResourceStatus::Running),
        "Base store should not be deleted - has Live dependent"
    );
    assert_eq!(
        get_status(&final_state, "dependent-func"),
        Some(ResourceStatus::Running),
        "Dependent func should remain"
    );

    Ok(())
}
