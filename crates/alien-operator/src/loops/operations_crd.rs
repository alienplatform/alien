//! Operations approval controller — pluggable surface.
//!
//! The operator exposes a trait + a no-op `Unimplemented` stub. The real
//! implementation (materializing leased operation-commands as `AlienOperation`
//! custom resources in the customer's cluster, watching them for the customer's
//! `approved: true` patch, and running approved operations) is injected by the
//! binary via [`crate::run_operator_with_cancel_and_loops`].
//!
//! Why a trait, not inline code:
//!
//! - The real loop reaches the in-cluster apiserver to create/patch a custom
//!   resource, runs proprietary operations-plugin code, and reports results —
//!   detail that doesn't belong on every OSS agent build's call graph.
//! - Trait injection keeps the type system whole: a fork can plug in its own
//!   approval backend without touching `alien-operator` internals, and the OSS
//!   crate keeps building standalone.
//!
//! When no loop is wired in, the stub is used, the rest of the operator runs
//! unaffected, and no `AlienOperation` resources are created or executed.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use crate::OperatorState;

/// Pluggable operations approval controller.
///
/// The implementation owns the whole customer-approval lifecycle: leasing
/// pending operation-commands, materializing each as an `AlienOperation`
/// custom resource (`REQUIRES_APPROVAL`), watching for the customer to patch
/// `spec.approved: true`, running approved operations via the operations
/// registry, and writing the result to the resource's `status`. Implementations
/// should run until `state.cancel` fires.
#[async_trait]
pub trait OperationsCrdLoop: Send + Sync + 'static {
    async fn run(self: Arc<Self>, state: Arc<OperatorState>);
}

/// Default no-op implementation used when no real loop is wired in (OSS builds,
/// tests, airgapped binaries). Logs once at startup and waits for shutdown so
/// the operator supervisor doesn't treat the unused loop as an unexpected exit.
pub struct UnimplementedOperationsCrdLoop;

#[async_trait]
impl OperationsCrdLoop for UnimplementedOperationsCrdLoop {
    async fn run(self: Arc<Self>, state: Arc<OperatorState>) {
        debug!(
            "Operations approval controller not configured — AlienOperation \
             resources are not created or executed. Provide an \
             `OperationsCrdLoop` via `run_operator_with_cancel_and_loops` to \
             enable."
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

    use super::{OperationsCrdLoop, UnimplementedOperationsCrdLoop};
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

        let mut task = tokio::spawn(Arc::new(UnimplementedOperationsCrdLoop).run(state));
        assert!(
            timeout(Duration::from_millis(50), &mut task).await.is_err(),
            "unimplemented operations-crd loop must not terminate the operator"
        );

        cancel.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .expect("operations-crd loop should stop after operator cancellation")
            .expect("operations-crd loop task should not panic");
    }
}
