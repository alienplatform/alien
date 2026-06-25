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

/// Alternate OS variant of a Horizon machine image catalog.
///
/// Keyed by OS name (matching `ComputeCluster.os`, e.g. `"ubuntu"`) under
/// [`HorizonMachineImage::os_images`]. Carries its own base image and
/// per-provider image catalogs, built from the same horizond version as the
/// default (Flatcar) catalog at the top level of the manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonOsImageVariant {
    /// Base OS image metadata for this variant.
    pub base_image: HorizonMachineBaseImage,
    /// AWS image catalog for this variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws: Option<HorizonAwsMachineImages>,
    /// GCP image catalog for this variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcp: Option<HorizonGcpMachineImages>,
    /// Azure image catalog for this variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azure: Option<HorizonAzureMachineImages>,
    /// Whether this variant is experimental and not yet recommended for
    /// production use. Conveyed for visibility; not enforced at resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<bool>,
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
    /// AWS image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws: Option<HorizonAwsMachineImages>,
    /// GCP image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcp: Option<HorizonGcpMachineImages>,
    /// Azure image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azure: Option<HorizonAzureMachineImages>,
    /// Alternate OS image variants, keyed by OS name (matching
    /// `ComputeCluster.os`, e.g. `"ubuntu"`). Absent on manifests that only
    /// publish the default (Flatcar) catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_images: Option<HashMap<String, HorizonOsImageVariant>>,
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

#[cfg(test)]
mod os_images_tests {
    use super::*;

    #[test]
    fn manifest_os_images_round_trip() {
        let json = r#"{
            "channel":"dev",
            "machineImageVersion":"v1",
            "horizondVersion":"1.0",
            "gitSha":"abc",
            "createdAt":"2026-01-01T00:00:00Z",
            "baseImage":{"name":"flatcar","version":"stable-current"},
            "aws":{"amis":{"amd64":{"us-east-1":"ami-flat"}}},
            "osImages":{"ubuntu":{
                "baseImage":{"name":"ubuntu","version":"24.04-lts"},
                "experimental":true,
                "aws":{"amis":{"arm64":{"us-east-1":"ami-ubu"}}}
            }}
        }"#;
        let img: HorizonMachineImage = serde_json::from_str(json).expect("deserialize manifest");

        let ubuntu = img
            .os_images
            .as_ref()
            .expect("os_images present")
            .get("ubuntu")
            .expect("ubuntu variant present");
        assert_eq!(ubuntu.base_image.name, "ubuntu");
        assert_eq!(ubuntu.experimental, Some(true));
        assert_eq!(
            ubuntu.aws.as_ref().unwrap().amis[&HorizonMachineArchitecture::Arm64]["us-east-1"],
            "ami-ubu"
        );

        // Round-trips with the camelCase "osImages" key preserved.
        let back = serde_json::to_string(&img).unwrap();
        assert!(back.contains("\"osImages\""), "got {back}");
        assert_eq!(img, serde_json::from_str::<HorizonMachineImage>(&back).unwrap());
    }

    #[test]
    fn manifest_without_os_images_is_backward_compatible() {
        let json = r#"{"channel":"prod","machineImageVersion":"v","horizondVersion":"1","gitSha":"s","createdAt":"t","baseImage":{"name":"flatcar","version":"stable-current"},"aws":{"amis":{"amd64":{"us-east-1":"ami-x"}}}}"#;
        let img: HorizonMachineImage =
            serde_json::from_str(json).expect("old manifest still parses");
        assert!(img.os_images.is_none());
        // Re-serialization omits osImages entirely.
        assert!(!serde_json::to_string(&img).unwrap().contains("osImages"));
    }
}
