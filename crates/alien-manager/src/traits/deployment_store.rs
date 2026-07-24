use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use alien_core::{
    import::ImportSourceKind, sync::OperatorCapabilityReport, DeploymentConfig, DeploymentModel,
    DeploymentState, DeploymentStatus, EnvironmentInfo, EnvironmentVariable, ManagementConfig,
    ObservedInventoryBatch, Platform, ResourceHeartbeat, RuntimeMetadata, StackSettings,
    StackState,
};
use alien_error::AlienError;

pub(crate) fn deployment_status_from_record(status: &str) -> Option<DeploymentStatus> {
    serde_json::from_value(serde_json::Value::String(status.to_string())).ok()
}

/// A deployment record as stored in the database.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRecord {
    pub id: String,
    /// Workspace this deployment belongs to. Always `"default"` in OSS.
    #[serde(default = "super::default_string")]
    pub workspace_id: String,
    /// Project this deployment belongs to. Always `"default"` in OSS.
    #[serde(default = "super::default_string")]
    pub project_id: String,
    pub name: String,
    pub deployment_group_id: String,
    pub platform: Platform,
    pub deployment_protocol_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_platform: Option<Platform>,
    pub status: String,
    /// `None` when a record is built from a source that doesn't carry stack
    /// settings; records produced by this crate's stores always set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_settings: Option<StackSettings>,
    pub stack_state: Option<StackState>,
    pub environment_info: Option<EnvironmentInfo>,
    pub runtime_metadata: Option<RuntimeMetadata>,
    pub current_release_id: Option<String>,
    pub desired_release_id: Option<String>,
    /// Setup source that created this deployment, if it was imported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_source: Option<ImportSourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_metadata: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_fingerprint_version: Option<u32>,
    pub user_environment_variables: Option<Vec<EnvironmentVariable>>,
    /// Management config from the platform API (platform mode only).
    /// In standalone/E2E mode this is None — the credential resolver derives it from bindings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_config: Option<ManagementConfig>,
    /// Full config supplied by an external control plane.
    ///
    /// This can include deployment secrets, so records may deserialize it from
    /// trusted control-plane responses but never serialize it back to clients.
    #[serde(default, skip_serializing)]
    pub deployment_config: Option<DeploymentConfig>,
    /// Raw deployment token for proxy pull auth.
    /// Set during deployment creation. Used by the deployment loop to
    /// configure registry credentials (Container App secrets, K8s imagePullSecrets).
    #[serde(default, skip_serializing)]
    pub deployment_token: Option<String>,
    /// Deployer-provided stack input values, keyed by input id. Gated live
    /// resources resolve against these on every reconcile; without them a
    /// stored deployment would fall back to declared defaults.
    #[serde(default)]
    pub input_values: HashMap<String, serde_json::Value>,
    pub retry_requested: bool,
    pub locked_by: Option<String>,
    pub locked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub error: Option<serde_json::Value>,
}

impl std::fmt::Debug for DeploymentRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeploymentRecord")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("deployment_group_id", &self.deployment_group_id)
            .field("platform", &self.platform)
            .field(
                "deployment_protocol_version",
                &self.deployment_protocol_version,
            )
            .field("status", &self.status)
            .field("stack_settings", &self.stack_settings)
            .field("stack_state", &self.stack_state)
            .field("environment_info", &self.environment_info)
            .field("runtime_metadata", &self.runtime_metadata)
            .field("current_release_id", &self.current_release_id)
            .field("desired_release_id", &self.desired_release_id)
            .field("import_source", &self.import_source)
            .field("setup_method", &self.setup_method)
            .field("setup_metadata", &self.setup_metadata)
            .field("setup_target", &self.setup_target)
            .field("setup_fingerprint", &self.setup_fingerprint)
            .field("setup_fingerprint_version", &self.setup_fingerprint_version)
            .field(
                "user_environment_variables",
                &self.user_environment_variables,
            )
            .field("management_config", &self.management_config)
            .field(
                "deployment_config",
                &self.deployment_config.as_ref().map(|_| "[PRESENT]"),
            )
            .field(
                "deployment_token",
                &self.deployment_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("input_values", &self.input_values)
            .field("retry_requested", &self.retry_requested)
            .field("locked_by", &self.locked_by)
            .field("locked_at", &self.locked_at)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("error", &self.error)
            .finish()
    }
}

/// Parameters for creating a new deployment.
#[derive(Debug, Clone)]
pub struct CreateDeploymentParams {
    pub name: String,
    pub deployment_group_id: String,
    pub platform: Platform,
    pub deployment_protocol_version: u32,
    pub base_platform: Option<Platform>,
    pub stack_settings: StackSettings,
    pub stack_state: Option<StackState>,
    pub environment_variables: Option<Vec<EnvironmentVariable>>,
    /// Optional generated-domain subdomain requested by the deployer.
    pub public_subdomain: Option<String>,
    /// Stack input values collected before deployment creation.
    pub input_values: HashMap<String, serde_json::Value>,
    /// Raw deployment token for proxy pull auth.
    pub deployment_token: Option<String>,
}

/// Parameters for creating a deployment whose layer-2 stack state was produced
/// by a setup artifact (CloudFormation, Terraform, Helm).
#[derive(Debug, Clone)]
pub struct CreateImportedDeploymentParams {
    pub name: String,
    pub deployment_group_id: String,
    pub platform: Platform,
    pub deployment_protocol_version: u32,
    pub base_platform: Option<Platform>,
    pub stack_settings: StackSettings,
    pub stack_state: StackState,
    pub environment_info: Option<EnvironmentInfo>,
    pub runtime_metadata: RuntimeMetadata,
    /// Initial status — imported deployments normally start at
    /// `"provisioning"` so the manager can complete layer-3 runtime work.
    pub status: String,
    pub current_release_id: Option<String>,
    pub desired_release_id: Option<String>,
    pub import_source: Option<ImportSourceKind>,
    pub setup_metadata: Option<serde_json::Value>,
    pub setup_target: String,
    pub setup_fingerprint: String,
    pub setup_fingerprint_version: u32,
    pub deployment_token: Option<String>,
    pub management_config: Option<ManagementConfig>,
    /// Stack input values collected by setup artifacts.
    pub input_values: HashMap<String, serde_json::Value>,
}

/// Import-owned fields replaced when setup re-registers a deployment.
#[derive(Debug, Clone)]
pub struct UpdateImportedDeploymentParams {
    pub stack_state: StackState,
    pub environment_info: Option<EnvironmentInfo>,
    pub runtime_metadata: RuntimeMetadata,
    pub setup_metadata: Option<serde_json::Value>,
    pub current_release_id: Option<String>,
    pub setup_target: String,
    pub setup_fingerprint: String,
    pub setup_fingerprint_version: u32,
    /// Move the deployment to `update-pending` in the same write as the import data.
    pub schedule_reconciliation: bool,
    /// Deployer stack input values carried by the re-import; they overwrite
    /// the stored map — the edit surface for gate flips.
    pub input_values: HashMap<String, serde_json::Value>,
}

/// A deployment group record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentGroupRecord {
    pub id: String,
    /// Workspace this deployment group belongs to. Always `"default"` in OSS.
    #[serde(default = "super::default_string")]
    pub workspace_id: String,
    /// Project this deployment group belongs to. Always `"default"` in OSS.
    #[serde(default = "super::default_string")]
    pub project_id: String,
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
    pub name: Option<String>,
    pub deployment_ids: Option<Vec<String>>,
    pub statuses: Option<Vec<String>>,
    pub platforms: Option<Vec<Platform>>,
    pub setup_method: Option<String>,
    pub acquire_mode: Option<DeploymentAcquireMode>,
    pub deployment_model: Option<DeploymentModel>,
    pub limit: Option<u32>,
}

/// Ownership mode for deployment acquisition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentAcquireMode {
    Runtime,
    SetupRun,
    SetupTeardown,
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
    pub suggested_delay_ms: Option<u64>,
    pub heartbeats: Vec<ResourceHeartbeat>,
    pub observed_inventory_batches: Vec<ObservedInventoryBatch>,
    pub capabilities: Vec<OperatorCapabilityReport>,
    pub operator_version: Option<String>,
}

/// Persistence for deployments and deployment groups.
///
/// Every method takes `caller: &Subject`. Single-tenant impls
/// (`SqliteDeploymentStore`) ignore it. Multi-tenant embedders that proxy
/// through an upstream API can use `caller.bearer_token` to forward the
/// original request's authentication, so cross-tenant calls remain gated
/// by the inbound caller's scope rather than the embedder's own service
/// credential. Internal callers without an inbound request (background
/// loops, startup hooks) pass [`Subject::system`]; embedders that need to
/// know whether to fall back to a service credential must check
/// [`Subject::is_system`] explicitly — never use an empty `bearer_token`
/// as an implicit fallback signal, since a buggy validator could otherwise
/// silently escalate privilege.
///
/// `caller` is metadata-about-who; the per-method `params` / IDs are
/// data-to-act-on — never conflate the two on a single struct.
#[async_trait]
pub trait DeploymentStore: Send + Sync {
    // --- Deployment CRUD ---

    async fn create_deployment(
        &self,
        caller: &crate::auth::Subject,
        params: CreateDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError>;

    /// Persist a deployment whose stack state was produced by a setup artifact
    /// (CloudFormation custom resource, Terraform provider, or Helm chart).
    /// Same idempotency contract as
    /// [`Self::get_deployment_by_name`] — callers should look up the existing
    /// record first; this method only handles the "doesn't exist yet" path.
    ///
    /// Implementations that proxy to an upstream API (platform mode) translate
    /// this into the upstream's import endpoint; SQLite-backed standalone
    /// stores insert directly with `stack_state` populated and `status` set
    /// from the params.
    async fn create_with_state(
        &self,
        caller: &crate::auth::Subject,
        params: CreateImportedDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError>;

    /// Merge import-owned `stack_state` resources (and pin
    /// `current_release_id`) on an existing
    /// imported deployment. Used by `POST /v1/stack/import` when the request
    /// re-imports a deployment that was created by an earlier call —
    /// CloudFormation Update events, Terraform refresh+apply cycles, and Helm
    /// upgrades all fire this path. Implementations must update only
    /// import-owned fields/resources from the import payload and leave fields outside the
    /// import contract (status, deployment token, environment variables, …)
    /// untouched. `setup_metadata` advances the durable replay baseline for
    /// the registration operation that produced this update.
    async fn update_imported_stack_state(
        &self,
        caller: &crate::auth::Subject,
        deployment_id: &str,
        params: UpdateImportedDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError>;

    async fn get_deployment(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<Option<DeploymentRecord>, AlienError>;

    async fn get_deployment_by_name(
        &self,
        caller: &crate::auth::Subject,
        deployment_group_id: &str,
        name: &str,
    ) -> Result<Option<DeploymentRecord>, AlienError>;

    async fn list_deployments(
        &self,
        caller: &crate::auth::Subject,
        filter: &DeploymentFilter,
    ) -> Result<Vec<DeploymentRecord>, AlienError>;

    async fn delete_deployment(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<(), AlienError>;

    async fn set_delete_pending(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<(), AlienError>;

    async fn set_retry_requested(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<(), AlienError>;

    async fn set_redeploy(&self, caller: &crate::auth::Subject, id: &str)
        -> Result<(), AlienError>;

    /// Set desired_release_id on a specific deployment.
    async fn set_deployment_desired_release(
        &self,
        caller: &crate::auth::Subject,
        deployment_id: &str,
        release_id: &str,
    ) -> Result<(), AlienError>;

    /// Set desired_release_id on eligible deployments when a new release is created.
    async fn set_desired_release(
        &self,
        caller: &crate::auth::Subject,
        release_id: &str,
        platform: Option<Platform>,
    ) -> Result<(), AlienError>;

    // --- Deployment loop coordination ---

    /// Acquire deployments that need processing. Sets locked_by on matched rows.
    async fn acquire(
        &self,
        caller: &crate::auth::Subject,
        session: &str,
        filter: &DeploymentFilter,
        limit: u32,
    ) -> Result<Vec<AcquiredDeployment>, AlienError>;

    /// Write new state back after processing.
    async fn reconcile(
        &self,
        caller: &crate::auth::Subject,
        data: ReconcileData,
    ) -> Result<DeploymentRecord, AlienError>;

    /// Release lock on a deployment.
    async fn release(
        &self,
        caller: &crate::auth::Subject,
        deployment_id: &str,
        session: &str,
    ) -> Result<(), AlienError>;

    // --- Deployment groups ---

    async fn create_deployment_group(
        &self,
        caller: &crate::auth::Subject,
        params: CreateDeploymentGroupParams,
    ) -> Result<DeploymentGroupRecord, AlienError>;

    /// Create a deployment group with a specific ID (for dev mode well-known IDs).
    async fn create_deployment_group_with_id(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
        params: CreateDeploymentGroupParams,
    ) -> Result<DeploymentGroupRecord, AlienError>;

    async fn get_deployment_group(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<Option<DeploymentGroupRecord>, AlienError>;

    async fn list_deployment_groups(
        &self,
        caller: &crate::auth::Subject,
    ) -> Result<Vec<DeploymentGroupRecord>, AlienError>;

    /// Clean up stale locks from crashed sessions. Called on startup with
    /// `Subject::system()` from the standalone binary; embedders that mount
    /// the manager's startup hook into a request context can pass the request
    /// caller instead.
    async fn cleanup_stale_locks(&self, _caller: &crate::auth::Subject) -> Result<u64, AlienError> {
        Ok(0)
    }
}
