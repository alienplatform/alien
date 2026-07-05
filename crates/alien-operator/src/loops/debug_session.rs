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
/// (OSS builds, tests, airgapped binaries). Logs once at startup and
/// returns; the agent's other loops are unaffected.
pub struct UnimplementedDebugSessionLoop;

#[async_trait]
impl DebugSessionLoop for UnimplementedDebugSessionLoop {
    async fn run(self: Arc<Self>, _state: Arc<OperatorState>) {
        debug!(
            "Debug-session loop not configured — `alien debug` tunnels are \
             not supported. Provide a `DebugSessionLoop` via \
             `run_operator_with_cancel_and_debug_loop` to enable."
        );
    }
}
