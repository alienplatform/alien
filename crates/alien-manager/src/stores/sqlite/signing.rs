//! SQLite implementation of [`SigningStore`]. Statements are stored as the
//! JSON the client sent, so they come back byte for byte.

use alien_core::SignedStatement;
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use async_trait::async_trait;
use std::sync::Arc;

use super::database::{RowParser, SqliteDatabase};
use crate::traits::signing_store::*;

pub struct SqliteSigningStore {
    db: Arc<SqliteDatabase>,
}

impl SqliteSigningStore {
    pub fn new(db: Arc<SqliteDatabase>) -> Self {
        Self { db }
    }

    async fn run(&self, sql: &str, params: impl turso::IntoParams) -> Result<u64, AlienError> {
        let conn = self.db.conn().lock().await;
        conn.execute(sql, params)
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Signing store update failed".to_string(),
            })
    }

    async fn rows(
        &self,
        sql: &str,
        params: impl turso::IntoParams,
    ) -> Result<Vec<turso::Row>, AlienError> {
        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(sql, params)
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Signing store query failed".to_string(),
            })?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to read a signing store row".to_string(),
        })? {
            out.push(row);
        }
        Ok(out)
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String, AlienError> {
    serde_json::to_string(value)
        .into_alien_error()
        .context(GenericError {
            message: "Failed to encode a signing record".to_string(),
        })
}

fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, AlienError> {
    serde_json::from_str(text)
        .into_alien_error()
        .context(GenericError {
            message: "A stored signing record is not valid JSON".to_string(),
        })
}

fn parse_pending(row: &turso::Row) -> Result<PendingChangeRecord, AlienError> {
    let p = RowParser::new(row);
    from_json(&p.string(0, "record")?)
}

#[async_trait]
impl SigningStore for SqliteSigningStore {
    async fn signer_lists(&self, project_id: &str) -> Result<Vec<SignedStatement>, AlienError> {
        self.rows(
            "SELECT statement FROM signer_lists WHERE project_id = ?1 ORDER BY version",
            (project_id,),
        )
        .await?
        .iter()
        .map(|row| from_json(&RowParser::new(row).string(0, "statement")?))
        .collect()
    }

    async fn add_signer_list(
        &self,
        project_id: &str,
        version: u64,
        statement: &SignedStatement,
    ) -> Result<SignerListWrite, AlienError> {
        let text = to_json(statement)?;
        let version = version as i64;
        let inserted = self
            .run(
                "INSERT OR IGNORE INTO signer_lists (project_id, version, statement, created_at) VALUES (?1, ?2, ?3, ?4)",
                (project_id, version, text.clone(), chrono::Utc::now().to_rfc3339()),
            )
            .await?;
        if inserted == 1 {
            return Ok(SignerListWrite::Added);
        }
        let stored = self
            .rows(
                "SELECT statement FROM signer_lists WHERE project_id = ?1 AND version = ?2",
                (project_id, version),
            )
            .await?;
        let stored = stored
            .first()
            .map(|row| RowParser::new(row).string(0, "statement"))
            .transpose()?;
        Ok(match stored {
            Some(stored) if stored == text => SignerListWrite::AlreadyStored,
            _ => SignerListWrite::Conflict,
        })
    }

    async fn create_pending_change(&self, change: &PendingChangeRecord) -> Result<(), AlienError> {
        self.run(
            "INSERT INTO pending_changes (id, project_id, record, created_at) VALUES (?1, ?2, ?3, ?4)",
            (
                change.id.as_str(),
                change.project_id.as_str(),
                to_json(change)?,
                change.created_at.to_rfc3339(),
            ),
        )
        .await?;
        Ok(())
    }

    async fn pending_changes(
        &self,
        project_id: &str,
    ) -> Result<Vec<PendingChangeRecord>, AlienError> {
        self.rows(
            "SELECT record FROM pending_changes WHERE project_id = ?1 ORDER BY created_at DESC",
            (project_id,),
        )
        .await?
        .iter()
        .map(parse_pending)
        .collect()
    }

    async fn get_pending_change(
        &self,
        id: &str,
    ) -> Result<Option<PendingChangeRecord>, AlienError> {
        self.rows("SELECT record FROM pending_changes WHERE id = ?1", (id,))
            .await?
            .first()
            .map(parse_pending)
            .transpose()
    }

    async fn update_pending_change(&self, change: &PendingChangeRecord) -> Result<(), AlienError> {
        let updated = self
            .run(
                "UPDATE pending_changes SET record = ?2 WHERE id = ?1",
                (change.id.as_str(), to_json(change)?),
            )
            .await?;
        if updated != 1 {
            return Err(AlienError::new(GenericError {
                message: format!("Pending change '{}' does not exist", change.id),
            }));
        }
        Ok(())
    }

    async fn add_config_statement(
        &self,
        deployment_id: &str,
        sequence: u64,
        statement: &SignedStatement,
    ) -> Result<(), AlienError> {
        self.run(
            "INSERT INTO config_statements (deployment_id, sequence, statement, created_at) VALUES (?1, ?2, ?3, ?4)",
            (
                deployment_id,
                sequence as i64,
                to_json(statement)?,
                chrono::Utc::now().to_rfc3339(),
            ),
        )
        .await?;
        Ok(())
    }

    async fn latest_config_statement(
        &self,
        deployment_id: &str,
    ) -> Result<Option<SignedStatement>, AlienError> {
        self.rows(
            "SELECT statement FROM config_statements WHERE deployment_id = ?1 ORDER BY sequence DESC LIMIT 1",
            (deployment_id,),
        )
        .await?
        .first()
        .map(|row| from_json(&RowParser::new(row).string(0, "statement")?))
        .transpose()
    }
}
