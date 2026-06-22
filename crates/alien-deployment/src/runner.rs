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
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

const CHECKPOINT_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(1);
const CHECKPOINT_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);

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

/// Progress snapshot after each deployment step.
pub struct StepProgress<'a> {
    pub step_number: usize,
    pub status: DeploymentStatus,
    pub stack_state: Option<&'a StackState>,
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

/// Run one refresh step for a deployment whose initial status is already Running.
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
    if !state.has_desired() {
        let service_provider = service_provider
            .unwrap_or_else(|| Arc::new(alien_infra::DefaultPlatformServiceProvider::default()));
        let observe_report = run_observe_pass(
            state.platform,
            client_config,
            &service_provider,
            deployment_id,
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
        });
    }

    for step_count in 1..=policy.max_steps {
        // Pre-step terminal check
        if !should_step_retryable_failure(state) {
            let should_run_initial_running_refresh = allow_initial_running_step
                && step_count == 1
                && state.status == DeploymentStatus::Running;
            if let Some(result) = classify_status(&state.status, policy.operation) {
                if !should_run_initial_running_refresh {
                    return Ok(RunnerResult {
                        loop_result: result,
                        steps_executed: step_count - 1,
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

        let step_result = match step(
            state.clone(),
            config.clone(),
            client_config.clone(),
            service_provider.clone(),
        )
        .await
        {
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
                        Err(e) => {
                            warn!(
                                deployment_id = %deployment_id,
                                attempt = checkpoint_attempt,
                                retry_after_ms = checkpoint_delay.as_millis() as u64,
                                error = %e,
                                "Failed to checkpoint deployment error; retrying before returning failure"
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
                });
            }
        };

        // Capture step metadata before overwriting state.
        let update_heartbeat = step_result.update_heartbeat;
        let suggested_delay_ms = step_result.suggested_delay_ms;
        let heartbeats = step_result.heartbeats.clone();
        let mut observed_inventory_batches = step_result.observed_inventory_batches.clone();

        *state = step_result.state;

        if state.status == DeploymentStatus::Running {
            let observe_service_provider = service_provider.clone().unwrap_or_else(|| {
                Arc::new(alien_infra::DefaultPlatformServiceProvider::default())
            });
            let observe_report = run_observe_pass(
                state.platform,
                client_config,
                &observe_service_provider,
                deployment_id,
            )
            .await?;
            observed_inventory_batches.extend(observe_report.inventory_batches);
        }

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
                Err(e) => {
                    warn!(
                        deployment_id = %deployment_id,
                        attempt = checkpoint_attempt,
                        retry_after_ms = checkpoint_delay.as_millis() as u64,
                        error = %e,
                        "Failed to checkpoint deployment step; retrying before any further progress"
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
            });
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
        DeploymentStatus::Provisioning => DeploymentStatus::ProvisioningFailed,
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
        EnvironmentVariablesSnapshot, Platform, ReleaseInfo, ResourceEntry, ResourceLifecycle,
        Stack, StackSettings, StackState, Worker, WorkerCode, DEPLOYMENT_PROTOCOL_VERSION,
    };
    use alien_error::GenericError;
    use async_trait::async_trait;
    use indexmap::IndexMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    #[derive(Debug, Default)]
    struct FailFirstCheckpointTransport {
        attempts: AtomicUsize,
        checkpointed_statuses: Mutex<Vec<DeploymentStatus>>,
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

            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                return Err(alien_error::AlienError::new(GenericError {
                    message: "simulated checkpoint failure".to_string(),
                }));
            }

            Ok(StepReconcileResult {
                state: None,
                config: None,
            })
        }
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
                remote_access: false,
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
        }
    }

    fn test_state() -> DeploymentState {
        DeploymentState {
            status: DeploymentStatus::Pending,
            platform: Platform::Test,
            current_release: None,
            target_release: Some(ReleaseInfo {
                release_id: "rel_test".to_string(),
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

    fn test_config() -> DeploymentConfig {
        DeploymentConfig {
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
            compute_backend: None,
            allow_frozen_changes: false,
            domain_metadata: None,
            public_endpoints: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn retries_failed_checkpoint_before_next_step() {
        let transport = FailFirstCheckpointTransport::default();
        let mut state = test_state();
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 1,
            operation: LoopOperation::Deploy,
            delay_threshold: None,
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

    #[tokio::test(start_paused = true)]
    async fn checkpoints_pending_step_failure_as_preflights_failed() {
        let transport = FailFirstCheckpointTransport::default();
        let mut state = test_state();
        state.platform = Platform::Aws;
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 1,
            operation: LoopOperation::Deploy,
            delay_threshold: None,
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
            delay_threshold: None,
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
            delay_threshold: None,
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
            delay_threshold: None,
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
    async fn initial_running_runs_one_step_when_policy_allows_running_start() {
        let transport = FailFirstCheckpointTransport::default();
        let mut state = test_state();
        state.status = DeploymentStatus::Running;
        state.stack_state = Some(StackState::new(Platform::Test));
        let mut config = test_config();
        let policy = RunnerPolicy {
            max_steps: 1,
            operation: LoopOperation::Deploy,
            delay_threshold: None,
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
        .expect("runner should run one heartbeat step for an initially running deployment");

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
