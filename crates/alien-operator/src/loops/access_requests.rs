//! Access-request sync loop — pluggable surface.
//!
//! An **access request** is a time-boxed grant: the control plane proposes a
//! set of commands (usually a remediation plan) and asks the customer to
//! authorize them. This loop is about *authorization*, NOT execution:
//!
//! 1. Pull pending access requests for this deployment from the control plane.
//! 2. Materialize each through an [`AccessRequestBackend`] so the customer can
//!    review + approve it in their environment (the Kubernetes backend creates
//!    a custom resource; other backends can be added later).
//! 3. Read approvals — each carries an "approved until" instant — and report
//!    them back to the control plane.
//!
//! Once the control plane has an approval (valid until T), it dispatches the
//! request's commands one by one through the **normal commands queue**. So the
//! access-request loop grants access; the commands loop executes.
//!
//! The operator exposes the loop trait + a no-op stub. The real loop and its
//! backends are injected by the binary via
//! [`crate::run_operator_with_cancel_and_loops`], so the proprietary backend
//! code (apiserver access, CR shapes) stays off the OSS call graph and the OSS
//! crate keeps building standalone.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use tracing::debug;

use crate::OperatorState;

/// One command an access request covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRequestCommand {
    /// The command name, `<plugin>/<operation>`.
    pub command: String,
    /// One-line human summary for display.
    pub summary: String,
    /// Exact operation parameters the customer is being asked to authorize.
    pub params: Map<String, Value>,
}

/// A control-plane access request: a grant the customer must approve before its
/// commands may be dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRequest {
    /// Stable id assigned by the control plane.
    pub id: String,
    /// Human-readable title (e.g. the remediation plan title).
    pub title: String,
    /// Why access is being requested (the investigation's purpose), if given.
    pub reason: Option<String>,
    /// The commands this grant covers.
    pub commands: Vec<AccessRequestCommand>,
}

/// A customer approval read back from a backend: the request is approved and
/// authorized until `approved_until`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessApproval {
    /// The access-request id this approval is for.
    pub id: String,
    /// The instant the grant is authorized until. After this, the control plane
    /// must not dispatch the request's commands.
    pub approved_until: DateTime<Utc>,
}

/// A place access requests are materialized for the customer to approve, and
/// approvals are read from. Kubernetes is the first backend (a custom resource
/// the customer patches); others can be added without touching the loop.
#[async_trait]
pub trait AccessRequestBackend: Send + Sync + 'static {
    /// Ensure a materialized artifact exists for each request (idempotent). A
    /// backend that already has one for an id leaves it untouched.
    async fn sync(&self, requests: &[AccessRequest]) -> Result<(), String>;

    /// Read the currently-approved requests and their time windows.
    async fn poll_approvals(&self) -> Result<Vec<AccessApproval>, String>;
}

/// Pluggable access-request sync loop. The implementation owns the pull →
/// materialize → report-back cycle using an [`AccessRequestBackend`]. Runs until
/// `state.cancel` fires.
#[async_trait]
pub trait AccessRequestSyncLoop: Send + Sync + 'static {
    async fn run(self: Arc<Self>, state: Arc<OperatorState>);
}

/// Default no-op implementation used when no real loop is wired in (OSS builds,
/// tests, airgapped binaries). Logs once and waits for shutdown so the
/// supervisor doesn't treat the unused loop as an unexpected exit.
pub struct UnimplementedAccessRequestSyncLoop;

#[async_trait]
impl AccessRequestSyncLoop for UnimplementedAccessRequestSyncLoop {
    async fn run(self: Arc<Self>, state: Arc<OperatorState>) {
        debug!(
            "Access-request sync loop not configured — access requests are not \
             materialized for approval. Provide an `AccessRequestSyncLoop` via \
             `run_operator_with_cancel_and_loops` to enable."
        );
        state.cancel.cancelled().await;
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use alien_core::Platform;
    use tokio::time::timeout;
    use tokio_util::sync::CancellationToken;

    use super::{AccessRequestSyncLoop, UnimplementedAccessRequestSyncLoop};
    use crate::{db::OperatorDb, OperatorConfig, OperatorState};

    #[tokio::test]
    async fn unimplemented_loop_stays_alive_until_operator_shutdown() {
        let data_dir = tempfile::tempdir().expect("create operator data directory");
        let data_dir_path = data_dir.path().to_string_lossy().into_owned();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = Arc::new(
            OperatorDb::new(&data_dir_path, encryption_key)
                .await
                .expect("create operator database"),
        );
        let cancel = CancellationToken::new();
        let config = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .data_dir(data_dir_path)
            .encryption_key(encryption_key)
            .build();
        let state = Arc::new(OperatorState {
            config,
            db,
            service_provider: None,
            cancel: cancel.clone(),
        });

        let mut task = tokio::spawn(Arc::new(UnimplementedAccessRequestSyncLoop).run(state));
        assert!(
            timeout(Duration::from_millis(50), &mut task).await.is_err(),
            "unimplemented access-request loop must not terminate the operator"
        );

        cancel.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .expect("access-request loop should stop after operator cancellation")
            .expect("access-request loop task should not panic");
    }
}
