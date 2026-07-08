//! Local daemon supervision.
//!
//! Daemons run as a direct child of the supervisor with no runtime wrapper: the
//! app binary is the main process, and this module owns spawning it, capturing
//! its stdout/stderr for log export, and applying restart/health to the process
//! directly. This is a self-contained unit with no dependence on the embedded
//! worker runtime that [`crate::worker_manager`] hosts for the Worker path.

use crate::error::{ErrorData, Result};
use crate::worker_manager::{LocalWorkerManager, WorkerMetadata};
use alien_error::{AlienError, Context, ContextError as _, IntoAlienError};
use alien_worker_runtime::{LogExporter, OwnedOtlpLogger};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Runtime state for a supervised daemon (ephemeral).
#[derive(Debug)]
pub(crate) struct DaemonRuntime {
    /// Tokio task handle supervising the daemon's app process (returns our local Result type).
    /// The supervised process is the app binary itself, spawned as a direct child of this
    /// supervisor — there is no runtime wrapper between the supervisor and the app.
    pub(crate) task_handle: JoinHandle<crate::error::Result<()>>,
    /// Shutdown channel sender
    pub(crate) shutdown_tx: tokio::sync::broadcast::Sender<()>,
    /// OS process id of the supervised app process (for heartbeats / process-tree checks).
    pub(crate) pid: Option<u32>,
    /// When the daemon was started (used for monitoring)
    #[allow(dead_code)]
    started_at: chrono::DateTime<chrono::Utc>,
    /// Persistent metadata for this daemon (used for crash recovery)
    pub(crate) metadata: WorkerMetadata,
    /// This daemon's own OTLP log exporter, if configured. Owned per-daemon (not the
    /// process-global provider) so each daemon keeps its own endpoint/service identity and its
    /// own flush lifecycle. Held here to keep the provider alive for the daemon's lifetime.
    #[allow(dead_code)]
    otlp_logger: Option<Arc<OwnedOtlpLogger>>,
}

impl LocalWorkerManager {
    /// Starts a daemon under direct local supervision.
    ///
    /// The daemon's app binary is spawned as the MAIN process — a direct child of this
    /// supervisor with no runtime wrapper. There is no gRPC bindings/control server, no
    /// `ALIEN_TRANSPORT`, no `ALIEN_WORKER_GRPC_ADDRESS`, and no `ALIEN_SECRETS` marker in the
    /// child environment: the controller resolves bindings and secrets into plain env vars
    /// before start, and a command-enabled daemon runs its own app-owned receiver from the
    /// injected `ALIEN_COMMANDS_*` config. The supervisor captures the child's stdout/stderr for
    /// log export itself, and applies restart/health to the app process directly.
    pub async fn start_daemon(
        &self,
        id: &str,
        env_vars: HashMap<String, String>,
        runtime_only_binding_names: Vec<String>,
    ) -> Result<()> {
        Self::start_daemon_internal(
            id,
            env_vars,
            runtime_only_binding_names,
            &self.state_dir,
            &self.daemons,
            self.bindings_provider.clone(),
        )
        .await
    }

    /// Internal static implementation of start_daemon for use by background task.
    pub(crate) async fn start_daemon_internal(
        id: &str,
        env_vars: HashMap<String, String>,
        runtime_only_binding_names: Vec<String>,
        state_dir: &PathBuf,
        daemons: &Arc<Mutex<HashMap<String, DaemonRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        {
            let daemons_guard = daemons.lock().await;
            if daemons_guard.contains_key(id) {
                debug!(daemon_id = %id, "Daemon already running");
                return Ok(());
            }
        }

        let extracted_dir = state_dir.join("daemons").join(id);
        let metadata_file = extracted_dir.join("metadata.json");
        if !metadata_file.exists() {
            return Err(AlienError::new(ErrorData::Other {
                message: format!(
                    "Daemon metadata not found at {}. Run extract_daemon_image first.",
                    metadata_file.display()
                ),
            }));
        }

        let metadata_contents = std::fs::read_to_string(&metadata_file)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to read metadata file: {}", metadata_file.display()),
            })?;

        let existing_metadata: WorkerMetadata = serde_json::from_str(&metadata_contents)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to parse daemon metadata".to_string(),
            })?;

        let working_dir = if let Some(ref oci_working_dir) = existing_metadata.working_dir {
            let relative_path = oci_working_dir.trim_start_matches('/');
            extracted_dir
                .join(relative_path)
                .to_string_lossy()
                .to_string()
        } else {
            extracted_dir.to_string_lossy().to_string()
        };

        // Re-resolve each secret live (kept out of persisted metadata; see plan_worker_launch).
        let mut resolved_bindings = Vec::new();
        for name in &runtime_only_binding_names {
            if let Some(entry) = bindings_provider
                .resolve_runtime_only_binding_env(name)
                .await
                .context(ErrorData::Other {
                    message: format!("Failed to resolve runtime-only binding '{}'", name),
                })?
            {
                resolved_bindings.push((name.clone(), entry));
            }
        }
        let (updated_metadata, runtime_env_vars) = Self::plan_worker_launch(
            id,
            &extracted_dir,
            &existing_metadata,
            None,
            env_vars,
            runtime_only_binding_names,
            &resolved_bindings,
        );

        let log_exporter = log_exporter_from_env(&runtime_env_vars, id);

        Self::save_daemon_metadata_static(state_dir, &updated_metadata)?;

        // Export the daemon's captured logs over OTLP when an endpoint is configured. Each daemon
        // owns its own exporter (not the process-global provider), so concurrent daemons keep
        // distinct log identities and one daemon's shutdown flushes only its own logs.
        let otlp_logger = match log_exporter.to_otlp_config() {
            Some(otlp_config) => Some(Arc::new(
                OwnedOtlpLogger::from_config(otlp_config).context(ErrorData::Other {
                    message: format!("Failed to initialize daemon log export for '{}'", id),
                })?,
            )),
            None => None,
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

        // Spawn the app binary directly as the main process.
        let mut child = spawn_daemon_child(
            id,
            &existing_metadata.runtime_command,
            &working_dir,
            &runtime_env_vars,
        )?;
        let pid = child.id();

        // Capture stdout/stderr for log export (responsibility of the supervisor, not a wrapper).
        if let Some(stdout) = child.stdout.take() {
            let logger = otlp_logger.clone();
            let daemon_id = id.to_string();
            tokio::spawn(
                async move { stream_daemon_output(stdout, true, logger, daemon_id).await },
            );
        }
        if let Some(stderr) = child.stderr.take() {
            let logger = otlp_logger.clone();
            let daemon_id = id.to_string();
            tokio::spawn(
                async move { stream_daemon_output(stderr, false, logger, daemon_id).await },
            );
        }

        let supervised_id = id.to_string();
        let supervisor_logger = otlp_logger.clone();
        let runtime_task: JoinHandle<crate::error::Result<()>> = tokio::spawn(async move {
            supervise_daemon_process(supervised_id, child, shutdown_rx, supervisor_logger).await
        });

        // Give an immediately-failing process a chance to surface its exit before we report success.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if runtime_task.is_finished() {
            match runtime_task.await {
                Ok(Ok(())) => {
                    return Err(AlienError::new(ErrorData::Other {
                        message: format!("Daemon '{}' process exited during startup", id),
                    }));
                }
                Ok(Err(e)) => {
                    return Err(e.context(ErrorData::Other {
                        message: format!("Daemon '{}' process failed during startup", id),
                    }));
                }
                Err(e) => {
                    return Err(AlienError::new(ErrorData::Other {
                        message: format!("Daemon supervision task for '{}' panicked: {}", id, e),
                    }));
                }
            }
        }

        let mut daemons_mut = daemons.lock().await;
        daemons_mut.insert(
            id.to_string(),
            DaemonRuntime {
                task_handle: runtime_task,
                shutdown_tx,
                pid,
                started_at: chrono::Utc::now(),
                metadata: updated_metadata,
                otlp_logger,
            },
        );

        info!(daemon_id = %id, pid = ?pid, "Daemon started (direct supervision, no runtime wrapper)");

        Ok(())
    }
}

/// Builds a log-export sink from a resolved environment. An OTLP logs endpoint (with optional
/// headers) turns on OTLP export; without one, captured output is only echoed locally. Shared by
/// the daemon supervisor and the embedded worker path.
pub(crate) fn log_exporter_from_env(env_vars: &HashMap<String, String>, id: &str) -> LogExporter {
    let Some(endpoint) = env_vars.get("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT") else {
        return LogExporter::None;
    };

    let mut headers = HashMap::new();
    if let Some(headers_str) = env_vars.get("OTEL_EXPORTER_OTLP_HEADERS") {
        for header in headers_str.split(',') {
            if let Some((key, value)) = header.split_once('=') {
                headers.insert(key.trim().to_lowercase(), value.trim().to_string());
            }
        }
    }

    let service_name = env_vars
        .get("OTEL_SERVICE_NAME")
        .cloned()
        .unwrap_or_else(|| id.to_string());

    LogExporter::Otlp {
        endpoint: endpoint.clone(),
        headers,
        service_name,
    }
}

/// Spawns the daemon's app binary as a direct child of the supervisor.
///
/// This is a plain process launch — no runtime wrapper, no injected `ALIEN_TRANSPORT` /
/// `ALIEN_WORKER_GRPC_ADDRESS`. The env passed here is exactly what the app sees, minus the
/// vault-load markers (`ALIEN_SECRETS` / `ALIEN_RUNTIME_SECRETS`) which the app must never receive
/// on the local platform: bindings and secrets are already resolved into plain env vars upstream.
fn spawn_daemon_child(
    id: &str,
    command: &[String],
    working_dir: &str,
    env_vars: &HashMap<String, String>,
) -> Result<tokio::process::Child> {
    if command.is_empty() {
        return Err(AlienError::new(ErrorData::Other {
            message: format!("Daemon '{}' has an empty entrypoint command", id),
        }));
    }

    // Resolve a relative program path against the working dir. Windows' CreateProcessW does not
    // resolve the executable relative to `current_dir`, only relative to the parent's cwd, so an
    // absolute path is required there; joining is harmless on Unix.
    let working_dir_path = PathBuf::from(working_dir);
    let program = {
        let raw = std::path::Path::new(&command[0]);
        if raw.is_relative() {
            working_dir_path.join(raw).to_string_lossy().to_string()
        } else {
            command[0].clone()
        }
    };

    let mut cmd = Command::new(&program);
    if command.len() > 1 {
        cmd.args(&command[1..]);
    }
    cmd.current_dir(&working_dir_path);

    for (key, value) in env_vars {
        if key == alien_core::ENV_ALIEN_SECRETS || key == alien_core::ENV_ALIEN_RUNTIME_SECRETS {
            continue;
        }
        cmd.env(key, value);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    cmd.spawn().into_alien_error().context(ErrorData::Other {
        message: format!("Failed to spawn daemon '{}' process ({})", id, program),
    })
}

/// Streams a captured stdout/stderr pipe: echoes each line locally and, when this daemon has an
/// OTLP exporter, emits it through the daemon's own exporter.
async fn stream_daemon_output(
    output: impl AsyncRead + Unpin,
    is_stdout: bool,
    otlp_logger: Option<Arc<OwnedOtlpLogger>>,
    daemon_id: String,
) {
    let stream_name = if is_stdout { "stdout" } else { "stderr" };
    let mut lines = BufReader::new(output).lines();

    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if is_stdout {
                    println!("{}", line);
                } else {
                    eprintln!("{}", line);
                }
                if let Some(logger) = &otlp_logger {
                    let timestamp_nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
                    logger.emit_log(stream_name, &line, timestamp_nanos);
                }
            }
            Ok(None) => break,
            Err(e) => {
                warn!(daemon_id = %daemon_id, stream = stream_name, error = %e, "Error reading daemon output");
                break;
            }
        }
    }
}

/// Supervises the daemon's app process: waits for either a shutdown signal or the process exit.
///
/// On shutdown the child is terminated and this daemon's own OTLP logs are flushed. A non-zero exit
/// (or a wait error) is returned as an error so the monitor loop treats it as a crash and restarts.
/// A clean exit returns `Ok(())`; the monitor still restarts it, since a daemon is expected to run
/// forever.
async fn supervise_daemon_process(
    daemon_id: String,
    mut child: tokio::process::Child,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    otlp_logger: Option<Arc<OwnedOtlpLogger>>,
) -> Result<()> {
    let result = tokio::select! {
        status = child.wait() => {
            match status {
                Ok(status) if status.success() => {
                    info!(daemon_id = %daemon_id, "Daemon process exited cleanly");
                    Ok(())
                }
                Ok(status) => {
                    let code = status.code().unwrap_or(-1);
                    Err(AlienError::new(ErrorData::LocalProcessError {
                        process_id: daemon_id.clone(),
                        operation: "run".to_string(),
                        reason: format!("Daemon process exited with code {}", code),
                    }))
                }
                Err(e) => Err(e).into_alien_error().context(ErrorData::LocalProcessError {
                    process_id: daemon_id.clone(),
                    operation: "wait".to_string(),
                    reason: "Failed to wait for daemon process".to_string(),
                }),
            }
        }
        _ = shutdown_rx.recv() => {
            info!(daemon_id = %daemon_id, "Daemon shutdown signal received; terminating process");
            if let Err(e) = child.kill().await {
                warn!(daemon_id = %daemon_id, error = %e, "Failed to kill daemon process");
            }
            Ok(())
        }
    };

    // Flush on EVERY exit path, not just shutdown. On an exit/crash the child
    // produced its final buffered log batch exactly when operators most need
    // it; returning without flushing (as the old crash path did) drops it. A
    // single flush here covers both the shutdown and the child-exit/crash arms.
    if let Some(logger) = &otlp_logger {
        if let Err(e) = logger.flush().await {
            warn!(daemon_id = %daemon_id, error = %e, "Failed to flush daemon logs");
        }
    }

    result
}
