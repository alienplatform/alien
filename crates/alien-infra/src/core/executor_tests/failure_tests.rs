//! Tests for failure handling, retries, and recovery scenarios.

use std::collections::HashMap;
use std::time::Duration;

use super::helpers::*;
use crate::core::state_utils::{StackResourceStateExt, StackStateExt};
use crate::core::StackExecutor;
use crate::error::Result;
use crate::function::{TestFunctionController, TestFunctionState};
use alien_core::{
    Function, FunctionCode, Platform, Resource, ResourceLifecycle, ResourceRef, ResourceStatus,
    Stack, StackSettings, StackState, Storage,
};

/// Helper to create a function that will fail on first attempt (memory > 4GB).
fn failing_function(id: &str) -> Function {
    Function::new(id.to_string())
        .code(FunctionCode::Image {
            image: format!("image-{}", id),
        })
        .memory_mb(5120) // > 4096 will fail
        .permissions("execution".to_string())
        .build()
}

/// Helper to create a function with specific memory.
fn function_with_memory(id: &str, memory_mb: u32) -> Function {
    Function::new(id.to_string())
        .code(FunctionCode::Image {
            image: format!("image-{}", id),
        })
        .memory_mb(memory_mb)
        .permissions("execution".to_string())
        .build()
}

/// Helper to create a function that fails N times before succeeding.
fn retryable_function(id: &str, fail_count: u32) -> Function {
    Function::new(id.to_string())
        .code(FunctionCode::Image {
            image: format!("image-{}", id),
        })
        .memory_mb(1024)
        .environment(HashMap::from([(
            "SIMULATE_RETRYABLE_FAILURE_COUNT".to_string(),
            fail_count.to_string(),
        )]))
        .permissions("execution".to_string())
        .build()
}

/// Tests automatic recovery when config is updated after failure.
#[tokio::test]
async fn test_failure_auto_recovery_on_config_update() -> Result<()> {
    // v1: Function with high memory that will fail
    let fail_func_v1 = failing_function("fail-func");
    let dep_func = test_function("dep-func");

    let stack_v1 = Stack::new("auto-recovery-stack".to_owned())
        .add(fail_func_v1.clone(), ResourceLifecycle::Live)
        .add_with_dependencies(
            dep_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Function::RESOURCE_TYPE, "fail-func")],
        )
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let mut state = new_test_state();

    // Run until fail-func fails
    let max_steps = 10;
    for _ in 0..max_steps {
        let step_result = executor_v1.step(state).await?;
        state = step_result.next_state;
        if get_status(&state, "fail-func") == Some(ResourceStatus::ProvisionFailed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    // Verify failure
    assert_eq!(
        get_status(&state, "fail-func"),
        Some(ResourceStatus::ProvisionFailed),
        "fail-func should be ProvisionFailed"
    );
    assert!(
        state.resources.get("fail-func").unwrap().error.is_some(),
        "fail-func should have an error"
    );
    assert_eq!(
        get_status(&state, "dep-func"),
        Some(ResourceStatus::Pending),
        "dep-func should be Pending (blocked by failed dep)"
    );

    // v2: Same function with corrected memory
    let fail_func_v2 = function_with_memory("fail-func", 2048);

    let stack_v2 = Stack::new("auto-recovery-stack".to_owned())
        .add(fail_func_v2.clone(), ResourceLifecycle::Live)
        .add_with_dependencies(
            dep_func.clone(),
            ResourceLifecycle::Live,
            vec![ResourceRef::new(Function::RESOURCE_TYPE, "fail-func")],
        )
        .build();

    let executor_v2 = new_executor(&stack_v2)?;

    // Run v2 executor on failed state - should auto-recover
    let final_state = run_to_synced(&executor_v2, state).await?;

    assert_eq!(
        get_status(&final_state, "fail-func"),
        Some(ResourceStatus::Running),
        "fail-func should be Running after auto-recovery"
    );
    assert_eq!(
        get_status(&final_state, "dep-func"),
        Some(ResourceStatus::Running),
        "dep-func should be Running after auto-recovery"
    );

    Ok(())
}

/// Tests manual retry after failure.
#[tokio::test]
async fn test_failure_manual_retry() -> Result<()> {
    // Function that fails exactly 10 times (our max retries)
    let fail_func = retryable_function("retry-func", 10);

    let stack = Stack::new("manual-retry-stack".to_owned())
        .add(fail_func.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let mut state = new_test_state();

    // Run until function fails after max retries
    let max_steps = 50;
    for _ in 0..max_steps {
        if get_status(&state, "retry-func") == Some(ResourceStatus::ProvisionFailed) {
            break;
        }
        let step_result = executor.step(state).await?;
        state = step_result.next_state;
        std::thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(
        get_status(&state, "retry-func"),
        Some(ResourceStatus::ProvisionFailed),
        "retry-func should be ProvisionFailed after max retries"
    );

    // Manual retry - reset the retry state
    state.retry_failed();

    // Run again - should succeed now since failure count is persisted
    let final_state = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&final_state, "retry-func"),
        Some(ResourceStatus::Running),
        "retry-func should be Running after manual retry"
    );

    Ok(())
}

/// Tests transient failure with automatic retry.
#[tokio::test]
async fn test_transient_failure_auto_retry() -> Result<()> {
    // Function that fails 2 times then succeeds (within retry limit)
    let fail_func = retryable_function("transient-func", 2);

    let stack = Stack::new("transient-retry-stack".to_owned())
        .add(fail_func.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    // Run to completion - should auto-retry and succeed
    let final_state = run_to_synced(&executor, state).await?;

    assert_eq!(
        get_status(&final_state, "transient-func"),
        Some(ResourceStatus::Running),
        "transient-func should be Running after auto-retry"
    );

    // The important thing is that it recovered from transient failure
    Ok(())
}

/// Tests that retry attempt counter resets on config change.
#[tokio::test]
async fn test_retry_attempt_counter_reset_on_config_change() -> Result<()> {
    let func_v1 = retryable_function("reset-func", 10); // Will fail more than max retries

    let stack_v1 = Stack::new("counter-reset-stack".to_owned())
        .add(func_v1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let mut state = new_test_state();

    // Run until failure
    let max_steps = 50;
    for _ in 0..max_steps {
        if get_status(&state, "reset-func") == Some(ResourceStatus::ProvisionFailed) {
            break;
        }
        let step_result = executor_v1.step(state).await?;
        state = step_result.next_state;
        std::thread::sleep(Duration::from_millis(1));
    }

    // Verify failed with some retry attempts
    let resource_state = state.resources.get("reset-func").unwrap();
    assert_eq!(resource_state.status, ResourceStatus::ProvisionFailed);
    let failed_retry_attempt = resource_state.retry_attempt;
    assert!(failed_retry_attempt > 0, "Should have used retry attempts");

    // v2: Change config to fix the issue
    let func_v2 = test_function("reset-func"); // No failure simulation

    let stack_v2 = Stack::new("counter-reset-stack".to_owned())
        .add(func_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state).await?;

    // Should succeed with reset counter
    assert_eq!(
        get_status(&final_state, "reset-func"),
        Some(ResourceStatus::Running)
    );

    Ok(())
}

/// Tests failure state preservation across different failure types.
#[tokio::test]
async fn test_failure_state_preservation() -> Result<()> {
    let fail_func = failing_function("preserve-func");

    let stack = Stack::new("preserve-stack".to_owned())
        .add(fail_func.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let mut state = new_test_state();

    // Run until failure
    let max_steps = 10;
    for _ in 0..max_steps {
        if get_status(&state, "preserve-func") == Some(ResourceStatus::ProvisionFailed) {
            break;
        }
        let step_result = executor.step(state).await?;
        state = step_result.next_state;
        std::thread::sleep(Duration::from_millis(1));
    }

    // Verify failure info is preserved
    let resource_state = state.resources.get("preserve-func").unwrap();
    assert_eq!(resource_state.status, ResourceStatus::ProvisionFailed);
    assert!(resource_state.error.is_some(), "Should have error info");
    assert!(
        resource_state.last_failed_state.is_some(),
        "Should preserve last failed state"
    );

    Ok(())
}

/// Tests config drift during failure recovery.
/// ProvisionFailed with config change goes to creates (restart), not updates.
#[tokio::test]
async fn test_retry_with_config_drift() -> Result<()> {
    // Start with failing config
    let func_v1 = failing_function("drift-func");

    let stack_v1 = Stack::new("drift-stack".to_owned())
        .add(func_v1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let mut state = new_test_state();

    // Run until failure
    let max_steps = 10;
    for _ in 0..max_steps {
        if get_status(&state, "drift-func") == Some(ResourceStatus::ProvisionFailed) {
            break;
        }
        let step_result = executor_v1.step(state).await?;
        state = step_result.next_state;
        std::thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(
        get_status(&state, "drift-func"),
        Some(ResourceStatus::ProvisionFailed)
    );

    // Change config completely
    let func_v2 = test_function_with_image("drift-func", "completely-new-image");

    let stack_v2 = Stack::new("drift-stack".to_owned())
        .add(func_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;

    // Plan should show create (restart) for ProvisionFailed with config change
    let plan = executor_v2.plan(&state)?;
    assert!(
        plan.creates.contains(&"drift-func".to_string()),
        "Should detect config drift and mark for create (restart)"
    );

    // Run to completion
    let final_state = run_to_synced(&executor_v2, state).await?;

    assert_eq!(
        get_status(&final_state, "drift-func"),
        Some(ResourceStatus::Running)
    );

    Ok(())
}

/// Tests that retryable and non-retryable errors behave differently.
/// - Retryable errors go through multiple retry attempts before failing
/// - Non-retryable errors fail immediately (0 retry attempts)
/// - Both have `last_failed_state` for manual retry
/// - Different error codes for each type
#[tokio::test]
async fn test_retryable_vs_non_retryable_error_behavior() -> Result<()> {
    // Function with retryable failure (persistent failure that retries)
    let retryable_func = Function::new("retryable-func".to_string())
        .code(FunctionCode::Image {
            image: "image-retryable".to_string(),
        })
        .memory_mb(1024) // Normal memory
        .environment(HashMap::from([(
            "SIMULATE_PERSISTENT_FAILURE".to_string(),
            "true".to_string(),
        )]))
        .permissions("execution".to_string())
        .build();

    // Function with non-retryable failure (memory > 4GB fails immediately)
    let non_retryable_func = Function::new("non-retryable-func".to_string())
        .code(FunctionCode::Image {
            image: "image-non-retryable".to_string(),
        })
        .memory_mb(5120) // > 4096 will fail immediately with non-retryable error
        .permissions("execution".to_string())
        .build();

    let stack = Stack::new("retry-behavior-stack".to_owned())
        .add(retryable_func.clone(), ResourceLifecycle::Live)
        .add(non_retryable_func.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let mut state = new_test_state();

    // Run until both functions fail
    let max_steps = 50;
    let mut retryable_retry_attempts = Vec::new();
    let mut non_retryable_retry_attempts = Vec::new();

    for _ in 0..max_steps {
        let step_result = executor.step(state).await?;
        state = step_result.next_state;

        // Track retry attempts
        if let Some(retryable_state) = state.resources.get("retryable-func") {
            retryable_retry_attempts.push(retryable_state.retry_attempt);
        }
        if let Some(non_retryable_state) = state.resources.get("non-retryable-func") {
            non_retryable_retry_attempts.push(non_retryable_state.retry_attempt);
        }

        // Check if both have failed
        let retryable_failed =
            get_status(&state, "retryable-func") == Some(ResourceStatus::ProvisionFailed);
        let non_retryable_failed =
            get_status(&state, "non-retryable-func") == Some(ResourceStatus::ProvisionFailed);

        if retryable_failed && non_retryable_failed {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    // --- Verify retryable function went through multiple retries ---
    assert_eq!(
        get_status(&state, "retryable-func"),
        Some(ResourceStatus::ProvisionFailed),
        "retryable-func should be ProvisionFailed after max retries"
    );

    let retryable_final_state = state.resources.get("retryable-func").unwrap();
    // Should have reached max retry attempts (typically 5)
    assert!(
        retryable_final_state.retry_attempt >= 5,
        "retryable-func should have reached max retry attempts (got {})",
        retryable_final_state.retry_attempt
    );

    // --- Verify non-retryable function failed immediately (minimal retries) ---
    assert_eq!(
        get_status(&state, "non-retryable-func"),
        Some(ResourceStatus::ProvisionFailed),
        "non-retryable-func should be ProvisionFailed"
    );

    let non_retryable_final_state = state.resources.get("non-retryable-func").unwrap();
    // Non-retryable errors fail immediately with 0 retry attempts
    assert_eq!(
        non_retryable_final_state.retry_attempt, 0,
        "non-retryable-func should have 0 retry attempts (fails immediately)"
    );

    // --- Verify both have last_failed_state for manual retry ---
    assert!(
        retryable_final_state.last_failed_state.is_some(),
        "retryable-func should have last_failed_state"
    );
    assert!(
        non_retryable_final_state.last_failed_state.is_some(),
        "non-retryable-func should have last_failed_state"
    );

    // --- Verify the error types are different ---
    let retryable_error = retryable_final_state.error.as_ref().unwrap();
    let non_retryable_error = non_retryable_final_state.error.as_ref().unwrap();

    // Retryable error should be EXECUTION_STEP_FAILED
    assert_eq!(
        retryable_error.code, "EXECUTION_STEP_FAILED",
        "Retryable error should have EXECUTION_STEP_FAILED code"
    );

    // Non-retryable error should be RESOURCE_CONFIG_INVALID (memory limit exceeded)
    assert_eq!(
        non_retryable_error.code, "RESOURCE_CONFIG_INVALID",
        "Non-retryable error should have RESOURCE_CONFIG_INVALID code"
    );

    Ok(())
}

/// Tests manual retry with config change during failure.
#[tokio::test]
async fn test_manual_retry_with_config_change() -> Result<()> {
    let func_v1 = failing_function("change-func");

    let stack_v1 = Stack::new("change-stack".to_owned())
        .add(func_v1.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let mut state = new_test_state();

    // Run until failure
    let max_steps = 10;
    for _ in 0..max_steps {
        if get_status(&state, "change-func") == Some(ResourceStatus::ProvisionFailed) {
            break;
        }
        let step_result = executor_v1.step(state).await?;
        state = step_result.next_state;
        std::thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(
        get_status(&state, "change-func"),
        Some(ResourceStatus::ProvisionFailed)
    );

    // Manually reset retry state
    state.retry_failed();

    // Create fixed config
    let func_v2 = test_function("change-func");

    let stack_v2 = Stack::new("change-stack".to_owned())
        .add(func_v2.clone(), ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state).await?;

    assert_eq!(
        get_status(&final_state, "change-func"),
        Some(ResourceStatus::Running)
    );

    Ok(())
}

/// Tests retry counter isolation between resources.
/// Each resource maintains its own independent retry state.
#[tokio::test]
async fn test_retry_counter_isolation_between_resources() -> Result<()> {
    // Two functions with different failure behaviors
    // func1: Will fail with retryable errors (many retries)
    let func1 = Function::new("func1".to_string())
        .code(FunctionCode::Image {
            image: "image-func1".to_string(),
        })
        .memory_mb(1024)
        .environment(HashMap::from([(
            "SIMULATE_PERSISTENT_FAILURE".to_string(),
            "true".to_string(),
        )]))
        .permissions("execution".to_string())
        .build();

    // func2: Will succeed normally (no failures)
    let func2 = test_function("func2");

    let stack = Stack::new("isolation-stack".to_owned())
        .add(func1.clone(), ResourceLifecycle::Live)
        .add(func2.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let mut state = new_test_state();

    // Track retry attempts for both functions
    let mut func1_retry_attempts = Vec::new();
    let mut func2_retry_attempts = Vec::new();

    let max_steps = 50;
    for step in 0..max_steps {
        let step_result = executor.step(state).await?;
        state = step_result.next_state;

        // Track retry attempts
        if let Some(func1_state) = state.resources.get("func1") {
            func1_retry_attempts.push((step, func1_state.retry_attempt));
        }
        if let Some(func2_state) = state.resources.get("func2") {
            func2_retry_attempts.push((step, func2_state.retry_attempt));
        }

        // Stop when func1 has failed and func2 has completed
        let func1_failed = get_status(&state, "func1") == Some(ResourceStatus::ProvisionFailed);
        let func2_done = get_status(&state, "func2") == Some(ResourceStatus::Running);

        if func1_failed && func2_done {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    // Verify func1 failed with retry attempts
    let func1_state = state.resources.get("func1").unwrap();
    assert_eq!(
        func1_state.status,
        ResourceStatus::ProvisionFailed,
        "func1 should be ProvisionFailed"
    );
    assert!(
        func1_state.retry_attempt >= 5,
        "func1 should have used multiple retry attempts"
    );

    // Verify func2 succeeded with 0 retry attempts
    let func2_state = state.resources.get("func2").unwrap();
    assert_eq!(
        func2_state.status,
        ResourceStatus::Running,
        "func2 should be Running"
    );
    assert_eq!(
        func2_state.retry_attempt, 0,
        "func2 should have 0 retry attempts (succeeded without retries)"
    );

    // Verify retry counters were isolated - func2's success didn't affect func1's retries
    let func2_max_retry = func2_retry_attempts
        .iter()
        .map(|(_, r)| *r)
        .max()
        .unwrap_or(0);
    assert_eq!(
        func2_max_retry, 0,
        "func2 should never have non-zero retry attempts"
    );

    Ok(())
}

/// Tests that transient failure counter resets after successful completion.
#[tokio::test]
async fn test_transient_failure_counter_reset_after_success() -> Result<()> {
    // Function that fails 2 times then succeeds
    let func = retryable_function("transient-reset-func", 2);

    let stack = Stack::new("transient-reset-stack".to_owned())
        .add(func.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let state = new_test_state();

    // Run to completion
    let final_state = run_to_synced(&executor, state).await?;

    // Verify successful completion
    assert_eq!(
        get_status(&final_state, "transient-reset-func"),
        Some(ResourceStatus::Running)
    );

    // After success, retry_attempt should be 0
    let resource_state = final_state.resources.get("transient-reset-func").unwrap();
    assert_eq!(
        resource_state.retry_attempt, 0,
        "Successful resource should have retry_attempt = 0"
    );

    Ok(())
}

/// Tests that manual retry properly resets retry state and resumes from last failed state.
#[tokio::test]
async fn test_manual_retry_resets_state_properly() -> Result<()> {
    let func = retryable_function("manual-reset-func", 10); // Will fail more than max

    let stack = Stack::new("manual-reset-stack".to_owned())
        .add(func.clone(), ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let mut state = new_test_state();

    // Run until failure
    let max_steps = 50;
    for _ in 0..max_steps {
        if get_status(&state, "manual-reset-func") == Some(ResourceStatus::ProvisionFailed) {
            break;
        }
        let step_result = executor.step(state).await?;
        state = step_result.next_state;
        std::thread::sleep(Duration::from_millis(1));
    }

    // Verify failed state
    let failed_state = state.resources.get("manual-reset-func").unwrap();
    assert_eq!(failed_state.status, ResourceStatus::ProvisionFailed);
    assert!(failed_state.retry_attempt > 0, "Should have retry attempts");
    assert!(failed_state.error.is_some(), "Should have error");
    assert!(
        failed_state.last_failed_state.is_some(),
        "Should have last_failed_state"
    );

    // Perform manual retry
    state.retry_failed();

    // Verify retry reset the state properly
    let retried_state = state.resources.get("manual-reset-func").unwrap();
    assert_eq!(
        retried_state.retry_attempt, 0,
        "Manual retry should reset retry_attempt to 0"
    );
    assert!(
        retried_state.error.is_none(),
        "Manual retry should clear error"
    );
    assert_eq!(
        retried_state.status,
        ResourceStatus::Provisioning,
        "Manual retry should set status to Provisioning"
    );
    // last_failed_state should be consumed during retry
    assert!(
        retried_state.last_failed_state.is_none(),
        "Manual retry should consume last_failed_state"
    );

    Ok(())
}

/// Creates a Function configured to use Stay-based polling that never advances,
/// causing the macro's max_times exhaustion path to be triggered.
fn stay_exhausting_function(id: &str, max_times: u32) -> Function {
    Function::new(id.to_string())
        .code(FunctionCode::Image {
            image: format!("image-{}", id),
        })
        .memory_mb(1024)
        .environment(HashMap::from([(
            "SIMULATE_STAY_EXHAUSTION".to_string(),
            max_times.to_string(),
        )]))
        .permissions("execution".to_string())
        .build()
}

/// Test B: Stay exhaustion surfaces an error and saves lastFailedState (Bug 1 fix).
///
/// Before the fix the macro silently transitioned to the failure terminal state and
/// returned Ok(None), so the executor never saved lastFailedState. After the fix it
/// returns Err(PollingTimeout), which forces the executor to save lastFailedState at
/// the polling handler state.
#[tokio::test]
async fn test_stay_exhaustion_saves_last_failed_state() -> Result<()> {
    let func = stay_exhausting_function("stay-func", 3);

    let stack = Stack::new("stay-stack".to_owned())
        .add(func, ResourceLifecycle::Live)
        .build();

    let executor = new_executor(&stack)?;
    let mut state = new_test_state();

    for _ in 0..50 {
        if get_status(&state, "stay-func") == Some(ResourceStatus::ProvisionFailed) {
            break;
        }
        let step_result = executor.step(state).await?;
        state = step_result.next_state;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    assert_eq!(
        get_status(&state, "stay-func"),
        Some(ResourceStatus::ProvisionFailed),
        "stay-func should be ProvisionFailed after Stay exhaustion"
    );

    let resource = state.resources.get("stay-func").unwrap();

    // Bug 1 fix: error must be present — exhaustion should surface, not be silently swallowed.
    assert!(
        resource.error.is_some(),
        "Stay exhaustion must record an error on the resource"
    );
    assert_eq!(
        resource.error.as_ref().unwrap().code,
        "RESOURCE_POLLING_TIMEOUT",
        "error code must be RESOURCE_POLLING_TIMEOUT"
    );

    // Bug 1 fix: lastFailedState must be saved so retry can resume from the polling handler.
    assert!(
        resource.last_failed_state.is_some(),
        "lastFailedState must be saved when Stay is exhausted"
    );

    // The lastFailedState must hold the controller at the polling handler, not at the
    // terminal failure state. (internal_state correctly holds the terminal CreateFailed.)
    let saved: TestFunctionController =
        serde_json::from_value(resource.last_failed_state.as_ref().unwrap().clone())
            .expect("last_failed_state must deserialize to TestFunctionController");
    assert_eq!(
        saved.state,
        TestFunctionState::CreateFunctionPolling,
        "lastFailedState must capture the polling state, not the failure terminal"
    );

    Ok(())
}

/// Config change during Provisioning triggers delete-then-recreate with new config.
///
/// Flow:
/// 1. One step runs with image-v1 → func1 enters Provisioning (CreateStart executed).
/// 2. We switch to image-v2 executor — plan() detects the config change and plans a delete.
/// 3. step() transitions func1 to DeleteStart (unconditional, from Provisioning).
/// 4. Executor runs to completion: delete finishes → func1 is recreated with image-v2.
/// 5. Final state: func1 is Running with image-v2 config.
#[tokio::test]
async fn test_config_change_during_provisioning_recreates_with_new_config() -> Result<()> {
    let func1_v1 = test_function_with_image("func1", "image-v1");

    let stack_v1 = Stack::new("provisioning-recreate-test".to_owned())
        .add(func1_v1, ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let state = new_test_state();

    // Run exactly ONE step to get func1 into Provisioning with an internal controller.
    let step_result = executor_v1.step(state).await?;
    let state_after_one_step = step_result.next_state;

    assert_eq!(
        get_status(&state_after_one_step, "func1"),
        Some(ResourceStatus::Provisioning),
        "func1 should be Provisioning after one step"
    );

    // Now switch to image-v2 -- config has changed while func1 is mid-provisioning.
    let func1_v2 = test_function_with_image("func1", "image-v2");
    let stack_v2 = Stack::new("provisioning-recreate-test".to_owned())
        .add(func1_v2, ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state_after_one_step).await?;

    assert_eq!(
        get_status(&final_state, "func1"),
        Some(ResourceStatus::Running),
        "func1 should be Running after delete-then-recreate"
    );

    // Verify func1 was recreated with image-v2 config.
    let func1_state = final_state.resources.get("func1").unwrap();
    let func1_final = func1_state.config.downcast_ref::<Function>().unwrap();
    assert_eq!(
        func1_final.code,
        FunctionCode::Image {
            image: "image-v2".to_string()
        },
        "func1 must have been recreated with image-v2 config"
    );

    Ok(())
}

/// Test C: Full retry cycle — Stay exhaustion → retry → polling succeeds (Bug 1 + Bug 2).
///
/// 1. First run: SIMULATE_STAY_EXHAUSTION causes ProvisionFailed.
/// 2. retry_failed() restores the controller. Verify _internal_stay_count is reset (Bug 2).
/// 3. Second run: normal function (no exhaustion) — must reach Running.
#[tokio::test]
async fn test_stay_exhaustion_full_retry_cycle() -> Result<()> {
    // --- First run: Stay exhaustion causes ProvisionFailed ---
    let func_v1 = stay_exhausting_function("cycle-func", 3);

    let stack_v1 = Stack::new("cycle-stack".to_owned())
        .add(func_v1, ResourceLifecycle::Live)
        .build();

    let executor_v1 = new_executor(&stack_v1)?;
    let mut state = new_test_state();

    for _ in 0..50 {
        if get_status(&state, "cycle-func") == Some(ResourceStatus::ProvisionFailed) {
            break;
        }
        let step_result = executor_v1.step(state).await?;
        state = step_result.next_state;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    assert_eq!(
        get_status(&state, "cycle-func"),
        Some(ResourceStatus::ProvisionFailed),
        "cycle-func should be ProvisionFailed after Stay exhaustion"
    );
    assert!(
        state
            .resources
            .get("cycle-func")
            .unwrap()
            .last_failed_state
            .is_some(),
        "lastFailedState must be saved (Bug 1 fix required)"
    );

    // --- Manual retry ---
    state.retry_failed();

    // Bug 2 fix: _internal_stay_count must be None after retry so the polling
    // handler gets a full fresh window on the next run.
    let retried_resource = state.resources.get("cycle-func").unwrap();
    assert_eq!(retried_resource.status, ResourceStatus::Provisioning);
    let retried_controller = retried_resource
        .get_internal_controller_typed::<TestFunctionController>()
        .unwrap();
    assert!(
        retried_controller._internal_stay_count.is_none(),
        "_internal_stay_count must be None after retry_failed(), got {:?}",
        retried_controller._internal_stay_count
    );

    // --- Second run: fixed function (no Stay exhaustion) — must reach Running ---
    let func_v2 = test_function("cycle-func");

    let stack_v2 = Stack::new("cycle-stack".to_owned())
        .add(func_v2, ResourceLifecycle::Live)
        .build();

    let executor_v2 = new_executor(&stack_v2)?;
    let final_state = run_to_synced(&executor_v2, state).await?;

    assert_eq!(
        get_status(&final_state, "cycle-func"),
        Some(ResourceStatus::Running),
        "cycle-func must reach Running after retry"
    );

    Ok(())
}
