//! SQLite backend for deployment values. The database file is encrypted with
//! the manager's database key, so values are not readable from a copy of it.

use std::sync::Arc;

use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};

use super::database::{RowParser, SqliteDatabase};
use crate::error::{ErrorData, Result};
use crate::values::{StoredValue, ValueBackend, ValueKind, ValueScope, ValueSlot};

pub struct SqliteValueBackend {
    db: Arc<SqliteDatabase>,
}

impl SqliteValueBackend {
    pub fn new(db: Arc<SqliteDatabase>) -> Self {
        Self { db }
    }

    fn query_error(action: &str) -> ErrorData {
        ErrorData::InternalError {
            message: format!("Could not {action} in the database"),
        }
    }

    async fn rows(
        &self,
        sql: &str,
        params: impl turso::IntoParams,
        action: &str,
    ) -> Result<Vec<turso::Row>> {
        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(sql, params)
            .await
            .into_alien_error()
            .context(Self::query_error(action))?;
        let mut out = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .into_alien_error()
            .context(Self::query_error(action))?
        {
            out.push(row);
        }
        Ok(out)
    }

    fn parse(row: &turso::Row) -> Result<StoredValue> {
        let p = RowParser::new(row);
        let read = |e: alien_error::AlienError| ErrorData::internal(e.message);
        let slot: ValueSlot = p.string(0, "slot").map_err(read)?.parse()?;
        let kind = match p.string(1, "kind").map_err(read)?.as_str() {
            "plain" => ValueKind::Plain,
            "secret" => ValueKind::Secret,
            other => {
                return Err(ErrorData::internal(format!(
                    "Stored value '{slot}' has unknown kind '{other}'"
                )))
            }
        };
        let updated_at = Utc
            .timestamp_millis_opt(p.i64(3, "updated_at").map_err(read)?)
            .single()
            .ok_or_else(|| ErrorData::internal(format!("Stored value '{slot}' has a bad time")))?;
        Ok(StoredValue {
            slot,
            kind,
            value: p.string(2, "value").map_err(read)?,
            updated_at,
        })
    }
}

#[async_trait]
impl ValueBackend for SqliteValueBackend {
    async fn put(&self, scope: &ValueScope, value: &StoredValue) -> Result<()> {
        let kind = match value.kind {
            ValueKind::Plain => "plain",
            ValueKind::Secret => "secret",
        };
        let conn = self.db.conn().lock().await;
        conn.execute(
            "INSERT INTO deployment_values (scope, slot, kind, value, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (scope, slot) DO UPDATE SET
                kind = excluded.kind,
                value = excluded.value,
                updated_at = excluded.updated_at",
            (
                scope.as_str(),
                value.slot.as_str(),
                kind,
                value.value.as_str(),
                value.updated_at.timestamp_millis(),
            ),
        )
        .await
        .into_alien_error()
        .context(Self::query_error("write a value"))?;
        Ok(())
    }

    async fn get(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<Option<StoredValue>> {
        self.rows(
            "SELECT slot, kind, value, updated_at FROM deployment_values
             WHERE scope = ?1 AND slot = ?2",
            (scope.as_str(), slot.as_str()),
            "read a value",
        )
        .await?
        .first()
        .map(Self::parse)
        .transpose()
    }

    async fn delete(&self, scope: &ValueScope, slot: &ValueSlot) -> Result<()> {
        let conn = self.db.conn().lock().await;
        conn.execute(
            "DELETE FROM deployment_values WHERE scope = ?1 AND slot = ?2",
            (scope.as_str(), slot.as_str()),
        )
        .await
        .into_alien_error()
        .context(Self::query_error("delete a value"))?;
        Ok(())
    }

    async fn list(&self, scope: &ValueScope) -> Result<Vec<StoredValue>> {
        self.rows(
            "SELECT slot, kind, value, updated_at FROM deployment_values
             WHERE scope = ?1 ORDER BY slot",
            (scope.as_str(),),
            "list values",
        )
        .await?
        .iter()
        .map(Self::parse)
        .collect()
    }
}
