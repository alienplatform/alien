use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use std::path::PathBuf;
use tracing::{debug, info};

/// Manager for local KV resources.
///
/// Creates the on-disk directory for each KV resource. The `LocalKv` binding
/// opens its own SQLite-compatible database (`localkv.v1`) inside that
/// directory and performs all operations directly.
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
    /// The binding will open its own SQLite-compatible database
    /// (`localkv.sqlite`) inside this directory.
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

    /// Verifies that a KV resource exists and is healthy by inspecting its
    /// store against the `localkv.v1` on-disk contract.
    ///
    /// Mirrors the format check `LocalKv` performs on open: it opens
    /// `<kv_path>/localkv.sqlite` with turso (SQLite-compatible file format),
    /// reads the `('format', ...)` row from the `meta` table, and reports
    /// healthy only when it equals `localkv.v1`. turso exposes no read-only
    /// open, so the probe is a plain open that only ever runs `SELECT`;
    /// turso's multi-process WAL mode (experimental upstream, enabled
    /// explicitly here exactly as in the binding) plus a busy_timeout let it
    /// run concurrently with the worker runtime and trigger service that hold
    /// live read-write handles to the same file.
    ///
    /// Health-vs-not decisions (faithful to the previous embedded-KV check):
    /// - Missing resource directory → `ServiceResourceNotFound` (the resource
    ///   was never created / was deleted — same as the old check's
    ///   `!kv_path.exists()` branch).
    /// - Path exists but is a file → `LocalDirectoryError`.
    /// - Directory exists but `localkv.sqlite` is absent → **healthy**. The
    ///   controller creates the resource directory (`create_kv`) and reaches
    ///   this `Ready`-state health check before any binding has opened the
    ///   store, so the file only materializes on first `LocalKv::new`. The old
    ///   embedded-KV check opened (and thereby created) its database on this
    ///   empty directory and returned Ok, so reporting healthy here preserves
    ///   that behavior; a not-yet-opened store is a valid state, not a failure.
    /// - `localkv.sqlite` present with a wrong/unreadable format → error.
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

        let db_path = kv_path.join("localkv.sqlite");

        // Store not yet materialized (directory created, no binding opened yet):
        // healthy, matching the old open-creates-the-database behavior.
        if !db_path.exists() {
            return Ok(());
        }

        let db = turso::Builder::new_local(&db_path.to_string_lossy())
            // Same explicit opt-in as the LocalKv binding: the store may be
            // held open by other processes while we probe it.
            .experimental_multiprocess_wal(true)
            .build()
            .await
            .into_alien_error()
            .context(ErrorData::LocalDatabaseError {
                database_path: db_path.display().to_string(),
                operation: "open".to_string(),
                reason: "Failed to open localkv.v1 database for probing".to_string(),
            })?;
        let conn = db
            .connect()
            .into_alien_error()
            .context(ErrorData::LocalDatabaseError {
                database_path: db_path.display().to_string(),
                operation: "open".to_string(),
                reason: "Failed to open probe connection".to_string(),
            })?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .into_alien_error()
            .context(ErrorData::LocalDatabaseError {
                database_path: db_path.display().to_string(),
                operation: "open".to_string(),
                reason: "Failed to set busy_timeout".to_string(),
            })?;

        // Read the format marker, draining the query to completion (an
        // unfinished turso statement would keep a read transaction open).
        let read_error = |reason: &str| ErrorData::LocalDatabaseError {
            database_path: db_path.display().to_string(),
            operation: "read".to_string(),
            reason: reason.to_string(),
        };
        let mut rows = conn
            .query("SELECT value FROM meta WHERE key = 'format'", ())
            .await
            .into_alien_error()
            .context(read_error("Failed to read format marker from meta table"))?;
        let mut format: Option<String> = None;
        while let Some(row) = rows
            .next()
            .await
            .into_alien_error()
            .context(read_error("Failed to read format marker row"))?
        {
            if format.is_none() {
                format = row.get_value(0).ok().and_then(|value| match value {
                    turso::Value::Text(s) => Some(s),
                    _ => None,
                });
            }
        }
        let format = format
            .ok_or_else(|| AlienError::new(read_error("Format marker missing from meta table")))?;

        if format != "localkv.v1" {
            return Err(AlienError::new(ErrorData::LocalDatabaseError {
                database_path: db_path.display().to_string(),
                operation: "read".to_string(),
                reason: format!("Unsupported store format '{format}' (expected 'localkv.v1')"),
            }));
        }

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
