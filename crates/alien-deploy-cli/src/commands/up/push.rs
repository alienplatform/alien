use super::*;

pub(super) fn should_print_push_setup_neutral_completion(platform: Platform) -> bool {
    platform != Platform::Machines
}

pub(super) async fn run_push_model(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    manager_url: &str,
    deployment_token: &str,
    management_config: Option<ManagementConfig>,
    network_args: &NetworkArgs,
    on_progress: Option<alien_deployment::runner::ProgressCallback>,
) -> Result<()> {
    let credential_platform = base_platform.unwrap_or(platform);
    let client_config = ClientConfig::from_std_env(credential_platform)
        .await
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to load {} credentials from environment. Ensure the required environment variables are set.",
                credential_platform
            ),
        })?;

    push_initial_setup(
        client,
        deployment_id,
        platform,
        base_platform,
        client_config,
        management_config,
        manager_url,
        deployment_token,
        Some(network_args),
        on_progress,
    )
    .await
}

pub(super) fn apply_external_bindings_from_stack_settings(
    config: &mut DeploymentConfig,
    stack_settings: &StackSettings,
) {
    if let Some(ref external_bindings) = stack_settings.external_bindings {
        config.external_bindings = external_bindings.clone();
    }
}

/// Run the push-model initial setup flow for a deployment.
///
/// Fetches deployment and release state from the manager, acquires a sync lock,
/// steps the deployment through InitialSetup until it reaches Provisioning (or a
/// terminal state), reconciles state back to the manager, and releases the lock.
///
/// This is used by both `alien-deploy deploy` (push model) and `alien-test` (e2e setup).
pub async fn push_initial_setup(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    client_config: ClientConfig,
    management_config: Option<alien_core::ManagementConfig>,
    manager_base_url: &str,
    deployment_token: &str,
    network_args: Option<&NetworkArgs>,
    on_progress: Option<alien_deployment::runner::ProgressCallback>,
) -> Result<()> {
    let setup_management_config = management_config.clone();

    // Get deployment from manager
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    // Reconstruct DeploymentState from flat API response
    let status = parse_deployment_status(&deployment.status)?;

    let stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    let environment_info = deployment
        .environment_info
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize environment_info from manager".to_string(),
        })?;

    // If there's a desired release, fetch the full release info. A failed fetch must fail the
    // setup, not silently degrade to a no-release deploy: swallowing it would report success while
    // having installed nothing the caller asked for.
    let target_release = if let Some(ref release_id) = deployment.desired_release_id {
        let resp = client
            .get_release()
            .id(release_id)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to fetch desired release {release_id} from manager"),
            })?;
        let rel = resp.into_inner();
        let platform_stack_value = release_stack_value_for_platform(rel.stack, platform)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "Release {} has no stack for platform {}",
                        release_id,
                        platform.as_str()
                    ),
                })
            })?;

        // No stack rewriting — release already stores proxy URIs.
        // Controllers use image URIs as-is.
        let stack = serde_json::from_value(platform_stack_value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse release stack".to_string(),
            })?;

        Some(ReleaseInfo {
            release_id: Some(rel.id),
            version: None,
            description: None,
            stack,
        })
    } else {
        None
    };

    let mut state = DeploymentState {
        status,
        platform,
        current_release: None,
        target_release,
        stack_state,
        error: None,
        environment_info,
        runtime_metadata: None,
        retry_requested: deployment.retry_requested,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };

    // Always override environment_info with the target client_config.
    // The manager may have already run the Pending step with management
    // credentials, setting environment_info to the management project.
    // push_initial_setup runs with *target* credentials, so re-collecting
    // ensures the environment_info reflects the actual target project.
    let environment_platform = base_platform.unwrap_or(platform);
    // Fail fast rather than proceed with absent/stale environment info: a setup that silently drops
    // the target environment would report success while the deployment's env_info is wrong. Wrap in
    // DeploymentFailed (retryable/internal = inherit), not the hard-non-retryable ConfigurationError,
    // so a transient cloud blip in collect_environment_info (live STS / project-metadata calls) stays
    // retryable instead of becoming a permanent setup failure.
    if should_collect_push_setup_environment_info(environment_platform) {
        let env_info =
            alien_deployment::collect_environment_info(environment_platform, &client_config)
                .await
                .context(ErrorData::DeploymentFailed {
                    operation: "target environment-info collection".to_string(),
                })?;
        state.environment_info = Some(env_info);
    } else {
        state.environment_info = None;
    }

    // Reconstruct DeploymentConfig from stack_settings
    let mut stack_settings: StackSettings = deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_settings from manager".to_string(),
        })?
        .unwrap_or_default();

    // Override network settings if the customer provided CLI flags
    if let Some(net_args) = network_args {
        let network_platform = base_platform.unwrap_or(platform);
        let network_override = network::parse_network_settings(net_args, network_platform.as_str())
            .map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "network".to_string(),
                    message: e,
                })
            })?;
        if let Some(ns) = network_override {
            stack_settings.network = Some(ns);
        }
    }

    // Build a minimal config JSON and deserialize to get proper defaults
    let mut config: DeploymentConfig = serde_json::from_value(serde_json::json!({
        "stackSettings": serde_json::to_value(&stack_settings).unwrap_or_default(),
        "managementConfig": serde_json::to_value(&management_config).unwrap_or_default(),
        "environmentVariables": {
            "variables": [],
            "hash": "",
            "createdAt": ""
        }
    }))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to construct deployment config".to_string(),
    })?;

    // Set manager URL and deployment token so controllers can configure
    // pull auth (RegistryCredentials, imagePullSecrets) for the manager's registry.
    config.manager_url = Some(manager_base_url.to_string());
    config.deployment_token = Some(deployment_token.to_string());
    config.base_platform = base_platform;

    apply_external_bindings_from_stack_settings(&mut config, &stack_settings);

    // Acquire sync lock — retry until the specific deployment is locked by us.
    // The manager's deployment loop may already hold the lock; we must wait for
    // it to release before proceeding. 2 minutes (60 × 2s) is sufficient because
    // the manager skips Pending/InitialSetup for push-mode deployments — if it
    // holds the lock, it checks push-mode + Pending and releases immediately.
    let session = format!("push-setup-{}", uuid::Uuid::new_v4());
    let acquired_deployment = acquire_setup_run_deployment(
        client,
        deployment_id,
        &session,
        stack_settings.deployment_model,
    )
    .await
    .context(ErrorData::DeploymentFailed {
        operation: "acquire sync lock".to_string(),
    })?;

    if let Some(acquired_config) = acquired_deployment.get("deploymentConfig").cloned() {
        config = serde_json::from_value(acquired_config)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize deploymentConfig from acquired deployment"
                    .to_string(),
            })?;

        if let Some(net_args) = network_args {
            let network_platform = base_platform.unwrap_or(platform);
            let network_override =
                network::parse_network_settings(net_args, network_platform.as_str()).map_err(
                    |e| {
                        AlienError::new(ErrorData::ValidationError {
                            field: "network".to_string(),
                            message: e,
                        })
                    },
                )?;
            if let Some(ns) = network_override {
                config.stack_settings.network = Some(ns);
            }
        }

        config.manager_url = Some(manager_base_url.to_string());
        config.deployment_token = Some(deployment_token.to_string());
        config.management_config = setup_management_config.clone();
        config.base_platform = base_platform.or(config.base_platform);
        let acquired_stack_settings = config.stack_settings.clone();
        apply_external_bindings_from_stack_settings(&mut config, &acquired_stack_settings);
    }

    // Re-fetch the deployment state now that we hold the lock.
    // The manager may have advanced the state while we were waiting.
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;

    state.status = status;
    state.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    state.runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    tracing::info!(
        has_runtime_metadata = state.runtime_metadata.is_some(),
        "push_initial_setup: state after re-fetch (before step loop)"
    );

    // Run the shared step loop with per-step reconciliation via the manager API
    let transport = ManagerApiTransport::new(client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        // Push model: run initial setup only, then hand off to the manager.
        // The CLI drives Pending → InitialSetup → Provisioning, then stops.
        // The manager picks up from Provisioning and drives to Running.
        operation: LoopOperation::InitialSetup,
        delay_threshold: None,
    };

    let runner_result = shared_run_step_loop(
        &mut state,
        &mut config,
        &client_config,
        deployment_id,
        &policy,
        &transport,
        None,
        on_progress.as_ref(),
    )
    .await;

    // Always reconcile + release, even on error.
    final_reconcile(client, deployment_id, &session, &state).await;
    release_deployment(client, deployment_id, &session).await;

    // Handle runner result after lock release
    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "initial setup".to_string(),
    })?;

    match result.loop_result.outcome {
        LoopOutcome::Success => {
            output::success("Deployment is running.");
            Ok(())
        }
        LoopOutcome::Failure => Err(AlienError::new(ErrorData::DeploymentFailed {
            operation: format!(
                "deployment failed at status {}",
                deployment_status_str(result.loop_result.final_status)
            ),
        })),
        LoopOutcome::Neutral => {
            if should_print_push_setup_neutral_completion(platform) {
                output::success(
                    "Setup complete. Your deployment is being provisioned and will be ready shortly.",
                );
            }
            Ok(())
        }
    }
}

pub(super) fn should_collect_push_setup_environment_info(platform: Platform) -> bool {
    !matches!(platform, Platform::Machines)
}

/// Run the push-model deletion flow for a deployment.
///
/// Fetches deployment and release state from the manager, acquires a sync lock,
/// steps the deployment through DeletePending → Deleting → Deleted (or DeleteFailed),
/// reconciles state back to the manager, and releases the lock.
pub async fn push_deletion(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    client_config: ClientConfig,
) -> Result<()> {
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;

    let stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    let environment_info = deployment
        .environment_info
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize environment_info from manager".to_string(),
        })?;
    let runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    let current_release = if let Some(ref release_id) = deployment.current_release_id {
        match client.get_release().id(release_id).send().await {
            Ok(resp) => {
                let rel = resp.into_inner();
                let platform_stack_value = release_stack_value_for_platform(rel.stack, platform);
                platform_stack_value
                    .and_then(|v| serde_json::from_value(v).ok())
                    .map(|stack| ReleaseInfo {
                        release_id: Some(rel.id),
                        version: None,
                        description: None,
                        stack,
                    })
            }
            Err(_) => None,
        }
    } else {
        None
    };

    let mut state = DeploymentState {
        status,
        platform,
        current_release: current_release.clone(),
        target_release: current_release,
        stack_state,
        error: None,
        environment_info,
        runtime_metadata,
        retry_requested: deployment.retry_requested,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };

    let stack_settings: StackSettings = deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_settings from manager".to_string(),
        })?
        .unwrap_or_default();

    let mut config: DeploymentConfig = serde_json::from_value(serde_json::json!({
        "stackSettings": serde_json::to_value(&stack_settings).unwrap_or_default(),
        "environmentVariables": {
            "variables": [],
            "hash": "",
            "createdAt": ""
        }
    }))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to construct deployment config".to_string(),
    })?;

    apply_external_bindings_from_stack_settings(&mut config, &stack_settings);
    let service_provider = runtime_service_provider(&client_config)?;

    if platform == Platform::Local
        && !matches!(
            state.status,
            DeploymentStatus::TeardownRequired | DeploymentStatus::TeardownFailed
        )
    {
        run_runtime_deletion(
            client,
            deployment_id,
            &mut state,
            &mut config,
            &client_config,
            stack_settings.deployment_model,
            service_provider.clone(),
        )
        .await?;

        if state.status == DeploymentStatus::Deleted {
            output::success("Deployment deleted successfully.");
            return Ok(());
        }
    }

    run_setup_deletion(
        client,
        deployment_id,
        &mut state,
        &mut config,
        &client_config,
        stack_settings.deployment_model,
        service_provider,
    )
    .await
}

fn runtime_service_provider(
    client_config: &ClientConfig,
) -> Result<Option<Arc<dyn alien_infra::PlatformServiceProvider>>> {
    let ClientConfig::Local { state_directory } = client_config else {
        return Ok(None);
    };

    let local_bindings = alien_local::LocalBindingsProvider::new(Path::new(state_directory))
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to create local runtime provider from '{}'",
                state_directory
            ),
        })?;

    Ok(Some(Arc::new(
        alien_infra::DefaultPlatformServiceProvider::with_local_bindings(local_bindings),
    )))
}

async fn run_runtime_deletion(
    client: &ServerClient,
    deployment_id: &str,
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_model: alien_core::DeploymentModel,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> Result<()> {
    let session = format!("push-runtime-deletion-{}", uuid::Uuid::new_v4());
    acquire_runtime_delete_deployment(client, deployment_id, &session, deployment_model)
        .await
        .context(ErrorData::DeploymentFailed {
            operation: "acquire runtime deletion lock".to_string(),
        })?;

    // Re-fetch deployment under lock
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;
    state.status = status;
    state.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    state.runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    let transport = ManagerApiTransport::new(client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_threshold: None,
    };

    let runner_result = shared_run_step_loop(
        state,
        config,
        client_config,
        deployment_id,
        &policy,
        &transport,
        service_provider,
        None,
    )
    .await;

    // Always reconcile + release, even on error
    final_reconcile(client, deployment_id, &session, state).await;
    release_deployment(client, deployment_id, &session).await;

    // Handle runner result after lock release
    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "deletion".to_string(),
    })?;

    match result.loop_result.outcome {
        LoopOutcome::Success => Ok(()),
        LoopOutcome::Failure => {
            let operation = format!(
                "deletion failed at status {}",
                deployment_status_str(result.loop_result.final_status)
            );
            if let Some(error) = state.error.clone() {
                Err(error.context(ErrorData::DeploymentFailed { operation }))
            } else {
                Err(AlienError::new(ErrorData::DeploymentFailed { operation }))
            }
        }
        LoopOutcome::Neutral => Ok(()),
    }
}

async fn run_setup_deletion(
    client: &ServerClient,
    deployment_id: &str,
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_model: alien_core::DeploymentModel,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> Result<()> {
    let session = format!("push-setup-deletion-{}", uuid::Uuid::new_v4());
    let acquire_outcome =
        acquire_setup_delete_deployment(client, deployment_id, &session, deployment_model)
            .await
            .context(ErrorData::DeploymentFailed {
                operation: "acquire setup teardown lock".to_string(),
            })?;

    if matches!(acquire_outcome, SetupDeleteAcquireOutcome::AlreadyDeleted) {
        output::success("Deployment deleted successfully.");
        return Ok(());
    }

    // Re-fetch deployment under lock
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;
    state.status = status;
    state.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    state.runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    let transport = ManagerApiTransport::new(client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_threshold: None,
    };

    let runner_result = alien_deployment::setup_teardown::run_setup_teardown_after_handoff(
        state,
        config,
        client_config,
        deployment_id,
        &policy,
        &transport,
        service_provider,
    )
    .await
    .map(|setup_result| {
        setup_result.unwrap_or_else(|| RunnerResult {
            loop_result: LoopResult {
                stop_reason: LoopStopReason::Synced,
                outcome: LoopOutcome::Neutral,
                final_status: state.status,
            },
            steps_executed: 0,
        })
    });

    // Always reconcile + release, even on error
    final_reconcile(client, deployment_id, &session, state).await;
    release_deployment(client, deployment_id, &session).await;

    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "setup teardown".to_string(),
    })?;

    match result.loop_result.outcome {
        LoopOutcome::Success => {
            output::success("Deployment deleted successfully.");
            Ok(())
        }
        LoopOutcome::Failure => {
            let operation = format!(
                "setup teardown failed at status {}",
                deployment_status_str(result.loop_result.final_status)
            );
            if let Some(error) = state.error.clone() {
                Err(error.context(ErrorData::DeploymentFailed { operation }))
            } else {
                Err(AlienError::new(ErrorData::DeploymentFailed { operation }))
            }
        }
        LoopOutcome::Neutral => Ok(()),
    }
}
