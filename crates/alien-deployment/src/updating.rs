use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{
    ComputeClusterOutputs, Platform, ResourceLifecycle, ResourceStatus, Stack, StackState,
    StackStatus,
};
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
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

fn compute_update_status(stack_state: &StackState, target_stack: &Stack) -> Result<StackStatus> {
    let statuses = stack_state
        .resources
        .iter()
        .filter_map(|(resource_id, resource)| {
            (target_stack.resources.contains_key(resource_id)
                || resource.status != ResourceStatus::Deleted)
                .then_some(resource.status)
        })
        .collect::<Vec<_>>();

    if statuses.is_empty() {
        return Ok(StackStatus::Running);
    }

    StackState::compute_stack_status_from_resources(&statuses).context(
        ErrorData::StackExecutionFailed {
            message: "Failed to compute update status".to_string(),
        },
    )
}

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

    // Drop gated resources the deployer declined, BEFORE the preflights: the
    // frozen-compatibility check compares against the previous prepared
    // stack, which was stripped the same way, and an unstripped new stack
    // would read as "frozen resource added" and refuse the update — or
    // worse, resurrect the resource the deployer declined. For a live gate
    // this strip is also what applies an input edit: the resource enters or
    // leaves the desired stack here, and the executor's create/delete
    // planning provisions or deprovisions it. Dependents share their
    // dependency's gate, so the strip stays closed.
    let target_stack = crate::pending::strip_declined_resources(
        target_stack,
        &stack_state,
        &config.input_values,
    )?;

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
    let target_release_id = current
        .target_release
        .as_ref()
        .and_then(|release| release.release_id.as_deref());
    let setup_update_authorization = current
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.setup_update_authorization.as_ref())
        .filter(|authorization| Some(authorization.release_id.as_str()) == target_release_id);
    let (mutated_stack, _deployment_summary, setup_update_authorized) = runner
        .run_deployment_time_preflights(
            target_stack.clone(),
            &stack_state,
            &config,
            &client_config,
            old_stack_for_comparison, // Pass old mutated stack for compatibility checks
            setup_update_authorization,
        )
        .await
        .context(ErrorData::PreflightChecksFailed)?;
    debug!(
        setup_update_authorized,
        "evaluated setup update authorization"
    );

    info!("Deployment-time preflight checks completed successfully");

    // Store the mutated stack in runtime_metadata for future compatibility checks
    let mut runtime_metadata = current.runtime_metadata.unwrap_or_default();
    runtime_metadata.pending_prepared_stack = Some(mutated_stack);

    // Transition to Updating
    next.status = DeploymentStatus::Updating;
    next.stack_state = Some(stack_state);
    next.error = None;
    next.runtime_metadata = Some(runtime_metadata);

    Ok(DeploymentStepResult {
        state: next,
        suggested_delay_ms: None,
        update_heartbeat: false,
        heartbeats: vec![],
        observed_inventory_batches: vec![],
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
    let mut target_stack = runtime_metadata
        .pending_prepared_stack
        .clone()
        .ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Pending prepared stack not found in runtime metadata".to_string(),
            })
        })?;
    let setup_update_authorized = runtime_metadata
        .setup_update_authorization
        .as_ref()
        .is_some_and(|authorization| {
            authorization.target_frozen_digest == target_stack.frozen_resources_digest()
                && current
                    .target_release
                    .as_ref()
                    .and_then(|release| release.release_id.as_deref())
                    == Some(authorization.release_id.as_str())
        });

    // Inject environment variables into the prepared stack
    crate::helpers::inject_environment_variables(&mut target_stack, &config, current.platform)?;

    // Inject OTLP monitoring env vars if monitoring is configured
    if let Some(monitoring) = &config.monitoring {
        crate::helpers::inject_monitoring_environment_variables(
            &mut target_stack,
            monitoring,
            current.platform,
        )?;
    }

    // Sync secrets to vault before updating workload resources.
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
    let mut lifecycle_filter_vec = vec![ResourceLifecycle::Live];
    if config.allow_frozen_changes || setup_update_authorized {
        info!("Including frozen resources in authorized update");
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
    let mut step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute update step".to_string(),
            })?;

    prune_deprovisioned_resources(
        &mut step_result.next_state,
        &target_stack,
        current.target_release.as_ref().map(|release| &release.stack),
    );

    // Compute the stack status from the resulting state
    let stack_status = compute_update_status(&step_result.next_state, &target_stack)?;

    // Check if update is complete
    let waiting_for_machines =
        machines_deployment_has_zero_machines(current.platform, &step_result.next_state);

    let result = if waiting_for_machines {
        info!("Machines update is waiting for the first machine to join");

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
        info!("Update completed successfully, transitioning to Running");

        next.status = DeploymentStatus::Running;
        next.stack_state = Some(step_result.next_state);
        next.error = None;
        runtime_metadata.prepared_stack = runtime_metadata.pending_prepared_stack.take();
        runtime_metadata.setup_update_authorization = None;
        next.runtime_metadata = Some(runtime_metadata);
        // Promote target to current: update successful
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

        next.status = DeploymentStatus::UpdateFailed;
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
        next.status = DeploymentStatus::Updating;
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

/// Handle UpdateFailed status - accept a retry request and re-run preflights
///
/// This step:
/// 1. Checks if retry_requested flag is set
/// 2. Transitions back to UpdatePending to re-run preflights
/// 3. Clears the retry marker
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
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    info!("Re-running preflights before retrying the update");

    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for retry".to_string(),
        })
    })?;

    // Restore each failed controller to the handler that failed and reset its
    // retry/backoff budget before re-running preflights. Stack preparation will
    // still replace this state when the desired resource config changed; when
    // it did not, execution resumes without losing durable provider IDs.
    use alien_infra::state_utils::StackStateExt;
    let retried = stack_state
        .retry_failed()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to retry resources during update".to_string(),
        })?;

    info!(
        resource_ids = ?retried,
        "Reset failed resource state before retrying update"
    );

    // Transition back to UpdatePending to re-run preflights
    next.status = DeploymentStatus::UpdatePending;
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

/// Drop the terminally deleted entries of gate-declined resources.
///
/// A declined resource is still declared by the release, so a later
/// re-enable must start from a clean slate rather than a `Deleted`
/// tombstone the executor would have to reconcile. Resources REMOVED from
/// the release keep their tombstone — it is the executor's deletion record,
/// and the update-status computation already ignores it. Destroy flows keep
/// their `Deleted` entries too — they never pass through the update handler.
fn prune_deprovisioned_resources(
    state: &mut StackState,
    target_stack: &Stack,
    release_stack: Option<&Stack>,
) {
    state.resources.retain(|resource_id, resource_state| {
        resource_state.status != ResourceStatus::Deleted
            || target_stack.resources.contains_key(resource_id)
            || !release_stack
                .and_then(|stack| stack.resources.get(resource_id))
                .is_some_and(|entry| entry.enabled_when.is_some())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Kv, Resource, ResourceLifecycle, StackResourceState, Worker, WorkerCode};

    fn state_entry(resource: Resource, status: ResourceStatus) -> StackResourceState {
        let mut entry = StackResourceState::new_pending(
            resource.resource_type().as_ref().to_string(),
            resource,
            Some(ResourceLifecycle::Live),
            Vec::new(),
        );
        entry.status = status;
        entry
    }

    /// Build a release stack where `cache` is declared behind a gate.
    fn release_stack_with_gated_cache(agent: &alien_core::Worker) -> Stack {
        let mut stack = Stack::new("s".to_string())
            .add(agent.clone(), ResourceLifecycle::Live)
            .add(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        stack
            .resources
            .get_mut("cache")
            .expect("cache entry exists")
            .enabled_when = Some("cacheEnabled".to_string());
        stack
    }

    /// The declined gated store leaves the state so the stack computes back
    /// to Running; the survivors stay untouched.
    #[test]
    fn a_deprovisioned_gated_resource_leaves_the_state() {
        let agent = Worker::new("agent".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/agent:latest".to_string(),
            })
            .build();
        let target_stack = Stack::new("s".to_string())
            .add(agent.clone(), ResourceLifecycle::Live)
            .build();
        let release_stack = release_stack_with_gated_cache(&agent);

        let mut state = StackState::new(Platform::Aws);
        state.resources.insert(
            "agent".to_string(),
            state_entry(Resource::new(agent), ResourceStatus::Running),
        );
        state.resources.insert(
            "cache".to_string(),
            state_entry(
                Resource::new(Kv::new("cache".to_string()).build()),
                ResourceStatus::Deleted,
            ),
        );

        prune_deprovisioned_resources(&mut state, &target_stack, Some(&release_stack));

        assert!(!state.resources.contains_key("cache"));
        assert!(state.resources.contains_key("agent"));
        assert_eq!(
            state.compute_stack_status().expect("status computes"),
            StackStatus::Running
        );
    }

    /// A Deleted entry the desired stack still wants is the executor's to
    /// recreate, not ours to forget.
    #[test]
    fn a_deleted_but_still_desired_resource_stays() {
        let cache = Kv::new("cache".to_string()).build();
        let target_stack = Stack::new("s".to_string())
            .add(cache.clone(), ResourceLifecycle::Live)
            .build();
        let release_stack = target_stack.clone();

        let mut state = StackState::new(Platform::Aws);
        state.resources.insert(
            "cache".to_string(),
            state_entry(Resource::new(cache), ResourceStatus::Deleted),
        );

        prune_deprovisioned_resources(&mut state, &target_stack, Some(&release_stack));

        assert!(state.resources.contains_key("cache"));
    }

    /// A resource removed from the release entirely keeps its Deleted entry:
    /// the tombstone is the executor's deletion record, and only gate
    /// declines are ours to clean up.
    #[test]
    fn a_removed_ungated_resource_keeps_its_tombstone() {
        let agent = Worker::new("agent".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/agent:latest".to_string(),
            })
            .build();
        let target_stack = Stack::new("s".to_string())
            .add(agent.clone(), ResourceLifecycle::Live)
            .build();
        let release_stack = target_stack.clone();

        let mut state = StackState::new(Platform::Aws);
        state.resources.insert(
            "agent".to_string(),
            state_entry(Resource::new(agent), ResourceStatus::Running),
        );
        state.resources.insert(
            "cache".to_string(),
            state_entry(
                Resource::new(Kv::new("cache".to_string()).build()),
                ResourceStatus::Deleted,
            ),
        );

        prune_deprovisioned_resources(&mut state, &target_stack, Some(&release_stack));

        assert!(state.resources.contains_key("cache"));
    }
}
