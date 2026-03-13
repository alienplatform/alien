use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{ResourceLifecycle, Stack, StackStatus};
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
use tracing::{debug, info};

/// Handle Provisioning status (deploy live resources)
///
/// This step:
/// 1. Uses the prepared stack from runtime_metadata (mutated in Pending phase)
/// 2. Syncs secrets to vault (vault was deployed during InitialSetup)
/// 3. Executes one deployment step for live resources
/// 4. Updates stack state with the result
/// 5. Transitions to Running when all live resources are Running
///
/// Note: This phase runs for both push=true (Agent Manager) and push=false (Operator).
/// The only difference is who calls it - the Agent Manager or the Operator.
///
/// Note: Stack settings are set during Pending phase and should not change mid-deployment.
pub async fn handle_provisioning(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling Provisioning status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Stack state is required
    let stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for provisioning".to_string(),
        })
    })?;

    // Get runtime metadata (must exist from Pending phase)
    let mut runtime_metadata = current.runtime_metadata.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Runtime metadata with prepared stack required for provisioning".to_string(),
        })
    })?;

    // Use the prepared stack from Pending phase (already mutated)
    let mut target_stack = runtime_metadata.prepared_stack.clone().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Prepared stack not found in runtime metadata".to_string(),
        })
    })?;

    // Stamp deployment-config values onto ContainerCluster template inputs.
    // Runs every step (not just during preflights) so the executor sees the latest
    // DeploymentConfig values — e.g., a new horizond binary ETag after recompilation.
    crate::helpers::stamp_template_inputs(&mut target_stack, &config)?;

    // Inject environment variables into the prepared stack
    crate::helpers::inject_environment_variables(&mut target_stack, &config)?;

    // Inject OTLP monitoring env vars if monitoring is configured
    if let Some(monitoring) = &config.monitoring {
        crate::helpers::inject_monitoring_environment_variables(&mut target_stack, monitoring)?;
    }

    // Sync secrets to vault before deploying functions
    // The vault was deployed during InitialSetup and is now Running
    // This checks the hash and only syncs if needed
    info!("Syncing secrets to vault before deploying live resources");
    let synced = crate::helpers::sync_secrets_to_vault(
        &stack_state,
        &client_config,
        &config,
        &mut runtime_metadata,
    )
    .await?;

    if synced {
        info!("Secrets synced successfully");
    } else {
        debug!("Secrets already synced, continuing with deployment");
    }

    // Create executor for live resources only
    let executor = StackExecutor::builder(&target_stack, client_config)
        .deployment_config(&config)
        .lifecycle_filter(vec![
            ResourceLifecycle::Live,
            ResourceLifecycle::LiveOnSetup,
        ])
        .service_provider(service_provider)
        .build()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to create stack executor for live resources".to_string(),
        })?;

    // Execute one step
    let step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute deployment step for live resources".to_string(),
            })?;

    // Compute the stack status from the resulting state
    let stack_status =
        step_result
            .next_state
            .compute_stack_status()
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to compute stack status".to_string(),
            })?;

    // Check if all live resources are deployed
    let result = if stack_status == StackStatus::Running {
        info!("All live resources deployed successfully, transitioning to Running");

        next.status = DeploymentStatus::Running;
        next.stack_state = Some(step_result.next_state);
        next.runtime_metadata = Some(runtime_metadata);

        // Promote target to current: deployment successful
        next.current_release = next.target_release.clone();
        next.target_release = None;

        DeploymentStepResult {
            state: next,
            error: None,
            suggested_delay_ms: None,
            update_heartbeat: false,
        }
    } else if stack_status == StackStatus::Failure {
        info!("Live resource deployment failed");

        let mut next_state = step_result.next_state;

        // Collect the IDs/types of resources that actually failed (not interrupted).
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

        // Interrupt all in-progress resources so every resource reflects its true status.
        crate::helpers::interrupt_in_progress_resources(&mut next_state, &failed_refs);

        // Create aggregated error from failed resources
        let error = crate::helpers::create_aggregated_error_from_stack_state(&next_state);

        next.status = DeploymentStatus::ProvisioningFailed;
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

/// Handle ProvisioningFailed status - retry failed resources and transition back to Provisioning
///
/// This step:
/// 1. Checks if retry_requested flag is set
/// 2. Calls retry_failed() on stack state to recover failed resources
/// 3. Transitions back to Provisioning status
/// 4. Sets clear_retry_requested flag to clear the retry marker
pub async fn handle_provisioning_failed(
    current: DeploymentState,
    _target_stack: Stack,
    _config: DeploymentConfig,
    _client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling ProvisioningFailed status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Check if retry was requested
    if !current.retry_requested {
        info!("No retry requested, staying in ProvisioningFailed status");
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

    // Transition back to Provisioning to continue deployment
    next.status = DeploymentStatus::Provisioning;
    next.stack_state = Some(stack_state);
    next.retry_requested = false; // Clear retry flag directly

    Ok(DeploymentStepResult {
        state: next,
        error: None,
        suggested_delay_ms: None,
        update_heartbeat: false,
    })
}
