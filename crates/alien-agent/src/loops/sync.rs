//! Sync loop - syncs state with Manager
//!
//! This loop exchanges state with the Manager:
//! - Reports: current deployment state (includes current_release and target_release)
//! - Receives: target deployment (release info + config)
//!
//! When approval_mode is Manual, new targets create approval records
//! that must be approved before deployment proceeds.

use crate::db::{Approval, ApprovalStatus};
use crate::AgentState;
use alien_core::sync::{AgentRegime, SyncRequest, SyncResponse};
use alien_error::{Context, IntoAlienError};
use chrono::Utc;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Run the sync loop
///
/// This loop:
/// 1. Gets current state from local database
/// 2. Sends state to Management server
/// 3. Receives target release and config
/// 4. Stores in local database for deployment loop
/// 5. Creates approval record if manual approval is required
pub async fn run_sync_loop(state: Arc<AgentState>) {
    let interval = Duration::from_secs(state.config.sync_interval_seconds);

    let sync_config = match &state.config.sync {
        Some(config) => config,
        None => {
            error!("Sync configuration not provided, sync loop cannot run");
            return;
        }
    };

    // Create authenticated client
    let client = match create_authenticated_client(&sync_config.token) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to create authenticated client");
            return;
        }
    };

    info!(
        interval_seconds = state.config.sync_interval_seconds,
        "Starting sync loop"
    );

    loop {
        match sync_with_manager(&state, &client, sync_config.url.as_str()).await {
            Ok(has_update) => {
                // First successful sync turns /readyz from 503 → 200 — the
                // gate Helm's --atomic --wait relies on so a freshly-rolled
                // agent isn't marked ready until it has actually talked to
                // the manager. Idempotent — only the first store matters.
                state
                    .first_sync_completed
                    .store(true, std::sync::atomic::Ordering::Release);
                if has_update {
                    info!("Received update from manager");
                } else {
                    debug!("Sync complete, no updates");
                }
            }
            Err(e) => {
                error!(error = %e, "Sync failed");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {},
            _ = state.cancel.cancelled() => {
                info!("Sync loop shutting down");
                return;
            }
        }
    }
}

/// Create an authenticated HTTP client
fn create_authenticated_client(token: &str) -> crate::error::Result<Client> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    let auth_value = format!("Bearer {}", token);
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(crate::error::ErrorData::SyncFailed {
                message: "Invalid auth token".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-agent"));

    Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::SyncFailed {
            message: "Failed to build HTTP client".to_string(),
        })
}

async fn sync_with_manager(
    state: &AgentState,
    client: &Client,
    base_url: &str,
) -> crate::error::Result<bool> {
    // Get current deployment state from local database (or create default if not exists)
    let deployment_state =
        state
            .db
            .get_deployment_state()
            .await?
            .unwrap_or_else(|| alien_core::DeploymentState {
                platform: state.config.platform,
                status: alien_core::DeploymentStatus::Pending,
                current_release: None,
                target_release: None,
                stack_state: None,
                environment_info: None,
                runtime_metadata: None,
                retry_requested: false,
                protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            });

    // Build the sync request with full deployment state
    // Deployment ID is stored in SQLite (from initialization)
    let deployment_id = state
        .db
        .get_deployment_id()
        .await?
        .expect("deployment_id must be set in online mode");

    let sync_request = SyncRequest {
        deployment_id: deployment_id.clone(),
        current_state: Some(deployment_state),
        // Agent self-update inventory — fleet visibility + upgrade gating.
        agent_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        agent_os: Some(std::env::consts::OS.to_string()),
        agent_arch: Some(std::env::consts::ARCH.to_string()),
        regime: Some(detect_agent_regime()),
    };

    // Call manager with deployment_id in request body.
    //
    // NOTE: We use raw reqwest here instead of the alien-manager-api SDK because
    // the SDK's generated `AgentSyncRequest.currentState` and
    // `AgentSyncResponse.target` fields are `serde_json::Value` (the OpenAPI spec
    // defines them as free-form objects). Using `alien_core::sync::{SyncRequest,
    // SyncResponse}` directly gives us proper typed serialization/deserialization.
    // If the OpenAPI spec is updated to use $ref'd schemas for these fields,
    // this code should be migrated to the SDK's `agent_sync()` method.
    let base_url = base_url.trim_end_matches('/');
    let url = format!("{}/v1/sync", base_url);

    debug!(url = %url, deployment_id = %deployment_id, "Sending sync request to manager");

    let response = client
        .post(&url)
        .json(&sync_request)
        .send()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::SyncFailed {
            message: "Failed to send sync request".to_string(),
        })?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read error>".to_string());

        // Attempt to deserialize as AlienError
        if let Ok(alien_error) =
            serde_json::from_str::<alien_error::AlienError<crate::error::ErrorData>>(&error_body)
        {
            return Err(alien_error);
        }

        return Err(alien_error::AlienError::new(
            crate::error::ErrorData::SyncFailed {
                message: format!("Manager returned error {}: {}", status, error_body),
            },
        ));
    }

    let sync_response: SyncResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::SyncFailed {
                message: "Failed to parse sync response".to_string(),
            })?;

    // Persist the commands URL so the deployment loop can inject it into
    // deployed functions. This is the public URL cloud functions use to poll
    // for pending commands (vs. the agent's local sync URL which is only
    // reachable from the machine running the agent).
    if let Some(ref commands_url) = sync_response.commands_url {
        if let Err(e) = state.db.set_commands_url(commands_url).await {
            error!(error = %e, "Failed to persist commands_url");
        }
    }

    let mut state_hydrated = false;
    if let Some(manager_state) = sync_response.current_state {
        let local_state = state.db.get_deployment_state().await?;
        if local_state
            .as_ref()
            .is_none_or(is_uninitialized_deployment_state)
        {
            state.db.set_deployment_state(&manager_state).await?;
            state_hydrated = true;
            info!("Hydrated deployment state from manager");
        } else {
            debug!("Manager returned deployment state, but local state is already initialized");
        }
    }

    // Check if there's a new target
    let has_update = sync_response.target.is_some();

    if has_update {
        if let Some(target_deployment) = sync_response.target {
            let now = Utc::now().to_rfc3339();

            // Get current deployment state (or create default)
            let mut deployment_state =
                state.db.get_deployment_state().await?.unwrap_or_else(|| {
                    alien_core::DeploymentState {
                        platform: state.config.platform,
                        status: alien_core::DeploymentStatus::Pending,
                        current_release: None,
                        target_release: None,
                        stack_state: None,
                        environment_info: None,
                        runtime_metadata: None,
                        retry_requested: false,
                        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                    }
                });

            // Update target_release in state
            let target_release_id = target_deployment.release_info.release_id.clone();
            let current_release_id = deployment_state
                .current_release
                .as_ref()
                .map(|release| release.release_id.clone());
            deployment_state.target_release = Some(target_deployment.release_info.clone());
            if deployment_state.status == alien_core::DeploymentStatus::Running
                && current_release_id.as_deref() != Some(target_release_id.as_str())
            {
                deployment_state.status = alien_core::DeploymentStatus::UpdatePending;
            }

            // Save state and config
            state.db.set_deployment_state(&deployment_state).await?;
            state
                .db
                .set_deployment_config(&target_deployment.config)
                .await?;

            // Handle deployment approval if required
            if state.config.requires_deployment_approval() {
                let apr_id = format!("apr_{}", Uuid::new_v4().simple());
                let approval = Approval {
                    id: apr_id.clone(),
                    release_info: Some(target_deployment.release_info.clone()),
                    deployment_config: target_deployment.config.clone(),
                    status: ApprovalStatus::Pending,
                    reason: None,
                    created_at: now,
                    decided_at: None,
                    decided_by: None,
                };
                state.db.create_approval(&approval).await?;
                info!(
                    approval_id = %apr_id,
                    release_id = %target_deployment.release_info.release_id,
                    "Created approval for new target release"
                );
            }
        }
    }

    // Agent self-update: act on `agent_target` when the manager emits one.
    // Best-effort — the actuator logs failures and the manager keeps
    // sending the target until the agent reports the new version.
    if let Some(target) = sync_response.agent_target.as_ref() {
        crate::loops::agent_upgrade::apply_agent_target(target).await;
    }

    Ok(has_update || state_hydrated)
}

fn is_uninitialized_deployment_state(state: &alien_core::DeploymentState) -> bool {
    state.status == alien_core::DeploymentStatus::Pending
        && state.current_release.is_none()
        && state.target_release.is_none()
        && state.stack_state.is_none()
        && state.environment_info.is_none()
        && state.runtime_metadata.is_none()
}

/// Detect the agent's supervisor regime. `KUBERNETES_SERVICE_HOST` is the
/// kubelet-injected signal and takes precedence over any other hint; outside
/// k8s the agent is supervised as a native OS service via the launcher.
fn detect_agent_regime() -> AgentRegime {
    if std::env::var_os("KUBERNETES_SERVICE_HOST").is_some() {
        AgentRegime::Kubernetes
    } else {
        AgentRegime::OsService
    }
}

#[cfg(test)]
mod tests {
    use alien_core::{
        DeploymentState, DeploymentStatus, Platform, CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    };

    use super::is_uninitialized_deployment_state;

    #[test]
    fn recognizes_empty_pending_state_as_uninitialized() {
        let state = DeploymentState {
            platform: Platform::Kubernetes,
            status: DeploymentStatus::Pending,
            current_release: None,
            target_release: None,
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };

        assert!(is_uninitialized_deployment_state(&state));
    }

    #[test]
    fn recognizes_non_pending_state_as_initialized() {
        let state = DeploymentState {
            platform: Platform::Kubernetes,
            status: DeploymentStatus::Provisioning,
            current_release: None,
            target_release: None,
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };

        assert!(!is_uninitialized_deployment_state(&state));
    }
}
