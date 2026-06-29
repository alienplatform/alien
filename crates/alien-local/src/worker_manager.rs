use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, ContextError as _, IntoAlienError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Manager for local worker resources.
///
/// Spawns and manages worker runtime processes. Workers run as separate processes
/// so their logs can be captured and streamed with resource ID prefixes.
///
/// This manager maintains persistent state and provides auto-recovery:
/// - Worker metadata is saved to disk for crash recovery
/// - Background task monitors health and auto-recovers crashed workers
/// - Graceful shutdown via shared signal
///
/// # State Scoping
/// Worker state is stored under `{state_dir}/workers/{worker_id}/`:
/// - `metadata.json` - Recovery metadata for auto-recovery
/// - Other files - Extracted OCI image contents
///
/// The `state_dir` should be scoped by agent ID (e.g., `~/.alien-cli/<agent_id>`)
/// to avoid conflicts between agents.
#[derive(Debug)]
pub struct LocalWorkerManager {
    /// Base directory for all local platform state
    state_dir: PathBuf,
    /// Map of worker ID to runtime state (ephemeral)
    workers: Arc<Mutex<HashMap<String, WorkerRuntime>>>,
    /// Map of daemon ID to runtime state (ephemeral)
    daemons: Arc<Mutex<HashMap<String, DaemonRuntime>>>,
    /// Bindings provider for worker runtimes
    bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
}

#[derive(Debug)]
struct WorkerRuntime {
    /// Tokio task handle for the worker (returns our local Result type)
    task_handle: JoinHandle<crate::error::Result<()>>,
    /// Shutdown channel sender
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    /// URL where the worker is accessible
    worker_url: String,
    /// When the worker was started (used for monitoring)
    #[allow(dead_code)]
    started_at: chrono::DateTime<chrono::Utc>,
    /// Persistent metadata for this worker (used for crash recovery)
    #[allow(dead_code)]
    metadata: WorkerMetadata,
}

#[derive(Debug)]
struct DaemonRuntime {
    /// Tokio task handle for the daemon (returns our local Result type)
    task_handle: JoinHandle<crate::error::Result<()>>,
    /// Shutdown channel sender
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    /// When the daemon was started (used for monitoring)
    #[allow(dead_code)]
    started_at: chrono::DateTime<chrono::Utc>,
    /// Persistent metadata for this daemon (used for crash recovery)
    #[allow(dead_code)]
    metadata: WorkerMetadata,
}

/// Persistent metadata for a worker (saved to disk)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkerMetadata {
    /// Worker identifier
    worker_id: String,
    /// Path to the extracted OCI image
    extracted_path: PathBuf,
    /// Environment variables for the worker
    env_vars: HashMap<String, String>,
    /// Runtime command from OCI image config (ENTRYPOINT + CMD)
    runtime_command: Vec<String>,
    /// Working directory from OCI image config
    working_dir: Option<String>,
    /// Transport port for the runtime (persisted to enable transparent recovery)
    #[serde(default)]
    transport_port: Option<u16>,
}

impl LocalWorkerManager {
    /// Creates a new worker manager with shared shutdown signal.
    ///
    /// # Arguments
    /// * `state_dir` - Base directory for all local platform state
    /// * `bindings_provider` - Bindings provider for worker runtimes
    /// * `shutdown_rx` - Shutdown signal receiver (shared across all services)
    ///
    /// # Returns
    /// (Manager, Optional JoinHandle for background task)
    pub fn new_with_shutdown(
        state_dir: PathBuf,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) -> (Self, Option<tokio::task::JoinHandle<()>>) {
        let workers = Arc::new(Mutex::new(HashMap::new()));
        let daemons = Arc::new(Mutex::new(HashMap::new()));

        // Spawn background task for health monitoring and auto-recovery
        let state_dir_clone = state_dir.clone();
        let workers_clone = workers.clone();
        let daemons_clone = daemons.clone();
        let bindings_provider_clone = bindings_provider.clone();
        let background_task = tokio::spawn(async move {
            Self::monitor_and_recover_loop(
                state_dir_clone,
                workers_clone,
                daemons_clone,
                bindings_provider_clone,
                shutdown_rx,
            )
            .await;
        });

        let manager = Self {
            state_dir,
            workers,
            daemons,
            bindings_provider,
        };

        (manager, Some(background_task))
    }

    /// Background loop that monitors worker health and handles auto-recovery
    async fn monitor_and_recover_loop(
        state_dir: PathBuf,
        workers: Arc<Mutex<HashMap<String, WorkerRuntime>>>,
        daemons: Arc<Mutex<HashMap<String, DaemonRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) {
        // First, attempt recovery of workers from previous run
        if let Err(e) =
            Self::recover_all_workers(&state_dir, &workers, bindings_provider.clone()).await
        {
            warn!("Failed to recover workers from metadata: {:?}", e);
        }
        if let Err(e) =
            Self::recover_all_daemons(&state_dir, &daemons, bindings_provider.clone()).await
        {
            warn!("Failed to recover daemons from metadata: {:?}", e);
        }

        // Then monitor health and auto-restart crashed workers
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Worker manager shutting down");
                    break;
                }
                _ = interval.tick() => {
                    if let Err(e) = Self::monitor_and_restart(&state_dir, &workers, bindings_provider.clone()).await {
                        warn!("Worker health check failed: {:?}", e);
                    }
                    if let Err(e) = Self::monitor_and_restart_daemons(&state_dir, &daemons, bindings_provider.clone()).await {
                        warn!("Daemon health check failed: {:?}", e);
                    }
                }
            }
        }
    }

    /// Recovers all workers from metadata files
    async fn recover_all_workers(
        state_dir: &PathBuf,
        workers: &Arc<Mutex<HashMap<String, WorkerRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        let workers_dir = state_dir.join("workers");
        if !workers_dir.exists() {
            return Ok(());
        }

        let entries = fs::read_dir(&workers_dir)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to read workers directory".to_string(),
            })?;

        for entry in entries {
            let entry = entry.into_alien_error().context(ErrorData::Other {
                message: "Failed to read worker entry".to_string(),
            })?;

            // Check if this is a directory (each worker has its own directory)
            if entry.path().is_dir() {
                let metadata_file = entry.path().join("metadata.json");
                if metadata_file.exists() {
                    if let Err(e) = Self::recover_single_worker(
                        &metadata_file,
                        state_dir,
                        workers,
                        bindings_provider.clone(),
                    )
                    .await
                    {
                        warn!("Failed to recover worker from {:?}: {:?}", metadata_file, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Recovers a single worker from metadata file
    async fn recover_single_worker(
        metadata_path: &PathBuf,
        state_dir: &PathBuf,
        workers: &Arc<Mutex<HashMap<String, WorkerRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        let contents = tokio::fs::read_to_string(metadata_path)
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to read {}", metadata_path.display()),
            })?;

        let metadata: WorkerMetadata =
            serde_json::from_str(&contents)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Failed to parse worker metadata".to_string(),
                })?;

        // Check if already running
        {
            let workers_guard = workers.lock().await;
            if workers_guard.contains_key(&metadata.worker_id) {
                debug!(worker_id = %metadata.worker_id, "Worker already running, skipping recovery");
                return Ok(());
            }
        }

        info!(worker_id = %metadata.worker_id, "Recovering worker from previous run");

        // Restart the worker using metadata
        Self::start_worker_internal(
            &metadata.worker_id,
            metadata.env_vars,
            state_dir,
            workers,
            bindings_provider,
        )
        .await?;

        Ok(())
    }

    /// Recovers all daemons from metadata files.
    async fn recover_all_daemons(
        state_dir: &PathBuf,
        daemons: &Arc<Mutex<HashMap<String, DaemonRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        let daemons_dir = state_dir.join("daemons");
        if !daemons_dir.exists() {
            return Ok(());
        }

        let entries = fs::read_dir(&daemons_dir)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to read daemons directory".to_string(),
            })?;

        for entry in entries {
            let entry = entry.into_alien_error().context(ErrorData::Other {
                message: "Failed to read daemon entry".to_string(),
            })?;

            if entry.path().is_dir() {
                let metadata_file = entry.path().join("metadata.json");
                if metadata_file.exists() {
                    if let Err(e) = Self::recover_single_daemon(
                        &metadata_file,
                        state_dir,
                        daemons,
                        bindings_provider.clone(),
                    )
                    .await
                    {
                        warn!("Failed to recover daemon from {:?}: {:?}", metadata_file, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Recovers a single daemon from metadata file.
    async fn recover_single_daemon(
        metadata_path: &PathBuf,
        state_dir: &PathBuf,
        daemons: &Arc<Mutex<HashMap<String, DaemonRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        let contents = tokio::fs::read_to_string(metadata_path)
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to read {}", metadata_path.display()),
            })?;

        let metadata: WorkerMetadata =
            serde_json::from_str(&contents)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Failed to parse daemon metadata".to_string(),
                })?;

        {
            let daemons_guard = daemons.lock().await;
            if daemons_guard.contains_key(&metadata.worker_id) {
                debug!(daemon_id = %metadata.worker_id, "Daemon already running, skipping recovery");
                return Ok(());
            }
        }

        info!(daemon_id = %metadata.worker_id, "Recovering daemon from previous run");

        Self::start_daemon_internal(
            &metadata.worker_id,
            metadata.env_vars,
            state_dir,
            daemons,
            bindings_provider,
        )
        .await?;

        Ok(())
    }

    /// Monitors running workers and restarts crashed ones
    async fn monitor_and_restart(
        state_dir: &PathBuf,
        workers: &Arc<Mutex<HashMap<String, WorkerRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        let worker_ids: Vec<String> = {
            let workers_guard = workers.lock().await;
            workers_guard.keys().cloned().collect()
        };

        for worker_id in worker_ids {
            let (metadata, task_result) = {
                let mut workers_mut = workers.lock().await;
                if let Some(runtime) = workers_mut.get(&worker_id) {
                    if runtime.task_handle.is_finished() {
                        // Worker crashed - remove and get metadata + task result
                        let mut runtime = workers_mut.remove(&worker_id).unwrap();
                        let task_result = (&mut runtime.task_handle).await;
                        (Some(runtime.metadata.clone()), Some(task_result))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            };

            if let Some(metadata) = metadata {
                // Log the crash reason if available
                if let Some(task_result) = task_result {
                    match task_result {
                        Ok(Ok(())) => {
                            warn!(worker_id = %worker_id, "Worker exited cleanly but unexpectedly");
                        }
                        Ok(Err(e)) => {
                            warn!(worker_id = %worker_id, error = ?e, "Worker crashed with error");
                        }
                        Err(e) => {
                            warn!(worker_id = %worker_id, error = ?e, "Worker task panicked");
                        }
                    }
                }

                warn!(worker_id = %worker_id, "Auto-restarting worker...");

                // Restart using metadata
                if let Err(e) = Self::start_worker_internal(
                    &metadata.worker_id,
                    metadata.env_vars,
                    state_dir,
                    workers,
                    bindings_provider.clone(),
                )
                .await
                {
                    warn!(worker_id = %worker_id, error = ?e, "Failed to restart");
                } else {
                    info!(worker_id = %worker_id, "Successfully restarted after crash");
                }
            }
        }

        Ok(())
    }

    /// Monitors running daemons and restarts crashed ones.
    async fn monitor_and_restart_daemons(
        state_dir: &PathBuf,
        daemons: &Arc<Mutex<HashMap<String, DaemonRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        let daemon_ids: Vec<String> = {
            let daemons_guard = daemons.lock().await;
            daemons_guard.keys().cloned().collect()
        };

        for daemon_id in daemon_ids {
            let (metadata, task_result) = {
                let mut daemons_mut = daemons.lock().await;
                if let Some(runtime) = daemons_mut.get(&daemon_id) {
                    if runtime.task_handle.is_finished() {
                        let mut runtime = daemons_mut.remove(&daemon_id).unwrap();
                        let task_result = (&mut runtime.task_handle).await;
                        (Some(runtime.metadata.clone()), Some(task_result))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            };

            if let Some(metadata) = metadata {
                if let Some(task_result) = task_result {
                    match task_result {
                        Ok(Ok(())) => {
                            warn!(daemon_id = %daemon_id, "Daemon exited cleanly but unexpectedly");
                        }
                        Ok(Err(e)) => {
                            warn!(daemon_id = %daemon_id, error = ?e, "Daemon crashed with error");
                        }
                        Err(e) => {
                            warn!(daemon_id = %daemon_id, error = ?e, "Daemon task panicked");
                        }
                    }
                }

                warn!(daemon_id = %daemon_id, "Auto-restarting daemon...");

                if let Err(e) = Self::start_daemon_internal(
                    &metadata.worker_id,
                    metadata.env_vars,
                    state_dir,
                    daemons,
                    bindings_provider.clone(),
                )
                .await
                {
                    warn!(daemon_id = %daemon_id, error = ?e, "Failed to restart daemon");
                } else {
                    info!(daemon_id = %daemon_id, "Successfully restarted daemon after crash");
                }
            }
        }

        Ok(())
    }

    /// Saves worker metadata to disk (static for use by background task)
    fn save_metadata_static(state_dir: &PathBuf, metadata: &WorkerMetadata) -> Result<()> {
        Self::save_metadata_in_namespace(state_dir, "workers", metadata)
    }

    /// Saves daemon metadata to disk.
    fn save_daemon_metadata_static(state_dir: &PathBuf, metadata: &WorkerMetadata) -> Result<()> {
        Self::save_metadata_in_namespace(state_dir, "daemons", metadata)
    }

    fn save_metadata_in_namespace(
        state_dir: &PathBuf,
        namespace: &str,
        metadata: &WorkerMetadata,
    ) -> Result<()> {
        let worker_dir = state_dir.join(namespace).join(&metadata.worker_id);
        fs::create_dir_all(&worker_dir)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to create {} directory", namespace),
            })?;

        let metadata_file = worker_dir.join("metadata.json");
        let contents = serde_json::to_string_pretty(metadata)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to serialize worker metadata".to_string(),
            })?;

        fs::write(&metadata_file, contents)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to write metadata file: {}", metadata_file.display()),
            })?;

        Ok(())
    }

    /// Deletes worker metadata from disk (static for use by background task)
    fn delete_metadata_static(state_dir: &PathBuf, worker_id: &str) -> Result<()> {
        Self::delete_metadata_in_namespace(state_dir, "workers", worker_id)
    }

    /// Deletes daemon metadata from disk.
    fn delete_daemon_metadata_static(state_dir: &PathBuf, daemon_id: &str) -> Result<()> {
        Self::delete_metadata_in_namespace(state_dir, "daemons", daemon_id)
    }

    fn delete_metadata_in_namespace(
        state_dir: &PathBuf,
        namespace: &str,
        resource_id: &str,
    ) -> Result<()> {
        let metadata_file = state_dir
            .join(namespace)
            .join(resource_id)
            .join("metadata.json");

        if metadata_file.exists() {
            fs::remove_file(&metadata_file)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!(
                        "Failed to delete metadata file: {}",
                        metadata_file.display()
                    ),
                })?;
        }

        Ok(())
    }

    /// Starts a worker runtime.
    ///
    /// # Arguments
    /// * `id` - Worker identifier
    /// * `env_vars` - Additional environment variables to pass to the worker
    ///
    /// # Returns
    /// URL where the worker is accessible (e.g., "http://localhost:3000")
    ///
    /// # Note
    /// This is idempotent - safe to call multiple times. Auto-recovery happens here.
    pub async fn start_worker(
        &self,
        id: &str,
        env_vars: HashMap<String, String>,
    ) -> Result<String> {
        Self::start_worker_internal(
            id,
            env_vars,
            &self.state_dir,
            &self.workers,
            self.bindings_provider.clone(),
        )
        .await
    }

    /// Starts a daemon runtime.
    ///
    /// Daemons use passthrough transport: there is no HTTP invocation proxy and no
    /// worker URL. The process is still wrapped by alien-runtime so bindings,
    /// commands polling, tracing, graceful shutdown, and log export behave the
    /// same as other local compute resources.
    pub async fn start_daemon(&self, id: &str, env_vars: HashMap<String, String>) -> Result<()> {
        Self::start_daemon_internal(
            id,
            env_vars,
            &self.state_dir,
            &self.daemons,
            self.bindings_provider.clone(),
        )
        .await
    }

    /// Internal static implementation of start_worker for use by background task
    async fn start_worker_internal(
        id: &str,
        env_vars: HashMap<String, String>,
        state_dir: &PathBuf,
        workers: &Arc<Mutex<HashMap<String, WorkerRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<String> {
        // Check if already running
        {
            let workers_guard = workers.lock().await;
            if let Some(runtime) = workers_guard.get(id) {
                debug!(worker_id = %id, "Worker already running");
                return Ok(runtime.worker_url.clone());
            }
        }

        // Get the extracted directory for this worker
        let extracted_dir = state_dir.join("workers").join(id);

        // Load metadata to get runtime command and saved transport port
        let metadata_file = extracted_dir.join("metadata.json");
        if !metadata_file.exists() {
            return Err(AlienError::new(ErrorData::Other {
                message: format!(
                    "Worker metadata not found at {}. Run extract_image first.",
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
                message: "Failed to parse worker metadata".to_string(),
            })?;

        let saved_port = existing_metadata.transport_port;

        // Allocate port for worker HTTP server (runtime proxy)
        // Try to reuse the saved port first for transparent recovery
        let port = allocate_port_with_preference(saved_port, id)?;

        let worker_url = format!("http://localhost:{}", port);

        // Build runtime config using the command from OCI image config
        // OCI working_dir is relative to container root, translate to host path
        let working_dir = if let Some(ref oci_working_dir) = existing_metadata.working_dir {
            // OCI working dir like "/app" -> extracted_dir + "app"
            let relative_path = oci_working_dir.trim_start_matches('/');
            extracted_dir
                .join(relative_path)
                .to_string_lossy()
                .to_string()
        } else {
            // Default to extracted_dir (root of extracted image)
            extracted_dir.to_string_lossy().to_string()
        };

        // Merge in required environment variables for local platform
        let runtime_env_vars = env_vars.clone();

        // Pick a unique port for this runtime's gRPC server
        let grpc_port = port_check::free_local_ipv4_port().ok_or_else(|| {
            AlienError::new(ErrorData::Other {
                message: "Failed to find free port for gRPC server".to_string(),
            })
        })?;
        let bindings_address = format!("127.0.0.1:{}", grpc_port);

        // Build log exporter configuration
        // For local workers, we extract OTLP config from env_vars and pass directly
        // This allows alien-runtime (running embedded) to send logs via OTLP
        let log_exporter =
            if let Some(endpoint) = runtime_env_vars.get("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT") {
                let mut headers = HashMap::new();
                if let Some(headers_str) = runtime_env_vars.get("OTEL_EXPORTER_OTLP_HEADERS") {
                    for header in headers_str.split(',') {
                        if let Some((key, value)) = header.split_once('=') {
                            headers.insert(key.trim().to_lowercase(), value.trim().to_string());
                        }
                    }
                }

                let service_name = runtime_env_vars
                    .get("OTEL_SERVICE_NAME")
                    .cloned()
                    .unwrap_or_else(|| id.to_string());

                alien_runtime::LogExporter::Otlp {
                    endpoint: endpoint.clone(),
                    headers,
                    service_name,
                }
            } else {
                // No OTLP config - shouldn't happen for workers, but fallback to None
                alien_runtime::LogExporter::None
            };

        let runtime_config = alien_runtime::RuntimeConfig::builder()
            .transport(alien_runtime::TransportType::Local)
            .transport_port(port)
            .bindings_address(bindings_address)
            .command(existing_metadata.runtime_command.clone())
            .working_dir(PathBuf::from(&working_dir))
            .env_vars(runtime_env_vars)
            .log_exporter(log_exporter)
            .build();

        // Update and save metadata with current env_vars and transport port
        let updated_metadata = WorkerMetadata {
            worker_id: id.to_string(),
            extracted_path: extracted_dir.clone(),
            env_vars: env_vars.clone(),
            runtime_command: existing_metadata.runtime_command.clone(),
            working_dir: existing_metadata.working_dir.clone(),
            transport_port: Some(port),
        };
        Self::save_metadata_static(state_dir, &updated_metadata)?;

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

        // Spawn alien_runtime::run in tokio task with custom bindings provider
        let id_clone = id.to_string();
        let runtime_task: JoinHandle<crate::error::Result<()>> = tokio::spawn(async move {
            alien_runtime::run(
                runtime_config,
                shutdown_rx,
                alien_runtime::BindingsSource::Provider(bindings_provider),
            )
            .await
            .context(ErrorData::Other {
                message: format!("Runtime failed for worker '{}'", id_clone),
            })?;

            Ok(())
        });

        // Wait for the HTTP transport to actually be ready. alien-runtime may
        // first wait for app HTTP registration and task subscription before it
        // opens the proxy listener.
        let max_wait = std::time::Duration::from_secs(60);
        let start = std::time::Instant::now();
        let check_interval = std::time::Duration::from_millis(50);

        loop {
            // Try to connect to the transport port
            if std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                std::time::Duration::from_millis(100),
            )
            .is_ok()
            {
                debug!(worker_id = %id, port = port, "Transport is ready");
                break;
            }

            // Check if we've exceeded the timeout
            if start.elapsed() > max_wait {
                return Err(AlienError::new(ErrorData::Other {
                    message: format!(
                        "Worker '{}' transport did not become ready within {:?}",
                        id, max_wait
                    ),
                }));
            }

            // Check if the runtime task has already failed
            if runtime_task.is_finished() {
                // Task finished early, get the error
                match runtime_task.await {
                    Ok(Ok(())) => {
                        return Err(AlienError::new(ErrorData::Other {
                            message: format!(
                                "Runtime for worker '{}' exited before transport was ready",
                                id
                            ),
                        }));
                    }
                    Ok(Err(e)) => {
                        return Err(e.context(ErrorData::Other {
                            message: format!("Runtime for worker '{}' failed during startup", id),
                        }));
                    }
                    Err(e) => {
                        return Err(AlienError::new(ErrorData::Other {
                            message: format!("Runtime task for worker '{}' panicked: {}", id, e),
                        }));
                    }
                }
            }

            // Wait before next check
            tokio::time::sleep(check_interval).await;
        }

        // Track handle
        let mut workers_mut = workers.lock().await;
        workers_mut.insert(
            id.to_string(),
            WorkerRuntime {
                task_handle: runtime_task,
                shutdown_tx,
                worker_url: worker_url.clone(),
                started_at: chrono::Utc::now(),
                metadata: updated_metadata,
            },
        );

        info!(
            worker_id = %id,
            url = %worker_url,
            "Worker runtime started"
        );

        Ok(worker_url)
    }

    /// Internal static implementation of start_daemon for use by background task.
    async fn start_daemon_internal(
        id: &str,
        env_vars: HashMap<String, String>,
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

        let runtime_env_vars = env_vars.clone();
        let grpc_port = port_check::free_local_ipv4_port().ok_or_else(|| {
            AlienError::new(ErrorData::Other {
                message: "Failed to find free port for gRPC server".to_string(),
            })
        })?;
        let bindings_address = format!("127.0.0.1:{}", grpc_port);

        let log_exporter =
            if let Some(endpoint) = runtime_env_vars.get("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT") {
                let mut headers = HashMap::new();
                if let Some(headers_str) = runtime_env_vars.get("OTEL_EXPORTER_OTLP_HEADERS") {
                    for header in headers_str.split(',') {
                        if let Some((key, value)) = header.split_once('=') {
                            headers.insert(key.trim().to_lowercase(), value.trim().to_string());
                        }
                    }
                }

                let service_name = runtime_env_vars
                    .get("OTEL_SERVICE_NAME")
                    .cloned()
                    .unwrap_or_else(|| id.to_string());

                alien_runtime::LogExporter::Otlp {
                    endpoint: endpoint.clone(),
                    headers,
                    service_name,
                }
            } else {
                alien_runtime::LogExporter::None
            };

        let runtime_config = alien_runtime::RuntimeConfig::builder()
            .transport(alien_runtime::TransportType::Passthrough)
            .transport_port(0)
            .bindings_address(bindings_address)
            .command(existing_metadata.runtime_command.clone())
            .working_dir(PathBuf::from(&working_dir))
            .env_vars(runtime_env_vars)
            .log_exporter(log_exporter)
            .build();

        let updated_metadata = WorkerMetadata {
            worker_id: id.to_string(),
            extracted_path: extracted_dir.clone(),
            env_vars: env_vars.clone(),
            runtime_command: existing_metadata.runtime_command.clone(),
            working_dir: existing_metadata.working_dir.clone(),
            transport_port: None,
        };
        Self::save_daemon_metadata_static(state_dir, &updated_metadata)?;

        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

        let id_clone = id.to_string();
        let runtime_task: JoinHandle<crate::error::Result<()>> = tokio::spawn(async move {
            alien_runtime::run(
                runtime_config,
                shutdown_rx,
                alien_runtime::BindingsSource::Provider(bindings_provider),
            )
            .await
            .context(ErrorData::Other {
                message: format!("Runtime failed for daemon '{}'", id_clone),
            })?;

            Ok(())
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if runtime_task.is_finished() {
            match runtime_task.await {
                Ok(Ok(())) => {
                    return Err(AlienError::new(ErrorData::Other {
                        message: format!("Runtime for daemon '{}' exited during startup", id),
                    }));
                }
                Ok(Err(e)) => {
                    return Err(e.context(ErrorData::Other {
                        message: format!("Runtime for daemon '{}' failed during startup", id),
                    }));
                }
                Err(e) => {
                    return Err(AlienError::new(ErrorData::Other {
                        message: format!("Runtime task for daemon '{}' panicked: {}", id, e),
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
                started_at: chrono::Utc::now(),
                metadata: updated_metadata,
            },
        );

        info!(daemon_id = %id, "Daemon runtime started");

        Ok(())
    }

    /// Stops a worker runtime (keeps extracted image directory and metadata for recovery).
    ///
    /// # Arguments
    /// * `id` - Worker identifier
    pub async fn stop_worker(&self, id: &str) -> Result<()> {
        let mut workers = self.workers.lock().await;

        if let Some(runtime) = workers.remove(id) {
            // Send shutdown signal (triggers wait_until drain, OTLP flush)
            if let Err(e) = runtime.shutdown_tx.send(()) {
                warn!(
                    worker_id = %id,
                    error = ?e,
                    "Failed to send shutdown signal to worker (receiver may be dropped)"
                );
            }

            // Wait for graceful shutdown
            match runtime.task_handle.await {
                Ok(Ok(())) => {
                    debug!(worker_id = %id, "Worker stopped gracefully");
                }
                Ok(Err(e)) => {
                    warn!(
                        worker_id = %id,
                        error = ?e,
                        "Worker task completed with error"
                    );
                }
                Err(e) => {
                    warn!(
                        worker_id = %id,
                        error = ?e,
                        "Worker task join failed"
                    );
                }
            }

            info!(worker_id = %id, "Worker stopped (metadata preserved for recovery)");
        } else {
            debug!(
                worker_id = %id,
                "Worker not running (already stopped)"
            );
        }

        Ok(())
    }

    /// Stops a daemon runtime (keeps extracted image directory and metadata for recovery).
    pub async fn stop_daemon(&self, id: &str) -> Result<()> {
        let mut daemons = self.daemons.lock().await;

        if let Some(runtime) = daemons.remove(id) {
            if let Err(e) = runtime.shutdown_tx.send(()) {
                warn!(
                    daemon_id = %id,
                    error = ?e,
                    "Failed to send shutdown signal to daemon (receiver may be dropped)"
                );
            }

            match runtime.task_handle.await {
                Ok(Ok(())) => {
                    debug!(daemon_id = %id, "Daemon stopped gracefully");
                }
                Ok(Err(e)) => {
                    warn!(daemon_id = %id, error = ?e, "Daemon task completed with error");
                }
                Err(e) => {
                    warn!(daemon_id = %id, error = ?e, "Daemon task join failed");
                }
            }

            info!(daemon_id = %id, "Daemon stopped (metadata preserved for recovery)");
        } else {
            debug!(daemon_id = %id, "Daemon not running (already stopped)");
        }

        Ok(())
    }

    /// Stops all active worker and daemon runtimes.
    ///
    /// The monitor loop uses the shared shutdown signal, but each active
    /// runtime has its own shutdown channel.
    pub async fn shutdown_all(&self) {
        let worker_ids = {
            let workers = self.workers.lock().await;
            workers.keys().cloned().collect::<Vec<_>>()
        };

        for id in worker_ids {
            if let Err(e) = self.stop_worker(&id).await {
                warn!(
                    worker_id = %id,
                    error = ?e,
                    "Failed to stop worker during shutdown"
                );
            }
        }

        let daemon_ids = {
            let daemons = self.daemons.lock().await;
            daemons.keys().cloned().collect::<Vec<_>>()
        };

        for id in daemon_ids {
            if let Err(e) = self.stop_daemon(&id).await {
                warn!(
                    daemon_id = %id,
                    error = ?e,
                    "Failed to stop daemon during shutdown"
                );
            }
        }
    }

    /// Deletes a worker (stops runtime, removes extracted image directory and metadata).
    ///
    /// # Arguments
    /// * `id` - Worker identifier
    pub async fn delete_worker(&self, id: &str) -> Result<()> {
        // Stop the worker first if it's running
        self.stop_worker(id).await?;

        // Delete the extracted image directory
        let worker_dir = self.state_dir.join("workers").join(id);
        if worker_dir.exists() {
            tokio::fs::remove_dir_all(&worker_dir)
                .await
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("Failed to delete worker directory for '{}'", id),
                })?;

            info!(
                worker_id = %id,
                path = %worker_dir.display(),
                "Worker directory deleted"
            );
        } else {
            debug!(
                worker_id = %id,
                path = %worker_dir.display(),
                "Worker directory does not exist (already deleted)"
            );
        }

        // Delete metadata so worker won't recover on restart
        Self::delete_metadata_static(&self.state_dir, id)?;

        Ok(())
    }

    /// Deletes a daemon (stops runtime, removes extracted image directory and metadata).
    pub async fn delete_daemon(&self, id: &str) -> Result<()> {
        self.stop_daemon(id).await?;

        let daemon_dir = self.state_dir.join("daemons").join(id);
        if daemon_dir.exists() {
            tokio::fs::remove_dir_all(&daemon_dir)
                .await
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("Failed to delete daemon directory for '{}'", id),
                })?;

            info!(
                daemon_id = %id,
                path = %daemon_dir.display(),
                "Daemon directory deleted"
            );
        } else {
            debug!(
                daemon_id = %id,
                path = %daemon_dir.display(),
                "Daemon directory does not exist (already deleted)"
            );
        }

        Self::delete_daemon_metadata_static(&self.state_dir, id)?;

        Ok(())
    }

    /// Checks if a worker is currently running.
    pub async fn is_running(&self, id: &str) -> bool {
        let workers = self.workers.lock().await;
        workers.contains_key(id)
    }

    /// Checks if a daemon is currently running.
    pub async fn is_daemon_running(&self, id: &str) -> bool {
        let daemons = self.daemons.lock().await;
        daemons.contains_key(id)
    }

    /// Gets the URL of a running worker.
    ///
    /// # Returns
    /// Worker URL or error if not running
    pub async fn get_worker_url(&self, id: &str) -> Result<String> {
        let workers = self.workers.lock().await;
        workers
            .get(id)
            .map(|runtime| runtime.worker_url.clone())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ServiceResourceNotFound {
                    resource_id: id.to_string(),
                    resource_type: "worker".to_string(),
                })
            })
    }

    /// Verifies that a worker resource exists and is healthy.
    ///
    /// This performs comprehensive health checks:
    /// 1. Verifies worker exists in manager's tracking
    /// 2. Checks task handle is still running
    /// 3. Verifies extracted directory exists (persistent state)
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// Ok(()) if worker exists and is running, error otherwise
    pub async fn check_health(&self, id: &str) -> Result<()> {
        let workers = self.workers.lock().await;

        match workers.get(id) {
            Some(runtime) => {
                // Check if the task is still running
                if runtime.task_handle.is_finished() {
                    return Err(AlienError::new(ErrorData::LocalProcessError {
                        process_id: id.to_string(),
                        operation: "health_check".to_string(),
                        reason: "Worker task has finished unexpectedly".to_string(),
                    }));
                }

                // Verify extracted directory still exists (persistent state check)
                let extracted_dir = self.state_dir.join("workers").join(id);
                if !extracted_dir.exists() {
                    return Err(AlienError::new(ErrorData::LocalDirectoryError {
                        path: extracted_dir.display().to_string(),
                        operation: "health_check".to_string(),
                        reason: "Worker extracted directory no longer exists".to_string(),
                    }));
                }

                // Verify metadata file exists
                let metadata_file = extracted_dir.join("metadata.json");
                if !metadata_file.exists() {
                    return Err(AlienError::new(ErrorData::Other {
                        message: format!("Worker metadata file missing for '{}'", id),
                    }));
                }

                Ok(())
            }
            None => Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "worker".to_string(),
            })),
        }
    }

    /// Verifies that a daemon resource exists and is healthy.
    pub async fn check_daemon_health(&self, id: &str) -> Result<()> {
        let daemons = self.daemons.lock().await;

        match daemons.get(id) {
            Some(runtime) => {
                if runtime.task_handle.is_finished() {
                    return Err(AlienError::new(ErrorData::LocalProcessError {
                        process_id: id.to_string(),
                        operation: "health_check".to_string(),
                        reason: "Daemon task has finished unexpectedly".to_string(),
                    }));
                }

                let extracted_dir = self.state_dir.join("daemons").join(id);
                if !extracted_dir.exists() {
                    return Err(AlienError::new(ErrorData::LocalDirectoryError {
                        path: extracted_dir.display().to_string(),
                        operation: "health_check".to_string(),
                        reason: "Daemon extracted directory no longer exists".to_string(),
                    }));
                }

                let metadata_file = extracted_dir.join("metadata.json");
                if !metadata_file.exists() {
                    return Err(AlienError::new(ErrorData::Other {
                        message: format!("Daemon metadata file missing for '{}'", id),
                    }));
                }

                Ok(())
            }
            None => Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "daemon".to_string(),
            })),
        }
    }

    /// Gets the binding configuration for a worker resource.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// WorkerBinding with current worker URL, or error if not running
    pub async fn get_binding(&self, id: &str) -> Result<alien_core::bindings::WorkerBinding> {
        use alien_core::bindings::{BindingValue, WorkerBinding};

        let worker_url = self.get_worker_url(id).await?;
        Ok(WorkerBinding::local(BindingValue::value(worker_url)))
    }

    pub async fn extract_image(
        &self,
        worker_id: &str,
        image_ref: &str,
        proxy_token: Option<&str>,
    ) -> Result<PathBuf> {
        self.extract_image_in_namespace("workers", "worker", worker_id, image_ref, proxy_token)
            .await
    }

    /// Extracts an OCI image for a daemon.
    pub async fn extract_daemon_image(
        &self,
        daemon_id: &str,
        image_ref: &str,
        proxy_token: Option<&str>,
    ) -> Result<PathBuf> {
        self.extract_image_in_namespace("daemons", "daemon", daemon_id, image_ref, proxy_token)
            .await
    }

    /// Extracts an OCI image into a local runtime namespace.
    ///
    /// The manager determines the extraction directory internally based on the
    /// resource ID and namespace.
    async fn extract_image_in_namespace(
        &self,
        namespace: &str,
        resource_kind: &str,
        worker_id: &str,
        image_ref: &str,
        proxy_token: Option<&str>,
    ) -> Result<PathBuf> {
        info!(
            resource_id = %worker_id,
            resource_kind = %resource_kind,
            image_ref = %image_ref,
            has_proxy_token = proxy_token.is_some(),
            "Extracting OCI image"
        );

        // Determine extraction directory using state_dir
        let target_dir = self.state_dir.join(namespace).join(worker_id);

        // Create target directory (idempotent with create_dir_all)
        fs::create_dir_all(&target_dir)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!(
                    "Failed to create extraction directory for {} '{}'",
                    resource_kind, worker_id
                ),
            })?;

        // Check if image_ref is a local path (file or directory)
        let image_path = std::path::Path::new(image_ref);
        let is_local =
            image_path.exists() || image_ref.ends_with(".tar") || image_ref.starts_with('/');

        if is_local && image_path.exists() {
            // Determine the actual tarball file to load
            let tarball_path = if image_path.is_dir() {
                // image_ref is a directory - find the appropriate .oci.tar file for current platform
                debug!(
                    worker_id = %worker_id,
                    image_ref = %image_ref,
                    "Image reference is a directory, searching for platform-specific tarball"
                );

                // Find all .oci.tar files in the directory
                let mut tarball_files = Vec::new();
                let entries =
                    fs::read_dir(image_path)
                        .into_alien_error()
                        .context(ErrorData::Other {
                            message: format!("Failed to read directory: {}", image_ref),
                        })?;

                for entry in entries {
                    let entry = entry.into_alien_error().context(ErrorData::Other {
                        message: format!("Failed to read directory entry in: {}", image_ref),
                    })?;
                    let path = entry.path();

                    if path.extension().and_then(|s| s.to_str()) == Some("tar")
                        && path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .map(|s| s.contains(".oci."))
                            .unwrap_or(false)
                    {
                        tarball_files.push(path);
                    }
                }

                if tarball_files.is_empty() {
                    return Err(AlienError::new(ErrorData::Other {
                        message: format!(
                            "No OCI tarball files (.oci.tar) found in directory: {}",
                            image_ref
                        ),
                    }));
                }

                let selected_tarball = select_host_tarball(&tarball_files)?;

                debug!(
                    worker_id = %worker_id,
                    selected_tarball = %selected_tarball.display(),
                    total_tarballs = tarball_files.len(),
                    "Selected tarball from directory"
                );

                selected_tarball.clone()
            } else {
                // image_ref is already a file path
                image_path.to_path_buf()
            };

            // Extract from local OCI tarball
            debug!(
                worker_id = %worker_id,
                tarball_path = %tarball_path.display(),
                target_dir = %target_dir.display(),
                "Extracting from local OCI tarball"
            );

            let image = dockdash::Image::from_tarball(tarball_path.to_str().unwrap())
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("Failed to load OCI tarball: {}", tarball_path.display()),
                })?;

            let (_extracted_path, metadata) = image
                .extract(&target_dir)
                .await
                .into_alien_error()
                .context(ErrorData::Other {
                message: format!("Failed to extract OCI image from tarball: {}", image_ref),
            })?;

            // Save metadata for runtime startup
            let worker_metadata = WorkerMetadata {
                worker_id: worker_id.to_string(),
                extracted_path: target_dir.clone(),
                env_vars: HashMap::new(), // Will be set during start_worker
                runtime_command: metadata.runtime_command(),
                working_dir: metadata.working_dir,
                transport_port: None, // Will be allocated during start_worker
            };
            if namespace == "daemons" {
                Self::save_daemon_metadata_static(&self.state_dir, &worker_metadata)?;
            } else {
                Self::save_metadata_static(&self.state_dir, &worker_metadata)?;
            }
        } else {
            // Pull from the manager's /v2/ registry.
            // The image URI already points at the proxy (set by the release).
            // The deployment token is required for auth.
            let token = proxy_token.ok_or_else(|| {
                AlienError::new(ErrorData::Other {
                    message:
                        "deployment_token is required for pulling images from the manager registry"
                            .to_string(),
                })
            })?;
            let auth = Some(dockdash::RegistryAuth::Basic(
                "deployment".to_string(),
                token.to_string(),
            ));
            let pull_policy = dockdash::PullPolicy::Always;

            debug!(
                worker_id = %worker_id,
                image_ref = %image_ref,
                target_dir = %target_dir.display(),
                has_auth = auth.is_some(),
                "Pulling OCI image from remote registry"
            );

            let current_target = alien_core::BinaryTarget::current_os();

            // Use HTTP for localhost registries (embedded local registry, dev mode).
            let protocol =
                if image_ref.starts_with("127.0.0.1") || image_ref.starts_with("localhost") {
                    dockdash::ClientProtocol::Http
                } else {
                    dockdash::ClientProtocol::Https
                };

            let pull_options = dockdash::PullAndExtractOptions {
                platform_os: Some(current_target.oci_os().to_string()),
                platform_arch: Some(match current_target.oci_arch() {
                    "arm64" => dockdash::Arch::ARM64,
                    _ => dockdash::Arch::Amd64,
                }),
                // dockdash seeds oci-client auth as a side effect of pulling
                // the manifest. With PullPolicy::Missing, a cached manifest can
                // skip auth setup and the first missing blob is pulled
                // anonymously. Manager-registry pulls are always authenticated,
                // so refresh the manifest to seed auth before blob pulls.
                pull_policy,
                blob_cache: None,
                auth,
                protocol,
            };

            let (_extracted_path, metadata) =
                dockdash::Image::pull_and_extract(image_ref, &target_dir, pull_options)
                    .await
                    .into_alien_error()
                    .context(ErrorData::Other {
                        message: format!("Failed to pull and extract OCI image: {}", image_ref),
                    })?;

            // Save metadata for runtime startup
            let worker_metadata = WorkerMetadata {
                worker_id: worker_id.to_string(),
                extracted_path: target_dir.clone(),
                env_vars: HashMap::new(), // Will be set during start_worker
                runtime_command: metadata.runtime_command(),
                working_dir: metadata.working_dir,
                transport_port: None, // Will be allocated during start_worker
            };
            if namespace == "daemons" {
                Self::save_daemon_metadata_static(&self.state_dir, &worker_metadata)?;
            } else {
                Self::save_metadata_static(&self.state_dir, &worker_metadata)?;
            }
        }

        info!(
            resource_id = %worker_id,
            resource_kind = %resource_kind,
            target_dir = %target_dir.display(),
            "OCI image extracted successfully"
        );

        Ok(target_dir)
    }
}

/// Extracts the repository name from an OCI image reference.
///
/// Allocates a port, preferring a saved port if available.
///
/// This enables transparent recovery - when a worker/container recovers from a crash,
/// it tries to bind to the same port it had before. Only allocates a new random port
/// if the saved port is unavailable.
///
/// **Important**: Tests port availability by binding to `0.0.0.0` (all interfaces), matching
/// what the LocalTransport does. Binding to `127.0.0.1` would give false positives because
/// `0.0.0.0:PORT` and `127.0.0.1:PORT` are different bindings to the OS.
///
/// # Arguments
/// * `saved_port` - Previously allocated port (if any)
/// * `resource_id` - Resource ID for logging
///
/// # Returns
/// The allocated port number
fn allocate_port_with_preference(saved_port: Option<u16>, resource_id: &str) -> Result<u16> {
    if let Some(saved_port) = saved_port {
        // Try to bind to the saved port on all interfaces (matching LocalTransport)
        match TcpListener::bind(format!("0.0.0.0:{}", saved_port)) {
            Ok(socket) => {
                let port = socket
                    .local_addr()
                    .into_alien_error()
                    .context(ErrorData::Other {
                        message: "Failed to get saved port address".to_string(),
                    })?
                    .port();
                drop(socket); // Release for actual use
                info!(
                    resource_id = %resource_id,
                    port = port,
                    "Reusing saved port (transparent recovery)"
                );
                return Ok(port);
            }
            Err(_) => {
                info!(
                    resource_id = %resource_id,
                    saved_port = saved_port,
                    "Saved port unavailable, allocating new port"
                );
            }
        }
    }

    // No saved port or it's unavailable - allocate a new random port on all interfaces
    let socket = TcpListener::bind("0.0.0.0:0")
        .into_alien_error()
        .context(ErrorData::Other {
            message: "Failed to allocate random port".to_string(),
        })?;
    let port = socket
        .local_addr()
        .into_alien_error()
        .context(ErrorData::Other {
            message: "Failed to get allocated port address".to_string(),
        })?
        .port();
    drop(socket); // Release for actual use

    if saved_port.is_none() {
        info!(resource_id = %resource_id, port = port, "Allocated new port");
    } else {
        info!(
            resource_id = %resource_id,
            old_port = saved_port,
            new_port = port,
            "Allocated new port (saved port unavailable)"
        );
    }

    Ok(port)
}

/// Pick the tarball to extract from an artifact directory. A native process can only exec
/// the host's binary, so when the directory holds several per-target tarballs the host's
/// must be chosen — another OS's binary would fail at spawn with an opaque exec error.
/// A single tarball is a single-target build output and is used as-is.
fn select_host_tarball(tarball_files: &[PathBuf]) -> Result<&PathBuf> {
    if tarball_files.len() == 1 {
        return Ok(&tarball_files[0]);
    }
    let host = alien_core::BinaryTarget::current_os();
    let host_tarball = format!("{}.oci.tar", host.runtime_platform_id());
    tarball_files
        .iter()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(host_tarball.as_str()))
        .ok_or_else(|| {
            let available: Vec<String> = tarball_files
                .iter()
                .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
                .map(str::to_string)
                .collect();
            AlienError::new(ErrorData::Other {
                message: format!(
                    "No tarball for host target '{}' in artifact directory (found: {}). \
                     Rebuild with this host among the targets.",
                    host.runtime_platform_id(),
                    available.join(", "),
                ),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| PathBuf::from(format!("/artifacts/{name}")))
            .collect()
    }

    #[test]
    fn single_tarball_is_used_as_is_regardless_of_target() {
        let files = paths(&["linux-x64.oci.tar"]);
        let selected = select_host_tarball(&files).expect("single tarball should be selected");
        assert_eq!(selected, &files[0]);
    }

    #[test]
    fn multiple_tarballs_select_the_host_target() {
        let host = alien_core::BinaryTarget::current_os();
        let host_name = format!("{}.oci.tar", host.runtime_platform_id());
        let files = paths(&[
            "darwin-aarch64.oci.tar",
            "linux-aarch64.oci.tar",
            "linux-x64.oci.tar",
            "windows-x64.oci.tar",
        ]);
        let selected = select_host_tarball(&files).expect("host tarball should be present");
        assert_eq!(
            selected.file_name().and_then(|name| name.to_str()),
            Some(host_name.as_str())
        );
    }

    #[test]
    fn multiple_tarballs_without_host_target_fail_fast() {
        // Exclude the host's own tarball so no entry matches, on any host OS.
        let host = alien_core::BinaryTarget::current_os();
        let all = [
            "darwin-aarch64.oci.tar",
            "linux-aarch64.oci.tar",
            "linux-x64.oci.tar",
            "windows-x64.oci.tar",
        ];
        let host_name = format!("{}.oci.tar", host.runtime_platform_id());
        let without_host: Vec<&str> = all
            .iter()
            .copied()
            .filter(|name| *name != host_name)
            .collect();
        let files = paths(&without_host);

        let error = match select_host_tarball(&files) {
            Err(error) => error,
            Ok(path) => panic!("expected failure, selected {}", path.display()),
        };
        let message = error.to_string();
        assert!(
            message.contains(host.runtime_platform_id()),
            "error names the host target: {message}"
        );
        for name in &without_host {
            assert!(message.contains(name), "error lists {name}: {message}");
        }
    }
}
