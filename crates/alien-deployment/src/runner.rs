//! Shared deployment loop runner.
//!
//! Centralizes step-loop behavior while leaving transport/scheduler details
//! to callers via the [`RunnerPolicy`].
//!
//! Test ownership: The runner's budget-exceeded outcome and loop contract
//! integration are tested via loop_contract.rs tests. Integration tests that
//! exercise the full step() pipeline live in alien-test (E2E crate).
//!
//! # Lock invariants
//!
//! The runner does **not** acquire or release deployment locks. Callers must:
//!
//! 1. Acquire the deployment lock **before** calling [`run_step_loop`].
//! 2. Persist / reconcile the returned state **after** the runner returns.
//! 3. Release the deployment lock **after** reconciliation, even on error.
//!
//! This separation keeps the runner transport-agnostic: push callers
//! (alien-deploy-cli) use HTTP lock/release against the manager API, while
//! the manager loop uses its own database-backed locking.

use crate::{
    loop_contract::{classify_status, LoopOperation, LoopOutcome, LoopResult, LoopStopReason},
    step, Result,
};
use alien_core::{ClientConfig, DeploymentConfig, DeploymentState};
use std::time::Duration;
use tracing::debug;

/// Policy configuration for the runner.
pub struct RunnerPolicy {
    /// Maximum number of step() calls before the runner yields.
    pub max_steps: usize,
    /// The operation being performed (determines success criteria).
    pub operation: LoopOperation,
    /// If a step suggests waiting longer than this threshold, the runner
    /// yields back to the caller instead of sleeping inline.
    pub delay_threshold: Option<Duration>,
}

impl Default for RunnerPolicy {
    fn default() -> Self {
        Self {
            max_steps: 200,
            operation: LoopOperation::Deploy,
            delay_threshold: None,
        }
    }
}

/// Result of running the step loop.
pub struct RunnerResult {
    /// The loop classification result (stop reason + outcome + final status).
    pub loop_result: LoopResult,
    /// The final deployment state after the loop.
    pub state: DeploymentState,
    /// Number of steps executed.
    pub steps_executed: usize,
}

/// Run the deployment step loop with the given policy.
///
/// Calls [`crate::step()`] repeatedly until a terminal condition
/// is reached according to [`classify_status`], or the step budget is exceeded.
pub async fn run_step_loop(
    state: &mut DeploymentState,
    config: &DeploymentConfig,
    client_config: &ClientConfig,
    policy: &RunnerPolicy,
) -> Result<RunnerResult> {
    for step_count in 1..=policy.max_steps {
        if let Some(result) = classify_status(&state.status, policy.operation) {
            return Ok(RunnerResult {
                loop_result: result,
                state: state.clone(),
                steps_executed: step_count - 1,
            });
        }

        debug!(
            step = step_count,
            status = ?state.status,
            "Running deployment step"
        );

        let step_result = step(
            state.clone(),
            config.clone(),
            client_config.clone(),
            None,
        )
        .await?;

        *state = step_result.state;

        if let Some(result) = classify_status(&state.status, policy.operation) {
            return Ok(RunnerResult {
                loop_result: result,
                state: state.clone(),
                steps_executed: step_count,
            });
        }

        if let Some(delay_ms) = step_result.suggested_delay_ms {
            if let Some(threshold) = policy.delay_threshold {
                if Duration::from_millis(delay_ms) > threshold {
                    return Ok(RunnerResult {
                        loop_result: LoopResult {
                            stop_reason: LoopStopReason::Synced,
                            outcome: LoopOutcome::Neutral,
                            final_status: state.status,
                        },
                        state: state.clone(),
                        steps_executed: step_count,
                    });
                }
            }
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    Ok(RunnerResult {
        loop_result: LoopResult {
            stop_reason: LoopStopReason::BudgetExceeded,
            outcome: LoopOutcome::Failure,
            final_status: state.status,
        },
        state: state.clone(),
        steps_executed: policy.max_steps,
    })
}
