//! Local bindings provider - the single entry point for local platform services.
//!
//! This struct:
//! - Holds all service managers
//! - Implements `BindingsProviderApi` for function runtimes
//! - Provides manager accessors for controllers via `PlatformServiceProvider`
//! - Handles graceful shutdown coordination

use crate::error::Result;
use crate::{
    LocalArtifactRegistryManager, LocalContainerManager, LocalFunctionManager, LocalKvManager,
    LocalStorageManager, LocalVaultManager,
};
use alien_bindings::{
    error::ErrorData as BindingsErrorData,
    providers::{
        artifact_registry::local::LocalArtifactRegistry,
        container::LocalContainer as LocalContainerBinding, kv::local::LocalKv,
        storage::local::LocalStorage, vault::local::LocalVault,
    },
    traits::{
        ArtifactRegistry, BindingsProviderApi, Build, Container, Function, Kv, Queue,
        ServiceAccount, Storage, Vault,
    },
};
use alien_core::bindings::{KvBinding, VaultBinding};
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Local bindings provider - manages all local platform services.
///
/// This is the single entry point for the local platform. It:
/// - Creates and holds all service managers
/// - Implements `BindingsProviderApi` for function runtimes to access resources
/// - Provides manager accessors for controllers via `PlatformServiceProvider`
/// - Coordinates graceful shutdown of background tasks
///
/// # State Directory Structure
///
/// All state is scoped by agent ID under the provided state directory:
///
/// ```text
/// {state_dir}/
/// ├── storage/{resource_id}/     # Storage directories
/// ├── kv/{resource_id}/          # KV databases  
/// ├── vault/{resource_id}/       # Vault directories
/// ├── functions/{function_id}/   # Extracted OCI images + metadata
/// └── artifact_registry/{id}/    # Registry data
/// ```
#[derive(Debug)]
pub struct LocalBindingsProvider {
    storage_manager: Arc<LocalStorageManager>,
    kv_manager: Arc<LocalKvManager>,
    vault_manager: Arc<LocalVaultManager>,
    artifact_registry_manager: Arc<LocalArtifactRegistryManager>,
    /// Container manager for Docker containers (optional - created lazily)
    container_manager: RwLock<Option<Arc<LocalContainerManager>>>,
    /// Function manager is set after construction to break circular dependency
    function_manager: RwLock<Option<Arc<LocalFunctionManager>>>,
    /// Shutdown signal sender
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    /// Background task handles for graceful shutdown
    background_tasks: Mutex<Vec<JoinHandle<()>>>,
    /// State directory for creating managers lazily
    state_dir: PathBuf,
}

impl Clone for LocalBindingsProvider {
    fn clone(&self) -> Self {
        Self {
            storage_manager: self.storage_manager.clone(),
            kv_manager: self.kv_manager.clone(),
            vault_manager: self.vault_manager.clone(),
            artifact_registry_manager: self.artifact_registry_manager.clone(),
            container_manager: RwLock::new(self.container_manager.read().unwrap().clone()),
            function_manager: RwLock::new(self.function_manager.read().unwrap().clone()),
            shutdown_tx: self.shutdown_tx.clone(),
            background_tasks: Mutex::new(Vec::new()), // Don't clone JoinHandles
            state_dir: self.state_dir.clone(),
        }
    }
}

impl LocalBindingsProvider {
    /// Creates a new local bindings provider with all services initialized.
    ///
    /// This is the main entry point for the local platform. It:
    /// 1. Creates all service managers
    /// 2. Starts background tasks for auto-recovery
    /// 3. Sets up shutdown coordination
    ///
    /// # Arguments
    /// * `state_dir` - Base directory for all local platform state. Should be scoped
    ///   by agent ID (e.g., `~/.alien-cli/<agent_id>`).
    ///
    /// # Returns
    /// (Provider, Optional log receiver for streaming logs in dev mode)
    ///
    /// # Example
    /// ```no_run
    /// use alien_local::LocalBindingsProvider;
    /// use std::path::PathBuf;
    ///
    /// # fn example() -> alien_local::Result<Arc<LocalBindingsProvider>> {
    /// let state_dir = PathBuf::from("~/.alien-cli/ag_1234567890abcdef");
    /// let provider = LocalBindingsProvider::new(&state_dir)?;
    /// # Ok(provider)
    /// # }
    /// ```
    pub fn new(state_dir: &Path) -> Result<Arc<Self>> {
        let state_dir = state_dir.to_path_buf();

        // Create shared shutdown signal for all services
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

        // Create simple managers (no background tasks)
        let storage_manager = Arc::new(LocalStorageManager::new(state_dir.clone()));
        let kv_manager = Arc::new(LocalKvManager::new(state_dir.clone()));
        let vault_manager = Arc::new(LocalVaultManager::new(state_dir.clone()));

        // Create artifact registry manager (has background task)
        let (artifact_registry_manager, registry_task) =
            LocalArtifactRegistryManager::new_with_shutdown(
                state_dir.clone(),
                shutdown_tx.subscribe(),
            );
        let artifact_registry_manager = Arc::new(artifact_registry_manager);

        // Create the provider (without function_manager and container_manager initially)
        let provider = Arc::new(Self {
            storage_manager: storage_manager.clone(),
            kv_manager: kv_manager.clone(),
            vault_manager: vault_manager.clone(),
            artifact_registry_manager: artifact_registry_manager.clone(),
            container_manager: RwLock::new(None),
            function_manager: RwLock::new(None),
            shutdown_tx: shutdown_tx.clone(),
            background_tasks: Mutex::new(Vec::new()),
            state_dir: state_dir.clone(),
        });

        // Create function manager with the provider (for bindings access)
        // ARC polling is configured via environment variables (ALIEN_COMMANDS_POLLING_*, ALIEN_AGENT_ID)
        let (function_manager, function_task) = LocalFunctionManager::new_with_shutdown(
            state_dir,
            provider.clone(),
            shutdown_tx.subscribe(),
        );
        let function_manager = Arc::new(function_manager);

        // Complete the circular reference
        *provider.function_manager.write().unwrap() = Some(function_manager);

        // Store background task handles
        {
            let mut tasks = provider.background_tasks.lock().unwrap();
            if let Some(task) = function_task {
                tasks.push(task);
            }
            if let Some(task) = registry_task {
                tasks.push(task);
            }
        }

        Ok(provider)
    }

    /// Triggers graceful shutdown and waits for all background tasks to complete.
    pub async fn shutdown(self: Arc<Self>) {
        // Trigger shutdown for all background tasks
        let _ = self.shutdown_tx.send(());

        // Take ownership of background tasks
        let tasks: Vec<_> = {
            let mut guard = self.background_tasks.lock().unwrap();
            std::mem::take(&mut *guard)
        };

        // Wait for all tasks to complete
        for task in tasks {
            if let Err(e) = task.await {
                tracing::warn!("Background task failed during shutdown: {:?}", e);
            }
        }
    }

    // ─────────────── Manager Accessors (for PlatformServiceProvider) ───────────────

    /// Returns the storage manager.
    pub fn storage_manager(&self) -> &Arc<LocalStorageManager> {
        &self.storage_manager
    }

    /// Returns the KV manager.
    pub fn kv_manager(&self) -> &Arc<LocalKvManager> {
        &self.kv_manager
    }

    /// Returns the vault manager.
    pub fn vault_manager(&self) -> &Arc<LocalVaultManager> {
        &self.vault_manager
    }

    /// Returns the function manager.
    ///
    /// # Panics
    /// Panics if called before initialization is complete.
    pub fn function_manager(&self) -> Arc<LocalFunctionManager> {
        self.function_manager
            .read()
            .unwrap()
            .clone()
            .expect("function_manager accessed before initialization")
    }

    /// Returns the artifact registry manager.
    pub fn artifact_registry_manager(&self) -> &Arc<LocalArtifactRegistryManager> {
        &self.artifact_registry_manager
    }

    /// Returns the container manager, creating it lazily if needed.
    ///
    /// The container manager is created lazily because it connects to Docker,
    /// which may not be available/needed in all scenarios (e.g., function-only stacks).
    pub fn container_manager(&self) -> Option<Arc<LocalContainerManager>> {
        // Try to get existing manager
        {
            let guard = self.container_manager.read().unwrap();
            if let Some(ref mgr) = *guard {
                return Some(mgr.clone());
            }
        }

        // Create manager lazily
        // Container logs are handled via docker logs API → OTLP (not channels)
        match LocalContainerManager::new(self.state_dir.clone()) {
            Ok(mgr) => {
                let mgr = Arc::new(mgr);
                *self.container_manager.write().unwrap() = Some(mgr.clone());
                Some(mgr)
            }
            Err(e) => {
                tracing::warn!("Failed to create LocalContainerManager: {:?}", e);
                None
            }
        }
    }
}

#[async_trait]
impl BindingsProviderApi for LocalBindingsProvider {
    async fn load_storage(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn Storage>> {
        use alien_core::bindings::StorageBinding;

        // Query storage manager for binding (fails if storage doesn't exist)
        let binding = self.storage_manager.get_binding(binding_name).context(
            BindingsErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: format!("Directory not found for '{}'", binding_name),
            },
        )?;

        // Extract storage path from binding
        let storage_path = match binding {
            StorageBinding::Local(config) => config
                .storage_path
                .into_value(binding_name, "storage_path")
                .context(BindingsErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Invalid storage_path in binding".to_string(),
                })?,
            _ => {
                return Err(AlienError::new(BindingsErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Expected Local storage binding variant".to_string(),
                }));
            }
        };

        // Create LocalStorage from path
        let storage =
            LocalStorage::new(storage_path).context(BindingsErrorData::BindingSetupFailed {
                binding_type: "storage".to_string(),
                reason: format!("Failed to initialize binding for '{}'", binding_name),
            })?;

        Ok(Arc::new(storage))
    }

    async fn load_build(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn Build>> {
        Err(AlienError::new(BindingsErrorData::OperationNotSupported {
            operation: "load_build".to_string(),
            reason: format!(
                "Build resource '{}' not yet implemented for local platform",
                binding_name
            ),
        }))
    }

    async fn load_artifact_registry(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn ArtifactRegistry>> {
        let binding = self
            .artifact_registry_manager
            .get_binding(binding_name)
            .await
            .context(BindingsErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: format!("Artifact registry '{}' not running", binding_name),
            })?;

        let registry = LocalArtifactRegistry::new(binding_name.to_string(), binding)
            .await
            .context(BindingsErrorData::BindingSetupFailed {
                binding_type: "artifact_registry".to_string(),
                reason: format!("Failed to initialize binding for '{}'", binding_name),
            })?;

        Ok(Arc::new(registry))
    }

    async fn load_vault(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn Vault>> {
        let binding = self.vault_manager.get_binding(binding_name).context(
            BindingsErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: format!("Directory not found for '{}'", binding_name),
            },
        )?;

        let vault_dir = match binding {
            VaultBinding::Local(config) => {
                let dir_str = config
                    .data_dir
                    .into_value(binding_name, "data_dir")
                    .context(BindingsErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Invalid data_dir in binding".to_string(),
                    })?;
                PathBuf::from(dir_str)
            }
            _ => {
                return Err(AlienError::new(BindingsErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Expected Local vault binding variant".to_string(),
                }));
            }
        };

        let vault = LocalVault::new(binding_name.to_string(), vault_dir);
        Ok(Arc::new(vault))
    }

    async fn load_kv(&self, binding_name: &str) -> alien_bindings::error::Result<Arc<dyn Kv>> {
        let binding = self.kv_manager.get_binding(binding_name).context(
            BindingsErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: format!("Database not found for '{}'", binding_name),
            },
        )?;

        let db_path = match binding {
            KvBinding::Local(config) => {
                let path_str = config
                    .data_dir
                    .into_value(binding_name, "data_dir")
                    .context(BindingsErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Invalid data_dir in binding".to_string(),
                    })?;
                PathBuf::from(path_str)
            }
            _ => {
                return Err(AlienError::new(BindingsErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Expected Local KV binding variant".to_string(),
                }));
            }
        };

        let kv = LocalKv::new(db_path)
            .await
            .context(BindingsErrorData::BindingSetupFailed {
                binding_type: "kv".to_string(),
                reason: format!("Failed to initialize binding for '{}'", binding_name),
            })?;

        Ok(Arc::new(kv))
    }

    async fn load_queue(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn Queue>> {
        Err(AlienError::new(BindingsErrorData::OperationNotSupported {
            operation: "load_queue".to_string(),
            reason: format!(
                "Queue resource '{}' not yet implemented for local platform",
                binding_name
            ),
        }))
    }

    async fn load_function(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn Function>> {
        let function_manager = {
            let guard = self.function_manager.read().unwrap();
            guard.clone().ok_or_else(|| {
                AlienError::new(BindingsErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Function manager not initialized".to_string(),
                })
            })?
        };

        let _binding = function_manager.get_binding(binding_name).await.context(
            BindingsErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: format!("Function '{}' not running", binding_name),
            },
        )?;

        // LocalFunction binding implementation not yet available
        Err(AlienError::new(BindingsErrorData::OperationNotSupported {
            operation: "load_function".to_string(),
            reason: format!(
                "Function binding implementation '{}' not yet available for local platform",
                binding_name
            ),
        }))
    }

    async fn load_container(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn Container>> {
        use alien_core::bindings::ContainerBinding;

        let container_manager = self.container_manager().ok_or_else(|| {
            AlienError::new(BindingsErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Container manager not available (Docker not running?)".to_string(),
            })
        })?;

        let binding = container_manager.get_binding(binding_name).await.context(
            BindingsErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: format!("Container '{}' not running", binding_name),
            },
        )?;

        match binding {
            ContainerBinding::Local(local_binding) => {
                let container = LocalContainerBinding::new(local_binding).context(
                    BindingsErrorData::BindingSetupFailed {
                        binding_type: "container".to_string(),
                        reason: format!("Failed to initialize binding for '{}'", binding_name),
                    },
                )?;
                Ok(Arc::new(container))
            }
            _ => Err(AlienError::new(BindingsErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Expected Local container binding variant".to_string(),
            })),
        }
    }

    async fn load_service_account(
        &self,
        binding_name: &str,
    ) -> alien_bindings::error::Result<Arc<dyn ServiceAccount>> {
        Err(AlienError::new(BindingsErrorData::OperationNotSupported {
            operation: "load_service_account".to_string(),
            reason: format!(
                "ServiceAccount '{}' not applicable for local platform (no permissions system)",
                binding_name
            ),
        }))
    }
}
