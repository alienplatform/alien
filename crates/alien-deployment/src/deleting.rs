use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{
    ownership_policy_for_resource_type, InitialSetupAuthority, Platform, ResourceLifecycle,
    ResourceStatus, StackState, StackStatus,
};
use alien_error::{AlienError, Context};
use alien_infra::{state_utils::StackStateExt, PlatformServiceProvider, StackExecutor};
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

    if stack_state.resources.contains_key("secrets") {
        let runtime_metadata = next.runtime_metadata.get_or_insert_default();
        let prepared_stack = runtime_metadata.prepared_stack.clone().ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: "Prepared stack required for vault secret deletion".to_string(),
            })
        })?;
        crate::helpers::delete_deployment_vault_secrets(
            &prepared_stack,
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

    let prepared =
        prepare_resources_for_destroy(&mut stack_state, next.platform, service_provider.as_ref())
            .context(ErrorData::StackExecutionFailed {
            message: "Failed to prepare runtime resources for destroy".to_string(),
        })?;

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

    let owns_setup = owns_runtime_setup(current_cloned.platform, service_provider.as_ref());
    let executor = if owns_setup {
        StackExecutor::for_deletion_with_service_provider(
            client_config,
            &config,
            service_provider,
            None,
        )
    } else {
        StackExecutor::for_runtime_cleanup_deletion_with_service_provider(
            client_config,
            &config,
            service_provider,
        )
    }
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

    let stack_status = if owns_setup && !step_result.next_state.resources.is_empty() {
        step_result
            .next_state
            .compute_stack_status()
            .context(ErrorData::StackExecutionFailed {
                message: "Failed to compute local teardown status".to_string(),
            })
    } else {
        compute_runtime_cleanup_status(&step_result.next_state)
    }
    .context(ErrorData::StackExecutionFailed {
        message: "Failed to compute runtime cleanup status".to_string(),
    })?;

    let result = if stack_status == StackStatus::Deleted {
        let next_status = if has_remaining_setup_resources(&step_result.next_state)
            || crate::setup_teardown::has_setup_scaffolding(
                current_cloned.runtime_metadata.as_ref(),
                current_cloned.platform,
            ) {
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

/// Where a destroy goes when the runtime never started, or `None` when runtime cleanup has work.
/// It has none while the management identity has no outputs and no Live resource, synced secrets
/// vault or Frozen runtime-cleanup type came up. Fails closed on a missing lifecycle.
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
    let metadata = current.runtime_metadata.as_ref()?;
    if metadata.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup {
        return None;
    }
    let stack_state = current.stack_state.as_ref()?;
    for (resource_id, resource) in &stack_state.resources {
        let lifecycle = resource.lifecycle?;
        let resource_type = resource.config.resource_type();
        if resource_type == alien_core::RemoteStackManagement::RESOURCE_TYPE {
            if resource.outputs.is_some() {
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
    Some(
        if has_remaining_setup_resources(stack_state)
            || crate::setup_teardown::has_setup_scaffolding(Some(metadata), current.platform)
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
    _config: DeploymentConfig,
    _client_config: alien_core::ClientConfig,
    service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
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

    let prepared =
        prepare_resources_for_destroy(&mut stack_state, next.platform, service_provider.as_ref())
            .context(ErrorData::StackExecutionFailed {
            message: "Failed to prepare runtime resources for delete retry".to_string(),
        })?;

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

fn owns_runtime_setup(platform: Platform, service_provider: &dyn PlatformServiceProvider) -> bool {
    service_provider.runtime_setup_authority(platform) == Some(InitialSetupAuthority::DirectSetup)
}

fn prepare_resources_for_destroy(
    stack_state: &mut StackState,
    platform: Platform,
    service_provider: &dyn PlatformServiceProvider,
) -> alien_infra::Result<Vec<String>> {
    if owns_runtime_setup(platform, service_provider) {
        stack_state.prepare_for_destroy()
    } else {
        stack_state.prepare_for_runtime_cleanup_destroy()
    }
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
        ClientConfig, ComputeCluster, Daemon, DaemonCode, DeploymentConfig, DeploymentState,
        DeploymentStatus, EnvironmentVariablesSnapshot, ExternalBindings, InitialSetupAuthority,
        Platform, Resource, ResourceLifecycle, ResourceStatus, RuntimeMetadata, Stack,
        StackResourceState, StackSettings, StackState, StackStatus, Storage,
    };
    use alien_infra::{state_utils::StackStateExt, DefaultPlatformServiceProvider, StackExecutor};

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
    async fn local_manager_destroy_removes_frozen_storage_with_its_own_services() {
        let fixture = local_storage_fixture().await;
        let neighbor = fixture.directory.path().join("neighbor.txt");
        std::fs::write(&neighbor, b"outside storage").expect("neighbor marker");
        let current = DeploymentState::builder()
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
        let failed = DeploymentState::builder()
            .status(DeploymentStatus::DeleteFailed)
            .platform(Platform::Local)
            .stack_state(step.next_state)
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
            .egress(alien_core::SandboxEgress::Allow)
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
                .egress(alien_core::SandboxEgress::Allow)
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
