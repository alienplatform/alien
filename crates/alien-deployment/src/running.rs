use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::Stack;
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
use tracing::info;

/// Handle Running status (health checks)
///
/// This step:
/// 1. Uses the prepared stack from runtime_metadata (mutated in Pending/UpdatePending phase)
/// 2. Executes one step for each Running resource (performs health checks)
/// 3. Each controller's Ready handler will verify the resource is still healthy
/// 4. Returns RefreshFailed if any health checks fail
/// 5. Updates stack state with the new state after health checks
pub async fn handle_running(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling Running status (health check)");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Stack state is required
    let stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for health checks".to_string(),
        })
    })?;

    // Get runtime metadata (should exist from previous phases)
    let runtime_metadata = current.runtime_metadata.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Runtime metadata with prepared stack required for health checks".to_string(),
        })
    })?;

    // Use the prepared stack (already mutated in Pending phase)
    let mut target_stack = runtime_metadata.prepared_stack.clone().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Prepared stack not found in runtime metadata".to_string(),
        })
    })?;

    // Inject environment variables so the executor sees the same Function config
    // as what was deployed during Provisioning. Without this, the executor detects
    // a config mismatch (prepared_stack without env vars vs stack_state with env vars)
    // and incorrectly triggers an update flow.
    crate::helpers::inject_environment_variables(&mut target_stack, &config)?;

    // Inject OTLP monitoring env vars if monitoring is configured
    if let Some(monitoring) = &config.monitoring {
        crate::helpers::inject_monitoring_environment_variables(&mut target_stack, monitoring)?;
    }

    // TODO: Add mechanism to limit executor to only perform read-only health checks
    // and prevent any mutable operations on cloud resources during refresh.
    // This could be done by:
    // 1. Adding a "read-only" mode to StackExecutor
    // 2. Having controllers check this mode and skip any mutating operations
    // 3. Or creating a separate HealthCheckExecutor that only calls read methods

    // Create executor with the target stack configuration
    // No lifecycle filter - check all resources during health checks
    let executor = StackExecutor::builder(&target_stack, client_config)
        .deployment_config(&config)
        .service_provider(service_provider.clone())
        .build()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to create executor for health checks".to_string(),
        })?;

    // Execute one step - this will call the Ready handler for each Running resource
    // The Ready handlers perform heartbeat checks (e.g., verify function still exists, bucket is accessible)
    let step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute health check step".to_string(),
            })?;

    // Check if any resources transitioned to RefreshFailed
    let has_refresh_failed = step_result
        .next_state
        .resources
        .values()
        .any(|resource| resource.status == alien_core::ResourceStatus::RefreshFailed);

    if has_refresh_failed {
        info!("Health check failed for one or more resources");

        // Create aggregated error from failed resources
        let error =
            crate::helpers::create_aggregated_error_from_stack_state(&step_result.next_state);

        next.status = DeploymentStatus::RefreshFailed;
        next.stack_state = Some(step_result.next_state);

        Ok(DeploymentStepResult {
            state: next,
            error,
            suggested_delay_ms: None,
            update_heartbeat: false,
        })
    } else {
        info!("Health check passed for all resources");

        next.stack_state = Some(step_result.next_state);

        Ok(DeploymentStepResult {
            state: next,
            error: None,
            suggested_delay_ms: None,
            update_heartbeat: true, // Update heartbeat timestamp for Running status
        })
    }
}

/// Handle RefreshFailed status - retry failed resources and transition back to Running
///
/// This step:
/// 1. Checks if retry_requested flag is set
/// 2. Calls retry_failed() on stack state to recover failed resources
/// 3. Transitions back to Running status
/// 4. Sets clear_retry_requested flag to clear the retry marker
pub async fn handle_refresh_failed(
    current: DeploymentState,
    _target_stack: Stack,
    _config: DeploymentConfig,
    _client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling RefreshFailed status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Check if retry was requested
    if !current.retry_requested {
        info!("No retry requested, staying in RefreshFailed status");
        return Ok(DeploymentStepResult {
            state: current,
            error: None,
            suggested_delay_ms: None,
            update_heartbeat: false,
        });
    }

    info!("Retrying failed resources");

    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for retry".to_string(),
        })
    })?;

    // Retry failed resources using alien-infra
    use alien_infra::state_utils::StackStateExt;
    let retried = stack_state
        .retry_failed()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to retry failed resources".to_string(),
        })?;

    info!("Retried {} failed resources: {:?}", retried.len(), retried);

    // Transition back to Running
    next.status = DeploymentStatus::Running;
    next.stack_state = Some(stack_state);
    next.retry_requested = false; // Clear retry flag directly

    Ok(DeploymentStepResult {
        state: next,
        error: None,
        suggested_delay_ms: None,
        update_heartbeat: false,
    })
}
