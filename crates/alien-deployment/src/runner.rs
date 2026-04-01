//! Shared deployment loop runner.
//!
//! Centralizes step-loop behavior: every deployment loop caller uses
//! [`run_step_loop`] with a [`DeploymentLoopTransport`] implementation
//! that matches their persistence layer.
//!
//! # Lock invariants
//!
//! The runner does **not** acquire or release deployment locks. Callers must:
//!
//! 1. Acquire the deployment lock **before** calling [`run_step_loop`].
//! 2. Release the deployment lock **after** the runner returns, even on error.
//!
//! # Per-step reconcile
//!
//! After every `step()` call the runner invokes
//! [`DeploymentLoopTransport::reconcile_step`] so that:
//!
//! - State is durably persisted after each step (crash-safe).
//! - The server can react to state changes between steps (e.g. setting up
//!   cross-account registry access after Pending → InitialSetup).
//! - The server can inject updated state or config (e.g. `image_pull_credentials`,
//!   refreshed environment variables).

use crate::{
    loop_contract::{classify_status, LoopOperation, LoopOutcome, LoopResult, LoopStopReason},
    step,
    transport::DeploymentLoopTransport,
    Result,
};
use alien_core::{ClientConfig, DeploymentConfig, DeploymentState};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

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
    /// Number of steps executed.
    pub steps_executed: usize,
}

/// Run the deployment step loop with per-step reconciliation.
///
/// Calls [`crate::step()`] repeatedly until a terminal condition
/// is reached according to [`classify_status`], or the step budget is exceeded.
/// After **every** step the transport's `reconcile_step` is called to persist
/// state and pick up any server-side updates.
pub async fn run_step_loop(
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    policy: &RunnerPolicy,
    transport: &dyn DeploymentLoopTransport,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> Result<RunnerResult> {
    for step_count in 1..=policy.max_steps {
        // Pre-step terminal check
        if let Some(result) = classify_status(&state.status, policy.operation) {
            return Ok(RunnerResult {
                loop_result: result,
                steps_executed: step_count - 1,
            });
        }

        info!(
            step = step_count,
            status = ?state.status,
            deployment_id = %deployment_id,
            "Running deployment step"
        );

        let step_result = step(
            state.clone(),
            config.clone(),
            client_config.clone(),
            service_provider.clone(),
        )
        .await?;

        // Capture step metadata before overwriting state
        let step_error = step_result.error.as_ref();
        let update_heartbeat = step_result.update_heartbeat;
        let suggested_delay_ms = step_result.suggested_delay_ms;

        if let Some(ref err) = step_result.error {
            warn!(
                deployment_id = %deployment_id,
                error = %err,
                "Deployment step returned error"
            );
        }

        *state = step_result.state;

        // Reconcile after every step
        match transport
            .reconcile_step(deployment_id, state, step_error, update_heartbeat)
            .await
        {
            Ok(reconciled) => {
                if let Some(updated_state) = reconciled.state {
                    *state = updated_state;
                }
                if let Some(updated_config) = reconciled.config {
                    *config = updated_config;
                }
            }
            Err(e) => {
                // Reconcile failure is not fatal — log and continue with local state.
                // The final reconcile after the loop is the caller's responsibility.
                warn!(
                    deployment_id = %deployment_id,
                    error = %e,
                    "Failed to reconcile step — continuing with local state"
                );
            }
        }

        // Post-step terminal check
        if let Some(result) = classify_status(&state.status, policy.operation) {
            return Ok(RunnerResult {
                loop_result: result,
                steps_executed: step_count,
            });
        }

        // Handle suggested delays
        if let Some(delay_ms) = suggested_delay_ms {
            if let Some(threshold) = policy.delay_threshold {
                if Duration::from_millis(delay_ms) > threshold {
                    debug!(
                        deployment_id = %deployment_id,
                        delay_ms = delay_ms,
                        "Step suggests delay above threshold, yielding to caller"
                    );
                    return Ok(RunnerResult {
                        loop_result: LoopResult {
                            stop_reason: LoopStopReason::Synced,
                            outcome: LoopOutcome::Neutral,
                            final_status: state.status,
                        },
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
        steps_executed: policy.max_steps,
    })
}
