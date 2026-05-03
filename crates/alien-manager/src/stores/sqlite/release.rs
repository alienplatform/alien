//! SQLite implementation of ReleaseStore.

use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use async_trait::async_trait;
use chrono::Utc;
use sea_query::{Expr, Order, Query, SqliteQueryBuilder};
use std::sync::Arc;

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

    /// SELECT column order. Indexes in `parse_release` MUST line up with
    /// this array — keep the two in sync.
    const RELEASE_COLUMNS: [Releases; 8] = [
        Releases::Id,                // 0
        Releases::Stack,             // 1
        Releases::GitCommitSha,      // 2
        Releases::GitCommitRef,      // 3
        Releases::GitCommitMessage,  // 4
        Releases::CreatedAt,         // 5
        Releases::WorkspaceId,       // 6
        Releases::ProjectId,         // 7
    ];

    fn parse_release(row: &turso::Row) -> Result<ReleaseRecord, AlienError> {
        let p = RowParser::new(row);
        Ok(ReleaseRecord {
            id: p.string(0, "id")?,
            stacks: p.json(1, "stack")?,
            git_commit_sha: p.optional_string(2, "git_commit_sha")?,
            git_commit_ref: p.optional_string(3, "git_commit_ref")?,
            git_commit_message: p.optional_string(4, "git_commit_message")?,
            created_at: p.datetime(5, "created_at")?,
            workspace_id: p.string(6, "workspace_id")?,
            project_id: p.string(7, "project_id")?,
        })
    }
}

#[async_trait]
impl ReleaseStore for SqliteReleaseStore {
    async fn create_release(
        &self,
        caller: &crate::auth::Subject,
        params: CreateReleaseParams,
    ) -> Result<ReleaseRecord, AlienError> {
        let id = alien_core::new_id(alien_core::IdType::Release);
        let now = Utc::now();

        // Persisted shape: `{ "<platform>": <stack>, ... }`. Each release
        // can target multiple platforms (one `alien release` invocation
        // produces stacks for every configured target).
        let stacks_json = serde_json::to_string(&params.stacks)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize stacks".to_string(),
            })?;

        // The caller's `workspace_id` is always `"default"` for this
        // single-tenant store; we still propagate it from the subject
        // rather than hardcoding it so a misbehaving validator surfaces
        // immediately rather than silently being normalised away.
        let workspace_id = caller.workspace_id.clone();
        let project_id = params.project_id.clone();

        let sql = Query::insert()
            .into_table(Releases::Table)
            .columns([
                Releases::Id,
                Releases::Stack,
                Releases::GitCommitSha,
                Releases::GitCommitRef,
                Releases::GitCommitMessage,
                Releases::CreatedAt,
                Releases::WorkspaceId,
                Releases::ProjectId,
            ])
            .values_panic([
                id.clone().into(),
                stacks_json.into(),
                params.git_commit_sha.clone().into(),
                params.git_commit_ref.clone().into(),
                params.git_commit_message.clone().into(),
                now.to_rfc3339().into(),
                workspace_id.clone().into(),
                project_id.clone().into(),
            ])
            .to_string(SqliteQueryBuilder);

        self.db.execute(&sql).await?;

        Ok(ReleaseRecord {
            id,
            workspace_id,
            project_id,
            stacks: params.stacks,
            git_commit_sha: params.git_commit_sha,
            git_commit_ref: params.git_commit_ref,
            git_commit_message: params.git_commit_message,
            created_at: now,
        })
    }

    async fn get_release(
        &self,
        _caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<Option<ReleaseRecord>, AlienError> {
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

    async fn get_latest_release(
        &self,
        _caller: &crate::auth::Subject,
    ) -> Result<Option<ReleaseRecord>, AlienError> {
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
