//! SQLite implementation of DeploymentStore.

use async_trait::async_trait;
use chrono::Utc;
use sea_query::{Cond, Expr, Order, Query, SqliteQueryBuilder};
use std::sync::Arc;
use tracing::warn;

use alien_core::{
    import::ImportSourceKind, EnvironmentInfo, EnvironmentVariable, Platform, RuntimeMetadata,
    StackState,
};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};

use super::database::{db_error, RowParser, SqliteDatabase};
use super::migrations::{DeploymentGroups, Deployments};
use crate::error::ErrorData;
use crate::ids;
use crate::traits::deployment_store::*;

fn import_source_to_string(source: &ImportSourceKind) -> String {
    serde_json::to_value(source)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| format!("{source:?}").to_ascii_lowercase())
}

pub struct SqliteDeploymentStore {
    db: Arc<SqliteDatabase>,
}

impl SqliteDeploymentStore {
    const WORK_STATUSES: [&'static str; 7] = [
        "pending",
        "initial-setup",
        "provisioning",
        "updating",
        "deleting",
        "update-pending",
        "delete-pending",
    ];
    const FAILED_STATUSES: [&'static str; 6] = [
        "preflights-failed",
        "initial-setup-failed",
        "provisioning-failed",
        "refresh-failed",
        "update-failed",
        "delete-failed",
    ];
    const SETUP_TEARDOWN_STATUSES: [&'static str; 2] = ["teardown-required", "teardown-failed"];
    const RUNNING_STATUS: &'static str = "running";

    pub fn new(db: Arc<SqliteDatabase>) -> Self {
        Self { db }
    }

    fn should_preserve_retry_requested(
        deployment: &DeploymentRecord,
        reported_state: &alien_core::DeploymentState,
    ) -> bool {
        deployment.retry_requested
            && !reported_state.retry_requested
            && Self::FAILED_STATUSES.contains(&deployment.status.as_str())
            && reported_state.status.is_failed()
    }

    fn acquire_status_condition(statuses: Option<&Vec<String>>) -> sea_query::Condition {
        let retryable_failed = Cond::all()
            .add(Expr::col(Deployments::Status).is_in(Self::FAILED_STATUSES))
            .add(Expr::col(Deployments::RetryRequested).eq(1));

        if let Some(statuses) = statuses {
            let requested_active: Vec<&str> = statuses
                .iter()
                .map(String::as_str)
                .filter(|status| {
                    Self::WORK_STATUSES.contains(status)
                        || Self::SETUP_TEARDOWN_STATUSES.contains(status)
                        || *status == Self::RUNNING_STATUS
                })
                .collect();
            let requested_failed: Vec<&str> = statuses
                .iter()
                .map(String::as_str)
                .filter(|status| Self::FAILED_STATUSES.contains(status))
                .collect();

            let mut condition = Cond::any();
            let mut has_status = false;
            if !requested_active.is_empty() {
                has_status = true;
                condition = condition.add(Expr::col(Deployments::Status).is_in(requested_active));
            }
            if !requested_failed.is_empty() {
                has_status = true;
                condition = condition.add(
                    Cond::all()
                        .add(Expr::col(Deployments::Status).is_in(requested_failed))
                        .add(Expr::col(Deployments::RetryRequested).eq(1)),
                );
            }

            if has_status {
                condition
            } else {
                Cond::all().add(Expr::cust("1 = 0"))
            }
        } else {
            Cond::any()
                .add(Expr::col(Deployments::Status).is_in(Self::WORK_STATUSES))
                .add(retryable_failed)
        }
    }

    fn stale_lock_condition_sql() -> String {
        "\"locked_at\" IS NOT NULL AND julianday(\"locked_at\") < julianday('now', '-5 minutes')"
            .to_string()
    }

    /// All columns needed for deployment queries (must match parse_deployment order).
    const DEPLOYMENT_COLUMNS: [Deployments; 27] = [
        Deployments::Id,
        Deployments::Name,
        Deployments::DeploymentGroupId,
        Deployments::Platform,
        Deployments::DeploymentProtocolVersion,
        Deployments::BasePlatform,
        Deployments::Status,
        Deployments::StackSettings,
        Deployments::StackState,
        Deployments::EnvironmentInfo,
        Deployments::RuntimeMetadata,
        Deployments::CurrentReleaseId,
        Deployments::DesiredReleaseId,
        Deployments::ImportSource,
        Deployments::SetupTarget,
        Deployments::SetupFingerprint,
        Deployments::SetupFingerprintVersion,
        Deployments::EnvironmentVariables,
        Deployments::DeploymentToken,
        Deployments::RetryRequested,
        Deployments::LockedBy,
        Deployments::LockedAt,
        Deployments::CreatedAt,
        Deployments::UpdatedAt,
        Deployments::Error,
        Deployments::WorkspaceId,
        Deployments::ProjectId,
    ];

    fn parse_deployment(row: &turso::Row) -> Result<DeploymentRecord, AlienError> {
        let p = RowParser::new(row);
        let platform_str: String = p.string(3, "platform")?;
        let platform: Platform = platform_str.parse().map_err(|e: String| db_error(&e))?;
        let deployment_protocol_version =
            u32::try_from(p.i64(4, "deployment_protocol_version")?)
                .map_err(|_| db_error("deployment_protocol_version must be a positive u32"))?;
        if deployment_protocol_version == 0 {
            return Err(db_error("deployment_protocol_version must be positive"));
        }
        let base_platform = p
            .optional_string(5, "base_platform")?
            .map(|value| value.parse().map_err(|e: String| db_error(&e)))
            .transpose()?;

        // Parse user environment variables from JSON TEXT column
        let import_source = p
            .optional_string(13, "import_source")?
            .map(|source| serde_json::from_value(serde_json::Value::String(source)))
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to parse import_source".to_string(),
            })?;

        let user_environment_variables: Option<Vec<EnvironmentVariable>> =
            p.optional_json(17, "environment_variables")?;

        let retry_requested_int: i64 = p.optional_i64(19, "retry_requested")?.unwrap_or(0);

        Ok(DeploymentRecord {
            id: p.string(0, "id")?,
            name: p.string(1, "name")?,
            deployment_group_id: p.string(2, "deployment_group_id")?,
            platform,
            deployment_protocol_version,
            base_platform,
            status: p.string(6, "status")?,
            stack_settings: Some(p.json(7, "stack_settings")?),
            stack_state: p.optional_json(8, "stack_state")?,
            environment_info: p.optional_json(9, "environment_info")?,
            runtime_metadata: p.optional_json(10, "runtime_metadata")?,
            current_release_id: p.optional_string(11, "current_release_id")?,
            desired_release_id: p.optional_string(12, "desired_release_id")?,
            import_source,
            setup_method: None,
            setup_metadata: None,
            setup_target: p.optional_string(14, "setup_target")?,
            setup_fingerprint: p.optional_string(15, "setup_fingerprint")?,
            setup_fingerprint_version: p
                .optional_i64(16, "setup_fingerprint_version")?
                .map(|value| value as u32),
            user_environment_variables,
            deployment_token: p.optional_string(18, "deployment_token")?,
            management_config: None,
            deployment_config: None,
            retry_requested: retry_requested_int != 0,
            locked_by: p.optional_string(20, "locked_by")?,
            locked_at: p.optional_datetime(21, "locked_at")?,
            created_at: p.datetime(22, "created_at")?,
            updated_at: p.optional_datetime(23, "updated_at")?,
            error: p.optional_json(24, "error")?,
            workspace_id: p
                .optional_string(25, "workspace_id")?
                .unwrap_or_else(|| "default".to_string()),
            project_id: p
                .optional_string(26, "project_id")?
                .unwrap_or_else(|| "default".to_string()),
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
            workspace_id: p
                .optional_string(5, "workspace_id")?
                .unwrap_or_else(|| "default".to_string()),
            project_id: p
                .optional_string(6, "project_id")?
                .unwrap_or_else(|| "default".to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use alien_core::{
        DeploymentState, DeploymentStatus, Platform, StackSettings,
        CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    };
    use chrono::Utc;

    use crate::traits::DeploymentRecord;

    use super::SqliteDeploymentStore;

    #[test]
    fn stale_lock_condition_parses_rfc3339_timestamps() {
        let condition = SqliteDeploymentStore::stale_lock_condition_sql();

        assert!(condition.contains("julianday(\"locked_at\")"));
        assert!(condition.contains("julianday('now', '-5 minutes')"));
        assert!(!condition.contains("\"locked_at\" < datetime"));
    }

    #[test]
    fn preserves_retry_when_manager_and_agent_states_are_failed() {
        let mut deployment = deployment_record("provisioning-failed");
        deployment.retry_requested = true;
        let reported_state = deployment_state(DeploymentStatus::ProvisioningFailed, false);

        assert!(SqliteDeploymentStore::should_preserve_retry_requested(
            &deployment,
            &reported_state
        ));
    }

    #[test]
    fn does_not_preserve_retry_after_agent_applies_it() {
        let mut deployment = deployment_record("provisioning-failed");
        deployment.retry_requested = true;
        let reported_state = deployment_state(DeploymentStatus::ProvisioningFailed, true);

        assert!(!SqliteDeploymentStore::should_preserve_retry_requested(
            &deployment,
            &reported_state
        ));
    }

    #[test]
    fn does_not_preserve_retry_for_active_agent_state() {
        let mut deployment = deployment_record("provisioning-failed");
        deployment.retry_requested = true;
        let reported_state = deployment_state(DeploymentStatus::Provisioning, false);

        assert!(!SqliteDeploymentStore::should_preserve_retry_requested(
            &deployment,
            &reported_state
        ));
    }

    fn deployment_state(status: DeploymentStatus, retry_requested: bool) -> DeploymentState {
        DeploymentState {
            platform: Platform::Local,
            status,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    fn deployment_record(status: &str) -> DeploymentRecord {
        let now = Utc::now();
        DeploymentRecord {
            id: "dep_test".to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: "test".to_string(),
            deployment_group_id: "dg_test".to_string(),
            platform: Platform::Local,
            deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: status.to_string(),
            stack_settings: Some(StackSettings::default()),
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            deployment_token: None,
            management_config: None,
            deployment_config: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: now,
            updated_at: Some(now),
            error: None,
        }
    }
}

#[async_trait]
impl DeploymentStore for SqliteDeploymentStore {
    async fn create_deployment(
        &self,
        caller: &crate::auth::Subject,
        params: CreateDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError> {
        if let Some(_existing) = self
            .get_deployment_by_name(caller, &params.deployment_group_id, &params.name)
            .await?
        {
            return Err(AlienError::new(ErrorData::DeploymentNameConflict {
                name: params.name,
                deployment_group_id: params.deployment_group_id,
            })
            .into_generic());
        }

        let id = ids::deployment_id();
        let now = Utc::now();

        let stack_settings_json = serde_json::to_string(&params.stack_settings)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize stack_settings".to_string(),
            })?;
        let stack_state_json: Option<String> = params
            .stack_state
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize stack_state".to_string(),
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
                Deployments::DeploymentProtocolVersion,
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
                (params.deployment_protocol_version as i64).into(),
                "pending".to_string().into(),
                stack_settings_json.into(),
                0i64.into(),
                now.to_rfc3339().into(),
            ];

            if let Some(ref ev_json) = env_vars_json {
                columns.push(Deployments::EnvironmentVariables);
                values.push(ev_json.clone().into());
            }

            if let Some(ref state_json) = stack_state_json {
                columns.push(Deployments::StackState);
                values.push(state_json.clone().into());
            }

            if let Some(ref token) = params.deployment_token {
                columns.push(Deployments::DeploymentToken);
                values.push(token.clone().into());
            }

            Query::insert()
                .into_table(Deployments::Table)
                .columns(columns)
                .values(values)
                .map_err(|e| db_error(&format!("Failed to build deployment insert query: {}", e)))?
                .to_string(SqliteQueryBuilder)
        };

        self.db.execute(&sql).await?;

        Ok(DeploymentRecord {
            id,
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: params.name,
            deployment_group_id: params.deployment_group_id,
            platform: params.platform,
            deployment_protocol_version: params.deployment_protocol_version,
            base_platform: None,
            status: "pending".to_string(),
            stack_settings: Some(params.stack_settings),
            stack_state: params.stack_state,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: params.environment_variables,
            deployment_token: params.deployment_token,
            management_config: None,
            deployment_config: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: now,
            updated_at: None,
            error: None,
        })
    }

    async fn create_with_state(
        &self,
        caller: &crate::auth::Subject,
        params: CreateImportedDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError> {
        if self
            .get_deployment_by_name(caller, &params.deployment_group_id, &params.name)
            .await?
            .is_some()
        {
            return Err(AlienError::new(ErrorData::DeploymentNameConflict {
                name: params.name,
                deployment_group_id: params.deployment_group_id,
            })
            .into_generic());
        }

        let id = ids::deployment_id();
        let now = Utc::now();

        let stack_settings_json = serde_json::to_string(&params.stack_settings)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize stack_settings".to_string(),
            })?;

        let stack_state_json = serde_json::to_string(&params.stack_state)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize imported stack_state".to_string(),
            })?;
        let runtime_metadata_json = serde_json::to_string(&params.runtime_metadata)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize imported runtime_metadata".to_string(),
            })?;
        let environment_info_json: Option<String> = params
            .environment_info
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize imported environment_info".to_string(),
            })?;

        let sql = {
            let mut columns = vec![
                Deployments::Id,
                Deployments::Name,
                Deployments::DeploymentGroupId,
                Deployments::Platform,
                Deployments::DeploymentProtocolVersion,
                Deployments::BasePlatform,
                Deployments::Status,
                Deployments::StackSettings,
                Deployments::StackState,
                Deployments::RuntimeMetadata,
                Deployments::SetupTarget,
                Deployments::SetupFingerprint,
                Deployments::SetupFingerprintVersion,
                Deployments::RetryRequested,
                Deployments::CreatedAt,
            ];
            let mut values: Vec<sea_query::SimpleExpr> = vec![
                id.clone().into(),
                params.name.clone().into(),
                params.deployment_group_id.clone().into(),
                params.platform.as_str().to_string().into(),
                (params.deployment_protocol_version as i64).into(),
                params
                    .base_platform
                    .map(|platform| platform.as_str().to_string())
                    .into(),
                params.status.clone().into(),
                stack_settings_json.into(),
                stack_state_json.into(),
                runtime_metadata_json.into(),
                params.setup_target.clone().into(),
                params.setup_fingerprint.clone().into(),
                (params.setup_fingerprint_version as i64).into(),
                0i64.into(),
                now.to_rfc3339().into(),
            ];

            if let Some(ref release_id) = params.current_release_id {
                columns.push(Deployments::CurrentReleaseId);
                values.push(release_id.clone().into());
            }
            if let Some(ref release_id) = params.desired_release_id {
                columns.push(Deployments::DesiredReleaseId);
                values.push(release_id.clone().into());
            }

            if let Some(ref env_info_json) = environment_info_json {
                columns.push(Deployments::EnvironmentInfo);
                values.push(env_info_json.clone().into());
            }

            if let Some(ref import_source) = params.import_source {
                columns.push(Deployments::ImportSource);
                values.push(import_source_to_string(import_source).into());
            }

            if let Some(ref token) = params.deployment_token {
                columns.push(Deployments::DeploymentToken);
                values.push(token.clone().into());
            }

            Query::insert()
                .into_table(Deployments::Table)
                .columns(columns)
                .values(values)
                .map_err(|e| {
                    db_error(&format!(
                        "Failed to build imported deployment insert query: {}",
                        e
                    ))
                })?
                .to_string(SqliteQueryBuilder)
        };

        self.db.execute(&sql).await?;

        Ok(DeploymentRecord {
            id,
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: params.name,
            deployment_group_id: params.deployment_group_id,
            platform: params.platform,
            deployment_protocol_version: params.deployment_protocol_version,
            base_platform: params.base_platform,
            status: params.status,
            stack_settings: Some(params.stack_settings),
            stack_state: Some(params.stack_state),
            environment_info: params.environment_info,
            runtime_metadata: Some(params.runtime_metadata),
            current_release_id: params.current_release_id,
            desired_release_id: params.desired_release_id,
            import_source: params.import_source.clone(),
            setup_method: params.import_source.as_ref().and_then(|source| {
                serde_json::to_value(source)
                    .ok()
                    .and_then(|value| value.as_str().map(ToString::to_string))
            }),
            setup_metadata: params.setup_metadata,
            setup_target: Some(params.setup_target),
            setup_fingerprint: Some(params.setup_fingerprint),
            setup_fingerprint_version: Some(params.setup_fingerprint_version),
            user_environment_variables: None,
            deployment_token: params.deployment_token,
            management_config: params.management_config,
            deployment_config: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: now,
            updated_at: None,
            error: None,
        })
    }

    async fn update_imported_stack_state(
        &self,
        caller: &crate::auth::Subject,
        deployment_id: &str,
        stack_state: StackState,
        environment_info: Option<EnvironmentInfo>,
        runtime_metadata: RuntimeMetadata,
        current_release_id: Option<String>,
        setup_target: String,
        setup_fingerprint: String,
        setup_fingerprint_version: u32,
    ) -> Result<DeploymentRecord, AlienError> {
        let mut merged_stack_state = stack_state;
        if let Some(existing) = self.get_deployment(caller, deployment_id).await? {
            if let Some(mut existing_stack_state) = existing.stack_state {
                for (resource_id, resource_state) in merged_stack_state.resources {
                    existing_stack_state
                        .resources
                        .insert(resource_id, resource_state);
                }
                merged_stack_state = existing_stack_state;
            }
        }

        let stack_state_json = serde_json::to_string(&merged_stack_state)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize imported stack_state".to_string(),
            })?;
        let runtime_metadata_json = serde_json::to_string(&runtime_metadata)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize imported runtime_metadata".to_string(),
            })?;
        let environment_info_json: Option<String> = environment_info
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize imported environment_info".to_string(),
            })?;

        let now = Utc::now();

        let sql = {
            let mut update = Query::update();
            update
                .table(Deployments::Table)
                .value(Deployments::StackState, stack_state_json)
                .value(Deployments::EnvironmentInfo, environment_info_json)
                .value(Deployments::RuntimeMetadata, runtime_metadata_json)
                .value(Deployments::SetupTarget, setup_target)
                .value(Deployments::SetupFingerprint, setup_fingerprint)
                .value(
                    Deployments::SetupFingerprintVersion,
                    setup_fingerprint_version as i64,
                )
                .value(Deployments::UpdatedAt, now.to_rfc3339())
                .and_where(Expr::col(Deployments::Id).eq(deployment_id));

            if let Some(release_id) = current_release_id {
                update.value(Deployments::CurrentReleaseId, release_id);
            }

            update.to_string(SqliteQueryBuilder)
        };
        self.db.execute(&sql).await?;

        self.get_deployment(caller, deployment_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(GenericError {
                    message: format!(
                        "Imported deployment '{deployment_id}' disappeared mid-update — race?"
                    ),
                })
            })
    }

    async fn get_deployment(
        &self,
        _caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<Option<DeploymentRecord>, AlienError> {
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

    async fn get_deployment_by_name(
        &self,
        _caller: &crate::auth::Subject,
        deployment_group_id: &str,
        name: &str,
    ) -> Result<Option<DeploymentRecord>, AlienError> {
        let sql = Query::select()
            .columns(Self::DEPLOYMENT_COLUMNS)
            .from(Deployments::Table)
            .and_where(Expr::col(Deployments::DeploymentGroupId).eq(deployment_group_id))
            .and_where(Expr::col(Deployments::Name).eq(name))
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to query deployment by name".to_string(),
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
        _caller: &crate::auth::Subject,
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
            if let Some(name) = &filter.name {
                query.and_where(Expr::col(Deployments::Name).eq(name.as_str()));
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

    async fn delete_deployment(
        &self,
        _caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<(), AlienError> {
        let sql = Query::delete()
            .from_table(Deployments::Table)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await
    }

    async fn set_delete_pending(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<(), AlienError> {
        let deployment = self
            .get_deployment(caller, id)
            .await?
            .ok_or_else(|| db_error(&format!("Deployment {} not found", id)))?;

        if deployment.status == "teardown-required" || deployment.status == "teardown-failed" {
            return Ok(());
        }

        let rejection_statuses = ["delete-pending", "deleting", "deleted"];
        if rejection_statuses.contains(&deployment.status.as_str()) {
            return Err(AlienError::new(ErrorData::DeploymentAlreadyDeleting {
                deployment_id: id.to_string(),
                status: deployment.status,
            })
            .into_generic());
        }

        // FUTURE: reject or queue delete requests when another session currently owns the deployment lock.
        let runtime_metadata = deployment.runtime_metadata.unwrap_or_default();
        let runtime_metadata_json = serde_json::to_string(&runtime_metadata)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize delete runtime metadata".to_string(),
            })?;

        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::Status, "delete-pending")
            .value(Deployments::RuntimeMetadata, runtime_metadata_json)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await
    }

    async fn set_retry_requested(
        &self,
        _caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<(), AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::RetryRequested, 1i64)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await
    }

    async fn set_redeploy(
        &self,
        _caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<(), AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::Status, "update-pending")
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await
    }

    async fn set_deployment_desired_release(
        &self,
        _caller: &crate::auth::Subject,
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
        _caller: &crate::auth::Subject,
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
        _caller: &crate::auth::Subject,
        session: &str,
        filter: &DeploymentFilter,
        limit: u32,
    ) -> Result<Vec<AcquiredDeployment>, AlienError> {
        let now = Utc::now();

        // Stale lock threshold: 5 minutes. If a manager crashed mid-processing,
        // the lock will self-heal after this period.
        let stale_lock_condition = Self::stale_lock_condition_sql();
        let explicit_status_filter = filter
            .statuses
            .as_ref()
            .filter(|statuses| !statuses.is_empty());

        // SELECT deployments that need work AND are either unlocked or stale-locked
        let select_sql = {
            let mut query = Query::select();
            query
                .columns(Self::DEPLOYMENT_COLUMNS)
                .from(Deployments::Table);

            query.cond_where(Self::acquire_status_condition(explicit_status_filter));

            query
                // Unlocked OR stale-locked (locked_at older than 5 minutes)
                .cond_where(
                    sea_query::Cond::any()
                        .add(Expr::col(Deployments::LockedBy).is_null())
                        .add(Expr::cust(stale_lock_condition.clone())),
                );

            if let Some(dg_id) = &filter.deployment_group_id {
                query.and_where(Expr::col(Deployments::DeploymentGroupId).eq(dg_id.as_str()));
            }
            if let Some(name) = &filter.name {
                query.and_where(Expr::col(Deployments::Name).eq(name.as_str()));
            }
            if let Some(ids) = &filter.deployment_ids {
                let id_strs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
                query.and_where(Expr::col(Deployments::Id).is_in(id_strs));
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

        // Must drop rows and conn before calling self.db methods
        drop(rows);
        drop(conn);

        // Lock each acquired deployment, checking rows_affected to avoid phantom locks
        let mut acquired = Vec::new();
        for dep in deployments {
            // Atomically lock: only succeeds if still unlocked or stale-locked
            let lock_sql = {
                let mut query = Query::update();
                query
                    .table(Deployments::Table)
                    .value(Deployments::LockedBy, session)
                    .value(Deployments::LockedAt, now.to_rfc3339())
                    .value(Deployments::RetryRequested, 0i64)
                    .and_where(Expr::col(Deployments::Id).eq(dep.id.as_str()));

                query.cond_where(Self::acquire_status_condition(explicit_status_filter));

                query
                    .cond_where(
                        sea_query::Cond::any()
                            .add(Expr::col(Deployments::LockedBy).is_null())
                            .add(Expr::cust(stale_lock_condition.clone())),
                    )
                    .to_string(SqliteQueryBuilder)
            };

            if dep.locked_by.is_some() {
                warn!(
                    deployment_id = %dep.id,
                    previous_session = %dep.locked_by.as_deref().unwrap_or("unknown"),
                    "Breaking stale lock on deployment"
                );
            }

            let rows_affected = self.db.execute_returning_rows_affected(&lock_sql).await?;

            // Only count as acquired if our UPDATE actually modified a row.
            // Another caller may have locked it between our SELECT and UPDATE.
            if rows_affected > 0 {
                acquired.push(AcquiredDeployment {
                    deployment: DeploymentRecord {
                        locked_by: Some(session.to_string()),
                        locked_at: Some(now),
                        ..dep
                    },
                });
            }
        }

        Ok(acquired)
    }

    async fn reconcile(
        &self,
        caller: &crate::auth::Subject,
        data: ReconcileData,
    ) -> Result<DeploymentRecord, AlienError> {
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
        let deployment = self.get_deployment(caller, &data.deployment_id).await?;
        let deployment_completed = deployment
            .as_ref()
            .and_then(|d| d.desired_release_id.as_ref())
            .and_then(|desired| {
                current_release_id
                    .as_ref()
                    .map(|current| current == desired)
            })
            .unwrap_or(false);
        let preserve_retry_requested = deployment
            .as_ref()
            .is_some_and(|d| Self::should_preserve_retry_requested(d, state));
        let retry_requested = state.retry_requested || preserve_retry_requested;

        let headline_error = alien_deployment::deployment_headline_error_from_state(state);
        let error_json: Option<String> = headline_error
            .as_ref()
            .map(|e| serde_json::to_string(e))
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize error".to_string(),
            })?;

        // Build UPDATE using sea_query for parameterized, injection-safe SQL
        let sql = {
            let mut query = Query::update();
            query
                .table(Deployments::Table)
                .value(Deployments::Status, &status_str as &str)
                .value(
                    Deployments::DeploymentProtocolVersion,
                    state.protocol_version as i64,
                )
                .value(
                    Deployments::RetryRequested,
                    if retry_requested { 1i64 } else { 0i64 },
                )
                .value(Deployments::UpdatedAt, now.to_rfc3339());

            // Nullable fields: set value or explicit NULL to clear stale data
            match &stack_state_json {
                Some(v) => query.value(Deployments::StackState, v as &str),
                None => query.value(Deployments::StackState, Option::<String>::None),
            };
            match &environment_info_json {
                Some(v) => query.value(Deployments::EnvironmentInfo, v as &str),
                None => query.value(Deployments::EnvironmentInfo, Option::<String>::None),
            };
            match &runtime_metadata_json {
                Some(v) => query.value(Deployments::RuntimeMetadata, v as &str),
                None => query.value(Deployments::RuntimeMetadata, Option::<String>::None),
            };
            match &current_release_id {
                Some(v) => query.value(Deployments::CurrentReleaseId, v as &str),
                None => query.value(Deployments::CurrentReleaseId, Option::<String>::None),
            };
            match &error_json {
                Some(v) => query.value(Deployments::Error, v as &str),
                None => query.value(Deployments::Error, Option::<String>::None),
            };

            // Clear desired_release_id when deployment completed (current matches desired)
            if deployment_completed {
                query.value(Deployments::DesiredReleaseId, Option::<String>::None);
            } else if let Some(desired) = deployment
                .as_ref()
                .and_then(|d| d.desired_release_id.as_ref())
            {
                query.value(Deployments::DesiredReleaseId, desired as &str);
            } else {
                query.value(Deployments::DesiredReleaseId, Option::<String>::None);
            }

            // Match by `Id` only. Authz at the route layer (`can_sync_deployment`)
            // gates who may call `reconcile`; race protection between competing
            // legitimate writers lives at the `acquire` lock. A `LockedBy = session`
            // WHERE clause adds no additional security guarantee here, and breaks
            // pull-mode `agent_sync` which writes state without holding the lock.
            query
                .and_where(Expr::col(Deployments::Id).eq(&data.deployment_id as &str))
                .to_string(SqliteQueryBuilder)
        };

        self.db.execute(&sql).await?;

        // Keep the active lease alive while a caller is making progress.
        // Long cloud waits can legitimately exceed the stale-lock window; a
        // reconcile from the lock owner is the durable progress signal.
        let lock_heartbeat_sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::LockedAt, now.to_rfc3339())
            .and_where(Expr::col(Deployments::Id).eq(&data.deployment_id as &str))
            .and_where(Expr::col(Deployments::LockedBy).eq(&data.session as &str))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&lock_heartbeat_sql).await?;

        // Fetch and return the updated deployment
        self.get_deployment(caller, &data.deployment_id)
            .await?
            .ok_or_else(|| db_error("Deployment not found after reconcile"))
    }

    async fn release(
        &self,
        _caller: &crate::auth::Subject,
        deployment_id: &str,
        session: &str,
    ) -> Result<(), AlienError> {
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
        _caller: &crate::auth::Subject,
        params: CreateDeploymentGroupParams,
    ) -> Result<DeploymentGroupRecord, AlienError> {
        let id = alien_core::new_id(alien_core::IdType::DeploymentGroup);
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
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: params.name,
            max_deployments: params.max_deployments,
            deployment_count: 0,
            created_at: now,
        })
    }

    async fn create_deployment_group_with_id(
        &self,
        _caller: &crate::auth::Subject,
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
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: params.name,
            max_deployments: params.max_deployments,
            deployment_count: 0,
            created_at: now,
        })
    }

    async fn get_deployment_group(
        &self,
        _caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<Option<DeploymentGroupRecord>, AlienError> {
        // Compute deployment_count via LEFT JOIN instead of stored column
        // to guarantee consistency even after crashes.
        let sql = Query::select()
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::Id)),
                sea_query::Alias::new("id"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::Name)),
                sea_query::Alias::new("name"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::MaxDeployments)),
                sea_query::Alias::new("max_deployments"),
            )
            .expr_as(
                Expr::cust("COUNT(\"deployments\".\"id\")"),
                sea_query::Alias::new("deployment_count"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::CreatedAt)),
                sea_query::Alias::new("created_at"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::WorkspaceId)),
                sea_query::Alias::new("workspace_id"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::ProjectId)),
                sea_query::Alias::new("project_id"),
            )
            .from(DeploymentGroups::Table)
            .join(
                sea_query::JoinType::LeftJoin,
                Deployments::Table,
                Expr::col((Deployments::Table, Deployments::DeploymentGroupId))
                    .equals((DeploymentGroups::Table, DeploymentGroups::Id)),
            )
            .and_where(Expr::col((DeploymentGroups::Table, DeploymentGroups::Id)).eq(id))
            .group_by_col((DeploymentGroups::Table, DeploymentGroups::Id))
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

    async fn cleanup_stale_locks(&self, _caller: &crate::auth::Subject) -> Result<u64, AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::LockedBy, Option::<String>::None)
            .value(Deployments::LockedAt, Option::<String>::None)
            .and_where(Expr::col(Deployments::LockedBy).is_not_null())
            .to_string(SqliteQueryBuilder);
        let rows = self.db.execute_returning_rows_affected(&sql).await?;
        if rows > 0 {
            warn!(
                count = rows,
                "Cleaned up stale deployment locks from previous session"
            );
        }
        Ok(rows)
    }

    async fn list_deployment_groups(
        &self,
        _caller: &crate::auth::Subject,
    ) -> Result<Vec<DeploymentGroupRecord>, AlienError> {
        // Compute deployment_count via LEFT JOIN for each group
        let sql = Query::select()
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::Id)),
                sea_query::Alias::new("id"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::Name)),
                sea_query::Alias::new("name"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::MaxDeployments)),
                sea_query::Alias::new("max_deployments"),
            )
            .expr_as(
                Expr::cust("COUNT(\"deployments\".\"id\")"),
                sea_query::Alias::new("deployment_count"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::CreatedAt)),
                sea_query::Alias::new("created_at"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::WorkspaceId)),
                sea_query::Alias::new("workspace_id"),
            )
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::ProjectId)),
                sea_query::Alias::new("project_id"),
            )
            .from(DeploymentGroups::Table)
            .join(
                sea_query::JoinType::LeftJoin,
                Deployments::Table,
                Expr::col((Deployments::Table, Deployments::DeploymentGroupId))
                    .equals((DeploymentGroups::Table, DeploymentGroups::Id)),
            )
            .group_by_col((DeploymentGroups::Table, DeploymentGroups::Id))
            .order_by(
                (DeploymentGroups::Table, DeploymentGroups::CreatedAt),
                Order::Desc,
            )
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
