//! SQLite database wrapper for alien-manager state storage.

use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use fs2::FileExt;
use rand::RngCore;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use turso::{Connection, Database, EncryptionOpts, Value};

use super::migrations;

/// SQLite database wrapper providing a thread-safe connection.
pub struct SqliteDatabase {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteDatabase {
    /// Open (or create) an encrypted SQLite database and run migrations.
    ///
    /// The generated key is stored next to the database. Call
    /// [`Self::new_with_key`] when configuration supplies a stable key.
    pub async fn new(path: &str) -> Result<Self, AlienError> {
        Self::new_with_key(path, None).await
    }

    /// Open an encrypted SQLite database with a configured or generated key.
    pub async fn new_with_key(
        path: &str,
        configured_key: Option<&str>,
    ) -> Result<Self, AlienError> {
        if let Some(parent) = Path::new(path)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .into_alien_error()
                .context(GenericError {
                    message: format!("Failed to create database directory: {}", parent.display()),
                })?;
        }

        let path = Path::new(path);
        // Key creation, plaintext migration, and encrypted database opening are
        // one initialization transaction. Lock before inspecting any of them
        // so a second manager cannot observe a partially-written key file.
        let migration_lock = acquire_initialization_lock(path).await?;

        let key = match configured_key {
            Some(key) => validate_key(key)?.to_string(),
            None => {
                let key_path = key_path(path);
                if !key_path.exists()
                    && path.exists()
                    && std::fs::metadata(path)
                        .into_alien_error()
                        .context(GenericError {
                            message: format!("Failed to inspect database at '{}'", path.display()),
                        })?
                        .len()
                        > 0
                    && !is_plaintext_database(path)?
                {
                    return Err(db_error(&format!(
                        "Manager database '{}' is encrypted but its key file '{}' is missing; restore the original key",
                        path.display(),
                        key_path.display()
                    )));
                }
                load_or_create_key(&key_path)?
            }
        };
        recover_or_migrate_plaintext(path, &key).await?;

        // Single-process open. Unlike the local binding stores in
        // alien-bindings, the manager is the only process touching this file,
        // so turso's experimental `experimental_multiprocess_wal` mode is
        // deliberately NOT enabled here — no cross-process coordination is
        // needed and we avoid depending on an experimental on-disk format.
        let db: Database = encrypted_builder(path, &key)
            .build()
            .await
            .into_alien_error()
            .context(GenericError {
                message: format!("Failed to open encrypted database at '{}'", path.display()),
            })?;

        let conn = db.connect().into_alien_error().context(GenericError {
            message: "Failed to connect to database".to_string(),
        })?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        // Enable WAL mode for concurrent read/write performance.
        // WAL persists per-database file, stays across restarts.
        // PRAGMAs like journal_mode return result rows, so use query() to absorb them.
        {
            let conn = db.conn.lock().await;
            for pragma in &[
                "PRAGMA journal_mode=WAL",
                "PRAGMA synchronous=NORMAL",
                "PRAGMA busy_timeout=5000",
            ] {
                let mut rows =
                    conn.query(pragma, ())
                        .await
                        .into_alien_error()
                        .context(GenericError {
                            message: format!("Failed to set PRAGMA: {}", pragma),
                        })?;
                // Consume the result row (journal_mode returns the mode name)
                let _ = rows.next().await;
            }
        }

        migrations::run_migrations(&db).await?;

        Ok(db)
    }

    /// Get a reference to the connection mutex.
    pub fn conn(&self) -> &Arc<Mutex<Connection>> {
        &self.conn
    }

    /// Execute a SQL statement (no result rows).
    pub(crate) async fn execute(&self, sql: &str) -> Result<(), AlienError> {
        let conn = self.conn.lock().await;
        conn.execute(sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Database execute failed".to_string(),
            })?;
        Ok(())
    }

    /// Execute a SQL statement and return the number of rows affected.
    pub(crate) async fn execute_returning_rows_affected(
        &self,
        sql: &str,
    ) -> Result<u64, AlienError> {
        let conn = self.conn.lock().await;
        conn.execute(sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Database execute failed".to_string(),
            })
    }
}

const SQLITE_HEADER: &[u8] = b"SQLite format 3\0";
const KEY_SUFFIX: &str = ".encryption-key";
const STAGING_SUFFIX: &str = ".encryption-migration-new";
const BACKUP_SUFFIX: &str = ".encryption-migration-plaintext";
const LOCK_SUFFIX: &str = ".encryption-migration.lock";

fn encrypted_builder(path: &Path, key: &str) -> turso::Builder {
    turso::Builder::new_local(&path.to_string_lossy())
        .experimental_encryption(true)
        .with_encryption(EncryptionOpts {
            cipher: "aegis256".to_string(),
            hexkey: key.to_string(),
        })
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn key_path(path: &Path) -> PathBuf {
    sibling_path(path, KEY_SUFFIX)
}

fn open_migration_lock(path: &Path) -> Result<File, AlienError> {
    let lock_path = sibling_path(path, LOCK_SUFFIX);
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&lock_path)
        .into_alien_error()
        .context(GenericError {
            message: format!(
                "Failed to open manager database migration lock '{}'",
                lock_path.display()
            ),
        })
}

async fn acquire_initialization_lock(path: &Path) -> Result<File, AlienError> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        let migration_lock = open_migration_lock(&path)?;
        migration_lock
            .lock_exclusive()
            .into_alien_error()
            .context(GenericError {
                message: format!(
                    "Failed to lock manager database migration for '{}'",
                    path.display()
                ),
            })?;
        Ok(migration_lock)
    })
    .await
    .into_alien_error()
    .context(GenericError {
        message: "Manager database initialization lock task failed".to_string(),
    })?
}

fn validate_key(key: &str) -> Result<&str, AlienError> {
    let key = key.trim();
    if key.len() != 64 || !key.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(db_error(
            "Manager database encryption key must be exactly 64 hexadecimal characters",
        ));
    }
    Ok(key)
}

fn load_or_create_key(path: &Path) -> Result<String, AlienError> {
    match read_key(path) {
        Ok(key) => return Ok(key),
        Err(error) if error.kind() != ErrorKind::NotFound => {
            return Err(db_error(&format!(
                "Failed to read manager database encryption key at '{}': {error}",
                path.display()
            )))
        }
        Err(_) => {}
    }

    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let generated = hex::encode(bytes);

    #[cfg(unix)]
    let opened = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    };
    #[cfg(not(unix))]
    let opened = OpenOptions::new().write(true).create_new(true).open(path);

    match opened {
        Ok(mut file) => {
            if let Err(error) = file
                .write_all(generated.as_bytes())
                .and_then(|_| file.sync_all())
            {
                let _ = std::fs::remove_file(path);
                return Err(db_error(&format!(
                    "Failed to persist manager database encryption key at '{}': {error}",
                    path.display()
                )));
            }
            sync_parent(path)?;
            Ok(generated)
        }
        Err(error) => Err(db_error(&format!(
            "Failed to create manager database encryption key at '{}': {error}",
            path.display()
        ))),
    }
}

fn read_key(path: &Path) -> std::io::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "manager database encryption key must be a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                format!(
                    "manager database encryption key '{}' must have mode 0600",
                    path.display()
                ),
            ));
        }
    }
    let mut key = String::new();
    File::open(path)?.read_to_string(&mut key)?;
    validate_key(&key)
        .map(str::to_string)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error.message))
}

fn is_plaintext_database(path: &Path) -> Result<bool, AlienError> {
    if !path.exists() {
        return Ok(false);
    }
    let mut header = [0_u8; 16];
    let mut file = File::open(path).into_alien_error().context(GenericError {
        message: format!("Failed to inspect database at '{}'", path.display()),
    })?;
    let bytes = file
        .read(&mut header)
        .into_alien_error()
        .context(GenericError {
            message: format!("Failed to inspect database at '{}'", path.display()),
        })?;
    Ok(bytes == SQLITE_HEADER.len() && header == SQLITE_HEADER)
}

async fn recover_or_migrate_plaintext(path: &Path, key: &str) -> Result<(), AlienError> {
    let staging = sibling_path(path, STAGING_SUFFIX);
    let backup = sibling_path(path, BACKUP_SUFFIX);

    if path.exists()
        && std::fs::metadata(path)
            .into_alien_error()
            .context(GenericError {
                message: format!("Failed to inspect database at '{}'", path.display()),
            })?
            .len()
            == 0
    {
        std::fs::remove_file(path)
            .into_alien_error()
            .context(GenericError {
                message: format!("Failed to remove empty database at '{}'", path.display()),
            })?;
    }

    if path.exists() && !is_plaintext_database(path)? {
        validate_encrypted_database(path, key).await?;
        remove_database_files(&staging)?;
        if backup.exists() {
            remove_database_files(&backup)?;
            sync_parent(path)?;
        }
        return Ok(());
    }

    if !path.exists() && backup.exists() {
        if staging.exists() && validate_encrypted_database(&staging, key).await.is_ok() {
            remove_database_sidecars(path)?;
            std::fs::rename(&staging, path)
                .into_alien_error()
                .context(GenericError {
                    message: "Failed to resume encrypted database promotion".to_string(),
                })?;
            sync_parent(path)?;
            remove_database_sidecars(&staging)?;
            validate_encrypted_database(path, key).await?;
            remove_database_files(&backup)?;
            sync_parent(path)?;
            return Ok(());
        }
        std::fs::rename(&backup, path)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to restore plaintext database after interrupted migration"
                    .to_string(),
            })?;
        sync_parent(path)?;
    }

    if !path.exists() {
        remove_database_files(&staging)?;
        return Ok(());
    }
    if !is_plaintext_database(path)? {
        return validate_encrypted_database(path, key).await;
    }
    if backup.exists() {
        return Err(db_error(&format!(
            "Cannot choose between plaintext manager databases '{}' and '{}'; remove neither and restore the intended database explicitly",
            path.display(),
            backup.display()
        )));
    }

    remove_database_files(&staging)?;
    migrate_plaintext_database(path, &staging, key).await?;
    sync_file(&staging)?;
    sync_parent(path)?;
    remove_database_sidecars(path)?;

    std::fs::rename(path, &backup)
        .into_alien_error()
        .context(GenericError {
            message: "Failed to preserve plaintext database during encryption migration"
                .to_string(),
        })?;
    sync_parent(path)?;
    std::fs::rename(&staging, path)
        .into_alien_error()
        .context(GenericError {
            message: "Failed to promote encrypted manager database".to_string(),
        })?;
    sync_parent(path)?;
    remove_database_sidecars(&staging)?;
    validate_encrypted_database(path, key).await?;
    remove_database_files(&backup)?;
    sync_parent(path)?;
    Ok(())
}

async fn migrate_plaintext_database(
    source_path: &Path,
    target_path: &Path,
    key: &str,
) -> Result<(), AlienError> {
    let source_db = turso::Builder::new_local(&source_path.to_string_lossy())
        .build()
        .await
        .into_alien_error()
        .context(GenericError {
            message: "Failed to open plaintext manager database for migration".to_string(),
        })?;
    let source = source_db
        .connect()
        .into_alien_error()
        .context(GenericError {
            message: "Failed to connect to plaintext manager database".to_string(),
        })?;
    consume_query(&source, "PRAGMA wal_checkpoint(TRUNCATE)").await?;
    consume_query(&source, "PRAGMA journal_mode=DELETE").await?;
    assert_integrity(&source).await?;

    let target_db = encrypted_builder(target_path, key)
        .build()
        .await
        .into_alien_error()
        .context(GenericError {
            message: "Failed to create encrypted manager database".to_string(),
        })?;
    let target = target_db
        .connect()
        .into_alien_error()
        .context(GenericError {
            message: "Failed to connect to encrypted manager database".to_string(),
        })?;

    let schema = read_schema(&source).await?;
    target
        .execute("BEGIN IMMEDIATE", ())
        .await
        .into_alien_error()
        .context(GenericError {
            message: "Failed to begin encrypted database migration".to_string(),
        })?;
    let result = copy_schema_and_rows(&source, &target, &schema).await;
    match result {
        Ok(()) => {
            target
                .execute("COMMIT", ())
                .await
                .into_alien_error()
                .context(GenericError {
                    message: "Failed to commit encrypted database migration".to_string(),
                })?;
        }
        Err(error) => {
            let _ = target.execute("ROLLBACK", ()).await;
            return Err(error);
        }
    }
    assert_integrity(&target).await?;
    compare_table_counts(&source, &target, &schema).await?;
    compare_schema(&source, &target).await?;
    consume_query(&target, "PRAGMA wal_checkpoint(TRUNCATE)").await?;
    drop(target);
    drop(target_db);
    drop(source);
    drop(source_db);
    validate_encrypted_database(target_path, key).await
}

#[derive(Debug, PartialEq, Eq)]
struct SchemaObject {
    object_type: String,
    name: String,
    sql: String,
}

async fn compare_schema(source: &Connection, target: &Connection) -> Result<(), AlienError> {
    let source_schema = read_schema(source).await?;
    let target_schema = read_schema(target).await?;
    if source_schema != target_schema {
        return Err(db_error(
            "Encrypted database migration changed the database schema",
        ));
    }
    Ok(())
}

async fn read_schema(connection: &Connection) -> Result<Vec<SchemaObject>, AlienError> {
    let mut rows = connection
        .query(
            "SELECT type, name, sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY CASE type WHEN 'table' THEN 0 ELSE 1 END, name",
            (),
        )
        .await
        .into_alien_error()
        .context(GenericError {
            message: "Failed to read manager database schema".to_string(),
        })?;
    let mut schema = Vec::new();
    while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
        message: "Failed to read manager database schema row".to_string(),
    })? {
        schema.push(SchemaObject {
            object_type: row.get(0).into_alien_error().context(GenericError {
                message: "Failed to read schema object type".to_string(),
            })?,
            name: row.get(1).into_alien_error().context(GenericError {
                message: "Failed to read schema object name".to_string(),
            })?,
            sql: row.get(2).into_alien_error().context(GenericError {
                message: "Failed to read schema object SQL".to_string(),
            })?,
        });
    }
    Ok(schema)
}

async fn copy_schema_and_rows(
    source: &Connection,
    target: &Connection,
    schema: &[SchemaObject],
) -> Result<(), AlienError> {
    for object in schema.iter().filter(|object| object.object_type == "table") {
        target
            .execute(&object.sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: format!("Failed to create migrated table '{}'", object.name),
            })?;
        let table = quoted_identifier(&object.name);
        let mut rows = source
            .query(format!("SELECT * FROM {table}"), ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: format!("Failed to read table '{}' during migration", object.name),
            })?;
        let placeholders = std::iter::repeat_n("?", rows.column_count())
            .collect::<Vec<_>>()
            .join(", ");
        while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: format!("Failed to read row from table '{}'", object.name),
        })? {
            let values: Vec<Value> = (0..row.column_count())
                .map(|index| row.get_value(index))
                .collect::<Result<_, _>>()
                .into_alien_error()
                .context(GenericError {
                    message: format!("Failed to copy row from table '{}'", object.name),
                })?;
            target
                .execute(
                    format!("INSERT INTO {table} VALUES ({placeholders})"),
                    values,
                )
                .await
                .into_alien_error()
                .context(GenericError {
                    message: format!("Failed to write row to table '{}'", object.name),
                })?;
        }
    }
    for object in schema.iter().filter(|object| object.object_type != "table") {
        target
            .execute(&object.sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: format!(
                    "Failed to create migrated {} '{}'",
                    object.object_type, object.name
                ),
            })?;
    }
    Ok(())
}

async fn compare_table_counts(
    source: &Connection,
    target: &Connection,
    schema: &[SchemaObject],
) -> Result<(), AlienError> {
    for table in schema.iter().filter(|object| object.object_type == "table") {
        let sql = format!("SELECT COUNT(*) FROM {}", quoted_identifier(&table.name));
        let source_count = query_i64(source, &sql).await?;
        let target_count = query_i64(target, &sql).await?;
        if source_count != target_count {
            return Err(db_error(&format!(
                "Encrypted database migration changed row count for table '{}': {source_count} -> {target_count}",
                table.name
            )));
        }
    }
    Ok(())
}

async fn validate_encrypted_database(path: &Path, key: &str) -> Result<(), AlienError> {
    let database = encrypted_builder(path, key)
        .build()
        .await
        .into_alien_error()
        .context(GenericError {
            message: format!(
                "Failed to open encrypted manager database '{}' (the configured key may be incorrect)",
                path.display()
            ),
        })?;
    let connection = database
        .connect()
        .into_alien_error()
        .context(GenericError {
            message: "Failed to connect to encrypted manager database".to_string(),
        })?;
    assert_integrity(&connection).await
}

async fn assert_integrity(connection: &Connection) -> Result<(), AlienError> {
    let mut rows = connection
        .query("PRAGMA integrity_check", ())
        .await
        .into_alien_error()
        .context(GenericError {
            message: "Failed to run manager database integrity check".to_string(),
        })?;
    let row = rows
        .next()
        .await
        .into_alien_error()
        .context(GenericError {
            message: "Failed to read manager database integrity result".to_string(),
        })?
        .ok_or_else(|| db_error("Manager database integrity check returned no result"))?;
    let result: String = row.get(0).into_alien_error().context(GenericError {
        message: "Failed to parse manager database integrity result".to_string(),
    })?;
    if result != "ok" {
        return Err(db_error(&format!(
            "Manager database integrity check failed: {result}"
        )));
    }
    Ok(())
}

async fn consume_query(connection: &Connection, sql: &str) -> Result<(), AlienError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .into_alien_error()
        .context(GenericError {
            message: format!("Failed to run '{sql}'"),
        })?;
    while rows
        .next()
        .await
        .into_alien_error()
        .context(GenericError {
            message: format!("Failed to finish '{sql}'"),
        })?
        .is_some()
    {}
    Ok(())
}

async fn query_i64(connection: &Connection, sql: &str) -> Result<i64, AlienError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .into_alien_error()
        .context(GenericError {
            message: format!("Failed to run '{sql}'"),
        })?;
    let row = rows
        .next()
        .await
        .into_alien_error()
        .context(GenericError {
            message: format!("Failed to read result for '{sql}'"),
        })?
        .ok_or_else(|| db_error(&format!("Query returned no result: {sql}")))?;
    row.get(0).into_alien_error().context(GenericError {
        message: format!("Failed to parse result for '{sql}'"),
    })
}

fn quoted_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn remove_database_files(path: &Path) -> Result<(), AlienError> {
    remove_file_if_exists(path)?;
    remove_database_sidecars(path)
}

fn remove_database_sidecars(path: &Path) -> Result<(), AlienError> {
    for candidate in [
        sibling_path(path, "-wal"),
        sibling_path(path, "-shm"),
        sibling_path(path, "-log"),
    ] {
        remove_file_if_exists(&candidate)?;
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<(), AlienError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(db_error(&format!(
            "Failed to remove database migration artifact '{}': {error}",
            path.display()
        ))),
    }
}

fn sync_file(path: &Path) -> Result<(), AlienError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .into_alien_error()
        .context(GenericError {
            message: format!("Failed to sync database file '{}'", path.display()),
        })
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), AlienError> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .into_alien_error()
        .context(GenericError {
            message: format!("Failed to sync database directory '{}'", parent.display()),
        })
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<(), AlienError> {
    // Opening and flushing directory handles is not portable outside Unix.
    // File contents are still synced before every rename boundary.
    Ok(())
}

/// Type-safe row parser that extracts columns by index with proper error handling.
pub(crate) struct RowParser<'a> {
    pub row: &'a turso::Row,
}

impl<'a> RowParser<'a> {
    pub fn new(row: &'a turso::Row) -> Self {
        Self { row }
    }

    pub fn string(&self, idx: usize, name: &str) -> Result<String, AlienError> {
        self.row.get(idx).into_alien_error().context(GenericError {
            message: format!("Failed to read column '{}' at index {}", name, idx),
        })
    }

    pub fn optional_string(&self, idx: usize, name: &str) -> Result<Option<String>, AlienError> {
        self.row.get(idx).into_alien_error().context(GenericError {
            message: format!("Failed to read optional column '{}' at index {}", name, idx),
        })
    }

    pub fn i64(&self, idx: usize, name: &str) -> Result<i64, AlienError> {
        self.row.get(idx).into_alien_error().context(GenericError {
            message: format!("Failed to read column '{}' at index {}", name, idx),
        })
    }

    pub fn optional_i64(&self, idx: usize, name: &str) -> Result<Option<i64>, AlienError> {
        self.row.get(idx).into_alien_error().context(GenericError {
            message: format!("Failed to read optional column '{}' at index {}", name, idx),
        })
    }

    pub fn datetime(
        &self,
        idx: usize,
        name: &str,
    ) -> Result<chrono::DateTime<chrono::Utc>, AlienError> {
        let s: String = self.string(idx, name)?;
        s.parse().into_alien_error().context(GenericError {
            message: format!("Failed to parse datetime for column '{}'", name),
        })
    }

    pub fn optional_datetime(
        &self,
        idx: usize,
        name: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, AlienError> {
        let s: Option<String> = self.optional_string(idx, name)?;
        match s {
            Some(s) => s
                .parse()
                .into_alien_error()
                .context(GenericError {
                    message: format!("Failed to parse datetime for column '{}'", name),
                })
                .map(Some),
            None => Ok(None),
        }
    }

    pub fn json<T: serde::de::DeserializeOwned>(
        &self,
        idx: usize,
        name: &str,
    ) -> Result<T, AlienError> {
        let s: String = self.string(idx, name)?;
        serde_json::from_str(&s)
            .into_alien_error()
            .context(GenericError {
                message: format!("Failed to parse JSON for column '{}'", name),
            })
    }

    pub fn optional_json<T: serde::de::DeserializeOwned>(
        &self,
        idx: usize,
        name: &str,
    ) -> Result<Option<T>, AlienError> {
        let s: Option<String> = self.optional_string(idx, name)?;
        match s {
            Some(s) => serde_json::from_str(&s)
                .into_alien_error()
                .context(GenericError {
                    message: format!("Failed to parse JSON for column '{}'", name),
                })
                .map(Some),
            None => Ok(None),
        }
    }
}

/// Create a database error with the given message.
pub(crate) fn db_error(message: &str) -> AlienError {
    AlienError::new(GenericError {
        message: message.to_string(),
    })
}

#[cfg(test)]
#[path = "database_tests.rs"]
mod tests;
