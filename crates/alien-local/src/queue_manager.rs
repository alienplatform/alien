use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use std::path::PathBuf;
use tracing::{debug, info};

/// Manager for local Queue resources.
///
/// Creates the on-disk directory for each queue resource. The `LocalQueue`
/// binding opens its own SQLite-compatible database (`localqueue.v1`) inside that
/// directory and performs all operations directly.
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
    /// The binding will open its own SQLite database (`localqueue.sqlite`)
    /// inside this directory.
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

    /// Verifies that a queue resource exists and its store is usable.
    ///
    /// Mirrors the KV health check:
    /// - Missing resource directory → `ServiceResourceNotFound`.
    /// - Path exists but is a file → `LocalDirectoryError`.
    /// - Directory exists but `localqueue.sqlite` is absent → **healthy**: the
    ///   `LocalQueue` binding materializes the database on first use, so a
    ///   not-yet-opened store is a valid state, not a failure.
    /// - `localqueue.sqlite` present with a wrong/unreadable `meta.format` →
    ///   error, so the resource is not reported healthy when the trigger
    ///   service or app would fail to open it on `LocalQueue::new`. The probe
    ///   opens with the same multi-process WAL opt-in as the binding, so it is
    ///   safe alongside the worker runtime and trigger service.
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

        let db_path = queue_path.join("localqueue.sqlite");

        // Store not yet materialized (directory created, no binding opened
        // yet): healthy.
        if !db_path.exists() {
            return Ok(());
        }

        crate::store_probe::check_store_format(&db_path, "localqueue.v1").await
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

#[cfg(test)]
mod tests {
    use super::LocalQueueManager;

    #[tokio::test]
    async fn health_check_missing_resource_is_unhealthy() {
        let state_dir = tempfile::tempdir().expect("tempdir");
        let manager = LocalQueueManager::new(state_dir.path().to_path_buf());

        manager
            .check_health("missing")
            .await
            .expect_err("a never-created queue must be unhealthy");
    }

    #[tokio::test]
    async fn health_check_empty_directory_is_healthy() {
        let state_dir = tempfile::tempdir().expect("tempdir");
        let manager = LocalQueueManager::new(state_dir.path().to_path_buf());

        manager.create_queue("q").await.expect("create queue");
        // No binding has opened the store yet: localqueue.sqlite is absent.
        manager
            .check_health("q")
            .await
            .expect("a not-yet-opened queue must be healthy");
    }

    #[tokio::test]
    async fn health_check_valid_store_is_healthy() {
        let state_dir = tempfile::tempdir().expect("tempdir");
        let manager = LocalQueueManager::new(state_dir.path().to_path_buf());

        let data_dir = manager.create_queue("q").await.expect("create queue");
        // Materialize a real localqueue.v1 store through the actual binding.
        alien_bindings::providers::queue::local::LocalQueue::new(data_dir)
            .await
            .expect("open local queue");

        manager
            .check_health("q")
            .await
            .expect("a valid localqueue.v1 store must be healthy");
    }

    #[tokio::test]
    async fn health_check_wrong_format_store_is_unhealthy() {
        let state_dir = tempfile::tempdir().expect("tempdir");
        let manager = LocalQueueManager::new(state_dir.path().to_path_buf());

        let data_dir = manager.create_queue("q").await.expect("create queue");
        let db_path = data_dir.join("localqueue.sqlite");
        let db = turso::Builder::new_local(&db_path.to_string_lossy())
            .build()
            .await
            .expect("build db");
        let conn = db.connect().expect("connect");
        conn.execute(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            (),
        )
        .await
        .expect("create meta");
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('format', 'localqueue.v2')",
            (),
        )
        .await
        .expect("insert format");

        let err = manager
            .check_health("q")
            .await
            .expect_err("a wrong-format store must be unhealthy");
        let msg = err.to_string();
        assert!(
            msg.contains("localqueue.v2") && msg.contains("localqueue.v1"),
            "error must name found and expected formats, got: {msg}"
        );
    }
}
