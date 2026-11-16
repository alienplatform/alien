//! Runs a pending operator request on the resource it names.
//!
//! The request is stepped instead of the status handler, whatever the deployment's active or
//! failed status is. It never plans, never changes the status or the deployment error, and it
//! is cleared from the state only when the controller reports it done or refused.

use crate::{DeploymentConfig, DeploymentState, DeploymentStepResult, ErrorData, Result};
use alien_core::OperatorRequestProgress;
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
use tracing::{info, warn};

/// Whether the runner must step the pending request instead of the status handler.
///
/// Only a deployment that is running, failed after running or waiting for its queued update
/// takes requests (the update waits until the request ends); every other status has its own
/// flow running, and the manager refuses a request then. A retry that is
/// already requested goes first, so a request never overtakes it.
pub(crate) fn is_pending(state: &DeploymentState) -> bool {
    use alien_core::DeploymentStatus::{RefreshFailed, Running, UpdateFailed, UpdatePending};
    state.operator_request.is_some()
        && !state.retry_requested
        && matches!(
            state.status,
            Running | RefreshFailed | UpdateFailed | UpdatePending
        )
}

/// Steps the pending request once and returns the state with the controller's progress.
pub(crate) async fn step_pending(
    current: DeploymentState,
    mut config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    let request = current.operator_request.clone().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "No operator request to step".to_string(),
        })
    })?;
    config.release_statements = crate::helpers::installed_release_statements(&current);

    let stack_state = current.stack_state.clone().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for an operator request".to_string(),
        })
    })?;
    let runtime_metadata = current.runtime_metadata.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Runtime metadata with a prepared stack required for an operator request"
                .to_string(),
        })
    })?;
    // The executor only needs a stack to be built on: the request is never planned against it.
    let stack = runtime_metadata
        .pending_prepared_stack
        .as_ref()
        .or(runtime_metadata.prepared_stack.as_ref())
        .ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Prepared stack not found in runtime metadata".to_string(),
            })
        })?;

    let executor = StackExecutor::builder(stack, client_config)
        .deployment_config(&config)
        .maybe_rollout_id(
            current
                .current_release
                .as_ref()
                .and_then(|release| release.release_id.clone()),
        )
        .service_provider(service_provider)
        .build()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to create executor for an operator request".to_string(),
        })?;

    let step = executor
        .step_requested(stack_state, &request)
        .await
        .context(ErrorData::StackExecutionFailed {
            message: format!("Failed to step the '{}' request", request.kind_name()),
        })?;

    let mut next = current;
    next.stack_state = Some(step.state);
    let suggested_delay_ms = match step.progress {
        OperatorRequestProgress::InProgress { waiting_for, delay } => {
            info!(request_id = %request.id, kind = request.kind_name(), %waiting_for, "Operator request in progress");
            Some(delay.as_millis() as u64)
        }
        OperatorRequestProgress::Done { summary } => {
            info!(request_id = %request.id, kind = request.kind_name(), %summary, "Operator request done");
            next.operator_request = None;
            Some(0)
        }
        OperatorRequestProgress::Refused { reason } => {
            warn!(request_id = %request.id, kind = request.kind_name(), %reason, "Operator request refused");
            next.operator_request = None;
            Some(0)
        }
    };

    Ok(DeploymentStepResult {
        state: next,
        suggested_delay_ms,
        update_heartbeat: false,
        heartbeats: step.heartbeats,
        observed_inventory_batches: vec![],
    })
}
