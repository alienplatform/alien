use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{
    ownership_policy_for_resource_type, ResourceLifecycle, ResourceStatus, StackState, StackStatus,
};
use alien_error::{AlienError, Context};
use alien_infra::StackExecutor;
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
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling DeletePending status");

    let mut next = current.clone();
    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for deletion".to_string(),
        })
    })?;

    if let Some(runtime_metadata) = next.runtime_metadata.as_mut() {
        crate::helpers::delete_deployment_vault_secrets(
            &stack_state,
            &client_config,
            &config,
            runtime_metadata,
        )
        .await
        .context(ErrorData::SecretSyncFailed {
            vault_name: "secrets".to_string(),
            reason: "Failed to delete deployment-owned secrets before runtime cleanup".to_string(),
        })?;
    }

    let prepared = prepare_runtime_resources_for_destroy(&mut stack_state).context(
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

    let current_cloned = current.clone();
    let stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for deletion".to_string(),
        })
    })?;

    let executor = StackExecutor::for_runtime_cleanup_deletion_with_service_provider(
        client_config,
        &config,
        service_provider,
    )
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to create stack executor for runtime cleanup".to_string(),
    })?;

    let step_result =
        executor
            .step(stack_state)
            .await
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to execute runtime cleanup step".to_string(),
            })?;

    let stack_status = compute_runtime_cleanup_status(&step_result.next_state).context(
        ErrorData::StackExecutionFailed {
            message: "Failed to compute runtime cleanup status".to_string(),
        },
    )?;

    let result = if stack_status == StackStatus::Deleted {
        let next_status = if has_remaining_setup_resources(&step_result.next_state) {
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
    _config: DeploymentConfig,
    _client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling DeleteFailed status");

    // Clone current first before moving any fields
    let mut next = current.clone();

    // Check if retry was requested
    if !current.retry_requested {
        info!("No retry requested, staying in DeleteFailed status");
        return Ok(DeploymentStepResult {
            state: current,
            suggested_delay_ms: None,
            update_heartbeat: false,
            heartbeats: vec![],
            observed_inventory_batches: vec![],
        });
    }

    let mut stack_state = current.stack_state.ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Stack state required for delete retry".to_string(),
        })
    })?;

    let prepared = prepare_runtime_resources_for_destroy(&mut stack_state).context(
        ErrorData::StackExecutionFailed {
            message: "Failed to prepare runtime resources for delete retry".to_string(),
        },
    )?;

    info!(
        "Prepared {} runtime resources for delete retry: {:?}",
        prepared.len(),
        prepared
    );

    next.status = DeploymentStatus::Deleting;
    next.stack_state = Some(stack_state);
    next.error = None;
    next.retry_requested = false;

    Ok(DeploymentStepResult {
        state: next,
        suggested_delay_ms: None,
        update_heartbeat: false,
        heartbeats: vec![],
        observed_inventory_batches: vec![],
    })
}

fn prepare_runtime_resources_for_destroy(
    stack_state: &mut StackState,
) -> alien_infra::Result<Vec<String>> {
    use alien_infra::state_utils::StackStateExt;
    stack_state.prepare_for_runtime_cleanup_destroy()
}

fn compute_runtime_cleanup_status(stack_state: &StackState) -> Result<StackStatus> {
    let mut statuses = Vec::new();

    for resource in stack_state.resources.values() {
        if !is_runtime_cleanup_resource(resource)? {
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

fn is_runtime_cleanup_resource(resource: &alien_core::StackResourceState) -> Result<bool> {
    if resource_lifecycle(resource)? == ResourceLifecycle::Live {
        return Ok(true);
    }

    Ok(
        ownership_policy_for_resource_type(resource.config.resource_type().as_ref())
            .has_runtime_cleanup_before_teardown(),
    )
}

fn has_remaining_setup_resources(stack_state: &StackState) -> bool {
    stack_state.resources.values().any(|resource| {
        resource.lifecycle != Some(ResourceLifecycle::Live)
            && resource.status != ResourceStatus::Deleted
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_core::{
        ComputeCluster, Daemon, DaemonCode, DeploymentConfig, DeploymentState, DeploymentStatus,
        EnvironmentVariablesSnapshot, ExternalBindings, Platform, Resource, ResourceLifecycle,
        ResourceStatus, StackResourceState, StackSettings, StackState, StackStatus, Storage,
    };
    use alien_infra::DefaultPlatformServiceProvider;

    use super::{
        compute_runtime_cleanup_status, handle_delete_pending, handle_deleting,
        has_remaining_setup_resources,
    };

    fn resource_state(
        resource: Resource,
        lifecycle: ResourceLifecycle,
        status: ResourceStatus,
    ) -> StackResourceState {
        StackResourceState {
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
            compute_runtime_cleanup_status(&stack_state).unwrap(),
            StackStatus::Deleted
        );
        assert!(has_remaining_setup_resources(&stack_state));
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
}
