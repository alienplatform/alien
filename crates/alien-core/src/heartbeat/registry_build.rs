use super::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum ArtifactRegistryHeartbeatData {
    AwsEcr(AwsEcrArtifactRegistryHeartbeatData),
    GcpArtifactRegistry(GcpArtifactRegistryHeartbeatData),
    AzureContainerRegistry(AzureContainerRegistryHeartbeatData),
    Local(LocalArtifactRegistryHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRegistryHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsEcrArtifactRegistryHeartbeatData {
    pub status: ArtifactRegistryHeartbeatStatus,
    pub registry_id: String,
    pub region: String,
    pub registry_uri: String,
    pub repository_prefix: String,
    pub pull_role_arn: Option<String>,
    pub push_role_arn: Option<String>,
    pub repository_count: u32,
    pub repositories_truncated: bool,
    pub repositories: Vec<AwsEcrRepositoryHeartbeatData>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsEcrRepositoryHeartbeatData {
    pub repository_arn: String,
    pub registry_id: String,
    pub repository_name: String,
    pub repository_uri: String,
    pub created_at: f64,
    pub image_tag_mutability: Option<String>,
    pub scan_on_push: Option<bool>,
    pub encryption_type: Option<String>,
    pub kms_key_present: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpArtifactRegistryHeartbeatData {
    pub status: ArtifactRegistryHeartbeatStatus,
    pub project_id: String,
    pub location: String,
    pub repository_id: String,
    pub name: Option<String>,
    pub format: Option<String>,
    pub mode: Option<String>,
    pub description: Option<String>,
    pub label_count: u32,
    pub cleanup_policy_count: u32,
    pub cleanup_policy_dry_run: Option<bool>,
    pub kms_key_name_present: bool,
    pub size_bytes: Option<String>,
    pub satisfies_pzs: Option<bool>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
    pub iam_policy_etag_present: bool,
    pub iam_binding_count: u32,
    pub iam_roles: Vec<String>,
    pub pull_service_account_email: Option<String>,
    pub push_service_account_email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureContainerRegistryHeartbeatData {
    pub status: ArtifactRegistryHeartbeatStatus,
    pub name: String,
    pub resource_id: Option<String>,
    pub resource_group: String,
    pub location: String,
    pub type_: Option<String>,
    pub login_server: Option<String>,
    pub sku_name: String,
    pub sku_tier: Option<String>,
    pub provisioning_state: Option<String>,
    pub admin_user_enabled: bool,
    pub anonymous_pull_enabled: bool,
    pub public_network_access: String,
    pub network_rule_bypass_options: String,
    pub network_rule_default_action: Option<String>,
    pub ip_rule_count: u32,
    pub encryption_status: Option<String>,
    pub encryption_key_vault_uri_present: bool,
    pub encryption_key_identifier_present: bool,
    pub policies_present: bool,
    pub policy_count: u32,
    pub private_endpoint_connection_count: u32,
    pub data_endpoint_enabled: Option<bool>,
    pub data_endpoint_host_names: Vec<String>,
    pub zone_redundancy: String,
    pub creation_date: Option<String>,
    pub managed_tag_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalArtifactRegistryHeartbeatData {
    pub status: ArtifactRegistryHeartbeatStatus,
    pub registry_url: String,
    pub reachable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum BuildHeartbeatData {
    AwsCodeBuild(AwsCodeBuildHeartbeatData),
    GcpCloudBuild(GcpCloudBuildHeartbeatData),
    AzureContainerApps(AzureContainerAppsBuildHeartbeatData),
    KubernetesJob(KubernetesBuildHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BuildHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsCodeBuildHeartbeatData {
    pub status: BuildHeartbeatStatus,
    pub project_name: String,
    pub project_arn: Option<String>,
    pub description: Option<String>,
    pub source_type: Option<String>,
    pub artifacts_type: Option<String>,
    pub artifacts_encryption_disabled: Option<bool>,
    pub environment_type: Option<String>,
    pub environment_image: Option<String>,
    pub compute_type: Option<String>,
    pub image_pull_credentials_type: Option<String>,
    pub privileged_mode: Option<bool>,
    pub environment_variable_count: u32,
    pub service_role_present: bool,
    pub encryption_key_present: bool,
    pub cloud_watch_logs_status: Option<String>,
    pub s3_logs_status: Option<String>,
    pub timeout_in_minutes: Option<i32>,
    pub queued_timeout_in_minutes: Option<i32>,
    pub created: Option<f64>,
    pub last_modified: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpCloudBuildHeartbeatData {
    pub status: BuildHeartbeatStatus,
    pub project_id: String,
    pub location: String,
    pub build_config_id: String,
    pub service_account: Option<String>,
    pub environment_variable_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureContainerAppsBuildHeartbeatData {
    pub status: BuildHeartbeatStatus,
    pub managed_environment_id: String,
    pub resource_group_name: String,
    pub managed_identity_id: Option<String>,
    pub resource_prefix: Option<String>,
    pub environment_variable_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesBuildHeartbeatData {
    pub status: BuildHeartbeatStatus,
    pub job_name: String,
    pub namespace: String,
    pub active: Option<i32>,
    pub succeeded: Option<i32>,
    pub failed: Option<i32>,
    pub start_time: Option<DateTime<Utc>>,
    pub completion_time: Option<DateTime<Utc>>,
    pub condition_count: u32,
    pub image_digest: Option<String>,
    pub events: Vec<KubernetesEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum ServiceActivationHeartbeatData {
    GcpServiceUsage(GcpServiceUsageActivationHeartbeatData),
    AzureResourceProvider(AzureResourceProviderActivationHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServiceActivationHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

impl Default for ServiceActivationHeartbeatStatus {
    fn default() -> Self {
        Self {
            health: ObservedHealth::Healthy,
            lifecycle: ProviderLifecycleState::Running,
            message: None,
            stale: false,
            partial: false,
            collection_issues: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpServiceUsageActivationHeartbeatData {
    pub status: ServiceActivationHeartbeatStatus,
    pub project_id: String,
    pub service_name: String,
    pub service_resource_name: Option<String>,
    pub title: Option<String>,
    pub state: Option<String>,
    pub enabled: bool,
    pub last_operation_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureResourceProviderActivationHeartbeatData {
    pub status: ServiceActivationHeartbeatStatus,
    pub namespace: String,
    pub provider_id: Option<String>,
    pub registration_state: Option<String>,
    pub registration_policy: Option<String>,
    pub resource_type_count: u32,
    pub registered: bool,
}
