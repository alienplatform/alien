//! Alien Operator
//!
//! The Operator is a library that runs in remote environments (Kubernetes, local machines)
//! and handles deployments for pull-model deployments.
//!
//! ## Usage
//!
//! ```ignore
//! use alien_operator::{OperatorConfig, SyncConfig, run_operator};
//!
//! let config = OperatorConfig::builder()
//!     .platform(alien_core::Platform::Aws)
//!     .maybe_sync(Some(SyncConfig {
//!         url: "https://manager.example.com".parse().unwrap(),
//!         token: "ax_dep_xxx".to_string(),
//!     }))
//!     .encryption_key("your_64_char_hex_encryption_key_here_for_aegis256_cipher")
//!     .build();
//!
//! run_operator(config, None).await?;
//! ```

pub mod cli;
pub mod collector_logs;
pub mod config;
pub mod db;
pub mod error;
pub mod lock;
pub mod loops;
pub mod otlp_server;
pub mod self_update;

// The test seam must never ship: a release build with `test-hooks` on would
// let an env var falsify the fleet's version inventory.
#[cfg(all(feature = "test-hooks", not(debug_assertions)))]
compile_error!("the `test-hooks` feature must not be enabled in release builds");

/// The version this operator reports on sync and compares against update
/// targets. Normally `CARGO_PKG_VERSION`; under the `test-hooks` feature
/// (debug builds only) `ALIEN_OPERATOR_FAKE_VERSION` overrides it so E2E
/// suites can drive an update without compiling two binaries.
pub fn operator_version() -> String {
    #[cfg(feature = "test-hooks")]
    if let Ok(fake) = std::env::var("ALIEN_OPERATOR_FAKE_VERSION") {
        if !fake.is_empty() {
            return fake;
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

pub use alien_core::{DeploymentState, DeploymentStatus, Platform, ReleaseInfo};
pub use config::{OperatorConfig, SyncConfig};
pub use db::{Approval, ApprovalStatus};
pub use error::ErrorData;
pub use lock::InstanceLock;

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Run the Operator with the given configuration.
///
/// This starts all background loops:
/// - Sync loop: Syncs with manager every 30s (disabled in airgapped mode)
/// - Deployment loop: Runs step() when updates are available
/// - Telemetry loop: Pushes collected telemetry every 10s (disabled in airgapped mode)
/// - OTLP server: Receives telemetry from local functions
///
/// All loops respect the returned `CancellationToken` — when cancelled (e.g.
/// on SIGTERM), each loop finishes its current iteration and exits cleanly.
///
/// # Arguments
/// * `config` - Operator configuration
/// * `service_provider` - Optional platform service provider for local platform.
///   When running on local platform, pass a `DefaultPlatformServiceProvider::with_local_bindings()`
///   to enable local controllers to access service managers.
pub async fn run_operator(
    config: OperatorConfig,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
) -> error::Result<()> {
    let cancel = CancellationToken::new();
    run_operator_with_cancel(config, service_provider, cancel).await
}

/// Like [`run_operator`] but accepts an external [`CancellationToken`].
///
/// Cancel the token to trigger a graceful shutdown of all loops.
pub async fn run_operator_with_cancel(
    config: OperatorConfig,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    cancel: CancellationToken,
) -> error::Result<()> {
    run_operator_with_cancel_and_debug_loop(config, service_provider, None, cancel).await
}

/// Full-control entry point. Binary callers that ship a real
/// [`DebugSessionLoop`] implementation pass it here; the OSS default is
/// `None`, which falls back to the no-op stub.
pub async fn run_operator_with_cancel_and_debug_loop(
    config: OperatorConfig,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    debug_session_loop: Option<Arc<dyn loops::debug_session::DebugSessionLoop>>,
    cancel: CancellationToken,
) -> error::Result<()> {
    use tracing::{info, warn};

    info!(
        sync_configured = config.sync.is_some(),
        deployment_approval = config.requires_deployment_approval(),
        telemetry_approval = config.requires_telemetry_approval(),
        telemetry_enabled = config.is_telemetry_enabled(),
        otlp_host = %config.otlp_server_host,
        otlp_port = config.otlp_server_port,
        "Starting operator"
    );

    let readiness = otlp_server::ReadinessSignals::new();

    // Initialize encrypted database
    let db = Arc::new(db::OperatorDb::new(&config.data_dir, &config.encryption_key).await?);
    readiness
        .db_open
        .store(true, std::sync::atomic::Ordering::Release);
    // The CLI acquires the InstanceLock before calling run_operator and the
    // guard lives for the process lifetime, so reaching this point means the
    // lock is held.
    readiness
        .lock_held
        .store(true, std::sync::atomic::Ordering::Release);

    // Create shared state
    let state = Arc::new(OperatorState {
        config: config.clone(),
        db: db.clone(),
        service_provider,
        cancel: cancel.clone(),
        readiness: readiness.clone(),
    });

    // Die-with-parent: under the launcher, exit if our supervisor dies. macOS
    // has no PR_SET_PDEATHSIG, and this also backstops the Linux fork→exec race;
    // a no-op outside the launcher (Kubernetes, tests). Runs on a dedicated OS
    // thread (not the async runtime, which can starve its timer under load); it
    // self-exits when `cancel` is tripped, so the handle needs no explicit join.
    let _parent_death_watch = self_update::spawn_parent_death_watch(cancel.clone());

    // Start OTLP server (for local functions to send telemetry).
    // Also serves /livez and /readyz on the same port for Kubernetes probes.
    // Best-effort — a port conflict should not take down the operator.
    // Under the launcher, ALIEN_HEALTH_ADDR overrides the bind so the
    // probation gate probes the exact address it handed us; an unparseable
    // value is a startup error (a silent fallback would fail every probe by
    // port mismatch).
    let (otlp_host, otlp_port) = match otlp_server::health_addr_override()? {
        Some(addr) => {
            info!(address = %addr, "Health/OTLP bind overridden by the launcher (ALIEN_HEALTH_ADDR)");
            (addr.ip(), addr.port())
        }
        None => (config.otlp_server_host, config.otlp_server_port),
    };
    let otlp_db = db.clone();
    let otlp_namespace = config.namespace.clone();
    let otlp_collector_token = config.collector_token.clone();
    let otlp_cancel = cancel.clone();
    let probe_readiness = readiness.clone();
    tokio::spawn(async move {
        if let Err(e) = otlp_server::start_otlp_server(
            otlp_host,
            otlp_port,
            otlp_db,
            probe_readiness,
            otlp_namespace,
            otlp_collector_token,
            otlp_cancel,
        )
        .await
        {
            warn!(error = %e, "OTLP server failed (telemetry collection disabled)");
        }
    });

    // Start deployment loop (always runs)
    let deployment_handle = tokio::spawn({
        let state = state.clone();
        async move {
            loops::deployment::run_deployment_loop(state).await;
        }
    });

    // Start sync and telemetry loops only if not airgapped
    let sync_handle = if !config.is_airgapped() {
        Some(tokio::spawn({
            let state = state.clone();
            async move {
                loops::sync::run_sync_loop(state).await;
            }
        }))
    } else {
        warn!("Running in airgapped mode - sync loop disabled");
        None
    };

    let telemetry_handle = if !config.is_airgapped() {
        Some(tokio::spawn({
            let state = state.clone();
            async move {
                loops::otlp::run_telemetry_loop(state).await;
            }
        }))
    } else {
        warn!("Running in airgapped mode - telemetry loop disabled");
        None
    };

    // Start commands dispatch loop for cloud function platforms.
    // The loop self-guards: it exits immediately for K8s/Local/airgapped.
    let commands_handle = if !config.is_airgapped()
        && matches!(
            config.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) {
        Some(tokio::spawn({
            let state = state.clone();
            async move {
                loops::commands::run_commands_loop(state).await;
            }
        }))
    } else {
        None
    };

    // Pull-mode `alien debug` tunnel loop. K8s uses it for kubectl/cloud API
    // forwarding. Local only starts it when the service was installed with an
    // explicit runtime debug opt-in flag.
    let debug_session_handle = if !config.is_airgapped()
        && (matches!(config.platform, Platform::Kubernetes)
            || (matches!(config.platform, Platform::Local) && config.local_debug_enabled))
    {
        // Resolve which loop implementation to run. Binary callers that ship
        // the closed loop inject it via `run_operator_with_cancel_and_debug_loop`;
        // OSS callers fall through to the no-op stub.
        let loop_impl: Arc<dyn loops::debug_session::DebugSessionLoop> = debug_session_loop
            .unwrap_or_else(|| Arc::new(loops::debug_session::UnimplementedDebugSessionLoop));
        Some(tokio::spawn({
            let state = state.clone();
            async move {
                loop_impl.run(state).await;
            }
        }))
    } else {
        None
    };

    // Wait for cancellation or any loop to exit unexpectedly
    tokio::select! {
        _ = cancel.cancelled() => {
            info!("Shutdown signal received, waiting for loops to finish...");
        },
        _ = deployment_handle => {
            warn!("Deployment loop exited unexpectedly");
        },
        _ = async {
            if let Some(h) = debug_session_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            warn!("Debug-session loop exited unexpectedly");
        },
        _ = async {
            if let Some(h) = sync_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            warn!("Sync loop exited unexpectedly");
        },
        _ = async {
            if let Some(h) = telemetry_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            warn!("Telemetry loop exited unexpectedly");
        },
        _ = async {
            if let Some(h) = commands_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            warn!("Commands dispatch loop exited unexpectedly");
        },
    }

    // Signal all loops to stop (idempotent if already cancelled)
    cancel.cancel();

    // Give loops a moment to finish current work
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    info!("Operator shutdown complete");
    Ok(())
}

/// Operator state shared across loops.
pub struct OperatorState {
    pub config: OperatorConfig,
    pub db: Arc<db::OperatorDb>,
    /// Platform service provider for deployment operations.
    /// When running on local platform, this should contain a LocalBindingsProvider.
    pub service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    /// Cancellation token for graceful shutdown.
    pub cancel: CancellationToken,
    /// Readiness signals consumed by the `/readyz` probe handler — the
    /// explicit health conditions (DB open, InstanceLock held, first sync
    /// completed, deployment loop progressing). /readyz returns 503 until
    /// ALL hold, so a freshly-rolled operator isn't marked ready before it
    /// has proven it can run and reach the manager.
    pub readiness: otlp_server::ReadinessSignals,
}
