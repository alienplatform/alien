//! Command registry abstraction for ARC server
//!
//! The CommandRegistry is the **source of truth** for all command metadata.
//! It tracks command state, timestamps, sizes, and errors.
//!
//! Implementations:
//! - `InMemoryCommandRegistry`: In-memory implementation for tests and local dev (in this crate)
//! - `PlatformCommandRegistry`: Platform API integration (in alien-manager)
//!
//! The ARC KV store holds only operational data: params/response blobs, pending indices, leases.

use crate::error::Result;
use alien_core::{CommandState, DeploymentModel};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Metadata returned when creating a command
#[derive(Debug, Clone)]
pub struct CommandMetadata {
    /// Unique command ID
    pub command_id: String,
    /// How to dispatch the command (Push or Pull)
    pub deployment_model: DeploymentModel,
    /// Project ID for routing/authorization
    pub project_id: String,
}

/// Data needed to build an envelope during lease acquisition
#[derive(Debug, Clone)]
pub struct CommandEnvelopeData {
    pub command_id: String,
    pub deployment_id: String,
    pub command: String, // command name
    pub attempt: u32,
    pub deadline: Option<DateTime<Utc>>,
    pub state: CommandState,
    pub deployment_model: DeploymentModel,
}

/// Full status for GET /commands/{id}
#[derive(Debug, Clone)]
pub struct CommandStatus {
    pub command_id: String,
    pub deployment_id: String,
    pub command: String, // command name
    pub state: CommandState,
    pub attempt: u32,
    pub deadline: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub dispatched_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<serde_json::Value>,
    pub request_size_bytes: Option<u64>,
    pub response_size_bytes: Option<u64>,
}

/// Internal command record stored in memory
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandRecord {
    id: String,
    deployment_id: String,
    command: String,
    state: CommandState,
    attempt: u32,
    deadline: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    dispatched_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    request_size_bytes: Option<u64>,
    response_size_bytes: Option<u64>,
    error: Option<serde_json::Value>,
    deployment_model: DeploymentModel,
    project_id: String,
}

/// Abstraction for command metadata storage and lifecycle tracking.
///
/// The CommandRegistry is the source of truth for all command metadata.
/// Implementations store command state, timestamps, and result information.
#[async_trait]
pub trait CommandRegistry: Send + Sync {
    /// Create a new command and return metadata for routing.
    ///
    /// The registry generates the command_id, determines the deployment_model,
    /// and stores all metadata (state, timestamps, etc.).
    async fn create_command(
        &self,
        deployment_id: &str,
        command_name: &str,
        initial_state: CommandState,
        deadline: Option<DateTime<Utc>>,
        request_size_bytes: Option<u64>,
    ) -> Result<CommandMetadata>;

    /// Get metadata needed to build an envelope during lease acquisition.
    ///
    /// Returns None if command doesn't exist.
    async fn get_command_metadata(&self, command_id: &str) -> Result<Option<CommandEnvelopeData>>;

    /// Get full command status for status endpoint.
    ///
    /// Returns None if command doesn't exist.
    async fn get_command_status(&self, command_id: &str) -> Result<Option<CommandStatus>>;

    /// Update command state during lifecycle (dispatched, completed, failed).
    async fn update_command_state(
        &self,
        command_id: &str,
        state: CommandState,
        dispatched_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
        response_size_bytes: Option<u64>,
        error: Option<serde_json::Value>,
    ) -> Result<()>;

    /// Increment attempt count (on lease release/expiry).
    ///
    /// Returns the new attempt number.
    async fn increment_attempt(&self, command_id: &str) -> Result<u32>;
}

/// In-memory implementation for tests and local development.
///
/// Tracks command metadata in memory. Configurable deployment model (defaults to Pull).
pub struct InMemoryCommandRegistry {
    commands: Arc<RwLock<HashMap<String, CommandRecord>>>,
    deployment_model: DeploymentModel,
}

impl InMemoryCommandRegistry {
    /// Create a new in-memory registry with Pull deployment model (default).
    pub fn new() -> Self {
        Self {
            commands: Arc::new(RwLock::new(HashMap::new())),
            deployment_model: DeploymentModel::Pull,
        }
    }

    /// Create a new in-memory registry with the specified deployment model.
    pub fn with_deployment_model(deployment_model: DeploymentModel) -> Self {
        Self {
            commands: Arc::new(RwLock::new(HashMap::new())),
            deployment_model,
        }
    }

    /// List all command IDs (useful for debugging/testing)
    #[allow(dead_code)]
    pub async fn list_command_ids(&self) -> Vec<String> {
        let commands = self.commands.read().await;
        commands.keys().cloned().collect()
    }
}

impl Default for InMemoryCommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CommandRegistry for InMemoryCommandRegistry {
    async fn create_command(
        &self,
        deployment_id: &str,
        command_name: &str,
        initial_state: CommandState,
        deadline: Option<DateTime<Utc>>,
        request_size_bytes: Option<u64>,
    ) -> Result<CommandMetadata> {
        let command_id = format!("cmd_{}", Uuid::new_v4());

        let record = CommandRecord {
            id: command_id.clone(),
            deployment_id: deployment_id.to_string(),
            command: command_name.to_string(),
            state: initial_state,
            attempt: 1,
            deadline,
            created_at: Utc::now(),
            dispatched_at: None,
            completed_at: None,
            request_size_bytes,
            response_size_bytes: None,
            error: None,
            deployment_model: self.deployment_model,
            project_id: "local-dev".to_string(),
        };

        self.commands
            .write()
            .await
            .insert(command_id.clone(), record);

        Ok(CommandMetadata {
            command_id,
            deployment_model: self.deployment_model,
            project_id: "local-dev".to_string(),
        })
    }

    async fn get_command_metadata(&self, command_id: &str) -> Result<Option<CommandEnvelopeData>> {
        let commands = self.commands.read().await;

        Ok(commands.get(command_id).map(|r| CommandEnvelopeData {
            command_id: r.id.clone(),
            deployment_id: r.deployment_id.clone(),
            command: r.command.clone(),
            attempt: r.attempt,
            deadline: r.deadline,
            state: r.state,
            deployment_model: r.deployment_model,
        }))
    }

    async fn get_command_status(&self, command_id: &str) -> Result<Option<CommandStatus>> {
        let commands = self.commands.read().await;

        Ok(commands.get(command_id).map(|r| CommandStatus {
            command_id: r.id.clone(),
            deployment_id: r.deployment_id.clone(),
            command: r.command.clone(),
            state: r.state,
            attempt: r.attempt,
            deadline: r.deadline,
            created_at: r.created_at,
            dispatched_at: r.dispatched_at,
            completed_at: r.completed_at,
            error: r.error.clone(),
            request_size_bytes: r.request_size_bytes,
            response_size_bytes: r.response_size_bytes,
        }))
    }

    async fn update_command_state(
        &self,
        command_id: &str,
        state: CommandState,
        dispatched_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
        response_size_bytes: Option<u64>,
        error: Option<serde_json::Value>,
    ) -> Result<()> {
        let mut commands = self.commands.write().await;

        if let Some(record) = commands.get_mut(command_id) {
            record.state = state;

            if let Some(ts) = dispatched_at {
                record.dispatched_at = Some(ts);
            }

            if let Some(ts) = completed_at {
                record.completed_at = Some(ts);
            }

            if let Some(size) = response_size_bytes {
                record.response_size_bytes = Some(size);
            }

            if let Some(err) = error {
                record.error = Some(err);
            }
        }

        Ok(())
    }

    async fn increment_attempt(&self, command_id: &str) -> Result<u32> {
        let mut commands = self.commands.write().await;

        if let Some(record) = commands.get_mut(command_id) {
            record.attempt += 1;
            Ok(record.attempt)
        } else {
            Ok(1) // Default if not found
        }
    }
}
