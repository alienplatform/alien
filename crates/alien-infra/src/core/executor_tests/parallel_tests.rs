//! Tests for parallel execution of independent resources.
//!
//! These tests verify that resources without dependencies are processed concurrently.

use super::helpers::*;
use crate::error::Result;
use alien_core::{ResourceLifecycle, Stack};

/// Tests that independent resources (no dependencies between them) are processed in parallel.
/// Both function and storage should progress through their state machines concurrently.
#[tokio::test]
async fn test_independent_resources_run_in_parallel() -> Result<()> {
    let func1 = test_function("func1");
    let store1 = test_storage("store1");

    let stack = Stack::new("parallel-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["func1", "store1"]);
    Ok(())
}

/// Tests that multiple functions without dependencies run in parallel.
#[tokio::test]
async fn test_multiple_independent_functions() -> Result<()> {
    let func1 = test_function("func1");
    let func2 = test_function("func2");
    let func3 = test_function("func3");

    let stack = Stack::new("multi-func-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .add(func2.clone(), ResourceLifecycle::Live)
        .add(func3.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["func1", "func2", "func3"]);
    Ok(())
}

/// Tests that multiple storages without dependencies run in parallel.
#[tokio::test]
async fn test_multiple_independent_storages() -> Result<()> {
    let store1 = test_storage("store1");
    let store2 = test_storage("store2");
    let store3 = test_storage("store3");

    let stack = Stack::new("multi-storage-test".to_owned())
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add(store2.clone(), ResourceLifecycle::Frozen)
        .add(store3.clone(), ResourceLifecycle::Frozen)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["store1", "store2", "store3"]);
    Ok(())
}

/// Tests a mix of functions and storages all independent.
#[tokio::test]
async fn test_mixed_independent_resources() -> Result<()> {
    let func1 = test_function("func1");
    let func2 = test_function("func2");
    let store1 = test_storage("store1");
    let store2 = test_storage("store2");

    let stack = Stack::new("mixed-test".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .add(func2.clone(), ResourceLifecycle::Live)
        .add(store1.clone(), ResourceLifecycle::Frozen)
        .add(store2.clone(), ResourceLifecycle::Frozen)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    let final_state = run_to_synced(&executor, state).await?;

    assert_all_running(&final_state, &["func1", "func2", "store1", "store2"]);
    Ok(())
}
