//! Local infrastructure controllers.

pub mod ai;
pub mod artifact_registry;
pub mod compute_cluster;
pub mod container;
mod container_utils;
pub mod daemon;
pub mod kv;
pub mod postgres;
pub mod queue;
pub mod sandbox;
pub mod service_account;
pub mod storage;
pub mod vault;
pub mod worker;

pub mod core {
    pub use crate::LocalServiceProvider;
    pub use alien_infra_core::*;
}

pub mod error {
    pub use alien_infra_core::{ErrorData, Result};
}

pub use error::Result;

use std::sync::Arc;

/// Local services required by Local infrastructure controllers.
pub trait LocalServiceProvider: Send + Sync {
    fn get_local_storage_manager(&self) -> Option<Arc<alien_local::LocalStorageManager>>;
    fn get_local_kv_manager(&self) -> Option<Arc<alien_local::LocalKvManager>>;
    fn get_local_postgres_manager(&self) -> Option<Arc<alien_local::LocalPostgresManager>>;
    fn get_local_vault_manager(&self) -> Option<Arc<alien_local::LocalVaultManager>>;
    fn get_local_worker_manager(&self) -> Option<Arc<alien_local::LocalWorkerManager>>;
    fn get_local_artifact_registry_manager(
        &self,
    ) -> Option<Arc<alien_local::LocalArtifactRegistryManager>>;
    fn get_local_container_manager(&self) -> Option<Arc<alien_local::LocalContainerManager>>;
    fn get_local_sandbox_manager(&self) -> Option<Arc<alien_local::LocalSandboxManager>>;
    fn get_local_queue_manager(&self) -> Option<Arc<alien_local::LocalQueueManager>>;
    fn get_local_bindings_provider(&self) -> Option<Arc<alien_local::LocalBindingsProvider>>;
}
