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

    // Local runtimes are real child processes owned by LocalBindingsProvider.
    // Keep a shutdown handle before moving the service provider into shared
    // operator state so cancellation can drain those children before exit.
    let local_bindings = service_provider
        .as_ref()
        .and_then(|provider| provider.get_local_bindings_provider());

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

    if let Some(local_bindings) = local_bindings {
        info!("Stopping local runtimes...");
        local_bindings.shutdown().await;
    }

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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{collections::HashMap, path::Path, time::Duration};

    use alien_local::{DaemonLaunchOptions, LocalBindingsProvider};

    #[tokio::test]
    async fn cancellation_stops_local_daemon_processes() {
        let temp = tempfile::tempdir().expect("create test directory");
        let daemon_dir = temp.path().join("daemons").join("test-daemon");
        std::fs::create_dir_all(&daemon_dir).expect("create daemon directory");

        let pid_file = temp.path().join("daemon.pid");
        let script = daemon_dir.join("run.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho $$ > '{}'\nwhile :; do sleep 1; done\n",
                pid_file.display()
            ),
        )
        .expect("write daemon script");
        std::fs::write(
            daemon_dir.join("metadata.json"),
            serde_json::to_vec(&serde_json::json!({
                "worker_id": "test-daemon",
                "extracted_path": daemon_dir.to_string_lossy(),
                "env_vars": {},
                "runtime_command": ["/bin/sh", script.to_string_lossy()],
                "working_dir": null,
                "transport_port": null,
                "runtime_only_binding_names": [],
            }))
            .expect("serialize daemon metadata"),
        )
        .expect("write daemon metadata");

        let local_bindings = LocalBindingsProvider::new(temp.path()).expect("create provider");
        local_bindings
            .worker_manager()
            .start_daemon(
                "test-daemon",
                HashMap::new(),
                DaemonLaunchOptions {
                    stop_grace_period_seconds: Some(2),
                    ..Default::default()
                },
            )
            .await
            .expect("start daemon");

        let pid = read_pid(&pid_file).await;
        assert!(
            process_exists(pid),
            "daemon should be alive before shutdown"
        );

        let provider: Arc<dyn alien_infra::PlatformServiceProvider> = Arc::new(
            alien_infra::DefaultPlatformServiceProvider::with_local_bindings(local_bindings),
        );
        let cancel = CancellationToken::new();
        cancel.cancel();
        let config = OperatorConfig::builder()
            .platform(Platform::Local)
            .data_dir(temp.path().to_string_lossy().to_string())
            .encryption_key("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
            .otlp_server_port(0)
            .build();

        run_operator_with_cancel(config, Some(provider), cancel)
            .await
            .expect("operator shutdown should succeed");

        assert!(
            !process_exists(pid),
            "operator cancellation must stop local daemon pid {pid}"
        );
    }

    async fn read_pid(path: &Path) -> u32 {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(value) = tokio::fs::read_to_string(path).await {
                    if let Ok(pid) = value.trim().parse() {
                        return pid;
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("daemon pid should be written")
    }

    fn process_exists(pid: u32) -> bool {
        // SAFETY: signal 0 performs an existence/permission check only.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
}
