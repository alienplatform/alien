//! Tests for dependency ordering during creation and updates.

use std::time::Duration;

use super::helpers::*;
use crate::error::Result;
use alien_core::{
    Function, Resource, ResourceLifecycle, ResourceRef, ResourceStatus, Stack, Storage,
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
            vec![ResourceRef::new(Function::RESOURCE_TYPE, "func-b")],
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
            vec![ResourceRef::new(Function::RESOURCE_TYPE, "func-b")],
        )
        .add_with_dependencies(
            func_b,
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Function::RESOURCE_TYPE, "func-a")],
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
                ResourceRef::new(Function::RESOURCE_TYPE, "func-b"),
                ResourceRef::new(Function::RESOURCE_TYPE, "func-c"),
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
/// - An independent Function.
/// - A Storage resource.
/// - A Function dependent on the Storage resource.
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
