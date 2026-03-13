use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{ResourceLifecycle, Stack, StackStatus};
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
use tracing::{debug, info};

/// Handle UpdatePending → Updating transition
///
/// This step:
/// 1. Updates stack settings from config if they changed
/// 2. Runs compatibility checks between old and new stacks (comparing mutated stacks)
/// 3. Runs runtime checks to verify environment is ready for update
/// 4. Transitions to Updating status
pub async fn handle_update_pending(
    current: DeploymentState,
    target_stack: Stack,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling UpdatePending status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Stack state is required
    let stack_state = current.stack_state.clone().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for update".to_string(),
        })
    })?;

    let runner = alien_preflights::runner::PreflightRunner::new();

    // For compatibility checks, use the prepared (mutated) stack from the previous deployment
    // if available. This ensures we compare mutated stacks (old mutated vs new mutated),
    // not raw user stacks. Falls back to current_release.stack if prepared_stack isn't available
    // (for backward compatibility with existing deployments).
    let old_stack_for_comparison = current
        .runtime_metadata
        .as_ref()
        .and_then(|m| m.prepared_stack.as_ref())
        .or(current.current_release.as_ref().map(|r| &r.stack));

    // Run deployment-time preflights: compatibility checks + mutations + runtime checks
    // Store the mutated stack to use for the actual update and for future compatibility checks
    // Pass allow_frozen_changes flag to skip frozen resource checks if requested
    let (mutated_stack, _deployment_summary) = runner
        .run_deployment_time_preflights(
            target_stack.clone(),
            &stack_state,
            &config,
            &client_config,
            old_stack_for_comparison, // Pass old mutated stack for compatibility checks
            config.allow_frozen_changes, // Skip frozen check if flag is set
        )
        .await
        .context(ErrorData::PreflightChecksFailed)?;

    info!("Deployment-time preflight checks completed successfully");

    // Store the mutated stack in runtime_metadata for future compatibility checks
    let mut runtime_metadata = current.runtime_metadata.unwrap_or_default();
    runtime_metadata.prepared_stack = Some(mutated_stack);

    // Transition to Updating
    next.status = DeploymentStatus::Updating;
    next.stack_state = Some(stack_state);
    next.runtime_metadata = Some(runtime_metadata);

    Ok(DeploymentStepResult {
        state: next,
        error: None,
        suggested_delay_ms: None,
        update_heartbeat: false,
    })
}

/// Handle Updating status (update live resources)
///
/// This step:
/// 1. Uses the prepared stack from runtime_metadata (mutated in UpdatePending phase)
/// 2. Executes one deployment step for live resources only
/// 3. Updates stack state with the result
/// 4. Transitions to Running when complete
///
/// Note: Stack settings are updated during UpdatePending phase and should not change mid-update.
pub async fn handle_updating(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling Updating status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Stack state is required
    let stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for updating".to_string(),
        })
    })?;

    // Get runtime metadata (must exist from UpdatePending phase)
    let mut runtime_metadata = current.runtime_metadata.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Runtime metadata with prepared stack required for updating".to_string(),
        })
    })?;

    // Use the prepared stack from UpdatePending phase (already mutated)
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

    // Sync secrets to vault before updating functions
    // The vault is Running and secrets may have been updated
    // This checks the hash and only syncs if needed
    info!("Syncing secrets to vault before updating live resources");
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
        debug!("Secrets already synced, continuing with update");
    }

    // Create executor for resources
    // By default, only deploy live resources (frozen resources don't change)
    // If allow_frozen_changes is true, also deploy frozen resources
    let mut lifecycle_filter_vec = vec![ResourceLifecycle::Live, ResourceLifecycle::LiveOnSetup];
    if config.allow_frozen_changes {
        info!("Including frozen resources in update (allow_frozen_changes=true)");
        lifecycle_filter_vec.push(ResourceLifecycle::Frozen);
    }

    let executor = StackExecutor::builder(&target_stack, client_config)
        .deployment_config(&config)
        .lifecycle_filter(lifecycle_filter_vec)
        .service_provider(service_provider)
        .build()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to create stack executor for update".to_string(),
        })?;

    // Execute one step
    let step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute update step".to_string(),
            })?;

    // Compute the stack status from the resulting state
    let stack_status =
        step_result
            .next_state
            .compute_stack_status()
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to compute stack status".to_string(),
            })?;

    // Check if update is complete
    let result = if stack_status == StackStatus::Running {
        info!("Update completed successfully, transitioning to Running");

        next.status = DeploymentStatus::Running;
        next.stack_state = Some(step_result.next_state);
        next.runtime_metadata = Some(runtime_metadata);
        // Promote target to current: update successful
        next.current_release = next.target_release.clone();
        next.target_release = None;

        DeploymentStepResult {
            state: next,
            error: None,
            suggested_delay_ms: None,
            update_heartbeat: false,
        }
    } else if stack_status == StackStatus::Failure {
        info!("Update failed");

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

        next.status = DeploymentStatus::UpdateFailed;
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

/// Handle UpdateFailed status - retry failed resources and re-run preflights
///
/// This step:
/// 1. Checks if retry_requested flag is set
/// 2. Calls retry_failed() on stack state to recover failed resources
/// 3. Transitions back to UpdatePending to re-run preflights
/// 4. Sets clear_retry_requested flag to clear the retry marker
///
/// Note: We transition to UpdatePending (not Updating) because update failures
/// might indicate compatibility issues that preflights should re-validate.
pub async fn handle_update_failed(
    current: DeploymentState,
    _target_stack: Stack,
    _config: DeploymentConfig,
    _client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling UpdateFailed status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Check if retry was requested
    if !current.retry_requested {
        info!("No retry requested, staying in UpdateFailed status");
        return Ok(DeploymentStepResult {
            state: current,
            error: None,
            suggested_delay_ms: None,
            update_heartbeat: false,
        });
    }

    info!("Retrying failed resources and re-running preflights");

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

    // Transition back to UpdatePending to re-run preflights
    next.status = DeploymentStatus::UpdatePending;
    next.stack_state = Some(stack_state);
    next.retry_requested = false; // Clear retry flag directly

    Ok(DeploymentStepResult {
        state: next,
        error: None,
        suggested_delay_ms: None,
        update_heartbeat: false,
    })
}
