use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use turso::{Builder, Connection, Database, EncryptionOpts};

use crate::error::{Error, Result};

/// Event stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: i64,
    pub event_type: String,
    pub data: serde_json::Value,
    pub timestamp: String,
}

/// Encrypted database for storing events
#[derive(Clone)]
pub struct EncryptedDb {
    conn: Arc<Mutex<Connection>>,
}

impl EncryptedDb {
    /// Create or open the encrypted database
    pub async fn new(data_dir: &str, encryption_key: &str) -> Result<Self> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| Error::Database(format!("Failed to create data directory: {}", e)))?;

        let db_path = Path::new(data_dir).join("events.db");
        let db_path_str = db_path.to_string_lossy();

        let encryption_opts = EncryptionOpts {
            cipher: "aegis256".to_string(),
            hexkey: encryption_key.to_string(),
        };

        let db: Database = Builder::new_local(&db_path_str)
            .with_encryption(encryption_opts)
            .build()
            .await
            .map_err(|e| Error::Database(format!("Failed to open encrypted database: {}", e)))?;

        let conn = db
            .connect()
            .map_err(|e| Error::Database(format!("Failed to connect to database: {}", e)))?;

        Self::run_migrations(&conn).await?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    async fn run_migrations(conn: &Connection) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type TEXT NOT NULL,
                data TEXT NOT NULL,
                timestamp TEXT NOT NULL
            )",
            (),
        )
        .await
        .map_err(|e| Error::Database(format!("Failed to create events table: {}", e)))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp)",
            (),
        )
        .await
        .map_err(|e| Error::Database(format!("Failed to create events index: {}", e)))?;

        Ok(())
    }

    /// Insert an event into the database
    pub async fn insert_event(&self, event_type: &str, data: &serde_json::Value) -> Result<()> {
        let conn = self.conn.lock().await;

        let json = serde_json::to_string(data)
            .map_err(|e| Error::Database(format!("Failed to serialize event data: {}", e)))?;

        conn.execute(
            "INSERT INTO events (type, data, timestamp) VALUES (?, ?, datetime('now'))",
            (event_type.to_string(), json),
        )
        .await
        .map_err(|e| Error::Database(format!("Failed to insert event: {}", e)))?;

        Ok(())
    }

    /// Get events since a given timestamp
    pub async fn get_events_since(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> Result<Vec<Event>> {
        let conn = self.conn.lock().await;

        let cutoff = since.to_rfc3339();

        let mut rows = conn
            .query(
                "SELECT id, type, data, timestamp FROM events WHERE timestamp > ? ORDER BY timestamp DESC LIMIT ?",
                (cutoff, limit as i64),
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to query events: {}", e)))?;

        let mut events = Vec::new();

        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| Error::Database(format!("Failed to fetch event row: {}", e)))?
        {
            let id: i64 = row
                .get(0)
                .map_err(|e| Error::Database(format!("Failed to read event id: {}", e)))?;

            let event_type: String = row
                .get(1)
                .map_err(|e| Error::Database(format!("Failed to read event type: {}", e)))?;

            let data_str: String = row
                .get(2)
                .map_err(|e| Error::Database(format!("Failed to read event data: {}", e)))?;

            let timestamp: String = row
                .get(3)
                .map_err(|e| Error::Database(format!("Failed to read event timestamp: {}", e)))?;

            let data: serde_json::Value = serde_json::from_str(&data_str)
                .map_err(|e| Error::Database(format!("Failed to parse event data: {}", e)))?;

            events.push(Event {
                id,
                event_type,
                data,
                timestamp,
            });
        }

        Ok(events)
    }

    /// Clean up old events
    pub async fn cleanup_old_events(&self, retention_days: i64) -> Result<u64> {
        let conn = self.conn.lock().await;

        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days);
        let cutoff_str = cutoff.to_rfc3339();

        let rows_affected = conn
            .execute("DELETE FROM events WHERE timestamp < ?", (cutoff_str,))
            .await
            .map_err(|e| Error::Database(format!("Failed to cleanup old events: {}", e)))?;

        Ok(rows_affected)
    }
}
