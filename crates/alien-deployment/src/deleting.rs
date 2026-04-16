use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::StackStatus;
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
use tracing::info;

/// Handle DeletePending → Deleting transition
///
/// This step:
/// 1. Prepares stack for destroy (handles all resource states)
/// 2. Transitions to Deleting status
pub async fn handle_delete_pending(
    current: DeploymentState,
    _config: DeploymentConfig,
    _client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling DeletePending status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Stack state is required
    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for deletion".to_string(),
        })
    })?;

    // Prepare stack for destroy (handles failed resources appropriately)
    use alien_infra::state_utils::StackStateExt;
    let prepared = stack_state
        .prepare_for_destroy()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to prepare stack for destroy".to_string(),
        })?;

    info!(
        "Prepared {} resources for destroy: {:?}",
        prepared.len(),
        prepared
    );

    next.status = DeploymentStatus::Deleting;
    next.stack_state = Some(stack_state);

    Ok(DeploymentStepResult {
        state: next,
        error: None,
        suggested_delay_ms: None,
        update_heartbeat: false,
    })
}

/// Handle Deleting status (delete all resources)
///
/// This step:
/// 1. Executes one deletion step
/// 2. Updates stack state with the result
/// 3. Transitions to Deleted when all resources are deleted
pub async fn handle_deleting(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling Deleting status");

    // Clone current first before moving any fields
    let current_cloned = current.clone();

    // Stack state is required
    let stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for deletion".to_string(),
        })
    })?;

    // Note: Stack mutations are applied in lib.rs before dispatching to this handler
    // For deletion, we work with stack_state.resources which already contains
    // all deployed resources (including those added by mutations during initial deployment)

    // Create executor for deletion (all resources)
    let executor = StackExecutor::for_deletion_with_service_provider(
        client_config,
        &config,
        service_provider,
        None,
    )
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to create stack executor for deletion".to_string(),
    })?;

    // Execute one step
    let step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute deletion step".to_string(),
            })?;

    // Compute the stack status from the resulting state
    let stack_status =
        step_result
            .next_state
            .compute_stack_status()
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to compute stack status".to_string(),
            })?;

    // Check if all resources are deleted
    let result = if stack_status == StackStatus::Deleted {
        info!("All resources deleted successfully, transitioning to Deleted");

        // Note: Cross-account access removal happens in the manager after this step
        // The manager has access to the artifact registry binding

        let mut next = current_cloned;
        next.status = DeploymentStatus::Deleted;
        next.stack_state = Some(step_result.next_state);

        DeploymentStepResult {
            state: next,
            error: None,
            suggested_delay_ms: None,
            update_heartbeat: false,
        }
    } else if stack_status == StackStatus::Failure {
        info!("Deletion failed");

        // Create aggregated error from failed resources
        let error =
            crate::helpers::create_aggregated_error_from_stack_state(&step_result.next_state);

        let mut next = current_cloned;
        next.status = DeploymentStatus::DeleteFailed;
        next.stack_state = Some(step_result.next_state);

        DeploymentStepResult {
            state: next,
            error,
            suggested_delay_ms: None,
            update_heartbeat: false,
        }
    } else {
        // Still in progress
        let mut next = current_cloned;
        next.stack_state = Some(step_result.next_state);

        DeploymentStepResult {
            state: next,
            error: None,
            suggested_delay_ms: step_result.suggested_delay_ms,
            update_heartbeat: false,
        }
    };

    Ok(result)
}

/// Handle DeleteFailed status - prepare failed resources for destroy and transition back to Deleting
///
/// This step:
/// 1. Checks if retry_requested flag is set
/// 2. Calls prepare_for_destroy() on stack state to handle failed resources:
///    - ProvisionFailed/UpdateFailed resources: transition to delete start
///    - DeleteFailed resources: retry the delete operation
/// 3. Transitions back to Deleting status
/// 4. Sets clear_retry_requested flag to clear the retry marker
pub async fn handle_delete_failed(
    current: DeploymentState,
    _config: DeploymentConfig,
    _client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling DeleteFailed status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Check if retry was requested
    if !current.retry_requested {
        info!("No retry requested, staying in DeleteFailed status");
        return Ok(DeploymentStepResult {
            state: current,
            error: None,
            suggested_delay_ms: None,
            update_heartbeat: false,
        });
    }

    info!("Preparing stack for destroy");

    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for delete retry".to_string(),
        })
    })?;

    // Prepare stack for destroy (handles failed resources appropriately)
    use alien_infra::state_utils::StackStateExt;
    let prepared = stack_state
        .prepare_for_destroy()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to prepare stack for destroy".to_string(),
        })?;

    info!(
        "Prepared {} resources for destroy: {:?}",
        prepared.len(),
        prepared
    );

    // Transition back to Deleting
    next.status = DeploymentStatus::Deleting;
    next.stack_state = Some(stack_state);
    next.retry_requested = false; // Clear retry flag directly

    Ok(DeploymentStepResult {
        state: next,
        error: None,
        suggested_delay_ms: None,
        update_heartbeat: false,
    })
}
