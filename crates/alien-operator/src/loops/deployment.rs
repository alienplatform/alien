//! Deployment loop - runs step() when updates are available
//!
//! This loop checks for pending updates in the local database and runs
//! alien-deployment::step() to deploy changes. Unlike the manager,
//! the Operator uses local credentials since it runs in the target environment.
//!
//! Storage model:
//! - `deployment_state` key: Full DeploymentState (includes target_release and current_release)
//! - `deployment_config` key: DeploymentConfig for the target deployment
//!
//! The loop runs steps continuously, respecting the suggested delay between each step,
//! until the deployment is synced (Running, Failed, or Deleted).

use crate::config::OperatorConfig;
use crate::db::OperatorDb;
use crate::OperatorState;
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, EnvironmentVariable,
    EnvironmentVariableType, KubernetesClientConfig, ObservedInventoryBatch, Platform,
    ResourceHeartbeat, ENV_ALIEN_DEPLOYMENT_ID, ENV_ALIEN_DEPLOYMENT_NAME,
};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome, LoopStopReason};
use alien_deployment::runner::{RunnerPolicy, RunnerResult};
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::{AlienError, Context};
use alien_infra::ClientConfigExt;
use async_trait::async_trait;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tracing::{debug, error, info};

use crate::error::{ErrorData, Result};

/// Transport implementation that persists state to the operator's local DB
/// and re-reads config each step to pick up sync loop changes.
struct OperatorTransport {
    db: Arc<OperatorDb>,
    operator_config: OperatorConfig,
    platform: Platform,
    suggested_delay_ms: AtomicU64,
}

#[async_trait]
impl DeploymentLoopTransport for OperatorTransport {
    async fn reconcile_step(
        &self,
        _deployment_id: &str,
        state: &DeploymentState,
        _config: &alien_core::DeploymentConfig,
        _update_heartbeat: bool,
        suggested_delay_ms: Option<u64>,
        heartbeats: Vec<ResourceHeartbeat>,
        observed_inventory_batches: Vec<ObservedInventoryBatch>,
    ) -> std::result::Result<StepReconcileResult, AlienError> {
        self.suggested_delay_ms
            .store(suggested_delay_ms.unwrap_or(0), Ordering::Relaxed);
        let previous = self
            .db
            .get_deployment_state()
            .await
            .map_err(|e| e.into_generic())?;
        let reports_changed = previous
            .as_ref()
            .and_then(|state| state.runtime_metadata.as_ref())
            .map(|metadata| &metadata.deployer_secrets)
            != state
                .runtime_metadata
                .as_ref()
                .map(|metadata| &metadata.deployer_secrets);
        // Persist state to local DB after each step
        self.db
            .set_deployment_state(state)
            .await
            .map_err(|e| e.into_generic())?;

        if !heartbeats.is_empty() {
            self.db
                .set_pending_heartbeats(&heartbeats)
                .await
                .map_err(|e| e.into_generic())?;
        }
        if !observed_inventory_batches.is_empty() {
            self.db
                .set_pending_observed_inventory_batches(&observed_inventory_batches)
                .await
                .map_err(|e| e.into_generic())?;
        }

        if reports_changed {
            self.db.request_sync();
        }

        // Re-read config from DB (sync loop may have updated it)
        let config = self
            .db
            .get_deployment_config()
            .await
            .map_err(|e| e.into_generic())?;

        let stack = state
            .target_release
            .as_ref()
            .or(state.current_release.as_ref())
            .map(|release| &release.stack);

        let enriched_config = match config {
            Some(config) => Some(
                enrich_config(
                    config,
                    &self.operator_config,
                    self.platform,
                    &self.db,
                    stack,
                )
                .await
                .map_err(|e| e.into_generic())?,
            ),
            None => None,
        };

        Ok(StepReconcileResult {
            state: None,
            config: enriched_config,
        })
    }
}

/// Run the deployment loop
///
/// This loop:
/// 1. Checks local database for pending updates (set by sync loop)
/// 2. Checks approval status if manual approval is required
/// 3. Runs alien-deployment::runner::run_step_loop() with OperatorTransport
/// 4. OperatorTransport persists state and re-reads config after each step
/// 5. Sync loop will pick up changes and report to manager
pub async fn run_deployment_loop(state: Arc<OperatorState>) {
    let interval = Duration::from_secs(state.config.deployment_interval_seconds);

    info!(
        interval_seconds = state.config.deployment_interval_seconds,
        "Starting deployment loop"
    );

    loop {
        let mut delay = interval;
        match run_deployment_continuously(&state).await {
            Ok((steps, suggested_delay)) => {
                delay = delay.max(suggested_delay);
                if steps > 0 {
                    info!(steps = steps, "Deployment completed");
                }
            }
            Err(e) => {
                error!(error = %e, "Deployment failed");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            _ = state.cancel.cancelled() => {
                info!("Deployment loop shutting down");
                return;
            }
        }
    }
}

/// Run deployment steps continuously until synced or long delay.
/// Returns the number of steps executed and the delay before resuming.
pub(crate) async fn run_deployment_continuously(
    state: &OperatorState,
) -> Result<(usize, Duration)> {
    // Get deployment state and config
    let mut current = match state.db.get_deployment_state().await? {
        Some(s) => s,
        None => return Ok((0, Duration::ZERO)),
    };

    // Check if there's a target to deploy
    let target_release = match &current.target_release {
        Some(r) => r,
        None => return Ok((0, Duration::ZERO)),
    };
    // A pushed target always carries an Alien release id (observe deployments never
    // receive a target), so an empty fallback is unreachable in practice.
    let target_release_id = target_release.release_id.as_deref().unwrap_or_default();

    // Check deployment approval status if required
    if state.config.requires_deployment_approval() {
        match state
            .db
            .get_approval_status_for_release(target_release_id)
            .await?
        {
            Some(crate::db::ApprovalStatus::Pending) => {
                debug!("Target release pending approval, skipping deployment");
                return Ok((0, Duration::ZERO));
            }
            Some(crate::db::ApprovalStatus::Rejected) => {
                info!("Target release was rejected, clearing target");
                current.target_release = None;
                state.db.set_deployment_state(&current).await?;
                state.db.clear_deployment_config().await?;
                return Ok((0, Duration::ZERO));
            }
            Some(crate::db::ApprovalStatus::Auto)
            | Some(crate::db::ApprovalStatus::Approved)
            | None => {
                // Proceed with deployment
            }
        }
    }

    debug!(
        release_id = %target_release_id,
        "Found target release to deploy"
    );

    let operation = if matches!(
        current.status,
        alien_core::DeploymentStatus::DeletePending
            | alien_core::DeploymentStatus::Deleting
            | alien_core::DeploymentStatus::DeleteFailed
    ) {
        LoopOperation::Delete
    } else {
        LoopOperation::Deploy
    };

    // Get deployment ID (used for logging in runner)
    let deployment_id = state
        .db
        .get_deployment_id()
        .await?
        .unwrap_or_else(|| "unknown".to_string());

    // Build initial enriched config
    let base_config = match state.db.get_deployment_config().await? {
        Some(c) => c,
        None => {
            debug!("No deployment config found, skipping");
            return Ok((0, Duration::ZERO));
        }
    };
    let mut enriched_config = enrich_config(
        base_config,
        &state.config,
        current.platform,
        &state.db,
        Some(&target_release.stack),
    )
    .await?;

    // Resolve client config once (it doesn't change between steps)
    let client_config = resolve_client_config(
        current.platform,
        enriched_config.base_platform.or(state.config.base_platform),
        &state.config.data_dir,
        state.config.namespace.clone(),
        state.config.sync.as_ref(),
    )
    .await?;

    let policy = RunnerPolicy {
        max_steps: 100,
        operation,
        delay_strategy: alien_deployment::runner::DelayStrategy::Yield,
    };

    let transport = OperatorTransport {
        db: Arc::clone(&state.db),
        operator_config: state.config.clone(),
        platform: current.platform,
        suggested_delay_ms: AtomicU64::new(0),
    };

    // A target that left the deployment Running or RefreshFailed is maintenance of
    // the installed release (see `sync::accept_target`). Run one Running refresh
    // step, as the manager does for each maintenance claim of a push deployment:
    // every Ready handler runs, so health checks and in-place repairs such as a
    // linked-binding refresh happen. A repair a handler starts continues on the
    // next maintenance claim.
    let refresh = operation == LoopOperation::Deploy
        && matches!(
            current.status,
            DeploymentStatus::Running | DeploymentStatus::RefreshFailed
        );
    if refresh {
        let steps = run_maintenance_refresh(
            state,
            &mut current,
            &mut enriched_config,
            &client_config,
            &deployment_id,
            &policy,
            &transport,
        )
        .await?;
        finish_maintenance_refresh(state, &current).await?;
        return Ok((steps, Duration::ZERO));
    }

    // The claim whose target this loop applies. Captured before stepping: the
    // sync loop replaces the durable claim once it reports this apply complete.
    let applying_operation_id = state
        .db
        .get_sync_execution()
        .await?
        .and_then(|execution| execution.claim)
        .map(|claim| claim.operation_id);

    let result = alien_deployment::runner::run_step_loop(
        &mut current,
        &mut enriched_config,
        &client_config,
        &deployment_id,
        &policy,
        &transport,
        state.service_provider.clone(),
        None,
    )
    .await
    .context(ErrorData::DeploymentFailed {
        message: "Deployment step loop failed".to_string(),
    })?;

    let RunnerResult {
        loop_result,
        steps_executed,
        ..
    } = result;

    if loop_result.outcome == LoopOutcome::Neutral {
        debug!(
            status = ?loop_result.final_status,
            stop_reason = ?loop_result.stop_reason,
            steps = steps_executed,
            "Deployment step loop yielded"
        );
    } else if loop_result.stop_reason != LoopStopReason::Handoff {
        if loop_result.outcome == LoopOutcome::Success {
            if operation == LoopOperation::Deploy
                && loop_result.final_status == DeploymentStatus::Running
            {
                if let Some(operation_id) = applying_operation_id.as_deref() {
                    state.db.set_applied_operation_id(operation_id).await?;
                }
            }
            debug!("Deployment synced, clearing deployment config");
            state.db.clear_deployment_config().await?;
        }

        info!(
            status = ?loop_result.final_status,
            stop_reason = ?loop_result.stop_reason,
            outcome = ?loop_result.outcome,
            steps = steps_executed,
            "Deployment reached terminal state"
        );
    }

    // Only waits on the deployer (keys, machines) honor the step's delay; resource
    // waits during setup or updates keep the loop's short polling interval.
    let waiting_on_deployer = matches!(
        loop_result.final_status,
        alien_core::DeploymentStatus::WaitingForSecrets
            | alien_core::DeploymentStatus::WaitingForMachines
    );
    let delay = if loop_result.stop_reason == LoopStopReason::Delayed && waiting_on_deployer {
        Duration::from_millis(transport.suggested_delay_ms.load(Ordering::Relaxed))
    } else {
        Duration::ZERO
    };
    Ok((steps_executed, delay))
}

/// One Running refresh pass over the installed release.
async fn run_maintenance_refresh(
    state: &OperatorState,
    current: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    policy: &RunnerPolicy,
    transport: &OperatorTransport,
) -> Result<usize> {
    let result = alien_deployment::runner::run_running_refresh_step_loop(
        current,
        config,
        client_config,
        deployment_id,
        policy,
        transport,
        state.service_provider.clone(),
        None,
    )
    .await
    .context(ErrorData::DeploymentFailed {
        message: "Maintenance refresh failed".to_string(),
    })?;
    debug!(
        status = ?result.loop_result.final_status,
        steps = result.steps_executed,
        "Maintenance refresh step completed"
    );
    Ok(result.steps_executed)
}

/// Consume a finished maintenance target: the deployment is settled on its
/// installed release again, so passive health reporting resumes and the next
/// tick does not refresh again. Sync right away, so the claim completes with
/// this refresh's result. A state the sync loop changed meanwhile (for example
/// a newer target) is left, with its config, for the next tick.
async fn finish_maintenance_refresh(
    state: &OperatorState,
    refreshed: &DeploymentState,
) -> Result<()> {
    let mut settled = refreshed.clone();
    settled.target_release = None;
    if state
        .db
        .finish_target_if_unchanged(refreshed, &settled)
        .await?
    {
        state.db.request_sync();
    }
    Ok(())
}

/// Enrich a deployment config with operator-specific settings.
///
/// Applies public_endpoints and stack_settings from operator config,
/// and injects command delivery env vars for K8s/Local platforms.
/// External bindings also populate DeploymentConfig for executor preflights.
async fn enrich_config(
    mut config: DeploymentConfig,
    operator_config: &OperatorConfig,
    platform: Platform,
    db: &OperatorDb,
    stack: Option<&alien_core::Stack>,
) -> Result<DeploymentConfig> {
    // Pass through public endpoints from operator config.
    if operator_config.public_endpoints.is_some() {
        config.public_endpoints = operator_config.public_endpoints.clone();
    }

    // Installation-provided bindings take precedence over manager-provided ones.
    if let Some(ref stack_settings) = operator_config.stack_settings {
        if let Some(ref bindings) = stack_settings.external_bindings {
            for (resource_id, binding) in &bindings.0 {
                config
                    .external_bindings
                    .insert(resource_id.clone(), binding.clone());
            }
        }
        config.stack_settings = stack_settings.clone();
    }
    if config.base_platform.is_none() {
        config.base_platform = operator_config.base_platform;
    }
    if config.label_domain.is_none() {
        config.label_domain = operator_config.label_domain.clone();
    }
    if config.deployment_name.is_none() {
        config.deployment_name = operator_config.agent_name.clone();
    }
    if config.observe_label_selector.is_none() {
        config.observe_label_selector = operator_config.label_selector.clone();
    }
    config.observe_all_namespaces = operator_config.observe_all_namespaces;

    // Local/Kubernetes Workers expose an authenticated runtime-owned push
    // endpoint. Cloud-native Worker transports authenticate through their
    // platform instead.
    let needs_http_push_auth = matches!(platform, Platform::Kubernetes | Platform::Local);

    if needs_http_push_auth {
        if let Some(ref sync_config) = operator_config.sync {
            let commands_url = match db.get_commands_url().await {
                Ok(Some(url)) => url,
                _ => format!("{}/v1", sync_config.url.as_str().trim_end_matches('/')),
            };

            let mut vars = config.environment_variables.variables.clone();

            // The Manager authenticates command delivery with the deployment's
            // sync token. The Worker runtime keeps it out of the application
            // child environment; Container/Daemon receivers necessarily read it.
            if let Some(stack) = stack {
                vars.extend(
                    stack
                        .worker_command_push_env_vars(Some(&sync_config.token))
                        .context(ErrorData::ConfigurationError {
                            message: "Failed to build Worker command push auth env".to_string(),
                        })?,
                );
                // Container/Daemon command receiver env (ALIEN_COMMANDS_URL/TOKEN/
                // TARGET_RESOURCE_ID/TARGET_RESOURCE_TYPE), scoped per resource.
                // Container/Daemon always deliver via Pull; the operator
                // only ever manages K8s/Local, so this block already covers every
                // platform it serves. It uses the same deployment-token auth
                // contract as Worker push handling above.
                vars.extend(
                    stack
                        .receiver_command_env_vars(&commands_url, Some(&sync_config.token))
                        .context(ErrorData::ConfigurationError {
                            message: "Failed to build command receiver env".to_string(),
                        })?,
                );
            }

            // Ensure ALIEN_DEPLOYMENT_ID is present (should come from manager config,
            // but add defensively in case it's missing)
            if !vars.iter().any(|v| v.name == ENV_ALIEN_DEPLOYMENT_ID) {
                if let Ok(Some(dep_id)) = db.get_deployment_id().await {
                    vars.push(EnvironmentVariable {
                        name: ENV_ALIEN_DEPLOYMENT_ID.to_string(),
                        value: dep_id,
                        var_type: EnvironmentVariableType::Plain,
                        target_resources: None,
                    });
                }
            }

            if !vars.iter().any(|v| v.name == ENV_ALIEN_DEPLOYMENT_NAME) {
                if let Some(name) = config.deployment_name.as_ref() {
                    vars.push(EnvironmentVariable {
                        name: ENV_ALIEN_DEPLOYMENT_NAME.to_string(),
                        value: name.clone(),
                        var_type: EnvironmentVariableType::Plain,
                        target_resources: None,
                    });
                }
            }

            config.environment_variables.variables = vars;

            info!("Injected command delivery configuration for K8s/Local deployment");
        }
    }

    // Image pull credentials are no longer needed here — pull-model operators
    // pull images through the manager's /v2/ registry proxy, which handles
    // upstream authentication using the manager's own credentials.

    Ok(config)
}

/// Resolve client config based on platform
pub(super) async fn resolve_client_config(
    platform: Platform,
    base_platform: Option<Platform>,
    data_dir: &str,
    namespace: Option<String>,
    _sync_config: Option<&crate::config::SyncConfig>,
) -> Result<ClientConfig> {
    match platform {
        Platform::Kubernetes => {
            let kubernetes = KubernetesClientConfig::InCluster {
                    namespace,
                    additional_headers: None,
            };
            if let Some(base_platform) = base_platform {
                let cloud = ClientConfig::from_std_env(base_platform)
                    .await
                    .context(ErrorData::ConfigurationError {
                        message: format!(
                            "Failed to create {} base client config for Kubernetes deployment",
                            base_platform
                        ),
                    })?;
                Ok(ClientConfig::KubernetesCloud {
                    kubernetes: Box::new(kubernetes),
                    cloud: Box::new(cloud),
                })
            } else {
                Ok(ClientConfig::Kubernetes(Box::new(kubernetes)))
            }
        }
        Platform::Local => {
            // No artifact_registry_config needed — the deployment token for proxy
            // pull auth flows through DeploymentConfig.deployment_token (set by
            // the sync handler from the operator's Bearer token).
            Ok(ClientConfig::Local {
                state_directory: data_dir.to_string(),
            })
        }
        Platform::Machines => Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Machines deployments are reconciled by the manager, not by a local operator"
                .to_string(),
        })),
        Platform::Test => Ok(ClientConfig::Test),
        Platform::Aws | Platform::Gcp | Platform::Azure => {
            ClientConfig::from_std_env(platform)
                .await
                .context(ErrorData::ConfigurationError {
                    message: format!(
                        "Failed to create {} client config from environment. Ensure the required environment variables are set.",
                        platform
                    ),
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{OperatorConfig, SyncConfig};
    use crate::db::OperatorDb;
    use alien_core::{
        DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBinding, ExternalBindings,
        StackSettings, StorageBinding,
    };
    use std::collections::HashMap;

    fn test_deployment_config() -> DeploymentConfig {
        DeploymentConfig {
            gcp_federation_scope: None,
            stored_secret_input_ids: None,
            input_values: Default::default(),
            deployment_name: None,
            stack_settings: StackSettings::default(),
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            },
            allow_frozen_changes: false,
            setup_run: false,
            compute_backend: None,
            external_bindings: ExternalBindings::default(),
            base_platform: None,
            label_domain: None,
            observe_label_selector: None,
            observe_all_namespaces: false,
            public_endpoints: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
            volume_restores: Vec::new(),
        }
    }

    #[tokio::test]
    async fn secret_wait_yields_for_the_requested_delay_and_wakes_sync_only_on_change() {
        use alien_bindings::{BindingsProvider, BindingsProviderApi};
        use alien_core::{
            DeploymentStatus, Resource, ResourceLifecycle, ResourceStatus, RuntimeMetadata, Stack,
            StackResourceState, StackState, Vault, VaultBinding,
        };
        let directory = tempfile::tempdir().unwrap();
        let data_dir = directory.path().to_str().unwrap();
        let db = Arc::new(
            OperatorDb::new(
                data_dir,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            )
            .await
            .unwrap(),
        );
        let mut stack = Stack::new("secret-test".into()).build();
        stack.inputs = vec![serde_json::from_value(serde_json::json!({
            "id": "key", "kind": "secret", "providedBy": ["deployer"],
            "required": true, "label": "Test key", "description": ""
        }))
        .unwrap()];
        let mut vault = StackResourceState::new_pending(
            "vault".into(),
            Resource::new(Vault::new("deployer-secrets".into()).build()),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        );
        vault.status = ResourceStatus::Running;
        vault.remote_binding_params =
            Some(serde_json::to_value(VaultBinding::local("deployer-secrets", data_dir)).unwrap());
        let mut stack_state = StackState::new(Platform::Local);
        stack_state
            .resources
            .insert("deployer-secrets".into(), vault);
        let provider = BindingsProvider::from_stack_state(
            &stack_state,
            ClientConfig::Local {
                state_directory: data_dir.into(),
            },
        )
        .unwrap();
        let vault = provider.load_vault("deployer-secrets").await.unwrap();
        let release = alien_core::ReleaseInfo {
            release_id: Some("rel_test".into()),
            version: None,
            description: None,
            stack: stack.clone(),
        };
        let current = DeploymentState {
            platform: Platform::Local,
            status: DeploymentStatus::WaitingForSecrets,
            current_release: None,
            target_release: Some(release.clone()),
            stack_state: Some(stack_state),
            runtime_metadata: Some(RuntimeMetadata {
                prepared_stack: Some(stack),
                ..Default::default()
            }),
            error: None,
            environment_info: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        };
        db.set_deployment_state(&current).await.unwrap();
        db.set_deployment_config(&test_deployment_config())
            .await
            .unwrap();
        let state = OperatorState {
            config: OperatorConfig::builder()
                .platform(Platform::Local)
                .data_dir(data_dir)
                .encryption_key("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .build(),
            db,
            service_provider: None,
            operations_sync_handler: None,
            cancel: tokio_util::sync::CancellationToken::new(),
        };
        let (steps, delay) = run_deployment_continuously(&state).await.unwrap();
        assert_eq!(steps, 1);
        assert_eq!(delay, Duration::from_secs(30));
        tokio::time::timeout(Duration::from_secs(1), state.db.wait_for_sync())
            .await
            .unwrap();
        let (_, delay) = run_deployment_continuously(&state).await.unwrap();
        assert_eq!(delay, Duration::from_secs(30));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), state.db.wait_for_sync())
                .await
                .is_err()
        );

        // A completed deployment drops its installation config. Refresh still
        // observes a changed slot without starting any resource controllers.
        let mut running = state.db.get_deployment_state().await.unwrap().unwrap();
        running.status = DeploymentStatus::Running;
        running.error = None;
        running.target_release = None;
        running.current_release = Some(release);
        // An input supplied at install time must not become a new deployer slot.
        let metadata = running.runtime_metadata.as_mut().unwrap();
        let mut supplied = metadata.prepared_stack.as_ref().unwrap().inputs[0].clone();
        supplied.id = "supplied".into();
        supplied
            .provided_by
            .push(alien_core::StackInputProvider::Developer);
        metadata
            .prepared_stack
            .as_mut()
            .unwrap()
            .inputs
            .push(supplied);
        // Read-back can reorder resource HashMaps without changing the checkpoint.
        let resources = &mut running.stack_state.as_mut().unwrap().resources;
        let vault_resource = resources["deployer-secrets"].clone();
        for index in 0..32 {
            resources.insert(format!("unchanged-{index}"), vault_resource.clone());
        }
        state.db.set_deployment_state(&running).await.unwrap();
        running = state.db.get_deployment_state().await.unwrap().unwrap();
        state.db.clear_deployment_config().await.unwrap();
        vault
            .set_secret("key", "fake-test-value-1566")
            .await
            .unwrap();
        super::super::sync::refresh_running_secrets(&state, &mut running)
            .await
            .unwrap();
        let reports = &running.runtime_metadata.as_ref().unwrap().deployer_secrets;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].status, alien_core::DeployerSecretStatus::Present);
        let stale = running.clone();
        vault.delete_secret("key").await.unwrap();
        super::super::sync::refresh_running_secrets(&state, &mut running)
            .await
            .unwrap();
        assert_eq!(
            running.runtime_metadata.as_ref().unwrap().deployer_secrets[0].status,
            alien_core::DeployerSecretStatus::Missing
        );
        let persisted = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(persisted.runtime_metadata, running.runtime_metadata);
        // An older observation cannot overwrite a newer deployment checkpoint.
        assert!(!state
            .db
            .compare_and_set_deployment_state(&stale, &current)
            .await
            .unwrap());
        assert_eq!(
            state
                .db
                .get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .status,
            DeploymentStatus::Running
        );
    }

    #[tokio::test]
    async fn enrich_config_passes_operator_bindings_to_executor() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "data",
            ExternalBinding::Storage(StorageBinding::s3("existing-bucket")),
        );
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .stack_settings(StackSettings {
                external_bindings: Some(bindings.clone()),
                ..StackSettings::default()
            })
            .encryption_key(encryption_key)
            .build();

        let enriched = enrich_config(
            test_deployment_config(),
            &operator_config,
            Platform::Kubernetes,
            &db,
            None,
        )
        .await
        .unwrap();

        assert_eq!(enriched.external_bindings, bindings);
    }

    #[tokio::test]
    async fn enrich_config_preserves_manager_bindings_not_overridden_by_installation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        let mut manager_bindings = ExternalBindings::new();
        manager_bindings.insert(
            "manager-only",
            ExternalBinding::Storage(StorageBinding::s3("manager-bucket")),
        );
        manager_bindings.insert(
            "shared",
            ExternalBinding::Storage(StorageBinding::s3("old-bucket")),
        );
        let mut installation_bindings = ExternalBindings::new();
        installation_bindings.insert(
            "shared",
            ExternalBinding::Storage(StorageBinding::s3("new-bucket")),
        );
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .stack_settings(StackSettings {
                external_bindings: Some(installation_bindings.clone()),
                ..StackSettings::default()
            })
            .encryption_key(encryption_key)
            .build();
        let mut config = test_deployment_config();
        config.external_bindings = manager_bindings;

        let enriched = enrich_config(config, &operator_config, Platform::Kubernetes, &db, None)
            .await
            .unwrap();

        assert!(enriched.external_bindings.has("manager-only"));
        assert_eq!(
            enriched.external_bindings.get("shared"),
            installation_bindings.get("shared")
        );
    }

    #[tokio::test]
    async fn enrich_config_uses_operator_name_for_runtime_deployment_name() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        db.set_deployment_id("dep_local").await.unwrap();

        let config = test_deployment_config();
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Local)
            .agent_name("local-runner")
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_dep_test".to_string(),
            }))
            .encryption_key(encryption_key)
            .build();

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db, None)
            .await
            .unwrap();

        assert_eq!(enriched.deployment_name.as_deref(), Some("local-runner"));
        assert!(enriched
            .environment_variables
            .variables
            .iter()
            .any(|var| { var.name == ENV_ALIEN_DEPLOYMENT_NAME && var.value == "local-runner" }));
        assert!(enriched
            .environment_variables
            .variables
            .iter()
            .any(|var| var.name == ENV_ALIEN_DEPLOYMENT_ID && var.value == "dep_local"));
    }

    #[tokio::test]
    async fn enrich_config_applies_operator_public_endpoints() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();

        let config = test_deployment_config();
        let public_endpoints = HashMap::from([(
            "gateway".to_string(),
            HashMap::from([(
                "api".to_string(),
                "https://api.gateway.example.test".to_string(),
            )]),
        )]);
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Local)
            .agent_name("local-runner")
            .maybe_public_endpoints(Some(public_endpoints.clone()))
            .encryption_key(encryption_key)
            .build();

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db, None)
            .await
            .unwrap();

        assert_eq!(enriched.public_endpoints, Some(public_endpoints));
    }

    #[tokio::test]
    async fn enrich_config_scopes_push_token_per_command_enabled_worker() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        db.set_deployment_id("dep_local").await.unwrap();

        let config = test_deployment_config();
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Local)
            .agent_name("local-runner")
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_dep_test".to_string(),
            }))
            .encryption_key(encryption_key)
            .build();

        let worker_a = alien_core::Worker::new("worker-a".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        let worker_b = alien_core::Worker::new("worker-b".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        // Commands-disabled Worker must not expose a command push endpoint.
        let worker_off = alien_core::Worker::new("worker-off".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        let stack = alien_core::Stack::new("operator-command-target-stack".to_string())
            .add(worker_a, alien_core::ResourceLifecycle::Live)
            .add(worker_b, alien_core::ResourceLifecycle::Live)
            .add(worker_off, alien_core::ResourceLifecycle::Live)
            .build();

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db, Some(&stack))
            .await
            .unwrap();

        let push_vars: Vec<_> = enriched
            .environment_variables
            .variables
            .iter()
            .filter(|var| var.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN)
            .collect();

        // Every push token is scoped to exactly one command-enabled Worker —
        // nothing deployment-wide, nothing scoped to the disabled Worker.
        assert!(push_vars.iter().all(|var| {
            var.target_resources == Some(vec!["worker-a".to_string()])
                || var.target_resources == Some(vec!["worker-b".to_string()])
        }));

        // One token per command-enabled Worker.
        for worker_id in ["worker-a", "worker-b"] {
            let scoped: Vec<_> = push_vars
                .iter()
                .filter(|var| var.target_resources == Some(vec![worker_id.to_string()]))
                .collect();
            assert_eq!(scoped.len(), 1, "expected one token for {worker_id}");
            assert!(scoped.iter().any(|var| {
                var.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN && var.value == "ax_dep_test"
            }));
        }
    }

    #[tokio::test]
    async fn enrich_config_scopes_receiver_env_per_command_enabled_container_and_daemon() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        db.set_deployment_id("dep_local").await.unwrap();

        let config = test_deployment_config();
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Local)
            .agent_name("local-runner")
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_dep_test".to_string(),
            }))
            .encryption_key(encryption_key)
            .build();

        let container = alien_core::Container::new("container-a".to_string())
            .code(alien_core::ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .commands_enabled(true)
            .build();
        let daemon = alien_core::Daemon::new("daemon-b".to_string())
            .code(alien_core::DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();
        let stack = alien_core::Stack::new("operator-receiver-stack".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .add(daemon, alien_core::ResourceLifecycle::Live)
            .build();

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db, Some(&stack))
            .await
            .unwrap();

        let receiver_names = [
            alien_core::ENV_ALIEN_COMMANDS_URL,
            alien_core::ENV_ALIEN_COMMANDS_TOKEN,
            alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
            alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
        ];
        let receiver_vars: Vec<_> = enriched
            .environment_variables
            .variables
            .iter()
            .filter(|var| receiver_names.contains(&var.name.as_str()))
            .collect();

        assert!(receiver_vars.iter().all(|var| {
            var.target_resources == Some(vec!["container-a".to_string()])
                || var.target_resources == Some(vec!["daemon-b".to_string()])
        }));

        for (resource_id, expected_type) in [("container-a", "container"), ("daemon-b", "daemon")] {
            let scoped: Vec<_> = receiver_vars
                .iter()
                .filter(|var| var.target_resources == Some(vec![resource_id.to_string()]))
                .collect();
            assert_eq!(
                scoped.len(),
                4,
                "expected 4 receiver vars for {resource_id}"
            );
            // Same normalized commands URL the operator derives for receivers.
            assert!(scoped
                .iter()
                .any(|var| var.name == alien_core::ENV_ALIEN_COMMANDS_URL
                    && var.value == "https://manager.example.com/v1"));
            assert!(scoped.iter().any(|var| {
                var.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN && var.value == "ax_dep_test"
            }));
            assert!(scoped.iter().any(|var| {
                var.name == alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE
                    && var.value == expected_type
            }));
            assert!(scoped.iter().any(|var| {
                var.name == alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID
                    && var.value == resource_id
            }));
        }
    }
}
