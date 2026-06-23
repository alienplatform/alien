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

    // Initialize encrypted database
    let db = Arc::new(db::OperatorDb::new(&config.data_dir, &config.encryption_key).await?);

    // Create shared state
    let state = Arc::new(OperatorState {
        config: config.clone(),
        db: db.clone(),
        service_provider,
        cancel: cancel.clone(),
    });

    // Start OTLP server (for local functions to send telemetry).
    // This is best-effort — a port conflict should not take down the operator.
    let otlp_host = config.otlp_server_host;
    let otlp_port = config.otlp_server_port;
    let otlp_db = db.clone();
    let otlp_namespace = config.namespace.clone();
    let otlp_collector_token = config.collector_token.clone();
    let otlp_cancel = cancel.clone();
    tokio::spawn(async move {
        if let Err(e) = otlp_server::start_otlp_server(
            otlp_host,
            otlp_port,
            otlp_db,
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

    // Wait for cancellation or any loop to exit unexpectedly
    tokio::select! {
        _ = cancel.cancelled() => {
            info!("Shutdown signal received, waiting for loops to finish...");
        },
        _ = deployment_handle => {
            warn!("Deployment loop exited unexpectedly");
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
}
