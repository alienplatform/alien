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
    ClientConfig, DeploymentConfig, DeploymentState, EnvironmentVariable, EnvironmentVariableType,
    KubernetesClientConfig, ObservedInventoryBatch, Platform, ResourceHeartbeat,
    ENV_ALIEN_COMMANDS_POLLING_ENABLED, ENV_ALIEN_COMMANDS_POLLING_URL, ENV_ALIEN_COMMANDS_TOKEN,
    ENV_ALIEN_DEPLOYMENT_ID, ENV_ALIEN_DEPLOYMENT_NAME,
};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome, LoopStopReason};
use alien_deployment::runner::{RunnerPolicy, RunnerResult};
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::{AlienError, Context};
use alien_infra::ClientConfigExt;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::error::{ErrorData, Result};

const SUGGESTED_DELAY_YIELD_THRESHOLD: Duration = Duration::from_millis(500);

/// Transport implementation that persists state to the operator's local DB
/// and re-reads config each step to pick up sync loop changes.
struct OperatorTransport {
    db: Arc<OperatorDb>,
    operator_config: OperatorConfig,
    platform: Platform,
}

#[async_trait]
impl DeploymentLoopTransport for OperatorTransport {
    async fn reconcile_step(
        &self,
        _deployment_id: &str,
        state: &DeploymentState,
        _config: &alien_core::DeploymentConfig,
        _update_heartbeat: bool,
        _suggested_delay_ms: Option<u64>,
        heartbeats: Vec<ResourceHeartbeat>,
        observed_inventory_batches: Vec<ObservedInventoryBatch>,
    ) -> std::result::Result<StepReconcileResult, AlienError> {
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

        // Re-read config from DB (sync loop may have updated it)
        let config = self
            .db
            .get_deployment_config()
            .await
            .map_err(|e| e.into_generic())?;

        let enriched_config = match config {
            Some(config) => Some(
                enrich_config(config, &self.operator_config, self.platform, &self.db)
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
        match run_deployment_continuously(&state).await {
            Ok(steps) => {
                if steps > 0 {
                    info!(steps = steps, "Deployment completed");
                }
            }
            Err(e) => {
                error!(error = %e, "Deployment failed");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {},
            _ = state.cancel.cancelled() => {
                info!("Deployment loop shutting down");
                return;
            }
        }
    }
}

/// Run deployment steps continuously until synced or long delay.
/// Returns the number of steps executed.
async fn run_deployment_continuously(state: &OperatorState) -> Result<usize> {
    // Get deployment state and config
    let mut current = match state.db.get_deployment_state().await? {
        Some(s) => s,
        None => return Ok(0),
    };

    // Check if there's a target to deploy
    let target_release = match &current.target_release {
        Some(r) => r,
        None => return Ok(0),
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
                return Ok(0);
            }
            Some(crate::db::ApprovalStatus::Rejected) => {
                info!("Target release was rejected, clearing target");
                current.target_release = None;
                state.db.set_deployment_state(&current).await?;
                state.db.clear_deployment_config().await?;
                return Ok(0);
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
            return Ok(0);
        }
    };
    let mut enriched_config =
        enrich_config(base_config, &state.config, current.platform, &state.db).await?;

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
        delay_threshold: Some(SUGGESTED_DELAY_YIELD_THRESHOLD),
    };

    let transport = OperatorTransport {
        db: Arc::clone(&state.db),
        operator_config: state.config.clone(),
        platform: current.platform,
    };

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

    Ok(steps_executed)
}

/// Enrich a deployment config with operator-specific settings.
///
/// Applies public_endpoints and stack_settings from operator config,
/// and injects commands polling env vars for K8s/Local platforms.
/// External bindings live on a separate DeploymentConfig field that the
/// StackExecutor reads, so they do NOT flow through `stack_settings` alone —
/// the copy must be explicit (see also alien-deploy-cli's `up` command).
async fn enrich_config(
    mut config: DeploymentConfig,
    operator_config: &OperatorConfig,
    platform: Platform,
    db: &OperatorDb,
) -> Result<DeploymentConfig> {
    // Pass through public endpoints from operator config.
    if operator_config.public_endpoints.is_some() {
        config.public_endpoints = operator_config.public_endpoints.clone();
    }

    // Pass through stack settings from operator config (includes external_bindings)
    if let Some(ref stack_settings) = operator_config.stack_settings {
        config.stack_settings = stack_settings.clone();
        // external_bindings does NOT flow through stack_settings automatically:
        // the StackExecutor reads DeploymentConfig::external_bindings, a separate
        // field. Copy it explicitly, mirroring alien-deploy-cli's `up` command.
        if let Some(ref ext_bindings) = stack_settings.external_bindings {
            config.external_bindings = ext_bindings.clone();
        }
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

    // Inject commands polling env vars only for K8s/Local containers.
    // Serverless functions (Lambda, Cloud Run, Container Apps) receive commands
    // via platform-native push (InvokeFunction, Pub/Sub, Service Bus) regardless
    // of the deployment model (push vs pull).
    let needs_polling = matches!(platform, Platform::Kubernetes | Platform::Local);

    if needs_polling {
        if let Some(ref sync_config) = operator_config.sync {
            let commands_url = match db.get_commands_url().await {
                Ok(Some(url)) => url,
                _ => format!("{}/v1", sync_config.url),
            };

            let mut vars = config.environment_variables.variables.clone();

            vars.extend([
                EnvironmentVariable {
                    name: ENV_ALIEN_COMMANDS_POLLING_ENABLED.to_string(),
                    value: "true".to_string(),
                    var_type: EnvironmentVariableType::Plain,
                    target_resources: None,
                },
                EnvironmentVariable {
                    name: ENV_ALIEN_COMMANDS_POLLING_URL.to_string(),
                    value: commands_url,
                    var_type: EnvironmentVariableType::Plain,
                    target_resources: None,
                },
                // SECURITY: The sync token is reused as the commands polling token.
                // This means deployed application code has access to the operator's sync token.
                // TODO: Issue a separate, scoped commands-only token during initialization
                // to limit the blast radius if the application is compromised.
                // See: security/04-CRITICAL-sync-token-reused-as-commands-token.md
                EnvironmentVariable {
                    name: ENV_ALIEN_COMMANDS_TOKEN.to_string(),
                    value: sync_config.token.clone(),
                    var_type: EnvironmentVariableType::Secret,
                    target_resources: None,
                },
            ]);

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

            info!("Injected commands polling configuration for K8s/Local deployment");
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
        DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, StackSettings,
    };
    use std::collections::HashMap;

    fn test_deployment_config() -> DeploymentConfig {
        DeploymentConfig {
            deployment_name: None,
            stack_settings: StackSettings::default(),
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            },
            allow_frozen_changes: false,
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
        }
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

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db)
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

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db)
            .await
            .unwrap();

        assert_eq!(enriched.public_endpoints, Some(public_endpoints));
    }
}
