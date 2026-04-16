//! Alien Local Platform Services
//!
//! This crate provides service managers for running Alien applications on the local platform.
//! Services handle infrastructure provisioning (creating directories, starting processes, etc.)
//! while bindings in `alien-bindings` handle the actual resource operations.
//!
//! # Architecture
//!
//! Local platform follows the same controller pattern as cloud platforms:
//! - **Controllers** (in `alien-infra`) manage resource lifecycles through state machines
//! - **Service Managers** (this crate) handle low-level operations (like cloud APIs)
//! - **Bindings** (in `alien-bindings`) provide resource APIs to user code
//!
//! # Entry Point
//!
//! The main entry point is `LocalBindingsProvider::new(state_dir)` which:
//! - Creates all service managers
//! - Starts background tasks for auto-recovery
//! - Implements `BindingsProviderApi` for function runtimes
//! - Provides manager accessors for controllers
//!
//! # Key Components
//!
//! - `LocalBindingsProvider` - Main entry point, implements `BindingsProviderApi`
//! - `LocalStorageManager` - Creates storage directories
//! - `LocalKvManager` - Creates KV database directories
//! - `LocalVaultManager` - Creates vault directories
//! - `LocalFunctionManager` - Manages function runtime tasks with auto-recovery
//! - `LocalArtifactRegistryManager` - Manages OCI registry servers with auto-recovery
//!
//! # State Directory Structure
//!
//! All state is scoped by agent ID under `~/.alien-cli/<agent_id>/`:
//!
//! ```text
//! ~/.alien-cli/<agent_id>/
//! ├── state.json                      # Deployment state
//! ├── storage/
//! │   └── {resource_id}/              # Storage directory
//! ├── kv/
//! │   └── {resource_id}/              # KV database
//! ├── vault/
//! │   └── {resource_id}/              # Vault directory
//! ├── functions/
//! │   └── {function_id}/
//! │       ├── metadata.json           # Recovery metadata
//! │       └── ...                     # Extracted OCI image
//! └── artifact_registry/
//!     └── {registry_id}/
//!         ├── metadata.json           # Recovery metadata
//!         └── ...                     # Registry data (blobs, manifests)
//! ```

mod artifact_registry_manager;
mod container_manager;
mod error;
mod function_manager;
mod kv_manager;
mod local_bindings_provider;
mod queue_manager;
mod storage_manager;
pub mod trigger_service;
mod vault_manager;

pub use artifact_registry_manager::LocalArtifactRegistryManager;

pub use container_manager::{
    BindMount, ContainerConfig, ContainerInfo, ContainerMetadata, LocalContainerManager,
};
pub use error::{ErrorData, Result};
pub use function_manager::LocalFunctionManager;
pub use kv_manager::LocalKvManager;
pub use local_bindings_provider::LocalBindingsProvider;
pub use queue_manager::LocalQueueManager;
pub use storage_manager::LocalStorageManager;
pub use vault_manager::LocalVaultManager;
