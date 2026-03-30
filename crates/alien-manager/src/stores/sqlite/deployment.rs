//! SQLite implementation of DeploymentStore.

use async_trait::async_trait;
use chrono::Utc;
use sea_query::{Expr, Order, Query, SqliteQueryBuilder};
use std::sync::Arc;
use uuid::Uuid;

use alien_core::{EnvironmentVariable, Platform};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};

use super::database::{db_error, RowParser, SqliteDatabase};
use super::migrations::{DeploymentGroups, Deployments};
use crate::ids;
use crate::traits::deployment_store::*;

pub struct SqliteDeploymentStore {
    db: Arc<SqliteDatabase>,
}

impl SqliteDeploymentStore {
    pub fn new(db: Arc<SqliteDatabase>) -> Self {
        Self { db }
    }

    /// All columns needed for deployment queries (must match parse_deployment order).
    const DEPLOYMENT_COLUMNS: [Deployments; 18] = [
        Deployments::Id,
        Deployments::Name,
        Deployments::DeploymentGroupId,
        Deployments::Platform,
        Deployments::Status,
        Deployments::StackSettings,
        Deployments::StackState,
        Deployments::EnvironmentInfo,
        Deployments::RuntimeMetadata,
        Deployments::CurrentReleaseId,
        Deployments::DesiredReleaseId,
        Deployments::EnvironmentVariables,
        Deployments::RetryRequested,
        Deployments::LockedBy,
        Deployments::LockedAt,
        Deployments::CreatedAt,
        Deployments::UpdatedAt,
        Deployments::Error,
    ];

    fn parse_deployment(row: &turso::Row) -> Result<DeploymentRecord, AlienError> {
        let p = RowParser::new(row);
        let platform_str: String = p.string(3, "platform")?;
        let platform: Platform = platform_str.parse().map_err(|e: String| db_error(&e))?;

        // Parse user environment variables from JSON TEXT column
        let user_environment_variables: Option<Vec<EnvironmentVariable>> =
            p.optional_json(11, "environment_variables")?;

        let retry_requested_int: i64 = p.optional_i64(12, "retry_requested")?.unwrap_or(0);

        Ok(DeploymentRecord {
            id: p.string(0, "id")?,
            name: p.string(1, "name")?,
            deployment_group_id: p.string(2, "deployment_group_id")?,
            platform,
            status: p.string(4, "status")?,
            stack_settings: p.json(5, "stack_settings")?,
            stack_state: p.optional_json(6, "stack_state")?,
            environment_info: p.optional_json(7, "environment_info")?,
            runtime_metadata: p.optional_json(8, "runtime_metadata")?,
            current_release_id: p.optional_string(9, "current_release_id")?,
            desired_release_id: p.optional_string(10, "desired_release_id")?,
            user_environment_variables,
            management_config: None,
            retry_requested: retry_requested_int != 0,
            locked_by: p.optional_string(13, "locked_by")?,
            locked_at: p.optional_datetime(14, "locked_at")?,
            created_at: p.datetime(15, "created_at")?,
            updated_at: p.optional_datetime(16, "updated_at")?,
            error: p.optional_json(17, "error")?,
        })
    }

    fn parse_deployment_group(row: &turso::Row) -> Result<DeploymentGroupRecord, AlienError> {
        let p = RowParser::new(row);
        Ok(DeploymentGroupRecord {
            id: p.string(0, "id")?,
            name: p.string(1, "name")?,
            max_deployments: p.i64(2, "max_deployments")?,
            deployment_count: p.i64(3, "deployment_count")?,
            created_at: p.datetime(4, "created_at")?,
        })
    }
}

#[async_trait]
impl DeploymentStore for SqliteDeploymentStore {
    async fn create_deployment(
        &self,
        params: CreateDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError> {
        let id = ids::deployment_id();
        let now = Utc::now();

        let stack_settings_json = serde_json::to_string(&params.stack_settings)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize stack_settings".to_string(),
            })?;

        let env_vars_json: Option<String> = params
            .environment_variables
            .as_ref()
            .map(|ev| serde_json::to_string(ev))
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize environment_variables".to_string(),
            })?;

        // Build SQL in a block so sea_query types (which contain Rc and are !Send)
        // are dropped before the .await.
        let sql = {
            let mut columns = vec![
                Deployments::Id,
                Deployments::Name,
                Deployments::DeploymentGroupId,
                Deployments::Platform,
                Deployments::Status,
                Deployments::StackSettings,
                Deployments::RetryRequested,
                Deployments::CreatedAt,
            ];
            let mut values: Vec<sea_query::SimpleExpr> = vec![
                id.clone().into(),
                params.name.clone().into(),
                params.deployment_group_id.clone().into(),
                params.platform.as_str().to_string().into(),
                "pending".to_string().into(),
                stack_settings_json.into(),
                0i64.into(),
                now.to_rfc3339().into(),
            ];

            if let Some(ref ev_json) = env_vars_json {
                columns.push(Deployments::EnvironmentVariables);
                values.push(ev_json.clone().into());
            }

            Query::insert()
                .into_table(Deployments::Table)
                .columns(columns)
                .values(values)
                .map_err(|e| db_error(&format!("Failed to build deployment insert query: {}", e)))?
                .to_string(SqliteQueryBuilder)
        };

        self.db.execute(&sql).await?;

        // Increment deployment count on the group
        let inc_sql = Query::update()
            .table(DeploymentGroups::Table)
            .value(
                DeploymentGroups::DeploymentCount,
                Expr::col(DeploymentGroups::DeploymentCount).add(1),
            )
            .and_where(Expr::col(DeploymentGroups::Id).eq(params.deployment_group_id.as_str()))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&inc_sql).await?;

        Ok(DeploymentRecord {
            id,
            name: params.name,
            deployment_group_id: params.deployment_group_id,
            platform: params.platform,
            status: "pending".to_string(),
            stack_settings: params.stack_settings,
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            user_environment_variables: params.environment_variables,
            management_config: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: now,
            updated_at: None,
            error: None,
        })
    }

    async fn get_deployment(&self, id: &str) -> Result<Option<DeploymentRecord>, AlienError> {
        let sql = Query::select()
            .columns(Self::DEPLOYMENT_COLUMNS)
            .from(Deployments::Table)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to query deployment".to_string(),
            })?;

        match rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch deployment row".to_string(),
        })? {
            Some(row) => Ok(Some(Self::parse_deployment(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_deployments(
        &self,
        filter: &DeploymentFilter,
    ) -> Result<Vec<DeploymentRecord>, AlienError> {
        let sql = {
            let mut query = Query::select();
            query
                .columns(Self::DEPLOYMENT_COLUMNS)
                .from(Deployments::Table)
                .order_by(Deployments::CreatedAt, Order::Desc);

            if let Some(dg_id) = &filter.deployment_group_id {
                query.and_where(Expr::col(Deployments::DeploymentGroupId).eq(dg_id.as_str()));
            }
            if let Some(statuses) = &filter.statuses {
                let status_strs: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
                query.and_where(Expr::col(Deployments::Status).is_in(status_strs));
            }
            if let Some(platforms) = &filter.platforms {
                let platform_strs: Vec<&str> = platforms.iter().map(|p| p.as_str()).collect();
                query.and_where(Expr::col(Deployments::Platform).is_in(platform_strs));
            }
            if let Some(limit) = filter.limit {
                query.limit(limit as u64);
            }
            query.to_string(SqliteQueryBuilder)
        };

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to list deployments".to_string(),
            })?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch deployment row".to_string(),
        })? {
            results.push(Self::parse_deployment(&row)?);
        }
        Ok(results)
    }

    async fn delete_deployment(&self, id: &str) -> Result<(), AlienError> {
        // Get the deployment first to find its group for decrementing count
        let deployment = self.get_deployment(id).await?;

        let sql = Query::delete()
            .from_table(Deployments::Table)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await?;

        // Decrement deployment count on the group
        if let Some(dep) = deployment {
            let dec_sql = Query::update()
                .table(DeploymentGroups::Table)
                .value(
                    DeploymentGroups::DeploymentCount,
                    Expr::col(DeploymentGroups::DeploymentCount).sub(1),
                )
                .and_where(Expr::col(DeploymentGroups::Id).eq(dep.deployment_group_id.as_str()))
                .to_string(SqliteQueryBuilder);
            self.db.execute(&dec_sql).await?;
        }

        Ok(())
    }

    async fn set_retry_requested(&self, id: &str) -> Result<(), AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::RetryRequested, 1i64)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await
    }

    async fn set_redeploy(&self, id: &str) -> Result<(), AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::Status, "update-pending")
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await
    }

    async fn set_deployment_desired_release(
        &self,
        deployment_id: &str,
        release_id: &str,
    ) -> Result<(), AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::DesiredReleaseId, release_id)
            .and_where(Expr::col(Deployments::Id).eq(deployment_id))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await
    }

    async fn set_desired_release(
        &self,
        release_id: &str,
        platform: Option<Platform>,
    ) -> Result<(), AlienError> {
        // Set desired_release_id on all deployments that are in a state that can receive updates
        let eligible_statuses = ["running", "update-failed", "refresh-failed"];

        let sql = {
            let mut query = Query::update();
            query
                .table(Deployments::Table)
                .value(Deployments::DesiredReleaseId, release_id)
                .value(Deployments::Status, "update-pending")
                .and_where(Expr::col(Deployments::Status).is_in(eligible_statuses));

            if let Some(p) = platform {
                query.and_where(Expr::col(Deployments::Platform).eq(p.as_str()));
            }

            query.to_string(SqliteQueryBuilder)
        };

        self.db.execute(&sql).await
    }

    async fn acquire(
        &self,
        session: &str,
        filter: &DeploymentFilter,
        limit: u32,
    ) -> Result<Vec<AcquiredDeployment>, AlienError> {
        let now = Utc::now();

        // Work statuses that need active deployment work
        let work_statuses = [
            "pending",
            "initial-setup",
            "provisioning",
            "updating",
            "deleting",
            "update-pending",
            "delete-pending",
        ];

        // Failed statuses that can be retried when retry_requested is true
        let failed_statuses = [
            "initial-setup-failed",
            "provisioning-failed",
            "update-failed",
            "delete-failed",
        ];

        // SELECT deployments that need work AND are not locked
        let select_sql = {
            let mut query = Query::select();
            query
                .columns(Self::DEPLOYMENT_COLUMNS)
                .from(Deployments::Table)
                .cond_where(
                    sea_query::Cond::any()
                        .add(Expr::col(Deployments::Status).is_in(work_statuses))
                        .add(
                            sea_query::Cond::all()
                                .add(Expr::col(Deployments::Status).is_in(failed_statuses))
                                .add(Expr::col(Deployments::RetryRequested).eq(1)),
                        ),
                )
                .and_where(Expr::col(Deployments::LockedBy).is_null());

            if let Some(dg_id) = &filter.deployment_group_id {
                query.and_where(Expr::col(Deployments::DeploymentGroupId).eq(dg_id.as_str()));
            }
            if let Some(ids) = &filter.deployment_ids {
                let id_strs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
                query.and_where(Expr::col(Deployments::Id).is_in(id_strs));
            }
            if let Some(statuses) = &filter.statuses {
                let status_strs: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
                query.and_where(Expr::col(Deployments::Status).is_in(status_strs));
            }
            if let Some(platforms) = &filter.platforms {
                let platform_strs: Vec<&str> = platforms.iter().map(|p| p.as_str()).collect();
                query.and_where(Expr::col(Deployments::Platform).is_in(platform_strs));
            }

            query.limit(limit as u64);
            query.to_string(SqliteQueryBuilder)
        };

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&select_sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to query deployments for acquire".to_string(),
            })?;

        let mut deployments = Vec::new();
        while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch deployment row".to_string(),
        })? {
            deployments.push(Self::parse_deployment(&row)?);
        }
        // Must drop rows before using conn again
        drop(rows);

        // Lock each acquired deployment
        let mut acquired = Vec::new();
        for dep in deployments {
            let lock_sql = Query::update()
                .table(Deployments::Table)
                .value(Deployments::LockedBy, session)
                .value(Deployments::LockedAt, now.to_rfc3339())
                .and_where(Expr::col(Deployments::Id).eq(dep.id.as_str()))
                .and_where(Expr::col(Deployments::LockedBy).is_null())
                .to_string(SqliteQueryBuilder);
            conn.execute(&lock_sql, ())
                .await
                .into_alien_error()
                .context(GenericError {
                    message: "Failed to lock deployment".to_string(),
                })?;

            acquired.push(AcquiredDeployment {
                deployment: DeploymentRecord {
                    locked_by: Some(session.to_string()),
                    locked_at: Some(now),
                    ..dep
                },
            });
        }

        Ok(acquired)
    }

    async fn reconcile(&self, data: ReconcileData) -> Result<DeploymentRecord, AlienError> {
        let now = Utc::now();
        let state = &data.state;

        let stack_state_json: Option<String> = state
            .stack_state
            .as_ref()
            .map(|s| serde_json::to_string(s))
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize stack_state".to_string(),
            })?;

        let environment_info_json: Option<String> = state
            .environment_info
            .as_ref()
            .map(|e| serde_json::to_string(e))
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize environment_info".to_string(),
            })?;

        let runtime_metadata_json: Option<String> = state
            .runtime_metadata
            .as_ref()
            .map(|r| serde_json::to_string(r))
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize runtime_metadata".to_string(),
            })?;

        let current_release_id = state.current_release.as_ref().map(|r| r.release_id.clone());

        // Serialize status using serde (kebab-case per DeploymentStatus definition)
        let status_str = serde_json::to_value(&state.status)
            .and_then(|v| Ok(v.as_str().unwrap_or("pending").to_string()))
            .unwrap_or_else(|_| "pending".to_string());

        // Check if deployment completed (current matches desired)
        let deployment = self.get_deployment(&data.deployment_id).await?;
        let deployment_completed = deployment
            .as_ref()
            .and_then(|d| d.desired_release_id.as_ref())
            .and_then(|desired| {
                current_release_id
                    .as_ref()
                    .map(|current| current == desired)
            })
            .unwrap_or(false);

        let desired_release_value = if deployment_completed {
            "NULL".to_string()
        } else {
            deployment
                .as_ref()
                .and_then(|d| d.desired_release_id.as_ref())
                .map(|id| format!("'{}'", id.replace('\'', "''")))
                .unwrap_or_else(|| "NULL".to_string())
        };

        let error_json: Option<String> = data
            .error
            .as_ref()
            .map(|e| serde_json::to_string(e))
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize error".to_string(),
            })?;

        // Always set ALL fields (even when None) to clear stale data
        let sql = format!(
            "UPDATE deployments SET status = '{}', stack_state = {}, environment_info = {}, runtime_metadata = {}, current_release_id = {}, desired_release_id = {}, retry_requested = 0, error = {}, updated_at = '{}' WHERE id = '{}'",
            status_str.replace('\'', "''"),
            stack_state_json.as_ref().map(|s| format!("'{}'", s.replace('\'', "''"))).unwrap_or_else(|| "NULL".to_string()),
            environment_info_json.as_ref().map(|s| format!("'{}'", s.replace('\'', "''"))).unwrap_or_else(|| "NULL".to_string()),
            runtime_metadata_json.as_ref().map(|s| format!("'{}'", s.replace('\'', "''"))).unwrap_or_else(|| "NULL".to_string()),
            current_release_id.as_ref().map(|s| format!("'{}'", s.replace('\'', "''"))).unwrap_or_else(|| "NULL".to_string()),
            desired_release_value,
            error_json.as_ref().map(|s| format!("'{}'", s.replace('\'', "''"))).unwrap_or_else(|| "NULL".to_string()),
            now.to_rfc3339(),
            data.deployment_id.replace('\'', "''"),
        );

        self.db.execute(&sql).await?;

        // Fetch and return the updated deployment
        self.get_deployment(&data.deployment_id)
            .await?
            .ok_or_else(|| db_error("Deployment not found after reconcile"))
    }

    async fn release(&self, deployment_id: &str, session: &str) -> Result<(), AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::LockedBy, Option::<String>::None)
            .value(Deployments::LockedAt, Option::<String>::None)
            .and_where(Expr::col(Deployments::Id).eq(deployment_id))
            .and_where(Expr::col(Deployments::LockedBy).eq(session))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await
    }

    // --- Deployment groups ---

    async fn create_deployment_group(
        &self,
        params: CreateDeploymentGroupParams,
    ) -> Result<DeploymentGroupRecord, AlienError> {
        let id = format!("dg_{}", Uuid::new_v4());
        let now = Utc::now();

        let sql = Query::insert()
            .into_table(DeploymentGroups::Table)
            .columns([
                DeploymentGroups::Id,
                DeploymentGroups::Name,
                DeploymentGroups::MaxDeployments,
                DeploymentGroups::DeploymentCount,
                DeploymentGroups::CreatedAt,
            ])
            .values_panic([
                id.clone().into(),
                params.name.clone().into(),
                params.max_deployments.into(),
                0i64.into(),
                now.to_rfc3339().into(),
            ])
            .to_string(SqliteQueryBuilder);

        self.db.execute(&sql).await?;

        Ok(DeploymentGroupRecord {
            id,
            name: params.name,
            max_deployments: params.max_deployments,
            deployment_count: 0,
            created_at: now,
        })
    }

    async fn create_deployment_group_with_id(
        &self,
        id: &str,
        params: CreateDeploymentGroupParams,
    ) -> Result<DeploymentGroupRecord, AlienError> {
        let now = Utc::now();

        let sql = Query::insert()
            .into_table(DeploymentGroups::Table)
            .columns([
                DeploymentGroups::Id,
                DeploymentGroups::Name,
                DeploymentGroups::MaxDeployments,
                DeploymentGroups::DeploymentCount,
                DeploymentGroups::CreatedAt,
            ])
            .values_panic([
                id.into(),
                params.name.clone().into(),
                params.max_deployments.into(),
                0i64.into(),
                now.to_rfc3339().into(),
            ])
            .to_string(SqliteQueryBuilder);

        self.db.execute(&sql).await?;

        Ok(DeploymentGroupRecord {
            id: id.to_string(),
            name: params.name,
            max_deployments: params.max_deployments,
            deployment_count: 0,
            created_at: now,
        })
    }

    async fn get_deployment_group(
        &self,
        id: &str,
    ) -> Result<Option<DeploymentGroupRecord>, AlienError> {
        let sql = Query::select()
            .columns([
                DeploymentGroups::Id,
                DeploymentGroups::Name,
                DeploymentGroups::MaxDeployments,
                DeploymentGroups::DeploymentCount,
                DeploymentGroups::CreatedAt,
            ])
            .from(DeploymentGroups::Table)
            .and_where(Expr::col(DeploymentGroups::Id).eq(id))
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to query deployment group".to_string(),
            })?;

        match rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch deployment group row".to_string(),
        })? {
            Some(row) => Ok(Some(Self::parse_deployment_group(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_deployment_groups(&self) -> Result<Vec<DeploymentGroupRecord>, AlienError> {
        let sql = Query::select()
            .columns([
                DeploymentGroups::Id,
                DeploymentGroups::Name,
                DeploymentGroups::MaxDeployments,
                DeploymentGroups::DeploymentCount,
                DeploymentGroups::CreatedAt,
            ])
            .from(DeploymentGroups::Table)
            .order_by(DeploymentGroups::CreatedAt, Order::Desc)
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to list deployment groups".to_string(),
            })?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to fetch deployment group row".to_string(),
        })? {
            results.push(Self::parse_deployment_group(&row)?);
        }
        Ok(results)
    }
}
