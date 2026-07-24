use std::collections::BTreeMap;

use super::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum ComputeClusterHeartbeatData {
    Aws(AwsComputeClusterHeartbeatData),
    Gcp(GcpComputeClusterHeartbeatData),
    Azure(AzureComputeClusterHeartbeatData),
    Machines(MachinesComputeClusterHeartbeatData),
    Local(LocalComputeClusterHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputeClusterHeartbeatStatus {
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
pub struct LocalComputeClusterHeartbeatData {
    pub status: ComputeClusterHeartbeatStatus,
    pub nodes: ObservedCounts,
    pub name: String,
    pub host_identifier: Option<String>,
    pub docker_available: bool,
    pub docker_version: Option<String>,
    pub docker_api_version: Option<String>,
    pub docker_os: Option<String>,
    pub docker_arch: Option<String>,
    pub network_name: Option<String>,
    pub network_available: bool,
    pub tracked_containers: Option<u32>,
    pub running_containers: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsComputeClusterHeartbeatData {
    pub status: ComputeClusterHeartbeatStatus,
    pub nodes: ObservedCounts,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub name: String,
    pub region: Option<String>,
    pub backend_cluster_id: Option<String>,
    pub capacity_groups: Vec<ComputeCapacityGroupStatus>,
    pub provider_fleets: Vec<ProviderFleetStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpComputeClusterHeartbeatData {
    pub status: ComputeClusterHeartbeatStatus,
    pub nodes: ObservedCounts,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub name: String,
    pub region: Option<String>,
    pub backend_cluster_id: Option<String>,
    pub capacity_groups: Vec<ComputeCapacityGroupStatus>,
    pub provider_fleets: Vec<ProviderFleetStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureComputeClusterHeartbeatData {
    pub status: ComputeClusterHeartbeatStatus,
    pub nodes: ObservedCounts,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub name: String,
    pub region: Option<String>,
    pub backend_cluster_id: Option<String>,
    pub capacity_groups: Vec<ComputeCapacityGroupStatus>,
    pub provider_fleets: Vec<ProviderFleetStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MachinesComputeClusterHeartbeatData {
    pub status: ComputeClusterHeartbeatStatus,
    pub nodes: ObservedCounts,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub name: String,
    pub backend_cluster_id: Option<String>,
    pub capacity_groups: Vec<ComputeCapacityGroupStatus>,
    pub machines: Vec<MachinesComputeMachineStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MachinesComputeMachineStatus {
    pub machine_id: String,
    pub status: String,
    pub capacity_group: String,
    pub zone: String,
    pub public_ip: Option<String>,
    pub overlay_ip: Option<String>,
    pub last_heartbeat: String,
    pub horizond_version: Option<String>,
    pub replica_count: i64,
    pub cpu_cores: Option<f64>,
    pub memory_bytes: Option<i64>,
    pub drain_force: bool,
    pub drain_requested_at: Option<String>,
    pub drain_deadline_at: Option<String>,
    pub drained_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drain_blockers: Vec<ComputeDrainBlocker>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputeCapacityGroupStatus {
    pub group_id: String,
    pub current_machines: u32,
    pub desired_machines: u32,
    pub min_machines: Option<u32>,
    pub max_machines: Option<u32>,
    pub instance_type: Option<String>,
    pub recommendation: Option<ComputeCapacityRecommendation>,
    pub capacity_blocker: Option<ComputeCapacityBlocker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drain_progress: Option<ComputeDrainProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ComputeCapacityBlockerCategory {
    Quota,
    Capacity,
    Allocation,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputeCapacityBlocker {
    pub category: ComputeCapacityBlockerCategory,
    pub provider_code: Option<String>,
    pub message: String,
    pub provider_reference: Option<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputeCapacityRecommendation {
    pub desired_machines: u32,
    pub reason: Option<String>,
    pub utilization: Option<MetricSample>,
    pub unschedulable_replicas: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ComputeDrainProgressStatus {
    Draining,
    Drained,
    Terminating,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputeDrainBlocker {
    pub workload_name: String,
    pub replica_id: String,
    pub scheduling_mode: String,
    pub state: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputeDrainProgress {
    pub machine_id: String,
    pub status: ComputeDrainProgressStatus,
    pub replica_count: i64,
    pub force: bool,
    pub stalled: bool,
    pub drain_requested_at: Option<String>,
    pub drain_deadline_at: Option<String>,
    pub drained_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<ComputeDrainBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ProviderFleetStatus {
    pub group_id: String,
    pub provider_id: String,
    pub location: Option<String>,
    pub current_machines: u32,
    pub desired_machines: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesClusterHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub node_counts: ObservedCounts,
    pub pod_counts: ObservedCounts,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub name: String,
    pub region: Option<String>,
    pub namespace: Option<String>,
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_statuses: Vec<KubernetesClusterNodeStatus>,
    pub events: Vec<KubernetesEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesClusterNodeStatus {
    pub name: String,
    pub uid: Option<String>,
    pub ready: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<KubernetesNodeConditionStatus>,
    pub roles: Vec<String>,
    pub labels: BTreeMap<String, String>,
    pub allocatable: KubernetesNodeResources,
    pub capacity: KubernetesNodeResources,
    pub usage: Option<KubernetesNodeUsage>,
    pub kubelet_version: Option<String>,
    pub container_runtime_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeConditionStatus {
    pub type_: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeResources {
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub pods: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeUsage {
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
}
