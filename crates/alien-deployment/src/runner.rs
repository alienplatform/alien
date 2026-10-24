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
//! - The server can inject updated state or config (e.g. refreshed environment
//!   variables).

use crate::{
    loop_contract::{classify_status, LoopOperation, LoopOutcome, LoopResult, LoopStopReason},
    observe::run_observe_pass,
    step,
    transport::DeploymentLoopTransport,
    Result,
};
use alien_core::{ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, StackState};
use alien_error::{AlienError, ContextError as _};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

const CHECKPOINT_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(1);
const CHECKPOINT_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);
/// Failed checkpoints in a row after which the runner reports each retry to `on_progress`, so a
/// CLI that hides logs still shows that the deployment is stuck, and why.
const CHECKPOINT_RETRIES_BEFORE_NOTICE: usize = 3;
const LEASE_RENEW_INTERVAL: Duration = Duration::from_secs(60);
const LEASE_RENEW_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const LEASE_RENEW_FAILURE_DEADLINE: Duration = Duration::from_secs(3 * 60 + 30);

/// The shortest confirmed lease a deployment store is expected to grant.
///
/// The runner must give up ownership strictly before the store would let another
/// session acquire the deployment, otherwise two runners could act on the same
/// target. The worst case is a renewal attempt that starts just before the
/// failure deadline and then times out, so the latest the runner can stop is
/// `LEASE_RENEW_FAILURE_DEADLINE + LEASE_RENEW_REQUEST_TIMEOUT` after the last
/// success. The assertion below keeps that strictly inside the lease.
const MINIMUM_CONFIRMED_LEASE: Duration = Duration::from_secs(5 * 60);

const _: () = assert!(
    LEASE_RENEW_FAILURE_DEADLINE.as_secs() + LEASE_RENEW_REQUEST_TIMEOUT.as_secs()
        < MINIMUM_CONFIRMED_LEASE.as_secs(),
    "the runner must abandon a deployment before its lease can be reacquired elsewhere",
);

const _: () = assert!(
    LEASE_RENEW_INTERVAL.as_secs() < LEASE_RENEW_FAILURE_DEADLINE.as_secs(),
    "at least one renewal must be attempted before the failure deadline",
);

/// Policy configuration for the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayStrategy {
    /// Sleep inside the current process. Intended for interactive/local callers.
    Inline,
    /// Persist the schedule and return so a shared scheduler can release capacity.
    Yield,
}

pub struct RunnerPolicy {
    /// Maximum number of step() calls before the runner yields.
    pub max_steps: usize,
    /// The operation being performed (determines success criteria).
    pub operation: LoopOperation,
    pub delay_strategy: DelayStrategy,
}

impl Default for RunnerPolicy {
    fn default() -> Self {
        Self {
            max_steps: 200,
            operation: LoopOperation::Deploy,
            delay_strategy: DelayStrategy::Inline,
        }
    }
}

/// Result of running the step loop.
#[derive(Debug)]
pub struct RunnerResult {
    /// The loop classification result (stop reason + outcome + final status).
    pub loop_result: LoopResult,
    /// Number of steps executed.
    pub steps_executed: usize,
    /// Whether the manager has recorded the state the loop stopped at, either
    /// by accepting its checkpoint or because the loop read it from the
    /// manager. When false, finalization must persist the state rather than
    /// only release the execution claim.
    pub state_persisted: bool,
}

/// Convert a terminal semantic failure carried in deployment state into the
/// operation error before callers finalize and release the execution claim.
/// The step loop returns `Ok(RunnerResult)` after durably checkpointing such a
/// failure; treating every `Ok` as success would let a later finalization error
/// replace the controller/preflight error that actually stopped the operation.
pub fn preserve_semantic_failure(
    result: Result<RunnerResult>,
    state: &DeploymentState,
) -> std::result::Result<RunnerResult, AlienError> {
    match result {
        Ok(result) if result.loop_result.outcome == LoopOutcome::Failure => Err(
            crate::deployment_headline_error_from_state(state).unwrap_or_else(|| {
                AlienError::new(alien_error::GenericError {
                    message: format!(
                        "deployment failed at status {:?}",
                        result.loop_result.final_status
                    ),
                })
            }),
        ),
        Ok(result) => Ok(result),
        Err(error) => Err(error.into_generic()),
    }
}

/// Progress snapshot after each deployment step.
pub struct StepProgress<'a> {
    pub step_number: usize,
    pub status: DeploymentStatus,
    pub stack_state: Option<&'a StackState>,
    /// Set while the step's result keeps failing to checkpoint: the runner retries without
    /// making further progress until the manager accepts it.
    pub checkpoint_retry: Option<CheckpointRetry<'a>>,
}

/// A checkpoint that failed again and will be retried.
pub struct CheckpointRetry<'a> {
    /// Failed attempts so far.
    pub attempt: usize,
    /// How long the runner waits before the next attempt.
    pub retry_after: Duration,
    /// Why the last attempt failed.
    pub error: &'a AlienError,
}

impl CheckpointRetry<'_> {
    /// One line for a person watching the deployment.
    pub fn notice(&self) -> String {
        format!(
            "Warning: the manager has not accepted this deployment step after {} attempts; retrying in {}s. Last error: {}",
            self.attempt,
            self.retry_after.as_secs().max(1),
            self.error
        )
    }
}

fn notify_checkpoint_retry(
    on_progress: Option<&ProgressCallback>,
    step_number: usize,
    state: &DeploymentState,
    attempt: usize,
    retry_after: Duration,
    error: &AlienError,
) {
    if attempt < CHECKPOINT_RETRIES_BEFORE_NOTICE {
        return;
    }
    if let Some(cb) = on_progress {
        cb(&StepProgress {
            step_number,
            status: state.status,
            stack_state: state.stack_state.as_ref(),
            checkpoint_retry: Some(CheckpointRetry {
                attempt,
                retry_after,
                error,
            }),
        });
    }
}

/// Callback invoked after each deployment step with the current progress.
pub type ProgressCallback = Box<dyn Fn(&StepProgress) + Send + Sync>;

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
    on_progress: Option<&ProgressCallback>,
) -> Result<RunnerResult> {
    run_step_loop_inner(
        state,
        config,
        client_config,
        deployment_id,
        policy,
        transport,
        service_provider,
        on_progress,
        false,
    )
    .await
}

/// Run one observation step for a Running or RefreshFailed deployment.
///
/// Normal deployment loops treat Running as already synced and stop before
/// calling [`crate::step()`]. Heartbeat loops use this entry point to execute the
/// Running refresh path exactly once, then checkpoint emitted resource
/// heartbeats through the provided transport.
pub async fn run_running_refresh_step_loop(
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    policy: &RunnerPolicy,
    transport: &dyn DeploymentLoopTransport,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    on_progress: Option<&ProgressCallback>,
) -> Result<RunnerResult> {
    run_step_loop_inner(
        state,
        config,
        client_config,
        deployment_id,
        policy,
        transport,
        service_provider,
        on_progress,
        true,
    )
    .await
}

async fn run_step_loop_inner(
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    policy: &RunnerPolicy,
    transport: &dyn DeploymentLoopTransport,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    on_progress: Option<&ProgressCallback>,
    allow_initial_running_step: bool,
) -> Result<RunnerResult> {
    run_with_lease_renewal(
        deployment_id,
        transport,
        run_step_loop_body(
            state,
            config,
            client_config,
            deployment_id,
            policy,
            transport,
            service_provider,
            on_progress,
            allow_initial_running_step,
        ),
    )
    .await
}

/// Runs an already-owned deployment operation while renewing its lease.
///
/// The operation is cancelled if Alien cannot confirm ownership before the
/// lease safety deadline. Callers remain responsible for acquiring and
/// releasing the lease.
pub(crate) async fn run_with_lease_renewal<T>(
    deployment_id: &str,
    transport: &dyn DeploymentLoopTransport,
    operation: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::pin!(operation);
    let mut renew_interval = tokio::time::interval(LEASE_RENEW_INTERVAL);
    renew_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    renew_interval.tick().await;
    let mut last_successful_renewal = tokio::time::Instant::now();

    loop {
        tokio::select! {
            result = &mut operation => return result,
            _ = renew_interval.tick() => {
                match tokio::time::timeout(
                    LEASE_RENEW_REQUEST_TIMEOUT,
                    transport.renew_lease(deployment_id),
                )
                .await
                {
                    Err(_) if last_successful_renewal.elapsed() >= LEASE_RENEW_FAILURE_DEADLINE => {
                        return Err(AlienError::new(crate::ErrorData::DeploymentLeaseLost {
                            message: "lease renewal requests timed out before the safety deadline"
                                .to_string(),
                        }))
                    }
                    Err(_) => warn!(
                        deployment_id = %deployment_id,
                        "Deployment lease renewal timed out; will retry before the lease deadline"
                    ),
                    Ok(Ok(())) => last_successful_renewal = tokio::time::Instant::now(),
                    Ok(Err(error)) if !error.retryable => {
                        return Err(error.context(crate::ErrorData::DeploymentLeaseLost {
                            message: "the active session no longer owns the deployment".to_string(),
                        }))
                    }
                    Ok(Err(error)) if last_successful_renewal.elapsed() >= LEASE_RENEW_FAILURE_DEADLINE => {
                        return Err(error.context(crate::ErrorData::DeploymentLeaseLost {
                            message: "the lease could not be renewed before its safety deadline".to_string(),
                        }))
                    }
                    Ok(Err(error)) => warn!(
                        deployment_id = %deployment_id,
                        error = %error,
                        "Failed to renew deployment lease; will retry before the lease deadline"
                    ),
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_step_loop_body(
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    policy: &RunnerPolicy,
    transport: &dyn DeploymentLoopTransport,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    on_progress: Option<&ProgressCallback>,
    allow_initial_running_step: bool,
) -> Result<RunnerResult> {
    if !state.has_desired() {
        let service_provider = service_provider
            .unwrap_or_else(|| Arc::new(alien_infra::DefaultPlatformServiceProvider::default()));
        let observe_report = run_observe_pass(
            state.platform,
            client_config,
            &service_provider,
            deployment_id,
            config.observe_label_selector.as_deref(),
            config.observe_all_namespaces,
        )
        .await?;

        if !observe_report.inventory_batches.is_empty() {
            let mut checkpoint_attempt = 1usize;
            let mut checkpoint_delay = CHECKPOINT_RETRY_INITIAL_DELAY;
            loop {
                match transport
                    .reconcile_step(
                        deployment_id,
                        state,
                        config,
                        true,
                        None,
                        Vec::new(),
                        observe_report.inventory_batches.clone(),
                    )
                    .await
                {
                    Ok(reconciled) => {
                        if let Some(updated_state) = reconciled.state {
                            *state = updated_state;
                        }
                        if let Some(updated_config) = reconciled.config {
                            *config = updated_config;
                        }
                        break;
                    }
                    Err(e) if !e.retryable => {
                        return Err(e.context(crate::ErrorData::DeploymentCheckpointFailed {
                            message: "the observed deployment state was not accepted".to_string(),
                        }))
                    }
                    Err(e) => {
                        warn!(
                            deployment_id = %deployment_id,
                            attempt = checkpoint_attempt,
                            retry_after_ms = checkpoint_delay.as_millis() as u64,
                            error = %e,
                            "Failed to checkpoint observe-only deployment; retrying before returning"
                        );
                        tokio::time::sleep(checkpoint_delay).await;
                        checkpoint_attempt += 1;
                        checkpoint_delay = (checkpoint_delay * 2).min(CHECKPOINT_RETRY_MAX_DELAY);
                    }
                }
            }
        }

        return Ok(RunnerResult {
            loop_result: LoopResult {
                stop_reason: LoopStopReason::NoWork,
                outcome: LoopOutcome::Neutral,
                final_status: state.status,
            },
            steps_executed: 0,
            state_persisted: true,
        });
    }

    for step_count in 1..=policy.max_steps {
        // Pre-step terminal check
        if !should_step_retryable_failure(state) {
            let should_run_initial_running_refresh = allow_initial_running_step
                && step_count == 1
                && matches!(
                    state.status,
                    DeploymentStatus::Running | DeploymentStatus::RefreshFailed
                );
            if let Some(result) = classify_status(&state.status, policy.operation) {
                if !should_run_initial_running_refresh {
                    return Ok(RunnerResult {
                        loop_result: result,
                        steps_executed: step_count - 1,
                        state_persisted: true,
                    });
                }
            }
        }

        if should_step_retryable_failure(state) {
            info!(
                step = step_count,
                status = ?state.status,
                deployment_id = %deployment_id,
                "Retry requested for failed deployment; running failed-status handler"
            );
        }

        info!(
            step = step_count,
            status = ?state.status,
            deployment_id = %deployment_id,
            "Running deployment step"
        );

        // `step` dispatches every deployment phase and therefore has a large async
        // state machine. Keep that future off Tokio's worker stack: complex stacks can
        // otherwise cross the default thread-stack limit while entering Pending.
        let step_result = Box::pin(step(
            state.clone(),
            config.clone(),
            client_config.clone(),
            service_provider.clone(),
        ))
        .await;

        let step_result = match step_result {
            Ok(step_result) => step_result,
            Err(error) => {
                let deployment_error = error.into_generic();
                *state = failed_state_for_step_failure(state, deployment_error.clone());

                let mut checkpoint_attempt = 1usize;
                let mut checkpoint_delay = CHECKPOINT_RETRY_INITIAL_DELAY;
                loop {
                    match transport
                        .reconcile_step(deployment_id, state, config, false, None, vec![], vec![])
                        .await
                    {
                        Ok(reconciled) => {
                            if let Some(updated_state) = reconciled.state {
                                *state = updated_state;
                            }
                            if let Some(updated_config) = reconciled.config {
                                *config = updated_config;
                            }
                            break;
                        }
                        Err(e) if !e.retryable => {
                            return Err(e.context(crate::ErrorData::DeploymentCheckpointFailed {
                                message: "the failed deployment state was not accepted".to_string(),
                            }))
                        }
                        Err(e) => {
                            warn!(
                                deployment_id = %deployment_id,
                                attempt = checkpoint_attempt,
                                retry_after_ms = checkpoint_delay.as_millis() as u64,
                                error = %e,
                                "Failed to checkpoint deployment error; retrying before returning failure"
                            );
                            notify_checkpoint_retry(
                                on_progress,
                                step_count,
                                state,
                                checkpoint_attempt,
                                checkpoint_delay,
                                &e,
                            );
                            tokio::time::sleep(checkpoint_delay).await;
                            checkpoint_attempt += 1;
                            checkpoint_delay =
                                (checkpoint_delay * 2).min(CHECKPOINT_RETRY_MAX_DELAY);
                        }
                    }
                }

                let loop_result =
                    classify_status(&state.status, policy.operation).unwrap_or(LoopResult {
                        stop_reason: LoopStopReason::Failed,
                        outcome: LoopOutcome::Failure,
                        final_status: state.status,
                    });

                return Ok(RunnerResult {
                    loop_result,
                    steps_executed: step_count,
                    state_persisted: true,
                });
            }
        };

        // Capture step metadata before overwriting state.
        let update_heartbeat = step_result.update_heartbeat;
        let suggested_delay_ms = step_result.suggested_delay_ms;
        let heartbeats = step_result.heartbeats.clone();
        let observed_inventory_batches = step_result.observed_inventory_batches.clone();

        *state = step_result.state;

        // Checkpoint after every step. This is a durability barrier: once a
        // cloud/API step has produced new state, the runner must not execute
        // another step or return control to the caller until that exact state is
        // persisted. Otherwise the next actor can replay a non-idempotent cloud
        // create from stale durable state.
        let mut checkpoint_attempt = 1usize;
        let mut checkpoint_delay = CHECKPOINT_RETRY_INITIAL_DELAY;
        loop {
            match transport
                .reconcile_step(
                    deployment_id,
                    state,
                    config,
                    update_heartbeat,
                    suggested_delay_ms,
                    heartbeats.clone(),
                    observed_inventory_batches.clone(),
                )
                .await
            {
                Ok(reconciled) => {
                    if let Some(updated_state) = reconciled.state {
                        *state = updated_state;
                    }
                    if let Some(updated_config) = reconciled.config {
                        *config = updated_config;
                    }
                    break;
                }
                Err(e) if !e.retryable => {
                    return Err(e.context(crate::ErrorData::DeploymentCheckpointFailed {
                        message: "the deployment state was not accepted".to_string(),
                    }))
                }
                Err(e) => {
                    warn!(
                        deployment_id = %deployment_id,
                        attempt = checkpoint_attempt,
                        retry_after_ms = checkpoint_delay.as_millis() as u64,
                        error = %e,
                        "Failed to checkpoint deployment step; retrying before any further progress"
                    );
                    notify_checkpoint_retry(
                        on_progress,
                        step_count,
                        state,
                        checkpoint_attempt,
                        checkpoint_delay,
                        &e,
                    );
                    tokio::time::sleep(checkpoint_delay).await;
                    checkpoint_attempt += 1;
                    checkpoint_delay = (checkpoint_delay * 2).min(CHECKPOINT_RETRY_MAX_DELAY);
                }
            }
        }

        // Notify progress callback
        if let Some(cb) = on_progress {
            cb(&StepProgress {
                step_number: step_count,
                status: state.status,
                stack_state: state.stack_state.as_ref(),
                checkpoint_retry: None,
            });
        }

        // Post-step terminal check
        if let Some(result) = classify_status(&state.status, policy.operation) {
            return Ok(RunnerResult {
                loop_result: result,
                steps_executed: step_count,
                state_persisted: true,
            });
        }

        // Handle suggested delays
        if let Some(delay_ms) = suggested_delay_ms {
            let delay = Duration::from_millis(delay_ms);
            if !delay.is_zero() && policy.delay_strategy == DelayStrategy::Yield {
                debug!(
                    deployment_id = %deployment_id,
                    delay_ms,
                    "Step scheduled future work; yielding to the caller"
                );
                return Ok(RunnerResult {
                    loop_result: LoopResult {
                        stop_reason: LoopStopReason::Delayed,
                        outcome: LoopOutcome::Neutral,
                        final_status: state.status,
                    },
                    steps_executed: step_count,
                    state_persisted: true,
                });
            }
            tokio::time::sleep(delay).await;
        }
    }

    Ok(RunnerResult {
        loop_result: LoopResult {
            stop_reason: LoopStopReason::BudgetExceeded,
            outcome: LoopOutcome::Failure,
            final_status: state.status,
        },
        steps_executed: policy.max_steps,
        state_persisted: true,
    })
}

fn should_step_retryable_failure(state: &DeploymentState) -> bool {
    state.retry_requested && state.status.is_failed()
}

fn failed_state_for_step_failure(
    current: &DeploymentState,
    error: alien_error::AlienError,
) -> DeploymentState {
    let mut next = current.clone();
    next.status = failed_status_for_deployment_error(current.status);
    next.error = Some(error);
    next.retry_requested = false;
    next
}

pub fn failed_status_for_deployment_error(status: DeploymentStatus) -> DeploymentStatus {
    match status {
        DeploymentStatus::Pending => DeploymentStatus::PreflightsFailed,
        DeploymentStatus::InitialSetup => DeploymentStatus::InitialSetupFailed,
        DeploymentStatus::Provisioning
        | DeploymentStatus::WaitingForMachines
        | DeploymentStatus::WaitingForSecrets => DeploymentStatus::ProvisioningFailed,
        DeploymentStatus::Running => DeploymentStatus::RefreshFailed,
        DeploymentStatus::UpdatePending | DeploymentStatus::Updating => {
            DeploymentStatus::UpdateFailed
        }
        DeploymentStatus::DeletePending | DeploymentStatus::Deleting => {
            DeploymentStatus::DeleteFailed
        }
        DeploymentStatus::PreflightsFailed
        | DeploymentStatus::InitialSetupFailed
        | DeploymentStatus::ProvisioningFailed
        | DeploymentStatus::RefreshFailed
        | DeploymentStatus::UpdateFailed
        | DeploymentStatus::DeleteFailed
        | DeploymentStatus::TeardownRequired
        | DeploymentStatus::TeardownFailed
        | DeploymentStatus::Error => status,
        DeploymentStatus::Deleted => DeploymentStatus::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{DeploymentLoopTransport, StepReconcileResult};
    use alien_core::{
        Container, ContainerCode, EnvironmentVariablesSnapshot, Platform, ReleaseInfo,
        ResourceEntry, ResourceLifecycle, ResourceSpec, Stack, StackSettings, StackState, Worker,
        WorkerCode, DEPLOYMENT_PROTOCOL_VERSION,
    };
    use alien_error::AlienErrorData;
    use async_trait::async_trait;
    use indexmap::IndexMap;
    use serde::{Deserialize, Serialize};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    #[derive(Debug, Default)]
    struct FailFirstCheckpointTransport {
        attempts: AtomicUsize,
        checkpointed_statuses: Mutex<Vec<DeploymentStatus>>,
        checkpointed_states: Mutex<Vec<serde_json::Value>>,
    }

    #[derive(Debug, Default)]
    struct TerminalCheckpointTransport {
        attempts: AtomicUsize,
    }

    #[derive(Debug, Default)]
    struct CountingRenewalTransport {
        renewals: AtomicUsize,
    }

    #[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
    enum TestTransportError {
        #[error(
            code = "TEST_TRANSIENT_CHECKPOINT",
            message = "Transient checkpoint failure",
            retryable = "true",
            internal = "true"
        )]
        TransientCheckpoint,
    }

    #[test]
    fn semantic_runner_failure_returns_the_checkpointed_operation_error() {
        let mut state = test_state();
        state.status = DeploymentStatus::PreflightsFailed;
        state.error = Some(AlienError::new(alien_error::GenericError {
            message: "compute pool 'preview' has no selected machine".to_string(),
        }));
        let result = RunnerResult {
            loop_result: LoopResult {
                stop_reason: LoopStopReason::Failed,
                outcome: LoopOutcome::Failure,
                final_status: DeploymentStatus::PreflightsFailed,
            },
            steps_executed: 1,
            state_persisted: true,
        };

        let error = preserve_semantic_failure(Ok(result), &state)
            .expect_err("semantic failure must become an operation error");
        assert_eq!(
            error.message,
            "compute pool 'preview' has no selected machine"
        );
    }

    #[async_trait]
    impl DeploymentLoopTransport for FailFirstCheckpointTransport {
        async fn reconcile_step(
            &self,
            _deployment_id: &str,
            state: &DeploymentState,
            _config: &DeploymentConfig,
            _update_heartbeat: bool,
            _suggested_delay_ms: Option<u64>,
            _heartbeats: Vec<alien_core::ResourceHeartbeat>,
            _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
        ) -> std::result::Result<StepReconcileResult, alien_error::AlienError> {
            self.checkpointed_statuses
                .lock()
                .expect("statuses lock poisoned")
                .push(state.status);

            self.checkpointed_states
                .lock()
                .expect("states lock poisoned")
                .push(serde_json::to_value(state).expect("state should serialize"));

            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                return Err(
                    alien_error::AlienError::new(TestTransportError::TransientCheckpoint)
                        .into_generic(),
                );
            }

            Ok(StepReconcileResult {
                state: None,
                config: None,
            })
        }
    }

    #[async_trait]
    impl DeploymentLoopTransport for TerminalCheckpointTransport {
        async fn reconcile_step(
            &self,
            _deployment_id: &str,
            _state: &DeploymentState,
            _config: &DeploymentConfig,
            _update_heartbeat: bool,
            _suggested_delay_ms: Option<u64>,
            _heartbeats: Vec<alien_core::ResourceHeartbeat>,
            _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
        ) -> std::result::Result<StepReconcileResult, alien_error::AlienError> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            Err(alien_error::AlienError::new(alien_error::GenericError {
                message: "the deployment lease is no longer owned".to_string(),
            }))
        }
    }

    #[async_trait]
    impl DeploymentLoopTransport for CountingRenewalTransport {
        async fn renew_lease(
            &self,
            _deployment_id: &str,
        ) -> std::result::Result<(), alien_error::AlienError> {
            self.renewals.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn reconcile_step(
            &self,
            _deployment_id: &str,
            _state: &DeploymentState,
            _config: &DeploymentConfig,
            _update_heartbeat: bool,
            _suggested_delay_ms: Option<u64>,
            _heartbeats: Vec<alien_core::ResourceHeartbeat>,
            _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
        ) -> std::result::Result<StepReconcileResult, alien_error::AlienError> {
            Ok(StepReconcileResult {
                state: None,
                config: None,
            })
        }
    }

    /// Refuses every checkpoint and renewal the way a manager answers once the
    /// execution claim the runner holds is gone.
    #[derive(Debug)]
    struct LostClaimTransport {
        retryable: bool,
        /// Status and heartbeat flag of every refused checkpoint.
        checkpoints: Mutex<Vec<(DeploymentStatus, bool)>>,
        renewals: AtomicUsize,
    }

    impl LostClaimTransport {
        fn new(retryable: bool) -> Self {
            Self {
                retryable,
                checkpoints: Mutex::new(Vec::new()),
                renewals: AtomicUsize::new(0),
            }
        }

        fn refusal(&self) -> alien_error::AlienError {
            let mut error = alien_error::AlienError::new(alien_error::GenericError {
                message: "The execution attempt is no longer live.".to_string(),
            });
            error.code = "DEPLOYMENT_UPDATE_CLAIM_LOST".to_string();
            error.retryable = self.retryable;
            error.internal = false;
            error.http_status_code = Some(409);
            error
        }
    }

    #[async_trait]
    impl DeploymentLoopTransport for LostClaimTransport {
        async fn renew_lease(
            &self,
            _deployment_id: &str,
        ) -> std::result::Result<(), alien_error::AlienError> {
            self.renewals.fetch_add(1, Ordering::SeqCst);
            Err(self.refusal())
        }

        async fn reconcile_step(
            &self,
            _deployment_id: &str,
            state: &DeploymentState,
            _config: &DeploymentConfig,
            update_heartbeat: bool,
            _suggested_delay_ms: Option<u64>,
            _heartbeats: Vec<alien_core::ResourceHeartbeat>,
            _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
        ) -> std::result::Result<StepReconcileResult, alien_error::AlienError> {
            self.checkpoints
                .lock()
                .expect("checkpoints lock poisoned")
                .push((state.status, update_heartbeat));
            Err(self.refusal())
        }
    }

    /// A heartbeat whose claim was already settled: the step runs, and every
    /// checkpoint and renewal after it is refused. A retryable refusal keeps the
    /// deployment until the lease deadline; a final one stops at the first refusal.
    #[tokio::test(start_paused = true)]
    async fn lost_claim_holds_the_deployment_only_while_the_refusal_is_retryable() {
        for retryable in [true, false] {
            let transport = LostClaimTransport::new(retryable);
            let mut state = test_state();
            state.status = DeploymentStatus::Running;
            state.current_release = state.target_release.take();
            state.stack_state = Some(StackState::new(Platform::Test));
            // An empty prepared stack passes its health check, so the step is a
            // healthy heartbeat and the refused checkpoint is the heartbeat's own.
            let mut prepared_stack = test_stack();
            prepared_stack.resources.clear();
            state.runtime_metadata = Some(alien_core::RuntimeMetadata {
                prepared_stack: Some(prepared_stack),
                ..Default::default()
            });
            let mut config = test_config();
            let policy = RunnerPolicy {
                max_steps: 1,
                operation: LoopOperation::Deploy,
                delay_strategy: DelayStrategy::Inline,
            };

            let started = tokio::time::Instant::now();
            let error = run_running_refresh_step_loop(
                &mut state,
                &mut config,
                &ClientConfig::Test,
                "dep_test",
                &policy,
                &transport,
                None,
                None,
            )
            .await
            .expect_err("a lost claim must stop the runner");
            let held = started.elapsed();
            let checkpoints = transport
                .checkpoints
                .lock()
                .expect("checkpoints lock poisoned")
                .clone();
            let renewals = transport.renewals.load(Ordering::SeqCst);

            assert!(
                checkpoints
                    .iter()
                    .all(|checkpoint| *checkpoint == (DeploymentStatus::Running, true)),
                "every refused checkpoint is the healthy heartbeat: {checkpoints:?}"
            );
            assert_eq!(error.retryable, retryable);
            if retryable {
                // Renewals at 60, 120 and 180 s fail inside the deadline; the
                // one at 240 s is past it and cancels the step loop.
                assert_eq!(error.code, "DEPLOYMENT_LEASE_LOST");
                assert_eq!(held, LEASE_RENEW_INTERVAL * 4);
                assert_eq!(renewals, 4);
                assert!(checkpoints.len() > 1, "the refused checkpoint is retried");
            } else {
                assert_eq!(error.code, "DEPLOYMENT_CHECKPOINT_FAILED");
                assert!(held < LEASE_RENEW_INTERVAL, "stopped after {held:?}");
                assert_eq!(renewals, 0);
                assert_eq!(checkpoints.len(), 1);
            }
        }
    }

    /// A renewal refused as final cancels the operation at the renewal that
    /// learned it, not at the lease deadline.
    #[tokio::test(start_paused = true)]
    async fn lease_wrapper_cancels_at_the_first_final_renewal_refusal() {
        for (retryable, renewals, held) in [
            (true, 4, LEASE_RENEW_INTERVAL * 4),
            (false, 1, LEASE_RENEW_INTERVAL),
        ] {
            let transport = LostClaimTransport::new(retryable);
            let started = tokio::time::Instant::now();

            let error = run_with_lease_renewal("dep_test", &transport, async {
                tokio::time::sleep(MINIMUM_CONFIRMED_LEASE * 2).await;
                Ok(())
            })
            .await
            .expect_err("a lost claim must cancel the operation");

            assert_eq!(error.code, "DEPLOYMENT_LEASE_LOST");
            assert_eq!(error.retryable, retryable);
            assert_eq!(started.elapsed(), held, "retryable={retryable}");
            assert_eq!(transport.renewals.load(Ordering::SeqCst), renewals);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn lease_wrapper_renews_while_operation_is_in_flight() {
        let transport = CountingRenewalTransport::default();

        run_with_lease_renewal("dep_test", &transport, async {
            tokio::time::sleep(LEASE_RENEW_INTERVAL + Duration::from_secs(1)).await;
            Ok(())
        })
        .await
        .expect("operation should complete while its lease is renewed");

        assert_eq!(transport.renewals.load(Ordering::SeqCst), 1);
    }

    fn test_stack() -> Stack {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("default".to_string(), alien_core::PermissionProfile::new());

        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::PermissionsConfig {
                profiles,
                management: alien_core::ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: Vec::new(),
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
        }
    }

    fn test_state() -> DeploymentState {
        DeploymentState {
            status: DeploymentStatus::Pending,
            platform: Platform::Test,
            current_release: None,
            target_release: Some(ReleaseInfo {
                release_id: Some("rel_test".to_string()),
                version: None,
                description: None,
                stack: test_stack(),
            }),
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    fn linked_container_state() -> DeploymentState {
        let backend = Container::new("backend".to_string())
            .code(ContainerCode::Image {
                image: "backend:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(ResourceSpec {
                min: "256Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .port(8080)
            .permissions("default".to_string())
            .build();
        let frontend = Container::new("frontend".to_string())
            .code(ContainerCode::Image {
                image: "frontend:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(ResourceSpec {
                min: "256Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .port(8080)
            .permissions("default".to_string())
            .link(&backend)
            .build();
        let stack = Stack::new("linked-containers".to_string())
            .permissions({
                let mut profiles = IndexMap::new();
                profiles.insert("default".to_string(), alien_core::PermissionProfile::new());
                alien_core::PermissionsConfig {
                    profiles,
                    management: alien_core::ManagementPermissions::Auto,
                }
            })
            .add(backend, ResourceLifecycle::Live)
            .add(frontend, ResourceLifecycle::Live)
            .build();

        DeploymentState {
            status: DeploymentStatus::Pending,
            platform: Platform::Test,
            current_release: None,
            target_release: Some(ReleaseInfo {
                release_id: Some("rel_linked".to_string()),
                version: None,
                description: None,
                stack,
            }),
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    fn test_config() -> DeploymentConfig {
        DeploymentConfig {
            gcp_federation_scope: None,
            stored_secret_input_ids: None,
            input_values: Default::default(),
            deployment_name: Some("test deployment".to_string()),
            stack_settings: StackSettings::default(),
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                hash: "hash".to_string(),
                variables: Vec::new(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
            },
            external_bindings: alien_core::ExternalBindings::default(),
            base_platform: None,
            label_domain: None,
            observe_label_selector: None,
            observe_all_namespaces: false,
            compute_backend: None,
            allow_frozen_changes: false,
            setup_run: false,
            domain_metadata: None,
            public_endpoints: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
            volume_restores: Vec::new(),
        }
    }

    #[test]
    fn linked_containers_enter_initial_setup_on_bounded_tokio_worker_stack() {
        // The test build's deployment future is smaller than the fully featured CLI
        // future that originally overflowed Tokio's 2 MiB default. A smaller worker
        // stack preserves enough headroom to make the same boundary regression fail
        // before `step` is boxed, while still exercising the real linked graph.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_stack_size(1536 * 1024)
            .enable_all()
            .build()
            .expect("bounded Tokio runtime should build");

        let task = runtime.spawn(async {
            let transport = CountingRenewalTransport::default();
            let mut state = linked_container_state();
            let mut config = test_config();
            let policy = RunnerPolicy {
                max_steps: 1,
                operation: LoopOperation::Deploy,
                delay_strategy: DelayStrategy::Inline,
            };

            let result = run_step_loop(
                &mut state,
                &mut config,
                &ClientConfig::Test,
                "dep_linked",
                &policy,
                &transport,
                None,
                None,
            )
            .await
            .expect("linked deployment should complete its pending step");

            assert_eq!(result.steps_executed, 1);
            assert_eq!(state.status, DeploymentStatus::InitialSetup);
        });

        runtime
            .block_on(task)
            .expect("Tokio worker should not overflow its stack");
    }

    #[tokio::test(start_paused = true)]
    async fn retries_failed_checkpoint_before_next_step() {
        let transport = FailFirstCheckpointTransport::default();
        let mut state = test_state();
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 1,
            operation: LoopOperation::Deploy,
            delay_strategy: DelayStrategy::Inline,
        };

        let result = run_step_loop(
            &mut state,
            &mut config,
            &ClientConfig::Test,
            "dep_test",
            &policy,
            &transport,
            None,
            None,
        )
        .await
        .expect("runner should retry checkpoint and complete the step budget");

        assert_eq!(result.steps_executed, 1);
        assert_eq!(
            transport.attempts.load(Ordering::SeqCst),
            2,
            "the same checkpoint should be retried after a transient failure"
        );
        assert_eq!(
            transport
                .checkpointed_statuses
                .lock()
                .expect("statuses lock poisoned")
                .as_slice(),
            &[
                DeploymentStatus::InitialSetup,
                DeploymentStatus::InitialSetup
            ],
            "checkpoint retry must persist the same produced state, not run another step"
        );
    }

    /// Answers the first `failures` checkpoints the way a manager behind a load balancer does
    /// when the request outlives the balancer's idle timeout.
    #[derive(Debug)]
    struct GatewayTimeoutTransport {
        failures: usize,
        attempts: AtomicUsize,
    }

    #[async_trait]
    impl DeploymentLoopTransport for GatewayTimeoutTransport {
        async fn reconcile_step(
            &self,
            _deployment_id: &str,
            _state: &DeploymentState,
            _config: &DeploymentConfig,
            _update_heartbeat: bool,
            _suggested_delay_ms: Option<u64>,
            _heartbeats: Vec<alien_core::ResourceHeartbeat>,
            _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
        ) -> std::result::Result<StepReconcileResult, alien_error::AlienError> {
            if self.attempts.fetch_add(1, Ordering::SeqCst) < self.failures {
                let mut error = alien_error::AlienError::new(alien_error::GenericError {
                    message: "reconcile step: 504 Gateway Timeout".to_string(),
                });
                error.retryable = true;
                return Err(error);
            }
            Ok(StepReconcileResult {
                state: None,
                config: None,
            })
        }
    }

    /// The caller hears about a checkpoint the manager keeps refusing, with the reason, once the
    /// retries stop looking like a blip. A CLI hides runner logs by default, so without this a
    /// stuck checkpoint looks like a deployment that is still working.
    #[tokio::test(start_paused = true)]
    async fn repeated_checkpoint_failures_reach_the_progress_callback() {
        let transport = GatewayTimeoutTransport {
            failures: 4,
            attempts: AtomicUsize::new(0),
        };
        let mut state = test_state();
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 1,
            operation: LoopOperation::Deploy,
            delay_strategy: DelayStrategy::Inline,
        };
        let notices = Arc::new(Mutex::new(Vec::new()));
        let seen = notices.clone();
        let on_progress: ProgressCallback = Box::new(move |progress| {
            if let Some(retry) = &progress.checkpoint_retry {
                seen.lock()
                    .expect("notices lock poisoned")
                    .push((retry.attempt, retry.notice()));
            }
        });

        run_step_loop(
            &mut state,
            &mut config,
            &ClientConfig::Test,
            "dep_test",
            &policy,
            &transport,
            None,
            Some(&on_progress),
        )
        .await
        .expect("the checkpoint is accepted on the fifth attempt");

        assert_eq!(transport.attempts.load(Ordering::SeqCst), 5);
        let notices = notices.lock().expect("notices lock poisoned");
        let attempts: Vec<usize> = notices.iter().map(|(attempt, _)| *attempt).collect();
        assert_eq!(attempts, vec![3, 4], "the first two failures stay quiet");
        for (_, notice) in notices.iter() {
            assert!(notice.contains("504 Gateway Timeout"), "{notice}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stops_immediately_when_checkpoint_rejection_is_terminal() {
        let transport = TerminalCheckpointTransport::default();
        let mut state = test_state();
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 2,
            operation: LoopOperation::Deploy,
            delay_strategy: DelayStrategy::Inline,
        };

        let error = run_step_loop(
            &mut state,
            &mut config,
            &ClientConfig::Test,
            "dep_test",
            &policy,
            &transport,
            None,
            None,
        )
        .await
        .expect_err("terminal checkpoint rejection must stop the active runner");

        assert_eq!(error.code, "DEPLOYMENT_CHECKPOINT_FAILED");
        assert!(!error.retryable);
        assert_eq!(transport.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn checkpoints_pending_step_failure_as_preflights_failed() {
        let transport = FailFirstCheckpointTransport::default();
        let mut state = test_state();
        state.platform = Platform::Aws;
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 1,
            operation: LoopOperation::Deploy,
            delay_strategy: DelayStrategy::Inline,
        };

        let result = run_step_loop(
            &mut state,
            &mut config,
            &ClientConfig::Test,
            "dep_test",
            &policy,
            &transport,
            None,
            None,
        )
        .await
        .expect("runner should checkpoint pending step error and return failed result");

        assert_eq!(result.steps_executed, 1);
        assert_eq!(result.loop_result.stop_reason, LoopStopReason::Failed);
        assert_eq!(state.status, DeploymentStatus::PreflightsFailed);
        assert!(
            state.error.is_some(),
            "deployment-level step error should be stored in DeploymentState"
        );
        assert_eq!(
            transport
                .checkpointed_statuses
                .lock()
                .expect("statuses lock poisoned")
                .as_slice(),
            &[
                DeploymentStatus::PreflightsFailed,
                DeploymentStatus::PreflightsFailed
            ],
            "failed pending state must be retried as the same durable checkpoint"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn invalid_quota_override_is_checkpointed_and_visible_on_create_and_update() {
        for (platform, initial, failed) in [
            (
                Platform::Test,
                DeploymentStatus::Pending,
                DeploymentStatus::PreflightsFailed,
            ),
            (
                Platform::Test,
                DeploymentStatus::UpdatePending,
                DeploymentStatus::UpdateFailed,
            ),
            (
                Platform::Aws,
                DeploymentStatus::UpdatePending,
                DeploymentStatus::UpdateFailed,
            ),
            (
                Platform::Gcp,
                DeploymentStatus::UpdatePending,
                DeploymentStatus::UpdateFailed,
            ),
            (
                Platform::Azure,
                DeploymentStatus::UpdatePending,
                DeploymentStatus::UpdateFailed,
            ),
        ] {
            let transport = FailFirstCheckpointTransport::default();
            let mut state = test_state();
            state.status = initial;
            state.platform = platform;
            state.protocol_version =
                alien_core::deployment_protocol_version_for_platform(platform, None);
            // An empty stack must reject an invalid setting too. Test credentials
            // keep this regression independent of cloud services and permissions.
            state.target_release.as_mut().unwrap().stack =
                Stack::new("quota-validation".to_string()).build();
            if initial == DeploymentStatus::UpdatePending {
                state.current_release = state.target_release.clone();
                state.stack_state = Some(StackState::new(platform));
            }
            let original_stack_state = serde_json::to_value(&state.stack_state).unwrap();
            let mut config = test_config();
            config.stack_settings.quota_warning_overrides = vec!["unknown-quota".to_string()];
            let policy = RunnerPolicy {
                max_steps: 2,
                ..RunnerPolicy::default()
            };

            let result = run_step_loop(
                &mut state,
                &mut config,
                &ClientConfig::Test,
                "dep_invalid_override",
                &policy,
                &transport,
                None,
                None,
            )
            .await
            .expect("invalid settings should be checkpointed as a semantic failure");

            assert_eq!(result.steps_executed, 1);
            assert!(result.state_persisted);
            assert_eq!(result.loop_result.stop_reason, LoopStopReason::Failed);
            assert_eq!(result.loop_result.outcome, LoopOutcome::Failure);
            assert_eq!(result.loop_result.final_status, failed);
            assert_eq!(state.status, failed);
            assert_eq!(
                serde_json::to_value(&state.stack_state).unwrap(),
                original_stack_state
            );
            assert!(
                state.runtime_metadata.is_none(),
                "no prepared provisioning state"
            );

            let checkpoints = transport.checkpointed_states.lock().unwrap();
            assert_eq!(checkpoints.len(), 2, "retry the failed checkpoint once");
            assert_eq!(checkpoints[0], checkpoints[1]);
            let restored: DeploymentState = serde_json::from_value(checkpoints[1].clone()).unwrap();
            let error = preserve_semantic_failure(Ok(result), &restored)
                .expect_err("finalization must retain the saved preflight failure")
                .into_external();
            assert!(!error.retryable);
            assert!(!error.internal);
            let report = error.human_report();
            let cause = report
                .causes
                .iter()
                .find(|cause| cause.code == "INVALID_QUOTA_OVERRIDE")
                .unwrap_or_else(|| panic!("{platform:?} {initial:?}: {report:?}"));
            assert!(cause.message.contains("unknown-quota"));
            let wire = serde_json::to_value(error).unwrap();
            assert!(wire.to_string().contains("INVALID_QUOTA_OVERRIDE"));
        }
    }

    #[tokio::test]
    async fn retry_requested_failed_status_runs_one_step() {
        let transport = FailFirstCheckpointTransport::default();
        let mut state = test_state();
        state.status = DeploymentStatus::ProvisioningFailed;
        state.retry_requested = true;
        state.stack_state = Some(StackState::new(Platform::Test));
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 1,
            operation: LoopOperation::Deploy,
            delay_strategy: DelayStrategy::Inline,
        };

        let result = run_step_loop(
            &mut state,
            &mut config,
            &ClientConfig::Test,
            "dep_test",
            &policy,
            &transport,
            None,
            None,
        )
        .await
        .expect("runner should step failed status when retry is requested");

        assert_eq!(result.steps_executed, 1);
        assert_eq!(state.status, DeploymentStatus::Provisioning);
        assert!(!state.retry_requested);
        assert_eq!(
            transport
                .checkpointed_statuses
                .lock()
                .expect("statuses lock poisoned")
                .as_slice(),
            &[
                DeploymentStatus::Provisioning,
                DeploymentStatus::Provisioning
            ],
        );
    }

    #[tokio::test]
    async fn failed_status_without_retry_stops_before_step() {
        let transport = FailFirstCheckpointTransport::default();
        let mut state = test_state();
        state.status = DeploymentStatus::ProvisioningFailed;
        state.stack_state = Some(StackState::new(Platform::Test));
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 1,
            operation: LoopOperation::Deploy,
            delay_strategy: DelayStrategy::Inline,
        };

        let result = run_step_loop(
            &mut state,
            &mut config,
            &ClientConfig::Test,
            "dep_test",
            &policy,
            &transport,
            None,
            None,
        )
        .await
        .expect("runner should classify failed status without stepping");

        assert_eq!(result.steps_executed, 0);
        assert_eq!(state.status, DeploymentStatus::ProvisioningFailed);
        assert_eq!(transport.attempts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn observe_only_state_stops_without_transport_or_steps() {
        let transport = FailFirstCheckpointTransport::default();
        let mut state = test_state();
        state.status = DeploymentStatus::Running;
        state.target_release = None;
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 1,
            operation: LoopOperation::Deploy,
            delay_strategy: DelayStrategy::Inline,
        };

        let result = run_running_refresh_step_loop(
            &mut state,
            &mut config,
            &ClientConfig::Test,
            "dep_test",
            &policy,
            &transport,
            None,
            None,
        )
        .await
        .expect("observe-only deployment should stop before step execution");

        assert_eq!(result.steps_executed, 0);
        assert_eq!(result.loop_result.stop_reason, LoopStopReason::NoWork);
        assert_eq!(result.loop_result.outcome, LoopOutcome::Neutral);
        assert_eq!(result.loop_result.final_status, DeploymentStatus::Running);
        assert_eq!(transport.attempts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn heartbeat_runs_one_step_from_running_or_refresh_failed() {
        for initial_status in [DeploymentStatus::Running, DeploymentStatus::RefreshFailed] {
            let transport = FailFirstCheckpointTransport::default();
            let mut state = test_state();
            state.status = initial_status;
            state.current_release = state.target_release.take();
            state.stack_state = Some(StackState::new(Platform::Test));
            let mut config = test_config();
            let policy = RunnerPolicy {
                max_steps: 1,
                operation: LoopOperation::Deploy,
                delay_strategy: DelayStrategy::Inline,
            };

            let result = run_running_refresh_step_loop(
                &mut state,
                &mut config,
                &ClientConfig::Test,
                "dep_test",
                &policy,
                &transport,
                None,
                None,
            )
            .await
            .expect("runner should run one heartbeat step even after a health failure");

            assert_eq!(result.steps_executed, 1);
            assert_eq!(state.status, DeploymentStatus::RefreshFailed);
            assert_eq!(
                transport
                    .checkpointed_statuses
                    .lock()
                    .expect("statuses lock poisoned")
                    .as_slice(),
                &[
                    DeploymentStatus::RefreshFailed,
                    DeploymentStatus::RefreshFailed
                ],
            );
        }
    }
}
