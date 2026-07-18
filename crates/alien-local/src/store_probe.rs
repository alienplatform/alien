//! Shared health probe for versioned local stores (`localkv.v1`, `localqueue.v1`).
//!
//! Opens the store database read-only-in-spirit (same multi-process WAL opt-in
//! as the bindings) and verifies the `meta.format` marker, so a resource whose
//! on-disk database the binding would reject on open is reported unhealthy
//! instead of failing later at binding resolution.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use std::path::Path;

/// Verifies that the store database at `db_path` carries the expected
/// `meta.format` marker.
///
/// The caller decides whether a missing database file is healthy (stores are
/// materialized lazily on first binding open); this probe assumes the file
/// exists.
pub(crate) async fn check_store_format(db_path: &Path, expected_format: &str) -> Result<()> {
    let db = turso::Builder::new_local(&db_path.to_string_lossy())
        // Same explicit opt-in as the local bindings: the store may be held
        // open by other processes while we probe it.
        .experimental_multiprocess_wal(true)
        .build()
        .await
        .into_alien_error()
        .context(ErrorData::LocalDatabaseError {
            database_path: db_path.display().to_string(),
            operation: "open".to_string(),
            reason: format!("Failed to open {expected_format} database for probing"),
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

    // Read the format marker, draining the query to completion (an unfinished
    // turso statement would keep a read transaction open).
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

    if format != expected_format {
        return Err(AlienError::new(ErrorData::LocalDatabaseError {
            database_path: db_path.display().to_string(),
            operation: "read".to_string(),
            reason: format!("Unsupported store format '{format}' (expected '{expected_format}')"),
        }));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_store_format;
    use std::path::PathBuf;

    /// Creates a store database with the given `meta.format` marker and
    /// returns its path (plus the tempdir guard).
    async fn create_store(format: Option<&str>) -> (PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("store.sqlite");
        let db = turso::Builder::new_local(&db_path.to_string_lossy())
            .experimental_multiprocess_wal(true)
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
        if let Some(format) = format {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('format', ?1)",
                (format,),
            )
            .await
            .expect("insert format");
        }
        (db_path, dir)
    }

    #[tokio::test]
    async fn matching_format_is_healthy() {
        let (db_path, _dir) = create_store(Some("localqueue.v1")).await;
        check_store_format(&db_path, "localqueue.v1")
            .await
            .expect("matching format must be healthy");
    }

    #[tokio::test]
    async fn wrong_format_is_unhealthy_and_names_both_formats() {
        let (db_path, _dir) = create_store(Some("localqueue.v2")).await;
        let err = check_store_format(&db_path, "localqueue.v1")
            .await
            .expect_err("wrong format must fail the probe");
        let msg = err.to_string();
        assert!(
            msg.contains("localqueue.v2"),
            "error must name the found format, got: {msg}"
        );
        assert!(
            msg.contains("localqueue.v1"),
            "error must name the expected format, got: {msg}"
        );
    }

    #[tokio::test]
    async fn missing_format_marker_is_unhealthy() {
        let (db_path, _dir) = create_store(None).await;
        check_store_format(&db_path, "localkv.v1")
            .await
            .expect_err("missing format marker must fail the probe");
    }
}
