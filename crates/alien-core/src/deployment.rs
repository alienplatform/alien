use crate::{
    ExternalBindings, ImagePullCredentials, ManagementConfig, Platform, Stack, StackSettings,
    StackState,
};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Deployment status in the deployment lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentStatus {
    Pending,
    InitialSetup,
    InitialSetupFailed,
    Provisioning,
    ProvisioningFailed,
    Running,
    RefreshFailed,
    UpdatePending,
    Updating,
    UpdateFailed,
    DeletePending,
    Deleting,
    DeleteFailed,
    Deleted,
}

impl DeploymentStatus {
    /// Check if deployment is synced (current state matches desired state).
    ///
    /// When synced, no more deployment steps are needed *for the current operation*.
    /// Note: This doesn't mean the deployment is "done forever":
    /// - `Running` → heartbeats continue, updates can come
    /// - `*Failed` → can be retried
    /// - `Deleted` → can be recreated
    ///
    /// "Synced" means: "we've reached the goal of the current deployment phase"
    pub fn is_synced(&self) -> bool {
        matches!(
            self,
            DeploymentStatus::Running
                | DeploymentStatus::InitialSetupFailed
                | DeploymentStatus::ProvisioningFailed
                | DeploymentStatus::UpdateFailed
                | DeploymentStatus::DeleteFailed
                | DeploymentStatus::RefreshFailed
                | DeploymentStatus::Deleted
        )
    }

    /// Check if deployment is in a failed state that requires retry to proceed.
    pub fn is_failed(&self) -> bool {
        matches!(
            self,
            DeploymentStatus::InitialSetupFailed
                | DeploymentStatus::ProvisioningFailed
                | DeploymentStatus::UpdateFailed
                | DeploymentStatus::DeleteFailed
                | DeploymentStatus::RefreshFailed
        )
    }
}

/// Release metadata
///
/// Identifies a specific release version and includes the stack definition.
/// The deployment engine uses this to track which release is currently deployed
/// and which is the target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    /// Release ID (e.g., rel_xyz)
    pub release_id: String,
    /// Version string (e.g., 2.1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Short description of the release
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Stack definition for this release
    pub stack: Stack,
}

/// AWS-specific environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsEnvironmentInfo {
    /// AWS account ID
    pub account_id: String,
    /// AWS region
    pub region: String,
}

/// GCP-specific environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpEnvironmentInfo {
    /// GCP project number (e.g., "123456789012")
    pub project_number: String,
    /// GCP project ID (e.g., "my-project")
    pub project_id: String,
    /// GCP region
    pub region: String,
}

/// Azure-specific environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureEnvironmentInfo {
    /// Azure tenant ID
    pub tenant_id: String,
    /// Azure subscription ID
    pub subscription_id: String,
    /// Azure location/region
    pub location: String,
}

/// Local platform environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalEnvironmentInfo {
    /// Hostname of the machine running the deployment
    pub hostname: String,
    /// Operating system (e.g., "linux", "macos", "windows")
    pub os: String,
    /// Architecture (e.g., "x86_64", "aarch64")
    pub arch: String,
}

/// Test platform environment information (mock)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct TestEnvironmentInfo {
    /// Test identifier for this environment
    pub test_id: String,
}

/// Platform-specific environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "platform")]
pub enum EnvironmentInfo {
    /// AWS environment information
    Aws(AwsEnvironmentInfo),
    /// GCP environment information
    Gcp(GcpEnvironmentInfo),
    /// Azure environment information
    Azure(AzureEnvironmentInfo),
    /// Local platform environment information
    Local(LocalEnvironmentInfo),
    /// Test platform environment information (mock)
    Test(TestEnvironmentInfo),
}

impl EnvironmentInfo {
    /// Get the platform for this environment info
    pub fn platform(&self) -> Platform {
        match self {
            EnvironmentInfo::Aws(_) => Platform::Aws,
            EnvironmentInfo::Gcp(_) => Platform::Gcp,
            EnvironmentInfo::Azure(_) => Platform::Azure,
            EnvironmentInfo::Local(_) => Platform::Local,
            EnvironmentInfo::Test(_) => Platform::Test,
        }
    }
}

/// Runtime metadata for deployment
///
/// Stores deployment state that needs to persist across step calls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetadata {
    /// Hash of the environment variables snapshot that was last synced to the vault
    /// Used to avoid redundant sync operations during incremental deployment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_synced_env_vars_hash: Option<String>,

    /// The prepared (mutated) stack from the last successful deployment phase
    /// This is the stack AFTER mutations have been applied (with service accounts, vault, etc.)
    /// Used for compatibility checks during updates to compare mutated stacks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepared_stack: Option<Stack>,
}

/// Certificate status in the certificate lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CertificateStatus {
    Pending,
    Issued,
    Renewing,
    RenewalFailed,
    Failed,
    Deleting,
}

/// DNS record status in the DNS lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum DnsRecordStatus {
    Pending,
    Active,
    Updating,
    Deleting,
    Failed,
}

/// Certificate and DNS metadata for a public resource.
///
/// Includes decrypted certificate data for issued certificates.
/// Private keys are deployment-scoped secrets (like environment variables).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceDomainInfo {
    /// Fully qualified domain name.
    pub fqdn: String,
    /// Certificate ID (for tracking/logging).
    pub certificate_id: String,
    /// Current certificate status
    pub certificate_status: CertificateStatus,
    /// Current DNS record status
    pub dns_status: DnsRecordStatus,
    /// Last DNS error message. Present when DNS previously failed, even if status
    /// was reset to pending for retry. Used to surface actionable error context
    /// in WaitingForDns failure messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_error: Option<String>,
    /// Full PEM certificate chain (only present if status is "issued").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_chain: Option<String>,
    /// Decrypted private key (only present if status is "issued").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// ISO 8601 timestamp when certificate was issued (for renewal detection).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
}

/// Domain metadata for auto-managed public resources (no private keys).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DomainMetadata {
    /// Base domain for auto-generated domains (e.g., "vpc.direct").
    pub base_domain: String,
    /// Deployment public subdomain (e.g., "k8f2j3").
    pub public_subdomain: String,
    /// Hosted zone ID for DNS records.
    pub hosted_zone_id: String,
    /// Metadata per resource ID.
    pub resources: HashMap<String, ResourceDomainInfo>,
}

/// Deployment state
///
/// Represents the current state of deployed infrastructure, including release tracking.
/// This is platform-agnostic - no backend IDs or database relationships.
///
/// The deployment engine manages releases internally: when a deployment succeeds,
/// it promotes `target_release` to `current_release` and clears `target_release`.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentState {
    /// Current lifecycle phase
    pub status: DeploymentStatus,
    /// Target cloud platform (AWS, GCP, Azure, Kubernetes)
    pub platform: Platform,
    /// Currently deployed release (None for first deployment)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_release: Option<ReleaseInfo>,
    /// Target release to deploy (None when synced with current)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_release: Option<ReleaseInfo>,
    /// Infrastructure resource tracking (which resources exist, their status, outputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_state: Option<StackState>,
    /// Cloud account details (account ID, project number, region)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_info: Option<EnvironmentInfo>,
    /// Deployment-specific data (prepared stacks, phase tracking, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_metadata: Option<RuntimeMetadata>,
    /// Whether a retry has been requested for a failed deployment
    /// When true and status is a failed state, the deployment system will retry failed resources
    #[serde(default, skip_serializing_if = "is_false")]
    pub retry_requested: bool,
}

/// Type of environment variable
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentVariableType {
    /// Plain variable (injected directly into function config)
    Plain,
    /// Secret variable (stored in vault, loaded at runtime)
    Secret,
}

/// Environment variable for deployment
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    /// Variable name
    pub name: String,
    /// Variable value (decrypted - deployment has access to decryption keys)
    pub value: String,
    /// Variable type (plain or secret)
    #[serde(rename = "type")]
    pub var_type: EnvironmentVariableType,
    /// Target resource patterns (null = all resources, Some = wildcard patterns)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_resources: Option<Vec<String>>,
}

/// Snapshot of environment variables at a point in time
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariablesSnapshot {
    /// Environment variables in the snapshot
    pub variables: Vec<EnvironmentVariable>,
    /// Deterministic hash of all variables (for change detection)
    pub hash: String,
    /// ISO 8601 timestamp when snapshot was created
    pub created_at: String,
}

/// Artifact registry configuration for pulling container images.
///
/// Used when the deployment needs to pull images from a manager's artifact registry.
/// This is required for Local platform and can optionally be used by cloud platforms
/// instead of native registry mechanisms (ECR/GCR/ACR).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRegistryConfig {
    /// Manager base URL for fetching credentials and accessing the registry
    pub manager_url: String,
    /// Optional authentication token (JWT) for manager API access
    /// When present, must be included in Authorization header as "Bearer {token}"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
}

/// OTLP log export configuration for a deployment.
///
/// When set, all compute workloads (containers and horizond VM workers) export
/// their logs to the given endpoint via OTLP/HTTP.
///
/// The `logs_auth_header` is stored as plain text in DeploymentConfig because
/// alien-runtime reads `OTEL_EXPORTER_OTLP_HEADERS` at tracing-init time,
/// before vault secrets load. For horizond, the infra controller writes the
/// same value to the cloud vault (same pattern as the machine token) and the
/// startup script fetches it at boot via IAM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OtlpConfig {
    /// Full OTLP logs endpoint URL.
    /// Example: "https://<manager-host>/v1/logs"
    pub logs_endpoint: String,
    /// Auth header value in "key=value,..." format used for container OTLP env var injection.
    ///
    /// `alien-deployment` injects this as the `OTEL_EXPORTER_OTLP_HEADERS` plain env var
    /// into all containers. It must be plain (not a vault secret) because alien-runtime
    /// reads `OTEL_EXPORTER_OTLP_HEADERS` at tracing-init time, before vault secrets load.
    ///
    /// horizond VM workers do NOT use this field directly. The ContainerCluster infra
    /// controller writes the same value to the cloud vault (GCP: Secret Manager,
    /// AWS: Secrets Manager, Azure: Key Vault) and the startup script fetches it at
    /// boot via IAM — the same pattern as the machine token.
    ///
    /// Example: "authorization=Bearer <write-token>"
    pub logs_auth_header: String,
    /// Full OTLP metrics endpoint URL (optional).
    /// When set, horizond exports its own VM/container orchestration metrics here.
    /// Example: "https://api.axiom.co/v1/metrics"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics_endpoint: Option<String>,
    /// Auth header value for the metrics endpoint in "key=value,..." format (optional).
    ///
    /// When absent, `logs_auth_header` is reused for metrics — suitable when the same
    /// credential covers both signals. When present (e.g. Axiom with separate datasets),
    /// this value is used exclusively for metrics.
    ///
    /// Example: "authorization=Bearer <token>,x-axiom-dataset=<metrics-dataset>"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics_auth_header: Option<String>,
}

/// Configuration for a single Horizon cluster.
///
/// Contains the cluster ID and management token needed to interact with
/// the Horizon control plane API for container operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonClusterConfig {
    /// Cluster ID (deterministic: workspace/project/deployment/resourceid)
    pub cluster_id: String,

    /// Management token for API access (hm_...)
    /// Used by alien-deployment controllers to create/update containers
    pub management_token: String,
    // Note: Machine token (hj_...) is NOT in DeploymentConfig
    // It's added to environmentVariables snapshot as a built-in secret variable
    // and synced to vault (Parameter Store/Secret Manager/Key Vault)
}

/// Horizon configuration for container orchestration.
///
/// Contains all the information needed for Alien to interact with Horizon
/// clusters during deployment. Each ContainerCluster resource gets its own
/// entry in the clusters map.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonConfig {
    /// Horizon API base URL (e.g., "https://horizon.alien.dev")
    pub url: String,

    /// Base URL for downloading the horizond binary, without arch suffix.
    ///
    /// Each cloud controller appends `/linux-{arch}/horizond` to construct the
    /// final download URL used in VM startup scripts.
    ///
    /// Production example: "https://releases.alien.dev/horizond/v0.3.0"
    /// Dev example (ngrok): "https://abc123.ngrok.io"
    pub horizond_download_base_url: String,

    /// ETag of the horizond binary fetched from the releases server — used as a
    /// change-detection signal only. nginx auto-generates ETags from mtime+size,
    /// so every `cargo zigbuild` changes this value and triggers a rolling update.
    ///
    /// Optional: when absent (releases server unreachable), change detection
    /// falls back to URL-only (sufficient for versioned production releases).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizond_binary_hash: Option<String>,

    /// Cluster configurations (one per ContainerCluster resource)
    /// Key: ContainerCluster resource ID from stack
    /// Value: Cluster ID and management token for that cluster
    pub clusters: HashMap<String, HorizonClusterConfig>,
}

/// Compute backend for Container and Function resources.
///
/// Determines how compute workloads are orchestrated on cloud platforms.
/// When None, the platform default is used (Horizon for cloud platforms).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ComputeBackend {
    /// VMs with Horizon orchestration (default for cloud platforms)
    Horizon(HorizonConfig),
    // Future backends:
    // /// Deploy to existing Kubernetes cluster (EKS/GKE/AKS)
    // Kubernetes(KubernetesCredentials),
    // /// AWS ECS Fargate (serverless containers)
    // EcsFargate,
}

/// Deployment configuration
///
/// Configuration for how to perform the deployment.
/// Note: Credentials (ClientConfig) are passed separately to step() function.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentConfig {
    /// User-customizable deployment settings (network, deployment model, approvals).
    /// Provided by customer via CloudFormation, Terraform, CLI, or Helm.
    #[serde(default)]
    pub stack_settings: StackSettings,
    /// Platform service account/role that will manage the infrastructure remotely.
    /// Derived from Manager's ServiceAccount, not user-specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_config: Option<ManagementConfig>,
    /// Environment variables snapshot
    pub environment_variables: EnvironmentVariablesSnapshot,
    /// Allow frozen resource changes during updates
    /// When true, skips the frozen resources compatibility check.
    /// This requires running with elevated cloud credentials.
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_frozen_changes: bool,
    /// Artifact registry configuration for pulling container images.
    /// Required for Local platform, optional for cloud platforms.
    /// When present, the deployment will fetch credentials from the manager
    /// before pulling images, enabling centralized registry access control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_registry: Option<ArtifactRegistryConfig>,
    /// Compute backend for Container and Function resources.
    /// When None, the platform default is used (Horizon for cloud platforms).
    /// Contains cluster IDs and management tokens for container orchestration.
    /// Machine tokens are stored in environment_variables as built-in secret vars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_backend: Option<ComputeBackend>,
    /// External bindings for pre-existing services.
    /// Required for Kubernetes platform (all infrastructure resources).
    /// Optional for cloud platforms (override specific resources).
    #[serde(default)]
    pub external_bindings: ExternalBindings,
    /// Image pull credentials for private container registries.
    /// Used when pulling images from registries that require authentication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_pull_credentials: Option<ImagePullCredentials>,
    /// Public URLs for exposed resources (optional override for all platforms).
    ///
    /// - **Kubernetes**: Pre-computed by Helm from services config (highly recommended)
    /// - **Cloud**: Optional override of domain_metadata or load balancer DNS
    /// - **Local**: Optional override of dynamic localhost URLs
    ///
    /// If not set, platforms determine public URLs from other sources:
    /// - Cloud: domain_metadata FQDN or load balancer DNS
    /// - Local: http://localhost:{allocated_port}
    /// - Kubernetes: None (unless provided by Helm)
    ///
    /// Key: resource ID, Value: public URL (e.g., "https://api.acme.com")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_urls: Option<HashMap<String, String>>,
    /// Domain metadata for auto-managed public resources (AWS/GCP/Azure).
    /// Contains certificate data for cloud provider import and renewal detection.
    /// Not used by Kubernetes (uses TLS Secrets) or Local (no TLS) platforms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_metadata: Option<DomainMetadata>,
    /// OTLP observability configuration for log export (optional).
    ///
    /// When set, alien-deployment injects OTEL_EXPORTER_OTLP_* env vars into
    /// container/function configs, and alien-infra embeds --otlp-logs-* flags
    /// into horizond VM startup scripts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring: Option<OtlpConfig>,
}

/// Result of a deployment step
///
/// Contains the complete next deployment state along with hints for the platform.
/// This replaces the old delta-based `DeploymentStateUpdate` approach.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentStepResult {
    /// The complete next deployment state
    pub state: DeploymentState,

    /// Error that occurred during this step (if any)
    /// - `None`: No error, step succeeded
    /// - `Some(error)`: Step failed or encountered an error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AlienError>,

    /// Suggested delay before next step (optimization hint)
    /// - `None`: No suggested delay, can poll immediately
    /// - `Some(ms)`: Wait this many milliseconds before next step
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_delay_ms: Option<u64>,

    /// Whether to update heartbeat timestamp (monitoring signal)
    /// - `false`: Don't update heartbeat (default for most steps)
    /// - `true`: Update lastHeartbeatAt (for successful health checks in Running state)
    #[serde(default, skip_serializing_if = "is_false")]
    pub update_heartbeat: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}
