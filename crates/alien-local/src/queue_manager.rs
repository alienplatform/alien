use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use std::path::PathBuf;
use tracing::{debug, info};

/// Manager for local Queue resources.
///
/// Creates directories for sled databases. The binding opens its own database
/// handle and performs all operations directly.
///
/// # State Scoping
/// All queue databases are created under `{state_dir}/queue/{resource_id}/`.
/// The `state_dir` should be scoped by agent ID (e.g., `~/.alien-cli/<agent_id>`)
/// to avoid conflicts between agents.
#[derive(Debug, Clone)]
pub struct LocalQueueManager {
    state_dir: PathBuf,
}

impl LocalQueueManager {
    /// Creates a new queue manager.
    ///
    /// # Arguments
    /// * `state_dir` - Base directory for all local platform state
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }

    /// Creates a queue database directory for a resource.
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
    pub async fn create_queue(&self, id: &str) -> Result<PathBuf> {
        let db_path = self.state_dir.join("queue").join(id);

        tokio::fs::create_dir_all(&db_path)
            .await
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: db_path.display().to_string(),
                operation: "create".to_string(),
                reason: "Failed to create queue database directory".to_string(),
            })?;

        info!(
            resource_id = %id,
            path = %db_path.display(),
            "Queue database directory created"
        );

        Ok(db_path)
    }

    /// Gets the path to a queue database.
    ///
    /// Returns an error if the database doesn't exist.
    pub fn get_queue_path(&self, id: &str) -> Result<PathBuf> {
        let queue_path = self.state_dir.join("queue").join(id);

        if !queue_path.exists() {
            return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "queue".to_string(),
            }));
        }

        Ok(queue_path)
    }

    /// Deletes a queue database directory and all its contents.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Note
    /// This is idempotent - succeeds even if the queue doesn't exist.
    pub async fn delete_queue(&self, id: &str) -> Result<()> {
        let db_path = self.state_dir.join("queue").join(id);

        if db_path.exists() {
            tokio::fs::remove_dir_all(&db_path)
                .await
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: db_path.display().to_string(),
                    operation: "delete".to_string(),
                    reason: "Failed to delete queue database directory".to_string(),
                })?;

            debug!(
                resource_id = %id,
                path = %db_path.display(),
                "Queue database deleted"
            );
        } else {
            debug!(
                resource_id = %id,
                path = %db_path.display(),
                "Queue database does not exist (already deleted)"
            );
        }

        Ok(())
    }

    /// Checks if a queue database exists on disk.
    pub fn queue_exists(&self, id: &str) -> bool {
        self.state_dir.join("queue").join(id).exists()
    }

    /// Verifies that a queue resource exists and is accessible.
    ///
    /// Checks that the queue directory exists and is readable. Does NOT open
    /// the sled database — sled acquires an exclusive file lock which would
    /// conflict with the function runtime and trigger service that hold
    /// long-lived handles to the same database.
    pub async fn check_health(&self, id: &str) -> Result<()> {
        let queue_path = self.state_dir.join("queue").join(id);

        if !queue_path.exists() {
            return Err(AlienError::new(ErrorData::ServiceResourceNotFound {
                resource_id: id.to_string(),
                resource_type: "queue".to_string(),
            }));
        }

        if !queue_path.is_dir() {
            return Err(AlienError::new(ErrorData::LocalDirectoryError {
                path: queue_path.display().to_string(),
                operation: "health_check".to_string(),
                reason: "Expected directory but found file".to_string(),
            }));
        }

        // Verify the directory is readable (don't open sled — it takes an exclusive lock)
        std::fs::read_dir(&queue_path).into_alien_error().context(
            ErrorData::LocalDirectoryError {
                path: queue_path.display().to_string(),
                operation: "read".to_string(),
                reason: "Failed to read queue directory".to_string(),
            },
        )?;

        Ok(())
    }

    /// Gets the binding configuration for a queue resource.
    ///
    /// # Arguments
    /// * `id` - Resource identifier
    ///
    /// # Returns
    /// QueueBinding configured for this queue database, or error if queue doesn't exist
    pub fn get_binding(&self, id: &str) -> Result<alien_core::bindings::QueueBinding> {
        use alien_core::bindings::{BindingValue, QueueBinding};

        let queue_path = self.get_queue_path(id)?;

        Ok(QueueBinding::local(BindingValue::value(
            queue_path.to_string_lossy().to_string(),
        )))
    }
}
