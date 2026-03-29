use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use alien_core::{
    DeploymentState, EnvironmentInfo, EnvironmentVariable, ManagementConfig, Platform,
    RuntimeMetadata, StackSettings, StackState,
};
use alien_error::AlienError;

/// A deployment record as stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentRecord {
    pub id: String,
    pub name: String,
    pub deployment_group_id: String,
    pub platform: Platform,
    pub status: String,
    pub stack_settings: StackSettings,
    pub stack_state: Option<StackState>,
    pub environment_info: Option<EnvironmentInfo>,
    pub runtime_metadata: Option<RuntimeMetadata>,
    pub current_release_id: Option<String>,
    pub desired_release_id: Option<String>,
    pub user_environment_variables: Option<Vec<EnvironmentVariable>>,
    /// Management config from the platform API (platform mode only).
    /// In standalone/E2E mode this is None — the credential resolver derives it from bindings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_config: Option<ManagementConfig>,
    pub retry_requested: bool,
    pub locked_by: Option<String>,
    pub locked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub error: Option<serde_json::Value>,
}

/// Parameters for creating a new deployment.
#[derive(Debug, Clone)]
pub struct CreateDeploymentParams {
    pub name: String,
    pub deployment_group_id: String,
    pub platform: Platform,
    pub stack_settings: StackSettings,
    pub environment_variables: Option<Vec<EnvironmentVariable>>,
}

/// A deployment group record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentGroupRecord {
    pub id: String,
    pub name: String,
    pub max_deployments: i64,
    pub deployment_count: i64,
    pub created_at: DateTime<Utc>,
}

/// Parameters for creating a deployment group.
#[derive(Debug, Clone)]
pub struct CreateDeploymentGroupParams {
    pub name: String,
    pub max_deployments: i64,
}

/// Filter for listing deployments.
#[derive(Debug, Clone, Default)]
pub struct DeploymentFilter {
    pub deployment_group_id: Option<String>,
    pub statuses: Option<Vec<String>>,
    pub platforms: Option<Vec<Platform>>,
    pub limit: Option<u32>,
}

/// Result of acquiring deployments for processing.
#[derive(Debug, Clone)]
pub struct AcquiredDeployment {
    pub deployment: DeploymentRecord,
}

/// Data for reconciling a deployment after processing.
#[derive(Debug, Clone)]
pub struct ReconcileData {
    pub deployment_id: String,
    pub session: String,
    pub state: DeploymentState,
    pub update_heartbeat: bool,
    pub error: Option<serde_json::Value>,
}

/// Persistence for deployments and deployment groups.
#[async_trait]
pub trait DeploymentStore: Send + Sync {
    // --- Deployment CRUD ---

    async fn create_deployment(
        &self,
        params: CreateDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError>;

    async fn get_deployment(&self, id: &str) -> Result<Option<DeploymentRecord>, AlienError>;

    async fn list_deployments(
        &self,
        filter: &DeploymentFilter,
    ) -> Result<Vec<DeploymentRecord>, AlienError>;

    async fn delete_deployment(&self, id: &str) -> Result<(), AlienError>;

    async fn set_retry_requested(&self, id: &str) -> Result<(), AlienError>;

    async fn set_redeploy(&self, id: &str) -> Result<(), AlienError>;

    /// Set desired_release_id on a specific deployment.
    async fn set_deployment_desired_release(
        &self,
        deployment_id: &str,
        release_id: &str,
    ) -> Result<(), AlienError>;

    /// Set desired_release_id on eligible deployments when a new release is created.
    async fn set_desired_release(
        &self,
        release_id: &str,
        platform: Option<Platform>,
    ) -> Result<(), AlienError>;

    // --- Deployment loop coordination ---

    /// Acquire deployments that need processing. Sets locked_by on matched rows.
    async fn acquire(
        &self,
        session: &str,
        filter: &DeploymentFilter,
        limit: u32,
    ) -> Result<Vec<AcquiredDeployment>, AlienError>;

    /// Write new state back after processing.
    async fn reconcile(&self, data: ReconcileData) -> Result<DeploymentRecord, AlienError>;

    /// Release lock on a deployment.
    async fn release(&self, deployment_id: &str, session: &str) -> Result<(), AlienError>;

    // --- Deployment groups ---

    async fn create_deployment_group(
        &self,
        params: CreateDeploymentGroupParams,
    ) -> Result<DeploymentGroupRecord, AlienError>;

    /// Create a deployment group with a specific ID (for dev mode well-known IDs).
    async fn create_deployment_group_with_id(
        &self,
        id: &str,
        params: CreateDeploymentGroupParams,
    ) -> Result<DeploymentGroupRecord, AlienError>;

    async fn get_deployment_group(
        &self,
        id: &str,
    ) -> Result<Option<DeploymentGroupRecord>, AlienError>;

    async fn list_deployment_groups(&self) -> Result<Vec<DeploymentGroupRecord>, AlienError>;
}
