use std::{sync::Arc, time::Duration};

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, ResourceLifecycle,
    StackState, StackStatus,
};
use alien_error::{AlienError, Context};
use alien_infra::{state_utils::StackStateExt, StackExecutor};
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::{
    loop_contract::{LoopOperation, LoopOutcome, LoopResult, LoopStopReason},
    runner::{RunnerPolicy, RunnerResult},
    transport::DeploymentLoopTransport,
    ErrorData, Result,
};

/// Run privileged setup teardown after runtime cleanup reached the handoff
/// point.
///
/// This is intentionally separate from the normal deployment step machine:
/// managers and agents stop at `TeardownRequired`, while setup-authority
/// callers such as the CLI can continue with their own credentials.
pub async fn run_setup_teardown_after_handoff(
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    policy: &RunnerPolicy,
    transport: &dyn DeploymentLoopTransport,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> Result<Option<RunnerResult>> {
    if !matches!(
        state.status,
        DeploymentStatus::TeardownRequired | DeploymentStatus::TeardownFailed
    ) {
        return Ok(None);
    }

    if policy.operation != LoopOperation::Delete {
        return Err(AlienError::new(ErrorData::MissingConfiguration {
            message: "Setup teardown can only run during delete operations".to_string(),
        }));
    }

    info!(deployment_id = %deployment_id, "Starting setup-owned teardown");
    state.status = DeploymentStatus::TeardownRequired;
    state.error = None;

    let mut stack_state = state.stack_state.take().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for setup teardown".to_string(),
        })
    })?;

    let prepared = match stack_state
        .prepare_for_teardown_with_lifecycle_filter(&[ResourceLifecycle::Frozen])
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to prepare setup-owned resources for teardown".to_string(),
        }) {
        Ok(prepared) => prepared,
        Err(error) => {
            state.stack_state = Some(stack_state);
            fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;
            return Err(error);
        }
    };
    info!(
        deployment_id = %deployment_id,
        prepared_count = prepared.len(),
        prepared = ?prepared,
        "Prepared setup-owned resources for teardown"
    );

    state.stack_state = Some(stack_state);
    checkpoint_setup_teardown_state(deployment_id, state, config, transport, None, Vec::new())
        .await?;

    let service_provider = service_provider
        .unwrap_or_else(|| Arc::new(alien_infra::DefaultPlatformServiceProvider::default()));
    let executor = StackExecutor::for_deletion_with_service_provider(
        client_config.clone(),
        config,
        service_provider,
        Some(vec![ResourceLifecycle::Frozen]),
    )
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to create stack executor for setup teardown".to_string(),
    })?;

    for step_count in 1..=policy.max_steps {
        let status = setup_teardown_status(state.stack_state.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Stack state required during setup teardown".to_string(),
            })
        })?)?;

        match status {
            StackStatus::Deleted => {
                state.status = DeploymentStatus::Deleted;
                state.error = None;
                checkpoint_setup_teardown_state(
                    deployment_id,
                    state,
                    config,
                    transport,
                    None,
                    Vec::new(),
                )
                .await?;
                return Ok(Some(RunnerResult {
                    loop_result: LoopResult {
                        stop_reason: LoopStopReason::Deleted,
                        outcome: LoopOutcome::Success,
                        final_status: state.status,
                    },
                    steps_executed: step_count - 1,
                }));
            }
            StackStatus::Failure => {
                let error = AlienError::new(ErrorData::StackExecutionFailed {
                    message: "Setup-owned resource teardown failed".to_string(),
                });
                fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;
                return Err(error);
            }
            StackStatus::Pending | StackStatus::InProgress | StackStatus::Running => {}
        }

        let current_stack_state = state.stack_state.take().ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Stack state required for setup teardown step".to_string(),
            })
        })?;
        let stack_state_before_step = current_stack_state.clone();
        let step_result = match executor.step(current_stack_state).await.context(
            ErrorData::StackExecutionFailed {
                message: "Failed to execute setup teardown step".to_string(),
            },
        ) {
            Ok(step_result) => step_result,
            Err(error) => {
                state.stack_state = Some(stack_state_before_step);
                fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;
                return Err(error);
            }
        };
        let suggested_delay_ms = step_result.suggested_delay_ms;
        let heartbeats = step_result.heartbeats.clone();
        state.stack_state = Some(step_result.next_state);

        checkpoint_setup_teardown_state(
            deployment_id,
            state,
            config,
            transport,
            suggested_delay_ms,
            heartbeats,
        )
        .await?;

        if let Some(delay_ms) = suggested_delay_ms {
            if let Some(threshold) = policy.delay_threshold {
                if Duration::from_millis(delay_ms) > threshold {
                    debug!(
                        deployment_id = %deployment_id,
                        delay_ms = delay_ms,
                        "Setup teardown step delay exceeds threshold; yielding"
                    );
                    return Ok(Some(RunnerResult {
                        loop_result: LoopResult {
                            stop_reason: LoopStopReason::Synced,
                            outcome: LoopOutcome::Neutral,
                            final_status: state.status,
                        },
                        steps_executed: step_count,
                    }));
                }
            }
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    let error = AlienError::new(ErrorData::StackExecutionFailed {
        message: format!(
            "Setup-owned resource teardown did not complete within {} steps",
            policy.max_steps
        ),
    });
    fail_setup_teardown(deployment_id, state, config, transport, error.clone()).await?;

    Ok(Some(RunnerResult {
        loop_result: LoopResult {
            stop_reason: LoopStopReason::BudgetExceeded,
            outcome: LoopOutcome::Failure,
            final_status: state.status,
        },
        steps_executed: policy.max_steps,
    }))
}

async fn fail_setup_teardown(
    deployment_id: &str,
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    transport: &dyn DeploymentLoopTransport,
    error: AlienError<ErrorData>,
) -> Result<()> {
    state.status = DeploymentStatus::TeardownFailed;
    state.error = Some(error.into_generic());
    checkpoint_setup_teardown_state(deployment_id, state, config, transport, None, Vec::new()).await
}

async fn checkpoint_setup_teardown_state(
    deployment_id: &str,
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    transport: &dyn DeploymentLoopTransport,
    suggested_delay_ms: Option<u64>,
    heartbeats: Vec<alien_core::ResourceHeartbeat>,
) -> Result<()> {
    match transport
        .reconcile_step(
            deployment_id,
            state,
            config,
            false,
            suggested_delay_ms,
            heartbeats,
            Vec::new(),
        )
        .await
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to checkpoint setup teardown".to_string(),
        }) {
        Ok(reconciled) => {
            if let Some(updated_state) = reconciled.state {
                *state = updated_state;
            }
            if let Some(updated_config) = reconciled.config {
                *config = updated_config;
            }
            Ok(())
        }
        Err(error) => {
            warn!(deployment_id = %deployment_id, error = %error, "Failed to checkpoint setup teardown");
            Err(error)
        }
    }
}

fn setup_teardown_status(stack_state: &StackState) -> Result<StackStatus> {
    let mut statuses = Vec::new();
    for resource in stack_state.resources.values() {
        match resource.lifecycle {
            Some(ResourceLifecycle::Frozen) => statuses.push(resource.status),
            Some(ResourceLifecycle::Live) => {}
            None => {
                return Err(AlienError::new(ErrorData::MissingConfiguration {
                    message: format!(
                        "Resource '{}' is missing lifecycle metadata required for setup teardown",
                        resource.config.id()
                    ),
                }));
            }
        }
    }

    if statuses.is_empty() {
        return Ok(StackStatus::Deleted);
    }

    StackState::compute_stack_status_from_resources(&statuses).context(
        ErrorData::StackExecutionFailed {
            message: "Failed to compute setup teardown status".to_string(),
        },
    )
}
