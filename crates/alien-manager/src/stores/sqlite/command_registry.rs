//! SQLite implementation of CommandRegistry from alien-commands.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{Expr, Query, SqliteQueryBuilder};
use std::sync::Arc;

use alien_commands::error::ErrorData as CommandErrorData;
use alien_commands::server::{
    delivery_mode_for, select_command_target, CommandEnvelopeData, CommandMetadata,
    CommandRegistry, CommandStatus, ResolvedCommandTarget,
};
use alien_core::{
    CommandDeliveryMode, CommandState, CommandTarget, CommandTargetType, DeploymentModel, Platform,
};
use alien_error::IntoAlienError;

use super::database::{RowParser, SqliteDatabase};
use super::migrations::Commands;

pub struct SqliteCommandRegistry {
    db: Arc<SqliteDatabase>,
    deployment_store: Arc<dyn crate::traits::DeploymentStore>,
    release_store: Arc<dyn crate::traits::ReleaseStore>,
}

impl SqliteCommandRegistry {
    pub fn new(
        db: Arc<SqliteDatabase>,
        deployment_store: Arc<dyn crate::traits::DeploymentStore>,
        release_store: Arc<dyn crate::traits::ReleaseStore>,
    ) -> Self {
        Self {
            db,
            deployment_store,
            release_store,
        }
    }

    /// Derive the Worker delivery context for this deployment: Push only when
    /// the platform has a push path (not Kubernetes or Local) AND the stack
    /// settings use the Push deployment model; otherwise Pull.
    ///
    /// This platform-dependent derivation stays manager-side; the pinned
    /// per-type rule (Container/Daemon always Pull) lives in the shared
    /// [`delivery_mode_for`] that both registries call.
    fn worker_delivery_mode(
        platform: Platform,
        stack_deployment_model: DeploymentModel,
    ) -> CommandDeliveryMode {
        let platform_has_push_path = !matches!(platform, Platform::Kubernetes | Platform::Local);
        if platform_has_push_path && stack_deployment_model == DeploymentModel::Push {
            CommandDeliveryMode::Push
        } else {
            CommandDeliveryMode::Pull
        }
    }

    /// Derive the delivery mode for a resolved target, failing fast when the
    /// deployment carries no stack settings.
    ///
    /// A missing `stack_settings` is an invariant violation: defaulting it
    /// (`DeploymentModel::default()` is Push) would silently resolve Worker
    /// targets to Push delivery — the riskiest direction. Per the repo's "fail
    /// fast, don't fall back silently" rule we return a typed error naming the
    /// deployment instead. The per-type dispatch (Container/Daemon always Pull)
    /// then goes through the shared [`delivery_mode_for`].
    fn resolve_delivery_mode(
        resource_type: CommandTargetType,
        platform: Platform,
        stack_settings: Option<&alien_core::StackSettings>,
        deployment_id: &str,
    ) -> alien_commands::error::Result<CommandDeliveryMode> {
        let stack_deployment_model =
            stack_settings.map(|s| s.deployment_model).ok_or_else(|| {
                alien_error::AlienError::new(CommandErrorData::Other {
                    message: format!(
                        "Deployment '{}' is missing stack_settings.deployment_model; \
                     cannot derive command delivery mode",
                        deployment_id
                    ),
                })
            })?;
        let worker_mode = Self::worker_delivery_mode(platform, stack_deployment_model);
        Ok(delivery_mode_for(resource_type, worker_mode))
    }

    fn parse_command_status(row: &turso::Row) -> alien_commands::error::Result<CommandStatus> {
        let p = RowParser::new(row);
        let state_str: String = p.string(3, "state").map_err(to_cmd_err)?;

        let request_size: Option<i64> = p
            .optional_i64(10, "request_size_bytes")
            .map_err(to_cmd_err)?;
        let response_size: Option<i64> = p
            .optional_i64(11, "response_size_bytes")
            .map_err(to_cmd_err)?;

        Ok(CommandStatus {
            command_id: p.string(0, "id").map_err(to_cmd_err)?,
            deployment_id: p.string(1, "deployment_id").map_err(to_cmd_err)?,
            command: p.string(2, "name").map_err(to_cmd_err)?,
            state: deserialize_command_state(&state_str),
            attempt: p.i64(5, "attempt").map_err(to_cmd_err)? as u32,
            deadline: p.optional_datetime(6, "deadline").map_err(to_cmd_err)?,
            created_at: p.datetime(7, "created_at").map_err(to_cmd_err)?,
            dispatched_at: p
                .optional_datetime(8, "dispatched_at")
                .map_err(to_cmd_err)?,
            completed_at: p.optional_datetime(9, "completed_at").map_err(to_cmd_err)?,
            request_size_bytes: request_size.map(|n| n as u64),
            response_size_bytes: response_size.map(|n| n as u64),
            error: p.optional_json(12, "error").map_err(to_cmd_err)?,
            // ALIEN-219: status is a read-only path, so it tolerates legacy
            // rows written before target columns existed (NULL target). Such
            // rows can never be leased or dispatched (they are absent from the
            // per-target pending index), so a synthesized deployment-scoped
            // marker here is display-only and cannot cause misdelivery.
            target: parse_target_columns(&p, 13, 14)
                .map_err(to_cmd_err)?
                .unwrap_or_else(|| {
                    let deployment_id = p.string(1, "deployment_id").unwrap_or_default();
                    CommandTarget::new(deployment_id, CommandTargetType::Worker)
                }),
        })
    }

    fn parse_envelope_data(row: &turso::Row) -> alien_commands::error::Result<CommandEnvelopeData> {
        let p = RowParser::new(row);
        let state_str: String = p.string(3, "state").map_err(to_cmd_err)?;
        let delivery_mode_str: String = p.string(4, "delivery_mode").map_err(to_cmd_err)?;
        let command_id = p.string(0, "id").map_err(to_cmd_err)?;

        // ALIEN-219: envelope data feeds the lease/dispatch path, which must
        // never deliver to the wrong resource. A legacy row without a target
        // (NULL columns) cannot be safely dispatched, so we fail loudly rather
        // than synthesize a target. In practice such rows are unreachable here
        // — they predate the per-target pending index and so are never leased
        // — but a defensive loud error beats silent misdelivery.
        let target = parse_target_columns(&p, 7, 8)
            .map_err(to_cmd_err)?
            .ok_or_else(|| {
                alien_error::AlienError::new(CommandErrorData::Other {
                    message: format!(
                        "Command '{}' predates ALIEN-219 target columns and cannot be \
                         leased or dispatched (no resolved target)",
                        command_id
                    ),
                })
            })?;

        Ok(CommandEnvelopeData {
            command_id,
            deployment_id: p.string(1, "deployment_id").map_err(to_cmd_err)?,
            command: p.string(2, "name").map_err(to_cmd_err)?,
            state: deserialize_command_state(&state_str),
            delivery_mode: deserialize_delivery_mode(&delivery_mode_str),
            attempt: p.i64(5, "attempt").map_err(to_cmd_err)? as u32,
            deadline: p.optional_datetime(6, "deadline").map_err(to_cmd_err)?,
            target,
        })
    }

    /// All columns for full command queries.
    const COMMAND_COLUMNS: [Commands; 15] = [
        Commands::Id,                 // 0
        Commands::DeploymentId,       // 1
        Commands::Name,               // 2
        Commands::State,              // 3
        Commands::DeliveryMode,       // 4
        Commands::Attempt,            // 5
        Commands::Deadline,           // 6
        Commands::CreatedAt,          // 7
        Commands::DispatchedAt,       // 8
        Commands::CompletedAt,        // 9
        Commands::RequestSizeBytes,   // 10
        Commands::ResponseSizeBytes,  // 11
        Commands::Error,              // 12
        Commands::TargetResourceId,   // 13
        Commands::TargetResourceType, // 14
    ];

    /// Columns needed for envelope data (subset).
    const ENVELOPE_COLUMNS: [Commands; 9] = [
        Commands::Id,                 // 0
        Commands::DeploymentId,       // 1
        Commands::Name,               // 2
        Commands::State,              // 3
        Commands::DeliveryMode,       // 4
        Commands::Attempt,            // 5
        Commands::Deadline,           // 6
        Commands::TargetResourceId,   // 7
        Commands::TargetResourceType, // 8
    ];
}

#[async_trait]
impl CommandRegistry for SqliteCommandRegistry {
    async fn resolve_target(
        &self,
        deployment_id: &str,
        requested: Option<&str>,
    ) -> alien_commands::error::Result<ResolvedCommandTarget> {
        // Single-tenant SQLite registry — resolution reads deployment + release
        // state directly, so `Subject::system()` is the correct synthetic caller
        // (the route layer already authorized the human caller against the
        // deployment; see the auth note in routes/commands.rs).
        let caller = crate::auth::Subject::system();

        let deployment = self
            .deployment_store
            .get_deployment(&caller, deployment_id)
            .await
            .map_err(|e| {
                alien_error::AlienError::new(CommandErrorData::Other {
                    message: format!("Failed to load deployment '{}': {}", deployment_id, e),
                })
            })?
            .ok_or_else(|| {
                alien_error::AlienError::new(CommandErrorData::InvalidCommand {
                    message: format!("Deployment '{}' not found", deployment_id),
                })
            })?;

        // Load the deployment's current release stack (if any) and derive its
        // command-capable targets in declaration order. A deployment with no
        // current release, or no stack for its platform, simply has no targets
        // — the resolution rules below then surface NO_COMMAND_TARGETS (or
        // COMMAND_TARGET_NOT_FOUND for an explicit request).
        let targets: Vec<CommandTarget> = match deployment.current_release_id.as_deref() {
            Some(release_id) => {
                let release = self
                    .release_store
                    .get_release(&caller, release_id)
                    .await
                    .map_err(|e| {
                        alien_error::AlienError::new(CommandErrorData::Other {
                            message: format!("Failed to load release '{}': {}", release_id, e),
                        })
                    })?
                    .ok_or_else(|| {
                        alien_error::AlienError::new(CommandErrorData::Other {
                            message: format!(
                                "Release '{}' for deployment '{}' not found",
                                release_id, deployment_id
                            ),
                        })
                    })?;
                match release.stacks.get(&deployment.platform) {
                    Some(stack) => stack.command_targets(),
                    None => Vec::new(),
                }
            }
            None => Vec::new(),
        };

        // Selection rules (empty-id guard, explicit lookup, shorthand codes,
        // and the `:`-charset guard) come from the single shared implementation
        // in alien-commands — the same one the in-memory registry calls.
        let target = select_command_target(deployment_id, &targets, requested)?;

        // Fail fast if the stored deployment carries no stack settings rather
        // than silently defaulting Worker delivery to Push (the riskiest
        // direction).
        let delivery_mode = Self::resolve_delivery_mode(
            target.resource_type,
            deployment.platform,
            deployment.stack_settings.as_ref(),
            deployment_id,
        )?;

        Ok(ResolvedCommandTarget {
            target,
            delivery_mode,
        })
    }

    async fn create_command(
        &self,
        deployment_id: &str,
        command_name: &str,
        target: &ResolvedCommandTarget,
        initial_state: CommandState,
        deadline: Option<DateTime<Utc>>,
        request_size_bytes: Option<u64>,
    ) -> alien_commands::error::Result<CommandMetadata> {
        let command_id = alien_core::new_id(alien_core::IdType::Command);
        let now = Utc::now();

        let state_str = serialize_enum(&initial_state);
        // Look up deployment and require it to be fully running before accepting commands.
        // SQLite-backed (single-tenant) registry — caller is unused; use system.
        let caller = crate::auth::Subject::system();
        let deployment = self
            .deployment_store
            .get_deployment(&caller, deployment_id)
            .await
            .map_err(|e| {
                alien_error::AlienError::new(CommandErrorData::Other {
                    message: format!("Failed to load deployment '{}': {}", deployment_id, e),
                })
            })?
            .ok_or_else(|| {
                alien_error::AlienError::new(CommandErrorData::InvalidCommand {
                    message: format!("Deployment '{}' not found", deployment_id),
                })
            })?;

        if deployment.status != "running" {
            return Err(alien_error::AlienError::new(CommandErrorData::InvalidCommand {
                message: format!(
                    "Deployment '{}' is '{}' and cannot receive commands yet. Wait until status is 'running'.",
                    deployment_id, deployment.status
                ),
            }));
        }

        // The delivery mode is decided at resolution time and passed in with
        // the resolved target; it is stored in the delivery-mode column (whose
        // physical name is still `deployment_model` — same "push"/"pull"
        // strings). The resolved target's id and type are stored in the
        // dedicated ALIEN-219 columns.
        let delivery_mode_str = serialize_enum(&target.delivery_mode);
        let target_type_str = serialize_enum(&target.target.resource_type);

        let sql = Query::insert()
            .into_table(Commands::Table)
            .columns([
                Commands::Id,
                Commands::DeploymentId,
                Commands::Name,
                Commands::State,
                Commands::DeliveryMode,
                Commands::Attempt,
                Commands::Deadline,
                Commands::CreatedAt,
                Commands::RequestSizeBytes,
                Commands::TargetResourceId,
                Commands::TargetResourceType,
            ])
            .values_panic([
                command_id.clone().into(),
                deployment_id.into(),
                command_name.into(),
                state_str.into(),
                delivery_mode_str.into(),
                1i64.into(),
                deadline.map(|d| d.to_rfc3339()).into(),
                now.to_rfc3339().into(),
                request_size_bytes.map(|n| n as i64).into(),
                target.target.resource_id.clone().into(),
                target_type_str.into(),
            ])
            .to_string(SqliteQueryBuilder);

        self.db.execute(&sql).await.map_err(to_cmd_err)?;

        Ok(CommandMetadata {
            command_id,
            target: target.target.clone(),
            delivery_mode: target.delivery_mode,
            project_id: "local".to_string(),
        })
    }

    async fn get_command_metadata(
        &self,
        command_id: &str,
    ) -> alien_commands::error::Result<Option<CommandEnvelopeData>> {
        let sql = Query::select()
            .columns(Self::ENVELOPE_COLUMNS)
            .from(Commands::Table)
            .and_where(Expr::col(Commands::Id).eq(command_id))
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .map_err(to_cmd_err)?;

        match rows.next().await.into_alien_error().map_err(to_cmd_err)? {
            Some(row) => Ok(Some(Self::parse_envelope_data(&row)?)),
            None => Ok(None),
        }
    }

    async fn get_command_status(
        &self,
        command_id: &str,
    ) -> alien_commands::error::Result<Option<CommandStatus>> {
        let sql = Query::select()
            .columns(Self::COMMAND_COLUMNS)
            .from(Commands::Table)
            .and_where(Expr::col(Commands::Id).eq(command_id))
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(&sql, ())
            .await
            .into_alien_error()
            .map_err(to_cmd_err)?;

        match rows.next().await.into_alien_error().map_err(to_cmd_err)? {
            Some(row) => Ok(Some(Self::parse_command_status(&row)?)),
            None => Ok(None),
        }
    }

    async fn update_command_state(
        &self,
        command_id: &str,
        state: CommandState,
        dispatched_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
        response_size_bytes: Option<u64>,
        error: Option<serde_json::Value>,
    ) -> alien_commands::error::Result<()> {
        let sql = {
            let mut query = Query::update();
            query
                .table(Commands::Table)
                .value(Commands::State, state.as_ref())
                .and_where(Expr::col(Commands::Id).eq(command_id));
            if let Some(d) = dispatched_at {
                query.value(Commands::DispatchedAt, d.to_rfc3339());
            }
            if let Some(c) = completed_at {
                query.value(Commands::CompletedAt, c.to_rfc3339());
            }
            if let Some(r) = response_size_bytes {
                query.value(Commands::ResponseSizeBytes, r as i64);
            }
            if let Some(e) = &error {
                query.value(
                    Commands::Error,
                    serde_json::to_string(e).unwrap_or_default(),
                );
            }
            query.to_string(SqliteQueryBuilder)
        };

        self.db.execute(&sql).await.map_err(to_cmd_err)?;
        Ok(())
    }

    async fn complete_command(
        &self,
        command_id: &str,
        state: CommandState,
        completed_at: DateTime<Utc>,
        response_size_bytes: Option<u64>,
        error: Option<serde_json::Value>,
    ) -> alien_commands::error::Result<bool> {
        // Single conditional UPDATE: the WHERE clause excludes terminal
        // states, so exactly one of two racing submitters wins and a
        // terminal record is never overwritten.
        let sql = {
            let mut query = Query::update();
            query
                .table(Commands::Table)
                .value(Commands::State, state.as_ref())
                .value(Commands::CompletedAt, completed_at.to_rfc3339())
                .and_where(Expr::col(Commands::Id).eq(command_id))
                .and_where(Expr::col(Commands::State).is_not_in([
                    CommandState::Succeeded.as_ref(),
                    CommandState::Failed.as_ref(),
                    CommandState::Expired.as_ref(),
                ]));
            if let Some(r) = response_size_bytes {
                query.value(Commands::ResponseSizeBytes, r as i64);
            }
            if let Some(e) = &error {
                query.value(
                    Commands::Error,
                    serde_json::to_string(e).unwrap_or_default(),
                );
            }
            query.to_string(SqliteQueryBuilder)
        };

        let rows_affected = self
            .db
            .execute_returning_rows_affected(&sql)
            .await
            .map_err(to_cmd_err)?;
        Ok(rows_affected > 0)
    }

    async fn mark_dispatched_if_not_terminal(
        &self,
        command_id: &str,
        dispatched_at: DateTime<Utc>,
    ) -> alien_commands::error::Result<bool> {
        let sql = Query::update()
            .table(Commands::Table)
            .value(Commands::State, CommandState::Dispatched.as_ref())
            .value(Commands::DispatchedAt, dispatched_at.to_rfc3339())
            .and_where(Expr::col(Commands::Id).eq(command_id))
            .and_where(Expr::col(Commands::State).is_not_in([
                CommandState::Succeeded.as_ref(),
                CommandState::Failed.as_ref(),
                CommandState::Expired.as_ref(),
            ]))
            .to_string(SqliteQueryBuilder);
        let rows_affected = self
            .db
            .execute_returning_rows_affected(&sql)
            .await
            .map_err(to_cmd_err)?;
        Ok(rows_affected > 0)
    }

    async fn increment_attempt(&self, command_id: &str) -> alien_commands::error::Result<u32> {
        let update_sql = Query::update()
            .table(Commands::Table)
            .value(Commands::Attempt, Expr::col(Commands::Attempt).add(1))
            .and_where(Expr::col(Commands::Id).eq(command_id))
            .to_string(SqliteQueryBuilder);
        let select_sql = Query::select()
            .column(Commands::Attempt)
            .from(Commands::Table)
            .and_where(Expr::col(Commands::Id).eq(command_id))
            .to_string(SqliteQueryBuilder);

        let conn = self.db.conn().lock().await;
        conn.execute(&update_sql, ())
            .await
            .into_alien_error()
            .map_err(to_cmd_err)?;

        let mut rows = conn
            .query(&select_sql, ())
            .await
            .into_alien_error()
            .map_err(to_cmd_err)?;

        match rows.next().await.into_alien_error().map_err(to_cmd_err)? {
            Some(row) => {
                let attempt: i64 = row.get(0).into_alien_error().map_err(to_cmd_err)?;
                Ok(attempt as u32)
            }
            None => Ok(1),
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Serialize an enum to its serde string representation.
fn serialize_enum<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Parse the target columns into a [`CommandTarget`], or `None` for a legacy
/// row that predates ALIEN-219 (NULL/empty `target_resource_id`).
fn parse_target_columns(
    p: &RowParser,
    id_idx: usize,
    type_idx: usize,
) -> Result<Option<CommandTarget>, alien_error::AlienError> {
    let resource_id = p.optional_string(id_idx, "target_resource_id")?;
    let Some(resource_id) = resource_id.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let resource_type = p
        .optional_string(type_idx, "target_resource_type")?
        .as_deref()
        .map(deserialize_target_type)
        .unwrap_or(CommandTargetType::Worker);
    Ok(Some(CommandTarget::new(resource_id, resource_type)))
}

/// Deserialize CommandTargetType from its serde string representation.
fn deserialize_target_type(s: &str) -> CommandTargetType {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or_else(|e| {
        tracing::warn!(
            "Failed to deserialize CommandTargetType '{}': {}, using Worker",
            s,
            e
        );
        CommandTargetType::Worker
    })
}

/// Deserialize CommandState from its serde string representation.
fn deserialize_command_state(s: &str) -> CommandState {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or_else(|e| {
        tracing::warn!(
            "Failed to deserialize CommandState '{}': {}, using Pending",
            s,
            e
        );
        CommandState::Pending
    })
}

/// Deserialize CommandDeliveryMode from its serde string representation.
///
/// The deployment_model column historically stored `DeploymentModel` values;
/// both enums serialize to the same "push"/"pull" strings, so old rows parse
/// unchanged.
fn deserialize_delivery_mode(s: &str) -> CommandDeliveryMode {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or_else(|e| {
        tracing::warn!(
            "Failed to deserialize CommandDeliveryMode '{}': {}, using Pull",
            s,
            e
        );
        CommandDeliveryMode::Pull
    })
}

/// Convert an AlienError (generic) into a command error (ErrorData).
fn to_cmd_err<E: std::fmt::Display>(err: E) -> alien_commands::error::Error {
    alien_error::AlienError::new(alien_commands::error::ErrorData::Other {
        message: err.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::sqlite::{SqliteDeploymentStore, SqliteReleaseStore};

    async fn registry() -> (Arc<SqliteDatabase>, SqliteCommandRegistry) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        std::mem::forget(dir);
        let db = Arc::new(SqliteDatabase::new(path.to_str().unwrap()).await.unwrap());
        let dep_store = Arc::new(SqliteDeploymentStore::new(db.clone()));
        let rel_store = Arc::new(SqliteReleaseStore::new(db.clone()));
        let reg = SqliteCommandRegistry::new(db.clone(), dep_store, rel_store);
        (db, reg)
    }

    /// A deployment with no stack settings is an invariant violation. The
    /// delivery-mode derivation must fail loudly (naming the deployment) rather
    /// than silently default to Push — the store's `stack_settings` column is
    /// NOT NULL so this state is unreachable through the store, but the guard
    /// still protects the resolver if that invariant is ever broken upstream.
    #[test]
    fn resolve_delivery_mode_fails_fast_without_stack_settings() {
        let err = SqliteCommandRegistry::resolve_delivery_mode(
            CommandTargetType::Worker,
            Platform::Aws,
            None,
            "dep-x",
        )
        .unwrap_err();
        assert!(
            err.message.contains("deployment_model"),
            "expected a fail-fast deployment-model error, got: {}",
            err.message
        );
    }

    /// With stack settings present, the derivation returns a real mode (Push for
    /// a Worker on a push-capable platform with the Push model) — never the
    /// silent default masking a missing invariant.
    #[test]
    fn resolve_delivery_mode_worker_push_when_present() {
        let settings = alien_core::StackSettings {
            deployment_model: DeploymentModel::Push,
            ..alien_core::StackSettings::default()
        };
        let mode = SqliteCommandRegistry::resolve_delivery_mode(
            CommandTargetType::Worker,
            Platform::Aws,
            Some(&settings),
            "dep-x",
        )
        .unwrap();
        assert_eq!(mode, CommandDeliveryMode::Push);
    }

    /// A pre-ALIEN-219 command row has NULL target columns. Status reads must
    /// tolerate it (read-only path), while envelope/lease reads must fail
    /// loudly rather than synthesize a target that could misdeliver.
    #[tokio::test]
    async fn legacy_null_target_row_status_tolerant_envelope_loud() {
        let (db, reg) = registry().await;

        // Insert a legacy row directly: target columns left NULL.
        db.execute(
            "INSERT INTO commands (id, deployment_id, name, state, deployment_model, attempt, created_at) \
             VALUES ('legacy-cmd', 'dep-legacy', 'sync', 'PENDING', 'pull', 1, '2020-01-01T00:00:00Z')",
        )
        .await
        .unwrap();

        // Status read tolerates the NULL target with a synthesized marker.
        let status = reg.get_command_status("legacy-cmd").await.unwrap().unwrap();
        assert_eq!(status.target.resource_id, "dep-legacy");
        assert_eq!(status.target.resource_type, CommandTargetType::Worker);

        // Envelope/lease read fails loudly — never synthesizes a target.
        let err = reg.get_command_metadata("legacy-cmd").await.unwrap_err();
        assert!(
            err.message.contains("predates ALIEN-219"),
            "expected a loud legacy error, got: {}",
            err.message
        );
    }
}
