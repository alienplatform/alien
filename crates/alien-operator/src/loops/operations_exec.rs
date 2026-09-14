//! Operations execution — the executor contract for operator-run plugins.
//!
//! An **operations command** is a `<plugin>/<operation>` the control plane has
//! authorized (a customer approved its access request). These are operator-level
//! actions (e.g. `kubernetes/restart-pod`) with no workload receiver to run
//! them, so the operator runs them itself.
//!
//! Delivery uses the **normal commands queue**: the operator registers itself as
//! a command target and an embedded [`alien_commands`] receiver leases these
//! commands from the manager — the same path a Container/Daemon uses to receive
//! a dashboard command in pull mode. The only piece the OSS operator can't ship
//! is the *executor* itself, which owns the proprietary plugin registry
//! (builtins + baked custom bundles) and the approval policy.
//!
//! So this module defines just the executor contract. A downstream binary
//! provides an [`OperationsExecutor`], wires it into a command receiver, and runs
//! that receiver alongside the operator's other loops. OSS builds provide none,
//! so the operator runs no operations commands. `alien-operator` intentionally
//! does not depend on `alien-commands`' receiver here — the receiver lives in the
//! downstream binary next to the executor.

use std::sync::Arc;

use alien_core::sync::{OperationsReport, TargetOperationsBundleSet};
use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;

use crate::OperatorState;

/// The outcome of running one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOutcome {
    /// The operation ran and produced a JSON result.
    Success { result: Value },
    /// The operation was held because policy requires manual approval and none
    /// is on record. Not an execution error — left for a later decision.
    Held { reason: String },
    /// The operation failed (unknown plugin, non-zero exit, etc.).
    Failed { code: String, message: String },
}

/// Runs an authorized `<plugin>/<operation>`. Injected by the binary; the OSS
/// operator has no implementation, so it runs no operations commands. The
/// downstream binary calls this from its embedded command receiver's handler.
#[async_trait]
pub trait OperationsExecutor: Send + Sync + 'static {
    /// Run `plugin`/`operation` with inline JSON `params` (already decoded from
    /// the command envelope).
    async fn execute(&self, plugin: &str, operation: &str, params: &Value) -> OperationOutcome;
}

/// Pluggable operations receiver loop. The downstream binary's implementation
/// registers itself as a command target and runs an embedded command receiver
/// that leases operations-commands from the manager (the normal commands queue)
/// and executes them through an [`OperationsExecutor`]. It gets [`OperatorState`]
/// so it can read the commands URL / sync token / deployment id. Runs until
/// `state.cancel` fires.
#[async_trait]
pub trait OperationsExecLoop: Send + Sync + 'static {
    async fn run(self: Arc<Self>, state: Arc<OperatorState>);
}

/// Drives the Operator's plugin registry toward the target bundle set the
/// sync loop receives, and reports what's actually loaded. Injected by the
/// binary; the OSS operator has no plugin registry, so it downloads nothing
/// and reports nothing.
///
/// The sync loop calls [`sync_bundles`](Self::sync_bundles) once per sync
/// tick with the server's `targetOperationsBundleSet` (or `None` if the
/// project has never enabled a plugin). An implementation should:
/// 1. Compare `target.hash` (if any) against what it currently has loaded.
/// 2. If different, download the bundles it doesn't already have via their
///    presigned URLs and hot-reload its registry — never block the sync
///    loop itself; downloads should happen in the background and this call
///    should return promptly with the state as of *now*.
/// 3. Return an [`OperationsReport`] reflecting the currently loaded catalog
///    (not the target) so the platform can detect drift and mark the
///    installation `stuck` if downloads keep failing.
#[async_trait]
pub trait OperationsSyncHandler: Send + Sync + 'static {
    async fn sync_bundles(&self, target: Option<&TargetOperationsBundleSet>) -> OperationsReport;
}

/// No-op handler used when no plugin registry is wired in (OSS builds,
/// airgapped binaries). Reports an empty, hash-less catalog — the platform
/// reads a `None`/absent report as "this Operator doesn't report," never as
/// "stuck."
pub struct UnimplementedOperationsSyncHandler;

#[async_trait]
impl OperationsSyncHandler for UnimplementedOperationsSyncHandler {
    async fn sync_bundles(&self, _target: Option<&TargetOperationsBundleSet>) -> OperationsReport {
        OperationsReport {
            loaded_bundle_hash: None,
            operations: Vec::new(),
        }
    }
}

/// No-op loop used when no executor is wired in (OSS builds, airgapped
/// binaries). Logs once and parks until shutdown so the supervisor doesn't
/// treat the unused loop as an early exit.
pub struct UnimplementedOperationsExecLoop;

#[async_trait]
impl OperationsExecLoop for UnimplementedOperationsExecLoop {
    async fn run(self: Arc<Self>, state: Arc<OperatorState>) {
        debug!(
            "Operations receiver not configured — authorized operations commands \
             are not run by this operator. Provide an `OperationsExecLoop` via \
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

    use super::{
        OperationsExecLoop, OperationsSyncHandler, UnimplementedOperationsExecLoop,
        UnimplementedOperationsSyncHandler,
    };
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
            operations_sync_handler: None,
            cancel: cancel.clone(),
        });

        let mut task = tokio::spawn(Arc::new(UnimplementedOperationsExecLoop).run(state));
        assert!(
            timeout(Duration::from_millis(50), &mut task).await.is_err(),
            "unimplemented operations receiver must not terminate the operator"
        );

        cancel.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .expect("operations receiver should stop after operator cancellation")
            .expect("operations receiver task should not panic");
    }

    #[tokio::test]
    async fn unimplemented_sync_handler_reports_empty_hashless_catalog() {
        let report = UnimplementedOperationsSyncHandler.sync_bundles(None).await;
        assert!(report.loaded_bundle_hash.is_none());
        assert!(report.operations.is_empty());
    }
}
