//! SQLite implementation of TokenStore.

use async_trait::async_trait;
use chrono::Utc;
use sea_query::{Expr, Query, SqliteQueryBuilder};
use std::sync::Arc;
use uuid::Uuid;

use alien_error::{AlienError, Context, GenericError, IntoAlienError};

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
        })
    }
}

#[async_trait]
impl TokenStore for SqliteTokenStore {
    async fn create_token(&self, params: CreateTokenParams) -> Result<TokenRecord, AlienError> {
        let id = format!("tok_{}", Uuid::new_v4());
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
            message: "Failed to fetch token row".to_string(),
        })? {
            Some(row) => Ok(Some(Self::parse_token(&row)?)),
            None => Ok(None),
        }
    }
}
