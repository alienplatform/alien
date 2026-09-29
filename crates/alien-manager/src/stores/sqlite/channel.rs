//! SQLite implementation of ReleaseChannelStore, alongside the SQLite
//! deployment store (routing joins the deployments table).

use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;

use super::database::{RowParser, SqliteDatabase};
use crate::traits::release_channel_store::*;

/// Statuses in which a deployment takes a new release, as when a release is
/// created without channels.
const ELIGIBLE_STATUSES: &str = "('running', 'update-failed', 'refresh-failed')";

pub struct SqliteReleaseChannelStore {
    db: Arc<SqliteDatabase>,
}

impl SqliteReleaseChannelStore {
    pub fn new(db: Arc<SqliteDatabase>) -> Self {
        Self { db }
    }

    async fn run(&self, sql: &str, params: impl turso::IntoParams) -> Result<u64, AlienError> {
        let conn = self.db.conn().lock().await;
        conn.execute(sql, params)
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Release channel update failed".to_string(),
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
                message: "Release channel query failed".to_string(),
            })?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to read release channel row".to_string(),
        })? {
            out.push(row);
        }
        Ok(out)
    }

    fn parse_channel(row: &turso::Row) -> Result<ReleaseChannelRecord, AlienError> {
        let p = RowParser::new(row);
        Ok(ReleaseChannelRecord {
            name: p.string(0, "name")?,
            current_release_id: p.optional_string(1, "current_release_id")?,
            updated_at: p.datetime(2, "updated_at")?,
        })
    }

    fn not_found(name: &str) -> AlienError {
        AlienError::new(GenericError {
            message: format!("Release channel '{name}' was not found after writing it"),
        })
    }
}

#[async_trait]
impl ReleaseChannelStore for SqliteReleaseChannelStore {
    async fn list_channels(&self) -> Result<Vec<ReleaseChannelRecord>, AlienError> {
        self.rows(
            "SELECT name, current_release_id, updated_at FROM release_channels ORDER BY name",
            (),
        )
        .await?
        .iter()
        .map(Self::parse_channel)
        .collect()
    }

    async fn get_channel(&self, name: &str) -> Result<Option<ReleaseChannelRecord>, AlienError> {
        self.rows(
            "SELECT name, current_release_id, updated_at FROM release_channels WHERE name = ?1",
            (name,),
        )
        .await?
        .first()
        .map(Self::parse_channel)
        .transpose()
    }

    async fn create_channel(
        &self,
        name: &str,
        release_id: Option<&str>,
    ) -> Result<ReleaseChannelRecord, AlienError> {
        let inserted = self
            .run(
                "INSERT OR IGNORE INTO release_channels (name, current_release_id, updated_at) VALUES (?1, ?2, ?3)",
                (name, release_id.map(str::to_string), Utc::now().to_rfc3339()),
            )
            .await?;
        if inserted == 0 {
            return Err(AlienError::new(GenericError {
                message: format!("Release channel '{name}' already exists"),
            }));
        }
        self.get_channel(name)
            .await?
            .ok_or_else(|| Self::not_found(name))
    }

    async fn delete_channel(&self, name: &str) -> Result<bool, AlienError> {
        Ok(self
            .run("DELETE FROM release_channels WHERE name = ?1", (name,))
            .await?
            > 0)
    }

    async fn set_channel_release(
        &self,
        name: &str,
        release_id: &str,
    ) -> Result<ReleaseChannelRecord, AlienError> {
        self.run(
            "INSERT INTO release_channels (name, current_release_id, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET current_release_id = excluded.current_release_id, updated_at = excluded.updated_at",
            (name, release_id, Utc::now().to_rfc3339()),
        )
        .await?;
        self.get_channel(name)
            .await?
            .ok_or_else(|| Self::not_found(name))
    }

    async fn deployment_routing(
        &self,
        deployment_id: &str,
    ) -> Result<DeploymentRouting, AlienError> {
        let rows = self
            .rows(
                "SELECT channel, pinned_release_id FROM deployment_channels WHERE deployment_id = ?1",
                (deployment_id,),
            )
            .await?;
        match rows.first() {
            Some(row) => {
                let p = RowParser::new(row);
                Ok(DeploymentRouting {
                    channel: p.string(0, "channel")?,
                    pinned_release_id: p.optional_string(1, "pinned_release_id")?,
                })
            }
            None => Ok(DeploymentRouting {
                channel: DEFAULT_CHANNEL.to_string(),
                pinned_release_id: None,
            }),
        }
    }

    async fn set_deployment_routing(
        &self,
        deployment_id: &str,
        routing: &DeploymentRouting,
    ) -> Result<(), AlienError> {
        self.run(
            "INSERT INTO deployment_channels (deployment_id, channel, pinned_release_id) VALUES (?1, ?2, ?3)
             ON CONFLICT(deployment_id) DO UPDATE SET channel = excluded.channel, pinned_release_id = excluded.pinned_release_id",
            (
                deployment_id,
                routing.channel.as_str(),
                routing.pinned_release_id.clone(),
            ),
        )
        .await
        .map(|_| ())
    }

    async fn count_following(&self, channel: &str) -> Result<u64, AlienError> {
        let rows = self
            .rows(
                "SELECT COUNT(*) FROM deployments d
                 LEFT JOIN deployment_channels c ON c.deployment_id = d.id
                 WHERE COALESCE(c.channel, ?1) = ?2",
                (DEFAULT_CHANNEL, channel),
            )
            .await?;
        let count = rows
            .first()
            .map(|row| RowParser::new(row).i64(0, "count"))
            .transpose()?
            .unwrap_or(0);
        Ok(count as u64)
    }

    async fn roll_out_channel(&self, channel: &str, release_id: &str) -> Result<(), AlienError> {
        self.run(
            &format!(
                "UPDATE deployments
                 SET desired_release_id = ?1, status = 'update-pending', next_step_after = NULL
                 WHERE status IN {ELIGIBLE_STATUSES}
                   AND id IN (
                     SELECT d.id FROM deployments d
                     LEFT JOIN deployment_channels c ON c.deployment_id = d.id
                     WHERE COALESCE(c.channel, ?2) = ?3 AND c.pinned_release_id IS NULL
                   )"
            ),
            (release_id, DEFAULT_CHANNEL, channel),
        )
        .await
        .map(|_| ())
    }

    async fn roll_out_deployment(
        &self,
        deployment_id: &str,
        release_id: &str,
    ) -> Result<(), AlienError> {
        // Deployments still being set up take the release when setup reaches
        // it; running ones start an update now.
        self.run(
            "UPDATE deployments SET desired_release_id = ?1, next_step_after = NULL WHERE id = ?2",
            (release_id, deployment_id),
        )
        .await?;
        self.run(
            &format!(
                "UPDATE deployments SET status = 'update-pending'
                 WHERE id = ?1 AND status IN {ELIGIBLE_STATUSES}
                   AND (current_release_id IS NULL OR current_release_id != ?2)"
            ),
            (deployment_id, release_id),
        )
        .await
        .map(|_| ())
    }
}
