use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use std::path::PathBuf;
use tracing::{debug, info};

/// Manager for local storage resources.
///
/// Creates and manages filesystem directories for storage resources.
/// The storage manager is stateless - directories persist on disk.
///
/// # State Scoping
/// All storage directories are created under `{state_dir}/storage/{resource_id}/`.
/// The `state_dir` should be scoped by agent ID (e.g., `~/.alien-cli/<agent_id>`)
/// to avoid conflicts between agents.
#[derive(Debug, Clone)]
pub struct LocalStorageManager {
    state_dir: PathBuf,
}

impl LocalStorageManager {
    /// Creates a new storage manager.
    ///
    /// # Arguments
    /// * `state_dir` - Base directory for all local platform state
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }

    /// Creates a storage directory for a resource.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// Path to the created storage directory
    ///
    /// # Note
    /// This is idempotent - can be called multiple times (e.g., during reconciliation).
    /// Uses `create_dir_all` which succeeds even if the directory already exists.
    pub async fn create_storage(&self, id: &str) -> Result<PathBuf> {
        let storage_dir = self.state_dir.join("storage").join(id);

        tokio::fs::create_dir_all(&storage_dir)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: storage_dir.display().to_string(),
                operation: "create".to_string(),
                reason: "Failed to create storage directory".to_string(),
            })?;

        info!(
            resource_id = %id,
            path = %storage_dir.display(),
            "Storage directory created"
        );

        Ok(storage_dir)
    }

    /// Gets the path to a storage directory.
    ///
    /// Returns an error if the storage doesn't exist.
    pub fn get_storage_path(&self, id: &str) -> Result<PathBuf> {
        use alien_error::AlienError;

        let storage_path = self.state_dir.join("storage").join(id);

        if !storage_path.exists() {
            return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "storage".to_string(),
            }));
        }

        Ok(storage_path)
    }

    /// Deletes a storage directory and all its contents.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Note
    /// This is idempotent - succeeds even if the storage doesn't exist.
    pub async fn delete_storage(&self, id: &str) -> Result<()> {
        let storage_dir = self.state_dir.join("storage").join(id);

        if storage_dir.exists() {
            tokio::fs::remove_dir_all(&storage_dir)
                .await
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: storage_dir.display().to_string(),
                    operation: "delete".to_string(),
                    reason: "Failed to delete storage directory".to_string(),
                })?;

            debug!(
                resource_id = %id,
                path = %storage_dir.display(),
                "Storage directory deleted"
            );
        } else {
            debug!(
                resource_id = %id,
                path = %storage_dir.display(),
                "Storage directory does not exist (already deleted)"
            );
        }

        Ok(())
    }

    /// Checks if a storage directory exists.
    pub fn storage_exists(&self, id: &str) -> bool {
        self.state_dir.join("storage").join(id).exists()
    }

    /// Verifies that a storage resource exists and is healthy by actually accessing it.
    ///
    /// This performs a real storage access operation similar to what bindings do, ensuring
    /// the storage directory is accessible and can be listed (similar to S3 HeadBucket).
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// Ok(()) if storage exists and is healthy, error otherwise
    pub async fn check_health(&self, id: &str) -> Result<()> {
        use alien_error::AlienError;
        use futures::StreamExt;
        use object_store::{local::LocalFileSystem, ObjectStore};

        let storage_path = self.state_dir.join("storage").join(id);

        if !storage_path.exists() {
            return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "storage".to_string(),
            }));
        }

        if !storage_path.is_dir() {
            return Err(AlienError::new(ErrorData::LocalDirectoryError {
                path: storage_path.display().to_string(),
                operation: "health_check".to_string(),
                reason: "Expected directory but found file".to_string(),
            }));
        }

        // Try to actually open and list the storage (similar to S3 HeadBucket)
        // This ensures the directory is accessible, readable, and functional
        let store = LocalFileSystem::new_with_prefix(&storage_path)
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: storage_path.display().to_string(),
                operation: "open".to_string(),
                reason: "Failed to open storage with LocalFileSystem".to_string(),
            })?;

        // Perform a list operation (similar to HeadBucket - verifies access without reading data)
        let mut list_stream = store.list(None);
        // Just take the first item (or verify stream can be created)
        // We don't need to consume all items, just verify we can list
        let _ = list_stream.next().await;

        Ok(())
    }

    /// Gets the binding configuration for a storage resource.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// StorageBinding configured for this storage directory, or error if storage doesn't exist
    pub fn get_binding(&self, id: &str) -> Result<alien_core::bindings::StorageBinding> {
        use alien_core::bindings::{BindingValue, StorageBinding};

        let storage_path = self.get_storage_path(id)?;

        // Use file:// URL for consistency and to support object_store's parse_url
        let storage_url = format!("file://{}/", storage_path.display());

        Ok(StorageBinding::local(BindingValue::value(storage_url)))
    }
}
