//! ComputeCluster resource for long-running container workloads.
//!
//! A ComputeCluster represents the setup-owned compute boundary for containers.
//! Setup provisions:
//! - Auto Scaling Groups (AWS), Managed Instance Groups (GCP), or VM Scale Sets (Azure)
//! - IAM roles/service accounts for machine authentication
//! - Security groups/firewall rules
//! - Launch templates/instance configurations

use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef};
use crate::ResourceType;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;

/// GPU specification for a capacity group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GpuSpec {
    /// GPU type identifier (e.g., "nvidia-a100", "nvidia-t4")
    #[serde(rename = "type")]
    pub gpu_type: String,
    /// Number of GPUs per machine
    pub count: u32,
}

/// Machine resource profile for a capacity group.
///
/// Represents the hardware specifications for machines in a capacity group.
/// These are hardware totals (what the instance type advertises), not allocatable
/// capacity. The managed container scheduler internally subtracts system reserves for planning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MachineProfile {
    /// CPU cores per machine (hardware total) - stored as string to preserve precision
    /// (e.g., "8.0", "4.5")
    pub cpu: String,
    /// Memory in bytes (hardware total)
    pub memory_bytes: u64,
    /// Ephemeral storage in bytes (hardware total)
    pub ephemeral_storage_bytes: u64,
    /// GPU specification (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuSpec>,
}

/// Capacity group definition.
///
/// A capacity group represents machines with identical hardware profiles.
/// Each group becomes a separate Auto Scaling Group (AWS), Managed Instance Group (GCP),
/// or VM Scale Set (Azure).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CapacityGroup {
    /// Unique identifier for this capacity group (must be lowercase alphanumeric with hyphens)
    pub group_id: String,
    /// Instance type for machines in this group (e.g., "m7g.xlarge", "n2-standard-8")
    /// Auto-selected if not specified, based on profile requirements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_type: Option<String>,
    /// Machine resource profile (auto-derived from instance_type if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<MachineProfile>,
    /// Minimum number of machines (can be 0 for scale-to-zero)
    pub min_size: u32,
    /// Maximum number of machines (must be ≤ 10)
    pub max_size: u32,
}

/// ComputeCluster resource for running long-running container workloads.
///
/// A ComputeCluster provides the setup-owned machine boundary for containers.
/// Alien may manage the worker fleet inside that boundary when setup grants
/// `compute-cluster/management`.
///
/// ## Architecture
///
/// - **Setup** creates cloud resources: ASGs/MIGs/VMSSs, IAM roles, security groups
/// - **Alien** manages allowed fleet operations: machine count and Horizon machine image rollout
/// - **horizond** runs on each machine from the selected Horizon machine image channel
///
/// ## Example
///
/// ```rust
/// use alien_core::{ComputeCluster, CapacityGroup};
///
/// let cluster = ComputeCluster::new("compute".to_string())
///     .capacity_group(CapacityGroup {
///         group_id: "general".to_string(),
///         instance_type: Some("m7g.xlarge".to_string()),
///         profile: None,
///         min_size: 1,
///         max_size: 5,
///     })
///     .build();
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct ComputeCluster {
    /// Unique identifier for the container cluster.
    /// Must contain only alphanumeric characters, hyphens, and underscores.
    #[builder(start_fn)]
    pub id: String,

    /// Capacity groups defining the machine pools for this cluster.
    /// Each group becomes a separate ASG/MIG/VMSS.
    #[builder(field)]
    pub capacity_groups: Vec<CapacityGroup>,

    /// Container CIDR block for internal container networking.
    /// Auto-generated as "10.244.0.0/16" if not specified.
    /// Each machine gets a /24 subnet from this range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_cidr: Option<String>,
}

impl ComputeCluster {
    /// The resource type identifier for ComputeCluster
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("compute-cluster");

    /// Returns the cluster's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the container CIDR, defaulting to "10.244.0.0/16" if not specified.
    pub fn container_cidr(&self) -> &str {
        self.container_cidr.as_deref().unwrap_or("10.244.0.0/16")
    }
}

impl<S: compute_cluster_builder::State> ComputeClusterBuilder<S> {
    /// Adds a capacity group to the cluster.
    pub fn capacity_group(mut self, group: CapacityGroup) -> Self {
        self.capacity_groups.push(group);
        self
    }
}

/// Status of a single capacity group within a ComputeCluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CapacityGroupStatus {
    /// Capacity group ID
    pub group_id: String,
    /// Current number of machines
    pub current_machines: u32,
    /// Desired number of machines (from the managed container capacity plan)
    pub desired_machines: u32,
    /// Instance type being used
    pub instance_type: String,
}

/// Outputs generated by a successfully provisioned ComputeCluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputeClusterOutputs {
    /// Managed container cluster ID (workspace/project/deployment/resourceid format)
    pub cluster_id: String,
    /// Whether the managed container cluster is ready
    pub horizon_ready: bool,
    /// Status of each capacity group
    pub capacity_group_statuses: Vec<CapacityGroupStatus>,
    /// Total number of machines across all capacity groups
    pub total_machines: u32,
}

impl ResourceOutputsDefinition for ComputeClusterOutputs {
    fn get_resource_type(&self) -> ResourceType {
        ComputeCluster::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<ComputeClusterOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceDefinition for ComputeCluster {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        // ComputeCluster has no static dependencies.
        // Network dependency is platform-specific:
        // - AWS/GCP/Azure: Added by ComputeClusterMutation
        // - Local/Kubernetes: Not needed (Docker/K8s handles networking)
        // Platform controllers use require_dependency() at runtime to access Network state.
        Vec::new()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_cluster = new_config
            .as_any()
            .downcast_ref::<ComputeCluster>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self.id != new_cluster.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        // Container CIDR is immutable once set
        if self.container_cidr.is_some()
            && new_cluster.container_cidr.is_some()
            && self.container_cidr != new_cluster.container_cidr
        {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'containerCidr' field is immutable once set".to_string(),
            }));
        }

        // Validate capacity groups
        for new_group in &new_cluster.capacity_groups {
            if let Some(existing_group) = self
                .capacity_groups
                .iter()
                .find(|g| g.group_id == new_group.group_id)
            {
                // Instance type is immutable for existing groups
                if existing_group.instance_type.is_some()
                    && new_group.instance_type.is_some()
                    && existing_group.instance_type != new_group.instance_type
                {
                    return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                        resource_id: self.id.clone(),
                        reason: format!(
                            "instance type for capacity group '{}' is immutable",
                            new_group.group_id
                        ),
                    }));
                }
            }
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<ComputeCluster>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_cluster_creation() {
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.xlarge".to_string()),
                profile: None,
                min_size: 1,
                max_size: 5,
            })
            .build();

        assert_eq!(cluster.id(), "compute");
        assert_eq!(cluster.capacity_groups.len(), 1);
        assert_eq!(cluster.capacity_groups[0].group_id, "general");
        assert_eq!(cluster.container_cidr(), "10.244.0.0/16");
    }

    #[test]
    fn test_compute_cluster_multiple_capacity_groups() {
        let cluster = ComputeCluster::new("multi-pool".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.xlarge".to_string()),
                profile: None,
                min_size: 1,
                max_size: 3,
            })
            .capacity_group(CapacityGroup {
                group_id: "gpu".to_string(),
                instance_type: Some("g5.xlarge".to_string()),
                profile: Some(MachineProfile {
                    cpu: "4.0".to_string(),
                    memory_bytes: 17179869184,             // 16 GiB
                    ephemeral_storage_bytes: 214748364800, // 200 GiB
                    gpu: Some(GpuSpec {
                        gpu_type: "nvidia-a10g".to_string(),
                        count: 1,
                    }),
                }),
                min_size: 0,
                max_size: 2,
            })
            .build();

        assert_eq!(cluster.capacity_groups.len(), 2);
        assert_eq!(cluster.capacity_groups[0].group_id, "general");
        assert_eq!(cluster.capacity_groups[1].group_id, "gpu");
        assert!(cluster.capacity_groups[1]
            .profile
            .as_ref()
            .unwrap()
            .gpu
            .is_some());
    }

    #[test]
    fn test_compute_cluster_custom_cidr() {
        let cluster = ComputeCluster::new("custom-net".to_string())
            .container_cidr("172.30.0.0/16".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 1,
                max_size: 5,
            })
            .build();

        assert_eq!(cluster.container_cidr(), "172.30.0.0/16");
    }

    #[test]
    fn test_compute_cluster_validate_update_immutable_id() {
        let cluster1 = ComputeCluster::new("cluster-1".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 1,
                max_size: 5,
            })
            .build();

        let cluster2 = ComputeCluster::new("cluster-2".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 1,
                max_size: 5,
            })
            .build();

        let result = cluster1.validate_update(&cluster2);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_cluster_validate_update_scale_change() {
        let cluster1 = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.xlarge".to_string()),
                profile: None,
                min_size: 1,
                max_size: 5,
            })
            .build();

        let cluster2 = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.xlarge".to_string()),
                profile: None,
                min_size: 2,
                max_size: 10,
            })
            .build();

        // Scale changes should be allowed
        let result = cluster1.validate_update(&cluster2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compute_cluster_serialization() {
        let cluster = ComputeCluster::new("test-cluster".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.xlarge".to_string()),
                profile: None,
                min_size: 1,
                max_size: 5,
            })
            .build();

        let json = serde_json::to_string(&cluster).unwrap();
        let deserialized: ComputeCluster = serde_json::from_str(&json).unwrap();
        assert_eq!(cluster, deserialized);
    }
}
