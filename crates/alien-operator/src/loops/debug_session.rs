//! Pull-mode `alien debug` tunnel loop — pluggable surface.
//!
//! The operator exposes a trait + a no-op `Unimplemented` stub. The real
//! implementation (cluster-side identity exchange, per-session WebSocket
//! dial-back, kubectl + cloud frame forwarding, audit lifecycle) is
//! injected by the binary via
//! [`crate::run_operator_with_cancel_and_debug_loop`].
//!
//! Why a trait, not inline code:
//!
//! - The real loop depends on per-deployment management-SA impersonation
//!   chains, cloud-specific token minting (IRSA / GKE WI / AKS WI), and
//!   audit-DB lifecycle calls — implementation detail that doesn't belong
//!   on every agent build's call graph.
//! - Trait injection keeps the type system whole: forks can plug in their
//!   own debug backend without touching `alien-operator` internals.
//!
//! When no loop is wired in, the stub is used, the rest of the operator runs
//! unaffected, and `alien debug` simply reports "not supported".

use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use crate::OperatorState;

/// Pluggable per-deployment debug-session loop.
///
/// The implementation owns the entire claim-and-tunnel lifecycle: polling
/// the manager for pending sessions, dialing the per-session WebSocket,
/// forwarding frames to the in-cluster apiserver and (push-mode) to cloud
/// APIs with platform-signed credentials, and unwinding cleanly on
/// shutdown. Implementations should run until `state.cancel` fires.
#[async_trait]
pub trait DebugSessionLoop: Send + Sync + 'static {
    async fn run(self: Arc<Self>, state: Arc<OperatorState>);
}

/// Default no-op implementation used when no real loop is wired in
/// (OSS builds, tests, airgapped binaries). Logs once at startup and waits for
/// shutdown so the operator supervisor does not treat the unsupported loop as
/// an unexpected exit.
pub struct UnimplementedDebugSessionLoop;

#[async_trait]
impl DebugSessionLoop for UnimplementedDebugSessionLoop {
    async fn run(self: Arc<Self>, state: Arc<OperatorState>) {
        debug!(
            "Debug-session loop not configured — `alien debug` tunnels are \
             not supported. Provide a `DebugSessionLoop` via \
             `run_operator_with_cancel_and_debug_loop` to enable."
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

    use super::{DebugSessionLoop, UnimplementedDebugSessionLoop};
    use crate::{db::OperatorDb, OperatorConfig, OperatorState};

    #[tokio::test]
    async fn unsupported_loop_stays_alive_until_operator_shutdown() {
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

        let mut task = tokio::spawn(Arc::new(UnimplementedDebugSessionLoop).run(state));
        assert!(
            timeout(Duration::from_millis(50), &mut task).await.is_err(),
            "unsupported debug loop must not terminate the operator"
        );

        cancel.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .expect("debug loop should stop after operator cancellation")
            .expect("debug loop task should not panic");
    }
}
