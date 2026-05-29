use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{DeleteScope, ResourceLifecycle, ResourceStatus, StackState, StackStatus};
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
    let delete_scope = delete_scope(&current)?;

    // Stack state is required
    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for deletion".to_string(),
        })
    })?;

    // Prepare stack for destroy (handles failed resources appropriately)
    use alien_infra::state_utils::StackStateExt;
    let prepared = match delete_scope {
        DeleteScope::Full => stack_state.prepare_for_destroy(),
        DeleteScope::LiveOnly => {
            stack_state.prepare_for_destroy_with_lifecycle_filter(&[ResourceLifecycle::Live])
        }
    }
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
        heartbeats: vec![],
    })
}

/// Handle Deleting status.
///
/// This step:
/// 1. Executes one deletion step
/// 2. Updates stack state with the result
/// 3. Transitions to Deleted when the selected delete scope is deleted
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

    let delete_scope = delete_scope(&current_cloned)?;
    let lifecycle_filter = match delete_scope {
        DeleteScope::Full => None,
        DeleteScope::LiveOnly => Some(vec![ResourceLifecycle::Live]),
    };

    // Create executor for deletion.
    let executor = StackExecutor::for_deletion_with_service_provider(
        client_config,
        &config,
        service_provider,
        lifecycle_filter,
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

    // Compute status for the selected delete scope.
    let stack_status = compute_delete_scope_status(&step_result.next_state, delete_scope).context(
        ErrorData::StackExecutionFailed {
            message: "Failed to compute stack status".to_string(),
        },
    )?;

    // Check if all resources are deleted
    let result = if stack_status == StackStatus::Deleted {
        info!(delete_scope = ?delete_scope, "Delete scope completed, transitioning to Deleted");

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
            heartbeats: vec![],
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
            heartbeats: vec![],
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
            heartbeats: step_result.heartbeats,
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
            heartbeats: vec![],
        });
    }

    info!("Preparing stack for destroy");
    let delete_scope = delete_scope(&current)?;

    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for delete retry".to_string(),
        })
    })?;

    // Prepare stack for destroy (handles failed resources appropriately)
    use alien_infra::state_utils::StackStateExt;
    let prepared = match delete_scope {
        DeleteScope::Full => stack_state.prepare_for_destroy(),
        DeleteScope::LiveOnly => {
            stack_state.prepare_for_destroy_with_lifecycle_filter(&[ResourceLifecycle::Live])
        }
    }
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
        heartbeats: vec![],
    })
}

fn delete_scope(state: &DeploymentState) -> Result<DeleteScope> {
    state
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.delete_scope)
        .ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "deleteScope is required before deleting a deployment".to_string(),
            })
        })
}

fn compute_delete_scope_status(
    stack_state: &alien_core::StackState,
    delete_scope: DeleteScope,
) -> alien_core::Result<StackStatus> {
    match delete_scope {
        DeleteScope::Full => stack_state.compute_stack_status(),
        DeleteScope::LiveOnly => {
            let statuses: Vec<ResourceStatus> = stack_state
                .resources
                .values()
                .filter(|resource| resource.lifecycle == Some(ResourceLifecycle::Live))
                .map(|resource| resource.status)
                .collect();

            if statuses.is_empty() {
                return Ok(StackStatus::Deleted);
            }

            StackState::compute_stack_status_from_resources(&statuses)
        }
    }
}
