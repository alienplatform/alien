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

    /// Nonsecret configuration for bounded machine identity. Existing clusters
    /// remain on their prior authentication until explicitly migrated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_identity: Option<MachineRuntimeIdentity>,

    /// Machine image this cluster's machines run. When set, it replaces
    /// [`HorizonConfig::horizon_machine_image`] for this cluster only, so one
    /// cluster can stay on the image it runs while others move to a newer one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_machine_image: Option<HorizonMachineImage>,

    /// Control-plane URL this cluster's machines use, when it differs from
    /// [`HorizonConfig::url`], for example a relay in front of the control
    /// plane. Chosen when the cluster is created and never changed by later
    /// updates, so machines that have already joined keep the URL they use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_url: Option<String>,
}

/// Trusted manager endpoint and audience used by a machine to obtain leaf capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MachineRuntimeIdentity {
    pub exchange_url: String,
    pub deployment_id: String,
    pub audience: String,
}

/// Machine image architecture.
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

/// AWS machine image catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonAwsMachineImages {
    /// AMI IDs by architecture, then AWS region.
    pub amis: HashMap<HorizonMachineArchitecture, HashMap<String, String>>,
}

/// GCP machine image entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonGcpMachineImage {
    /// Source image self link or image-family URL.
    pub source_image: String,
}

/// GCP machine image catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonGcpMachineImages {
    /// Images by architecture.
    pub images: HashMap<HorizonMachineArchitecture, HorizonGcpMachineImage>,
}

/// Azure machine image entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonAzureMachineImage {
    /// Azure Compute Gallery image version ID.
    pub image_version_id: String,
}

/// Base image metadata for the machine image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonMachineBaseImage {
    /// Base OS image name.
    pub name: String,
    /// Base OS image version or channel.
    pub version: String,
}

/// Azure machine image catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonAzureMachineImages {
    /// Images by architecture.
    pub images: HashMap<HorizonMachineArchitecture, HorizonAzureMachineImage>,
}

/// Machine agent download for one release platform.
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

/// Machine image catalog.
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
    /// Machine agent version baked into the image.
    pub horizond_version: String,
    /// Git commit SHA used to build the image.
    pub git_sha: String,
    /// Image manifest creation timestamp.
    pub created_at: String,
    /// Base OS image metadata.
    pub base_image: HorizonMachineBaseImage,
    /// Per-architecture machine agent downloads by release-platform key.
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

/// Machine control-plane configuration for container orchestration.
///
/// Contains all the information needed for Alien to interact with managed
/// container clusters during deployment. Each ComputeCluster resource gets its own
/// entry in the clusters map.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonConfig {
    /// Machine control-plane API base URL.
    pub url: String,

    /// Machine image catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_machine_image: Option<HorizonMachineImage>,

    /// Cluster configurations (one per ComputeCluster resource)
    /// Key: ComputeCluster resource ID from stack
    /// Value: Cluster ID and management token for that cluster
    pub clusters: HashMap<String, HorizonClusterConfig>,
}

impl HorizonConfig {
    /// The machine image for the cluster of the ComputeCluster resource
    /// `resource_id`: its own image when it has one, otherwise the catalog's.
    pub fn machine_image_for(&self, resource_id: &str) -> Option<&HorizonMachineImage> {
        self.clusters
            .get(resource_id)
            .and_then(|cluster| cluster.horizon_machine_image.as_ref())
            .or(self.horizon_machine_image.as_ref())
    }
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

    use std::collections::HashMap;

    use serde::Deserialize;

    use super::{HorizonClusterConfig, HorizonConfig, HorizonMachineImage, HorizondArtifact};

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

    fn catalog_version(version: &str) -> Value {
        let mut catalog = legacy_catalog();
        catalog["machineImageVersion"] = json!(version);
        catalog
    }

    #[test]
    fn cluster_without_its_own_machine_image_reads_as_before() {
        let config: HorizonConfig = serde_json::from_value(json!({
            "url": "https://horizon.example.com",
            "horizonMachineImage": catalog_version("2"),
            "clusters": {
                "compute": { "clusterId": "cluster-1", "managementToken": "hm_test" }
            }
        }))
        .expect("a config written before the field existed must deserialize");

        assert_eq!(config.clusters["compute"].horizon_machine_image, None);
        assert_eq!(
            config
                .machine_image_for("compute")
                .map(|image| image.machine_image_version.as_str()),
            Some("2")
        );
        // Writing it back adds nothing, so readers that predate the field see the same document.
        let written = serde_json::to_value(&config.clusters["compute"]).unwrap();
        assert_eq!(
            written,
            json!({ "clusterId": "cluster-1", "managementToken": "hm_test" })
        );
    }

    #[test]
    fn each_cluster_uses_its_own_machine_image_when_it_has_one() {
        let config: HorizonConfig = serde_json::from_value(json!({
            "url": "https://horizon.example.com",
            "horizonMachineImage": catalog_version("2"),
            "clusters": {
                "held": {
                    "clusterId": "cluster-1",
                    "managementToken": "hm_test",
                    "horizonMachineImage": catalog_version("1")
                },
                "moving": { "clusterId": "cluster-2", "managementToken": "hm_test" }
            }
        }))
        .unwrap();

        let version = |resource_id| {
            config
                .machine_image_for(resource_id)
                .map(|image| image.machine_image_version.clone())
        };
        assert_eq!(version("held").as_deref(), Some("1"));
        assert_eq!(version("moving").as_deref(), Some("2"));
        assert_eq!(version("unknown").as_deref(), Some("2"));
    }

    /// The cluster and catalog types as they were before clusters could carry their own image.
    /// Nothing on the path from the deployment config to a cluster rejects unknown fields, so an
    /// older reader keeps working when a newer writer sets the image.
    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct PreviousHorizonClusterConfig {
        cluster_id: String,
        management_token: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PreviousHorizonConfig {
        #[allow(dead_code)]
        url: String,
        #[serde(default)]
        horizon_machine_image: Option<HorizonMachineImage>,
        clusters: HashMap<String, PreviousHorizonClusterConfig>,
    }

    #[test]
    fn readers_that_predate_cluster_machine_images_ignore_them() {
        let config = HorizonConfig {
            url: "https://horizon.example.com".to_string(),
            horizon_machine_image: Some(serde_json::from_value(catalog_version("2")).unwrap()),
            clusters: HashMap::from([(
                "compute".to_string(),
                HorizonClusterConfig {
                    machine_url: None,
                    cluster_id: "cluster-1".to_string(),
                    management_token: "hm_test".to_string(),
                    runtime_identity: None,
                    horizon_machine_image: Some(
                        serde_json::from_value(catalog_version("1")).unwrap(),
                    ),
                },
            )]),
        };
        let written = serde_json::to_value(&config).unwrap();
        assert!(written["clusters"]["compute"]["horizonMachineImage"].is_object());

        let previous: PreviousHorizonConfig = serde_json::from_value(written)
            .expect("an older reader must accept a cluster that carries its own image");
        assert_eq!(
            previous.clusters["compute"],
            PreviousHorizonClusterConfig {
                cluster_id: "cluster-1".to_string(),
                management_token: "hm_test".to_string(),
            }
        );
        assert_eq!(
            previous
                .horizon_machine_image
                .map(|image| image.machine_image_version),
            Some("2".to_string())
        );
    }

    #[test]
    fn cluster_machine_url_is_optional_and_ignored_by_older_readers() {
        let config: HorizonConfig = serde_json::from_value(json!({
            "url": "https://horizon.example.com",
            "clusters": {
                "relayed": {
                    "clusterId": "cluster-1",
                    "managementToken": "hm_test",
                    "machineUrl": "https://relay.example.com/v1/horizon"
                },
                "direct": { "clusterId": "cluster-2", "managementToken": "hm_test" }
            }
        }))
        .expect("a config written before the field existed must deserialize");

        assert_eq!(
            config.clusters["relayed"].machine_url.as_deref(),
            Some("https://relay.example.com/v1/horizon")
        );
        assert_eq!(config.clusters["direct"].machine_url, None);
        // A cluster without its own machine URL writes back exactly as before.
        assert_eq!(
            serde_json::to_value(&config.clusters["direct"]).unwrap(),
            json!({ "clusterId": "cluster-2", "managementToken": "hm_test" })
        );
        // An older reader accepts a cluster that carries a machine URL.
        let previous: PreviousHorizonConfig =
            serde_json::from_value(serde_json::to_value(&config).unwrap()).unwrap();
        assert_eq!(previous.clusters["relayed"].cluster_id, "cluster-1");
    }
}
