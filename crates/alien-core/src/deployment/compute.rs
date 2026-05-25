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

/// Horizon host image architecture.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum HorizonHostArchitecture {
    /// Linux arm64 / aarch64 host image.
    #[serde(rename = "arm64")]
    Arm64,
    /// Linux amd64 / x86_64 host image.
    #[serde(rename = "amd64")]
    Amd64,
}

/// AWS Horizon host image catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonAwsHostImages {
    /// AMI IDs by architecture, then AWS region.
    pub amis: HashMap<HorizonHostArchitecture, HashMap<String, String>>,
}

/// GCP Horizon host image entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonGcpHostImage {
    /// Source image self link or image-family URL.
    pub source_image: String,
}

/// GCP Horizon host image catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonGcpHostImages {
    /// Images by architecture.
    pub images: HashMap<HorizonHostArchitecture, HorizonGcpHostImage>,
}

/// Azure Horizon host image entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonAzureHostImage {
    /// Azure Compute Gallery image definition ID.
    pub image_definition_id: String,
}

/// Azure Horizon host image catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonAzureHostImages {
    /// Images by architecture.
    pub images: HashMap<HorizonHostArchitecture, HorizonAzureHostImage>,
}

/// Horizon host image catalog.
///
/// Platform resolves concrete provider images from this catalog during rollout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonHostImage {
    /// Logical image channel, such as prod, staging, or canary.
    pub channel: String,
    /// Published image catalog version.
    pub version: String,
    /// AWS image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws: Option<HorizonAwsHostImages>,
    /// GCP image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcp: Option<HorizonGcpHostImages>,
    /// Azure image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azure: Option<HorizonAzureHostImages>,
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

    /// Horizon host image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_host_image: Option<HorizonHostImage>,

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
