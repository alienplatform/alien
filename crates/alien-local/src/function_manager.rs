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

/// Manager for local function resources.
///
/// Spawns and manages function runtime processes. Functions run as separate processes
/// so their logs can be captured and streamed with resource ID prefixes.
///
/// This manager maintains persistent state and provides auto-recovery:
/// - Function metadata is saved to disk for crash recovery
/// - Background task monitors health and auto-recovers crashed functions
/// - Graceful shutdown via shared signal
///
/// # State Scoping
/// Function state is stored under `{state_dir}/functions/{function_id}/`:
/// - `metadata.json` - Recovery metadata for auto-recovery
/// - Other files - Extracted OCI image contents
///
/// The `state_dir` should be scoped by agent ID (e.g., `~/.alien-cli/<agent_id>`)
/// to avoid conflicts between agents.
#[derive(Debug)]
pub struct LocalFunctionManager {
    /// Base directory for all local platform state
    state_dir: PathBuf,
    /// Map of function ID to runtime state (ephemeral)
    functions: Arc<Mutex<HashMap<String, FunctionRuntime>>>,
    /// Bindings provider for function runtimes
    bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
}

#[derive(Debug)]
struct FunctionRuntime {
    /// Tokio task handle for the function (returns our local Result type)
    task_handle: JoinHandle<crate::error::Result<()>>,
    /// Shutdown channel sender
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    /// URL where the function is accessible
    function_url: String,
    /// When the function was started (used for monitoring)
    #[allow(dead_code)]
    started_at: chrono::DateTime<chrono::Utc>,
    /// Persistent metadata for this function (used for crash recovery)
    #[allow(dead_code)]
    metadata: FunctionMetadata,
}

/// Persistent metadata for a function (saved to disk)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FunctionMetadata {
    /// Function identifier
    function_id: String,
    /// Path to the extracted OCI image
    extracted_path: PathBuf,
    /// Environment variables for the function
    env_vars: HashMap<String, String>,
    /// Runtime command from OCI image config (ENTRYPOINT + CMD)
    runtime_command: Vec<String>,
    /// Working directory from OCI image config
    working_dir: Option<String>,
    /// Transport port for the runtime (persisted to enable transparent recovery)
    #[serde(default)]
    transport_port: Option<u16>,
}

impl LocalFunctionManager {
    /// Creates a new function manager with shared shutdown signal.
    ///
    /// # Arguments
    /// * `state_dir` - Base directory for all local platform state
    /// * `bindings_provider` - Bindings provider for function runtimes
    /// * `shutdown_rx` - Shutdown signal receiver (shared across all services)
    ///
    /// # Returns
    /// (Manager, Optional JoinHandle for background task)
    pub fn new_with_shutdown(
        state_dir: PathBuf,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) -> (Self, Option<tokio::task::JoinHandle<()>>) {
        let functions = Arc::new(Mutex::new(HashMap::new()));

        // Spawn background task for health monitoring and auto-recovery
        let state_dir_clone = state_dir.clone();
        let functions_clone = functions.clone();
        let bindings_provider_clone = bindings_provider.clone();
        let background_task = tokio::spawn(async move {
            Self::monitor_and_recover_loop(
                state_dir_clone,
                functions_clone,
                bindings_provider_clone,
                shutdown_rx,
            )
            .await;
        });

        let manager = Self {
            state_dir,
            functions,
            bindings_provider,
        };

        (manager, Some(background_task))
    }

    /// Background loop that monitors function health and handles auto-recovery
    async fn monitor_and_recover_loop(
        state_dir: PathBuf,
        functions: Arc<Mutex<HashMap<String, FunctionRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) {
        // First, attempt recovery of functions from previous run
        if let Err(e) =
            Self::recover_all_functions(&state_dir, &functions, bindings_provider.clone()).await
        {
            warn!("Failed to recover functions from metadata: {:?}", e);
        }

        // Then monitor health and auto-restart crashed functions
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Function manager shutting down");
                    break;
                }
                _ = interval.tick() => {
                    if let Err(e) = Self::monitor_and_restart(&state_dir, &functions, bindings_provider.clone()).await {
                        warn!("Function health check failed: {:?}", e);
                    }
                }
            }
        }
    }

    /// Recovers all functions from metadata files
    async fn recover_all_functions(
        state_dir: &PathBuf,
        functions: &Arc<Mutex<HashMap<String, FunctionRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        let functions_dir = state_dir.join("functions");
        if !functions_dir.exists() {
            return Ok(());
        }

        let entries =
            fs::read_dir(&functions_dir)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Failed to read functions directory".to_string(),
                })?;

        for entry in entries {
            let entry = entry.into_alien_error().context(ErrorData::Other {
                message: "Failed to read function entry".to_string(),
            })?;

            // Check if this is a directory (each function has its own directory)
            if entry.path().is_dir() {
                let metadata_file = entry.path().join("metadata.json");
                if metadata_file.exists() {
                    if let Err(e) = Self::recover_single_function(
                        &metadata_file,
                        state_dir,
                        functions,
                        bindings_provider.clone(),
                    )
                    .await
                    {
                        warn!(
                            "Failed to recover function from {:?}: {:?}",
                            metadata_file, e
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Recovers a single function from metadata file
    async fn recover_single_function(
        metadata_path: &PathBuf,
        state_dir: &PathBuf,
        functions: &Arc<Mutex<HashMap<String, FunctionRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        let contents = tokio::fs::read_to_string(metadata_path)
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to read {}", metadata_path.display()),
            })?;

        let metadata: FunctionMetadata = serde_json::from_str(&contents)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to parse function metadata".to_string(),
            })?;

        // Check if already running
        {
            let functions_guard = functions.lock().await;
            if functions_guard.contains_key(&metadata.function_id) {
                debug!(function_id = %metadata.function_id, "Function already running, skipping recovery");
                return Ok(());
            }
        }

        info!(function_id = %metadata.function_id, "Recovering function from previous run");

        // Restart the function using metadata
        Self::start_function_internal(
            &metadata.function_id,
            metadata.env_vars,
            state_dir,
            functions,
            bindings_provider,
        )
        .await?;

        Ok(())
    }

    /// Monitors running functions and restarts crashed ones
    async fn monitor_and_restart(
        state_dir: &PathBuf,
        functions: &Arc<Mutex<HashMap<String, FunctionRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<()> {
        let function_ids: Vec<String> = {
            let functions_guard = functions.lock().await;
            functions_guard.keys().cloned().collect()
        };

        for function_id in function_ids {
            let (metadata, task_result) = {
                let mut functions_mut = functions.lock().await;
                if let Some(runtime) = functions_mut.get(&function_id) {
                    if runtime.task_handle.is_finished() {
                        // Function crashed - remove and get metadata + task result
                        let mut runtime = functions_mut.remove(&function_id).unwrap();
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
                            warn!(function_id = %function_id, "Function exited cleanly but unexpectedly");
                        }
                        Ok(Err(e)) => {
                            warn!(function_id = %function_id, error = ?e, "Function crashed with error");
                        }
                        Err(e) => {
                            warn!(function_id = %function_id, error = ?e, "Function task panicked");
                        }
                    }
                }

                warn!(function_id = %function_id, "Auto-restarting function...");

                // Restart using metadata
                if let Err(e) = Self::start_function_internal(
                    &metadata.function_id,
                    metadata.env_vars,
                    state_dir,
                    functions,
                    bindings_provider.clone(),
                )
                .await
                {
                    warn!(function_id = %function_id, error = ?e, "Failed to restart");
                } else {
                    info!(function_id = %function_id, "Successfully restarted after crash");
                }
            }
        }

        Ok(())
    }

    /// Saves function metadata to disk (static for use by background task)
    fn save_metadata_static(state_dir: &PathBuf, metadata: &FunctionMetadata) -> Result<()> {
        let function_dir = state_dir.join("functions").join(&metadata.function_id);
        fs::create_dir_all(&function_dir)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to create function directory".to_string(),
            })?;

        let metadata_file = function_dir.join("metadata.json");
        let contents = serde_json::to_string_pretty(metadata)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to serialize function metadata".to_string(),
            })?;

        fs::write(&metadata_file, contents)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to write metadata file: {}", metadata_file.display()),
            })?;

        Ok(())
    }

    /// Deletes function metadata from disk (static for use by background task)
    fn delete_metadata_static(state_dir: &PathBuf, function_id: &str) -> Result<()> {
        let metadata_file = state_dir
            .join("functions")
            .join(function_id)
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

    /// Starts a function runtime.
    ///
    /// # Arguments
    /// * `id` - Function identifier
    /// * `env_vars` - Additional environment variables to pass to the function
    ///
    /// # Returns
    /// URL where the function is accessible (e.g., "http://localhost:3000")
    ///
    /// # Note
    /// This is idempotent - safe to call multiple times. Auto-recovery happens here.
    pub async fn start_function(
        &self,
        id: &str,
        env_vars: HashMap<String, String>,
    ) -> Result<String> {
        Self::start_function_internal(
            id,
            env_vars,
            &self.state_dir,
            &self.functions,
            self.bindings_provider.clone(),
        )
        .await
    }

    /// Internal static implementation of start_function for use by background task
    async fn start_function_internal(
        id: &str,
        env_vars: HashMap<String, String>,
        state_dir: &PathBuf,
        functions: &Arc<Mutex<HashMap<String, FunctionRuntime>>>,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Result<String> {
        // Check if already running
        {
            let functions_guard = functions.lock().await;
            if let Some(runtime) = functions_guard.get(id) {
                debug!(function_id = %id, "Function already running");
                return Ok(runtime.function_url.clone());
            }
        }

        // Get the extracted directory for this function
        let extracted_dir = state_dir.join("functions").join(id);

        // Load metadata to get runtime command and saved transport port
        let metadata_file = extracted_dir.join("metadata.json");
        if !metadata_file.exists() {
            return Err(AlienError::new(ErrorData::Other {
                message: format!(
                    "Function metadata not found at {}. Run extract_image first.",
                    metadata_file.display()
                ),
            }));
        }

        let metadata_contents = std::fs::read_to_string(&metadata_file)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to read metadata file: {}", metadata_file.display()),
            })?;

        let existing_metadata: FunctionMetadata = serde_json::from_str(&metadata_contents)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to parse function metadata".to_string(),
            })?;

        let saved_port = existing_metadata.transport_port;

        // Allocate port for function HTTP server (runtime proxy)
        // Try to reuse the saved port first for transparent recovery
        let port = allocate_port_with_preference(saved_port, id)?;

        let function_url = format!("http://localhost:{}", port);

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
        // For local functions, we extract OTLP config from env_vars and pass directly
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
                // No OTLP config - shouldn't happen for functions, but fallback to None
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
        let updated_metadata = FunctionMetadata {
            function_id: id.to_string(),
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
                message: format!("Runtime failed for function '{}'", id_clone),
            })?;

            Ok(())
        });

        // Wait for the HTTP transport to actually be ready (up to 10 seconds)
        // This prevents race conditions where tests start before the proxy is listening
        let max_wait = std::time::Duration::from_secs(10);
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
                debug!(function_id = %id, port = port, "Transport is ready");
                break;
            }

            // Check if we've exceeded the timeout
            if start.elapsed() > max_wait {
                return Err(AlienError::new(ErrorData::Other {
                    message: format!(
                        "Function '{}' transport did not become ready within {:?}",
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
                                "Runtime for function '{}' exited before transport was ready",
                                id
                            ),
                        }));
                    }
                    Ok(Err(e)) => {
                        return Err(e.context(ErrorData::Other {
                            message: format!("Runtime for function '{}' failed during startup", id),
                        }));
                    }
                    Err(e) => {
                        return Err(AlienError::new(ErrorData::Other {
                            message: format!("Runtime task for function '{}' panicked: {}", id, e),
                        }));
                    }
                }
            }

            // Wait before next check
            tokio::time::sleep(check_interval).await;
        }

        // Track handle
        let mut functions_mut = functions.lock().await;
        functions_mut.insert(
            id.to_string(),
            FunctionRuntime {
                task_handle: runtime_task,
                shutdown_tx,
                function_url: function_url.clone(),
                started_at: chrono::Utc::now(),
                metadata: updated_metadata,
            },
        );

        info!(
            function_id = %id,
            url = %function_url,
            "Function runtime started"
        );

        Ok(function_url)
    }

    /// Stops a function runtime (keeps extracted image directory and metadata for recovery).
    ///
    /// # Arguments
    /// * `id` - Function identifier
    pub async fn stop_function(&self, id: &str) -> Result<()> {
        let mut functions = self.functions.lock().await;

        if let Some(runtime) = functions.remove(id) {
            // Send shutdown signal (triggers wait_until drain, OTLP flush)
            if let Err(e) = runtime.shutdown_tx.send(()) {
                warn!(
                    function_id = %id,
                    error = ?e,
                    "Failed to send shutdown signal to function (receiver may be dropped)"
                );
            }

            // Wait for graceful shutdown
            match runtime.task_handle.await {
                Ok(Ok(())) => {
                    debug!(function_id = %id, "Function stopped gracefully");
                }
                Ok(Err(e)) => {
                    warn!(
                        function_id = %id,
                        error = ?e,
                        "Function task completed with error"
                    );
                }
                Err(e) => {
                    warn!(
                        function_id = %id,
                        error = ?e,
                        "Function task join failed"
                    );
                }
            }

            info!(function_id = %id, "Function stopped (metadata preserved for recovery)");
        } else {
            debug!(
                function_id = %id,
                "Function not running (already stopped)"
            );
        }

        Ok(())
    }

    /// Deletes a function (stops runtime, removes extracted image directory and metadata).
    ///
    /// # Arguments
    /// * `id` - Function identifier
    pub async fn delete_function(&self, id: &str) -> Result<()> {
        // Stop the function first if it's running
        self.stop_function(id).await?;

        // Delete the extracted image directory
        let function_dir = self.state_dir.join("functions").join(id);
        if function_dir.exists() {
            tokio::fs::remove_dir_all(&function_dir)
                .await
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("Failed to delete function directory for '{}'", id),
                })?;

            info!(
                function_id = %id,
                path = %function_dir.display(),
                "Function directory deleted"
            );
        } else {
            debug!(
                function_id = %id,
                path = %function_dir.display(),
                "Function directory does not exist (already deleted)"
            );
        }

        // Delete metadata so function won't recover on restart
        Self::delete_metadata_static(&self.state_dir, id)?;

        Ok(())
    }

    /// Checks if a function is currently running.
    pub async fn is_running(&self, id: &str) -> bool {
        let functions = self.functions.lock().await;
        functions.contains_key(id)
    }

    /// Gets the URL of a running function.
    ///
    /// # Returns
    /// Function URL or error if not running
    pub async fn get_function_url(&self, id: &str) -> Result<String> {
        let functions = self.functions.lock().await;
        functions
            .get(id)
            .map(|runtime| runtime.function_url.clone())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ServiceResourceNotFound {
                    resource_id: id.to_string(),
                    resource_type: "function".to_string(),
                })
            })
    }

    /// Verifies that a function resource exists and is healthy.
    ///
    /// This performs comprehensive health checks:
    /// 1. Verifies function exists in manager's tracking
    /// 2. Checks task handle is still running
    /// 3. Verifies extracted directory exists (persistent state)
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// Ok(()) if function exists and is running, error otherwise
    pub async fn check_health(&self, id: &str) -> Result<()> {
        let functions = self.functions.lock().await;

        match functions.get(id) {
            Some(runtime) => {
                // Check if the task is still running
                if runtime.task_handle.is_finished() {
                    return Err(AlienError::new(ErrorData::LocalProcessError {
                        process_id: id.to_string(),
                        operation: "health_check".to_string(),
                        reason: "Function task has finished unexpectedly".to_string(),
                    }));
                }

                // Verify extracted directory still exists (persistent state check)
                let extracted_dir = self.state_dir.join("functions").join(id);
                if !extracted_dir.exists() {
                    return Err(AlienError::new(ErrorData::LocalDirectoryError {
                        path: extracted_dir.display().to_string(),
                        operation: "health_check".to_string(),
                        reason: "Function extracted directory no longer exists".to_string(),
                    }));
                }

                // Verify metadata file exists
                let metadata_file = extracted_dir.join("metadata.json");
                if !metadata_file.exists() {
                    return Err(AlienError::new(ErrorData::Other {
                        message: format!("Function metadata file missing for '{}'", id),
                    }));
                }

                Ok(())
            }
            None => Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "function".to_string(),
            })),
        }
    }

    /// Gets the binding configuration for a function resource.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// FunctionBinding with current function URL, or error if not running
    pub async fn get_binding(&self, id: &str) -> Result<alien_core::bindings::FunctionBinding> {
        use alien_core::bindings::{BindingValue, FunctionBinding};

        let function_url = self.get_function_url(id).await?;
        Ok(FunctionBinding::local(BindingValue::value(function_url)))
    }

    /// Extracts an OCI image for a function.
    ///
    /// The manager determines the extraction directory internally based on the function ID.
    ///
    /// # Arguments
    /// * `function_id` - Function identifier
    /// * `image_ref` - OCI image reference (e.g., "ghcr.io/myorg/myimage:latest" or local file path)
    /// * `artifact_registry_config` - Optional artifact registry configuration for authentication
    ///
    /// # Returns
    /// Path to the extracted directory
    pub async fn extract_image(
        &self,
        function_id: &str,
        image_ref: &str,
        artifact_registry_config: Option<&alien_core::ArtifactRegistryConfig>,
    ) -> Result<PathBuf> {
        info!(
            function_id = %function_id,
            image_ref = %image_ref,
            has_artifact_registry_config = artifact_registry_config.is_some(),
            "Extracting OCI image for function"
        );

        // Determine extraction directory using state_dir
        let target_dir = self.state_dir.join("functions").join(function_id);

        // Create target directory (idempotent with create_dir_all)
        fs::create_dir_all(&target_dir)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!(
                    "Failed to create extraction directory for function '{}'",
                    function_id
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
                    function_id = %function_id,
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

                // For now, just use the first tarball
                // TODO: In the future, we could detect the current platform and select the matching tarball
                // (e.g., darwin-aarch64.oci.tar vs linux-x86_64.oci.tar)
                let selected_tarball = &tarball_files[0];

                debug!(
                    function_id = %function_id,
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
                function_id = %function_id,
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

            // Save metadata for function startup
            let function_metadata = FunctionMetadata {
                function_id: function_id.to_string(),
                extracted_path: target_dir.clone(),
                env_vars: HashMap::new(), // Will be set during start_function
                runtime_command: metadata.runtime_command(),
                working_dir: metadata.working_dir,
                transport_port: None, // Will be allocated during start_function
            };
            Self::save_metadata_static(&self.state_dir, &function_metadata)?;
        } else {
            // Pull from remote registry
            // Fetch credentials from agent manager if artifact_registry_config is provided
            let auth = if let Some(config) = artifact_registry_config {
                debug!(
                    manager_url = %config.manager_url,
                    "Fetching artifact registry credentials from manager"
                );
                let repo_name = extract_repository_from_image_ref(image_ref)?;
                let credentials = fetch_artifact_registry_credentials(
                    &config.manager_url,
                    &repo_name,
                    config.auth_token.as_deref(),
                )
                .await?;
                Some(credentials)
            } else {
                None
            };

            debug!(
                function_id = %function_id,
                image_ref = %image_ref,
                target_dir = %target_dir.display(),
                has_auth = auth.is_some(),
                "Pulling OCI image from remote registry"
            );

            let pull_options = dockdash::PullAndExtractOptions {
                platform_os: Some("linux".to_string()),
                platform_arch: Some(dockdash::Arch::Amd64),
                pull_policy: dockdash::PullPolicy::Missing,
                blob_cache: None, // Use default cache
                auth,
            };

            let (_extracted_path, metadata) =
                dockdash::Image::pull_and_extract(image_ref, &target_dir, pull_options)
                    .await
                    .into_alien_error()
                    .context(ErrorData::Other {
                        message: format!("Failed to pull and extract OCI image: {}", image_ref),
                    })?;

            // Save metadata for function startup
            let function_metadata = FunctionMetadata {
                function_id: function_id.to_string(),
                extracted_path: target_dir.clone(),
                env_vars: HashMap::new(), // Will be set during start_function
                runtime_command: metadata.runtime_command(),
                working_dir: metadata.working_dir,
                transport_port: None, // Will be allocated during start_function
            };
            Self::save_metadata_static(&self.state_dir, &function_metadata)?;
        }

        info!(
            function_id = %function_id,
            target_dir = %target_dir.display(),
            "OCI image extracted successfully"
        );

        Ok(target_dir)
    }
}

/// Extracts the repository name from an OCI image reference.
///
/// For example:
/// - "123456789012.dkr.ecr.us-east-1.amazonaws.com/my-repo:tag" -> "my-repo"
/// - "gcr.io/my-project/my-repo:tag" -> "my-project/my-repo"
/// - "my-registry.com/org/repo:tag" -> "org/repo"
fn extract_repository_from_image_ref(image_ref: &str) -> Result<String> {
    // Remove tag or digest if present
    let without_tag = image_ref
        .split('@')
        .next()
        .and_then(|s| s.split(':').next())
        .ok_or_else(|| {
            AlienError::new(ErrorData::Other {
                message: format!("Invalid image reference format: {}", image_ref),
            })
        })?;

    // Split by '/' and take everything after the registry domain
    let parts: Vec<&str> = without_tag.split('/').collect();
    if parts.len() < 2 {
        return Err(AlienError::new(ErrorData::Other {
            message: format!(
                "Image reference must include registry domain and repository: {}",
                image_ref
            ),
        }));
    }

    // Join all parts after the registry domain (e.g., "org/repo")
    Ok(parts[1..].join("/"))
}

/// Fetches artifact registry credentials from the manager API.
async fn fetch_artifact_registry_credentials(
    manager_url: &str,
    repo_name: &str,
    auth_token: Option<&str>,
) -> Result<dockdash::RegistryAuth> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

    // Note: These match the types in agent-manager's models.rs
    #[derive(serde::Serialize)]
    #[serde(rename_all = "lowercase")]
    #[allow(dead_code)]
    enum OperationType {
        Push,
        Pull,
    }

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct GenerateCredentialsRequest {
        operation: OperationType,
        duration_seconds: Option<u32>,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CredentialsResponse {
        username: String,
        password: String,
    }

    // Build the request URL
    let url = format!(
        "{}/v1/artifact-registry/repositories/{}/credentials",
        manager_url.trim_end_matches('/'),
        urlencoding::encode(repo_name)
    );

    // Build headers (including auth token if provided)
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(token) = auth_token {
        let auth_value = HeaderValue::from_str(&format!("Bearer {}", token))
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Invalid auth token format".to_string(),
            })?;
        headers.insert(AUTHORIZATION, auth_value);
    }

    // Build request body - request Pull permissions with 1 hour TTL
    let request_body = GenerateCredentialsRequest {
        operation: OperationType::Pull,
        duration_seconds: Some(3600), // 1 hour
    };

    // Make the POST request
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .headers(headers)
        .json(&request_body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::Other {
            message: format!("Failed to fetch credentials from agent manager: {}", url),
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".to_string());
        return Err(AlienError::new(ErrorData::Other {
            message: format!("Agent manager returned error status {}: {}", status, body),
        }));
    }

    let creds: CredentialsResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to parse credentials response from agent manager".to_string(),
            })?;

    Ok(dockdash::RegistryAuth::Basic(
        creds.username,
        creds.password,
    ))
}

/// Allocates a port, preferring a saved port if available.
///
/// This enables transparent recovery - when a function/container recovers from a crash,
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
