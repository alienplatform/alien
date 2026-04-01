//! Deployment loop - runs step() when updates are available
//!
//! This loop checks for pending updates in the local database and runs
//! alien-deployment::step() to deploy changes. Unlike the manager,
//! the Agent uses local credentials since it runs in the target environment.
//!
//! Storage model:
//! - `deployment_state` key: Full DeploymentState (includes target_release and current_release)
//! - `deployment_config` key: DeploymentConfig for the target deployment
//!
//! The loop runs steps continuously, respecting the suggested delay between each step,
//! until the deployment is synced (Running, Failed, or Deleted).

use crate::config::AgentConfig;
use crate::db::AgentDb;
use crate::AgentState;
use alien_core::{ClientConfig, DeploymentConfig, DeploymentState, KubernetesClientConfig, Platform};
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

/// Transport implementation that persists state to the agent's local DB
/// and re-reads config each step to pick up sync loop changes.
struct AgentTransport {
    db: Arc<AgentDb>,
    agent_config: AgentConfig,
    platform: Platform,
}

#[async_trait]
impl DeploymentLoopTransport for AgentTransport {
    async fn reconcile_step(
        &self,
        _deployment_id: &str,
        state: &DeploymentState,
        _step_error: Option<&AlienError>,
        _update_heartbeat: bool,
    ) -> std::result::Result<StepReconcileResult, AlienError> {
        // Persist state to local DB after each step
        self.db
            .set_deployment_state(state)
            .await
            .map_err(|e| e.into_generic())?;

        // Re-read config from DB (sync loop may have updated it)
        let config = self
            .db
            .get_deployment_config()
            .await
            .map_err(|e| e.into_generic())?;

        let enriched_config = match config {
            Some(config) => Some(enrich_config(
                config,
                &self.agent_config,
                self.platform,
                &self.db,
            ).await.map_err(|e| e.into_generic())?),
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
/// 3. Runs alien-deployment::runner::run_step_loop() with AgentTransport
/// 4. AgentTransport persists state and re-reads config after each step
/// 5. Sync loop will pick up changes and report to manager
pub async fn run_deployment_loop(state: Arc<AgentState>) {
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
async fn run_deployment_continuously(state: &AgentState) -> Result<usize> {
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

    // Check deployment approval status if required
    if state.config.requires_deployment_approval() {
        match state
            .db
            .get_approval_status_for_release(&target_release.release_id)
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
        release_id = %target_release.release_id,
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
    let mut enriched_config = enrich_config(
        base_config,
        &state.config,
        current.platform,
        &state.db,
    ).await?;

    // Resolve client config once (it doesn't change between steps)
    let client_config = resolve_client_config(
        current.platform,
        &state.config.data_dir,
        state.config.namespace.clone(),
    )
    .await?;

    let policy = RunnerPolicy {
        max_steps: 100,
        operation,
        delay_threshold: None,
    };

    let transport = AgentTransport {
        db: Arc::clone(&state.db),
        agent_config: state.config.clone(),
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
    )
    .await
    .context(ErrorData::DeploymentFailed {
        message: "Deployment step loop failed".to_string(),
    })?;

    let RunnerResult {
        loop_result,
        steps_executed,
    } = result;

    if loop_result.stop_reason != LoopStopReason::Handoff {
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

/// Enrich a deployment config with agent-specific settings.
///
/// Applies external_bindings, public_urls, stack_settings from agent config,
/// and injects commands polling env vars for K8s/Local platforms.
async fn enrich_config(
    mut config: DeploymentConfig,
    agent_config: &AgentConfig,
    platform: Platform,
    db: &AgentDb,
) -> Result<DeploymentConfig> {
    // Pass through external bindings from agent config (Kubernetes platform)
    if let Some(ref external_bindings) = agent_config.external_bindings {
        config.external_bindings = external_bindings.clone();
    }

    // Pass through public URLs from agent config
    if agent_config.public_urls.is_some() {
        config.public_urls = agent_config.public_urls.clone();
    }

    // Pass through stack settings from agent config
    if let Some(ref stack_settings) = agent_config.stack_settings {
        config.stack_settings = stack_settings.clone();
    }

    // Inject commands polling env vars only for K8s/Local containers.
    // Serverless functions (Lambda, Cloud Run, Container Apps) receive commands
    // via platform-native push (InvokeFunction, Pub/Sub, Service Bus) regardless
    // of the deployment model (push vs pull).
    let needs_polling = matches!(platform, Platform::Kubernetes | Platform::Local);

    if needs_polling {
        if let Some(ref sync_config) = agent_config.sync {
            use alien_core::{EnvironmentVariable, EnvironmentVariableType};

            let commands_url = match db.get_commands_url().await {
                Ok(Some(url)) => url,
                _ => format!("{}/v1", sync_config.url),
            };

            let mut vars = config.environment_variables.variables.clone();

            vars.extend([
                EnvironmentVariable {
                    name: "ALIEN_COMMANDS_POLLING_ENABLED".to_string(),
                    value: "true".to_string(),
                    var_type: EnvironmentVariableType::Plain,
                    target_resources: None,
                },
                EnvironmentVariable {
                    name: "ALIEN_COMMANDS_POLLING_URL".to_string(),
                    value: commands_url,
                    var_type: EnvironmentVariableType::Plain,
                    target_resources: None,
                },
                EnvironmentVariable {
                    name: "ALIEN_COMMANDS_TOKEN".to_string(),
                    value: sync_config.token.clone(),
                    var_type: EnvironmentVariableType::Secret,
                    target_resources: None,
                },
            ]);

            config.environment_variables.variables = vars;

            info!("Injected commands polling configuration for K8s/Local deployment");
        }
    }

    Ok(config)
}

/// Resolve client config based on platform
async fn resolve_client_config(
    platform: Platform,
    data_dir: &str,
    namespace: Option<String>,
) -> Result<ClientConfig> {
    match platform {
        Platform::Kubernetes => {
            Ok(ClientConfig::Kubernetes(Box::new(
                KubernetesClientConfig::InCluster {
                    namespace,
                    additional_headers: None,
                },
            )))
        }
        Platform::Local => {
            Ok(ClientConfig::Local {
                state_directory: data_dir.to_string(),
                artifact_registry_config: None,
            })
        }
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
