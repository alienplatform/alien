//! SQLite implementation of DeploymentStore.

use async_trait::async_trait;
use chrono::{Duration, Utc};
use sea_query::{Cond, Expr, IntoCondition, Order, Query, SqliteQueryBuilder};
use std::{collections::HashSet, sync::Arc};
use tracing::warn;
use turso::transaction::TransactionBehavior;

use alien_core::{
    import::ImportSourceKind, DeploymentModel, DeploymentStatus, EnvironmentVariable, Platform,
};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};

use super::database::{db_error, RowParser, SqliteDatabase};
use super::migrations::{DeploymentGroups, Deployments};
use crate::error::ErrorData;
use crate::ids;
use crate::traits::deployment_store::*;

/// Setup JSON for the `deployment_groups.setup` column; NULL when empty.
fn setup_json(setup: &DeploymentGroupSetup) -> Result<Option<String>, AlienError> {
    if setup.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(setup)
        .map(Some)
        .into_alien_error()
        .context(GenericError {
            message: "Failed to serialize deployment group setup".to_string(),
        })
}

fn import_source_to_string(source: &ImportSourceKind) -> String {
    serde_json::to_value(source)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| format!("{source:?}").to_ascii_lowercase())
}

pub struct SqliteDeploymentStore {
    db: Arc<SqliteDatabase>,
}

/// A setup reservation whose installer has not registered yet. The manager has
/// installed nothing for it; the installer owns any cloud resources until then.
const UNREGISTERED_SETUP_RESERVATION: &str = "status = 'pending' AND current_release_id IS NULL AND json_extract(COALESCE(setup_metadata, '{}'), '$.setupReservation.releaseId') IS NOT NULL";

impl SqliteDeploymentStore {
    pub(crate) const WORK_STATUSES: [&'static str; 9] = [
        "pending",
        "initial-setup",
        "provisioning",
        "waiting-for-secrets",
        "waiting-for-machines",
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
    const SETUP_TEARDOWN_STATUSES: [&'static str; 3] =
        ["retention-required", "teardown-required", "teardown-failed"];
    const RUNTIME_DELETE_STATUSES: [&'static str; 3] =
        ["delete-pending", "deleting", "delete-failed"];
    const RUNNING_STATUS: &'static str = "running";

    async fn insert_deployment(
        &self,
        caller: &crate::auth::Subject,
        params: CreateDeploymentParams,
        reservation: Option<SetupReservation>,
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

        let setup_metadata = reservation.as_ref().map(|reservation| serde_json::json!({
            "setupReservation": { "releaseId": reservation.release_id, "createdAt": now.to_rfc3339() }
        }));
        let setup_metadata_json = setup_metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize setup reservation".to_string(),
            })?;
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

        let input_values_json = serde_json::to_string(&params.input_values)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize input_values".to_string(),
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
                Deployments::InputValues,
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
                input_values_json.into(),
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

            if let Some(base_platform) = params.base_platform {
                columns.push(Deployments::BasePlatform);
                values.push(base_platform.as_str().to_string().into());
            }
            if let Some(reservation) = &reservation {
                columns.extend([
                    Deployments::ImportSource,
                    Deployments::SetupMetadata,
                    Deployments::SetupTarget,
                    Deployments::SetupFingerprintVersion,
                ]);
                let source = import_source_to_string(&reservation.import_source);
                values.extend([
                    source.into(),
                    setup_metadata_json.clone().into(),
                    reservation.setup_target.clone().into(),
                    i64::from(reservation.setup_fingerprint_version).into(),
                ]);
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
            supplied_stacks: None,
            id,
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: params.name,
            deployment_group_id: params.deployment_group_id,
            platform: params.platform,
            deployment_protocol_version: params.deployment_protocol_version,
            base_platform: params.base_platform,
            status: "pending".to_string(),
            stack_settings: Some(params.stack_settings),
            stack_state: params.stack_state,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            import_source: reservation
                .as_ref()
                .map(|value| value.import_source.clone()),
            setup_method: reservation
                .as_ref()
                .map(|value| import_source_to_string(&value.import_source)),
            setup_metadata,
            setup_target: reservation.as_ref().map(|value| value.setup_target.clone()),
            setup_fingerprint: None,
            setup_fingerprint_version: reservation
                .as_ref()
                .map(|value| value.setup_fingerprint_version),
            user_environment_variables: params.environment_variables,
            deployment_token: params.deployment_token,
            input_values: params.input_values,
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

    pub fn new(db: Arc<SqliteDatabase>) -> Self {
        Self { db }
    }

    /// Release Local push execution claims after the caller exclusively owns the stopped database.
    /// Never call while a manager can still execute work against this database.
    pub async fn recover_local_execution_claims(&self) -> Result<u64, AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::LockedBy, Option::<String>::None)
            .value(Deployments::LockedAt, Option::<String>::None)
            .and_where(Expr::col(Deployments::Platform).eq("local"))
            .cond_where(Self::deployment_model_condition(DeploymentModel::Push))
            .and_where(Expr::col(Deployments::LockedBy).is_not_null())
            .to_string(SqliteQueryBuilder);
        self.db.execute_returning_rows_affected(&sql).await
    }

    /// Replace user environment variables without deleting the deployment or its resources.
    /// Callers must own the standalone database and stop its execution loop first.
    pub async fn replace_environment_variables(
        &self,
        id: &str,
        variables: &[EnvironmentVariable],
    ) -> Result<(), AlienError> {
        let json = serde_json::to_string(variables)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize deployment environment variables".to_string(),
            })?;
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::EnvironmentVariables, json)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        self.db.execute(&sql).await
    }

    /// Move an explicitly selected local deployment without rewriting resource state.
    /// The caller must exclusively own the stopped database during migration.
    pub async fn reassign_local_deployment_group(
        &self,
        id: &str,
        group_id: &str,
    ) -> Result<(), AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::DeploymentGroupId, group_id)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .and_where(Expr::col(Deployments::Platform).eq("local"))
            .to_string(SqliteQueryBuilder);
        let affected = self.db.execute_returning_rows_affected(&sql).await?;
        if affected != 1 {
            return Err(AlienError::new(GenericError {
                message: format!("Expected one local deployment to migrate; updated {affected}"),
            }));
        }
        Ok(())
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

    fn is_delete_progress(status: DeploymentStatus) -> bool {
        matches!(
            status,
            DeploymentStatus::DeletePending
                | DeploymentStatus::Deleting
                | DeploymentStatus::DeleteFailed
                | DeploymentStatus::TeardownRequired
                | DeploymentStatus::RetentionRequired
                | DeploymentStatus::Retained
                | DeploymentStatus::TeardownFailed
                | DeploymentStatus::Deleted
        )
    }

    fn acquire_status_condition(statuses: Option<&Vec<String>>) -> sea_query::Condition {
        Cond::all()
            .add(Expr::cust(format!(
                "NOT ({UNREGISTERED_SETUP_RESERVATION})"
            )))
            .add(Self::unreserved_acquire_status_condition(statuses))
    }

    fn unreserved_acquire_status_condition(statuses: Option<&Vec<String>>) -> sea_query::Condition {
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
                        // The heartbeat loop requests Running alongside RefreshFailed.
                        // Work-only acquisition must still require an explicit retry.
                        || (*status == "refresh-failed"
                            && statuses.iter().any(|status| status == Self::RUNNING_STATUS))
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

    fn acquire_status_matches(
        status: &str,
        retry_requested: bool,
        statuses: Option<&Vec<String>>,
    ) -> bool {
        let failed_is_retryable = Self::FAILED_STATUSES.contains(&status) && retry_requested;

        let Some(statuses) = statuses else {
            return Self::WORK_STATUSES.contains(&status) || failed_is_retryable;
        };

        let explicitly_active = statuses.iter().any(|requested| {
            requested == status
                && (Self::WORK_STATUSES.contains(&status)
                    || Self::SETUP_TEARDOWN_STATUSES.contains(&status)
                    || status == Self::RUNNING_STATUS
                    || (status == "refresh-failed"
                        && statuses
                            .iter()
                            .any(|candidate| candidate == Self::RUNNING_STATUS)))
        });
        let explicitly_failed = statuses.iter().any(|requested| requested == status)
            && Self::FAILED_STATUSES.contains(&status)
            && retry_requested;

        explicitly_active || explicitly_failed
    }

    fn deployment_model_condition(model: DeploymentModel) -> sea_query::Condition {
        match model {
            DeploymentModel::Push => Expr::cust(
                "json_extract(stack_settings, '$.deploymentModel') IS NULL OR json_extract(stack_settings, '$.deploymentModel') = 'push'",
            )
            .into_condition(),
            DeploymentModel::Pull => {
                Expr::cust("json_extract(stack_settings, '$.deploymentModel') = 'pull'")
                    .into_condition()
            }
        }
    }

    fn stale_lock_condition_sql() -> String {
        "\"locked_at\" IS NOT NULL AND julianday(\"locked_at\") < julianday('now', '-5 minutes')"
            .to_string()
    }

    /// All columns needed for deployment queries (must match parse_deployment order).
    const DEPLOYMENT_COLUMNS: [Deployments; 29] = [
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
        Deployments::SetupMetadata,
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
        Deployments::InputValues,
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
            p.optional_json(18, "environment_variables")?;

        let retry_requested_int: i64 = p.optional_i64(20, "retry_requested")?.unwrap_or(0);

        Ok(DeploymentRecord {
            supplied_stacks: None,
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
            setup_metadata: p.optional_json(14, "setup_metadata")?,
            setup_target: p.optional_string(15, "setup_target")?,
            setup_fingerprint: p.optional_string(16, "setup_fingerprint")?,
            setup_fingerprint_version: p
                .optional_i64(17, "setup_fingerprint_version")?
                .map(|value| value as u32),
            user_environment_variables,
            deployment_token: p.optional_string(19, "deployment_token")?,
            management_config: None,
            deployment_config: None,
            retry_requested: retry_requested_int != 0,
            locked_by: p.optional_string(21, "locked_by")?,
            locked_at: p.optional_datetime(22, "locked_at")?,
            created_at: p.datetime(23, "created_at")?,
            updated_at: p.optional_datetime(24, "updated_at")?,
            error: p.optional_json(25, "error")?,
            workspace_id: p
                .optional_string(26, "workspace_id")?
                .unwrap_or_else(|| "default".to_string()),
            project_id: p
                .optional_string(27, "project_id")?
                .unwrap_or_else(|| "default".to_string()),
            input_values: p.optional_json(28, "input_values")?.unwrap_or_default(),
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
            setup: match p.optional_string(7, "setup")? {
                Some(json) => {
                    serde_json::from_str(&json)
                        .into_alien_error()
                        .context(GenericError {
                            message: "Failed to parse deployment group setup".to_string(),
                        })?
                }
                None => Default::default(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use alien_core::{
        DeploymentState, DeploymentStatus, EnvironmentVariable, EnvironmentVariableType, Platform,
        StackSettings, CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
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
            supplied_stacks: None,
            id: "dep_test".to_string(),
            input_values: Default::default(),
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

    #[test]
    fn deployment_debug_redacts_sensitive_values() {
        let mut deployment = deployment_record("running");
        deployment.input_values.insert(
            "apiToken".to_string(),
            serde_json::json!("sentinel-secret-value"),
        );
        deployment.user_environment_variables = Some(vec![EnvironmentVariable {
            name: "SECRET_TOKEN".to_string(),
            value: "sentinel-environment-secret".to_string(),
            var_type: EnvironmentVariableType::Secret,
            target_resources: None,
        }]);

        let debug = format!("{deployment:?}");

        assert!(debug.contains("input_values: \"[REDACTED]\""));
        assert!(!debug.contains("apiToken"));
        assert!(!debug.contains("sentinel-secret-value"));
        assert!(!debug.contains("SECRET_TOKEN"));
        assert!(!debug.contains("sentinel-environment-secret"));
    }
}

#[async_trait]
impl DeploymentStore for SqliteDeploymentStore {
    fn supports_acquire_exclusions(&self) -> bool {
        true
    }

    async fn create_deployment(
        &self,
        caller: &crate::auth::Subject,
        params: CreateDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError> {
        self.insert_deployment(caller, params, None).await
    }

    async fn create_setup_reservation(
        &self,
        caller: &crate::auth::Subject,
        params: CreateDeploymentParams,
        reservation: SetupReservation,
    ) -> Result<DeploymentRecord, AlienError> {
        self.insert_deployment(caller, params, Some(reservation))
            .await
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

        let input_values_json = serde_json::to_string(&params.input_values)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize input_values".to_string(),
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
                Deployments::SetupMetadata,
                Deployments::SetupTarget,
                Deployments::SetupFingerprint,
                Deployments::SetupFingerprintVersion,
                Deployments::RetryRequested,
                Deployments::CreatedAt,
                Deployments::InputValues,
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
                params
                    .setup_metadata
                    .as_ref()
                    .map(serde_json::Value::to_string)
                    .into(),
                params.setup_target.clone().into(),
                params.setup_fingerprint.clone().into(),
                (params.setup_fingerprint_version as i64).into(),
                0i64.into(),
                now.to_rfc3339().into(),
                input_values_json.into(),
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
            supplied_stacks: None,
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
            input_values: params.input_values,
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
        params: UpdateImportedDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError> {
        let UpdateImportedDeploymentParams {
            stack_settings,
            stack_state,
            environment_info,
            runtime_metadata,
            setup_metadata,
            current_release_id,
            setup_target,
            setup_fingerprint,
            setup_fingerprint_version,
            activation_status,
            schedule_reconciliation,
            input_values,
        } = params;
        // The import route supplies a complete ownership-aware state: freshly
        // imported setup resources plus preserved Live controller state.
        // Re-merging here would resurrect setup-owned resources that the new
        // setup payload intentionally omitted.
        let stack_state_json = serde_json::to_string(&stack_state)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize imported stack_state".to_string(),
            })?;
        let stack_settings_json = serde_json::to_string(&stack_settings)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize imported stack_settings".to_string(),
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

        // The re-import request carries the deployer's current answers, so it
        // overwrites the stored map — that's the edit surface for gate flips.
        let input_values_json = serde_json::to_string(&input_values)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize input_values".to_string(),
            })?;

        let now = Utc::now();

        let sql = {
            let mut update = Query::update();
            update
                .table(Deployments::Table)
                .value(Deployments::StackSettings, stack_settings_json)
                .value(Deployments::StackState, stack_state_json)
                .value(Deployments::EnvironmentInfo, environment_info_json)
                .value(Deployments::RuntimeMetadata, runtime_metadata_json)
                .value(
                    Deployments::SetupMetadata,
                    setup_metadata.map(|metadata| metadata.to_string()),
                )
                .value(Deployments::InputValues, input_values_json)
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
            if let Some(status) = activation_status {
                update.value(Deployments::Status, status);
            }
            if schedule_reconciliation {
                update
                    .value(Deployments::Status, "update-pending")
                    .value(Deployments::NextStepAfter, Option::<String>::None);
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
            if !filter.excluded_deployment_ids.is_empty() {
                query.and_where(
                    Expr::col(Deployments::Id)
                        .is_not_in(filter.excluded_deployment_ids.iter().map(String::as_str)),
                );
            }
            if let Some(max_version) = filter.max_deployment_protocol_version {
                query.and_where(Expr::col(Deployments::DeploymentProtocolVersion).lte(max_version));
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
        // Hold the database connection across inspection and deletion so a
        // registration/reconcile cannot install data between these operations.
        let conn = self.db.conn().lock().await;
        let query = Query::select()
            .columns(Self::DEPLOYMENT_COLUMNS)
            .from(Deployments::Table)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        let mut rows = conn
            .query(&query, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to inspect deployment recovery inventory".to_string(),
            })?;
        if let Some(row) = rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to read deployment recovery inventory".to_string(),
        })? {
            let deployment = Self::parse_deployment(&row)?;
            if deployment.locked_by.is_some() && deployment.status != "deleted" {
                return Err(AlienError::new(ErrorData::DeploymentLocked {
                    deployment_id: id.to_string(),
                    locked_by: deployment.locked_by,
                })
                .into_generic());
            }
            if matches!(
                deployment.status.as_str(),
                "retention-required" | "retained"
            ) || deployment.stack_state.as_ref().is_some_and(|stack| {
                !alien_deployment::installed_durable_resource_ids(stack).is_empty()
            }) {
                return Err(ErrorData::bad_request(
                    "Recovery metadata is required by retained durable data; complete explicit erase before forgetting this deployment",
                ).into_generic());
            }
        }
        drop(rows);
        let sql = Query::delete()
            .from_table(Deployments::Table)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .to_string(SqliteQueryBuilder);
        conn.execute(&sql, ())
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to remove deployment metadata".to_string(),
            })?;
        Ok(())
    }

    async fn set_delete_pending_with_policy(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
        policy: alien_core::DeletionPolicy,
    ) -> Result<(), AlienError> {
        let deployment = self
            .get_deployment(caller, id)
            .await?
            .ok_or_else(|| db_error(&format!("Deployment {} not found", id)))?;

        let installed_policy = deployment
            .runtime_metadata
            .as_ref()
            .map(|metadata| metadata.deletion_policy)
            .unwrap_or_default();
        let handoff = matches!(
            deployment.status.as_str(),
            "teardown-required" | "teardown-failed" | "retention-required" | "retained"
        );
        let legacy_retained_handoff = matches!(
            deployment.status.as_str(),
            "teardown-required" | "teardown-failed"
        ) && installed_policy == alien_core::DeletionPolicy::Retain;
        if handoff
            && installed_policy == policy
            && !legacy_retained_handoff
            && !(deployment.status == "retained" && policy == alien_core::DeletionPolicy::Erase)
        {
            return Ok(());
        }
        if matches!(
            deployment.status.as_str(),
            "delete-pending" | "deleting" | "deleted"
        ) || (deployment.status == "delete-failed" && installed_policy != policy)
            || (handoff && policy != alien_core::DeletionPolicy::Erase && !legacy_retained_handoff)
        {
            return Err(AlienError::new(ErrorData::DeploymentAlreadyDeleting {
                deployment_id: id.to_string(),
                status: deployment.status,
            })
            .into_generic());
        }

        // Nothing to clean up for an unregistered reservation, so remove it. The
        // condition is rechecked in the delete, so a registration that activated
        // it in the meantime keeps it and it takes the normal cleanup path.
        let sql = Query::delete()
            .from_table(Deployments::Table)
            .and_where(Expr::col(Deployments::Id).eq(id))
            .and_where(Expr::cust(UNREGISTERED_SETUP_RESERVATION))
            .and_where(Expr::col(Deployments::LockedBy).is_null())
            .to_string(SqliteQueryBuilder);
        if self.db.execute_returning_rows_affected(&sql).await? > 0 {
            return Ok(());
        }

        // Runtime steps keep their lease and checkpoint against the recorded policy.
        // Setup cleanup must finish before its authorized policy can change. A
        // completed setup checkpoint also keeps its lease until explicit release.
        let requires_setup_release = Self::SETUP_TEARDOWN_STATUSES
            .contains(&deployment.status.as_str())
            || deployment.status == "retained";
        // Patch only the intent and fence concurrent requests.
        // Retained checkpoints remain available for a separate explicit erase.
        let policy_json = match policy {
            alien_core::DeletionPolicy::Retain => "retain",
            alien_core::DeletionPolicy::Erase => "erase",
        };
        // Retention removed the cloud management identity. A later erase must
        // use the setup authority's credentials, including for retained Live data.
        let next_status = if deployment.status == "retained"
            && policy == alien_core::DeletionPolicy::Erase
            && matches!(
                deployment.platform,
                alien_core::Platform::Aws | alien_core::Platform::Gcp | alien_core::Platform::Azure
            ) {
            "teardown-required"
        } else {
            "delete-pending"
        };
        let sql = {
            let mut update = Query::update();
            update
                .table(Deployments::Table)
                .value(Deployments::Status, next_status)
                .value(
                    Deployments::RuntimeMetadata,
                    Expr::cust_with_values(
                        "json_set(COALESCE(runtime_metadata, '{}'), '$.deletionPolicy', ?)",
                        [policy_json],
                    ),
                )
                .value(
                    Deployments::DeploymentProtocolVersion,
                    alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION as i64,
                )
                .value(Deployments::NextStepAfter, Option::<String>::None)
                .and_where(Expr::col(Deployments::Id).eq(id))
                .and_where(Expr::col(Deployments::Status).eq(deployment.status.clone()));
            if requires_setup_release {
                update.and_where(Expr::col(Deployments::LockedBy).is_null());
            }
            update.to_string(SqliteQueryBuilder)
        };
        if self.db.execute_returning_rows_affected(&sql).await? != 1 {
            if requires_setup_release {
                let current = self.get_deployment(caller, id).await?;
                if current.is_some_and(|current| {
                    current.status == deployment.status && current.locked_by.is_some()
                }) {
                    return Err(
                        AlienError::new(ErrorData::DeploymentSetupCleanupInProgress {
                            deployment_id: id.to_string(),
                        })
                        .into_generic(),
                    );
                }
            }
            return Err(ErrorData::bad_request(
                "Deployment changed; retry deletion against its current state",
            )
            .into_generic());
        }
        Ok(())
    }

    async fn set_quota_warning_overrides(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
        overrides: &[String],
    ) -> Result<bool, AlienError> {
        let Some(deployment) = self.get_deployment(caller, id).await? else {
            return Ok(false);
        };
        alien_preflights::quotas::validate_warning_overrides(deployment.platform, overrides)
            .map_err(AlienError::into_generic)?;
        let json = serde_json::to_string(overrides)
            .into_alien_error()
            .context(GenericError {
                message: "Failed to serialize quota warning overrides".to_string(),
            })?;

        // Patch the current JSON in the same conditional write that checks the
        // execution lease. A read/replace of StackSettings would lose concurrent
        // changes (including fields this manager version does not understand).
        let sql = Query::update()
            .table(Deployments::Table)
            .value(
                Deployments::StackSettings,
                Expr::cust_with_values(
                    "json_set(stack_settings, '$.quotaWarningOverrides', json(?))",
                    [json],
                ),
            )
            .value(Deployments::UpdatedAt, Utc::now().to_rfc3339())
            .and_where(Expr::col(Deployments::Id).eq(id))
            .and_where(Expr::col(Deployments::Platform).eq(deployment.platform.as_str()))
            .and_where(Expr::col(Deployments::LockedBy).is_null())
            .and_where(Expr::col(Deployments::Status).is_in([
                "running",
                "preflights-failed",
                "update-failed",
            ]))
            .to_string(SqliteQueryBuilder);
        Ok(self.db.execute_returning_rows_affected(&sql).await? == 1)
    }

    async fn set_retry_requested(
        &self,
        _caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<(), AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::RetryRequested, 1i64)
            .value(Deployments::NextStepAfter, Option::<String>::None)
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
            .value(Deployments::NextStepAfter, Option::<String>::None)
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
            .value(Deployments::NextStepAfter, Option::<String>::None)
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
        // A new local session must retry the release it just built, not the failed
        // first release. Keep the failed phase so the explicit retry resumes safely.
        let local_startup_failures = [
            "preflights-failed",
            "initial-setup-failed",
            "provisioning-failed",
        ];

        let sql = {
            let mut query = Query::update();
            query
                .table(Deployments::Table)
                .value(Deployments::DesiredReleaseId, release_id)
                .value(
                    Deployments::Status,
                    Expr::case(
                        sea_query::Cond::all()
                            .add(Expr::col(Deployments::Platform).eq(Platform::Local.as_str()))
                            .add(Expr::col(Deployments::Status).is_in(local_startup_failures)),
                        Expr::col(Deployments::Status),
                    )
                    .finally("update-pending"),
                )
                .value(Deployments::NextStepAfter, Option::<String>::None)
                .cond_where(
                    sea_query::Cond::any()
                        .add(Expr::col(Deployments::Status).is_in(eligible_statuses))
                        // Only the persisted installed grant permits a release to retry
                        // setup work. A requested setting must not authorize manual setup.
                        .add(
                            sea_query::Cond::all()
                                .add(Expr::col(Deployments::Status).eq("initial-setup-failed"))
                                .add(Expr::cust(
                                    "json_extract(runtime_metadata, '$.automaticSetup') = 1",
                                )),
                        )
                        .add(
                            sea_query::Cond::all()
                                .add(Expr::col(Deployments::Platform).eq(Platform::Local.as_str()))
                                .add(Expr::col(Deployments::Status).is_in(local_startup_failures)),
                        ),
                );

            if let Some(p) = platform {
                query.and_where(Expr::col(Deployments::Platform).eq(p.as_str()));
            }

            query.to_string(SqliteQueryBuilder)
        };

        self.db.execute(&sql).await
    }

    async fn acquire(
        &self,
        caller: &crate::auth::Subject,
        session: &str,
        filter: &DeploymentFilter,
        limit: u32,
    ) -> Result<Vec<AcquiredDeployment>, AlienError> {
        Ok(self
            .acquire_with_reasons(caller, session, filter, limit)
            .await?
            .deployments)
    }

    async fn acquire_with_reasons(
        &self,
        _caller: &crate::auth::Subject,
        session: &str,
        filter: &DeploymentFilter,
        limit: u32,
    ) -> Result<DeploymentAcquireResult, AlienError> {
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
            query.cond_where(
                sea_query::Cond::any()
                    .add(Expr::col(Deployments::NextStepAfter).is_null())
                    .add(Expr::col(Deployments::NextStepAfter).lte(now.to_rfc3339())),
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
            if !filter.excluded_deployment_ids.is_empty() {
                query.and_where(
                    Expr::col(Deployments::Id)
                        .is_not_in(filter.excluded_deployment_ids.iter().map(String::as_str)),
                );
            }
            if let Some(max_version) = filter.max_deployment_protocol_version {
                query.and_where(Expr::col(Deployments::DeploymentProtocolVersion).lte(max_version));
                if max_version < alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION {
                    query.and_where(Expr::col(Deployments::Status).is_not_in([
                        "delete-pending",
                        "deleting",
                        "delete-failed",
                        "teardown-required",
                        "teardown-failed",
                        "retention-required",
                        "retained",
                    ]));
                }
            }
            if let Some(platforms) = &filter.platforms {
                let platform_strs: Vec<&str> = platforms.iter().map(|p| p.as_str()).collect();
                query.and_where(Expr::col(Deployments::Platform).is_in(platform_strs));
            }
            if let Some(model) = filter.deployment_model {
                query.cond_where(Self::deployment_model_condition(model));
            }

            query.limit(limit as u64);
            query.to_string(SqliteQueryBuilder)
        };

        // BEGIN IMMEDIATE makes selection, lease mutation, and miss classification
        // one atomic decision. A follow-up read could otherwise misreport a phase
        // change as contention (or vice versa).
        let mut conn = self.db.conn().lock().await;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Failed to begin deployment acquisition transaction".to_string(),
            })?;
        let mut rows =
            tx.query(&select_sql, ())
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

        drop(rows);

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
                if let Some(model) = filter.deployment_model {
                    query.cond_where(Self::deployment_model_condition(model));
                }

                query
                    .cond_where(
                        sea_query::Cond::any()
                            .add(Expr::col(Deployments::LockedBy).is_null())
                            .add(Expr::cust(stale_lock_condition.clone())),
                    )
                    .cond_where(
                        sea_query::Cond::any()
                            .add(Expr::col(Deployments::NextStepAfter).is_null())
                            .add(Expr::col(Deployments::NextStepAfter).lte(now.to_rfc3339())),
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

            let rows_affected =
                tx.execute(&lock_sql, ())
                    .await
                    .into_alien_error()
                    .context(GenericError {
                        message: "Failed to acquire deployment lease".to_string(),
                    })?;

            // Only count as acquired if our UPDATE actually modified a row.
            // Another caller may have locked it between our SELECT and UPDATE.
            if rows_affected > 0 {
                acquired.push(AcquiredDeployment {
                    deployment: DeploymentRecord {
                        locked_by: Some(session.to_string()),
                        locked_at: Some(now),
                        ..dep
                    },
                    execution_claim: None,
                });
            }
        }

        let acquired_ids = acquired
            .iter()
            .map(|item| item.deployment.id.as_str())
            .collect::<HashSet<_>>();
        let mut not_acquired = Vec::new();

        if let Some(requested_ids) = filter.deployment_ids.as_ref().filter(|ids| !ids.is_empty()) {
            let reason_sql = {
                let id_strs = requested_ids.iter().map(String::as_str).collect::<Vec<_>>();
                Query::select()
                    .columns([
                        Deployments::Id,
                        Deployments::Status,
                        Deployments::RetryRequested,
                        Deployments::LockedBy,
                        Deployments::LockedAt,
                        Deployments::NextStepAfter,
                        Deployments::Platform,
                        Deployments::StackSettings,
                    ])
                    .from(Deployments::Table)
                    .and_where(Expr::col(Deployments::Id).is_in(id_strs))
                    .to_string(SqliteQueryBuilder)
            };
            let mut reason_rows =
                tx.query(&reason_sql, ())
                    .await
                    .into_alien_error()
                    .context(GenericError {
                        message: "Failed to classify unacquired deployments".to_string(),
                    })?;

            while let Some(row) =
                reason_rows
                    .next()
                    .await
                    .into_alien_error()
                    .context(GenericError {
                        message: "Failed to read unacquired deployment state".to_string(),
                    })?
            {
                let parser = RowParser::new(&row);
                let deployment_id = parser.string(0, "id")?;
                if acquired_ids.contains(deployment_id.as_str()) {
                    continue;
                }

                let status = parser.string(1, "status")?;
                let retry_requested = parser
                    .optional_i64(2, "retry_requested")?
                    .unwrap_or_default()
                    != 0;
                let locked_by = parser.optional_string(3, "locked_by")?;
                let locked_at = parser.optional_datetime(4, "locked_at")?;
                let retry_after = parser.optional_datetime(5, "next_step_after")?;
                let platform = parser
                    .string(6, "platform")?
                    .parse::<Platform>()
                    .map_err(|error| db_error(&error))?;
                let stack_settings =
                    parser.json::<alien_core::StackSettings>(7, "stack_settings")?;

                let reason = if filter
                    .deployment_model
                    .is_some_and(|model| model != stack_settings.deployment_model)
                {
                    DeploymentAcquireUnavailableReason::DeploymentModelMismatch
                } else if filter
                    .platforms
                    .as_ref()
                    .is_some_and(|platforms| !platforms.contains(&platform))
                {
                    DeploymentAcquireUnavailableReason::PlatformMismatch
                } else if !Self::acquire_status_matches(
                    &status,
                    retry_requested,
                    explicit_status_filter,
                ) {
                    DeploymentAcquireUnavailableReason::StatusMismatch
                } else if filter.excluded_deployment_ids.contains(&deployment_id)
                    || retry_after.is_some_and(|available_at| available_at > now)
                {
                    DeploymentAcquireUnavailableReason::Deferred
                } else if locked_by.is_some()
                    && locked_at.is_some_and(|locked_at| locked_at >= now - Duration::minutes(5))
                {
                    DeploymentAcquireUnavailableReason::Contended
                } else {
                    DeploymentAcquireUnavailableReason::LimitReached
                };

                not_acquired.push(UnacquiredDeployment {
                    deployment_id,
                    reason,
                    retry_after: (reason == DeploymentAcquireUnavailableReason::Deferred)
                        .then_some(retry_after)
                        .flatten(),
                });
            }
            drop(reason_rows);
        }

        tx.commit().await.into_alien_error().context(GenericError {
            message: "Failed to commit deployment acquisition".to_string(),
        })?;

        Ok(DeploymentAcquireResult {
            deployments: acquired,
            not_acquired,
        })
    }

    async fn reconcile(
        &self,
        caller: &crate::auth::Subject,
        mut data: ReconcileData,
    ) -> Result<ReconcileOutcome, AlienError> {
        let deployment = self.get_deployment(caller, &data.deployment_id).await?;
        let installed_record = deployment
            .as_ref()
            .ok_or_else(|| db_error("Deployment not found during reconcile"))?;
        let registered_protocol = installed_record.required_deployment_protocol_version();
        // The row may still hold an older protocol while a deletion upgrades it, so the
        // write guard compares against the stored value, not the required one.
        let stored_protocol = installed_record.deployment_protocol_version;
        let stored_deletion_policy = installed_record
            .runtime_metadata
            .as_ref()
            .map(|metadata| metadata.deletion_policy)
            .unwrap_or_default();
        if !alien_core::accepts_deployment_protocol_report(
            registered_protocol,
            data.state.protocol_version,
        ) || deployment.as_ref().is_some_and(|deployment| {
            deployment.base_platform.unwrap_or(deployment.platform) == Platform::Gcp
                && registered_protocol < alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION
                && data.state.protocol_version != registered_protocol
        }) {
            return Err(ErrorData::bad_request(
                "A deployment report cannot downgrade or use an unsupported protocol",
            )
            .into_generic());
        }
        // A delete can change policy while a runtime step holds the lease.
        // Save its completed work, but only the stored policy authorizes cleanup.
        if data.state.runtime_metadata.is_some()
            || stored_deletion_policy != alien_core::DeletionPolicy::default()
        {
            data.state
                .runtime_metadata
                .get_or_insert_default()
                .deletion_policy = stored_deletion_policy;
        }
        // Pull reports own runtime progress, not setup-owned identity cleanup.
        // Retained/deleted snapshots are final until an explicit delete request
        // reopens the row. A setup checkpoint may only be advanced by its lease owner.
        let setup_owned = Self::SETUP_TEARDOWN_STATUSES.contains(&installed_record.status.as_str());
        if setup_owned && installed_record.locked_by.as_deref() != Some(data.session.as_str()) {
            return Err(ErrorData::bad_request(
                "Setup cleanup requires its active lease; reload before retrying",
            )
            .into_generic());
        }
        if matches!(installed_record.status.as_str(), "retained" | "deleted") {
            return Ok(ReconcileOutcome {
                record: installed_record.clone(),
                target_operations_bundle_set: None,
                target_dynamic_containers: None,
            });
        }
        let installed_grant = deployment
            .as_ref()
            .and_then(|deployment| deployment.runtime_metadata.as_ref())
            .is_some_and(|metadata| metadata.automatic_setup);
        let may_install_grant = matches!(
            caller.role,
            crate::auth::Role::WorkspaceAdmin
                | crate::auth::Role::ProjectDeveloper
                | crate::auth::Role::DeploymentGroupDeployer
        ) && deployment.as_ref().is_some_and(|deployment| {
            deployment.import_source.is_none()
                && deployment.stack_settings.as_ref().is_some_and(|settings| {
                    settings.setup_management == alien_core::SetupManagement::Automatic
                })
                && matches!(
                    deployment.platform,
                    Platform::Aws | Platform::Gcp | Platform::Azure
                )
        });
        if !may_install_grant {
            if installed_grant || data.state.runtime_metadata.is_some() {
                data.state
                    .runtime_metadata
                    .get_or_insert_default()
                    .automatic_setup = installed_grant;
            }
        }
        let now = Utc::now();
        let state = &data.state;
        let next_step_after = data.suggested_delay_ms.map(|delay_ms| {
            let bounded_delay_ms = delay_ms.min(365 * 24 * 60 * 60 * 1_000);
            now + chrono::Duration::milliseconds(bounded_delay_ms as i64)
        });

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

        let current_release_id = state
            .current_release
            .as_ref()
            .and_then(|r| r.release_id.clone());

        // Serialize status using serde (kebab-case per DeploymentStatus definition)
        let status_str = serde_json::to_value(&state.status)
            .and_then(|v| Ok(v.as_str().unwrap_or("pending").to_string()))
            .unwrap_or_else(|_| "pending".to_string());
        // Check if deployment completed (current matches desired)
        let status_update: sea_query::SimpleExpr = if Self::is_delete_progress(state.status) {
            status_str.clone().into()
        } else {
            let mut status = Expr::case(
                Expr::col(Deployments::Status).is_in(Self::RUNTIME_DELETE_STATUSES),
                Expr::col(Deployments::Status),
            );
            if matches!(
                state.status,
                alien_core::DeploymentStatus::Running | alien_core::DeploymentStatus::RefreshFailed
            ) {
                let newer_release = match current_release_id.as_deref() {
                    Some(current) => Expr::col(Deployments::DesiredReleaseId).ne(current),
                    None => Expr::col(Deployments::Status)
                        .eq("update-pending")
                        .and(Expr::col(Deployments::DesiredReleaseId).is_not_null()),
                };
                // A heartbeat can finish after a new release queues its update.
                // Persist its resource observations without consuming that request.
                status = status.case(newer_release, "update-pending");
            }
            status.finally(status_str.clone()).into()
        };
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
                // A delete request can arrive while the current manager step owns
                // the lock. Persist that step's latest resource state without
                // allowing its stale status to erase the deletion request.
                .value(Deployments::Status, status_update)
                .value(
                    Deployments::DeploymentProtocolVersion,
                    state.protocol_version as i64,
                )
                .value(
                    Deployments::RetryRequested,
                    if retry_requested { 1i64 } else { 0i64 },
                )
                .value(
                    Deployments::NextStepAfter,
                    next_step_after.map(|value| value.to_rfc3339()),
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

            // Compare against the row being updated, not the earlier read: a new
            // release may arrive while reconciliation serializes its observations.
            let desired_release: sea_query::SimpleExpr = match current_release_id.as_deref() {
                Some(current) => Expr::case(
                    Expr::col(Deployments::DesiredReleaseId).eq(current),
                    Expr::val(Option::<String>::None),
                )
                .finally(Expr::col(Deployments::DesiredReleaseId))
                .into(),
                None => Expr::col(Deployments::DesiredReleaseId).into(),
            };
            query.value(Deployments::DesiredReleaseId, desired_release);

            // Runtime pull reports do not hold a lease. Setup cleanup does:
            // protect its checkpoint if ownership or lifecycle changes after the read.
            if setup_owned {
                query.and_where(Expr::col(Deployments::LockedBy).eq(data.session.as_str()));
            }
            query
                .and_where(Expr::col(Deployments::Id).eq(&data.deployment_id as &str))
                .and_where(Expr::col(Deployments::Status).eq(installed_record.status.as_str()))
                .and_where(
                    Expr::col(Deployments::DeploymentProtocolVersion).eq(stored_protocol as i64),
                )
                .and_where(Expr::cust_with_values(
                    "COALESCE(json_extract(runtime_metadata, '$.deletionPolicy'), 'retain') = ?",
                    [match stored_deletion_policy {
                        alien_core::DeletionPolicy::Retain => "retain",
                        alien_core::DeletionPolicy::Erase => "erase",
                    }],
                ))
                .to_string(SqliteQueryBuilder)
        };

        if self.db.execute_returning_rows_affected(&sql).await? != 1 {
            return Err(ErrorData::bad_request(
                "Deployment lifecycle, protocol, deletion policy or cleanup ownership changed during reconcile; reload before retrying",
            )
            .into_generic());
        }

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

        // Fetch and return the updated deployment. OSS has no operations-plugin
        // concept, so there is never a target bundle set to report back.
        let record = self
            .get_deployment(caller, &data.deployment_id)
            .await?
            .ok_or_else(|| db_error("Deployment not found after reconcile"))?;
        Ok(ReconcileOutcome {
            record,
            target_operations_bundle_set: None,
            target_dynamic_containers: None,
        })
    }

    async fn release(
        &self,
        _caller: &crate::auth::Subject,
        deployment_id: &str,
        session: &str,
        _execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
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

    /// Acquire clears `retry_requested` on the row it locks, so put it back in
    /// the same statement that releases the lock: the lease is never released
    /// without the retry request, or the other way around.
    async fn release_unused_acquisition(
        &self,
        _caller: &crate::auth::Subject,
        acquired: &crate::traits::deployment_store::AcquiredDeployment,
        session: &str,
    ) -> Result<(), AlienError> {
        let sql = {
            let mut query = Query::update();
            query
                .table(Deployments::Table)
                .value(Deployments::LockedBy, Option::<String>::None)
                .value(Deployments::LockedAt, Option::<String>::None);
            if acquired.deployment.retry_requested {
                query.value(Deployments::RetryRequested, 1i64);
            }
            query
                .and_where(Expr::col(Deployments::Id).eq(acquired.deployment.id.as_str()))
                .and_where(Expr::col(Deployments::LockedBy).eq(session))
                .to_string(SqliteQueryBuilder)
        };
        self.db.execute(&sql).await
    }

    async fn renew_lease(
        &self,
        _caller: &crate::auth::Subject,
        deployment_id: &str,
        session: &str,
        _execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
    ) -> Result<(), AlienError> {
        let sql = Query::update()
            .table(Deployments::Table)
            .value(Deployments::LockedAt, Utc::now().to_rfc3339())
            .and_where(Expr::col(Deployments::Id).eq(deployment_id))
            .and_where(Expr::col(Deployments::LockedBy).eq(session))
            .to_string(SqliteQueryBuilder);
        let rows_affected = self.db.execute_returning_rows_affected(&sql).await?;
        if rows_affected == 0 {
            return Err(AlienError::new(crate::error::ErrorData::DeploymentLocked {
                deployment_id: deployment_id.to_string(),
                locked_by: None,
            })
            .into_generic());
        }
        Ok(())
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
                DeploymentGroups::Setup,
            ])
            .values_panic([
                id.clone().into(),
                params.name.clone().into(),
                params.max_deployments.into(),
                0i64.into(),
                now.to_rfc3339().into(),
                setup_json(&params.setup)?.into(),
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
            setup: params.setup,
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
                DeploymentGroups::Setup,
            ])
            .values_panic([
                id.into(),
                params.name.clone().into(),
                params.max_deployments.into(),
                0i64.into(),
                now.to_rfc3339().into(),
                setup_json(&params.setup)?.into(),
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
            setup: params.setup,
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
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::Setup)),
                sea_query::Alias::new("setup"),
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
            .expr_as(
                Expr::col((DeploymentGroups::Table, DeploymentGroups::Setup)),
                sea_query::Alias::new("setup"),
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

#[cfg(test)]
mod gcp_project_consumer_tests {
    use std::{collections::HashMap, sync::Arc};

    use alien_core::{
        EnvironmentInfo, GcpEnvironmentInfo, Platform, RuntimeMetadata, StackSettings, StackState,
        CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    };

    use super::SqliteDeploymentStore;
    use crate::{
        auth::Subject,
        stores::sqlite::SqliteDatabase,
        traits::{
            deployment_store::{
                CreateDeploymentGroupParams, CreateImportedDeploymentParams, DeploymentGroupSetup,
            },
            DeploymentStore,
        },
    };

    const SHARED_PROJECT: &str = "111111111111";

    async fn create_gcp_deployment(
        store: &SqliteDeploymentStore,
        group_id: &str,
        name: &str,
        project_number: &str,
        status: &str,
    ) -> String {
        store
            .create_with_state(
                &Subject::system(),
                CreateImportedDeploymentParams {
                    name: name.to_string(),
                    deployment_group_id: group_id.to_string(),
                    platform: Platform::Gcp,
                    deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                    base_platform: None,
                    stack_settings: StackSettings::default(),
                    stack_state: StackState::new(Platform::Gcp),
                    environment_info: Some(EnvironmentInfo::Gcp(GcpEnvironmentInfo {
                        project_number: project_number.to_string(),
                        project_id: format!("project-{project_number}"),
                        region: "us-central1".to_string(),
                    })),
                    runtime_metadata: RuntimeMetadata::default(),
                    status: status.to_string(),
                    current_release_id: None,
                    desired_release_id: None,
                    import_source: None,
                    setup_metadata: None,
                    setup_target: "test".to_string(),
                    setup_fingerprint: "test".to_string(),
                    setup_fingerprint_version: 1,
                    deployment_token: None,
                    management_config: None,
                    input_values: HashMap::new(),
                },
            )
            .await
            .expect("create GCP deployment")
            .id
    }

    #[tokio::test]
    async fn quota_overrides_patch_only_idle_unlocked_deployments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manager.db");
        let db = Arc::new(SqliteDatabase::new(path.to_str().unwrap()).await.unwrap());
        let store = SqliteDeploymentStore::new(db.clone());
        let caller = Subject::system();
        let group = store
            .create_deployment_group(
                &caller,
                CreateDeploymentGroupParams {
                    name: "quota-overrides".into(),
                    max_deployments: 20,
                    setup: DeploymentGroupSetup::default(),
                },
            )
            .await
            .unwrap();
        let overrides = vec!["CPUS".to_string()];
        for (index, (status, locked, accepted)) in [
            ("running", false, true),
            ("preflights-failed", false, true),
            ("preflights-failed", true, false),
            ("running", true, false),
            ("pending", false, false),
            ("initial-setup", false, false),
            ("update-pending", false, false),
            ("provisioning", false, false),
            ("updating", false, false),
            ("update-failed", false, true),
            ("update-failed", true, false),
            ("deleting", false, false),
            ("deleted", false, false),
        ]
        .into_iter()
        .enumerate()
        {
            let id = create_gcp_deployment(
                &store,
                &group.id,
                &format!("case-{index}"),
                SHARED_PROJECT,
                status,
            )
            .await;
            // Unknown future settings must survive even though StackSettings
            // would discard them during deserialization.
            let original = serde_json::json!({
                "futureSetting": {"keep": true},
                "quotaWarningOverrides": [],
            });
            let sql = sea_query::Query::update()
                .table(super::Deployments::Table)
                .value(super::Deployments::StackSettings, original.to_string())
                .value(super::Deployments::LockedBy, locked.then_some("owner"))
                .and_where(sea_query::Expr::col(super::Deployments::Id).eq(id.as_str()))
                .to_string(sea_query::SqliteQueryBuilder);
            db.execute(&sql).await.unwrap();
            assert_eq!(
                store
                    .set_quota_warning_overrides(&caller, &id, &overrides)
                    .await
                    .unwrap(),
                accepted,
                "{status}, locked={locked}"
            );
            let read_settings = || async {
                let sql = sea_query::Query::select()
                    .column(super::Deployments::StackSettings)
                    .from(super::Deployments::Table)
                    .and_where(sea_query::Expr::col(super::Deployments::Id).eq(id.as_str()))
                    .to_string(sea_query::SqliteQueryBuilder);
                let conn = db.conn().lock().await;
                let mut rows = conn.query(&sql, ()).await.unwrap();
                let row = rows.next().await.unwrap().unwrap();
                serde_json::from_str::<serde_json::Value>(&row.get::<String>(0).unwrap()).unwrap()
            };
            let mut expected = original.clone();
            if accepted {
                expected["quotaWarningOverrides"] = serde_json::json!(overrides);
            }
            assert_eq!(read_settings().await, expected);
            let error = store
                .set_quota_warning_overrides(&caller, &id, &["L-1216C47A".into()])
                .await
                .unwrap_err();
            assert_eq!(error.code, "INVALID_QUOTA_OVERRIDE");
            assert_eq!(
                read_settings().await,
                expected,
                "invalid IDs cannot change settings"
            );
            assert_eq!(
                store
                    .set_quota_warning_overrides(&caller, &id, &[])
                    .await
                    .unwrap(),
                accepted
            );
            assert_eq!(
                read_settings().await,
                original,
                "explicit clear preserves all other settings"
            );
            let record = store.get_deployment(&caller, &id).await.unwrap().unwrap();
            assert_eq!(record.status, status);
            assert!(!record.retry_requested, "saving must not implicitly retry");
        }
        assert!(!store
            .set_quota_warning_overrides(&caller, "missing", &overrides)
            .await
            .unwrap());
    }

    /// The question a GCP revoke asks before removing a project's shared Cloud Run grant.
    #[tokio::test]
    async fn only_another_deployment_in_the_same_project_keeps_the_shared_grant() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let path = directory.path().join("manager.db");
        let db = Arc::new(
            SqliteDatabase::new(path.to_str().expect("utf-8 path"))
                .await
                .expect("open database"),
        );
        let store = SqliteDeploymentStore::new(db);
        let group = store
            .create_deployment_group(
                &Subject::system(),
                CreateDeploymentGroupParams {
                    name: "group".to_string(),
                    max_deployments: 10,
                    setup: DeploymentGroupSetup::default(),
                },
            )
            .await
            .expect("create deployment group");

        let deleting =
            create_gcp_deployment(&store, &group.id, "deleting", SHARED_PROJECT, "running").await;
        create_gcp_deployment(&store, &group.id, "elsewhere", "222222222222", "running").await;
        // A record a store keeps after deletion (status `deleted`) no longer pulls images.
        create_gcp_deployment(&store, &group.id, "deleted", SHARED_PROJECT, "deleted").await;

        create_gcp_deployment(
            &store,
            &group.id,
            "imported",
            SHARED_PROJECT,
            "teardown-required",
        )
        .await;
        for status in ["retention-required", "retained"] {
            create_gcp_deployment(&store, &group.id, status, SHARED_PROJECT, status).await;
        }

        assert!(
            !store
                .has_other_gcp_project_deployment(&Subject::system(), SHARED_PROJECT, Some("default".to_string()), &deleting)
                .await
                .expect("consumer check"),
            "neither the deployment itself, one in another project, nor a deleted one shares the grant"
        );

        assert!(!store
            .has_other_gcp_project_deployment(
                &Subject::system(),
                SHARED_PROJECT,
                Some("prj_other".to_string()),
                &deleting
            )
            .await
            .expect("another Alien project has no consumer"));

        let sibling =
            create_gcp_deployment(&store, &group.id, "sibling", SHARED_PROJECT, "running").await;
        assert!(
            store
                .has_other_gcp_project_deployment(
                    &Subject::system(),
                    SHARED_PROJECT,
                    Some("default".to_string()),
                    &deleting
                )
                .await
                .expect("consumer check"),
            "a sibling in the same project still pulls through the grant"
        );

        assert!(!store
            .has_other_gcp_project_deployment(
                &Subject::system(),
                SHARED_PROJECT,
                Some("prj_other".to_string()),
                &deleting
            )
            .await
            .expect("the sibling uses a different Alien repository"));

        store
            .delete_deployment(&Subject::system(), &sibling)
            .await
            .expect("delete sibling");
        assert!(
            !store
                .has_other_gcp_project_deployment(
                    &Subject::system(),
                    SHARED_PROJECT,
                    Some("default".to_string()),
                    &deleting
                )
                .await
                .expect("consumer check"),
            "a removed sibling no longer shares the grant"
        );
    }
}
