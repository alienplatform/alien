#[cfg(unix)]
mod unix_impl {
    use crate::error::{ErrorData, Result};
    use alien_error::{AlienError, Context, IntoAlienError};
    use container_registry::{auth, test_support::RunningRegistry, ContainerRegistry};
    use sec::Secret;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tracing::{debug, info, warn};

    /// Manager for local artifact registry resources.
    ///
    /// Starts and manages OCI registry server processes using the container_registry crate.
    ///
    /// This manager maintains persistent state and provides auto-recovery:
    /// - Registry metadata is saved to disk for crash recovery
    /// - Background task auto-recovers registries from previous runs
    /// - Graceful shutdown via shared signal
    ///
    /// # State Scoping
    /// Registry state is stored under `{state_dir}/artifact_registry/{registry_id}/`:
    /// - `metadata.json` - Recovery metadata for auto-recovery
    /// - Other files - Registry data (images, blobs, manifests)
    ///
    /// The `state_dir` should be scoped by agent ID (e.g., `~/.alien-cli/<agent_id>`)
    /// to avoid conflicts between agents.
    #[derive(Debug)]
    pub struct LocalArtifactRegistryManager {
        state_dir: PathBuf,
        /// Map of registry ID to runtime state (ephemeral)
        registries: Arc<Mutex<HashMap<String, RegistryRuntime>>>,
    }

    struct RegistryRuntime {
        /// Running registry handle from container_registry crate
        running_registry: RunningRegistry,
        /// Registry URL (e.g., "localhost:5000")
        registry_url: String,
        /// Directory where registry data is stored
        storage_dir: PathBuf,
        /// When the registry was started
        started_at: chrono::DateTime<chrono::Utc>,
        /// Persistent metadata for this registry (used for crash recovery)
        #[allow(dead_code)]
        metadata: RegistryMetadata,
    }

    impl std::fmt::Debug for RegistryRuntime {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RegistryRuntime")
                .field("registry_url", &self.registry_url)
                .field("storage_dir", &self.storage_dir)
                .field("started_at", &self.started_at)
                .finish()
        }
    }

    /// Persistent metadata for a registry (saved to disk)
    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct RegistryMetadata {
        /// Registry identifier
        registry_id: String,
    }

    impl LocalArtifactRegistryManager {
        /// Creates a new artifact registry manager (for testing).
        ///
        /// This creates a manager without background monitoring. For production use,
        /// prefer `new_with_shutdown()`.
        #[cfg(test)]
        pub fn new(state_dir: PathBuf) -> Self {
            Self {
                state_dir,
                registries: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        /// Creates a new artifact registry manager with shared shutdown signal.
        ///
        /// # Arguments
        /// * `state_dir` - Base directory for all local platform state
        /// * `shutdown_rx` - Shutdown signal receiver (shared across all services)
        ///
        /// # Returns
        /// (Manager, Optional JoinHandle for background task)
        pub fn new_with_shutdown(
            state_dir: PathBuf,
            shutdown_rx: tokio::sync::broadcast::Receiver<()>,
        ) -> (Self, Option<tokio::task::JoinHandle<()>>) {
            let registries = Arc::new(Mutex::new(HashMap::new()));

            // Spawn background task for auto-recovery
            let state_dir_clone = state_dir.clone();
            let registries_clone = registries.clone();
            let background_task = tokio::spawn(async move {
                Self::monitor_and_recover_loop(state_dir_clone, registries_clone, shutdown_rx)
                    .await;
            });

            let manager = Self {
                state_dir,
                registries,
            };

            (manager, Some(background_task))
        }

        /// Background loop that monitors registry health and handles auto-recovery
        async fn monitor_and_recover_loop(
            state_dir: PathBuf,
            registries: Arc<Mutex<HashMap<String, RegistryRuntime>>>,
            mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
        ) {
            // First, attempt recovery of registries from previous run
            if let Err(e) = Self::recover_all_registries(&state_dir, &registries).await {
                warn!("Failed to recover registries from metadata: {:?}", e);
            }

            // Monitor registries (RunningRegistry handles crashes internally, so minimal monitoring needed)
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("Artifact registry manager shutting down");
                        break;
                    }
                    _ = interval.tick() => {
                        // RunningRegistry handles server lifecycle
                        // No explicit health checks needed
                    }
                }
            }
        }

        /// Recovers all registries from metadata files
        async fn recover_all_registries(
            state_dir: &PathBuf,
            registries: &Arc<Mutex<HashMap<String, RegistryRuntime>>>,
        ) -> Result<()> {
            let registries_dir = state_dir.join("artifact_registry");
            if !registries_dir.exists() {
                return Ok(());
            }

            let entries = std::fs::read_dir(&registries_dir)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Failed to read artifact_registry directory".to_string(),
                })?;

            for entry in entries {
                let entry = entry.into_alien_error().context(ErrorData::Other {
                    message: "Failed to read registry entry".to_string(),
                })?;

                // Check if this is a directory (each registry has its own directory)
                if entry.path().is_dir() {
                    let metadata_file = entry.path().join("metadata.json");
                    if metadata_file.exists() {
                        if let Err(e) = Self::recover_single_registry(
                            &metadata_file,
                            state_dir,
                            registries,
                        )
                        .await
                        {
                            warn!(
                                "Failed to recover registry from {:?}: {:?}",
                                metadata_file, e
                            );
                        }
                    }
                }
            }

            Ok(())
        }

        /// Recovers a single registry from metadata file
        async fn recover_single_registry(
            metadata_path: &PathBuf,
            state_dir: &PathBuf,
            registries: &Arc<Mutex<HashMap<String, RegistryRuntime>>>,
        ) -> Result<()> {
            let contents = tokio::fs::read_to_string(metadata_path)
                .await
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("Failed to read {}", metadata_path.display()),
                })?;

            let metadata: RegistryMetadata = serde_json::from_str(&contents)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Failed to parse registry metadata".to_string(),
                })?;

            // Check if already running
            {
                let registries_guard = registries.lock().await;
                if registries_guard.contains_key(&metadata.registry_id) {
                    debug!(registry_id = %metadata.registry_id, "Registry already running, skipping recovery");
                    return Ok(());
                }
            }

            debug!(registry_id = %metadata.registry_id, "Recovering registry from previous run");

            // Restart the registry using metadata
            Self::start_registry_internal(&metadata.registry_id, state_dir, registries).await?;

            Ok(())
        }

        /// Saves registry metadata to disk (static for use by background task)
        fn save_metadata_static(state_dir: &PathBuf, metadata: &RegistryMetadata) -> Result<()> {
            let registry_dir = state_dir
                .join("artifact_registry")
                .join(&metadata.registry_id);
            std::fs::create_dir_all(&registry_dir)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Failed to create registry directory".to_string(),
                })?;

            let metadata_file = registry_dir.join("metadata.json");
            let contents = serde_json::to_string_pretty(metadata)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Failed to serialize registry metadata".to_string(),
                })?;

            std::fs::write(&metadata_file, contents)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("Failed to write metadata file: {}", metadata_file.display()),
                })?;

            Ok(())
        }

        /// Deletes registry metadata from disk (static for use by background task)
        fn delete_metadata_static(state_dir: &PathBuf, registry_id: &str) -> Result<()> {
            let metadata_file = state_dir
                .join("artifact_registry")
                .join(registry_id)
                .join("metadata.json");

            if metadata_file.exists() {
                std::fs::remove_file(&metadata_file)
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

        /// Starts a local OCI registry server.
        ///
        /// # Arguments
        /// * `id` - Registry identifier
        ///
        /// # Returns
        /// Registry URL (e.g., "localhost:5000")
        ///
        /// # Note
        /// This is idempotent - safe to call multiple times. Auto-recovery happens here.
        pub async fn start_registry(&self, id: &str) -> Result<String> {
            Self::start_registry_internal(id, &self.state_dir, &self.registries).await
        }

        /// Internal static implementation of start_registry for use by background task
        async fn start_registry_internal(
            id: &str,
            state_dir: &PathBuf,
            registries: &Arc<Mutex<HashMap<String, RegistryRuntime>>>,
        ) -> Result<String> {
            // Check if already running
            {
                let registries_guard = registries.lock().await;
                if let Some(runtime) = registries_guard.get(id) {
                    debug!(registry_id = %id, "Registry already running");
                    return Ok(runtime.registry_url.clone());
                }
            }

            // Create storage directory for this registry (idempotent with create_dir_all)
            let storage_dir = state_dir.join("artifact_registry").join(id);
            std::fs::create_dir_all(&storage_dir)
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: storage_dir.display().to_string(),
                    operation: "create".to_string(),
                    reason: "Failed to create registry storage directory".to_string(),
                })?;

            // Setup auth: Basic auth for local platform
            // All requests must use basic auth (local-user/local-password)
            // This matches what generate_credentials() returns in the provider
            let mut auth_map = std::collections::HashMap::new();
            auth_map.insert(
                "local-user".to_string(),
                Secret::new("local-password".to_owned()),
            );
            let auth = Arc::new(auth_map);

            // Start registry server with directory-backed storage
            // Using build_for_testing() for the convenience of run_in_background(),
            // but with explicit storage directory for persistence
            let running_registry = ContainerRegistry::builder()
                .storage(&storage_dir)
                .auth_provider(auth)
                .build_for_testing()
                .run_in_background();

            // Get the actual bound port
            let bound_addr = running_registry.bound_addr();
            let registry_url = format!("localhost:{}", bound_addr.port());

            // Create and save metadata for crash recovery
            let metadata = RegistryMetadata {
                registry_id: id.to_string(),
            };
            Self::save_metadata_static(state_dir, &metadata)?;

            info!(
                registry_id = %id,
                url = %registry_url,
                "Artifact registry started"
            );

            // Track runtime
            let mut registries_mut = registries.lock().await;
            registries_mut.insert(
                id.to_string(),
                RegistryRuntime {
                    running_registry,
                    registry_url: registry_url.clone(),
                    storage_dir,
                    started_at: chrono::Utc::now(),
                    metadata,
                },
            );

            Ok(registry_url)
        }

        /// Removes a registry from the manager, stopping it if running.
        ///
        /// This stops the server process and deletes the metadata file to prevent
        /// auto-recovery. Use this when permanently removing a registry (e.g., Delete flow).
        ///
        /// For graceful shutdown where you want recovery on next run, simply let the
        /// process exit without calling this method - metadata will persist and the
        /// registry will auto-recover on next startup.
        ///
        /// # Arguments
        /// * `id` - Registry identifier
        pub async fn remove_registry(&self, id: &str) -> Result<()> {
            let mut registries = self.registries.lock().await;

            if let Some(runtime) = registries.remove(id) {
                // Drop the running registry - this stops the server
                drop(runtime.running_registry);

                // Delete metadata to prevent auto-recovery of explicitly stopped registries
                Self::delete_metadata_static(&self.state_dir, id)?;

                debug!(
                    registry_id = %id,
                    "Artifact registry stopped"
                );
            } else {
                debug!(
                    registry_id = %id,
                    "Artifact registry not running (already stopped)"
                );
            }

            Ok(())
        }

        /// Checks if a registry is currently running.
        pub async fn is_running(&self, id: &str) -> bool {
            let registries = self.registries.lock().await;
            registries.contains_key(id)
        }

        /// Gets the URL of a running registry.
        ///
        /// # Returns
        /// Registry URL or error if not running
        pub async fn get_registry_url(&self, id: &str) -> Result<String> {
            let registries = self.registries.lock().await;
            registries
                .get(id)
                .map(|runtime| runtime.registry_url.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ServiceResourceNotFound {
                        resource_id: id.to_string(),
                        resource_type: "artifact_registry".to_string(),
                    })
                })
        }

        /// Verifies that a registry resource exists and is healthy.
        pub async fn check_health(&self, id: &str) -> Result<()> {
            let registries = self.registries.lock().await;

            if !registries.contains_key(id) {
                return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                    resource_id: id.to_string(),
                    resource_type: "artifact_registry".to_string(),
                }));
            }

            // Verify storage directory still exists (persistent state check)
            let storage_dir = self.state_dir.join("artifact_registry").join(id);
            if !storage_dir.exists() {
                return Err(AlienError::new(ErrorData::LocalDirectoryError {
                    path: storage_dir.display().to_string(),
                    operation: "health_check".to_string(),
                    reason: "Registry storage directory no longer exists".to_string(),
                }));
            }

            // Verify metadata file exists
            let metadata_file = storage_dir.join("metadata.json");
            if !metadata_file.exists() {
                return Err(AlienError::new(ErrorData::Other {
                    message: format!("Registry metadata file missing for '{}'", id),
                }));
            }

            Ok(())
        }

        /// Gets the binding configuration for an artifact registry resource.
        pub async fn get_binding(
            &self,
            id: &str,
        ) -> Result<alien_core::bindings::ArtifactRegistryBinding> {
            use alien_core::bindings::{ArtifactRegistryBinding, BindingValue};

            let registry_url = self.get_registry_url(id).await?;
            Ok(ArtifactRegistryBinding::local(
                BindingValue::value(registry_url),
                BindingValue::value(None::<String>),
            ))
        }

        /// Deletes registry storage directory and metadata.
        ///
        /// Note: Registry should be stopped before calling this.
        pub async fn delete_registry_storage(&self, id: &str) -> Result<()> {
            let storage_dir = self.state_dir.join("artifact_registry").join(id);

            if storage_dir.exists() {
                tokio::fs::remove_dir_all(&storage_dir)
                    .await
                    .into_alien_error()
                    .context(ErrorData::LocalDirectoryError {
                        path: storage_dir.display().to_string(),
                        operation: "delete".to_string(),
                        reason: "Failed to delete registry storage directory".to_string(),
                    })?;

                debug!(
                    registry_id = %id,
                    path = %storage_dir.display(),
                    "Artifact registry storage deleted"
                );
            } else {
                debug!(
                    registry_id = %id,
                    path = %storage_dir.display(),
                    "Artifact registry storage does not exist (already deleted)"
                );
            }

            // Delete metadata so registry won't recover on restart
            Self::delete_metadata_static(&self.state_dir, id)?;

            Ok(())
        }
    }
}

#[cfg(not(unix))]
mod non_unix_stub {
    use crate::error::{ErrorData, Result};
    use alien_error::AlienError;
    use std::path::PathBuf;

    /// Stub implementation of LocalArtifactRegistryManager for non-Unix platforms.
    ///
    /// Local OCI registries rely on Unix symlinks and are not supported on Windows.
    /// All operations return an appropriate error.
    #[derive(Debug)]
    pub struct LocalArtifactRegistryManager {
        _state_dir: PathBuf,
    }

    impl LocalArtifactRegistryManager {
        pub fn new_with_shutdown(
            state_dir: PathBuf,
            _shutdown_rx: tokio::sync::broadcast::Receiver<()>,
        ) -> (Self, Option<tokio::task::JoinHandle<()>>) {
            (Self { _state_dir: state_dir }, None)
        }

        pub async fn start_registry(&self, id: &str) -> Result<String> {
            Err(AlienError::new(ErrorData::Other {
                message: format!(
                    "Local artifact registries are not supported on Windows (registry '{}')",
                    id
                ),
            }))
        }

        pub async fn remove_registry(&self, _id: &str) -> Result<()> {
            Ok(())
        }

        pub async fn is_running(&self, _id: &str) -> bool {
            false
        }

        pub async fn get_registry_url(&self, id: &str) -> Result<String> {
            Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "artifact_registry".to_string(),
            }))
        }

        pub async fn check_health(&self, id: &str) -> Result<()> {
            Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "artifact_registry".to_string(),
            }))
        }

        pub async fn get_binding(
            &self,
            id: &str,
        ) -> Result<alien_core::bindings::ArtifactRegistryBinding> {
            Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "artifact_registry".to_string(),
            }))
        }

        pub async fn delete_registry_storage(&self, _id: &str) -> Result<()> {
            Ok(())
        }
    }
}

#[cfg(unix)]
pub use unix_impl::LocalArtifactRegistryManager;

#[cfg(not(unix))]
pub use non_unix_stub::LocalArtifactRegistryManager;
