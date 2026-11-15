//! SQLite implementation of TokenStore.

use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use async_trait::async_trait;
use chrono::Utc;
use sea_query::{Expr, Query, SqliteQueryBuilder};
use std::sync::Arc;
use turso::transaction::TransactionBehavior;

use super::database::{db_error, RowParser, SqliteDatabase};
use super::migrations::Tokens;
use crate::traits::token_store::*;

pub struct SqliteTokenStore {
    db: Arc<SqliteDatabase>,
}

impl SqliteTokenStore {
    pub fn new(db: Arc<SqliteDatabase>) -> Self {
        Self { db }
    }

    fn parse_token(row: &turso::Row) -> Result<TokenRecord, AlienError> {
        let p = RowParser::new(row);
        let type_str: String = p.string(1, "type")?;
        let token_type = match type_str.as_str() {
            "admin" => TokenType::Admin,
            "deployment-group" => TokenType::DeploymentGroup,
            "deployment" => TokenType::Deployment,
            "commands-receiver" => TokenType::CommandsReceiver,
            "setup" => TokenType::Setup,
            "setup-import" => TokenType::SetupImport,
            other => return Err(db_error(&format!("Unknown token type: {}", other))),
        };
        Ok(TokenRecord {
            id: p.string(0, "id")?,
            token_type,
            key_prefix: p.string(2, "key_prefix")?,
            key_hash: p.string(3, "key_hash")?,
            deployment_group_id: p.optional_string(4, "deployment_group_id")?,
            deployment_id: p.optional_string(5, "deployment_id")?,
            created_at: p.datetime(6, "created_at")?,
            expires_at: match p.optional_string(7, "expires_at")? {
                Some(value) => Some(
                    chrono::DateTime::parse_from_rfc3339(&value)
                        .map_err(|_| db_error("Token has an unreadable expiry"))?
                        .with_timezone(&Utc),
                ),
                None => None,
            },
        })
    }
}

#[async_trait]
impl TokenStore for SqliteTokenStore {
    async fn create_token(&self, params: CreateTokenParams) -> Result<TokenRecord, AlienError> {
        if params.token_type == TokenType::CommandsReceiver {
            return Err(db_error(
                "Commands keys are stored with create_commands_key, which checks the key generation",
            ));
        }
        let id = alien_core::new_id(alien_core::IdType::Token);
        let now = Utc::now();

        let sql = Query::insert()
            .into_table(Tokens::Table)
            .columns([
                Tokens::Id,
                Tokens::Type,
                Tokens::KeyPrefix,
                Tokens::KeyHash,
                Tokens::DeploymentGroupId,
                Tokens::DeploymentId,
                Tokens::CreatedAt,
            ])
            .values_panic([
                id.clone().into(),
                params.token_type.to_string().into(),
                params.key_prefix.clone().into(),
                params.key_hash.clone().into(),
                params.deployment_group_id.clone().into(),
                params.deployment_id.clone().into(),
                now.to_rfc3339().into(),
            ])
            .to_string(SqliteQueryBuilder);

        self.db.execute(&sql).await?;

        Ok(TokenRecord {
            id,
            token_type: params.token_type,
            key_prefix: params.key_prefix,
            key_hash: params.key_hash,
            deployment_group_id: params.deployment_group_id,
            deployment_id: params.deployment_id,
            created_at: now,
            expires_at: None,
        })
    }

    async fn validate_token(&self, key_hash: &str) -> Result<Option<TokenRecord>, AlienError> {
        let sql = Query::select()
            .columns([
                Tokens::Id,
                Tokens::Type,
                Tokens::KeyPrefix,
                Tokens::KeyHash,
                Tokens::DeploymentGroupId,
                Tokens::DeploymentId,
                Tokens::CreatedAt,
                Tokens::ExpiresAt,
            ])
            .from(Tokens::Table)
            .and_where(Expr::col(Tokens::KeyHash).eq(key_hash))
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to query token".to_string(),
            })?;

        match rows.next().await.into_alien_error().context(GenericError {
            message: format!("Failed to fetch token row (sql: {sql})"),
        })? {
            Some(row) => {
                let token = Self::parse_token(&row)?;
                // A token past its expiry is as good as deleted.
                Ok(token
                    .expires_at
                    .is_none_or(|expires_at| expires_at > Utc::now())
                    .then_some(token))
            }
            None => Ok(None),
        }
    }

    /// Deleting a deployment token also deletes that deployment's unused
    /// enrollment codes made from a deployment token, in the same transaction.
    /// A code made from a token must not outlive the revocation of the token.
    async fn delete_token(&self, id: &str) -> Result<(), AlienError> {
        let failed = || GenericError {
            message: "Token deletion failed".to_string(),
        };
        let mut conn = self.db.conn().lock().await;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .into_alien_error()
            .context(failed())?;
        let mut rows = tx
            .query(
                "SELECT type, deployment_id FROM tokens WHERE id = ?1",
                [id.to_owned()],
            )
            .await
            .into_alien_error()
            .context(failed())?;
        let Some(row) = rows.next().await.into_alien_error().context(failed())? else {
            return Err(db_error(&format!("Token {} not found", id)));
        };
        let p = RowParser::new(&row);
        let token_type = p.string(0, "type")?;
        let deployment_id = p.optional_string(1, "deployment_id")?;
        drop(rows);

        tx.execute("DELETE FROM tokens WHERE id = ?1", [id.to_owned()])
            .await
            .into_alien_error()
            .context(failed())?;
        if let (true, Some(deployment_id)) = (token_type == "deployment", deployment_id) {
            tx.execute(
                "DELETE FROM runtime_enrollment_codes \
                 WHERE deployment_id = ?1 AND from_token = 1 AND initial_key_hash IS NULL",
                [deployment_id],
            )
            .await
            .into_alien_error()
            .context(failed())?;
        }
        tx.commit().await.into_alien_error().context(failed())?;
        Ok(())
    }

    async fn list_tokens(&self) -> Result<Vec<TokenRecord>, AlienError> {
        let sql = Query::select()
            .columns([
                Tokens::Id,
                Tokens::Type,
                Tokens::KeyPrefix,
                Tokens::KeyHash,
                Tokens::DeploymentGroupId,
                Tokens::DeploymentId,
                Tokens::CreatedAt,
                Tokens::ExpiresAt,
            ])
            .from(Tokens::Table)
            .order_by(Tokens::CreatedAt, sea_query::Order::Desc)
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to list tokens".to_string(),
            })?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch token row".to_string(),
        })? {
            results.push(Self::parse_token(&row)?);
        }
        Ok(results)
    }

    async fn commands_key_generation(&self, deployment_id: &str) -> Result<u64, AlienError> {
        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(
                "SELECT generation FROM commands_key_generations WHERE deployment_id = ?1",
                [deployment_id.to_owned()],
            )
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to query the commands key generation".to_string(),
            })?;
        let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to read the commands key generation".to_string(),
        })?
        else {
            return Ok(0);
        };
        Ok(RowParser::new(&row)
            .i64(0, "generation")?
            .try_into()
            .unwrap_or_default())
    }

    async fn create_commands_key(
        &self,
        deployment_id: &str,
        key_prefix: &str,
        key_hash: &str,
        generation: u64,
    ) -> Result<Option<TokenRecord>, AlienError> {
        let id = alien_core::new_id(alien_core::IdType::Token);
        let now = Utc::now();
        let conn = self.db.conn().lock().await;
        // One statement: the row is inserted only while the generation is still
        // the one the key was derived at.
        let inserted = conn
            .execute(
                "INSERT INTO tokens (id, type, key_prefix, key_hash, deployment_group_id, deployment_id, created_at) \
                 SELECT ?1, 'commands-receiver', ?2, ?3, NULL, ?4, ?5 \
                 WHERE COALESCE((SELECT generation FROM commands_key_generations WHERE deployment_id = ?4), 0) = ?6",
                (
                    id.clone(),
                    key_prefix.to_owned(),
                    key_hash.to_owned(),
                    deployment_id.to_owned(),
                    now.to_rfc3339(),
                    i64::try_from(generation).unwrap_or(i64::MAX),
                ),
            )
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to store the commands key".to_string(),
            })?;
        if inserted == 0 {
            return Ok(None);
        }
        Ok(Some(TokenRecord {
            id,
            token_type: TokenType::CommandsReceiver,
            key_prefix: key_prefix.to_owned(),
            key_hash: key_hash.to_owned(),
            deployment_group_id: None,
            deployment_id: Some(deployment_id.to_owned()),
            created_at: now,
            expires_at: None,
        }))
    }

    async fn revoke_commands_keys(&self, deployment_id: &str) -> Result<(), AlienError> {
        let failed = || GenericError {
            message: "Commands key revocation failed".to_string(),
        };
        let mut conn = self.db.conn().lock().await;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .into_alien_error()
            .context(failed())?;
        tx.execute(
            "DELETE FROM tokens WHERE type = 'commands-receiver' AND deployment_id = ?1",
            [deployment_id.to_owned()],
        )
        .await
        .into_alien_error()
        .context(failed())?;
        tx.execute(
            "INSERT INTO commands_key_generations (deployment_id, generation) VALUES (?1, 1) \
             ON CONFLICT(deployment_id) DO UPDATE SET \
             generation = commands_key_generations.generation + 1",
            [deployment_id.to_owned()],
        )
        .await
        .into_alien_error()
        .context(failed())?;
        tx.commit().await.into_alien_error().context(failed())?;
        Ok(())
    }

    async fn create_setup_token(
        &self,
        token_type: TokenType,
        deployment_id: &str,
        key_prefix: &str,
        key_hash: &str,
        expires_at: chrono::DateTime<Utc>,
    ) -> Result<TokenRecord, AlienError> {
        let failed = || GenericError {
            message: "Setup token creation failed".to_string(),
        };
        if !matches!(token_type, TokenType::Setup | TokenType::SetupImport) {
            return Err(AlienError::new(failed()));
        }
        let type_name = token_type.to_string();
        let id = alien_core::new_id(alien_core::IdType::Token);
        let now = Utc::now();
        let mut conn = self.db.conn().lock().await;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .into_alien_error()
            .context(failed())?;
        // The earlier setup tokens go in the same transaction as the new one is
        // stored, so two reruns never both hold a live token.
        let mut revoked = Vec::new();
        let mut rows = tx
            .query(
                "SELECT id FROM tokens WHERE type = ?1 AND deployment_id = ?2",
                (type_name.clone(), deployment_id.to_owned()),
            )
            .await
            .into_alien_error()
            .context(failed())?;
        while let Some(row) = rows.next().await.into_alien_error().context(failed())? {
            revoked.push(RowParser::new(&row).string(0, "id")?);
        }
        drop(rows);
        tx.execute(
            "DELETE FROM tokens WHERE type = ?1 AND deployment_id = ?2",
            (type_name.clone(), deployment_id.to_owned()),
        )
        .await
        .into_alien_error()
        .context(failed())?;
        tx.execute(
            "INSERT INTO tokens (id, type, key_prefix, key_hash, deployment_group_id, deployment_id, created_at, expires_at) \
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7)",
            (
                id.clone(),
                type_name,
                key_prefix.to_owned(),
                key_hash.to_owned(),
                deployment_id.to_owned(),
                now.to_rfc3339(),
                expires_at.to_rfc3339(),
            ),
        )
        .await
        .into_alien_error()
        .context(failed())?;
        tx.commit().await.into_alien_error().context(failed())?;
        // The manager has no audit store, so every mint is recorded as a
        // structured event at info level. It names the key by id and its
        // non-secret prefix, never the key itself.
        tracing::info!(
            target: "audit",
            event = "setup_key_minted",
            deployment_id = %deployment_id,
            issuer = %token_type,
            token_id = %id,
            key_prefix = %key_prefix,
            created_at = %now.to_rfc3339(),
            expires_at = %expires_at.to_rfc3339(),
            revoked_token_ids = ?revoked,
            "Setup key minted"
        );
        Ok(TokenRecord {
            id,
            token_type,
            key_prefix: key_prefix.to_owned(),
            key_hash: key_hash.to_owned(),
            deployment_group_id: None,
            deployment_id: Some(deployment_id.to_owned()),
            created_at: now,
            expires_at: Some(expires_at),
        })
    }
}
