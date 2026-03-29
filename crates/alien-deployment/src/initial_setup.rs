use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{Stack, StackStatus};
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
use tracing::info;

/// Handle InitialSetup status (deploy ALL resources)
///
/// This step:
/// 1. Uses the prepared stack from runtime_metadata (mutated in Pending phase)
/// 2. Executes one deployment step for all resources (Frozen + Live)
/// 3. Updates stack state with the result
/// 4. Transitions to Provisioning when all resources are deployed
///
/// Note: Stack settings are set during Pending phase and should not change mid-deployment.
pub async fn handle_initial_setup(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling InitialSetup status");

    // Clone current first before moving any fields
    let current_cloned = current.clone();

    // Stack state is required
    let stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for initial setup".to_string(),
        })
    })?;

    // Get runtime metadata (must exist from Pending phase)
    let runtime_metadata = current.runtime_metadata.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Runtime metadata with prepared stack required for initial setup".to_string(),
        })
    })?;

    // Use the prepared stack from Pending phase (already mutated)
    let mut target_stack = runtime_metadata.prepared_stack.clone().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Prepared stack not found in runtime metadata".to_string(),
        })
    })?;

    // Inject environment variables so Function/Container resources are created with the
    // correct config from the start. Without this, the Function is created without env vars
    // during InitialSetup, then Provisioning detects a config mismatch and triggers an
    // unwanted update (e.g., Cloud Run revision 00002) that may fail.
    crate::helpers::inject_environment_variables(&mut target_stack, &config)?;

    // Inject OTLP monitoring env vars if monitoring is configured
    if let Some(monitoring) = &config.monitoring {
        crate::helpers::inject_monitoring_environment_variables(&mut target_stack, monitoring)?;
    }

    // Deploy all resources (Frozen + Live) during initial setup
    info!("Deploying all resources in initial setup");
    let executor = StackExecutor::builder(&target_stack, client_config)
        .deployment_config(&config)
        .service_provider(service_provider)
        .build()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to create stack executor for initial setup".to_string(),
        })?;

    // Execute one step
    let step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute deployment step".to_string(),
            })?;

    // Compute the stack status from the resulting state
    let stack_status =
        step_result
            .next_state
            .compute_stack_status()
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to compute stack status".to_string(),
            })?;

    // Check if all resources are deployed
    let result = if stack_status == StackStatus::Running {
        info!("Initial setup complete (all resources deployed), transitioning to Provisioning");

        // Note: Secrets sync happens at the start of Provisioning phase
        // Note: Cross-account access setup happens in the manager after this step
        // The manager has access to the artifact registry binding

        let mut next = current_cloned;
        next.status = DeploymentStatus::Provisioning;
        next.stack_state = Some(step_result.next_state);
        next.runtime_metadata = Some(runtime_metadata);

        // Don't carry suggested_delay when transitioning phases - that delay is for heartbeat
        // polling within a phase, not for blocking the next phase
        DeploymentStepResult {
            state: next,
            error: None,
            suggested_delay_ms: None,
            update_heartbeat: false,
        }
    } else if stack_status == StackStatus::Failure {
        info!("Initial setup failed");

        let mut next_state = step_result.next_state;

        let failed_resources: Vec<(String, String)> = next_state
            .resources
            .values()
            .filter(|r| {
                matches!(
                    r.status,
                    alien_core::ResourceStatus::ProvisionFailed
                        | alien_core::ResourceStatus::UpdateFailed
                        | alien_core::ResourceStatus::DeleteFailed
                        | alien_core::ResourceStatus::RefreshFailed
                )
            })
            .map(|r| (r.config.id().to_string(), r.resource_type.clone()))
            .collect();

        let failed_refs: Vec<(&str, &str)> = failed_resources
            .iter()
            .map(|(id, t)| (id.as_str(), t.as_str()))
            .collect();

        crate::helpers::interrupt_in_progress_resources(&mut next_state, &failed_refs);

        // Create aggregated error from failed resources
        let error = crate::helpers::create_aggregated_error_from_stack_state(&next_state);

        let mut next = current_cloned;
        next.status = DeploymentStatus::InitialSetupFailed;
        next.stack_state = Some(next_state);
        next.runtime_metadata = Some(runtime_metadata);

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
        next.runtime_metadata = Some(runtime_metadata);

        DeploymentStepResult {
            state: next,
            error: None,
            suggested_delay_ms: step_result.suggested_delay_ms,
            update_heartbeat: false,
        }
    };

    Ok(result)
}

/// Handle InitialSetupFailed status - retry failed resources and transition back to InitialSetup
///
/// This step:
/// 1. Checks if retry_requested flag is set
/// 2. Calls retry_failed() on stack state to recover failed resources
/// 3. Transitions back to InitialSetup status
/// 4. Sets clear_retry_requested flag to clear the retry marker
pub async fn handle_initial_setup_failed(
    current: DeploymentState,
    _target_stack: Stack,
    _config: DeploymentConfig,
    _client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling InitialSetupFailed status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Check if retry was requested
    if !current.retry_requested {
        info!("No retry requested, staying in InitialSetupFailed status");
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

    // Transition back to InitialSetup to continue deployment
    next.status = DeploymentStatus::InitialSetup;
    next.stack_state = Some(stack_state);
    next.retry_requested = false; // Clear retry flag directly

    Ok(DeploymentStepResult {
        state: next,
        error: None,
        suggested_delay_ms: None,
        update_heartbeat: false,
    })
}
