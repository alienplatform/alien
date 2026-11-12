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
    const RELEASE_COLUMNS: [Releases; 10] = [
        Releases::Id,               // 0
        Releases::Stack,            // 1
        Releases::GitCommitSha,     // 2
        Releases::GitCommitRef,     // 3
        Releases::GitCommitMessage, // 4
        Releases::CreatedAt,        // 5
        Releases::WorkspaceId,      // 6
        Releases::ProjectId,        // 7
        Releases::SignedStacks,     // 8
        Releases::SetupTemplates,   // 9
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
            signed_stacks: p.optional_json(8, "signed_stacks")?.unwrap_or_default(),
            setup_templates: p.optional_json(9, "setup_templates")?.unwrap_or_default(),
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
        let signed_stacks_json = if params.signed_stacks.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&params.signed_stacks)
                    .into_alien_error()
                    .context(GenericError {
                        message: "Failed to serialize signed stacks".to_string(),
                    })?,
            )
        };

        let setup_templates_json = if params.setup_templates.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&params.setup_templates)
                    .into_alien_error()
                    .context(GenericError {
                        message: "Failed to serialize setup templates".to_string(),
                    })?,
            )
        };

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
                Releases::SignedStacks,
                Releases::SetupTemplates,
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
                signed_stacks_json.into(),
                setup_templates_json.into(),
            ])
            .to_string(SqliteQueryBuilder);

        self.db.execute(&sql).await?;

        Ok(ReleaseRecord {
            id,
            workspace_id,
            project_id,
            stacks: params.stacks,
            signed_stacks: params.signed_stacks,
            setup_templates: params.setup_templates,
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

    async fn latest_release_id(
        &self,
        _caller: &crate::auth::Subject,
    ) -> Result<Option<String>, AlienError> {
        let sql = Query::select()
            .column(Releases::Id)
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
                message: "Failed to query latest release id".to_string(),
            })?;

        match rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch release id row".to_string(),
        })? {
            Some(row) => Ok(Some(RowParser::new(&row).string(0, "id")?)),
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

    async fn list_releases(
        &self,
        _caller: &crate::auth::Subject,
    ) -> Result<Vec<ReleaseRecord>, AlienError> {
        // Single-tenant store: no workspace filter (every row is the one
        // workspace). Multi-tenant scoping is the embedder's job.
        let sql = Query::select()
            .columns(Self::RELEASE_COLUMNS)
            .from(Releases::Table)
            .order_by(Releases::CreatedAt, Order::Desc)
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to query releases".to_string(),
            })?;

        let mut releases = Vec::new();
        while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch release row".to_string(),
        })? {
            releases.push(Self::parse_release(&row)?);
        }
        Ok(releases)
    }

    async fn store_chart_if_absent(
        &self,
        caller: &crate::auth::Subject,
        chart: StoredChart,
    ) -> Result<StoredChart, AlienError> {
        {
            let conn = self.db.conn().lock().await;
            conn.execute(
                "INSERT INTO release_charts (release_id, version, manifest, config, archive, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (release_id, version) DO NOTHING",
                (
                    chart.release_id.as_str(),
                    chart.version.as_str(),
                    chart.manifest.clone(),
                    chart.config.clone(),
                    chart.archive.clone(),
                    Utc::now().timestamp_millis(),
                ),
            )
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to store chart".to_string(),
            })?;
        }
        // The kept copy: ours, or the one another manager stored first.
        self.stored_charts(caller, std::slice::from_ref(&chart.release_id))
            .await?
            .into_iter()
            .find(|kept| kept.version == chart.version)
            .ok_or_else(|| {
                AlienError::new(GenericError {
                    message: format!(
                        "chart {} of release {} is missing right after it was stored",
                        chart.version, chart.release_id
                    ),
                })
            })
    }

    async fn stored_charts(
        &self,
        _caller: &crate::auth::Subject,
        release_ids: &[String],
    ) -> Result<Vec<StoredChart>, AlienError> {
        let mut charts = Vec::new();
        let conn = self.db.conn().lock().await;
        for release_id in release_ids {
            let mut rows = conn
                .query(
                    "SELECT release_id, version, manifest, config, archive
                     FROM release_charts WHERE release_id = ?1 ORDER BY version",
                    (release_id.as_str(),),
                )
                .await
                .into_alien_error()
                .context(GenericError {
                    message: "Failed to query charts".to_string(),
                })?;
            while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
                message: "Failed to fetch chart row".to_string(),
            })? {
                let read = |index: usize, name: &str| -> Result<Vec<u8>, AlienError> {
                    row.get(index).into_alien_error().context(GenericError {
                        message: format!("Failed to read chart column '{name}'"),
                    })
                };
                let p = RowParser::new(&row);
                charts.push(StoredChart {
                    release_id: p.string(0, "release_id")?,
                    version: p.string(1, "version")?,
                    manifest: read(2, "manifest")?,
                    config: read(3, "config")?,
                    archive: read(4, "archive")?,
                });
            }
        }
        Ok(charts)
    }
}
