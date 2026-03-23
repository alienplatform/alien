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

use crate::AgentState;
use alien_core::{ClientConfig, DeploymentStatus, KubernetesClientConfig, Platform};
use alien_error::Context;
use alien_infra::ClientConfigExt;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::error::{ErrorData, Result};

/// Run the deployment loop
///
/// This loop:
/// 1. Checks local database for pending updates (set by sync loop)
/// 2. Checks approval status if manual approval is required
/// 3. Runs alien-deployment::step() continuously until synced, respecting suggested delays
/// 4. Updates local state in database after each step
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

    let mut step_count = 0;
    const MAX_STEPS_PER_ITERATION: usize = 100;

    loop {
        if step_count >= MAX_STEPS_PER_ITERATION {
            debug!(steps = step_count, "Max steps per iteration reached");
            break;
        }

        // Re-read config each step so domain_metadata, env vars, etc. stay current.
        let config = match state.db.get_deployment_config().await? {
            Some(c) => c,
            None => {
                debug!("Deployment config cleared, stopping deployment loop");
                break;
            }
        };

        info!(
            status = ?current.status,
            platform = ?current.platform,
            step = step_count + 1,
            "Running deployment step"
        );

        // Get client config from environment
        let client_config = resolve_client_config(
            current.platform,
            &state.config.data_dir,
            state.config.namespace.clone(),
        )
        .await?;

        // Enrich deployment config with agent environment configuration
        let mut enriched_config = config;

        // Pass through external bindings from agent config (Kubernetes platform)
        if let Some(ref external_bindings) = state.config.external_bindings {
            enriched_config.external_bindings = external_bindings.clone();
        }

        // Pass through public URLs from agent config
        if state.config.public_urls.is_some() {
            enriched_config.public_urls = state.config.public_urls.clone();
        }

        // Pass through stack settings from agent config
        if let Some(ref stack_settings) = state.config.stack_settings {
            enriched_config.stack_settings = stack_settings.clone();
        }

        // Inject commands polling configuration from agent's sync token
        if let Some(ref sync_config) = state.config.sync {
            use alien_core::{EnvironmentVariable, EnvironmentVariableType};

            let mut vars = enriched_config.environment_variables.variables.clone();

            vars.extend([
                EnvironmentVariable {
                    name: "ALIEN_COMMANDS_POLLING_ENABLED".to_string(),
                    value: "true".to_string(),
                    var_type: EnvironmentVariableType::Secret,
                    target_resources: None,
                },
                EnvironmentVariable {
                    name: "ALIEN_COMMANDS_POLLING_URL".to_string(),
                    value: format!("{}/v1", sync_config.url),
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

            enriched_config.environment_variables.variables = vars;

            info!("Injected commands polling configuration with agent sync token");
        }

        // Execute the deployment step
        let step_result = alien_deployment::step(
            current.clone(),
            enriched_config,
            client_config,
            state.service_provider.clone(),
        )
        .await
        .context(ErrorData::DeploymentFailed {
            message: "Deployment step failed".to_string(),
        })?;

        step_count += 1;

        // Use the full state from step result
        current = step_result.state;

        // Persist state after each step
        state.db.set_deployment_state(&current).await?;

        // Check if deployment is synced
        if current.status.is_synced() {
            if matches!(
                current.status,
                DeploymentStatus::Running | DeploymentStatus::Deleted
            ) {
                debug!("Deployment synced, clearing deployment config");
                state.db.clear_deployment_config().await?;
            }

            info!(status = ?current.status, steps = step_count, "Deployment synced");
            break;
        }

        // Respect the suggested delay before next step
        if let Some(delay_ms) = step_result.suggested_delay_ms {
            debug!(delay_ms = delay_ms, "Waiting before next step");
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    Ok(step_count)
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
