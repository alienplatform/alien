//! SQLite database wrapper for alien-manager state storage.

use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use turso::{Connection, Database};

use super::migrations;

/// SQLite database wrapper providing a thread-safe connection.
pub struct SqliteDatabase {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteDatabase {
    /// Open (or create) a SQLite database at the given path and run migrations.
    pub async fn new(path: &str) -> Result<Self, AlienError> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .into_alien_error()
                .context(GenericError {
                    message: format!("Failed to create database directory: {}", parent.display()),
                })?;
        }

        let db: Database = turso::Builder::new_local(path)
            .build()
            .await
            .into_alien_error()
            .context(GenericError {
                message: format!("Failed to open database at '{}'", path),
            })?;

        let conn = db.connect().into_alien_error().context(GenericError {
            message: "Failed to connect to database".to_string(),
        })?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

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
