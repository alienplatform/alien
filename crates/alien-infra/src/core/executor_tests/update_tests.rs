//! Tests for resource update flows and config changes.

use super::helpers::*;
use crate::error::Result;
use alien_core::{
    Resource, ResourceLifecycle, ResourceRef, ResourceStatus, Stack, StackResourceState,
};

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

/// Refresh observes the persisted deployment; it must not turn desired release
/// drift into an update.
#[tokio::test]
async fn test_refresh_does_not_plan_config_changes() -> Result<()> {
    let func_v1 = test_function_with_image("func1", "image-v1");
    let stack_v1 = Stack::new("refresh-test".to_owned())
        .add(func_v1.clone(), ResourceLifecycle::Live)
        .build();
    let state = run_to_synced(&new_executor(&stack_v1)?, new_test_state()).await?;

    let stack_v2 = Stack::new("refresh-test".to_owned())
        .add(
            test_function_with_image("func1", "image-v2"),
            ResourceLifecycle::Live,
        )
        .add(test_storage("new-storage"), ResourceLifecycle::Frozen)
        .build();
    let refreshed = new_executor(&stack_v2)?.refresh(state).await?.next_state;

    assert_eq!(
        refreshed.resources.len(),
        1,
        "refresh must not create desired resources missing from persisted state"
    );
    let resource = refreshed.resources.get("func1").unwrap();
    assert_eq!(resource.status, ResourceStatus::Running);
    assert_eq!(
        resource.config,
        Resource::new(func_v1),
        "refresh must retain the deployed config instead of applying desired drift"
    );

    Ok(())
}

#[tokio::test]
async fn imported_continuation_preserves_complete_truthful_state() -> Result<()> {
    let dependency = test_storage("dependency");
    let dependent = test_storage("dependent");
    let stack = Stack::new("imported-continuation".to_owned())
        .add(dependency, ResourceLifecycle::Frozen)
        .add_with_dependencies(
            dependent,
            ResourceLifecycle::Frozen,
            vec![ResourceRef::new("storage".into(), "dependency")],
        )
        .build();
    let executor = new_executor(&stack)?;
    let state = run_to_synced(&executor, new_test_state()).await?;

    let continued = executor.continue_imported(state).await?.next_state;

    assert_eq!(
        continued.resources["dependent"].dependencies,
        vec![ResourceRef::new("storage".into(), "dependency")]
    );
    assert!(continued
        .resources
        .values()
        .all(|resource| resource.status == ResourceStatus::Running));
    Ok(())
}

#[tokio::test]
async fn imported_continuation_refuses_to_initialize_pending_resource() -> Result<()> {
    let storage = test_storage("assets");
    let stack = Stack::new("imported-pending".to_owned())
        .add(storage.clone(), ResourceLifecycle::Frozen)
        .build();
    let executor = new_executor(&stack)?;
    let mut state = new_test_state();
    state.resources.insert(
        "assets".to_string(),
        StackResourceState::new_pending(
            "storage".to_string(),
            Resource::new(storage),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        ),
    );

    let error = executor
        .continue_imported(state)
        .await
        .expect_err("imported Pending state must not initialize a controller");

    assert!(error
        .message
        .contains("cannot continue from status Pending"));
    Ok(())
}

#[tokio::test]
async fn imported_continuation_refuses_missing_setup_resource() -> Result<()> {
    let stack = Stack::new("imported-missing".to_owned())
        .add(test_storage("assets"), ResourceLifecycle::Frozen)
        .build();

    let error = new_executor(&stack)?
        .continue_imported(new_test_state())
        .await
        .expect_err("an incomplete setup handoff must fail");

    assert!(error.message.contains("missing resource 'assets'"));
    Ok(())
}

/// Tests that config changes while a resource is still provisioning do not
/// interrupt the in-flight create. The update should happen after create reaches
/// a stable state.
#[tokio::test]
async fn test_config_change_during_provisioning_waits_for_stable_state() -> Result<()> {
    let func_v1 = test_function_with_image("func1", "image-v1");

    let stack_v1 = Stack::new("provisioning-config-change-test".to_owned())
        .add(func_v1, ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let mut state = new_test_state();

    for _ in 0..3 {
        state = executor_v1.step(state).await?.next_state;
        if get_status(&state, "func1") == Some(ResourceStatus::Provisioning) {
            break;
        }
    }

    assert_eq!(
        get_status(&state, "func1"),
        Some(ResourceStatus::Provisioning)
    );

    let func_v2 = test_function_with_image("func1", "image-v2");
    let stack_v2 = Stack::new("provisioning-config-change-test".to_owned())
        .add(func_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let next_state = executor_v2.step(state).await?.next_state;

    assert_eq!(
        get_status(&next_state, "func1"),
        Some(ResourceStatus::Provisioning),
        "config drift during provisioning must not transition to delete"
    );

    let final_state = run_to_synced(&executor_v2, next_state).await?;
    let final_resource = final_state.resources.get("func1").unwrap();

    assert_eq!(
        final_resource.status,
        ResourceStatus::Running,
        "resource should finish create and reconcile to running"
    );
    assert_eq!(
        final_resource.config,
        Resource::new(func_v2),
        "stable resource should reconcile to the latest desired config"
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
