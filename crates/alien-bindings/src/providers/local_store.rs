//! Shared turso plumbing for the local KV and queue providers.
//!
//! Both local stores are `<dataDir>/<file>.sqlite` databases (turso writes the
//! SQLite-compatible file format) that share the same open/init handshake:
//! multi-process WAL mode + `busy_timeout`, a `meta(key,value)` table carrying
//! a `format` marker, and a fail-fast check that the marker matches the format
//! this build understands. Only the provider-specific tables and every
//! operational SQL statement stay in the provider modules; everything above
//! lives here so there is exactly one home for the on-disk-format handshake.
//!
//! # Engine and multi-process mode
//!
//! The engine is turso (SQLite-compatible, async-native — no `spawn_blocking`
//! boundary is needed). Multi-process safety comes from turso's
//! `experimental_multiprocess_wal` capability, enabled explicitly on every
//! open below. Per turso's own documentation this mode is **experimental**:
//! its cross-process WAL coordination format may change between turso
//! releases, and it is supported on 64-bit Unix targets. That trade-off is
//! acceptable for these local development stores (disposable state), and the
//! multi-handle concurrency tests in the kv/queue provider modules are the
//! gate that this mode actually delivers the pinned semantics.
//!
//! # Statement hygiene (important)
//!
//! turso statements keep their implicit transaction open until they are driven
//! to completion. Every query in the providers MUST be drained (`next()` until
//! `None` — use [`query_all`]) before the connection is used for anything
//! else; an undrained `Rows` blocks writers on every handle sharing the file
//! and freezes the connection's read snapshot.
use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context as _, IntoAlienError as _};
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;
use turso::{Connection, Database, IntoParams, Value};

/// Writers wait this long for the write lock before failing with `Busy`.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Static description of one concrete on-disk store format.
#[derive(Debug)]
pub(crate) struct StoreSpec {
    /// File name inside the data directory (e.g. `"localkv.sqlite"`).
    pub db_filename: &'static str,
    /// Format marker stored in `meta` (e.g. `"localkv.v1"`).
    pub format_version: &'static str,
    /// Human-readable binding type used in setup error contexts.
    pub binding_type: &'static str,
    /// Provider tables + indexes only — the shared `meta` table is created here.
    pub schema_ddl: &'static str,
}

/// A data-directory-rooted local store owning the shared open/init plumbing.
#[derive(Debug)]
pub(crate) struct LocalStore {
    data_dir: PathBuf,
    db: Database,
    spec: &'static StoreSpec,
}

/// Open the turso database at `path` with multi-process WAL mode enabled.
///
/// Exposed to sibling test code so raw white-box connections coexist safely
/// with live provider handles on the same file.
pub(crate) async fn open_database(path: &std::path::Path, binding_type: &str) -> Result<Database> {
    turso::Builder::new_local(&path.to_string_lossy())
        // Experimental in turso, deliberately enabled: multiple OS processes
        // (and independent Database handles) may share this file. Gated by
        // the multi-handle concurrency tests in the provider modules.
        .experimental_multiprocess_wal(true)
        .build()
        .await
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: binding_type.to_string(),
            reason: format!("failed to open local store database at {}", path.display()),
        })
}

/// Run a query and drain it to completion, returning every row's values.
///
/// Draining is mandatory (see the module docs on statement hygiene); this is
/// the only way provider code reads rows.
pub(crate) async fn query_all(
    conn: &Connection,
    sql: &str,
    params: impl IntoParams,
) -> turso::Result<Vec<Vec<Value>>> {
    let mut rows = conn.query(sql, params).await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        let mut values = Vec::with_capacity(row.column_count());
        for idx in 0..row.column_count() {
            values.push(row.get_value(idx)?);
        }
        out.push(values);
    }
    Ok(out)
}

/// Extract a TEXT column value.
pub(crate) fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::Text(s) => Some(s.clone()),
        _ => None,
    }
}

/// Extract an INTEGER column value.
pub(crate) fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(i) => Some(*i),
        _ => None,
    }
}

/// Extract a nullable INTEGER column value (`NULL` → `Some(None)`).
pub(crate) fn as_opt_i64(value: &Value) -> Option<Option<i64>> {
    match value {
        Value::Null => Some(None),
        Value::Integer(i) => Some(Some(*i)),
        _ => None,
    }
}

/// Extract a BLOB column value.
pub(crate) fn as_blob(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Blob(b) => Some(b.clone()),
        _ => None,
    }
}

/// Convert an optional millisecond timestamp into a bindable SQL value.
pub(crate) fn opt_i64_value(v: Option<i64>) -> Value {
    match v {
        Some(i) => Value::Integer(i),
        None => Value::Null,
    }
}

impl LocalStore {
    /// Open (creating if missing) the store at `<data_dir>/<spec.db_filename>`.
    ///
    /// The format marker is written and checked **before** the provider
    /// `schema_ddl` runs: a store whose format this build does not understand is
    /// rejected without gaining any provider tables — we never write into a file
    /// we then refuse to touch.
    pub(crate) async fn open(data_dir: PathBuf, spec: &'static StoreSpec) -> Result<Self> {
        tokio::fs::create_dir_all(&data_dir)
            .await
            .into_alien_error()
            .context(ErrorData::LocalFilesystemError {
                path: data_dir.display().to_string(),
                operation: "create_dir_all".to_string(),
            })?;

        let db_path = data_dir.join(spec.db_filename);
        let db = open_database(&db_path, spec.binding_type).await?;
        let store = Self {
            data_dir,
            db,
            spec,
        };
        let conn = store.connect().await?;

        // Create the shared meta table + marker and read it back FIRST.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .await
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: spec.binding_type.to_string(),
            reason: "failed to create meta table".to_string(),
        })?;
        // `INSERT OR IGNORE` never overwrites an existing marker, so this
        // catches stores written by a newer (or foreign) implementation.
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('format', ?1)",
            (spec.format_version,),
        )
        .await
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: spec.binding_type.to_string(),
            reason: "failed to write format marker".to_string(),
        })?;
        let rows = query_all(&conn, "SELECT value FROM meta WHERE key = 'format'", ())
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: spec.binding_type.to_string(),
                reason: "failed to read format marker from meta table".to_string(),
            })?;
        let format = rows
            .first()
            .and_then(|row| row.first())
            .and_then(as_text)
            .ok_or_else(|| {
                AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: spec.binding_type.to_string(),
                    reason: "format marker missing from meta table".to_string(),
                })
            })?;
        if format != spec.format_version {
            return Err(AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: spec.binding_type.to_string(),
                reason: format!(
                    "unsupported store format '{format}' (this implementation supports '{}')",
                    spec.format_version
                ),
            }));
        }

        // Format accepted: only now create the provider-specific tables.
        conn.execute_batch(spec.schema_ddl)
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: spec.binding_type.to_string(),
                reason: format!("failed to initialize {} schema", spec.format_version),
            })?;

        Ok(store)
    }

    /// The data directory that holds this store's database file.
    pub(crate) fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Open a fresh connection with the store's `busy_timeout` applied.
    async fn connect(&self) -> Result<Connection> {
        let conn = self
            .db
            .connect()
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: self.spec.binding_type.to_string(),
                reason: "failed to open store connection".to_string(),
            })?;
        conn.busy_timeout(BUSY_TIMEOUT)
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: self.spec.binding_type.to_string(),
                reason: "failed to set busy_timeout".to_string(),
            })?;
        Ok(conn)
    }

    /// Run an async closure with a freshly opened connection.
    ///
    /// The connection is created for this operation and dropped when the
    /// closure's future completes, so no statement state leaks between
    /// operations. turso's `Connection` is `Send + Sync`, so the owning
    /// provider stays `Send + Sync` without any lock.
    pub(crate) async fn with_conn<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Connection) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let conn = self.connect().await?;
        f(conn).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_SPEC: StoreSpec = StoreSpec {
        db_filename: "teststore.sqlite",
        format_version: "teststore.v1",
        binding_type: "test store",
        schema_ddl: "CREATE TABLE IF NOT EXISTS widgets (id INTEGER PRIMARY KEY);",
    };

    /// A file whose format marker names a version this build does not understand
    /// must be rejected on open, and the rejection must happen BEFORE the
    /// provider `schema_ddl` runs — the file gains no provider tables.
    #[tokio::test]
    async fn foreign_format_rejected_before_provider_ddl() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path().join("store");
        std::fs::create_dir_all(&dir).expect("create dir");
        let db_path = dir.join(TEST_SPEC.db_filename);

        // Seed a file with only meta + a FOREIGN format marker (no provider table).
        {
            let db = open_database(&db_path, "test store").await.expect("seed open");
            let conn = db.connect().expect("seed connect");
            conn.execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
            )
            .await
            .expect("seed meta table");
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('format', 'teststore.v2')",
                (),
            )
            .await
            .expect("seed meta marker");
        }

        // Opening must be rejected, naming both the found and expected formats.
        let err = LocalStore::open(dir.clone(), &TEST_SPEC)
            .await
            .expect_err("foreign format must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("teststore.v2"),
            "must name found format: {msg}"
        );
        assert!(
            msg.contains("teststore.v1"),
            "must name expected format: {msg}"
        );

        // The provider table must NOT have been created by the rejected open.
        let db = open_database(&db_path, "test store").await.expect("verify open");
        let conn = db.connect().expect("verify connect");
        let rows = query_all(
            &conn,
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='widgets')",
            (),
        )
        .await
        .expect("table existence query");
        let widgets_exists = rows
            .first()
            .and_then(|row| row.first())
            .and_then(as_i64)
            .expect("existence value");
        assert_eq!(
            widgets_exists, 0,
            "provider DDL must not run on a rejected foreign-format store"
        );
    }
}
