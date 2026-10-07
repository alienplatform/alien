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

    let reconciled_ids = executor.tracked_resource_ids();

    // Execute one step
    let step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute deployment step for live resources".to_string(),
            })?;

    // Use the same target-aware completion policy as updates: deleted records stay
    // durable, while desired drift and deferred deletions remain outstanding work.
    let pending_deletions = executor
        .pending_deletions(&step_result.next_state)
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to determine outstanding provisioning deletions".to_string(),
        })?;
    let stack_status = crate::updating::compute_update_status(
        &step_result.next_state,
        &target_stack,
        &reconciled_ids,
        &pending_deletions,
    )?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        ClientConfig, EnvironmentVariablesSnapshot, ExternalBindings, ReleaseInfo, ResourceRef,
        ResourceStatus, RuntimeMetadata, StackSettings, Storage,
    };
    use alien_infra::{DefaultPlatformServiceProvider, StackResourceStateExt};
    use std::sync::Arc;

    fn config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build()
    }

    fn stack(ids: &[&str]) -> Stack {
        let mut stack = Stack::new("provisioning-test".to_string()).build();
        for id in ids {
            let entry = Stack::new("entry".to_string())
                .add(
                    Storage::new((*id).to_string()).build(),
                    ResourceLifecycle::Live,
                )
                .build()
                .resources
                .shift_remove(*id)
                .unwrap();
            stack.resources.insert((*id).to_string(), entry);
        }
        stack
    }

    async fn installed(stack: &Stack) -> StackState {
        let executor = StackExecutor::builder(stack, ClientConfig::Test)
            .deployment_config(&config())
            .build()
            .unwrap();
        let result = executor
            .run_until_synced(StackState::new(Platform::Test))
            .await;
        assert!(result.success, "{:?}", result.error);
        result.final_state
    }

    fn provisioning(state: StackState, target: Stack) -> DeploymentState {
        DeploymentState {
            status: DeploymentStatus::Provisioning,
            platform: Platform::Test,
            current_release: None,
            target_release: Some(ReleaseInfo {
                release_id: Some("target-release".to_string()),
                version: None,
                description: None,
                stack: target.clone(),
            }),
            stack_state: Some(state),
            error: None,
            environment_info: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            runtime_metadata: Some(RuntimeMetadata {
                prepared_stack: Some(target),
                ..Default::default()
            }),
        }
    }

    async fn step(state: DeploymentState) -> DeploymentState {
        handle_provisioning(
            state,
            config(),
            ClientConfig::Test,
            Arc::new(DefaultPlatformServiceProvider::default()),
        )
        .await
        .unwrap()
        .state
    }

    #[tokio::test]
    async fn provisioning_finishes_after_removed_live_resource_is_deleted() {
        let installed_stack = stack(&["retained", "removed"]);
        let mut state = provisioning(installed(&installed_stack).await, stack(&["retained"]));
        let mut saw_deleting = false;
        for _ in 0..8 {
            state = step(state).await;
            let removed = &state.stack_state.as_ref().unwrap().resources["removed"];
            if removed.status == ResourceStatus::Deleting {
                saw_deleting = true;
                assert_eq!(state.status, DeploymentStatus::Provisioning);
                assert!(state.current_release.is_none());
                assert!(state.target_release.is_some());
            }
            if state.status == DeploymentStatus::Running {
                break;
            }
        }
        assert!(
            saw_deleting,
            "the real controller must execute its delete flow"
        );
        assert_eq!(state.status, DeploymentStatus::Running);
        let resources = &state.stack_state.as_ref().unwrap().resources;
        assert_eq!(resources["retained"].status, ResourceStatus::Running);
        assert_eq!(resources["removed"].status, ResourceStatus::Deleted);
        assert_eq!(
            state.current_release.unwrap().release_id.as_deref(),
            Some("target-release")
        );
        assert!(state.target_release.is_none());
    }

    #[tokio::test]
    async fn provisioning_waits_for_a_removed_dependency_to_finish_deleting() {
        let mut previous = stack(&["consumer", "old-dependency"]);
        previous.resources.get_mut("consumer").unwrap().dependencies =
            vec![ResourceRef::new("storage".into(), "old-dependency")];
        let target = stack(&["consumer"]);
        let mut state = provisioning(installed(&previous).await, target);
        state = step(state).await;
        assert_eq!(state.status, DeploymentStatus::Provisioning);
        assert!(state.current_release.is_none());
        assert_eq!(
            state.stack_state.as_ref().unwrap().resources["old-dependency"].status,
            ResourceStatus::Running,
            "deletion must wait while the consumer still holds its dependency"
        );
        let mut saw_running_with_pending_deletion = false;
        for _ in 0..12 {
            state = step(state).await;
            let resources = &state.stack_state.as_ref().unwrap().resources;
            if resources["consumer"].status == ResourceStatus::Running
                && resources["old-dependency"].status == ResourceStatus::Running
            {
                saw_running_with_pending_deletion = true;
                assert_eq!(state.status, DeploymentStatus::Provisioning);
                assert!(state.current_release.is_none());
            }
            if state.status == DeploymentStatus::Running {
                break;
            }
        }
        assert!(saw_running_with_pending_deletion);
        assert_eq!(state.status, DeploymentStatus::Running);
        let resources = &state.stack_state.as_ref().unwrap().resources;
        assert_eq!(resources["old-dependency"].status, ResourceStatus::Deleted);
        assert!(resources["consumer"].dependencies.is_empty());
        assert!(state.current_release.is_some());
        assert!(state.target_release.is_none());
    }

    #[tokio::test]
    async fn provisioning_recreates_a_desired_deleted_resource_before_promoting_release() {
        let target = stack(&["recreated"]);
        let executor = StackExecutor::builder(&stack(&[]), ClientConfig::Test)
            .deployment_config(&config())
            .build()
            .unwrap();
        let mut deleted = installed(&target).await;
        for _ in 0..8 {
            deleted = executor.step(deleted).await.unwrap().next_state;
            if deleted.resources["recreated"].status == ResourceStatus::Deleted {
                break;
            }
        }
        assert_eq!(
            deleted.resources["recreated"].status,
            ResourceStatus::Deleted
        );
        let mut state = step(provisioning(deleted, target)).await;
        assert_eq!(state.status, DeploymentStatus::Provisioning);
        assert!(state.current_release.is_none());
        for _ in 0..8 {
            state = step(state).await;
            if state.status == DeploymentStatus::Running {
                break;
            }
        }
        assert_eq!(state.status, DeploymentStatus::Running);
        assert_eq!(
            state.stack_state.unwrap().resources["recreated"].status,
            ResourceStatus::Running
        );
        assert!(state.current_release.is_some());
        assert!(state.target_release.is_none());
    }

    #[tokio::test]
    async fn provisioning_preserves_removed_resource_delete_failure() {
        let installed_stack = stack(&["retained", "failed-removal"]);
        let mut checkpoint = installed(&installed_stack).await;
        let removed = checkpoint.resources.get_mut("failed-removal").unwrap();
        let mut controller = removed.get_internal_controller().unwrap().unwrap();
        controller.transition_to_delete_start().unwrap();
        controller.transition_to_failure();
        removed.status = controller.get_status();
        removed.set_internal_controller(Some(controller)).unwrap();
        assert_eq!(removed.status, ResourceStatus::DeleteFailed);

        let state = step(provisioning(checkpoint, stack(&["retained"]))).await;
        assert_eq!(state.status, DeploymentStatus::ProvisioningFailed);
        assert_eq!(
            state.stack_state.unwrap().resources["failed-removal"].status,
            ResourceStatus::DeleteFailed
        );
        assert!(state.current_release.is_none());
        assert!(state.target_release.is_some());
    }
}
