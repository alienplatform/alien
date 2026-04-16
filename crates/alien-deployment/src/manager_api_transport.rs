//! Shared transport for callers that reconcile deployment state via the Manager API.
//!
//! Used by every "external" deployment loop caller:
//!
//! | Caller           | How it gets the manager client                     |
//! |------------------|----------------------------------------------------|
//! | alien-deploy-cli | `--manager-url` / embedded config / tracker        |
//! | alien-cli        | `resolve_manager()` (discovers URL via platform)   |
//! | alien-terraform  | `manager_url` from SyncAcquire response            |

use alien_core::DeploymentState;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager_api::{Client as ManagerClient, SdkResultExt};
use async_trait::async_trait;
use tracing::{error, info};

use crate::transport::{DeploymentLoopTransport, StepReconcileResult};

/// Transport that reconciles deployment state via the Manager API after each step.
///
/// Each `reconcile_step` call:
/// 1. POSTs the current state to `/v1/sync/reconcile` so the manager persists it
///    and runs server-side side-effects (e.g. cross-account registry access).
/// 2. Re-fetches the deployment to pick up any changes the manager made (e.g.
///    server-side mutations applied during reconciliation).
/// 3. Returns updated state if the manager injected server-side mutations.
pub struct ManagerApiTransport {
    client: ManagerClient,
    session: String,
}

impl ManagerApiTransport {
    pub fn new(client: ManagerClient, session: String) -> Self {
        Self { client, session }
    }
}

#[async_trait]
impl DeploymentLoopTransport for ManagerApiTransport {
    async fn reconcile_step(
        &self,
        deployment_id: &str,
        state: &DeploymentState,
        config: &alien_core::DeploymentConfig,
        step_error: Option<&AlienError>,
        update_heartbeat: bool,
    ) -> Result<StepReconcileResult, AlienError> {
        let state_json = serde_json::to_value(state)
            .into_alien_error()
            .context(alien_error::GenericError {
                message: "Failed to serialize state for reconcile".to_string(),
            })?;

        let error_json = step_error.map(|e| {
            serde_json::to_value(e)
                .unwrap_or_else(|_| serde_json::json!({ "message": e.to_string() }))
        });

        // POST state to the manager
        let resp = self
            .client
            .reconcile()
            .body(alien_manager_api::types::ReconcileRequest {
                deployment_id: deployment_id.to_string(),
                session: self.session.clone(),
                state: state_json,
                update_heartbeat: Some(update_heartbeat),
                error: error_json,
            })
            .send()
            .await
            .into_sdk_error()
            .context(alien_error::GenericError {
                message: "Failed to reconcile step via manager API".to_string(),
            })?
            .into_inner();

        // If the manager returned a native_image_host, inject it into the config
        // so Lambda/Cloud Run controllers resolve proxy URIs to native ECR/GAR URIs.
        let config_update = resp.native_image_host.and_then(|host| {
            if config.native_image_host.as_deref() == Some(&host) {
                None
            } else {
                let mut updated = config.clone();
                updated.native_image_host = Some(host);
                Some(updated)
            }
        });

        // Parse the server-returned state to pick up server-side mutations
        // (e.g. registry_access_granted flag set by reconcile_registry_access).
        // Without this, the client keeps sending stale state that overwrites
        // the server's updates on subsequent reconcile calls.
        let state_update = serde_json::from_value::<DeploymentState>(resp.current)
            .ok()
            .filter(|updated| updated.runtime_metadata != state.runtime_metadata);

        Ok(StepReconcileResult {
            state: state_update,
            config: config_update,
        })
    }
}

// ---------------------------------------------------------------------------
// Shared helpers for the acquire / final-reconcile / release pattern.
//
// Every external caller (alien-deploy-cli, alien-cli, alien-terraform) follows
// the same protocol:
//   1. acquire_deployment()   — lock the deployment with a retry loop
//   2. run_step_loop()        — step until terminal (uses ManagerApiTransport)
//   3. final_reconcile()      — persist terminal state (even on error)
//   4. release_deployment()   — unlock (even on error)
// ---------------------------------------------------------------------------

/// Maximum number of acquire attempts (60 × 2s = 2 minutes).
const MAX_ACQUIRE_ATTEMPTS: usize = 60;
/// Delay between acquire attempts in seconds.
const ACQUIRE_RETRY_DELAY_SECS: u64 = 2;

/// Acquire a deployment lock from the manager, retrying until the lock is granted
/// or the timeout is reached.
///
/// Returns `Ok(())` on success. The caller must call [`release_deployment`] when
/// done, even on error.
pub async fn acquire_deployment(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
) -> Result<(), AlienError> {
    for attempt in 1..=MAX_ACQUIRE_ATTEMPTS {
        let resp = client
            .acquire()
            .body(alien_manager_api::types::AcquireRequest {
                session: session.to_string(),
                deployment_ids: Some(vec![deployment_id.to_string()]),
                statuses: None,
                platforms: None,
                limit: None,
            })
            .send()
            .await
            .into_sdk_error()
            .context(alien_error::GenericError {
                message: "Failed to acquire sync lock".to_string(),
            })?;

        if !resp.into_inner().deployments.is_empty() {
            return Ok(());
        }

        if attempt == MAX_ACQUIRE_ATTEMPTS {
            return Err(AlienError::new(alien_error::GenericError {
                message: "Timed out waiting for deployment lock".to_string(),
            }));
        }

        info!(
            attempt = attempt,
            max = MAX_ACQUIRE_ATTEMPTS,
            "Waiting for deployment lock"
        );
        tokio::time::sleep(std::time::Duration::from_secs(ACQUIRE_RETRY_DELAY_SECS)).await;
    }

    unreachable!()
}

/// Persist the final deployment state to the manager.
///
/// Best-effort — failures are logged but do not propagate, because the lock
/// must still be released.
pub async fn final_reconcile(
    client: &ManagerClient,
    deployment_id: &str,
    session: &str,
    state: &DeploymentState,
) {
    let state_json = serde_json::to_value(state).unwrap_or_default();
    if let Err(e) = client
        .reconcile()
        .body(alien_manager_api::types::ReconcileRequest {
            deployment_id: deployment_id.to_string(),
            session: session.to_string(),
            state: state_json,
            update_heartbeat: Some(false),
            error: None,
        })
        .send()
        .await
    {
        error!(
            deployment_id = %deployment_id,
            error = %e,
            "Failed to reconcile final deployment state"
        );
    }
}

/// Release the deployment lock.
///
/// Best-effort — failures are logged but do not propagate.
pub async fn release_deployment(client: &ManagerClient, deployment_id: &str, session: &str) {
    if let Err(e) = client
        .release()
        .body(alien_manager_api::types::ReleaseRequest {
            deployment_id: deployment_id.to_string(),
            session: session.to_string(),
        })
        .send()
        .await
    {
        error!(
            deployment_id = %deployment_id,
            error = %e,
            "Failed to release sync lock"
        );
    }
}
