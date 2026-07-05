//! Compute backend configuration for container orchestration.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Configuration for a single container worker cluster.
///
/// Contains the cluster ID and management token needed to interact with
/// the managed container control plane API for container operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonClusterConfig {
    /// Cluster ID (deterministic: workspace/project/deployment/resourceid)
    pub cluster_id: String,

    /// Management token for API access (hm_...)
    /// Used by alien-deployment controllers to create/update containers
    pub management_token: String,
}

/// Horizon machine image architecture.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum HorizonMachineArchitecture {
    /// Linux arm64 / aarch64 machine image.
    #[serde(rename = "arm64")]
    Arm64,
    /// Linux amd64 / x86_64 machine image.
    #[serde(rename = "amd64")]
    Amd64,
}

/// AWS Horizon machine image catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonAwsMachineImages {
    /// AMI IDs by architecture, then AWS region.
    pub amis: HashMap<HorizonMachineArchitecture, HashMap<String, String>>,
}

/// GCP Horizon machine image entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonGcpMachineImage {
    /// Source image self link or image-family URL.
    pub source_image: String,
}

/// GCP Horizon machine image catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonGcpMachineImages {
    /// Images by architecture.
    pub images: HashMap<HorizonMachineArchitecture, HorizonGcpMachineImage>,
}

/// Azure Horizon machine image entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonAzureMachineImage {
    /// Azure Compute Gallery image version ID.
    pub image_version_id: String,
}

/// Base image metadata for the Horizon machine image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonMachineBaseImage {
    /// Base OS image name.
    pub name: String,
    /// Base OS image version or channel.
    pub version: String,
}

/// Azure Horizon machine image catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonAzureMachineImages {
    /// Images by architecture.
    pub images: HashMap<HorizonMachineArchitecture, HorizonAzureMachineImage>,
}

/// Download artifact for one horizond release platform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizondArtifact {
    /// HTTPS URL for the artifact.
    pub url: String,
    /// SHA-256 digest for the artifact payload.
    pub sha256: String,
}

/// Horizon machine image catalog.
///
/// Platform resolves concrete provider images from this catalog during rollout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonMachineImage {
    /// Logical image channel, such as prod, staging, or canary.
    pub channel: String,
    /// Published immutable machine image version.
    pub machine_image_version: String,
    /// horizond daemon version baked into the image.
    pub horizond_version: String,
    /// Git commit SHA used to build the image.
    pub git_sha: String,
    /// Image manifest creation timestamp.
    pub created_at: String,
    /// Base OS image metadata.
    pub base_image: HorizonMachineBaseImage,
    /// Per-architecture horizond artifacts by release-platform key.
    pub horizond_artifacts: HashMap<String, HorizondArtifact>,
    /// AWS image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws: Option<HorizonAwsMachineImages>,
    /// GCP image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcp: Option<HorizonGcpMachineImages>,
    /// Azure image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azure: Option<HorizonAzureMachineImages>,
}

/// Horizon control-plane configuration for container orchestration.
///
/// Contains all the information needed for Alien to interact with managed
/// container clusters during deployment. Each ComputeCluster resource gets its own
/// entry in the clusters map.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonConfig {
    /// Horizon control-plane API base URL.
    pub url: String,

    /// Horizon machine image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_machine_image: Option<HorizonMachineImage>,

    /// Cluster configurations (one per ComputeCluster resource)
    /// Key: ComputeCluster resource ID from stack
    /// Value: Cluster ID and management token for that cluster
    pub clusters: HashMap<String, HorizonClusterConfig>,
}

/// Compute backend for Container and Worker resources.
///
/// Determines how compute workloads are orchestrated on cloud platforms.
/// When None, the platform default is used for cloud platforms.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ComputeBackend {
    /// VM-backed container orchestration (default for cloud platforms)
    Horizon(HorizonConfig),
    // Future backends:
    // /// Deploy to existing Kubernetes cluster (EKS/GKE/AKS)
    // Kubernetes(KubernetesCredentials),
    // /// AWS ECS Fargate (serverless containers)
    // EcsFargate,
}
