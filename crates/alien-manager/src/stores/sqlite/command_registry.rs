//! SQLite implementation of CommandRegistry from alien-commands.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{Expr, Query, SqliteQueryBuilder};
use std::sync::Arc;
use uuid::Uuid;

use alien_commands::server::{
    CommandEnvelopeData, CommandMetadata, CommandRegistry, CommandStatus,
};
use alien_core::{CommandState, DeploymentModel};
use alien_error::IntoAlienError;

use super::database::{RowParser, SqliteDatabase};
use super::migrations::Commands;

pub struct SqliteCommandRegistry {
    db: Arc<SqliteDatabase>,
    deployment_store: Arc<dyn crate::traits::DeploymentStore>,
}

impl SqliteCommandRegistry {
    pub fn new(
        db: Arc<SqliteDatabase>,
        deployment_store: Arc<dyn crate::traits::DeploymentStore>,
    ) -> Self {
        Self {
            db,
            deployment_store,
        }
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
        })
    }

    fn parse_envelope_data(row: &turso::Row) -> alien_commands::error::Result<CommandEnvelopeData> {
        let p = RowParser::new(row);
        let state_str: String = p.string(3, "state").map_err(to_cmd_err)?;
        let deployment_model_str: String = p.string(4, "deployment_model").map_err(to_cmd_err)?;

        Ok(CommandEnvelopeData {
            command_id: p.string(0, "id").map_err(to_cmd_err)?,
            deployment_id: p.string(1, "deployment_id").map_err(to_cmd_err)?,
            command: p.string(2, "name").map_err(to_cmd_err)?,
            state: deserialize_command_state(&state_str),
            deployment_model: deserialize_deployment_model(&deployment_model_str),
            attempt: p.i64(5, "attempt").map_err(to_cmd_err)? as u32,
            deadline: p.optional_datetime(6, "deadline").map_err(to_cmd_err)?,
        })
    }

    /// All columns for full command queries.
    const COMMAND_COLUMNS: [Commands; 13] = [
        Commands::Id,                // 0
        Commands::DeploymentId,      // 1
        Commands::Name,              // 2
        Commands::State,             // 3
        Commands::DeploymentModel,   // 4
        Commands::Attempt,           // 5
        Commands::Deadline,          // 6
        Commands::CreatedAt,         // 7
        Commands::DispatchedAt,      // 8
        Commands::CompletedAt,       // 9
        Commands::RequestSizeBytes,  // 10
        Commands::ResponseSizeBytes, // 11
        Commands::Error,             // 12
    ];

    /// Columns needed for envelope data (subset).
    const ENVELOPE_COLUMNS: [Commands; 7] = [
        Commands::Id,              // 0
        Commands::DeploymentId,    // 1
        Commands::Name,            // 2
        Commands::State,           // 3
        Commands::DeploymentModel, // 4
        Commands::Attempt,         // 5
        Commands::Deadline,        // 6
    ];
}

#[async_trait]
impl CommandRegistry for SqliteCommandRegistry {
    async fn create_command(
        &self,
        deployment_id: &str,
        command_name: &str,
        initial_state: CommandState,
        deadline: Option<DateTime<Utc>>,
        request_size_bytes: Option<u64>,
    ) -> alien_commands::error::Result<CommandMetadata> {
        let command_id = format!("cmd_{}", Uuid::new_v4());
        let now = Utc::now();

        let state_str = serialize_enum(&initial_state);
        // Look up real deployment model from the deployment record
        let deployment_model = match self.deployment_store.get_deployment(deployment_id).await {
            Ok(Some(record)) => match record.platform {
                alien_core::Platform::Kubernetes | alien_core::Platform::Local => {
                    DeploymentModel::Pull
                }
                _ => record.stack_settings.deployment_model,
            },
            _ => DeploymentModel::Pull, // fallback
        };
        let deployment_model_str = serialize_enum(&deployment_model);

        let sql = Query::insert()
            .into_table(Commands::Table)
            .columns([
                Commands::Id,
                Commands::DeploymentId,
                Commands::Name,
                Commands::State,
                Commands::DeploymentModel,
                Commands::Attempt,
                Commands::Deadline,
                Commands::CreatedAt,
                Commands::RequestSizeBytes,
            ])
            .values_panic([
                command_id.clone().into(),
                deployment_id.into(),
                command_name.into(),
                state_str.into(),
                deployment_model_str.into(),
                1i64.into(),
                deadline.map(|d| d.to_rfc3339()).into(),
                now.to_rfc3339().into(),
                request_size_bytes.map(|n| n as i64).into(),
            ])
            .to_string(SqliteQueryBuilder);

        self.db.execute(&sql).await.map_err(to_cmd_err)?;

        Ok(CommandMetadata {
            command_id,
            deployment_model,
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

/// Deserialize DeploymentModel from its serde string representation.
fn deserialize_deployment_model(s: &str) -> DeploymentModel {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or_else(|e| {
        tracing::warn!(
            "Failed to deserialize DeploymentModel '{}': {}, using Pull",
            s,
            e
        );
        DeploymentModel::Pull
    })
}

/// Convert an AlienError (generic) into a command error (ErrorData).
fn to_cmd_err<E: std::fmt::Display>(err: E) -> alien_commands::error::Error {
    alien_error::AlienError::new(alien_commands::error::ErrorData::Other {
        message: err.to_string(),
    })
}
