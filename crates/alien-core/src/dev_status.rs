//! Types for `alien dev` status file output

use crate::DeploymentStatus;
use alien_error::AlienError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Overall status of the dev server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DevStatus {
    /// Dev server process ID
    pub pid: u32,
    /// Platform (always "local" for dev server)
    pub platform: String,
    /// Stack ID (always "dev" for dev server)
    pub stack_id: String,
    /// Path to state directory
    pub state_dir: String,
    /// Dev server API URL (e.g., http://localhost:9090)
    pub api_url: String,
    /// ISO 8601 timestamp when dev server started
    pub started_at: String,
    /// Overall dev server status
    pub status: DevStatusState,
    /// Agents being managed by this dev server (keyed by agent name)
    pub agents: HashMap<String, AgentStatus>,
    /// ISO 8601 timestamp of last status update
    pub last_updated: String,
    /// Global error if dev server itself has an error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AlienError>,
}

/// Overall dev server status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum DevStatusState {
    /// Dev server is starting up
    Initializing,
    /// At least one agent is running
    Ready,
    /// Dev server or agents encountered errors
    Error,
    /// Dev server is shutting down
    ShuttingDown,
}

/// Status of a single agent in the dev server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    /// Agent ID (e.g., ag_xyz123)
    pub id: String,
    /// Agent name (from --agent-name flag)
    pub name: String,
    /// Deployment status (running, provisioning, etc.)
    pub status: DeploymentStatus,
    /// Resources deployed by this agent (keyed by resource name)
    pub resources: HashMap<String, DevResourceInfo>,
    /// ISO 8601 timestamp when agent was created
    pub created_at: String,
    /// Error if this agent has failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AlienError>,
}

/// Information about a deployed resource
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DevResourceInfo {
    /// Resource URL (e.g., http://localhost:8080)
    pub url: String,
    /// Resource type ("function" | "container")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
}
