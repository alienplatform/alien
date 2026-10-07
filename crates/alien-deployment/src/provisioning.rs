use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{
    ComputeClusterOutputs, Platform, ResourceLifecycle, Stack, StackState, StackStatus,
};
use alien_error::{AlienError, Context};
use alien_infra::{RunningResourcePolicy, StackExecutor};
use tracing::{debug, info};

fn machines_deployment_has_zero_machines(platform: Platform, stack_state: &StackState) -> bool {
    platform == Platform::Machines
        && stack_state.resources.values().any(|resource| {
            resource
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<ComputeClusterOutputs>())
                .is_some_and(|outputs| outputs.total_machines == 0)
        })
}

/// Handle Provisioning status (deploy live resources)
///
/// This step:
/// 1. Uses the prepared stack from runtime_metadata (mutated in Pending phase)
/// 2. Syncs secrets to vault (vault was deployed during InitialSetup)
/// 3. Executes one deployment step for live resources
/// 4. Updates stack state with the result
/// 5. Transitions to Running when all live resources are Running
///
/// Note: This phase runs for both push=true (Manager) and push=false (Agent).
/// The only difference is who calls it - the Manager or the Agent.
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

    // Debug: log all resources in stack state at start of provisioning
    // to diagnose external binding resources disappearing between phases
    for (res_id, res_state) in &stack_state.resources {
        debug!(
            resource_id = %res_id,
            status = ?res_state.status,
            has_outputs = res_state.outputs.is_some(),
            lifecycle = ?res_state.lifecycle,
            "Provisioning: resource in stack state"
        );
    }

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

    // Check the deployer secret slots first: which are filled decides how
    // workloads read them (see inject_environment_variables).
    runtime_metadata.deployer_secrets = crate::helpers::check_deployer_secrets(
        &target_stack,
        &stack_state,
        &client_config,
        &config,
        current.platform,
    )
    .await?;

    // Inject environment variables into the prepared stack
    crate::helpers::inject_environment_variables(
        &mut target_stack,
        &config,
        current.platform,
        &runtime_metadata.deployer_secrets,
    )?;

    // Inject OTLP monitoring env vars if monitoring is configured
    if let Some(monitoring) = &config.monitoring {
        crate::helpers::inject_monitoring_environment_variables(
            &mut target_stack,
            monitoring,
            current.platform,
        )?;
    }

    // Sync secrets to vault before deploying workload resources.
    // The vault was deployed during InitialSetup and is now Running.
    // Hash check inside sync_secrets_to_vault prevents redundant cloud calls.
    let synced = crate::helpers::sync_secrets_to_vault(
        &target_stack,
        &stack_state,
        &client_config,
        &config,
        &mut runtime_metadata,
    )
    .await?;

    if synced {
        info!("Secrets synced to vault successfully");
    }

    // A required deployer secret the customer has not written blocks every
    // workload start. Nothing is deployed until it is; the reports above say
    // what is missing and where it goes.
    let blocking = crate::helpers::deployer_secrets_blocking_start(
        &target_stack,
        &config,
        current.platform,
        &runtime_metadata.deployer_secrets,
    );
    if !blocking.is_empty() {
        let summary = blocking
            .iter()
            .map(|report| report.summary())
            .collect::<Vec<_>>()
            .join(", ");
        info!(%summary, "Waiting for deployer secrets before starting workloads");

        next.status = DeploymentStatus::WaitingForSecrets;
        next.error =
            Some(AlienError::new(ErrorData::DeployerSecretsMissing { summary }).into_generic());
        next.runtime_metadata = Some(runtime_metadata);
        return Ok(DeploymentStepResult {
            state: next,
            suggested_delay_ms: Some(30_000),
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    // Create executor for live resources. Lifecycle filtering limits mutation
    // scope; already-running managed dependencies still run Ready handlers.
    let executor = StackExecutor::builder(&target_stack, client_config)
        .deployment_config(&config)
        .lifecycle_filter(vec![ResourceLifecycle::Live])
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .service_provider(service_provider)
        .build()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to create stack executor for live resources".to_string(),
        })?;

    // Captured before stepping: completion is judged over exactly what this executor
    // reconciles.
    let reconciled_ids = executor.tracked_resource_ids();

    // Execute one step
    let step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute deployment step for live resources".to_string(),
            })?;

    // Compute the stack status from the resulting state
    let mut stack_status =
        step_result
            .next_state
            .compute_stack_status()
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to compute stack status".to_string(),
            })?;

    // A create finishes with the config it started with. If the desired config changed while
    // it ran, the resource is Running on the old one and the next step plans its update.
    if stack_status == StackStatus::Running
        && !crate::updating::stack_has_converged(
            &step_result.next_state,
            &target_stack,
            &reconciled_ids,
        )
    {
        stack_status = StackStatus::InProgress;
    }

    // Check if all live resources are deployed
    let waiting_for_machines =
        machines_deployment_has_zero_machines(current.platform, &step_result.next_state);

    let result = if waiting_for_machines {
        info!("Machines deployment is waiting for the first machine to join");

        next.status = DeploymentStatus::WaitingForMachines;
        next.stack_state = Some(step_result.next_state);
        next.error = None;
        next.runtime_metadata = Some(runtime_metadata);

        DeploymentStepResult {
            state: next,
            suggested_delay_ms: Some(30_000),
            update_heartbeat: false,
            heartbeats: step_result.heartbeats,
            observed_inventory_batches: vec![],
        }
    } else if stack_status == StackStatus::Running {
        info!("All live resources deployed successfully, transitioning to Running");

        next.status = DeploymentStatus::Running;
        next.stack_state = Some(step_result.next_state);
        next.error = None;
        next.runtime_metadata = Some(runtime_metadata);

        // Promote target to current: deployment successful
        next.current_release = next.target_release.clone();
        next.target_release = None;

        DeploymentStepResult {
            state: next,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
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
        crate::helpers::interrupt_in_progress_resources(&mut next_state, &failed_refs, None);

        next.status = DeploymentStatus::ProvisioningFailed;
        next.stack_state = Some(next_state);
        next.error = None;
        next.runtime_metadata = Some(runtime_metadata);

        DeploymentStepResult {
            state: next,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        }
    } else {
        // Still in progress
        next.status = DeploymentStatus::Provisioning;
        next.stack_state = Some(step_result.next_state);
        next.runtime_metadata = Some(runtime_metadata);

        DeploymentStepResult {
            state: next,
            suggested_delay_ms: step_result.suggested_delay_ms,
            update_heartbeat: false,
            heartbeats: step_result.heartbeats,
            observed_inventory_batches: vec![],
        }
    };

    Ok(result)
}

/// Handle ProvisioningFailed status - retry failed resources and transition back to Provisioning
///
/// This step:
/// 1. Checks if retry_requested flag is set
/// 2. Resumes every failed resource whose config is unchanged at its saved step; changed
///    runtime resources are left to the planner, changed setup-owned ones refuse the retry
/// 3. Transitions back to Provisioning status
/// 4. Sets clear_retry_requested flag to clear the retry marker
pub async fn handle_provisioning_failed(
    current: DeploymentState,
    _target_stack: Stack,
    config: DeploymentConfig,
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
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    info!("Retrying failed resources");

    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for retry".to_string(),
        })
    })?;

    // Every failure whose config is unchanged resumes where it stopped. A changed runtime
    // resource is left to the planner, which updates or replaces it. A changed setup-owned one
    // needs setup, which this runtime executor never does for it, so the retry is refused.
    let outcome = crate::helpers::retry_failed_runtime_resources(
        &mut stack_state,
        current.runtime_metadata.as_ref(),
        &config,
    )?;
    let blocking = outcome
        .unresumed
        .iter()
        .filter(|failure| failure.setup_owned)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(error) = crate::helpers::retry_cannot_resume(&blocking) {
        info!(%error, "Retry refused");
        next.status = DeploymentStatus::ProvisioningFailed;
        next.error = Some(error.into_generic());
        next.retry_requested = false;
        return Ok(DeploymentStepResult {
            state: next,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    info!(
        "Retried {} failed resources: {:?}",
        outcome.retried.len(),
        outcome.retried
    );

    // Transition back to Provisioning to continue deployment
    next.status = DeploymentStatus::Provisioning;
    next.stack_state = Some(stack_state);
    next.error = None;
    next.retry_requested = false; // Clear retry flag directly

    Ok(DeploymentStepResult {
        state: next,
        suggested_delay_ms: None,
        update_heartbeat: false,
        heartbeats: vec![],
        observed_inventory_batches: vec![],
    })
}
