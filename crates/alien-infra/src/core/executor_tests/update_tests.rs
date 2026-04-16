//! Tests for resource update flows and config changes.

use super::helpers::*;
use crate::error::Result;
use alien_core::{ResourceLifecycle, ResourceStatus, Stack};

/// Tests that a config change triggers an update.
#[tokio::test]
async fn test_config_change_triggers_update() -> Result<()> {
    let func1_v1 = test_function_with_image("func1", "image-v1");

    let stack_v1 = Stack::new("update-test".to_owned())
        .add(func1_v1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_v1 = run_to_synced(&executor_v1, state).await?;

    assert_eq!(
        get_status(&state_after_v1, "func1"),
        Some(ResourceStatus::Running)
    );

    let func1_v2 = test_function_with_image("func1", "image-v2");

    let stack_v2 = Stack::new("update-test".to_owned())
        .add(func1_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_after_v1).await?;

    assert_eq!(
        get_status(&final_state, "func1"),
        Some(ResourceStatus::Running)
    );
    Ok(())
}

/// Tests adding a new resource to an existing stack.
#[tokio::test]
async fn test_add_resource_to_existing_stack() -> Result<()> {
    let func_a = test_function("func-a");

    let stack_v1 = Stack::new("add-resource-test".to_owned())
        .add(func_a.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_v1 = run_to_synced(&executor_v1, state).await?;

    assert_all_running(&state_after_v1, &["func-a"]);

    let func_b = test_function("func-b");

    let stack_v2 = Stack::new("add-resource-test".to_owned())
        .add(func_a.clone(), ResourceLifecycle::Live)
        .add(func_b.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_after_v1).await?;

    assert_all_running(&final_state, &["func-a", "func-b"]);
    Ok(())
}

/// Tests removing a resource from an existing stack.
#[tokio::test]
async fn test_remove_resource_from_existing_stack() -> Result<()> {
    let func_a = test_function("func-a");
    let func_b = test_function("func-b");

    let stack_v1 = Stack::new("remove-resource-test".to_owned())
        .add(func_a.clone(), ResourceLifecycle::Live)
        .add(func_b.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_v1 = run_to_synced(&executor_v1, state).await?;

    assert_all_running(&state_after_v1, &["func-a", "func-b"]);

    let stack_v2 = Stack::new("remove-resource-test".to_owned())
        .add(func_a.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_after_v1).await?;

    assert_eq!(
        get_status(&final_state, "func-a"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&final_state, "func-b"),
        Some(ResourceStatus::Deleted)
    );
    Ok(())
}

/// Tests combined add, update, and remove in single stack change.
#[tokio::test]
async fn test_combined_add_update_remove() -> Result<()> {
    let func_a_v1 = test_function_with_image("func-a", "image-a-v1");
    let func_b = test_function("func-b");

    let stack_v1 = Stack::new("combined-test".to_owned())
        .add(func_a_v1.clone(), ResourceLifecycle::Live)
        .add(func_b.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();
    let state_after_v1 = run_to_synced(&executor_v1, state).await?;

    assert_all_running(&state_after_v1, &["func-a", "func-b"]);

    let func_a_v2 = test_function_with_image("func-a", "image-a-v2");
    let func_c = test_function("func-c");

    let stack_v2 = Stack::new("combined-test".to_owned())
        .add(func_a_v2.clone(), ResourceLifecycle::Live)
        .add(func_c.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_after_v1).await?;

    assert_eq!(
        get_status(&final_state, "func-a"),
        Some(ResourceStatus::Running)
    );
    assert_eq!(
        get_status(&final_state, "func-b"),
        Some(ResourceStatus::Deleted)
    );
    assert_eq!(
        get_status(&final_state, "func-c"),
        Some(ResourceStatus::Running)
    );
    Ok(())
}
