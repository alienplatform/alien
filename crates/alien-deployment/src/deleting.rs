use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{
    ownership_policy_for_resource_type, DeletionPolicy, InitialSetupAuthority, Platform,
    ResourceLifecycle, ResourceStatus, SetupManagement, StackState, StackStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::{PlatformServiceProvider, StackExecutor};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tracing::info;

/// Handle DeletePending → Deleting transition.
///
/// This step:
/// 1. Prepares runtime-cleanup resources for destroy.
/// 2. Transitions to Deleting status.
pub async fn handle_delete_pending(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling DeletePending status");

    let mut next = current.clone();
    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for deletion".to_string(),
        })
    })?;

    if stack_state.resources.contains_key("secrets")
        && !next
            .runtime_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.vault_credentials_revoked)
    {
        let runtime_metadata = next.runtime_metadata.get_or_insert_default();
        let prepared_stack = runtime_metadata.prepared_stack.clone().ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Prepared stack required for vault credential revocation".to_string(),
            })
        })?;
        let revoked = crate::helpers::revoke_deployment_vault_credentials(
            &prepared_stack,
            &stack_state,
            &client_config,
            &config,
            runtime_metadata,
        )
        .await
        .context(ErrorData::SecretSyncFailed {
            vault_name: "secrets".to_string(),
            reason: "Failed to revoke deployment credentials before runtime cleanup".to_string(),
        })?;
        runtime_metadata.vault_credentials_revoked = revoked;
        // Deployer secrets hold values the deployer wrote; Alien never owned
        // them, so they stay. Their reports carry the command that deletes them.
        for report in &runtime_metadata.deployer_secrets {
            info!(
                secret = %report.location.name,
                delete_command = report.location.delete_command.as_deref().unwrap_or_default(),
                "Keeping deployer secret"
            );
        }
    }

    if ensure_data_protection(
        &mut next,
        &stack_state,
        &config,
        &client_config,
        service_provider.as_ref(),
    )
    .await?
        == ProtectionProgress::Pending
    {
        // A retry enters here with DeleteFailed; resume the prerequisite wait
        // instead of returning a failed state with retry_requested cleared.
        next.status = DeploymentStatus::DeletePending;
        next.error = None;
        return Ok(DeploymentStepResult {
            state: next,
            suggested_delay_ms: Some(30_000),
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    let executor = deletion_executor(
        &next,
        &stack_state,
        &config,
        client_config,
        service_provider,
    )?;
    let prepared = executor.prepare_for_deletion(&mut stack_state).context(
        ErrorData::StackExecutionFailed {
            message: "Failed to prepare runtime resources for destroy".to_string(),
        },
    )?;

    info!(
        "Prepared {} runtime resources for destroy: {:?}",
        prepared.len(),
        prepared
    );

    next.status = DeploymentStatus::Deleting;
    next.stack_state = Some(stack_state);
    next.error = None;

    Ok(DeploymentStepResult {
        state: next,
        suggested_delay_ms: None,
        update_heartbeat: false,
        heartbeats: vec![],
        observed_inventory_batches: vec![],
    })
}

/// Handle Deleting status.
///
/// This step deletes runtime-cleanup resources. When it finishes, the deployment
/// either finishes deletion or stops at TeardownRequired for setup-owned resources.
pub async fn handle_deleting(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling Deleting status");

    let mut current_cloned = current.clone();
    let stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for deletion".to_string(),
        })
    })?;

    // Old Deleting checkpoints have no receipt. Persist protection before an
    // executor step can remove the grants required to confirm it again.
    let protection = ensure_data_protection(
        &mut current_cloned,
        &stack_state,
        &config,
        &client_config,
        service_provider.as_ref(),
    )
    .await?;
    if protection != ProtectionProgress::Unchanged {
        return Ok(DeploymentStepResult {
            state: current_cloned,
            suggested_delay_ms: (protection == ProtectionProgress::Pending).then_some(30_000),
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    let owns_setup = owns_runtime_setup(current_cloned.platform, service_provider.as_ref());
    let management = config.stack_settings.setup_management;
    let executor = deletion_executor(
        &current_cloned,
        &stack_state,
        &config,
        client_config,
        service_provider,
    )?;

    let step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute runtime cleanup step".to_string(),
            })?;

    // Completion concerns only deleted resources; retained checkpoints remain
    // byte-for-byte recoverable and must never be relabeled as deleted.
    let mut cleanup_state = step_result.next_state.clone();
    cleanup_state
        .resources
        .retain(|id, _| !executor.preserves_resource(id));
    let has_retained = cleanup_state.resources.len() != step_result.next_state.resources.len();
    let stack_status = if owns_setup && !cleanup_state.resources.is_empty() {
        cleanup_state
            .compute_stack_status()
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to compute local teardown status".to_string(),
            })
    } else {
        compute_runtime_cleanup_status(&cleanup_state, management)
    }
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to compute runtime cleanup status".to_string(),
    })?;

    let result = if stack_status == StackStatus::Deleted {
        let next_status = if has_retained {
            // Privileged revocation is a separate handoff. It must not expose
            // the legacy teardown state to clients that run raw destroy.
            DeploymentStatus::RetentionRequired
        } else if has_remaining_setup_resources(&step_result.next_state, management)
            || crate::setup_teardown::has_setup_scaffolding(
                current_cloned.runtime_metadata.as_ref(),
                current_cloned.platform,
            )
        {
            DeploymentStatus::TeardownRequired
        } else {
            DeploymentStatus::Deleted
        };

        info!(
            next_status = ?next_status,
            "Runtime cleanup completed"
        );

        let mut next = current_cloned;
        next.status = next_status;
        next.stack_state = Some(step_result.next_state);
        next.error = None;

        DeploymentStepResult {
            state: next,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        }
    } else if stack_status == StackStatus::Failure {
        info!("Runtime cleanup failed");

        let mut next = current_cloned;
        next.status = DeploymentStatus::DeleteFailed;
        next.stack_state = Some(step_result.next_state);
        next.error = None;

        DeploymentStepResult {
            state: next,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        }
    } else {
        let mut next = current_cloned;
        next.stack_state = Some(step_result.next_state);

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

#[derive(PartialEq)]
enum ProtectionProgress {
    Unchanged,
    Confirmed,
    Pending,
}

/// Confirmed receipts must be persisted before resource cleanup; Pending yields without cleanup.
async fn ensure_data_protection(
    current: &mut DeploymentState,
    state: &StackState,
    config: &DeploymentConfig,
    client_config: &alien_core::ClientConfig,
    provider: &dyn PlatformServiceProvider,
) -> Result<ProtectionProgress> {
    let retained = retained_resource_ids(current, state);
    let needs_kv = |id: &str| {
        alien_infra::retained_kv_requires_runtime_protection(state, id, &config.external_bindings)
    };
    let data: BTreeMap<_, _> = retained
        .iter()
        .filter_map(|id| {
            state
                .resources
                .get(id)
                .filter(|resource| {
                    needs_kv(id)
                        || std::iter::once(&resource.config)
                            .chain(resource.previous_config.iter())
                            .any(|config| config.downcast_ref::<alien_core::Storage>().is_some())
                })
                .map(|resource| (id, resource))
        })
        .collect();
    if data.is_empty() {
        return Ok(ProtectionProgress::Unchanged);
    }
    // Version the proof when protection semantics change. Bind it to exact
    // installed identities, ownership, current/previous declarations and adopted
    // bindings, never a loose resource-name list. Only the digest is persisted.
    let bindings: BTreeMap<_, _> = config
        .external_bindings
        .0
        .iter()
        .filter(|(id, _)| data.contains_key(id))
        .collect();
    // Preserve existing storage-only receipts after their grants have gone.
    // Only deployments with retained Live AWS KV need the additional proof.
    let has_kv = data.keys().any(|id| needs_kv(id));
    let has_azure_storage = data.values().any(|resource| {
        resource.controller_platform.unwrap_or(state.platform) == alien_core::Platform::Azure
            && resource.lifecycle == Some(ResourceLifecycle::Live)
    });
    let has_gcp_storage = data.values().any(|resource| {
        resource.controller_platform.unwrap_or(state.platform) == alien_core::Platform::Gcp
            && resource.lifecycle == Some(ResourceLifecycle::Live)
    });
    let proof_version = if has_azure_storage || has_gcp_storage {
        "storage-cloud-expiry-protection-v4"
    } else if has_kv {
        "storage-and-kv-protection-v2"
    } else {
        "storage-protection-v1"
    };
    let mut encoded = serde_json::to_value(&(
        proof_version,
        state.platform,
        &state.resource_prefix,
        data,
        bindings,
    ))
    .into_alien_error()
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to identify retained data protection inventory".into(),
    })?;
    // JSONB stores and HashMaps can reorder keys across a restart.
    encoded.sort_all_objects();
    let digest = format!("{:x}", Sha256::digest(encoded.to_string().as_bytes()));
    if current.runtime_metadata.as_ref().is_some_and(|metadata| {
        metadata.retained_storage_protection_hash.as_ref() == Some(&digest)
            && metadata.retained_storage_protection_confirmed_at.is_some()
    }) {
        return Ok(ProtectionProgress::Unchanged);
    }
    if current
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.retained_data_protection_started_at)
        .is_some_and(|started| {
            chrono::Utc::now().signed_duration_since(started) >= chrono::Duration::hours(2)
        })
    {
        return Err(AlienError::new(ErrorData::StackExecutionFailed {
            message: "Retained data expiry controls did not finish disabling within two hours. Inspect DynamoDB TTL status and retry deletion; data and access checkpoints are preserved.".into(),
        }));
    }
    if !alien_infra::protect_retained_kv(
        state,
        &retained,
        client_config,
        provider,
        &config.external_bindings,
        &[ResourceLifecycle::Live],
    )
    .await
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to protect retained KV before runtime cleanup".into(),
    })? {
        current
            .runtime_metadata
            .get_or_insert_default()
            .retained_data_protection_started_at
            .get_or_insert_with(chrono::Utc::now);
        return Ok(ProtectionProgress::Pending);
    }
    alien_infra::protect_retained_storage(
        state,
        &retained,
        client_config,
        provider,
        &config.external_bindings,
        &[ResourceLifecycle::Live],
    )
    .await
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to protect retained storage before runtime cleanup".into(),
    })?;
    let metadata = current.runtime_metadata.get_or_insert_default();
    metadata.retained_data_protection_started_at = None;
    metadata.retained_storage_protection_hash = Some(digest);
    metadata.retained_storage_protection_confirmed_at = Some(chrono::Utc::now());
    Ok(ProtectionProgress::Confirmed)
}

/// Stops expiry and public reads on every retained store before the setup authority removes the
/// remaining access, including setup-owned (Frozen) stores that runtime cleanup leaves alone.
/// Every call re-reads provider state. Returns false while DynamoDB TTL is still transitioning;
/// the caller waits and calls again before deleting anything.
pub(crate) async fn protect_retained_data_for_handoff(
    current: &DeploymentState,
    state: &StackState,
    config: &DeploymentConfig,
    client_config: &alien_core::ClientConfig,
    provider: &dyn PlatformServiceProvider,
) -> Result<bool> {
    let retained = retained_resource_ids(current, state);
    let lifecycles = [ResourceLifecycle::Live, ResourceLifecycle::Frozen];
    if !alien_infra::protect_retained_kv(
        state,
        &retained,
        client_config,
        provider,
        &config.external_bindings,
        &lifecycles,
    )
    .await
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to protect retained KV before removing access".into(),
    })? {
        return Ok(false);
    }
    alien_infra::protect_retained_storage(
        state,
        &retained,
        client_config,
        provider,
        &config.external_bindings,
        &lifecycles,
    )
    .await
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to protect retained storage before removing access".into(),
    })?;
    Ok(true)
}

/// Where a destroy goes when the runtime never started, or `None` when runtime cleanup has work.
/// It has none while the management identity has no outputs and no Live resource, synced secrets
/// vault or Frozen runtime-cleanup type came up. An empty first deployment with no setup
/// metadata can be deleted directly. A pre-federation GCP deployment skips runtime cleanup
/// when it has nothing that needs it. Fails closed on a missing inventory or lifecycle.
pub fn destroy_without_runtime(current: &DeploymentState) -> Option<DeploymentStatus> {
    let destroying = match current.status {
        DeploymentStatus::DeletePending | DeploymentStatus::Deleting => true,
        DeploymentStatus::DeleteFailed => current.retry_requested,
        _ => false,
    };
    if !destroying
        || !matches!(
            current.platform,
            alien_core::Platform::Aws | alien_core::Platform::Gcp | alien_core::Platform::Azure
        )
    {
        return None;
    }
    let stack_state = current.stack_state.as_ref()?;
    // Preflights can reject the first deployment before setup records either resources
    // or metadata. The default authority is ImportedHandoff, but there is no installed
    // identity to resolve in this case. Keep prepared imports and previous installs on
    // the normal cleanup path, even if their inventory happens to be empty.
    if current.current_release.is_none()
        && stack_state.resources.is_empty()
        && current
            .runtime_metadata
            .as_ref()
            .is_none_or(|metadata| metadata == &alien_core::RuntimeMetadata::default())
    {
        return Some(DeploymentStatus::Deleted);
    }
    // A GCP deployment installed before federation (protocol 1) keeps no target trust the
    // manager can use, so its runtime cleanup can never run. When nothing it owns needs
    // runtime cleanup, its setup-owned resources go straight to setup teardown, which uses
    // the deployer's own credentials.
    let pre_federation_gcp = current.platform == Platform::Gcp
        && stack_state.resources.values().any(|resource| {
            resource
                .outputs
                .as_ref()
                .and_then(|outputs| {
                    outputs.downcast_ref::<alien_core::RemoteStackManagementOutputs>()
                })
                .is_some_and(|outputs| {
                    // Legacy setup stored the impersonated service-account email.
                    // Federated setup stores a structured access document instead.
                    outputs.access_configuration == outputs.management_resource_id
                        && outputs
                            .access_configuration
                            .ends_with(".iam.gserviceaccount.com")
                })
        });
    let metadata = current.runtime_metadata.as_ref();
    if !pre_federation_gcp
        && metadata?.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup
    {
        return None;
    }
    for (resource_id, resource) in &stack_state.resources {
        let lifecycle = resource.lifecycle?;
        let resource_type = resource.config.resource_type();
        if resource_type == alien_core::RemoteStackManagement::RESOURCE_TYPE {
            if resource.outputs.is_some() && !pre_federation_gcp {
                return None;
            }
            continue;
        }
        if matches!(
            resource.status,
            ResourceStatus::Pending | ResourceStatus::Deleted
        ) {
            continue;
        }
        // A sandbox's delete ends in Deleted, never TeardownRequired, so setup teardown runs all of it.
        let runtime_cleans_up = match lifecycle {
            ResourceLifecycle::Live => true,
            ResourceLifecycle::Frozen => {
                resource_id == "secrets"
                    || (resource_type != alien_core::Sandbox::RESOURCE_TYPE
                        && ownership_policy_for_resource_type(resource_type.as_ref())
                            .has_runtime_cleanup_before_teardown())
            }
        };
        if runtime_cleans_up {
            return None;
        }
    }
    if !retained_resource_ids(current, stack_state).is_empty() {
        return Some(DeploymentStatus::RetentionRequired);
    }
    Some(
        // A direct setup's own teardown removes every resource that is not Live.
        if has_remaining_setup_resources(stack_state, SetupManagement::Manual)
            || crate::setup_teardown::has_setup_scaffolding(metadata, current.platform)
        {
            DeploymentStatus::TeardownRequired
        } else {
            DeploymentStatus::Deleted
        },
    )
}

/// Handle TeardownRequired status. This is a synced tombstone state: Live
/// resources are gone, but setup-owned resources still need a privileged
/// teardown request.
pub async fn handle_teardown_required(current: DeploymentState) -> Result<DeploymentStepResult> {
    info!("Handling TeardownRequired status");
    Ok(DeploymentStepResult {
        state: current,
        suggested_delay_ms: None,
        update_heartbeat: false,
        heartbeats: vec![],
        observed_inventory_batches: vec![],
    })
}

/// Handle DeleteFailed status: retry runtime cleanup when requested.
pub async fn handle_delete_failed(
    current: DeploymentState,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling DeleteFailed status");

    if !current.retry_requested {
        return Ok(DeploymentStepResult {
            state: current,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    // DeleteFailed can be checkpointed before credential revocation completes.
    // Retry the same prerequisites before preparing any resource for cleanup.
    let mut current = current;
    current
        .runtime_metadata
        .get_or_insert_default()
        .retained_data_protection_started_at = None;
    let mut result =
        handle_delete_pending(current, config, client_config, service_provider).await?;
    result.state.retry_requested = false;
    Ok(result)
}

/// Durable roots are selected from installed state, including interrupted updates.
/// Their structural dependencies are preserved by the executor. This does not
/// certify that access has been revoked; that requires the retention handoff.
pub(crate) fn retained_resource_ids(current: &DeploymentState, state: &StackState) -> Vec<String> {
    if current
        .runtime_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.deletion_policy == DeletionPolicy::Erase)
    {
        return Vec::new();
    }
    installed_durable_resource_ids(state)
}

/// Installed durable roots whose recovery metadata must survive deletion.
/// Empty declarations without evidence of installation do not own data.
///
/// An AWS artifact registry is not one: its controller owns only the pull and push roles, and
/// deleting it removes that access while the repositories and their images stay in the account.
/// GCP and Azure registries delete their repository or registry, so they are retained.
pub fn installed_durable_resource_ids(state: &StackState) -> Vec<String> {
    state
        .resources
        .iter()
        .filter_map(|(id, resource)| {
            let platform = resource.controller_platform.unwrap_or(state.platform);
            let durable = |resource: &alien_core::Resource| match resource.resource_type().as_ref()
            {
                "storage"
                | "kv"
                | "queue"
                | "vault"
                | "postgres"
                | "key"
                | "experimental/aws-opensearch" => true,
                "artifact-registry" => platform != Platform::Aws,
                _ => false,
            };
            // A failed storage create can save a controller before it owns a bucket.
            // Requested names are cleaned up by the controller, not retained as data.
            let storage_installed = resource.internal_state.as_ref().is_none_or(|controller| {
                let identity = match controller.get("type").and_then(serde_json::Value::as_str) {
                    Some(
                        "AwsStorageController" | "GcpStorageController" | "TestStorageController",
                    ) => "bucketName",
                    Some("AzureStorageController") => "containerName",
                    Some("LocalStorageController") => "storagePath",
                    _ => return true,
                };
                controller
                    .get(identity)
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| !id.is_empty())
            });
            (resource.status != ResourceStatus::Deleted
                && storage_installed
                && !(resource.status == ResourceStatus::Pending
                    && resource.internal_state.is_none()
                    && resource.outputs.is_none()
                    && resource.previous_config.is_none())
                && (durable(&resource.config)
                    || resource.previous_config.as_ref().is_some_and(durable)))
            .then(|| id.clone())
        })
        .collect()
}

fn deletion_executor(
    current: &DeploymentState,
    state: &StackState,
    config: &DeploymentConfig,
    client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn PlatformServiceProvider>,
) -> Result<StackExecutor> {
    let executor = if owns_runtime_setup(current.platform, service_provider.as_ref()) {
        StackExecutor::for_deletion_with_service_provider(
            client_config,
            config,
            service_provider,
            None,
        )
    } else {
        StackExecutor::for_runtime_cleanup_deletion_with_service_provider(
            client_config,
            config,
            service_provider,
        )
    }
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to create runtime cleanup executor".to_string(),
    })?;
    executor
        .preserving_resources(state, retained_resource_ids(current, state))
        .context(ErrorData::StackExecutionFailed {
            message: "Failed to preserve installed data and dependencies".to_string(),
        })
}

fn owns_runtime_setup(platform: Platform, service_provider: &dyn PlatformServiceProvider) -> bool {
    service_provider.runtime_setup_authority(platform) == Some(InitialSetupAuthority::DirectSetup)
}

fn compute_runtime_cleanup_status(
    stack_state: &StackState,
    management: SetupManagement,
) -> Result<StackStatus> {
    let mut statuses = Vec::new();

    for (resource_id, resource) in &stack_state.resources {
        if !is_runtime_cleanup_resource(resource, management, stack_state.platform)?
            || held_for_setup_teardown(stack_state, resource_id, resource, management)?
        {
            continue;
        }

        statuses.push({
            if resource_lifecycle(resource)? == ResourceLifecycle::Live {
                resource.status
            } else if resource.status == ResourceStatus::TeardownRequired {
                ResourceStatus::Deleted
            } else {
                resource.status
            }
        });
    }

    if statuses.is_empty() {
        return Ok(StackStatus::Deleted);
    }

    StackState::compute_stack_status_from_resources(&statuses).context(
        ErrorData::StackExecutionFailed {
            message: "Failed to compute runtime cleanup status".to_string(),
        },
    )
}

/// Whether runtime cleanup of an automatic package keeps this resource because a package
/// resource still depends on it. The executor never deletes it while that dependent exists,
/// whether it is Running or a failed resource destroy preparation moved to Deleting, so
/// waiting for it would hold the deployment in Deleting. A failed delete still surfaces.
fn held_for_setup_teardown(
    stack_state: &StackState,
    resource_id: &str,
    resource: &alien_core::StackResourceState,
    management: SetupManagement,
) -> Result<bool> {
    if management != SetupManagement::Automatic
        || matches!(
            resource.status,
            ResourceStatus::Deleted | ResourceStatus::DeleteFailed
        )
    {
        return Ok(false);
    }
    for dependent in stack_state.resources.values() {
        if matches!(
            dependent.status,
            ResourceStatus::Deleting | ResourceStatus::Deleted | ResourceStatus::TeardownRequired
        ) || !dependent
            .dependencies
            .iter()
            .any(|dependency| dependency.id() == resource_id)
        {
            continue;
        }
        if !is_runtime_cleanup_resource(dependent, management, stack_state.platform)?
            && !StackExecutor::permission_only_dependency(dependent, resource_id, stack_state)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn resource_lifecycle(resource: &alien_core::StackResourceState) -> Result<ResourceLifecycle> {
    resource.lifecycle.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: format!(
                "Resource '{}' is missing lifecycle metadata required for deletion",
                resource.config.id()
            ),
        })
    })
}

fn is_runtime_cleanup_resource(
    resource: &alien_core::StackResourceState,
    management: SetupManagement,
    platform: Platform,
) -> Result<bool> {
    Ok(alien_core::removed_by_runtime_cleanup(
        resource.config.resource_type().as_ref(),
        resource_lifecycle(resource)?,
        management,
        platform,
    ))
}

/// Whether setup teardown still has work: a resource runtime cleanup leaves to it, or one it
/// only half removed (TeardownRequired).
fn has_remaining_setup_resources(stack_state: &StackState, management: SetupManagement) -> bool {
    stack_state.resources.values().any(|resource| {
        if resource.status == ResourceStatus::Deleted {
            return false;
        }
        if management != SetupManagement::Automatic {
            return resource.lifecycle != Some(ResourceLifecycle::Live);
        }
        resource.status == ResourceStatus::TeardownRequired
            || !is_runtime_cleanup_resource(resource, management, stack_state.platform)
                .unwrap_or(true)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_aws_clients::{iam::MockIamApi, AwsClientConfig, AwsClientConfigExt};
    use alien_core::{
        ClientConfig, ComputeCluster, Daemon, DaemonCode, DeploymentConfig, DeploymentState,
        DeploymentStatus, EnvironmentVariablesSnapshot, ExternalBindings, InitialSetupAuthority,
        Platform, Resource, ResourceLifecycle, ResourceRef, ResourceStatus, RuntimeMetadata,
        ServiceAccount, SetupManagement, Stack, StackResourceState, StackSettings, StackState,
        StackStatus, Storage, Vault,
    };
    use alien_infra::{
        state_utils::{StackResourceStateExt, StackStateExt},
        AwsServiceAccountController, DefaultPlatformServiceProvider, MockPlatformServiceProvider,
        StackExecutor,
    };

    use super::{
        compute_runtime_cleanup_status, handle_delete_failed, handle_delete_pending,
        handle_deleting, has_remaining_setup_resources,
    };

    struct LocalStorageFixture {
        directory: tempfile::TempDir,
        state: StackState,
        config: DeploymentConfig,
        client: ClientConfig,
        services: Arc<DefaultPlatformServiceProvider>,
        prepared: Stack,
    }

    async fn local_storage_fixture() -> LocalStorageFixture {
        let directory = tempfile::tempdir().expect("temporary state directory");
        let bindings =
            alien_local::LocalBindingsProvider::new(directory.path()).expect("local bindings");
        let services = Arc::new(DefaultPlatformServiceProvider::with_local_bindings(
            bindings,
        ));
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
        let stack = Stack::new("local-storage".to_string())
            .add(
                Storage::new("data".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("evidence".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let initial = StackState::new(Platform::Local);
        let prepared = alien_preflights::runner::PreflightRunner::new()
            .run_deployment_time_preflights(
                stack,
                &initial,
                &config,
                &client,
                None,
                None,
                Some(InitialSetupAuthority::DirectSetup),
            )
            .await
            .expect("prepare local storage")
            .0;
        let executor = StackExecutor::builder(&prepared, client.clone())
            .deployment_config(&config)
            .service_provider(services.clone())
            .build()
            .expect("storage executor");
        let created = executor.run_until_synced(initial).await;
        assert!(created.success, "{:?}", created.error);
        for name in ["data", "evidence"] {
            let bucket = directory.path().join("storage").join(name);
            assert!(bucket.is_dir());
            std::fs::write(bucket.join("retained.txt"), b"stored data").expect("store marker");
        }
        LocalStorageFixture {
            directory,
            state: created.final_state,
            config,
            client,
            services,
            prepared,
        }
    }

    #[tokio::test]
    async fn executor_preserves_real_storage_across_restart_then_explicitly_erases_it() {
        let fixture = local_storage_fixture().await;
        let mut state = fixture.state;
        let original = serde_json::to_value(&state.resources["data"]).unwrap();
        for _ in 0..16 {
            state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
            let executor = StackExecutor::for_deletion_with_service_provider(
                fixture.client.clone(),
                &fixture.config,
                fixture.services.clone(),
                None,
            )
            .unwrap()
            .preserving_resources(&state, ["data".to_string()])
            .unwrap();
            let prepared = executor.prepare_for_deletion(&mut state).unwrap();
            assert!(!prepared.contains(&"data".to_string()));
            state = executor.step(state).await.unwrap().next_state;
            assert_eq!(
                serde_json::to_value(&state.resources["data"]).unwrap(),
                original
            );
            assert_eq!(
                std::fs::read(fixture.directory.path().join("storage/data/retained.txt")).unwrap(),
                b"stored data"
            );
            if state.resources["evidence"].status == ResourceStatus::Deleted {
                break;
            }
        }
        assert_eq!(state.resources["evidence"].status, ResourceStatus::Deleted);
        assert!(!fixture.directory.path().join("storage/evidence").exists());
        let executor = StackExecutor::for_deletion_with_service_provider(
            fixture.client,
            &fixture.config,
            fixture.services,
            None,
        )
        .unwrap();
        let erased = executor.run_until_synced(state).await;
        assert!(erased.success, "{:?}", erased.error);
        assert_eq!(
            erased.final_state.resources["data"].status,
            ResourceStatus::Deleted
        );
        assert!(!fixture.directory.path().join("storage/data").exists());
    }

    /// The real deployment handlers must preserve existing imported data even
    /// without a successful release, through reloads and a later explicit erase.
    #[tokio::test]
    async fn legacy_delete_retains_installed_data_then_explicit_erase_uses_same_ids() {
        let fixture = local_storage_fixture().await;
        let original = serde_json::to_value(&fixture.state).unwrap();
        let state = DeploymentState::builder()
            .status(DeploymentStatus::DeletePending)
            .platform(Platform::Local)
            .stack_state(fixture.state)
            .runtime_metadata(RuntimeMetadata {
                initial_setup_authority: InitialSetupAuthority::DirectSetup,
                prepared_stack: Some(fixture.prepared),
                ..Default::default()
            })
            .retry_requested(false)
            .protocol_version(alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION)
            .build();
        let mut old = serde_json::to_value(state).unwrap();
        old["runtimeMetadata"]
            .as_object_mut()
            .unwrap()
            .remove("deletionPolicy");
        let mut state: DeploymentState = serde_json::from_value(old).unwrap();
        assert!(state.current_release.is_none());
        state = handle_delete_pending(
            state,
            fixture.config.clone(),
            fixture.client.clone(),
            fixture.services.clone(),
        )
        .await
        .unwrap()
        .state;
        for _ in 0..16 {
            state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
            if state.status == DeploymentStatus::RetentionRequired {
                break;
            }
            state = handle_deleting(
                state,
                fixture.config.clone(),
                fixture.client.clone(),
                fixture.services.clone(),
            )
            .await
            .unwrap()
            .state;
        }
        assert_eq!(state.status, DeploymentStatus::RetentionRequired);
        assert_eq!(
            serde_json::to_value(state.stack_state.as_ref().unwrap()).unwrap(),
            original
        );
        for id in ["data", "evidence"] {
            assert_eq!(
                std::fs::read(
                    fixture
                        .directory
                        .path()
                        .join(format!("storage/{id}/retained.txt"))
                )
                .unwrap(),
                b"stored data"
            );
        }
        // The handoff never says Deleted: access revocation has not been proven.
        let handoff = crate::step(
            state.clone(),
            fixture.config.clone(),
            fixture.client.clone(),
            Some(fixture.services.clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(handoff.state).unwrap(),
            serde_json::to_value(&state).unwrap()
        );
        state.runtime_metadata.as_mut().unwrap().deletion_policy =
            alien_core::DeletionPolicy::Erase;
        state.status = DeploymentStatus::DeletePending;
        state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        state = handle_delete_pending(
            state,
            fixture.config.clone(),
            fixture.client.clone(),
            fixture.services.clone(),
        )
        .await
        .unwrap()
        .state;
        for _ in 0..16 {
            if state.status == DeploymentStatus::Deleted {
                break;
            }
            state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
            state = handle_deleting(
                state,
                fixture.config.clone(),
                fixture.client.clone(),
                fixture.services.clone(),
            )
            .await
            .unwrap()
            .state;
        }
        assert_eq!(state.status, DeploymentStatus::Deleted);
        for id in ["data", "evidence"] {
            assert!(!fixture
                .directory
                .path()
                .join(format!("storage/{id}"))
                .exists());
            assert_eq!(
                state.stack_state.as_ref().unwrap().resources[id].status,
                ResourceStatus::Deleted
            );
        }
    }

    #[tokio::test]
    async fn local_manager_explicit_erase_removes_frozen_storage_with_its_own_services() {
        let mut fixture = local_storage_fixture().await;
        let mut failed = StackResourceState::new_pending(
            "storage".to_string(),
            Resource::new(Storage::new("never-created".to_string()).build()),
            None,
            Vec::new(),
        );
        failed.lifecycle = Some(ResourceLifecycle::Frozen);
        failed.status = ResourceStatus::ProvisionFailed;
        fixture
            .state
            .resources
            .insert("never-created".to_string(), failed);
        let neighbor = fixture.directory.path().join("neighbor.txt");
        std::fs::write(&neighbor, b"outside storage").expect("neighbor marker");
        let current = DeploymentState::builder()
            .status(DeploymentStatus::DeletePending)
            .platform(Platform::Local)
            .stack_state(fixture.state)
            .runtime_metadata(RuntimeMetadata {
                deletion_policy: alien_core::DeletionPolicy::Erase,
                initial_setup_authority: InitialSetupAuthority::DirectSetup,
                prepared_stack: Some(fixture.prepared),
                ..Default::default()
            })
            .retry_requested(false)
            .protocol_version(alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION)
            .build();
        let mut state = handle_delete_pending(
            current,
            fixture.config.clone(),
            fixture.client.clone(),
            fixture.services.clone(),
        )
        .await
        .expect("prepare deletion")
        .state;
        for _ in 0..16 {
            if state.status == DeploymentStatus::Deleted {
                break;
            }
            state = handle_deleting(
                state,
                fixture.config.clone(),
                fixture.client.clone(),
                fixture.services.clone(),
            )
            .await
            .expect("local deletion step")
            .state;
            assert_ne!(state.status, DeploymentStatus::DeleteFailed);
            assert_ne!(state.status, DeploymentStatus::TeardownRequired);
        }
        assert_eq!(state.status, DeploymentStatus::Deleted);
        assert!(state
            .stack_state
            .as_ref()
            .unwrap()
            .resources
            .values()
            .all(|resource| resource.status == ResourceStatus::Deleted));
        for name in ["data", "evidence"] {
            assert!(!fixture.directory.path().join("storage").join(name).exists());
        }
        assert_eq!(std::fs::read(neighbor).unwrap(), b"outside storage");
    }

    #[tokio::test]
    async fn lost_controller_state_does_not_forget_evidence_of_existing_storage() {
        for evidence in ["outputs", "failed-state", "previous-config"] {
            let mut fixture = local_storage_fixture().await;
            let resource = fixture.state.resources.get_mut("data").unwrap();
            assert!(resource.outputs.is_some());
            resource.internal_state = None;
            resource.status = ResourceStatus::ProvisionFailed;
            match evidence {
                "outputs" => {}
                "failed-state" => {
                    resource.outputs = None;
                    resource.last_failed_state = Some(serde_json::json!({"corrupt": true}));
                }
                "previous-config" => {
                    resource.outputs = None;
                    resource.previous_config = Some(resource.config.clone());
                }
                _ => unreachable!(),
            }
            let before = resource.clone();
            fixture
                .state
                .prepare_for_destroy()
                .expect_err("lost controller with evidence of prior creation must fail closed");
            let retained = &fixture.state.resources["data"];
            assert_eq!(retained.status, ResourceStatus::ProvisionFailed);
            assert_eq!(retained.outputs, before.outputs);
            assert_eq!(retained.last_failed_state, before.last_failed_state);
            assert_eq!(retained.previous_config, before.previous_config);
            for name in ["data", "evidence"] {
                assert_eq!(
                    std::fs::read(
                        fixture
                            .directory
                            .path()
                            .join("storage")
                            .join(name)
                            .join("retained.txt")
                    )
                    .unwrap(),
                    b"stored data"
                );
            }
        }
    }

    #[tokio::test]
    async fn missing_local_storage_services_fail_without_forgetting_stored_data() {
        let mut fixture = local_storage_fixture().await;
        fixture
            .state
            .prepare_for_destroy()
            .expect("prepare storage teardown");
        let executor = StackExecutor::for_deletion_with_service_provider(
            fixture.client.clone(),
            &fixture.config,
            Arc::new(DefaultPlatformServiceProvider::default()),
            None,
        )
        .expect("teardown executor");
        let step = executor
            .step(fixture.state)
            .await
            .expect("controller failure is checkpointed");
        for name in ["data", "evidence"] {
            let resource = &step.next_state.resources[name];
            assert_eq!(resource.status, ResourceStatus::DeleteFailed);
            assert!(resource.error.is_some());
            assert_eq!(
                std::fs::read(
                    fixture
                        .directory
                        .path()
                        .join("storage")
                        .join(name)
                        .join("retained.txt")
                )
                .unwrap(),
                b"stored data"
            );
        }
        let mut retry_state = step.next_state;
        let mut never_created = StackResourceState::new_pending(
            "storage".to_string(),
            Resource::new(Storage::new("never-created".to_string()).build()),
            None,
            Vec::new(),
        );
        never_created.lifecycle = Some(ResourceLifecycle::Frozen);
        never_created.status = ResourceStatus::ProvisionFailed;
        retry_state
            .resources
            .insert("never-created".to_string(), never_created);
        let failed = DeploymentState::builder()
            .status(DeploymentStatus::DeleteFailed)
            .platform(Platform::Local)
            .stack_state(retry_state)
            .runtime_metadata(RuntimeMetadata {
                deletion_policy: alien_core::DeletionPolicy::Erase,
                ..Default::default()
            })
            .retry_requested(true)
            .protocol_version(alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION)
            .build();
        let mut retry = handle_delete_failed(
            failed,
            fixture.config.clone(),
            fixture.client.clone(),
            fixture.services.clone(),
        )
        .await
        .expect("retry with owning services")
        .state;
        assert!(!retry.retry_requested);
        for _ in 0..16 {
            if retry.status == DeploymentStatus::Deleted {
                break;
            }
            retry = handle_deleting(
                retry,
                fixture.config.clone(),
                fixture.client.clone(),
                fixture.services.clone(),
            )
            .await
            .expect("retry deletion step")
            .state;
            assert_ne!(retry.status, DeploymentStatus::DeleteFailed);
            assert_ne!(retry.status, DeploymentStatus::TeardownRequired);
        }
        assert_eq!(retry.status, DeploymentStatus::Deleted);
        for name in ["data", "evidence"] {
            assert!(!fixture.directory.path().join("storage").join(name).exists());
        }
    }

    fn resource_state(
        resource: Resource,
        lifecycle: ResourceLifecycle,
        status: ResourceStatus,
    ) -> StackResourceState {
        StackResourceState {
            progress: None,
            display_name: None,
            description: None,
            resource_type: resource.resource_type().as_ref().to_string(),
            internal_state: None,
            status,
            outputs: None,
            config: resource,
            previous_config: None,
            retry_attempt: 0,
            error: None,
            lifecycle: Some(lifecycle),
            controller_platform: None,
            dependencies: Vec::new(),
            last_failed_state: None,
            remote_binding_params: None,
        }
    }

    #[test]
    fn runtime_cleanup_status_includes_frozen_runtime_cleanup_resources() {
        let mut stack_state = StackState::new(Platform::Aws);
        stack_state.resources.insert(
            "compute".to_string(),
            resource_state(
                Resource::new(ComputeCluster::new("compute".to_string()).build()),
                ResourceLifecycle::Frozen,
                ResourceStatus::TeardownRequired,
            ),
        );
        stack_state.resources.insert(
            "live-storage".to_string(),
            resource_state(
                Resource::new(Storage::new("storage".to_string()).build()),
                ResourceLifecycle::Live,
                ResourceStatus::Deleted,
            ),
        );

        assert_eq!(
            compute_runtime_cleanup_status(&stack_state, SetupManagement::Manual).unwrap(),
            StackStatus::Deleted
        );
        assert!(has_remaining_setup_resources(
            &stack_state,
            SetupManagement::Manual
        ));
    }

    #[tokio::test]
    async fn automatic_cleanup_waits_for_role_deletion_before_setup_handoff() {
        for (fail_delete, managed_pages, inline_pages, fail_cleanup) in [
            (false, false, false, false),
            (true, false, false, false),
            (false, true, false, false),
            (false, false, true, false),
            (false, true, true, false),
            (true, true, true, false),
            (false, false, false, true),
        ] {
            let mut stack = StackState::with_resource_prefix(Platform::Aws, "test".to_string());
            let mut vault = resource_state(
                Resource::new(Vault::new("secrets".to_string()).build()),
                ResourceLifecycle::Frozen,
                ResourceStatus::Running,
            );
            vault.dependencies = vec![ResourceRef::new(
                ServiceAccount::RESOURCE_TYPE,
                "execution-sa",
            )];
            let vault_before = serde_json::to_value(&vault).unwrap();
            stack.resources.insert("secrets".to_string(), vault);
            let mut role = resource_state(
                Resource::new(ServiceAccount::new("execution-sa".to_string()).build()),
                ResourceLifecycle::Frozen,
                ResourceStatus::Deleting,
            );
            role.set_internal_controller(Some(Box::new(
                serde_json::from_value::<AwsServiceAccountController>(serde_json::json!({
                    "state": "deleteStart",
                    "roleName": "test-execution-sa",
                    "roleArn": "arn:aws:iam::123456789012:role/test-execution-sa",
                    "runtimeCreated": true,
                    "stackPermissionsApplied": true
                }))
                .unwrap(),
            )))
            .unwrap();
            stack.resources.insert("execution-sa".to_string(), role);
            let mut iam = MockIamApi::new();
            if fail_cleanup {
                // Managed policy removal succeeds before inline cleanup is denied.
                iam.expect_list_attached_role_policies()
                    .withf(|role| role == "test-execution-sa")
                    .times(1)
                    .return_once(|_| Ok(serde_json::from_value(serde_json::json!({
                        "ListAttachedRolePoliciesResult": {
                            "IsTruncated": false,
                            "AttachedPolicies": {"member": [{
                                "PolicyName": "removed-before-failure",
                                "PolicyArn": "arn:aws:iam::123456789012:policy/removed-before-failure"
                            }]}
                        }
                    })).unwrap()));
                iam.expect_detach_role_policy()
                    .withf(|role, policy| {
                        role == "test-execution-sa"
                            && policy == "arn:aws:iam::123456789012:policy/removed-before-failure"
                    })
                    .times(1)
                    .return_once(|_, _| Ok(()));
                iam.expect_list_role_policies()
                    .withf(|role| role == "test-execution-sa")
                    .times(1)
                    .return_once(|_| {
                        Ok(serde_json::from_value(serde_json::json!({
                            "ListRolePoliciesResult": {
                                "IsTruncated": false, "PolicyNames": {"member": ["last-inline"]}
                            }
                        }))
                        .unwrap())
                    });
                iam.expect_delete_role_policy()
                    .withf(|role, policy| role == "test-execution-sa" && policy == "last-inline")
                    .times(1)
                    .return_once(|_, _| {
                        Err(alien_error::AlienError::new(
                            alien_client_core::ErrorData::RemoteAccessDenied {
                                resource_type: "IAM role policy".to_string(),
                                resource_name: "last-inline".to_string(),
                            },
                        ))
                    });
            }
            let paginated = managed_pages || inline_pages;
            if paginated {
                iam.expect_list_attached_role_policies()
                    .times(1)
                    .return_once(move |_| {
                        Ok(serde_json::from_value(serde_json::json!({
                            "ListAttachedRolePoliciesResult": {
                                "IsTruncated": managed_pages,
                                "AttachedPolicies": {"member": [{
                                    "PolicyName": "first-managed",
                                    "PolicyArn": "arn:aws:iam::123456789012:policy/first-managed"
                                }]}
                            }
                        }))
                        .unwrap())
                    });
                iam.expect_list_role_policies()
                    .times(1)
                    .return_once(move |_| {
                        Ok(serde_json::from_value(serde_json::json!({
                            "ListRolePoliciesResult": {
                                "IsTruncated": inline_pages,
                                "PolicyNames": {"member": ["first-inline"]}
                            }
                        }))
                        .unwrap())
                    });
                iam.expect_detach_role_policy()
                    .withf(|role, policy| {
                        role == "test-execution-sa"
                            && policy == "arn:aws:iam::123456789012:policy/first-managed"
                    })
                    .times(1)
                    .returning(|_, _| Ok(()));
                iam.expect_delete_role_policy()
                    .withf(|role, policy| role == "test-execution-sa" && policy == "first-inline")
                    .times(1)
                    .returning(|_, _| Ok(()));
            }
            iam.expect_list_attached_role_policies()
                .times(1)
                .return_once(move |_| {
                    let policies = if managed_pages {
                        vec![serde_json::json!({
                            "PolicyName": "last-managed",
                            "PolicyArn": "arn:aws:iam::123456789012:policy/last-managed"
                        })]
                    } else {
                        vec![]
                    };
                    Ok(serde_json::from_value(serde_json::json!({
                        "ListAttachedRolePoliciesResult": {
                            "IsTruncated": false, "AttachedPolicies": {"member": policies}
                        }
                    }))
                    .unwrap())
                });
            iam.expect_list_role_policies()
                .times(1)
                .return_once(move |_| {
                    let policies = if inline_pages || fail_cleanup {
                        vec!["last-inline"]
                    } else {
                        vec![]
                    };
                    Ok(serde_json::from_value(serde_json::json!({
                        "ListRolePoliciesResult": {
                            "IsTruncated": false, "PolicyNames": {"member": policies}
                        }
                    }))
                    .unwrap())
                });
            iam.expect_detach_role_policy()
                .withf(|role, policy| {
                    role == "test-execution-sa"
                        && policy == "arn:aws:iam::123456789012:policy/last-managed"
                })
                .times(usize::from(managed_pages))
                .returning(|_, _| Ok(()));
            iam.expect_delete_role_policy()
                .withf(|role, policy| role == "test-execution-sa" && policy == "last-inline")
                .times(usize::from(inline_pages || fail_cleanup))
                .returning(|_, _| Ok(()));
            iam.expect_delete_role()
                .withf(|name| name == "test-execution-sa")
                .times(1)
                .return_once(move |_| {
                    if fail_delete {
                        Err(alien_error::AlienError::new(
                            alien_client_core::ErrorData::RemoteAccessDenied {
                                resource_type: "IAM role".to_string(),
                                resource_name: "test-execution-sa".to_string(),
                            },
                        ))
                    } else {
                        Ok(())
                    }
                });
            if fail_delete {
                iam.expect_delete_role()
                    .withf(|name| name == "test-execution-sa")
                    .times(1)
                    .return_once(|_| Ok(()));
            }
            let iam = Arc::new(iam);
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_runtime_setup_authority()
                .returning(|_| None);
            provider
                .expect_get_aws_iam_client()
                .returning(move |_| Ok(iam.clone()));
            let provider = Arc::new(provider);
            let config = DeploymentConfig::builder()
                .stack_settings(StackSettings {
                    setup_management: SetupManagement::Automatic,
                    ..Default::default()
                })
                .environment_variables(EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .external_bindings(ExternalBindings::default())
                .allow_frozen_changes(false)
                .build();
            let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
            let mut current = DeploymentState {
                status: DeploymentStatus::Deleting,
                platform: Platform::Aws,
                current_release: None,
                target_release: None,
                stack_state: Some(stack),
                error: None,
                environment_info: None,
                runtime_metadata: Some(RuntimeMetadata {
                    deletion_policy: alien_core::DeletionPolicy::Erase,
                    vault_credentials_revoked: true,
                    ..Default::default()
                }),
                retry_requested: false,
                protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            };
            if fail_cleanup {
                current
                    .stack_state
                    .as_mut()
                    .unwrap()
                    .resources
                    .get_mut("execution-sa")
                    .unwrap()
                    .retry_attempt = 9;
                let failed =
                    handle_deleting(current, config.clone(), client.clone(), provider.clone())
                        .await
                        .expect("record partial policy cleanup failure")
                        .state;
                assert_eq!(failed.status, DeploymentStatus::DeleteFailed);
                let stack = failed.stack_state.as_ref().unwrap();
                let role = &stack.resources["execution-sa"];
                assert_eq!(role.status, ResourceStatus::DeleteFailed);
                assert!(role.error.is_some());
                assert_eq!(
                    role.last_failed_state.as_ref().unwrap()["state"],
                    "deleteStart"
                );
                assert_eq!(
                    role.internal_state.as_ref().unwrap()["roleName"],
                    "test-execution-sa"
                );
                assert_eq!(
                    serde_json::to_value(&stack.resources["secrets"]).unwrap(),
                    vault_before
                );
                current = serde_json::from_value(serde_json::to_value(failed).unwrap()).unwrap();
                current.retry_requested = true;
                current =
                    handle_delete_failed(current, config.clone(), client.clone(), provider.clone())
                        .await
                        .expect("retry checkpointed policy cleanup")
                        .state;
                assert_eq!(current.status, DeploymentStatus::Deleting);
                assert!(!current.retry_requested);
                current = serde_json::from_value(serde_json::to_value(current).unwrap()).unwrap();
            }
            let mut pending =
                handle_deleting(current, config.clone(), client.clone(), provider.clone())
                    .await
                    .expect("policy cleanup step")
                    .state;
            assert_eq!(pending.status, DeploymentStatus::Deleting);
            assert_eq!(
                pending.stack_state.as_ref().unwrap().resources["execution-sa"].status,
                ResourceStatus::Deleting
            );
            if paginated {
                let role = &pending.stack_state.as_ref().unwrap().resources["execution-sa"];
                assert_eq!(
                    role.internal_state.as_ref().unwrap()["state"],
                    "deleteStart"
                );
                pending = serde_json::from_value(serde_json::to_value(pending).unwrap()).unwrap();
                pending =
                    handle_deleting(pending, config.clone(), client.clone(), provider.clone())
                        .await
                        .expect("resume policy cleanup after a truncated page")
                        .state;
                assert_eq!(pending.status, DeploymentStatus::Deleting);
                let role = &pending.stack_state.as_ref().unwrap().resources["execution-sa"];
                assert_eq!(
                    role.internal_state.as_ref().unwrap()["state"],
                    "deletingRole"
                );
            }
            // Exercise the final retry on failure without waiting through backoff.
            pending
                .stack_state
                .as_mut()
                .unwrap()
                .resources
                .get_mut("execution-sa")
                .unwrap()
                .retry_attempt = 9;
            let pending = serde_json::from_value(serde_json::to_value(pending).unwrap()).unwrap();
            let finished =
                handle_deleting(pending, config.clone(), client.clone(), provider.clone())
                    .await
                    .expect("role deletion step")
                    .state;
            let stack = finished.stack_state.as_ref().unwrap();
            assert_eq!(
                serde_json::to_value(&stack.resources["secrets"]).unwrap(),
                vault_before
            );
            if fail_delete {
                assert_eq!(finished.status, DeploymentStatus::DeleteFailed);
                assert_eq!(
                    stack.resources["execution-sa"].status,
                    ResourceStatus::DeleteFailed
                );
                assert!(stack.resources["execution-sa"].error.is_some());
                let mut restored: DeploymentState =
                    serde_json::from_value(serde_json::to_value(&finished).unwrap()).unwrap();
                restored.retry_requested = true;
                let mut retried = handle_delete_failed(
                    restored,
                    config.clone(),
                    client.clone(),
                    provider.clone(),
                )
                .await
                .expect("retry checkpointed role deletion")
                .state;
                assert_eq!(retried.status, DeploymentStatus::Deleting);
                assert!(!retried.retry_requested);
                for _ in 0..4 {
                    retried =
                        serde_json::from_value(serde_json::to_value(retried).unwrap()).unwrap();
                    retried =
                        handle_deleting(retried, config.clone(), client.clone(), provider.clone())
                            .await
                            .expect("resume role deletion")
                            .state;
                    if retried.status == DeploymentStatus::TeardownRequired {
                        break;
                    }
                    assert_eq!(retried.status, DeploymentStatus::Deleting);
                }
                assert_eq!(retried.status, DeploymentStatus::TeardownRequired);
                let stack = retried.stack_state.unwrap();
                assert_eq!(
                    stack.resources["execution-sa"].status,
                    ResourceStatus::Deleted
                );
                assert!(stack.resources["execution-sa"].error.is_none());
                assert_eq!(
                    serde_json::to_value(&stack.resources["secrets"]).unwrap(),
                    vault_before
                );
            } else {
                assert_eq!(finished.status, DeploymentStatus::TeardownRequired);
                assert_eq!(
                    stack.resources["execution-sa"].status,
                    ResourceStatus::Deleted
                );
            }
        }
    }

    async fn runtime_cleanup_of_an_empty_stack(
        runtime_metadata: Option<alien_core::RuntimeMetadata>,
    ) -> DeploymentStatus {
        let current = DeploymentState {
            status: DeploymentStatus::Deleting,
            platform: Platform::Aws,
            current_release: None,
            target_release: None,
            stack_state: Some(StackState::with_resource_prefix(
                Platform::Aws,
                "test".to_string(),
            )),
            error: None,
            environment_info: None,
            runtime_metadata,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
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
        handle_deleting(
            current,
            config,
            alien_core::ClientConfig::Aws(Box::new(
                <alien_aws_clients::AwsClientConfig as alien_aws_clients::AwsClientConfigExt>::mock(
                ),
            )),
            Arc::new(DefaultPlatformServiceProvider::default()),
        )
        .await
        .expect("runtime cleanup of an empty stack completes")
        .state
        .status
    }

    fn scaffolding_record(
        authority: alien_core::InitialSetupAuthority,
    ) -> alien_core::RuntimeMetadata {
        alien_core::RuntimeMetadata {
            initial_setup_authority: authority,
            setup_scaffolding: std::collections::BTreeMap::from([(
                "agents".to_string(),
                alien_core::SetupScaffolding::AwsSandbox {
                    build_role_name: "test-agents-build".to_string(),
                    egress: None,
                    image_arn: None,
                },
            )]),
            ..Default::default()
        }
    }

    /// A direct setup's scaffolding is not a stack resource, so a stack of only Live resources
    /// would otherwise read as fully deleted and leave the recorded roles and groups behind.
    #[tokio::test]
    async fn recorded_setup_scaffolding_keeps_a_cleaned_up_deployment_for_teardown() {
        assert_eq!(
            runtime_cleanup_of_an_empty_stack(Some(scaffolding_record(
                alien_core::InitialSetupAuthority::DirectSetup
            )))
            .await,
            DeploymentStatus::TeardownRequired
        );
    }

    fn prepared_with_a_sandbox(
        authority: alien_core::InitialSetupAuthority,
    ) -> alien_core::RuntimeMetadata {
        let sandbox = alien_core::Sandbox::new("agents".to_string())
            .code(alien_core::SandboxCode::Image {
                image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
            })
            .egress(alien_core::SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        alien_core::RuntimeMetadata {
            initial_setup_authority: authority,
            prepared_stack: Some(
                alien_core::Stack::new("acme".to_string())
                    .add(sandbox, ResourceLifecycle::Live)
                    .build(),
            ),
            ..Default::default()
        }
    }

    /// A step whose checkpoint never landed can leave scaffolding the record does not name, so
    /// setup teardown still runs to look for it.
    #[tokio::test]
    async fn a_scaffolded_stack_with_an_empty_record_is_kept_for_teardown() {
        assert_eq!(
            runtime_cleanup_of_an_empty_stack(Some(prepared_with_a_sandbox(
                alien_core::InitialSetupAuthority::DirectSetup
            )))
            .await,
            DeploymentStatus::TeardownRequired
        );
        assert_eq!(
            runtime_cleanup_of_an_empty_stack(Some(prepared_with_a_sandbox(
                alien_core::InitialSetupAuthority::ImportedHandoff
            )))
            .await,
            DeploymentStatus::Deleted
        );
    }

    #[tokio::test]
    async fn a_cleaned_up_deployment_with_nothing_left_for_setup_is_deleted() {
        assert_eq!(
            runtime_cleanup_of_an_empty_stack(None).await,
            DeploymentStatus::Deleted
        );
        assert_eq!(
            runtime_cleanup_of_an_empty_stack(Some(alien_core::RuntimeMetadata {
                initial_setup_authority: alien_core::InitialSetupAuthority::DirectSetup,
                ..Default::default()
            }))
            .await,
            DeploymentStatus::Deleted,
            "an emptied record has nothing left to tear down"
        );
        assert_eq!(
            runtime_cleanup_of_an_empty_stack(Some(alien_core::RuntimeMetadata {
                initial_setup_authority: alien_core::InitialSetupAuthority::ImportedHandoff,
                ..Default::default()
            }))
            .await,
            DeploymentStatus::Deleted,
            "a template setup's scaffolding is its template's to remove"
        );
    }

    #[tokio::test]
    async fn local_daemon_runtime_delete_without_local_provider_fails_at_resource() {
        let daemon = Daemon::new("gateway".to_string())
            .code(DaemonCode::Image {
                image: "gateway:test".to_string(),
            })
            .permissions("default".to_string())
            .build();
        let mut stack_state = StackState::new(Platform::Local);
        let mut daemon_state = resource_state(
            Resource::new(daemon),
            ResourceLifecycle::Live,
            ResourceStatus::Running,
        );
        daemon_state.internal_state = Some(serde_json::json!({
            "type": "LocalDaemonController",
            "_controllerStateVersion": 1,
            "extractedImagePath": "/var/lib/alien-agent/daemons/gateway",
            "daemonName": "gateway",
            "publicUrl": null,
            "state": "ready",
            "internalStayCount": null
        }));
        stack_state
            .resources
            .insert("gateway".to_string(), daemon_state);

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
        let current = DeploymentState {
            status: DeploymentStatus::DeletePending,
            platform: Platform::Local,
            current_release: None,
            target_release: None,
            stack_state: Some(stack_state),
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let client_config = alien_core::ClientConfig::Local {
            state_directory: "/var/lib/alien-agent".to_string(),
        };
        let service_provider = Arc::new(DefaultPlatformServiceProvider::default());

        let delete_pending = handle_delete_pending(
            current,
            config.clone(),
            client_config.clone(),
            service_provider.clone(),
        )
        .await
        .expect("delete-pending should transition to deleting");

        let deleting = handle_deleting(
            delete_pending.state,
            config,
            client_config,
            service_provider,
        )
        .await
        .expect("deleting handler should checkpoint resource failure");

        assert_eq!(deleting.state.status, DeploymentStatus::DeleteFailed);
        let stack_state = deleting
            .state
            .stack_state
            .expect("delete failed state should keep stack state");
        let resource = stack_state
            .resources
            .get("gateway")
            .expect("daemon resource should remain in stack state");
        assert_eq!(resource.status, ResourceStatus::DeleteFailed);
        let error = resource
            .error
            .as_ref()
            .expect("daemon delete failure should store resource error");
        assert!(
            error.message.contains("LocalWorkerManager"),
            "expected LocalWorkerManager error, got {error:?}"
        );
    }

    fn after_setup_failed(
        authority: alien_core::InitialSetupAuthority,
        management_outputs: bool,
        live_status: ResourceStatus,
    ) -> DeploymentState {
        let mut stack_state = StackState::with_resource_prefix(Platform::Aws, "test".to_string());
        let mut management = resource_state(
            Resource::new(alien_core::RemoteStackManagement {
                id: "management".to_string(),
            }),
            ResourceLifecycle::Frozen,
            ResourceStatus::ProvisionFailed,
        );
        if management_outputs {
            management.status = ResourceStatus::Running;
            management.outputs = Some(alien_core::ResourceOutputs::new(
                alien_core::RemoteStackManagementOutputs {
                    aws_permissions_boundary_arn: None,
                    management_resource_id: "arn:aws:iam::123456789012:role/test-management"
                        .to_string(),
                    access_configuration: "arn:aws:iam::123456789012:role/test-management"
                        .to_string(),
                    legacy_remote_bindings_access: None,
                },
            ));
        }
        stack_state
            .resources
            .insert("management".to_string(), management);
        stack_state.resources.insert(
            "live-storage".to_string(),
            resource_state(
                Resource::new(Storage::new("live-storage".to_string()).build()),
                ResourceLifecycle::Live,
                live_status,
            ),
        );
        DeploymentState {
            status: DeploymentStatus::DeletePending,
            platform: Platform::Aws,
            current_release: None,
            target_release: None,
            stack_state: Some(stack_state),
            error: None,
            environment_info: None,
            runtime_metadata: Some(scaffolding_record(authority)),
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    /// Setup failed on the management identity after building the sandbox's role: runtime
    /// cleanup could never obtain credentials, so destroy must reach setup teardown without it.
    #[test]
    fn a_destroy_before_the_management_identity_existed_goes_to_setup_teardown() {
        let state = after_setup_failed(
            alien_core::InitialSetupAuthority::DirectSetup,
            false,
            ResourceStatus::Pending,
        );
        assert_eq!(
            super::destroy_without_runtime(&state),
            Some(DeploymentStatus::TeardownRequired)
        );
        for (status, retry_requested) in [
            (DeploymentStatus::Deleting, false),
            (DeploymentStatus::DeleteFailed, true),
        ] {
            let later = DeploymentState {
                status,
                retry_requested,
                ..state.clone()
            };
            assert_eq!(
                super::destroy_without_runtime(&later),
                Some(DeploymentStatus::TeardownRequired),
                "{status:?}"
            );
        }
    }

    /// An installed GCP deployment at `protocol_version` whose Live storage is `live_status`.
    fn installed_gcp(
        protocol_version: u32,
        authority: alien_core::InitialSetupAuthority,
        live_status: ResourceStatus,
    ) -> DeploymentState {
        let mut state = after_setup_failed(authority, true, live_status);
        state.platform = Platform::Gcp;
        state.protocol_version = protocol_version;
        let stack_state = state.stack_state.as_mut().unwrap();
        if protocol_version < alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION {
            stack_state.resources.get_mut("management").unwrap().outputs = Some(
                alien_core::ResourceOutputs::new(alien_core::RemoteStackManagementOutputs {
                    aws_permissions_boundary_arn: None,
                    management_resource_id: "manager@test-project.iam.gserviceaccount.com".into(),
                    access_configuration: "manager@test-project.iam.gserviceaccount.com".into(),
                    legacy_remote_bindings_access: None,
                }),
            );
        }
        stack_state.platform = Platform::Gcp;
        stack_state.resources.insert(
            "data".to_string(),
            resource_state(
                Resource::new(Storage::new("data".to_string()).build()),
                ResourceLifecycle::Frozen,
                ResourceStatus::Running,
            ),
        );
        state
    }

    /// A GCP deployment installed before federation has no target trust the manager can use,
    /// so runtime cleanup can never run. With nothing that needs it, its setup-owned resources
    /// go to setup teardown, however setup was run.
    #[test]
    fn a_pre_federation_gcp_destroy_without_runtime_work_goes_to_setup_teardown() {
        for authority in [
            alien_core::InitialSetupAuthority::DirectSetup,
            alien_core::InitialSetupAuthority::ImportedHandoff,
        ] {
            let mut state = installed_gcp(1, authority, ResourceStatus::Pending);
            // Scheduling delete fences old readers, but does not install federation.
            state.protocol_version = alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION;
            assert_eq!(
                super::destroy_without_runtime(&state),
                Some(DeploymentStatus::RetentionRequired),
                "{authority:?}"
            );
        }
    }

    #[test]
    fn other_installed_deployments_keep_their_runtime_cleanup() {
        let direct = alien_core::InitialSetupAuthority::DirectSetup;
        for (case, state) in [
            (
                "a pre-federation GCP deployment with a provisioned Live resource",
                installed_gcp(1, direct, ResourceStatus::Running),
            ),
            (
                "a federated GCP deployment",
                installed_gcp(
                    alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION,
                    direct,
                    ResourceStatus::Pending,
                ),
            ),
            (
                "an AWS deployment at protocol 1",
                DeploymentState {
                    platform: Platform::Aws,
                    ..installed_gcp(1, direct, ResourceStatus::Pending)
                },
            ),
        ] {
            assert_eq!(super::destroy_without_runtime(&state), None, "{case}");
        }
    }

    /// A rejected first deploy has an empty inventory and no successful setup metadata.
    /// Destroy must not ask for credentials belonging to an identity it never created.
    #[test]
    fn destroy_of_an_empty_first_deployment_needs_no_runtime_credentials() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            // Both the legacy empty object and the explicit default authority occur in
            // stored records from before setup has run.
            for metadata in [
                None,
                Some(serde_json::from_str::<alien_core::RuntimeMetadata>("{}").unwrap()),
                Some(
                    serde_json::from_str::<alien_core::RuntimeMetadata>(
                        r#"{"initialSetupAuthority":"importedHandoff"}"#,
                    )
                    .unwrap(),
                ),
            ] {
                let state = DeploymentState {
                    status: DeploymentStatus::DeletePending,
                    platform,
                    current_release: None,
                    target_release: None,
                    stack_state: Some(
                        serde_json::from_value(serde_json::json!({
                            "platform": platform, "resourcePrefix": "test", "resources": {}
                        }))
                        .unwrap(),
                    ),
                    error: None,
                    environment_info: None,
                    runtime_metadata: metadata,
                    retry_requested: false,
                    protocol_version: alien_core::deployment_protocol_version_for_platform(
                        platform, None,
                    ),
                };
                for (status, retry_requested) in [
                    (DeploymentStatus::DeletePending, false),
                    (DeploymentStatus::Deleting, false),
                    (DeploymentStatus::DeleteFailed, true),
                ] {
                    let state = DeploymentState {
                        status,
                        retry_requested,
                        ..state.clone()
                    };
                    assert_eq!(
                        super::destroy_without_runtime(&state),
                        Some(DeploymentStatus::Deleted)
                    );
                }
                for (status, retry_requested) in [
                    (DeploymentStatus::PreflightsFailed, false),
                    (DeploymentStatus::DeleteFailed, false),
                ] {
                    assert_eq!(
                        super::destroy_without_runtime(&DeploymentState {
                            status,
                            retry_requested,
                            ..state.clone()
                        }),
                        None
                    );
                }
                // A missing inventory is not evidence of an empty inventory.
                assert_eq!(
                    super::destroy_without_runtime(&DeploymentState {
                        stack_state: None,
                        ..state.clone()
                    }),
                    None
                );
                // A previous successful install must retain normal cleanup even if empty.
                assert_eq!(
                    super::destroy_without_runtime(&DeploymentState {
                        current_release: Some(alien_core::ReleaseInfo {
                            release_id: Some("previous".into()),
                            version: None,
                            description: None,
                            stack: alien_core::Stack::new("test".into()).build(),
                        }),
                        ..state.clone()
                    }),
                    None
                );
                // Setup or access bookkeeping must not be discarded with an empty inventory.
                for metadata in [
                    alien_core::RuntimeMetadata {
                        prepared_stack: Some(alien_core::Stack::new("test".into()).build()),
                        ..Default::default()
                    },
                    scaffolding_record(alien_core::InitialSetupAuthority::ImportedHandoff),
                    alien_core::RuntimeMetadata {
                        registry_access_granted: true,
                        ..Default::default()
                    },
                ] {
                    assert_eq!(
                        super::destroy_without_runtime(&DeploymentState {
                            runtime_metadata: Some(metadata),
                            ..state.clone()
                        }),
                        None
                    );
                }
            }
        }
    }

    fn with_resource(
        mut state: DeploymentState,
        id: &str,
        resource: Resource,
        lifecycle: Option<ResourceLifecycle>,
        status: ResourceStatus,
    ) -> DeploymentState {
        let mut entry = resource_state(resource, ResourceLifecycle::Frozen, status);
        entry.lifecycle = lifecycle;
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert(id.to_string(), entry);
        state
    }

    fn sandbox(id: &str) -> Resource {
        Resource::new(
            alien_core::Sandbox::new(id.to_string())
                .code(alien_core::SandboxCode::Image {
                    image: "s3://acme/sandbox-bundle/f00d/bundle.zip".to_string(),
                })
                .egress(alien_core::SandboxEgress::Internet)
                .lifecycle(alien_core::SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
        )
    }

    /// A stack prepared with no management configuration has no management identity to wait for;
    /// setup failing on another Frozen resource still leaves nothing for runtime cleanup.
    #[test]
    fn a_destroy_with_no_management_identity_planned_goes_to_setup_teardown() {
        let mut state = after_setup_failed(
            alien_core::InitialSetupAuthority::DirectSetup,
            false,
            ResourceStatus::Pending,
        );
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .remove("management");
        let state = with_resource(
            state,
            "frozen-sandbox",
            sandbox("frozen-sandbox"),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
        );
        assert_eq!(
            super::destroy_without_runtime(&state),
            Some(DeploymentStatus::TeardownRequired)
        );
    }

    /// What runtime cleanup acts on beyond Live resources: synced secrets, and a Frozen type
    /// with a runtime share of its delete. Once either came up, the destroy keeps its cleanup.
    #[test]
    fn setup_created_objects_runtime_cleanup_owns_keep_it() {
        let direct = alien_core::InitialSetupAuthority::DirectSetup;
        let base = || after_setup_failed(direct, false, ResourceStatus::Pending);
        for (case, state) in [
            (
                "a secrets vault setup brought up",
                with_resource(
                    base(),
                    "secrets",
                    Resource::new(alien_core::Vault::new("secrets".to_string()).build()),
                    Some(ResourceLifecycle::Frozen),
                    ResourceStatus::Running,
                ),
            ),
            (
                "a compute cluster setup brought up",
                with_resource(
                    base(),
                    "compute",
                    Resource::new(ComputeCluster::new("compute".to_string()).build()),
                    Some(ResourceLifecycle::Frozen),
                    ResourceStatus::Running,
                ),
            ),
            (
                "a resource with no recorded lifecycle",
                with_resource(
                    base(),
                    "unknown",
                    Resource::new(Storage::new("unknown".to_string()).build()),
                    None,
                    ResourceStatus::Pending,
                ),
            ),
        ] {
            assert_eq!(super::destroy_without_runtime(&state), None, "{case}");
        }
    }

    #[test]
    fn a_runtime_that_started_keeps_its_runtime_cleanup() {
        let direct = alien_core::InitialSetupAuthority::DirectSetup;
        for (case, state) in [
            (
                "management identity has outputs",
                after_setup_failed(direct, true, ResourceStatus::Pending),
            ),
            (
                "a Live resource was provisioned",
                after_setup_failed(direct, false, ResourceStatus::Running),
            ),
            (
                "a Live resource failed mid-provision",
                after_setup_failed(direct, false, ResourceStatus::ProvisionFailed),
            ),
            (
                "setup was imported from a template",
                after_setup_failed(
                    alien_core::InitialSetupAuthority::ImportedHandoff,
                    false,
                    ResourceStatus::Pending,
                ),
            ),
            (
                "the platform has no management identity",
                DeploymentState {
                    platform: Platform::Kubernetes,
                    ..after_setup_failed(direct, false, ResourceStatus::Pending)
                },
            ),
            (
                "a failed destroy nobody asked to retry",
                DeploymentState {
                    status: DeploymentStatus::DeleteFailed,
                    ..after_setup_failed(direct, false, ResourceStatus::Pending)
                },
            ),
            (
                "the deployment is not being destroyed",
                DeploymentState {
                    status: DeploymentStatus::InitialSetupFailed,
                    ..after_setup_failed(direct, false, ResourceStatus::Pending)
                },
            ),
        ] {
            assert_eq!(super::destroy_without_runtime(&state), None, "{case}");
        }
    }
}
