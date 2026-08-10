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

use alien_error::AlienError;
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

/// Back-compat entry point that injects only a [`DebugSessionLoop`]. Forwards to
/// [`run_operator_with_cancel_and_loops`] with no operations-approval loop.
pub async fn run_operator_with_cancel_and_debug_loop(
    config: OperatorConfig,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    debug_session_loop: Option<Arc<dyn loops::debug_session::DebugSessionLoop>>,
    cancel: CancellationToken,
) -> error::Result<()> {
    run_operator_with_cancel_and_loops(
        config,
        service_provider,
        debug_session_loop,
        None,
        None,
        cancel,
    )
        .await
}

/// Full-control entry point. Binary callers that ship real pluggable-loop
/// implementations pass them here; the OSS default is `None` for each, falling
/// back to the corresponding no-op stub.
///
/// - `debug_session_loop` — the `alien debug` tunnel loop.
/// - `access_request_loop` — the access-request sync loop (materializes access
///   requests for customer approval and reports approvals back; execution flows
///   separately through the commands queue).
/// - `operations_exec_loop` — the pull-mode operations-execution loop (runs
///   authorized `<plugin>/<operation>` commands the customer approved; OSS
///   builds inject none and run nothing).
pub async fn run_operator_with_cancel_and_loops(
    config: OperatorConfig,
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    debug_session_loop: Option<Arc<dyn loops::debug_session::DebugSessionLoop>>,
    access_request_loop: Option<Arc<dyn loops::access_requests::AccessRequestSyncLoop>>,
    operations_exec_loop: Option<Arc<dyn loops::operations_exec::OperationsExecLoop>>,
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

    // Start commands dispatch for native cloud push transports and the
    // Kubernetes/Local environment-local relays. An embedded Local deployment
    // uses direct push, while a remote Local deployment uses this relay.
    let commands_handle = if should_run_commands_loop(config.platform, config.is_airgapped()) {
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

    // Access-request sync loop. Materializes control-plane access requests as
    // artifacts the customer approves (Kubernetes: a custom resource) and
    // reports approvals back — a Kubernetes-only flow for now. Execution of the
    // approved commands happens separately via the commands queue. OSS builds
    // inject no loop and fall through to the no-op stub, which parks until
    // shutdown.
    let access_request_handle =
        if !config.is_airgapped() && matches!(config.platform, Platform::Kubernetes) {
            let loop_impl: Arc<dyn loops::access_requests::AccessRequestSyncLoop> =
                access_request_loop.unwrap_or_else(|| {
                    Arc::new(loops::access_requests::UnimplementedAccessRequestSyncLoop)
                });
            Some(tokio::spawn({
                let state = state.clone();
                async move {
                    loop_impl.run(state).await;
                }
            }))
        } else {
            None
        };

    // Operations-execution loop (pull mode). Runs authorized `<plugin>/<operation>`
    // commands the customer already approved. Only spawned when a binary injects
    // a real executor loop — OSS builds pass `None` here and the operator runs no
    // operations commands. The loop leases + runs regardless of platform (the
    // executor decides what it can run).
    let operations_exec_handle = match operations_exec_loop {
        Some(loop_impl) if !config.is_airgapped() => Some(tokio::spawn({
            let state = state.clone();
            async move {
                loop_impl.run(state).await;
            }
        })),
        _ => None,
    };

    // Wait for cancellation or any loop to exit unexpectedly. `exited_loop`
    // captures which loop (if any) fell out on its own; `None` means we were
    // cancelled cleanly. A loop exiting is never expected — the operator has no
    // useful work left once one is gone — so we surface it as an error below
    // rather than reporting a clean exit to CLI/service callers.
    //
    // Distinguishing a genuine loop failure from a shutdown-driven loop return
    // is a race unless we resolve it atomically WITH the branch that wins. Two
    // measures do that, and neither relies on re-reading the token after the
    // select (which would reopen a window where an independent shutdown flips it
    // between the select resolving and the read):
    //   - `biased` polls the cancellation branch first, so when both are ready in
    //     the same tick the clean-shutdown branch wins.
    //   - each loop branch samples `cancel.is_cancelled()` in the same synchronous
    //     step it wins in (no `.await` in between), so a loop that returned
    //     BECAUSE it observed the cancelled token classifies itself as clean.
    // A loop branch that wins with the token NOT yet cancelled is a real failure,
    // and nothing that happens afterward can flip that verdict.
    let exited_loop: Option<&'static str> = tokio::select! {
        biased;

        _ = cancel.cancelled() => {
            info!("Shutdown signal received, waiting for loops to finish...");
            None
        },
        _ = deployment_handle => loop_exit(&cancel, "deployment"),
        _ = async {
            if let Some(h) = debug_session_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => loop_exit(&cancel, "debug-session"),
        _ = async {
            if let Some(h) = sync_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => loop_exit(&cancel, "sync"),
        _ = async {
            if let Some(h) = telemetry_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => loop_exit(&cancel, "telemetry"),
        _ = async {
            if let Some(h) = commands_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => loop_exit(&cancel, "commands-dispatch"),
        _ = async {
            if let Some(h) = access_request_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => loop_exit(&cancel, "access-request-sync"),
        _ = async {
            if let Some(h) = operations_exec_handle {
                h.await.ok();
            } else {
                std::future::pending::<()>().await;
            }
        } => loop_exit(&cancel, "operations-execution"),
    };

    // Signal all loops to stop (idempotent if already cancelled)
    cancel.cancel();

    // Give loops a moment to finish current work
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    if let Some(local_bindings) = local_bindings {
        info!("Stopping local runtimes...");
        local_bindings.shutdown().await;
    }

    if let Some(loop_name) = exited_loop {
        // A core loop exited on its own — report a non-zero exit so CLI and
        // Windows-service callers don't mistake a failed loop for a clean stop.
        return Err(AlienError::new(error::ErrorData::LoopExited {
            loop_name: loop_name.to_string(),
        }));
    }

    info!("Operator shutdown complete");
    Ok(())
}

/// Classify a supervised loop falling out of the `select!`. Called synchronously
/// in the winning branch (no `.await` between the branch resolving and this
/// call), so the `is_cancelled()` read reflects the token state AT THE MOMENT the
/// loop won — closing the race where an independent shutdown flips the token
/// afterward. A loop that returned because shutdown was already requested is a
/// clean exit (`None`); otherwise it is a genuine failure (`Some(name)`).
fn loop_exit(cancel: &CancellationToken, name: &'static str) -> Option<&'static str> {
    if cancel.is_cancelled() {
        tracing::info!(loop = name, "Loop stopped in response to shutdown");
        None
    } else {
        tracing::warn!(loop = name, "Loop exited unexpectedly");
        Some(name)
    }
}

fn should_run_commands_loop(platform: Platform, airgapped: bool) -> bool {
    !airgapped
        && matches!(
            platform,
            Platform::Aws
                | Platform::Gcp
                | Platform::Azure
                | Platform::Kubernetes
                | Platform::Local
        )
}

#[cfg(test)]
mod command_loop_routing_tests {
    use super::{loop_exit, should_run_commands_loop};
    use alien_core::Platform;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn loop_exit_is_a_failure_only_without_shutdown() {
        // A loop that falls out while the token is live is a real failure.
        let live = CancellationToken::new();
        assert_eq!(loop_exit(&live, "sync"), Some("sync"));

        // A loop that falls out after shutdown was requested is a clean exit —
        // it stopped because it was told to. `loop_exit` samples the token in the
        // same step the branch wins, so an independent cancel can't later flip a
        // real failure into a clean one (the race the review flagged).
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(loop_exit(&cancelled, "sync"), None);
    }

    #[test]
    fn kubernetes_and_local_start_environment_local_command_relays() {
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            assert!(should_run_commands_loop(platform, false));
            assert!(!should_run_commands_loop(platform, true));
        }
        for platform in [Platform::Machines, Platform::Test] {
            assert!(!should_run_commands_loop(platform, false));
        }
    }
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
