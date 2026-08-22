//! Local daemon supervision.
//!
//! Daemons run as a direct child of the supervisor with no runtime wrapper: the
//! app binary is the main process, and this module owns spawning it, capturing
//! its stdout/stderr for log export, and applying restart/health to the process
//! directly. This is a self-contained unit with no dependence on the embedded
//! worker runtime that [`crate::worker_manager`] hosts for the Worker path.

use crate::error::{ErrorData, Result};
use crate::worker_manager::{LocalWorkerManager, RuntimeOnlyBindingRef, WorkerMetadata};
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
    /// This daemon's own OTLP log exporter, if configured. Owned per-daemon (not the
    /// process-global provider) so each daemon keeps its own endpoint/service identity and its
    /// own flush lifecycle. Held here to keep the provider alive for the daemon's lifetime.
    #[allow(dead_code)]
    otlp_logger: Option<Arc<OwnedOtlpLogger>>,
}

/// Launch-time options for a supervised daemon, beyond its resolved env.
///
/// Every field follows the persist-on-start rule: values flow into the
/// daemon's on-disk metadata (or, for the runtime-only lists, only their
/// NAMES do), so monitor restarts and cold recovery see the same launch
/// shape without the caller re-supplying it.
#[derive(Debug, Default, Clone)]
pub struct DaemonLaunchOptions {
    /// Linked resources whose binding is a runtime-only secret (a local
    /// Postgres password or a local BYO-key AI binding): re-resolved live at
    /// every start by resource type, never persisted.
    pub runtime_only_bindings: Vec<RuntimeOnlyBindingRef>,
    /// Env var NAMES whose resolved values are deployment secrets (including
    /// the receiver's `ALIEN_COMMANDS_TOKEN`): delivered to the process but
    /// stripped from the persisted metadata. The in-memory runtime keeps the
    /// live values so crash restarts work; cold recovery defers to the
    /// controller, which re-resolves them fresh.
    pub runtime_only_env_names: Vec<String>,
    /// The Daemon config's `command` (image entrypoint override).
    pub command_override: Option<Vec<String>>,
    /// Stop grace period: SIGTERM, this window to drain, then SIGKILL.
    pub stop_grace_period_seconds: Option<u32>,
}

impl LocalWorkerManager {
    /// Starts a daemon under direct local supervision.
    ///
    /// The daemon's app binary is spawned as the MAIN process — a direct child of this
    /// supervisor with no runtime wrapper. There is no Worker app protocol/control server, no
    /// `ALIEN_TRANSPORT`, no `ALIEN_WORKER_GRPC_ADDRESS`, and no `ALIEN_SECRETS` marker in the
    /// child environment: the controller resolves bindings and secrets into plain env vars
    /// before start, and a command-enabled daemon runs its own app-owned receiver from the
    /// injected `ALIEN_COMMANDS_*` config. The supervisor captures the child's stdout/stderr for
    /// log export itself, and applies restart/health to the app process directly.
    /// See [`DaemonLaunchOptions`] for the launch shape (entrypoint override,
    /// stop grace, and the two runtime-only lists).
    pub async fn start_daemon(
        &self,
        id: &str,
        env_vars: HashMap<String, String>,
        options: DaemonLaunchOptions,
    ) -> Result<()> {
        Self::start_daemon_internal(
            id,
            env_vars,
            options,
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
        options: DaemonLaunchOptions,
        state_dir: &PathBuf,
        daemons: &Arc<Mutex<HashMap<String, DaemonRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        // Hold the map lock for the WHOLE start, not just the contains_key
        // guard: with separate acquisitions, two concurrent callers (the
        // crash monitor racing a controller start) both pass the guard and
        // spawn two app processes, with only the second insert's runtime
        // tracked — the orphan keeps running (double log export, duplicate
        // command receiver) with no handle to stop it. The lock is a tokio
        // Mutex, so holding it across the awaits below is safe; daemon
        // starts are rare and brief, so the serialization is cheap.
        let mut daemons_guard = daemons.lock().await;
        if daemons_guard.contains_key(id) {
            debug!(daemon_id = %id, "Daemon already running");
            return Ok(());
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

        let mut existing_metadata: WorkerMetadata = serde_json::from_str(&metadata_contents)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to parse daemon metadata".to_string(),
            })?;

        // Apply the Daemon config's entrypoint override over the
        // OCI-extracted command. Flows into the persisted metadata below, so
        // a monitor restart (override = None) keeps running the same command.
        if let Some(command) = options.command_override {
            existing_metadata.runtime_command = command;
        }
        // Same persist-on-start rule for the stop grace period and the
        // runtime-only env names (monitor restarts and recovery re-read them
        // from metadata).
        if options.stop_grace_period_seconds.is_some() {
            existing_metadata.stop_grace_period_seconds = options.stop_grace_period_seconds;
        }
        if !options.runtime_only_env_names.is_empty() {
            existing_metadata.runtime_only_env_names = options.runtime_only_env_names.clone();
        }

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
        for binding in &options.runtime_only_bindings {
            if let Some(entry) = bindings_provider
                .resolve_runtime_only_binding_env(&binding.name, &binding.resource_type)
                .await
                .context(ErrorData::Other {
                    message: format!("Failed to resolve runtime-only binding '{}'", binding.name),
                })?
            {
                resolved_bindings.push((binding.name.clone(), entry));
            }
        }
        let (updated_metadata, runtime_env_vars) = Self::plan_worker_launch(
            id,
            &extracted_dir,
            &existing_metadata,
            None,
            env_vars,
            options.runtime_only_bindings,
            &existing_metadata.runtime_only_env_names.clone(),
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
        let stop_grace_period = std::time::Duration::from_secs(u64::from(
            updated_metadata
                .stop_grace_period_seconds
                .unwrap_or(DEFAULT_STOP_GRACE_PERIOD_SECONDS),
        ));
        let runtime_task: JoinHandle<crate::error::Result<()>> = tokio::spawn(async move {
            supervise_daemon_process(
                supervised_id,
                child,
                shutdown_rx,
                supervisor_logger,
                stop_grace_period,
            )
            .await
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

        daemons_guard.insert(
            id.to_string(),
            DaemonRuntime {
                task_handle: runtime_task,
                shutdown_tx,
                pid,
                started_at: chrono::Utc::now(),
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

    // Own process group (unix): entrypoints that spawn children (`sh -c`,
    // npm, gunicorn) must have their WHOLE tree signaled on stop — signaling
    // only the direct child leaves grandchildren holding the port and, worse,
    // a still-leasing command receiver running old code.
    #[cfg(unix)]
    cmd.process_group(0);

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
/// (or a wait error) is returned as an error so the reaper removes it from the map and the controller relaunches it.
/// A clean exit returns `Ok(())`; the monitor still restarts it, since a daemon is expected to run
/// forever.
/// Default stop grace period when the Daemon config does not set one —
/// matches the Kubernetes pod default.
const DEFAULT_STOP_GRACE_PERIOD_SECONDS: u32 = 30;

async fn supervise_daemon_process(
    daemon_id: String,
    mut child: tokio::process::Child,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    otlp_logger: Option<Arc<OwnedOtlpLogger>>,
    stop_grace_period: std::time::Duration,
) -> Result<()> {
    // Captured before the waits: after the child exits, `child.id()` is
    // None, and the crash arm below still needs the group id to sweep
    // surviving grandchildren.
    #[cfg(unix)]
    let group_pid = child.id();

    let result = tokio::select! {
        status = child.wait() => {
            // The direct child is gone, but an entrypoint's own children
            // (sh -c, npm, gunicorn workers) may have survived it — holding
            // the port and, worse, still leasing commands with old code.
            // Sweep the process group the child led; ESRCH (nothing left)
            // is the normal case.
            #[cfg(unix)]
            if let Some(pid) = group_pid {
                unsafe {
                    let _ = libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
                }
            }
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
            // Graceful stop: SIGTERM, then the configured drain window, then
            // SIGKILL. An immediate kill would cut in-flight command
            // executions and buffered writes with zero notice — the config
            // accepts stop_grace_period_seconds, so honor it (the K8s daemon
            // controller maps the same field to terminationGracePeriodSeconds).
            info!(
                daemon_id = %daemon_id,
                grace_seconds = stop_grace_period.as_secs(),
                "Daemon shutdown signal received; sending SIGTERM"
            );
            match child.id() {
                #[cfg(unix)]
                Some(pid) => {
                    // Signal the PROCESS GROUP (negative pid): the child was
                    // spawned with process_group(0), so this reaches its
                    // whole tree, not just the direct child.
                    // SAFETY: plain kill(2) on the group we created; no
                    // memory access. A failure (e.g. already exited) is
                    // handled by the wait/kill fallback below.
                    let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGTERM) };
                    if rc != 0 {
                        warn!(daemon_id = %daemon_id, "Failed to send SIGTERM to daemon process group");
                    }
                    match tokio::time::timeout(stop_grace_period, child.wait()).await {
                        Ok(_) => {
                            info!(daemon_id = %daemon_id, "Daemon process exited within the grace period");
                        }
                        Err(_) => {
                            warn!(daemon_id = %daemon_id, "Daemon did not exit within the grace period; sending SIGKILL");
                            let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
                            if rc != 0 {
                                warn!(daemon_id = %daemon_id, "Failed to SIGKILL daemon process group");
                            }
                            let _ = child.wait().await;
                        }
                    }
                }
                // Windows has no SIGTERM to offer a drain window; hard-kill
                // as before. (The local platform is unix-first; this arm only
                // keeps the Windows CLI build compiling.)
                #[cfg(not(unix))]
                Some(_pid) => {
                    if let Err(e) = child.kill().await {
                        warn!(daemon_id = %daemon_id, error = %e, "Failed to kill daemon process");
                    }
                }
                // No pid means the child already exited; reap it.
                None => {
                    let _ = child.wait().await;
                }
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
