//! Deployment loop transport trait.
//!
//! Abstracts per-step state persistence so the shared [`runner::run_step_loop`]
//! can be used by every deployment loop caller:
//!
//! | Caller            | Transport                     | Persistence           |
//! |-------------------|-------------------------------|-----------------------|
//! | alien-manager     | `DeploymentStore::reconcile`  | SQLite or Platform API|
//! | alien-deploy-cli  | HTTP `POST /v1/sync/reconcile`| Manager API           |
//! | alien-agent       | Local Turso DB                | `set_deployment_state`|
//! | alien-cli         | HTTP `POST /sync/reconcile`   | Platform API          |
//!
//! Lock acquire/release stays with the caller — only per-step reconcile is
//! part of the trait.

use alien_core::{DeploymentConfig, DeploymentState};
use alien_error::AlienError;
use async_trait::async_trait;

/// Result of reconciling state after a single deployment step.
///
/// The server may modify the state (e.g. inject `image_pull_credentials` into
/// `runtime_metadata` after setting up cross-account registry access) or update
/// the config (e.g. refresh environment variables or domain metadata).
pub struct StepReconcileResult {
    /// Updated state from the server. When `Some`, the runner replaces its
    /// working state with this value before the next step.
    pub state: Option<DeploymentState>,
    /// Updated config from the server. When `Some`, the runner replaces its
    /// working config with this value before the next step.
    pub config: Option<DeploymentConfig>,
}

/// Per-step state persistence for the deployment loop.
///
/// Called by [`runner::run_step_loop`] after every `step()` invocation. The
/// implementation must persist the state durably and may return an updated
/// state/config if the server performed side-effects (registry access grants,
/// env var injection, etc.).
#[async_trait]
pub trait DeploymentLoopTransport: Send + Sync {
    /// Persist deployment state after a step and optionally return updates.
    ///
    /// # Arguments
    /// * `deployment_id` — identifies the deployment being stepped.
    /// * `state` — the state produced by the most recent `step()` call.
    /// * `step_error` — the error from the step, if any.
    /// * `update_heartbeat` — whether the step wants the heartbeat timestamp updated.
    async fn reconcile_step(
        &self,
        deployment_id: &str,
        state: &DeploymentState,
        step_error: Option<&AlienError>,
        update_heartbeat: bool,
    ) -> Result<StepReconcileResult, AlienError>;
}
