//! Tests for edge cases and special state handling.

use super::helpers::*;
use crate::error::Result;
use alien_core::{ResourceLifecycle, ResourceRef, ResourceStatus, Stack, Storage};

/// Tests recreating a deleted resource.
#[tokio::test]
async fn test_recreate_deleted_resource() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("recreate-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&state_after_create, "func1"),
        Some(ResourceStatus::Running)
    );

    // Delete the resource
    let deletion_executor = new_deletion_executor()?;
    let state_after_delete = run_to_synced(&deletion_executor, state_after_create).await?;

    assert_deleted(&state_after_delete, &["func1"]);

    // Recreate using the same stack
    let recreate_executor = new_executor(&stack)?;
    let final_state = run_to_synced(&recreate_executor, state_after_delete).await?;

    assert_eq!(
        get_status(&final_state, "func1"),
        Some(ResourceStatus::Running),
        "Resource should be recreated after deletion"
    );

    Ok(())
}

/// Tests handling resources that are externally provisioned.
#[tokio::test]
async fn test_externally_provisioned_skip_delete() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("external-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let mut state_after_create = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&state_after_create, "func1"),
        Some(ResourceStatus::Running)
    );

    // Mark the resource as externally provisioned
    if let Some(resource_state) = state_after_create.resources.get_mut("func1") {
        resource_state.is_externally_provisioned = true;
    }

    // Try to delete - should skip externally provisioned
    let empty_stack = Stack::new("external-test".to_owned()).build();
    let delete_executor = new_executor(&empty_stack)?;

    let plan = delete_executor.plan(&state_after_create)?;

    // Externally provisioned resources should not be in deletes
    assert!(
        !plan.deletes.contains(&"func1".to_string()),
        "Externally provisioned resource should not be marked for deletion"
    );

    Ok(())
}

/// Tests handling resources in various states during batch operations.
#[tokio::test]
async fn test_resources_in_various_states() -> Result<()> {
    let mut state = new_test_state();

    // Add resources in different states
    state.resources.insert(
        "running-func".to_string(),
        create_running_function_state("running-func", "image"),
    );
    state.resources.insert(
        "pending-func".to_string(),
        create_pending_function_state("pending-func"),
    );
    state.resources.insert(
        "deleted-func".to_string(),
        create_deleted_function_state("deleted-func"),
    );

    // Create stack with only running-func (should delete pending-func, skip deleted-func)
    let func = test_function("running-func");
    let stack = Stack::new("various-states-test".to_owned())
        .add(func, ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let plan = executor.plan(&state)?;

    // running-func: no changes (already running with same config)
    // pending-func: should be deleted (not in desired stack)
    // deleted-func: already deleted, no action

    assert!(
        plan.deletes.contains(&"pending-func".to_string()),
        "pending-func should be deleted"
    );
    assert!(
        !plan.deletes.contains(&"deleted-func".to_string()),
        "deleted-func already deleted, no action"
    );

    Ok(())
}

/// Tests handling pending-only resources during deletion.
/// Pending resources are removed directly from state (not marked as Deleted).
#[tokio::test]
async fn test_delete_only_pending() -> Result<()> {
    let mut state = new_test_state();

    // Add only pending resources
    state.resources.insert(
        "pending1".to_string(),
        create_pending_function_state("pending1"),
    );
    state.resources.insert(
        "pending2".to_string(),
        create_pending_function_state("pending2"),
    );

    let deletion_executor = new_deletion_executor()?;
    let final_state = run_to_synced(&deletion_executor, state).await?;

    // Pending resources are removed directly from state (not marked as Deleted)
    assert_not_in_state(&final_state, &["pending1", "pending2"]);

    Ok(())
}

/// Tests idempotent updates (no change when config is same).
#[tokio::test]
async fn test_idempotent_update() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("idempotent-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_v1 = run_to_synced(&executor, state).await?;

    // Run again with same config
    let state_v2 = run_to_synced(&executor, state_v1.clone()).await?;

    // Should be unchanged
    assert_eq!(
        get_status(&state_v2, "func1"),
        Some(ResourceStatus::Running)
    );

    // Check that config remained the same
    let config_v1 = &state_v1.resources.get("func1").unwrap().config;
    let config_v2 = &state_v2.resources.get("func1").unwrap().config;
    assert_eq!(config_v1, config_v2, "Config should remain unchanged");

    Ok(())
}

/// Tests step-by-step execution with no suggested delay.
#[tokio::test]
async fn test_step_without_delay() -> Result<()> {
    let store1 = test_storage("store1");

    let stack = Stack::new("no-delay-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    // Storage typically doesn't require polling delay
    let step_result = executor.step(state).await?;

    // First step should progress immediately
    assert!(
        matches!(
            get_status(&step_result.next_state, "store1"),
            Some(ResourceStatus::Provisioning) | Some(ResourceStatus::Running)
        ),
        "Storage should progress immediately"
    );

    Ok(())
}

/// Tests handling config change triggers update.
#[tokio::test]
async fn test_config_change_triggers_update() -> Result<()> {
    let func_v1 = test_function_with_image("func1", "image-v1");

    let stack_v1 = Stack::new("config-change-test".to_owned())
        .add(func_v1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_v1 = run_to_synced(&executor_v1, state).await?;

    let original_config = state_v1.resources.get("func1").unwrap().config.clone();

    // Change config
    let func_v2 = test_function_with_image("func1", "image-v2");

    let stack_v2 = Stack::new("config-change-test".to_owned())
        .add(func_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_v1).await?;

    let new_config = &final_state.resources.get("func1").unwrap().config;
    assert_ne!(
        &original_config, new_config,
        "Config should change after update"
    );

    Ok(())
}

/// Tests empty stack operations.
#[tokio::test]
async fn test_empty_stack() -> Result<()> {
    let empty_stack = Stack::new("empty-test".to_owned()).build();

    let executor = new_executor(&empty_stack)?;
    let state = new_test_state();

    // Should be immediately synced
    assert!(executor.is_synced(&state), "Empty stack should be synced");

    let plan = executor.plan(&state)?;
    assert!(plan.creates.is_empty());
    assert!(plan.updates.is_empty());
    assert!(plan.deletes.is_empty());

    Ok(())
}

/// Tests that dependencies are preserved through state transitions.
#[tokio::test]
async fn test_dependencies_preserved_in_state() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("deps-preserved-test".to_owned())
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

    // Check func1's dependencies are stored in state
    let func_state = final_state.resources.get("func1").unwrap();
    assert!(
        !func_state.dependencies.is_empty(),
        "Dependencies should be preserved in state"
    );
    assert!(
        func_state.dependencies.iter().any(|d| d.id() == "store1"),
        "Should depend on store1"
    );

    Ok(())
}

/// Tests lifecycle is preserved in state.
#[tokio::test]
async fn test_lifecycle_preserved_in_state() -> Result<()> {
    let frozen_store = test_storage("frozen-store");
    let live_func = test_function("live-func");

    let stack = Stack::new("lifecycle-preserved-test".to_owned())
        .add(frozen_store.clone(), ResourceLifecycle::Frozen)
        .add(live_func.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let final_state = run_to_synced(&executor, state).await?;

    assert_eq!(
        final_state.resources.get("frozen-store").unwrap().lifecycle,
        Some(ResourceLifecycle::Frozen)
    );
    assert_eq!(
        final_state.resources.get("live-func").unwrap().lifecycle,
        Some(ResourceLifecycle::Live)
    );

    Ok(())
}
