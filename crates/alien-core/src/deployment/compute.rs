//! Compute backend configuration (Horizon container orchestration).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// Dev example (ngrok): "https://abc123.ngrok.dev"
    pub horizond_download_base_url: String,

    /// ETag of the horizond binary fetched from the releases server -- used as a
    /// change-detection signal only. nginx auto-generates ETags from mtime+size,
    /// so every `cargo zigbuild` changes this value and triggers a rolling update.
    ///
    /// Optional: when absent (releases server unreachable), change detection
    /// falls back to URL-only (sufficient for versioned production releases).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizond_binary_hash: Option<String>,

    /// AMI / image ID for the Flatcar OS image used by EC2 instances.
    /// The Flatcar image has horizond baked in, so no user-data script is needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flatcar_image_id: Option<String>,

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
