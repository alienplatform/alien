//! SQLite implementation of ReleaseStore.

use async_trait::async_trait;
use chrono::Utc;
use sea_query::{Expr, Order, Query, SqliteQueryBuilder};
use std::sync::Arc;
use uuid::Uuid;

use alien_error::{AlienError, Context, GenericError, IntoAlienError};

use super::database::{RowParser, SqliteDatabase};
use super::migrations::Releases;
use crate::traits::release_store::*;

pub struct SqliteReleaseStore {
    db: Arc<SqliteDatabase>,
}

impl SqliteReleaseStore {
    pub fn new(db: Arc<SqliteDatabase>) -> Self {
        Self { db }
    }

    const RELEASE_COLUMNS: [Releases; 6] = [
        Releases::Id,
        Releases::Stack,
        Releases::GitCommitSha,
        Releases::GitCommitRef,
        Releases::GitCommitMessage,
        Releases::CreatedAt,
    ];

    fn parse_release(row: &turso::Row) -> Result<ReleaseRecord, AlienError> {
        let p = RowParser::new(row);
        Ok(ReleaseRecord {
            id: p.string(0, "id")?,
            stack: p.json(1, "stack")?,
            git_commit_sha: p.optional_string(2, "git_commit_sha")?,
            git_commit_ref: p.optional_string(3, "git_commit_ref")?,
            git_commit_message: p.optional_string(4, "git_commit_message")?,
            created_at: p.datetime(5, "created_at")?,
        })
    }
}

#[async_trait]
impl ReleaseStore for SqliteReleaseStore {
    async fn create_release(
        &self,
        params: CreateReleaseParams,
    ) -> Result<ReleaseRecord, AlienError> {
        let id = format!("rel_{}", Uuid::new_v4());
        let now = Utc::now();

        let stack_json = serde_json::to_string(&params.stack)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize stack".to_string(),
            })?;

        let sql = Query::insert()
            .into_table(Releases::Table)
            .columns([
                Releases::Id,
                Releases::Stack,
                Releases::GitCommitSha,
                Releases::GitCommitRef,
                Releases::GitCommitMessage,
                Releases::CreatedAt,
            ])
            .values_panic([
                id.clone().into(),
                stack_json.into(),
                params.git_commit_sha.clone().into(),
                params.git_commit_ref.clone().into(),
                params.git_commit_message.clone().into(),
                now.to_rfc3339().into(),
            ])
            .to_string(SqliteQueryBuilder);

        self.db.execute(&sql).await?;

        Ok(ReleaseRecord {
            id,
            stack: params.stack,
            git_commit_sha: params.git_commit_sha,
            git_commit_ref: params.git_commit_ref,
            git_commit_message: params.git_commit_message,
            created_at: now,
        })
    }

    async fn get_release(&self, id: &str) -> Result<Option<ReleaseRecord>, AlienError> {
        let sql = Query::select()
            .columns(Self::RELEASE_COLUMNS)
            .from(Releases::Table)
            .and_where(Expr::col(Releases::Id).eq(id))
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to query release".to_string(),
            })?;

        match rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch release row".to_string(),
        })? {
            Some(row) => Ok(Some(Self::parse_release(&row)?)),
            None => Ok(None),
        }
    }

    async fn get_latest_release(&self) -> Result<Option<ReleaseRecord>, AlienError> {
        let sql = Query::select()
            .columns(Self::RELEASE_COLUMNS)
            .from(Releases::Table)
            .order_by(Releases::CreatedAt, Order::Desc)
            .limit(1)
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to query latest release".to_string(),
            })?;

        match rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch release row".to_string(),
        })? {
            Some(row) => Ok(Some(Self::parse_release(&row)?)),
            None => Ok(None),
        }
    }
}
