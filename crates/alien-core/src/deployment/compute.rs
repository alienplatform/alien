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
    /// Runtime isolation capability generation of the immutable artifact, not live readiness.
    #[serde(default)]
    pub runtime_isolation_generation: u32,
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
    /// Runtime isolation capability generation of the immutable artifact, not live readiness.
    #[serde(default)]
    pub runtime_isolation_generation: u32,
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

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{HorizonMachineImage, HorizondArtifact};

    fn legacy_catalog() -> Value {
        json!({
            "channel": "stable",
            "machineImageVersion": "1",
            "horizondVersion": "1",
            "gitSha": "abc123",
            "createdAt": "2026-01-01T00:00:00Z",
            "baseImage": { "name": "linux", "version": "1" },
            "horizondArtifacts": {
                "linux-arm64": { "url": "https://example.com/arm64", "sha256": "a".repeat(64) },
                "linux-amd64": { "url": "https://example.com/amd64", "sha256": "b".repeat(64) }
            },
            "aws": { "amis": { "arm64": { "us-east-1": "ami-example" } } },
            "gcp": { "images": { "arm64": { "sourceImage": "example-image" } } },
            "azure": { "images": { "arm64": { "imageVersionId": "example-version" } } }
        })
    }

    #[test]
    fn runtime_isolation_generation_legacy_catalog_defaults_to_zero() {
        let mut expected = legacy_catalog();
        let catalog: HorizonMachineImage = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(catalog.runtime_isolation_generation, 0);
        expected["runtimeIsolationGeneration"] = json!(0);
        for (arch, artifact) in &catalog.horizond_artifacts {
            assert_eq!(artifact.runtime_isolation_generation, 0);
            let decoded: HorizondArtifact =
                serde_json::from_value(expected["horizondArtifacts"][arch].clone()).unwrap();
            assert_eq!(&decoded, artifact);
            expected["horizondArtifacts"][arch]["runtimeIsolationGeneration"] = json!(0);
            assert_eq!(
                serde_json::to_value(decoded).unwrap(),
                expected["horizondArtifacts"][arch]
            );
        }
        assert_eq!(serde_json::to_value(catalog).unwrap(), expected);
    }

    #[test]
    fn runtime_isolation_generation_roundtrips_catalog_and_each_artifact() {
        let mut expected = legacy_catalog();
        expected["runtimeIsolationGeneration"] = json!(1);
        for artifact in expected["horizondArtifacts"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            artifact["runtimeIsolationGeneration"] = json!(1);
        }
        let catalog: HorizonMachineImage = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(catalog.runtime_isolation_generation, 1);
        for (arch, artifact) in &catalog.horizond_artifacts {
            assert_eq!(artifact.runtime_isolation_generation, 1);
            let encoded = serde_json::to_value(artifact).unwrap();
            assert_eq!(encoded, expected["horizondArtifacts"][arch]);
            assert_eq!(
                serde_json::from_value::<HorizondArtifact>(encoded).unwrap(),
                *artifact
            );
        }
        assert_eq!(serde_json::to_value(catalog).unwrap(), expected);
    }

    #[test]
    fn runtime_isolation_generation_rejects_invalid_numbers_and_types() {
        for invalid in [
            json!(-1),
            json!(1.5),
            json!("1"),
            json!(null),
            json!(4294967296_u64),
        ] {
            let mut catalog = legacy_catalog();
            catalog["runtimeIsolationGeneration"] = invalid.clone();
            assert!(serde_json::from_value::<HorizonMachineImage>(catalog).is_err());
            for arch in ["linux-arm64", "linux-amd64"] {
                let mut catalog = legacy_catalog();
                catalog["horizondArtifacts"][arch]["runtimeIsolationGeneration"] = invalid.clone();
                assert!(serde_json::from_value::<HorizondArtifact>(
                    catalog["horizondArtifacts"][arch].clone()
                )
                .is_err());
                assert!(serde_json::from_value::<HorizonMachineImage>(catalog).is_err());
            }
        }
    }
}
