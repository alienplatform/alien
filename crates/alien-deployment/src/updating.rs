use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{
    ComputeClusterOutputs, InitialSetupAuthority, Platform, ReleaseInfo, ResourceLifecycle,
    ResourceStatus, SetupScaffolding, Stack, StackState, StackStatus,
};
use alien_error::{AlienError, Context};
use alien_infra::{RunningResourcePolicy, StackExecutor};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
};
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

pub(crate) fn compute_update_status(
    stack_state: &StackState,
    target_stack: &Stack,
    reconciled: &HashSet<&str>,
    pending_deletions: &[&str],
    pending_recovery_updates: &HashSet<&str>,
) -> Result<StackStatus> {
    // A superseded failure waiting to start is outstanding work, not a new handler failure.
    // Keep the stored record and its old dependency edges intact until planning can start it.
    let statuses = stack_state
        .resources
        .iter()
        .filter_map(|(resource_id, resource)| {
            (target_stack.resources.contains_key(resource_id)
                || resource.status != ResourceStatus::Deleted)
                .then_some(if pending_recovery_updates.contains(resource_id.as_str()) {
                    ResourceStatus::Updating
                } else {
                    resource.status
                })
        })
        .collect::<Vec<_>>();

    if statuses.is_empty() {
        return Ok(StackStatus::Running);
    }

    let status = StackState::compute_stack_status_from_resources(&statuses).context(
        ErrorData::StackExecutionFailed {
            message: "Failed to compute update status".to_string(),
        },
    )?;

    // Health is not completion, in both directions. The executor defers a resource's update
    // while its new dependency provisions, and defers a deletion while a consumer still
    // records the dependency — either way every status reads Running with work outstanding.
    if status == StackStatus::Running
        && (!stack_has_converged(stack_state, target_stack, reconciled)
            || !pending_deletions.is_empty())
    {
        return Ok(StackStatus::InProgress);
    }

    Ok(status)
}

/// Whether every resource the executor reconciles matches the config and dependencies the
/// release asks for. An edge the executor has not recorded yet is still outstanding work.
///
/// Scoped to `reconciled` because a resource the executor's lifecycle filter excluded is never
/// planned: a setup-owned resource whose recorded config differs from the declared one would
/// otherwise hold the update open forever. A resource missing from state has not converged.
fn stack_has_converged(
    stack_state: &StackState,
    target_stack: &Stack,
    reconciled: &HashSet<&str>,
) -> bool {
    target_stack
        .resources
        .iter()
        .filter(|(resource_id, _)| reconciled.contains(resource_id.as_str()))
        .all(|(resource_id, desired)| {
            stack_state
                .resources
                .get(resource_id)
                .is_some_and(|deployed| {
                    deployed.config == desired.config
                        && deployed.dependencies == desired.combined_dependencies()
                })
        })
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
    mut config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling UpdatePending status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Stack state is required
    let mut stack_state = current.stack_state.clone().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for update".to_string(),
        })
    })?;

    // Older deployments may not have saved a prepared stack. Keep release-based
    // compatibility checks and allocation pruning for those deployments.
    let old_stack_for_comparison = current
        .runtime_metadata
        .as_ref()
        .and_then(|m| m.prepared_stack.as_ref())
        .map(Cow::Borrowed)
        .or_else(|| {
            current.current_release.as_ref().map(|release| {
                let mut installed = release.stack.clone();
                // A raw release still lists gated resources that were never installed.
                // Preserve every ungated resource for the legacy compatibility checks.
                let declined: Vec<_> = installed
                    .resources
                    .iter()
                    .filter(|(id, entry)| {
                        entry.enabled_when.is_some()
                            && !stack_state
                                .resources
                                .get(*id)
                                .is_some_and(|resource| resource.status != ResourceStatus::Deleted)
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                // Match the two preparation phases: only setup-time declines remove node grants.
                let frozen_inputs =
                    crate::pending::frozen_gating_inputs(&installed, current.platform);
                let setup_declines: Vec<_> = declined
                    .iter()
                    .filter(|id| {
                        let entry = &installed.resources[*id];
                        entry.enabled_when.as_deref().is_some_and(|input| {
                            alien_core::frozen_gate_of(entry).is_some()
                                || frozen_inputs.contains(input)
                                || alien_core::gate_is_off_platform(
                                    &installed.inputs,
                                    input,
                                    current.platform,
                                )
                        })
                    })
                    .cloned()
                    .collect();
                alien_core::remove_declined_resources(&mut installed, &setup_declines);
                alien_core::remove_declined_live_resources(&mut installed, &declined);
                Cow::Owned(installed)
            })
        });

    // This grant is written by direct setup and retained by the store as setup-owned
    // metadata. A release or a mutable runtime setting cannot confer setup authority.
    if let Some(metadata) = current
        .runtime_metadata
        .as_ref()
        .filter(|metadata| metadata.automatic_setup)
    {
        let mut metadata = crate::pending::prepare_direct_setup_update(
            target_stack,
            &stack_state,
            &config,
            &client_config,
            metadata,
        )
        .await?;
        metadata.pending_prepared_release_id = current
            .target_release
            .as_ref()
            .and_then(|release| release.release_id.clone());
        crate::initial_setup::retry_failed_setup_resources(&mut stack_state, &metadata, &config)?;
        // Setup hands back to Provisioning, whose executor does not resume failures. Resume
        // unchanged failed runtime resources here, as the manual update executor does; changed
        // ones are left to the planner.
        let prepared_target =
            crate::helpers::injected_target_stack(&metadata, &config, stack_state.platform)?;
        crate::helpers::resume_unchanged_failed_resources(
            &mut stack_state,
            &prepared_target,
            |resource| resource.lifecycle == Some(ResourceLifecycle::Live),
        )?;
        next.status = DeploymentStatus::InitialSetup;
        next.stack_state = Some(stack_state);
        next.runtime_metadata = Some(metadata);
        next.error = None;
        next.retry_requested = false;
        return Ok(DeploymentStepResult {
            state: next,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    // A frozen gate is answered once: the answers recorded at creation are
    // what every later input value is held against. A gate with no recorded
    // answer was never asked, which leaves its resource in the stack for the
    // frozen-compatibility check to refuse.
    let persisted_gate_answers = current
        .runtime_metadata
        .as_ref()
        .map(|metadata| metadata.persisted_gate_answers.clone())
        .unwrap_or_default();
    let frozen_gating = crate::pending::frozen_gating_inputs(&target_stack, current.platform);
    crate::pending::enforce_frozen_gate_fixity(
        &persisted_gate_answers,
        &frozen_gating,
        &config.input_values,
    )?;

    // Drop gated setup resources the deployer declined, BEFORE the
    // preflights: the frozen-compatibility check compares against the
    // previous prepared stack, which was stripped the same way, and an
    // unstripped new stack would read as "frozen resource added" and refuse
    // the update — or worse, resurrect the resource the deployer declined.
    // Live declines apply AFTER the mutations, so a declined workload's service
    // account and capacity stay identical to the accepted render. Its grant is
    // scrubbed, which `permission_profiles_unchanged`'s gated exemption absorbs.
    let target_stack = crate::pending::strip_recorded_gate_declines(
        target_stack,
        &persisted_gate_answers,
        &frozen_gating,
        config.stack_settings.compute.as_mut(),
        current.platform,
    );

    let runner = alien_preflights::runner::PreflightRunner::new();

    let target_release_id = current
        .target_release
        .as_ref()
        .and_then(|release| release.release_id.as_deref());

    // Run deployment-time preflights: compatibility checks + mutations + runtime checks
    // Store the mutated stack to use for the actual update and for future compatibility checks
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
            old_stack_for_comparison.as_deref(), // Pass old mutated stack for compatibility checks
            setup_update_authorization,
            service_provider.runtime_setup_authority(current.platform),
        )
        .await
        .context(ErrorData::PreflightChecksFailed)?;
    debug!(
        setup_update_authorized,
        "evaluated setup update authorization"
    );

    info!("Deployment-time preflight checks completed successfully");

    // Drop gated live resources whose input says no — after the mutations,
    // at the boundary where the executor's desired set is built. For a live
    // gate this strip is what applies an input edit: the resource enters or
    // leaves the desired stack here, and the executor's create/delete
    // planning provisions or deprovisions it. Dependents share their
    // dependency's gate, so the strip stays closed.
    crate::pending::audit_live_gate_transitions(
        &mutated_stack,
        &stack_state,
        &config.input_values,
        &persisted_gate_answers,
        &frozen_gating,
        target_release_id,
        current.platform,
    );
    let mutated_stack = crate::pending::strip_declined_live_resources(
        mutated_stack,
        &config.input_values,
        &persisted_gate_answers,
        &frozen_gating,
        current.platform,
    )?;

    if current.runtime_metadata.as_ref().is_some_and(|metadata| {
        metadata.initial_setup_authority == InitialSetupAuthority::DirectSetup
    }) {
        // With nothing installed to compare against, every scaffolded resource and Frozen GCP
        // sandbox counts as new.
        let nothing_installed = Stack::new(mutated_stack.id.clone()).build();
        let nothing_recorded = BTreeMap::new();
        refuse_changes_requiring_setup(
            &client_config,
            old_stack_for_comparison
                .as_deref()
                .unwrap_or(&nothing_installed),
            current
                .runtime_metadata
                .as_ref()
                .map_or(&nothing_recorded, |metadata| &metadata.setup_scaffolding),
            &mutated_stack,
            current.platform,
        )?;
    }

    // Store the mutated stack in runtime_metadata for future compatibility checks
    let pending_prepared_release_id = target_release_id.map(str::to_string);
    let mut runtime_metadata = current.runtime_metadata.unwrap_or_default();
    runtime_metadata.pending_prepared_stack = Some(mutated_stack);
    runtime_metadata.pending_prepared_release_id = pending_prepared_release_id;
    runtime_metadata.persisted_gate_answers = persisted_gate_answers;

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

/// A direct setup's scaffolding and a Frozen GCP sandbox's template are made with the
/// deployer's credentials, which an update does not hold; applying the update anyway would serve
/// a sandbox without them, such as a newly denied one with open egress.
fn refuse_changes_requiring_setup(
    client_config: &alien_core::ClientConfig,
    installed_stack: &Stack,
    records: &BTreeMap<String, SetupScaffolding>,
    target_stack: &Stack,
    platform: Platform,
) -> Result<()> {
    let changes = alien_infra::setup_scaffolding::changes_requiring_setup(
        client_config,
        installed_stack,
        records,
        target_stack,
        platform,
    )
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to compare the release with the deployment's setup scaffolding"
            .to_string(),
    })?;
    if changes.is_empty() {
        return Ok(());
    }
    Err(AlienError::new(
        alien_preflights::error::ErrorData::SetupRequired {
            message: format!(
                "{}. Run the deployment's setup again with the updated stack, using the \
             credentials that set it up",
                changes.join("; ")
            ),
        },
    ))
    .context(ErrorData::PreflightChecksFailed)
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

    // Frozen resources omitted by a newer release remain setup-owned and must
    // not be deleted by an ordinary update. Keep their installed definitions
    // in the execution target while allowing explicitly runtime-managed frozen
    // resources (ComputeCluster capacity and, on AWS, a same-architecture machine
    // type) to reconcile changed configuration through their management
    // controller. A frozen service account that only lost permissions keeps its
    // installed definition too.
    // Only what is still installed is kept. Once setup has deleted an omitted
    // resource or rewritten a narrowed account (an imported setup re-imports the
    // result), keeping the old definition would make the update recreate the
    // resource or rewrite the account without setup authority.
    if let Some(installed_stack) = runtime_metadata.prepared_stack.as_ref() {
        for (resource_id, entry) in installed_stack.resources() {
            if entry.lifecycle != ResourceLifecycle::Frozen {
                continue;
            }
            let installed_state = stack_state
                .resources
                .get(resource_id)
                .filter(|state| state.status != ResourceStatus::Deleted);
            let keep_installed = match target_stack.resources.get(resource_id) {
                None => installed_state.is_some(),
                Some(target) => {
                    installed_state.is_some_and(|state| state.config == entry.config)
                        && alien_preflights::compatibility::narrowing::service_account_narrowed(
                            &target_stack,
                            &entry.config,
                            &target.config,
                        )
                }
            };
            if keep_installed {
                let mut retained = entry.clone();
                if let Some(target) = target_stack.resources.get(resource_id) {
                    retained.display_name = target.display_name.clone();
                    retained.description = target.description.clone();
                    // Retain setup-approved grants, but keep the freshly prepared
                    // ordering edges. Re-import may have installed those already.
                    retained.dependencies = target.dependencies.clone();
                }
                target_stack.resources.insert(resource_id.clone(), retained);
            }
        }
    }
    // The effective target is the new durable baseline. Persist it before
    // executor-only environment injection so a second release that also omits
    // a setup-owned resource cannot lose ownership information and delete it.
    runtime_metadata.pending_prepared_stack = Some(target_stack.clone());
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

    // Sync secrets to vault before updating workload resources.
    // The vault is Running and secrets may have been updated
    // This checks the hash and only syncs if needed
    info!("Syncing secrets to vault before updating managed resources");
    let synced = crate::helpers::sync_secrets_to_vault(
        &target_stack,
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

    let executor = StackExecutor::builder(&target_stack, client_config)
        .deployment_config(&config)
        .resume_unchanged_failed_resources(true)
        .running_resource_policy(RunningResourcePolicy::OptIn)
        .lifecycle_filter(vec![ResourceLifecycle::Live, ResourceLifecycle::Frozen])
        .service_provider(service_provider)
        .build()
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to create stack executor for update".to_string(),
        })?;

    // Captured before stepping: completion is judged over exactly what this executor
    // reconciles, so the two cannot disagree about which resources are in scope.
    let reconciled_ids = executor.tracked_resource_ids();

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
        current
            .target_release
            .as_ref()
            .map(|release| &release.stack),
    );

    // Compute the stack status from the resulting state
    let pending_deletions = executor
        .pending_deletions(&step_result.next_state)
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to determine outstanding deletions".to_string(),
        })?;
    let pending_recovery_updates = executor
        .pending_recovery_updates(&step_result.next_state)
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to determine outstanding recovery updates".to_string(),
        })?;
    let stack_status = compute_update_status(
        &step_result.next_state,
        &target_stack,
        &reconciled_ids,
        &pending_deletions,
        &pending_recovery_updates,
    )?;

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
        next.stack_state = Some(step_result.next_state);
        next.error = None;
        // The converged stack is the installed baseline either way.
        runtime_metadata.prepared_stack = runtime_metadata.pending_prepared_stack.take();
        let converged_release_id = runtime_metadata.pending_prepared_release_id.take();
        let target_release_id = next
            .target_release
            .as_ref()
            .and_then(|release| release.release_id.clone());

        // A state prepared before the release was recorded has no id and keeps
        // the old behavior.
        if converged_release_id.is_some() && converged_release_id != target_release_id {
            info!(
                converged_release_id = ?converged_release_id,
                target_release_id = ?target_release_id,
                "Update converged on a superseded release; preparing the newer target"
            );
            // The converged release is what is installed now, even if the newer
            // target later fails. Its prepared stack stands in for the release
            // stack, which this state no longer holds.
            next.current_release = Some(ReleaseInfo {
                release_id: converged_release_id,
                version: None,
                description: None,
                stack: runtime_metadata.prepared_stack.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::MissingConfiguration {
                        message: "Pending prepared stack not found in runtime metadata".to_string(),
                    })
                })?,
            });
            next.status = DeploymentStatus::UpdatePending;
            next.runtime_metadata = Some(runtime_metadata);
        } else {
            info!("Update completed successfully, transitioning to Running");

            next.status = DeploymentStatus::Running;
            runtime_metadata.setup_update_authorization = None;
            next.runtime_metadata = Some(runtime_metadata);
            // Promote target to current: update successful
            next.current_release = next.target_release.clone();
            next.target_release = None;
        }

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

        crate::helpers::interrupt_in_progress_resources(&mut next_state, &failed_refs, None)?;

        next.status = DeploymentStatus::UpdateFailed;
        next.error =
            crate::helpers::setup_required_error(&next_state).map(AlienError::into_generic);
        next.stack_state = Some(next_state);
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
            // An update held open for outstanding work can step nothing this pass and so
            // suggest no delay; without a floor that is a hot loop against cloud APIs.
            suggested_delay_ms: step_result.suggested_delay_ms.or(Some(5_000)),
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

    // A replace whose delete was denied waits for this explicit retry.
    let retried_replaces = alien_infra::allow_denied_replaces_to_retry(&mut stack_state);
    if !retried_replaces.is_empty() {
        info!(resources = ?retried_replaces, "Retrying replaces whose delete was denied");
    }

    // Do not restore failed controller checkpoints before preflights have built
    // the exact desired stack. A corrective release may change the resource
    // that failed; resuming its old polling checkpoint here would make that
    // checkpoint Provisioning again, causing the executor to defer the new
    // configuration. The executor resumes only unchanged failed resources once
    // mutations and runtime environment injection are complete.

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
    use alien_core::{
        Kv, Resource, ResourceLifecycle, ResourceRef, StackResourceState, Storage, Worker, WorkerCode,
    };

    #[tokio::test]
    async fn local_frozen_update_preserves_existing_storage() {
        use alien_core::{
            ClientConfig, EnvironmentVariablesSnapshot, ExternalBindings, RuntimeMetadata,
            StackSettings, Storage,
        };
        let directory = tempfile::tempdir().unwrap();
        let bindings = alien_local::LocalBindingsProvider::new(directory.path()).unwrap();
        let services = std::sync::Arc::new(
            alien_infra::DefaultPlatformServiceProvider::with_local_bindings(bindings.clone()),
        );
        let client = ClientConfig::Local {
            state_directory: directory.path().to_string_lossy().into_owned(),
        };
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();
        let installed = Stack::new("local-update".to_string())
            .add(
                Storage::new("existing".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let initial_state =
            StackState::with_resource_prefix(Platform::Local, "persistent".to_string());
        let prepared = alien_preflights::runner::PreflightRunner::new()
            .run_deployment_time_preflights(
                installed.clone(),
                &initial_state,
                &config,
                &client,
                None,
                None,
                Some(InitialSetupAuthority::DirectSetup),
            )
            .await
            .unwrap()
            .0;
        let executor = StackExecutor::builder(&prepared, client.clone())
            .deployment_config(&config)
            .service_provider(services.clone())
            .build()
            .unwrap();
        let created = executor.run_until_synced(initial_state).await;
        assert!(created.success, "{:?}", created.error);
        let marker = bindings
            .storage_manager()
            .get_storage_path("existing")
            .unwrap()
            .join("retained.txt");
        std::fs::write(&marker, b"retained across setup changes").unwrap();
        let target = Stack::new("local-update".to_string())
            .add(
                Storage::new("existing".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("added".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let release = |stack: Stack, id: &str| ReleaseInfo {
            release_id: Some(id.to_string()),
            version: None,
            description: None,
            stack,
        };
        let current = DeploymentState {
            status: DeploymentStatus::UpdatePending,
            platform: Platform::Local,
            current_release: Some(release(installed, "rel_installed")),
            target_release: Some(release(target.clone(), "rel_target")),
            stack_state: Some(created.final_state),
            error: None,
            environment_info: None,
            runtime_metadata: Some(RuntimeMetadata {
                initial_setup_authority: InitialSetupAuthority::DirectSetup,
                prepared_stack: Some(prepared),
                ..Default::default()
            }),
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let mut current = handle_update_pending(
            current,
            target,
            config.clone(),
            client.clone(),
            services.clone(),
        )
        .await
        .unwrap()
        .state;
        for _ in 0..20 {
            if current.status == DeploymentStatus::Running {
                break;
            }
            current = handle_updating(current, config.clone(), client.clone(), services.clone())
                .await
                .unwrap()
                .state;
        }
        assert_eq!(current.status, DeploymentStatus::Running);
        assert_eq!(
            std::fs::read(marker).unwrap(),
            b"retained across setup changes"
        );
        assert!(bindings
            .storage_manager()
            .get_storage_path("added")
            .unwrap()
            .is_dir());
        assert_eq!(current.stack_state.unwrap().resource_prefix, "persistent");
    }

    mod setup_scaffolding_drift {
        use super::super::*;
        use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt as _};
        use alien_core::{
            AwsServiceOverrides, ClientConfig, DeploymentState, EnvironmentVariablesSnapshot,
            ExternalBindings, ReleaseInfo, RuntimeMetadata, Sandbox, SandboxCode, SandboxEgress,
            SandboxLifecyclePolicy, StackSettings,
        };
        use alien_gcp_clients::{GcpClientConfig, GcpClientConfigExt as _};

        const BUNDLE: &str = "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip";

        fn sandbox(egress: SandboxEgress, bundle: &str, idle_pause_seconds: Option<u32>) -> Stack {
            Stack::new("acme".to_string())
                .add(
                    Sandbox::new("agents".to_string())
                        .code(SandboxCode::Image {
                            image: bundle.to_string(),
                        })
                        .egress(egress)
                        .lifecycle(SandboxLifecyclePolicy {
                            max_lifetime_seconds: None,
                            idle_pause_seconds,
                        })
                        .build(),
                    ResourceLifecycle::Live,
                )
                .build()
        }

        fn empty() -> Stack {
            Stack::new("acme".to_string()).build()
        }

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

        /// A Service Quotas endpoint that reports no quota, so the deployment-time quota check
        /// records it as unknown without calling AWS.
        async fn quota_server() -> httpmock::MockServer {
            let server = httpmock::MockServer::start_async().await;
            server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST);
                    then.status(200).body("{}");
                })
                .await;
            server
        }

        fn client_config(quotas: &httpmock::MockServer) -> ClientConfig {
            ClientConfig::Aws(Box::new(AwsClientConfig {
                service_overrides: Some(AwsServiceOverrides {
                    endpoints: [("servicequotas".into(), quotas.base_url())].into(),
                }),
                ..AwsClientConfig::mock()
            }))
        }

        fn stack_state() -> StackState {
            StackState::with_resource_prefix(Platform::Aws, "test".to_string())
        }

        /// The stack an earlier setup prepared, as Pending would have stored it.
        async fn prepared(stack: Stack) -> Stack {
            let quotas = quota_server().await;
            alien_preflights::runner::PreflightRunner::new()
                .run_deployment_time_preflights(
                    stack,
                    &stack_state(),
                    &config(),
                    &client_config(&quotas),
                    None,
                    None,
                    Some(InitialSetupAuthority::DirectSetup),
                )
                .await
                .expect("the installed stack passes preflights")
                .0
        }

        /// What setup recorded for `stack`'s sandboxes; the update check reads only that a
        /// record exists.
        fn recorded_for(stack: &Stack) -> BTreeMap<String, SetupScaffolding> {
            stack
                .resources()
                .filter(|(_, entry)| entry.config.downcast_ref::<Sandbox>().is_some())
                .map(|(id, _)| {
                    let record = SetupScaffolding::AwsSandbox {
                        build_role_name: format!("test-{id}-build"),
                        egress: None,
                        image_arn: None,
                    };
                    (id.clone(), record)
                })
                .collect()
        }

        fn release(stack: Stack, id: &str) -> ReleaseInfo {
            ReleaseInfo {
                release_id: Some(id.to_string()),
                version: None,
                description: None,
                stack,
            }
        }

        async fn update(
            authority: InitialSetupAuthority,
            installed: Stack,
            target: Stack,
        ) -> Result<DeploymentStepResult> {
            update_with_saved_stack(authority, installed, target, true).await
        }

        async fn update_with_saved_stack(
            authority: InitialSetupAuthority,
            installed: Stack,
            target: Stack,
            save_prepared: bool,
        ) -> Result<DeploymentStepResult> {
            let state = DeploymentState {
                status: DeploymentStatus::UpdatePending,
                platform: Platform::Aws,
                current_release: Some(release(installed.clone(), "rel_installed")),
                target_release: Some(release(target.clone(), "rel_target")),
                stack_state: Some(stack_state()),
                error: None,
                environment_info: None,
                runtime_metadata: Some(RuntimeMetadata {
                    initial_setup_authority: authority,
                    setup_scaffolding: recorded_for(&installed),
                    prepared_stack: if save_prepared {
                        Some(prepared(installed).await)
                    } else {
                        None
                    },
                    ..Default::default()
                }),
                retry_requested: false,
                protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            };
            let quotas = quota_server().await;
            handle_update_pending(
                state,
                target,
                config(),
                client_config(&quotas),
                std::sync::Arc::new(alien_infra::DefaultPlatformServiceProvider::default()),
            )
            .await
        }

        #[tokio::test]
        async fn legacy_direct_setup_update_keeps_existing_scaffolding() {
            let installed = prepared(sandbox(SandboxEgress::Allow, BUNDLE, None)).await;
            let updated = update_with_saved_stack(
                InitialSetupAuthority::DirectSetup,
                installed.clone(),
                installed,
                false,
            )
            .await
            .expect("existing scaffolding needs no new setup without a prepared stack");
            assert_eq!(updated.state.status, DeploymentStatus::Updating);
        }

        #[tokio::test]
        async fn legacy_update_still_refuses_frozen_resource_changes() {
            let installed = Stack::new("legacy".into())
                .add(
                    alien_core::Storage::new("data".into()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build();
            let target = Stack::new("legacy".into())
                .add(
                    alien_core::Storage::new("data".into())
                        .versioning(true)
                        .build(),
                    ResourceLifecycle::Frozen,
                )
                .build();
            let error = update_with_saved_stack(
                InitialSetupAuthority::ImportedHandoff,
                installed,
                target,
                false,
            )
            .await
            .expect_err("release fallback must still protect frozen storage");
            let cause = error.source.as_ref().expect("preflight cause");
            assert_eq!(cause.code, "DEPLOYMENT_SETUP_REQUIRED");
            assert!(
                cause
                    .message
                    .contains("Frozen resource 'data' was modified"),
                "{error:?}"
            );
        }

        /// `reason`, when given, is what the refusal must name.
        async fn assert_setup_required(installed: Stack, target: Stack, reason: Option<&str>) {
            let error = update(InitialSetupAuthority::DirectSetup, installed, target)
                .await
                .expect_err("the update needs setup to run first");
            let cause = error
                .source
                .as_deref()
                .expect("the preflight refusal is the cause");
            assert_eq!(cause.code, "DEPLOYMENT_SETUP_REQUIRED");
            assert!(!cause.retryable);
            if let Some(reason) = reason {
                assert!(cause.message.contains(reason), "{}", cause.message);
            }
        }

        async fn assert_updates(authority: InitialSetupAuthority, installed: Stack, target: Stack) {
            let updated = update(authority, installed, target)
                .await
                .expect("the update needs nothing from setup");
            assert_eq!(updated.state.status, DeploymentStatus::Updating);
        }

        #[tokio::test]
        async fn a_sandbox_switched_to_deny_waits_for_setup_to_build_its_connector() {
            assert_setup_required(
                sandbox(SandboxEgress::Allow, BUNDLE, None),
                sandbox(SandboxEgress::None, BUNDLE, None),
                None,
            )
            .await;
        }

        #[tokio::test]
        async fn a_sandbox_switched_to_allow_waits_for_setup_to_reseed_it() {
            assert_setup_required(
                sandbox(SandboxEgress::None, BUNDLE, None),
                sandbox(SandboxEgress::Allow, BUNDLE, None),
                None,
            )
            .await;
        }

        /// The runtime reads each release's bundle key from the bucket setup granted, so a release
        /// naming another bucket rolls without setup. A key outside the granted prefix still waits.
        #[tokio::test]
        async fn a_bundle_in_another_bucket_rolls_but_another_prefix_waits_for_setup() {
            assert_updates(
                InitialSetupAuthority::DirectSetup,
                sandbox(SandboxEgress::Allow, BUNDLE, None),
                sandbox(
                    SandboxEgress::Allow,
                    "s3://other-artifacts/sandbox-bundle/f00dcafe/bundle.zip",
                    None,
                ),
            )
            .await;
            assert_setup_required(
                sandbox(SandboxEgress::Allow, BUNDLE, None),
                sandbox(
                    SandboxEgress::Allow,
                    "s3://acme-artifacts/other-prefix/f00dcafe/bundle.zip",
                    None,
                ),
                Some("changes its build role policy"),
            )
            .await;
        }

        #[tokio::test]
        async fn a_sandbox_added_by_an_update_waits_for_setup_to_create_its_build_role() {
            assert_setup_required(empty(), sandbox(SandboxEgress::Allow, BUNDLE, None), None).await;
        }

        /// Every release publishes its bundle under a new version segment of the same prefix,
        /// which the build role already reads; refusing it would block every sandbox release.
        #[tokio::test]
        async fn a_new_bundle_version_or_session_policy_needs_no_setup() {
            assert_updates(
                InitialSetupAuthority::DirectSetup,
                sandbox(SandboxEgress::None, BUNDLE, None),
                sandbox(
                    SandboxEgress::None,
                    "s3://acme-artifacts/sandbox-bundle/0ddba11/bundle.zip",
                    Some(300),
                ),
            )
            .await;
        }

        /// With nothing installed to compare against, the sandbox is treated as new, not skipped.
        #[tokio::test]
        async fn an_update_with_no_installed_stack_waits_for_setup() {
            let target = sandbox(SandboxEgress::Allow, BUNDLE, None);
            let state = DeploymentState {
                status: DeploymentStatus::UpdatePending,
                platform: Platform::Aws,
                current_release: None,
                target_release: Some(release(target.clone(), "rel_target")),
                stack_state: Some(stack_state()),
                error: None,
                environment_info: None,
                runtime_metadata: Some(RuntimeMetadata {
                    initial_setup_authority: InitialSetupAuthority::DirectSetup,
                    prepared_stack: None,
                    ..Default::default()
                }),
                retry_requested: false,
                protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            };

            let quotas = quota_server().await;
            let error = handle_update_pending(
                state,
                target,
                config(),
                client_config(&quotas),
                std::sync::Arc::new(alien_infra::DefaultPlatformServiceProvider::default()),
            )
            .await
            .expect_err("the update needs setup to run first");

            let cause = error.source.as_deref().expect("the refusal is the cause");
            assert_eq!(cause.code, "DEPLOYMENT_SETUP_REQUIRED");
            assert!(cause.message.contains("is new"), "{}", cause.message);
        }

        /// A template setup renders the same build role and is held to the same setup inputs,
        /// which its runtime never re-reads: a new bundle version rolls, and so does a new bucket,
        /// read from the one the template granted.
        #[tokio::test]
        async fn a_template_setup_rolls_a_new_bundle_and_a_new_bucket() {
            for image in [
                "s3://acme-artifacts/sandbox-bundle/0ddba11/bundle.zip",
                "s3://other-artifacts/sandbox-bundle/f00dcafe/bundle.zip",
            ] {
                assert_updates(
                    InitialSetupAuthority::ImportedHandoff,
                    sandbox(SandboxEgress::Allow, BUNDLE, None),
                    sandbox(SandboxEgress::Allow, image, None),
                )
                .await;
            }
        }

        /// Only a Terraform setup grants the manager what replacing a Frozen GCP sandbox's
        /// template takes, so the same image-only release rolls after one and not after a
        /// direct setup.
        #[tokio::test]
        async fn a_gcp_frozen_image_waits_for_a_direct_setup_but_rolls_after_a_template_setup() {
            let frozen = |image: &str| {
                Stack::new("acme".to_string())
                    .add(
                        Sandbox::new("agents".to_string())
                            .code(SandboxCode::Image {
                                image: image.to_string(),
                            })
                            .egress(SandboxEgress::None)
                            .lifecycle(SandboxLifecyclePolicy {
                                max_lifetime_seconds: Some(3600),
                                idle_pause_seconds: None,
                            })
                            .build(),
                        ResourceLifecycle::Frozen,
                    )
                    .build()
            };
            let client_config = ClientConfig::Gcp(Box::new(GcpClientConfig::mock()));
            let stack_state = StackState::with_resource_prefix(Platform::Gcp, "test".to_string());
            let installed = alien_preflights::runner::PreflightRunner::new()
                .run_deployment_time_preflights(
                    frozen("us-docker.pkg.dev/acme/agents/sandbox:v1"),
                    &stack_state,
                    &config(),
                    &client_config,
                    None,
                    None,
                    Some(InitialSetupAuthority::DirectSetup),
                )
                .await
                .expect("the installed stack passes preflights")
                .0;
            let update_from = |authority, prepared_stack: Option<Stack>, target: Stack| {
                let state = DeploymentState {
                    status: DeploymentStatus::UpdatePending,
                    platform: Platform::Gcp,
                    current_release: None,
                    target_release: Some(release(target.clone(), "rel_target")),
                    stack_state: Some(stack_state.clone()),
                    error: None,
                    environment_info: None,
                    runtime_metadata: Some(RuntimeMetadata {
                        initial_setup_authority: authority,
                        prepared_stack,
                        ..Default::default()
                    }),
                    retry_requested: false,
                    protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                };
                handle_update_pending(
                    state,
                    target.clone(),
                    config(),
                    client_config.clone(),
                    std::sync::Arc::new(alien_infra::DefaultPlatformServiceProvider::default()),
                )
            };
            let update =
                |authority, target| update_from(authority, Some(installed.clone()), target);

            let unchanged = update(
                InitialSetupAuthority::DirectSetup,
                frozen("us-docker.pkg.dev/acme/agents/sandbox:v1"),
            )
            .await
            .expect("the same image needs nothing on the engine");
            assert_eq!(unchanged.state.status, DeploymentStatus::Updating);

            let target = || frozen("us-docker.pkg.dev/acme/agents/sandbox:v2");
            let rolled = update(InitialSetupAuthority::ImportedHandoff, target())
                .await
                .expect("a template setup granted the manager the template");
            assert_eq!(rolled.state.status, DeploymentStatus::Updating);

            let error = update(InitialSetupAuthority::DirectSetup, target())
                .await
                .expect_err("a direct setup granted the manager nothing on the engine");
            let cause = error.source.as_deref().expect("the refusal is the cause");
            assert_eq!(cause.code, "DEPLOYMENT_SETUP_REQUIRED");
            assert!(
                cause.message.contains("sandbox 'agents' changes its image"),
                "{}",
                cause.message
            );

            // With no installed stack to compare against, the sandbox counts as new.
            let error = update_from(InitialSetupAuthority::DirectSetup, None, target())
                .await
                .expect_err("a direct setup never granted the manager this template");
            let cause = error.source.as_deref().expect("the refusal is the cause");
            assert_eq!(cause.code, "DEPLOYMENT_SETUP_REQUIRED");
            assert!(
                cause
                    .message
                    .contains("sandbox 'agents' is new, and setup creates its template"),
                "{}",
                cause.message
            );
        }
    }

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

    /// An update is finished when the executor has nothing left to do, not when every
    /// resource is healthy. A consumer deferred while its dependency provisioned is still
    /// healthy on its old config, so status alone reports success too early.
    #[test]
    fn an_update_is_not_complete_while_a_resource_has_not_reached_its_desired_config() {
        let cache = Kv::new("cache".to_string()).build();
        let deployed_agent = Worker::new("agent".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/agent:latest".to_string(),
            })
            .build();
        // What the release asks for: the same worker, now linked to the cache.
        let desired_agent = Worker::new("agent".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/agent:latest".to_string(),
            })
            .link(&cache)
            .build();

        let target_stack = Stack::new("s".to_string())
            .add(desired_agent, ResourceLifecycle::Live)
            .add(cache.clone(), ResourceLifecycle::Live)
            .build();

        // The cache finished provisioning, so every resource is healthy, but the worker is
        // still running the config it had before the link was added.
        let mut stack_state = StackState::new(Platform::Aws);
        stack_state.resources.insert(
            "agent".to_string(),
            state_entry(Resource::new(deployed_agent), ResourceStatus::Running),
        );
        stack_state.resources.insert(
            "cache".to_string(),
            state_entry(Resource::new(cache), ResourceStatus::Running),
        );

        let reconciled = HashSet::from(["agent", "cache"]);
        let status = compute_update_status(
            &stack_state,
            &target_stack,
            &reconciled,
            &[],
            &HashSet::new(),
        )
        .expect("status should compute");

        assert_eq!(
            status,
            StackStatus::InProgress,
            "the worker has not reached its desired config, so the update is not complete"
        );
    }

    /// A release can add only an ordering edge. Until the executor records it, the deployed
    /// resource does not match the release, even with every config equal and every status Running.
    #[test]
    fn an_update_is_not_complete_while_a_new_edge_is_unrecorded() {
        let cache = Kv::new("cache".to_string()).build();
        let objects = Storage::new("objects".to_string()).build();
        let mut target_stack = Stack::new("s".to_string())
            .add(cache.clone(), ResourceLifecycle::Frozen)
            .add(objects.clone(), ResourceLifecycle::Frozen)
            .build();
        target_stack
            .resources
            .get_mut("objects")
            .expect("objects is in the stack")
            .dependencies = vec![ResourceRef::new(Kv::RESOURCE_TYPE, "cache")];

        let mut stack_state = StackState::new(Platform::Aws);
        stack_state.resources.insert(
            "cache".to_string(),
            state_entry(Resource::new(cache), ResourceStatus::Running),
        );
        stack_state.resources.insert(
            "objects".to_string(),
            state_entry(Resource::new(objects), ResourceStatus::Running),
        );
        let reconciled = HashSet::from(["cache", "objects"]);
        let status = |state: &StackState| {
            compute_update_status(state, &target_stack, &reconciled, &[], &HashSet::new())
                .expect("status should compute")
        };

        assert_eq!(status(&stack_state), StackStatus::InProgress);
        stack_state
            .resources
            .get_mut("objects")
            .expect("objects is deployed")
            .dependencies = target_stack.resources["objects"].combined_dependencies();
        assert_eq!(status(&stack_state), StackStatus::Running);
    }

    /// Declining a gate that was accepted must take the resource away, and the executor defers
    /// that deletion for a step while the consumer still records a dependency on it. Judging
    /// completion by config alone misses the deletion entirely, so the resource keeps running.
    #[test]
    fn an_update_is_not_complete_while_a_declined_resource_is_still_deployed() {
        let agent = Worker::new("agent".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/agent:latest".to_string(),
            })
            .build();
        // The gate was declined, so the release's target no longer declares the worker.
        let target_stack = Stack::new("s".to_string())
            .add(agent.clone(), ResourceLifecycle::Live)
            .build();

        let mut stack_state = StackState::new(Platform::Aws);
        stack_state.resources.insert(
            "agent".to_string(),
            state_entry(Resource::new(agent), ResourceStatus::Running),
        );
        stack_state.resources.insert(
            "declined".to_string(),
            state_entry(
                Resource::new(
                    Worker::new("declined".to_string())
                        .permissions("execution".to_string())
                        .code(WorkerCode::Image {
                            image: "example.com/declined:latest".to_string(),
                        })
                        .build(),
                ),
                ResourceStatus::Running,
            ),
        );

        // The declined worker is not in the target, so the executor does not track it; the
        // executor reports it as a deletion it still owes.
        let reconciled = HashSet::from(["agent"]);
        let status = compute_update_status(
            &stack_state,
            &target_stack,
            &reconciled,
            &["declined"],
            &HashSet::new(),
        )
        .expect("status should compute");

        assert_eq!(
            status,
            StackStatus::InProgress,
            "a declined resource still deployed means the update has work left"
        );
    }

    /// A resource the executor will never delete — outside its deletion scope, or held by a
    /// dependent that scope excludes — is absent from `pending_deletions`, so waiting on it
    /// cannot hold the update open.
    #[test]
    fn a_deletion_the_executor_will_not_perform_cannot_hold_the_update_open() {
        let agent = Worker::new("agent".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/agent:latest".to_string(),
            })
            .build();
        let target_stack = Stack::new("s".to_string())
            .add(agent.clone(), ResourceLifecycle::Live)
            .build();

        let mut stack_state = StackState::new(Platform::Aws);
        stack_state.resources.insert(
            "agent".to_string(),
            state_entry(Resource::new(agent), ResourceStatus::Running),
        );
        stack_state.resources.insert(
            "out-of-scope".to_string(),
            state_entry(
                Resource::new(Kv::new("out-of-scope".to_string()).build()),
                ResourceStatus::Running,
            ),
        );

        let reconciled = HashSet::from(["agent"]);
        let status = compute_update_status(
            &stack_state,
            &target_stack,
            &reconciled,
            &[],
            &HashSet::new(),
        )
        .expect("status should compute");

        assert_eq!(
            status,
            StackStatus::Running,
            "a deletion the executor does not own must not block completion"
        );
    }

    /// A resource not reported as reconciled by the executor cannot hold an
    /// update open merely because its recorded configuration differs.
    #[test]
    fn a_resource_the_executor_does_not_reconcile_cannot_hold_the_update_open() {
        let declared = Kv::new("store".to_string()).build();
        let target_stack = Stack::new("s".to_string())
            .add(declared, ResourceLifecycle::Frozen)
            .build();

        let deployed = Kv::new("other-name".to_string()).build();
        let mut stack_state = StackState::new(Platform::Aws);
        let mut entry = state_entry(Resource::new(deployed), ResourceStatus::Running);
        entry.lifecycle = Some(ResourceLifecycle::Frozen);
        stack_state.resources.insert("store".to_string(), entry);

        // The executor reconciles only Live resources, so the frozen store is not in scope.
        let reconciled = HashSet::new();
        let status = compute_update_status(
            &stack_state,
            &target_stack,
            &reconciled,
            &[],
            &HashSet::new(),
        )
        .expect("status should compute");

        assert_eq!(
            status,
            StackStatus::Running,
            "a resource outside the executor's scope must not block completion"
        );
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

#[cfg(test)]
mod aws_handoff_tests {
    use super::*;
    use alien_aws_clients::AwsClientConfigExt;
    use alien_core::{
        AwsClientConfig, ClientConfig, EnvironmentVariablesSnapshot, PermissionProfile, Resource,
        ResourceRef, ServiceAccount, StackResourceState,
    };
    use alien_infra::{
        AwsRemoteStackManagementController, AwsServiceAccountController,
        MockPlatformServiceProvider, StackResourceStateExt,
    };
    use alien_preflights::{mutations::InfrastructureDependenciesMutation, StackMutation};
    use std::sync::Arc;

    #[tokio::test]
    async fn repeated_setup_keeps_new_ordering_while_retaining_installed_grants() {
        for narrow_grants in [false, true] {
            let account = ServiceAccount::from_permission_profile(
                "execution-sa".into(),
                &PermissionProfile::new().resource("objects", ["storage/data-read"]),
                |id| alien_permissions::get_permission_set(id).cloned(),
            )
            .unwrap();
            let baseline = Stack::new("handoff".into())
                .add(
                    alien_core::RemoteStackManagement::new("management".into()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add(account, ResourceLifecycle::Frozen)
                .build();
            let dependency = ResourceRef::new(
                alien_core::RemoteStackManagement::RESOURCE_TYPE,
                "management",
            );
            let config = DeploymentConfig::builder()
                .stack_settings(Default::default())
                .environment_variables(EnvironmentVariablesSnapshot {
                    variables: Vec::new(),
                    hash: String::new(),
                    created_at: String::new(),
                })
                .external_bindings(Default::default())
                .allow_frozen_changes(false)
                .build();
            let empty_state = StackState::with_resource_prefix(Platform::Aws, "test".into());
            assert!(InfrastructureDependenciesMutation.should_run(
                &baseline,
                &empty_state,
                &config
            ));
            let mut target = InfrastructureDependenciesMutation
                .mutate(baseline.clone(), &empty_state, &config)
                .await
                .unwrap();
            let entry = target.resources.get_mut("execution-sa").unwrap();
            assert!(baseline.resources["execution-sa"].dependencies.is_empty());
            assert_eq!(entry.dependencies, vec![dependency.clone()]);
            if narrow_grants {
                entry.config = Resource::new(ServiceAccount::new("execution-sa".into()).build());
            }
            // Setup re-imported the same role and grants with the new ordering
            // edge; the last successful release still has the older entry.
            let mut saved = StackState::with_resource_prefix(Platform::Aws, "test".into());
            for (id, entry) in baseline.resources() {
                let mut resource = StackResourceState::new_pending(
                    entry.config.resource_type().to_string(),
                    entry.config.clone(),
                    Some(ResourceLifecycle::Frozen),
                    Vec::new(),
                );
                resource.status = ResourceStatus::Running;
                if id == "execution-sa" {
                    resource.dependencies = target.resources[id].dependencies.clone();
                    let controller: AwsServiceAccountController = serde_json::from_value(serde_json::json!({
                        "state": "ready", "roleArn": "arn:aws:iam::123456789012:role/test-execution-sa",
                        "roleName": "test-execution-sa", "stackPermissionsApplied": true
                    })).unwrap();
                    resource
                        .set_internal_controller(Some(Box::new(controller)))
                        .unwrap();
                } else {
                    resource
                        .set_internal_controller(Some(Box::new(
                            AwsRemoteStackManagementController::mock_ready("management"),
                        )))
                        .unwrap();
                }
                saved.resources.insert(id.clone(), resource);
            }
            let before = saved.resources["execution-sa"].clone();
            let result = handle_updating(
                DeploymentState {
                    status: DeploymentStatus::Updating,
                    platform: Platform::Aws,
                    current_release: None,
                    target_release: None,
                    stack_state: Some(saved),
                    error: None,
                    environment_info: None,
                    runtime_metadata: Some(alien_core::RuntimeMetadata {
                        initial_setup_authority: InitialSetupAuthority::ImportedHandoff,
                        prepared_stack: Some(baseline),
                        pending_prepared_stack: Some(target),
                        ..Default::default()
                    }),
                    retry_requested: false,
                    protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                },
                config,
                ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
                Arc::new(MockPlatformServiceProvider::new()),
            )
            .await
            .unwrap();
            assert_eq!(result.state.status, DeploymentStatus::Running);
            let after = &result.state.stack_state.as_ref().unwrap().resources["execution-sa"];
            assert_eq!(after.status, ResourceStatus::Running);
            assert_eq!(after.config, before.config);
            assert_eq!(after.dependencies, before.dependencies);
            assert_eq!(after.internal_state, before.internal_state);
            assert!(after.previous_config.is_none());
            assert!(after.error.is_none());
            let prepared = result
                .state
                .runtime_metadata
                .unwrap()
                .prepared_stack
                .unwrap();
            assert_eq!(
                prepared.resources["execution-sa"].dependencies,
                vec![dependency]
            );
            assert_eq!(prepared.resources["execution-sa"].config, before.config);
        }
    }
}
