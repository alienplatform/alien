//! Shared SQLite plumbing for the local KV and queue providers.
//!
//! Both local stores are `<dataDir>/<file>.sqlite` databases that share the same
//! open/init handshake: WAL + `busy_timeout` pragmas, a `meta(key,value)` table
//! carrying a `format` marker, a fail-fast check that the marker matches the
//! format this build understands, and a `spawn_blocking` boundary so the
//! synchronous rusqlite calls never run on an async worker. Only the
//! provider-specific tables and every operational SQL statement stay in the
//! provider modules; everything above lives here so there is exactly one home
//! for the on-disk-format handshake.
use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context as _, IntoAlienError as _};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Writers wait this long for the write lock before returning `SQLITE_BUSY`.
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

/// A data-directory-rooted SQLite store owning the shared open/init plumbing.
#[derive(Debug)]
pub(crate) struct SqliteStore {
    data_dir: PathBuf,
    db_path: PathBuf,
    spec: &'static StoreSpec,
}

/// Open a fresh connection with the store's WAL + `busy_timeout` pragmas.
fn open_conn(path: &Path, spec: &StoreSpec) -> Result<Connection> {
    let conn =
        Connection::open(path)
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: spec.binding_type.to_string(),
                reason: format!("failed to open sqlite database at {}", path.display()),
            })?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: spec.binding_type.to_string(),
            reason: "failed to set busy_timeout".to_string(),
        })?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: spec.binding_type.to_string(),
            reason: "failed to configure WAL pragmas".to_string(),
        })?;
    Ok(conn)
}

impl SqliteStore {
    /// Open (creating if missing) the store at `<data_dir>/<spec.db_filename>`.
    ///
    /// The format marker is written and checked **before** the provider
    /// `schema_ddl` runs: a store whose format this build does not understand is
    /// rejected without gaining any provider tables — we never write into a file
    /// we then refuse to touch.
    pub(crate) async fn open(data_dir: PathBuf, spec: &'static StoreSpec) -> Result<Self> {
        let db_path = data_dir.join(spec.db_filename);
        let dir = data_dir.clone();
        let init_path = db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            std::fs::create_dir_all(&dir).into_alien_error().context(
                ErrorData::LocalFilesystemError {
                    path: dir.display().to_string(),
                    operation: "create_dir_all".to_string(),
                },
            )?;
            let conn = open_conn(&init_path, spec)?;

            // Create the shared meta table + marker and read it back FIRST.
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
            )
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: spec.binding_type.to_string(),
                reason: "failed to create meta table".to_string(),
            })?;
            // `INSERT OR IGNORE` never overwrites an existing marker, so this
            // catches stores written by a newer (or foreign) implementation.
            conn.execute(
                "INSERT OR IGNORE INTO meta (key, value) VALUES ('format', ?1)",
                rusqlite::params![spec.format_version],
            )
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: spec.binding_type.to_string(),
                reason: "failed to write format marker".to_string(),
            })?;
            let format: String = conn
                .query_row("SELECT value FROM meta WHERE key = 'format'", [], |row| {
                    row.get(0)
                })
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: spec.binding_type.to_string(),
                    reason: "failed to read format marker from meta table".to_string(),
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
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: spec.binding_type.to_string(),
                    reason: format!("failed to initialize {} schema", spec.format_version),
                })?;
            Ok(())
        })
        .await
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: spec.binding_type.to_string(),
            reason: "schema init task failed".to_string(),
        })??;

        Ok(Self {
            data_dir,
            db_path,
            spec,
        })
    }

    /// The data directory that holds this store's SQLite file.
    pub(crate) fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Run a blocking closure with a freshly opened, WAL-configured connection.
    ///
    /// The connection lives entirely inside the `spawn_blocking` task — created,
    /// used, and dropped there — so it never crosses an `.await` and the owning
    /// provider stays `Send + Sync` without any lock. The closure gets a
    /// `&mut Connection` so it can open rusqlite transactions.
    pub(crate) async fn with_conn<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let path = self.db_path.clone();
        let spec = self.spec;
        tokio::task::spawn_blocking(move || -> Result<T> {
            let mut conn = open_conn(&path, spec)?;
            f(&mut conn)
        })
        .await
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: spec.binding_type.to_string(),
            reason: "sqlite blocking task panicked or was cancelled".to_string(),
        })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

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
            let conn = Connection::open(&db_path).expect("seed open");
            conn.execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);\
                 INSERT INTO meta (key, value) VALUES ('format', 'teststore.v2');",
            )
            .expect("seed meta");
        }

        // Opening must be rejected, naming both the found and expected formats.
        let err = SqliteStore::open(dir.clone(), &TEST_SPEC)
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
        let conn = Connection::open(&db_path).expect("verify open");
        let widgets_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='widgets')",
                [],
                |r| r.get(0),
            )
            .expect("table existence query");
        assert!(
            !widgets_exists,
            "provider DDL must not run on a rejected foreign-format store"
        );
    }
}
