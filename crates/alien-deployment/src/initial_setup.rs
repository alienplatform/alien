use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{ResourceLifecycle, ResourceStatus, Stack, StackState, StackStatus};
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
use tracing::{debug, info};

/// Handle InitialSetup status (deploy setup-owned Frozen resources)
///
/// This step:
/// 1. Uses the prepared stack from runtime_metadata (mutated in Pending phase)
/// 2. Executes one deployment step for Frozen resources
/// 3. Updates stack state with the result
/// 4. Transitions to Provisioning when Frozen resources are deployed
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

    // Get runtime metadata (must exist from Pending phase). Mutable because
    // sync_secrets_to_vault updates the last-synced hash.
    let mut runtime_metadata = current.runtime_metadata.ok_or_else(|| {
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

    // Inject all environment variables — plain AND secrets.
    //
    // The secrets vault is a dependency of every compute resource (added by
    // SecretsVaultMutation as a link, and links ARE dependencies). The executor
    // won't start a function until its vault dependency is Running, so
    // ALIEN_SECRETS is always safe to inject. Secret values are synced to the
    // vault below, between the step where the vault becomes Running and the
    // step where compute resources start.
    crate::helpers::inject_environment_variables(&mut target_stack, &config)?;

    // Inject OTLP monitoring env vars if monitoring is configured
    if let Some(monitoring) = &config.monitoring {
        crate::helpers::inject_monitoring_environment_variables(&mut target_stack, monitoring)?;
    }

    // Sync secrets to vault if the vault is already Running (from a previous
    // step). The executor checks dependencies against the pre-step state, so a
    // vault that became Running in step N won't unblock dependents until step
    // N+1 — giving us this window to sync secrets before any function starts.
    let vault_is_running = stack_state
        .resources
        .get("secrets")
        .map(|r| r.status == ResourceStatus::Running)
        .unwrap_or(false);

    if vault_is_running {
        let synced = crate::helpers::sync_secrets_to_vault(
            &stack_state,
            &client_config,
            &config,
            &mut runtime_metadata,
        )
        .await?;

        if synced {
            info!("Secrets synced to vault during InitialSetup");
        }
    }

    // Deploy setup-owned resources during initial setup. Live resources are
    // created later in Provisioning using the permissions granted by setup.
    info!("Deploying frozen resources in initial setup");
    let executor = StackExecutor::builder(&target_stack, client_config)
        .deployment_config(&config)
        .service_provider(service_provider)
        .lifecycle_filter(vec![ResourceLifecycle::Frozen])
        .step_running_resources(false)
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

    // Compute status only for Frozen resources. A stack with no Frozen
    // resources can hand off immediately to Provisioning.
    let stack_status = compute_lifecycle_status(
        &target_stack,
        &step_result.next_state,
        ResourceLifecycle::Frozen,
    )
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to compute initial setup status".to_string(),
    })?;

    // Check if all resources are deployed
    let result = if stack_status == StackStatus::Running {
        info!("Initial setup complete (frozen resources deployed), transitioning to Provisioning");

        // Debug: log all resources in stack state to diagnose external binding persistence
        for (res_id, res_state) in &step_result.next_state.resources {
            debug!(
                resource_id = %res_id,
                status = ?res_state.status,
                has_outputs = res_state.outputs.is_some(),
                lifecycle = ?res_state.lifecycle,
                "InitialSetup complete: resource in stack state"
            );
        }

        // Note: Cross-account access setup happens in the manager after this step
        // The manager has access to the artifact registry binding

        let mut next = current_cloned;
        next.status = DeploymentStatus::Provisioning;
        next.stack_state = Some(step_result.next_state);
        next.runtime_metadata = Some(runtime_metadata);

        // Add a short delay before starting Provisioning to allow AWS IAM inline
        // policies (applied during InitialSetup via ApplyingResourcePermissions) to
        // propagate. IAM eventual consistency can take up to ~60s, but typically
        // settles within 10s. Without this delay, Provisioning may start immediately
        // and hit AccessDenied on the management role for newly-attached policies.
        DeploymentStepResult {
            state: next,
            error: None,
            suggested_delay_ms: Some(10_000),
            update_heartbeat: false,
            heartbeats: vec![],
        }
    } else if stack_status == StackStatus::Failure {
        info!("Initial setup failed");

        let mut next_state = step_result.next_state;

        let failed_resources: Vec<(String, String)> = next_state
            .resources
            .values()
            .filter(|r| {
                r.lifecycle == Some(ResourceLifecycle::Frozen)
                    && matches!(
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
            heartbeats: vec![],
        }
    } else {
        // Still in progress — log which Frozen resources are not yet running.
        let non_running =
            non_running_resources_for_lifecycle(&target_stack, &step_result.next_state);
        info!(
            "Initial setup in progress. Non-running resources: [{}]",
            non_running.join(", ")
        );

        let mut next = current_cloned;
        next.stack_state = Some(step_result.next_state);
        next.runtime_metadata = Some(runtime_metadata);

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

fn compute_lifecycle_status(
    stack: &Stack,
    stack_state: &StackState,
    lifecycle: ResourceLifecycle,
) -> alien_core::Result<StackStatus> {
    let statuses: Vec<ResourceStatus> = stack
        .resources()
        .filter(|(_, entry)| entry.lifecycle == lifecycle)
        .map(|(resource_id, _)| {
            stack_state
                .resources
                .get(resource_id)
                .map(|resource| resource.status)
                .unwrap_or(ResourceStatus::Pending)
        })
        .collect();

    if statuses.is_empty() {
        return Ok(StackStatus::Running);
    }

    StackState::compute_stack_status_from_resources(&statuses)
}

fn non_running_resources_for_lifecycle(stack: &Stack, stack_state: &StackState) -> Vec<String> {
    stack
        .resources()
        .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Frozen)
        .filter_map(|(resource_id, _)| {
            let status = stack_state
                .resources
                .get(resource_id)
                .map(|resource| resource.status)
                .unwrap_or(ResourceStatus::Pending);

            (status != ResourceStatus::Running).then(|| format!("{resource_id}({status:?})"))
        })
        .collect()
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
            heartbeats: vec![],
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
        heartbeats: vec![],
    })
}
