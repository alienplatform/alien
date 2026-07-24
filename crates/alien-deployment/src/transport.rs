//! Deployment loop transport trait.
//!
//! Abstracts per-step state persistence so the shared [`runner::run_step_loop`]
//! can be used by every deployment loop caller:
//!
//! | Caller           | Transport                           | Persistence            |
//! |------------------|-------------------------------------|------------------------|
//! | alien-manager    | `ManagerTransport`                  | SQLite (DeploymentStore)|
//! | alien-deploy-cli | `ManagerApiTransport`               | Manager API            |
//! | alien-cli        | `ManagerApiTransport`               | Manager API            |
//! | alien-terraform  | `ManagerApiTransport`               | Manager API            |
//! | alien-operator      | `OperatorTransport`                    | Local SQLite           |
//!
//! Lock acquire/release stays with the caller — only per-step reconcile is
//! part of the trait.
//!
//! See [`crate::manager_api_transport`] for the shared `ManagerApiTransport`
//! used by external callers (CLI, Terraform).

use alien_core::{DeploymentConfig, DeploymentState, ObservedInventoryBatch, ResourceHeartbeat};
use alien_error::AlienError;
use async_trait::async_trait;

/// Result of reconciling state after a single deployment step.
///
/// The server may modify the state (e.g. after setting up cross-account registry
/// access) or update the config (e.g. refresh environment variables or domain metadata).
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
    /// Renew the caller's deployment lease while a cloud operation is still in
    /// flight. Transports without distributed locking may keep the default
    /// no-op implementation.
    async fn renew_lease(
        &self,
        _deployment_id: &str,
        _state: &DeploymentState,
        _config: &DeploymentConfig,
    ) -> Result<(), AlienError> {
        Ok(())
    }

    /// Persist deployment state after a step and optionally return updates.
    ///
    /// # Arguments
    /// * `deployment_id` — identifies the deployment being stepped.
    /// * `state` — the state produced by the most recent `step()` call.
    /// * `update_heartbeat` — whether the step wants the heartbeat timestamp updated.
    async fn reconcile_step(
        &self,
        deployment_id: &str,
        state: &DeploymentState,
        config: &DeploymentConfig,
        update_heartbeat: bool,
        suggested_delay_ms: Option<u64>,
        heartbeats: Vec<ResourceHeartbeat>,
        observed_inventory_batches: Vec<ObservedInventoryBatch>,
    ) -> Result<StepReconcileResult, AlienError>;
}
