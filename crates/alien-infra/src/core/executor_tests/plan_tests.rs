//! Tests for the plan() method that calculates diffs.

use super::helpers::*;
use crate::error::Result;
use alien_core::{ResourceLifecycle, ResourceStatus, Stack};

/// Tests that plan identifies resources to create.
#[tokio::test]
async fn test_plan_identifies_creates() -> Result<()> {
    let func1 = test_function("func1");
    let func2 = test_function("func2");

    let stack = Stack::new("plan-create-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .add(func2.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let plan = executor.plan(&state)?;

    assert_eq!(plan.creates.len(), 2, "Should identify 2 creates");
    assert!(plan.creates.contains(&"func1".to_string()));
    assert!(plan.creates.contains(&"func2".to_string()));
    assert!(plan.updates.is_empty(), "Should have no updates");
    assert!(plan.deletes.is_empty(), "Should have no deletes");
    Ok(())
}

/// Tests that plan identifies resources to delete.
#[tokio::test]
async fn test_plan_identifies_deletes() -> Result<()> {
    let func1 = test_function("func1");

    let stack_v1 = Stack::new("plan-delete-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor_v1, state).await?;

    let empty_stack = Stack::new("plan-delete-test".to_owned()).build();
    let executor_v2 = new_executor(&empty_stack)?;

    let plan = executor_v2.plan(&state_after_create)?;

    assert!(plan.creates.is_empty(), "Should have no creates");
    assert!(plan.updates.is_empty(), "Should have no updates");
    assert_eq!(plan.deletes.len(), 1, "Should identify 1 delete");
    assert!(plan.deletes.contains(&"func1".to_string()));
    Ok(())
}

/// Tests that plan identifies resources to update.
#[tokio::test]
async fn test_plan_identifies_updates() -> Result<()> {
    let func1_v1 = test_function_with_image("func1", "image-v1");

    let stack_v1 = Stack::new("plan-update-test".to_owned())
        .add(func1_v1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor_v1, state).await?;

    let func1_v2 = test_function_with_image("func1", "image-v2");

    let stack_v2 = Stack::new("plan-update-test".to_owned())
        .add(func1_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let plan = executor_v2.plan(&state_after_create)?;

    assert!(plan.creates.is_empty(), "Should have no creates");
    assert_eq!(plan.updates.len(), 1, "Should identify 1 update");
    assert!(plan.updates.contains_key("func1"));
    assert!(plan.deletes.is_empty(), "Should have no deletes");
    Ok(())
}

/// Tests that plan with no changes returns empty.
#[tokio::test]
async fn test_plan_no_changes() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("plan-no-change-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor, state).await?;

    let plan = executor.plan(&state_after_create)?;

    assert!(plan.creates.is_empty(), "Should have no creates");
    assert!(plan.updates.is_empty(), "Should have no updates");
    assert!(plan.deletes.is_empty(), "Should have no deletes");
    Ok(())
}

/// Tests that plan respects lifecycle filter.
#[tokio::test]
async fn test_plan_respects_lifecycle_filter() -> Result<()> {
    let store1 = test_storage("store1");
    let func1 = test_function("func1");

    let stack = Stack::new("plan-filter-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    let frozen_executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Frozen])?;
    let state = new_test_state();

    let plan = frozen_executor.plan(&state)?;

    assert_eq!(plan.creates.len(), 1, "Should only create Frozen resource");
    assert!(plan.creates.contains(&"store1".to_string()));
    assert!(!plan.creates.contains(&"func1".to_string()));
    Ok(())
}

/// Tests plan with combined creates, updates, and deletes.
#[tokio::test]
async fn test_plan_basic_changes() -> Result<()> {
    let func_a_v1 = test_function_with_image("func-a", "image-a-v1");
    let func_b = test_function("func-b");

    let stack_v1 = Stack::new("plan-combined-test".to_owned())
        .add(func_a_v1.clone(), ResourceLifecycle::Live)
        .add(func_b.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_v1 = run_to_synced(&executor_v1, state).await?;

    // v2: Update func-a, delete func-b, create func-c
    let func_a_v2 = test_function_with_image("func-a", "image-a-v2");
    let func_c = test_function("func-c");

    let stack_v2 = Stack::new("plan-combined-test".to_owned())
        .add(func_a_v2.clone(), ResourceLifecycle::Live)
        .add(func_c.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let plan = executor_v2.plan(&state_after_v1)?;

    assert_eq!(plan.creates.len(), 1, "Should create 1");
    assert!(plan.creates.contains(&"func-c".to_string()));

    assert_eq!(plan.updates.len(), 1, "Should update 1");
    assert!(plan.updates.contains_key("func-a"));

    assert_eq!(plan.deletes.len(), 1, "Should delete 1");
    assert!(plan.deletes.contains(&"func-b".to_string()));

    Ok(())
}

/// Tests plan handles provision failed resources with config change.
/// ProvisionFailed resources with config changes go to creates (restart creation).
#[tokio::test]
async fn test_plan_provision_failed_with_config_change() -> Result<()> {
    // State has func1 with image-v1 that failed
    let func1_v1 = test_function_with_image("func1", "image-v1");
    let mut state = new_test_state();
    let mut failed_state = create_provision_failed_function_state("func1");
    // Manually set the config to v1
    failed_state.config = alien_core::Resource::new(func1_v1);
    state.resources.insert("func1".to_string(), failed_state);

    // Desired stack has func1 with image-v2 (different config)
    let func1_v2 = test_function_with_image("func1", "image-v2");
    let stack = Stack::new("plan-provision-failed-test".to_owned())
        .add(func1_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let plan = executor.plan(&state)?;

    // ProvisionFailed resource with config change goes to creates (restart)
    assert!(
        plan.creates.contains(&"func1".to_string()),
        "ProvisionFailed resource with config change should be marked for create (restart)"
    );

    Ok(())
}

/// Tests plan handles update failed resources with config change.
/// UpdateFailed resources with config changes go to updates.
#[tokio::test]
async fn test_plan_update_failed_with_config_change() -> Result<()> {
    // State has func1 with image-v1 that failed during update
    let func1_v1 = test_function_with_image("func1", "image-v1");
    let mut state = new_test_state();
    let mut failed_state = create_update_failed_function_state("func1");
    failed_state.config = alien_core::Resource::new(func1_v1);
    state.resources.insert("func1".to_string(), failed_state);

    // Desired stack has func1 with image-v2 (different config)
    let func1_v2 = test_function_with_image("func1", "image-v2");
    let stack = Stack::new("plan-update-failed-test".to_owned())
        .add(func1_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let plan = executor.plan(&state)?;

    // UpdateFailed resource with config change goes to updates
    assert!(
        plan.updates.contains_key("func1"),
        "UpdateFailed resource with config change should be marked for update"
    );

    Ok(())
}

/// Tests plan ignores deleting resources (waits for deletion to complete).
#[tokio::test]
async fn test_plan_on_deleting_resource() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("plan-deleting-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    // Create state with resource in Deleting status
    let mut state = new_test_state();
    let deleting_state = create_deleting_function_state("func1");
    state.resources.insert("func1".to_string(), deleting_state);

    let executor = new_executor(&stack)?;
    let plan = executor.plan(&state)?;

    // Resources in Deleting status are ignored (wait for stabilization)
    // No creates, updates, or deletes for in-progress operations
    assert!(
        !plan.updates.contains_key("func1") && !plan.creates.contains(&"func1".to_string()),
        "Deleting resource should be ignored until stable"
    );

    Ok(())
}

/// Tests plan skips deleted resources not in desired stack.
#[tokio::test]
async fn test_plan_skip_already_deleted() -> Result<()> {
    let empty_stack = Stack::new("plan-deleted-test".to_owned()).build();

    let mut state = new_test_state();
    let deleted_state = create_deleted_function_state("func1");
    state.resources.insert("func1".to_string(), deleted_state);

    let executor = new_executor(&empty_stack)?;
    let plan = executor.plan(&state)?;

    // Already deleted, not in desired stack - no action needed
    assert!(plan.creates.is_empty());
    assert!(plan.updates.is_empty());
    assert!(plan.deletes.is_empty());

    Ok(())
}

/// Tests plan for recreating a deleted resource.
#[tokio::test]
async fn test_plan_recreate_deleted_resource() -> Result<()> {
    let func1 = test_function("func1");

    let stack = Stack::new("plan-recreate-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .build();

    // Create state with deleted resource
    let mut state = new_test_state();
    let deleted_state = create_deleted_function_state("func1");
    state.resources.insert("func1".to_string(), deleted_state);

    let executor = new_executor(&stack)?;
    let plan = executor.plan(&state)?;

    // Deleted resource in desired stack should be recreated
    assert!(
        plan.creates.contains(&"func1".to_string()),
        "Deleted resource should be marked for create"
    );

    Ok(())
}

/// Tests plan only includes creates.
#[tokio::test]
async fn test_plan_only_create() -> Result<()> {
    let func1 = test_function("func1");
    let func2 = test_function("func2");

    let stack = Stack::new("plan-only-create-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .add(func2.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let plan = executor.plan(&state)?;

    assert_eq!(plan.creates.len(), 2);
    assert!(plan.updates.is_empty());
    assert!(plan.deletes.is_empty());

    Ok(())
}

/// Tests plan only includes deletes.
#[tokio::test]
async fn test_plan_only_delete() -> Result<()> {
    // Create a state with running resources
    let state = create_state_with_running(vec![
        ("func1", ResourceLifecycle::Live),
        ("func2", ResourceLifecycle::Live),
    ]);

    let empty_stack = Stack::new("plan-only-delete-test".to_owned()).build();
    let executor = new_executor(&empty_stack)?;

    let plan = executor.plan(&state)?;

    assert!(plan.creates.is_empty());
    assert!(plan.updates.is_empty());
    assert_eq!(plan.deletes.len(), 2);
    assert!(plan.deletes.contains(&"func1".to_string()));
    assert!(plan.deletes.contains(&"func2".to_string()));

    Ok(())
}

/// Tests plan schedules deletion for failed resources not in desired stack.
#[tokio::test]
async fn test_plan_delete_on_failed() -> Result<()> {
    // State has func1 that failed during provisioning
    let mut state = new_test_state();
    let failed_state = create_provision_failed_function_state("func1");
    state.resources.insert("func1".to_string(), failed_state);

    // Desired stack is empty (implicitly deleting func1)
    let empty_stack = Stack::new("plan-delete-failed-test".to_owned()).build();

    let executor = new_executor(&empty_stack)?;
    let plan = executor.plan(&state)?;

    // Failed resource not in desired stack should be marked for delete
    assert!(
        plan.deletes.contains(&"func1".to_string()),
        "ProvisionFailed resource not in desired stack should be marked for delete"
    );
    assert!(plan.creates.is_empty());
    assert!(plan.updates.is_empty());

    Ok(())
}

/// Tests that a config change during Provisioning plans a delete (not an update or ignore).
#[tokio::test]
async fn test_plan_provisioning_with_config_change_plans_delete() -> Result<()> {
    let func1_v1 = test_function_with_image("func1", "image-v1");
    let mut state = new_test_state();

    // Place func1 in Provisioning status with image-v1 config
    let mut provisioning_state = create_provisioning_function_state("func1");
    provisioning_state.config = alien_core::Resource::new(func1_v1);
    state
        .resources
        .insert("func1".to_string(), provisioning_state);

    // Desired stack has func1 with image-v2 (config changed)
    let func1_v2 = test_function_with_image("func1", "image-v2");
    let stack = Stack::new("plan-provisioning-config-change-test".to_owned())
        .add(func1_v2, ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let plan = executor.plan(&state)?;

    assert!(
        plan.deletes.contains(&"func1".to_string()),
        "Config change during Provisioning should plan a delete"
    );
    assert!(
        plan.creates.is_empty(),
        "Should not plan a create yet (create happens after delete completes)"
    );
    assert!(plan.updates.is_empty(), "Should not plan an update");

    Ok(())
}

/// Tests plan only includes updates.
#[tokio::test]
async fn test_plan_only_update() -> Result<()> {
    let func1_v1 = test_function_with_image("func1", "image-v1");

    let stack_v1 = Stack::new("plan-only-update-test".to_owned())
        .add(func1_v1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_create = run_to_synced(&executor_v1, state).await?;

    let func1_v2 = test_function_with_image("func1", "image-v2");

    let stack_v2 = Stack::new("plan-only-update-test".to_owned())
        .add(func1_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let plan = executor_v2.plan(&state_after_create)?;

    assert!(plan.creates.is_empty());
    assert_eq!(plan.updates.len(), 1);
    assert!(plan.deletes.is_empty());

    Ok(())
}
