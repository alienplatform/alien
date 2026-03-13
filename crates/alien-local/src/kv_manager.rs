use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use std::path::PathBuf;
use tracing::{debug, info};

/// Manager for local KV resources.
///
/// Creates directories for sled databases. The binding opens its own database
/// handle and performs all operations directly.
///
/// # State Scoping
/// All KV databases are created under `{state_dir}/kv/{resource_id}/`.
/// The `state_dir` should be scoped by agent ID (e.g., `~/.alien-cli/<agent_id>`)
/// to avoid conflicts between agents.
#[derive(Debug, Clone)]
pub struct LocalKvManager {
    state_dir: PathBuf,
}

impl LocalKvManager {
    /// Creates a new KV manager.
    ///
    /// # Arguments
    /// * `state_dir` - Base directory for all local platform state
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }

    /// Creates a KV database directory for a resource.
    ///
    /// The binding will open its own sled database handle using this path.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// Path to the database directory
    ///
    /// # Note
    /// This is idempotent - can be called multiple times (e.g., during reconciliation).
    /// Uses `create_dir_all` which succeeds even if the directory already exists.
    pub async fn create_kv(&self, id: &str) -> Result<PathBuf> {
        let db_path = self.state_dir.join("kv").join(id);

        // Create the KV database directory (idempotent)
        // This creates both parent ({state_dir}/kv/) and the database directory itself
        tokio::fs::create_dir_all(&db_path)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: db_path.display().to_string(),
                operation: "create".to_string(),
                reason: "Failed to create KV database directory".to_string(),
            })?;

        info!(
            resource_id = %id,
            path = %db_path.display(),
            "KV database directory created"
        );

        Ok(db_path)
    }

    /// Gets the path to a KV database.
    ///
    /// Returns an error if the database doesn't exist.
    pub fn get_kv_path(&self, id: &str) -> Result<PathBuf> {
        use alien_error::AlienError;

        let kv_path = self.state_dir.join("kv").join(id);

        if !kv_path.exists() {
            return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "kv".to_string(),
            }));
        }

        Ok(kv_path)
    }

    /// Deletes a KV database directory and all its contents.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Note
    /// This is idempotent - succeeds even if the KV doesn't exist.
    pub async fn delete_kv(&self, id: &str) -> Result<()> {
        let db_path = self.state_dir.join("kv").join(id);

        if db_path.exists() {
            tokio::fs::remove_dir_all(&db_path)
                .await
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: db_path.display().to_string(),
                    operation: "delete".to_string(),
                    reason: "Failed to delete KV database directory".to_string(),
                })?;

            debug!(
                resource_id = %id,
                path = %db_path.display(),
                "KV database deleted"
            );
        } else {
            debug!(
                resource_id = %id,
                path = %db_path.display(),
                "KV database does not exist (already deleted)"
            );
        }

        Ok(())
    }

    /// Checks if a KV database exists on disk.
    pub fn kv_exists(&self, id: &str) -> bool {
        self.state_dir.join("kv").join(id).exists()
    }

    /// Verifies that a KV resource exists and is healthy by actually opening it.
    ///
    /// This performs a real 'open' operation similar to what bindings do, ensuring
    /// the KV database is accessible and can be opened with sled.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// Ok(()) if KV exists and is healthy, error otherwise
    pub async fn check_health(&self, id: &str) -> Result<()> {
        use alien_error::AlienError;

        let kv_path = self.state_dir.join("kv").join(id);

        if !kv_path.exists() {
            return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "kv".to_string(),
            }));
        }

        if !kv_path.is_dir() {
            return Err(AlienError::new(ErrorData::LocalDirectoryError {
                path: kv_path.display().to_string(),
                operation: "health_check".to_string(),
                reason: "Expected directory but found file".to_string(),
            }));
        }

        // Try to actually open the sled database
        // This ensures the database is accessible, not corrupted, etc.
        sled::open(&kv_path)
            .into_alien_error()
            .context(ErrorData::LocalDatabaseError {
                database_path: kv_path.display().to_string(),
                operation: "open".to_string(),
                reason: "Failed to open sled database".to_string(),
            })?;

        Ok(())
    }

    /// Gets the binding configuration for a KV resource.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// KvBinding configured for this KV database, or error if KV doesn't exist
    pub fn get_binding(&self, id: &str) -> Result<alien_core::bindings::KvBinding> {
        use alien_core::bindings::{BindingValue, KvBinding};

        let kv_path = self.get_kv_path(id)?;

        Ok(KvBinding::local(BindingValue::value(
            kv_path.to_string_lossy().to_string(),
        )))
    }
}
