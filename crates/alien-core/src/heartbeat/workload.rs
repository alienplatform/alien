use super::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum WorkerHeartbeatData {
    AwsLambda(AwsLambdaWorkerHeartbeatData),
    GcpCloudRun(GcpCloudRunWorkerHeartbeatData),
    AzureContainerApps(AzureContainerAppsWorkerHeartbeatData),
    Kubernetes(KubernetesWorkerHeartbeatData),
    Local(LocalWorkerHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum ContainerHeartbeatData {
    HorizonPlatform(HorizonContainerHeartbeatData),
    Kubernetes(KubernetesContainerHeartbeatData),
    Local(LocalContainerHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum DaemonHeartbeatData {
    Aws(AwsDaemonHeartbeatData),
    Gcp(GcpDaemonHeartbeatData),
    Azure(AzureDaemonHeartbeatData),
    Machines(MachinesDaemonHeartbeatData),
    Kubernetes(KubernetesDaemonHeartbeatData),
    Local(LocalDaemonHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WorkloadHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WorkloadReplicaStatus {
    pub desired: Option<u32>,
    pub current: Option<u32>,
    pub ready: Option<u32>,
    pub available: Option<u32>,
    pub updated: Option<u32>,
    pub misscheduled: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsLambdaWorkerHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub function_name: String,
    pub runtime: Option<String>,
    pub package_type: Option<String>,
    pub memory_size_mb: Option<i64>,
    pub timeout_seconds: Option<i64>,
    pub version: Option<String>,
    pub revision_id: Option<String>,
    pub last_modified: Option<String>,
    pub state: Option<String>,
    pub state_reason: Option<String>,
    pub state_reason_code: Option<String>,
    pub last_update_status: Option<String>,
    pub last_update_status_reason: Option<String>,
    pub last_update_status_reason_code: Option<String>,
    pub code_sha256: Option<String>,
    pub layer_count: u32,
    pub function_url_auth_type: Option<String>,
    pub function_url_cors_present: bool,
    pub trigger_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpCloudRunWorkerHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub service: String,
    pub region: Option<String>,
    pub uri: Option<String>,
    pub urls: Vec<String>,
    pub latest_created_revision: Option<String>,
    pub latest_ready_revision: Option<String>,
    pub generation: Option<i64>,
    pub observed_generation: Option<i64>,
    pub traffic_count: u32,
    pub min_instance_count: Option<i32>,
    pub max_instance_count: Option<i32>,
    pub container_image: Option<String>,
    pub cpu_limit: Option<String>,
    pub memory_limit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureContainerAppsWorkerHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub app_name: String,
    pub revision: Option<String>,
    pub environment_name: Option<String>,
    pub provisioning_state: Option<String>,
    pub running_status: Option<String>,
    pub ingress_fqdn: Option<String>,
    pub min_replicas: Option<i32>,
    pub max_replicas: Option<i32>,
    pub cpu: Option<f64>,
    pub memory: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesWorkerHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub namespace: String,
    pub name: String,
    pub workload_kind: KubernetesWorkloadKind,
    pub replicas: WorkloadReplicaStatus,
    pub restarts: Option<u32>,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub workload: Option<KubernetesWorkloadStatus>,
    pub pods: Vec<KubernetesPodRuntimeUnitStatus>,
    pub trigger_count: u32,
    pub events: Vec<KubernetesEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalWorkerHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub pid: Option<u32>,
    pub command_supported: bool,
    pub image_path_present: bool,
    pub readiness_probe_ok: Option<bool>,
    pub trigger_count: u32,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub process: Option<LocalRuntimeUnitStatus>,
    pub events: Vec<LocalRuntimeEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonContainerHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub container_id: String,
    pub image: Option<String>,
    #[serde(default)]
    pub observed_image: Option<String>,
    #[serde(default)]
    pub latest_update_timestamp: Option<String>,
    pub scheduling_mode: HorizonWorkloadSchedulingMode,
    pub replicas: WorkloadReplicaStatus,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub attention_count: u32,
    pub replica_units: Vec<ManagedRuntimeUnitStatus>,
    pub events: Vec<ManagedRuntimeEventSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum HorizonWorkloadSchedulingMode {
    Replicated,
    Stateful,
    Daemon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesContainerHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub namespace: String,
    pub name: String,
    pub workload_kind: KubernetesWorkloadKind,
    pub replicas: WorkloadReplicaStatus,
    pub restarts: Option<u32>,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub workload: Option<KubernetesWorkloadStatus>,
    pub pods: Vec<KubernetesPodRuntimeUnitStatus>,
    pub events: Vec<KubernetesEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalContainerHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub container_id: Option<String>,
    pub name: Option<String>,
    pub image: Option<String>,
    pub runtime_status: Option<String>,
    pub restart_count: Option<u32>,
    pub port_count: u32,
    pub bind_mount_count: u32,
    pub local_url: Option<String>,
    pub runtime_reachable: bool,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub container_unit: Option<LocalRuntimeUnitStatus>,
    pub events: Vec<LocalRuntimeEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsDaemonHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub horizon_cluster_id: String,
    #[serde(default)]
    pub daemon_name: String,
    pub horizon_status: String,
    pub horizon_status_reason: Option<String>,
    pub horizon_status_message: Option<String>,
    pub capacity_group: String,
    pub desired_machines: u32,
    pub assigned_machines: u32,
    pub healthy_instances: u32,
    pub unavailable_instances: u32,
    pub command_supported: bool,
    #[serde(default)]
    pub observed_image: Option<String>,
    pub latest_update_timestamp: String,
    pub daemon_instances: Vec<ManagedRuntimeUnitStatus>,
    pub events: Vec<ManagedRuntimeEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpDaemonHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub horizon_cluster_id: String,
    #[serde(default)]
    pub daemon_name: String,
    pub horizon_status: String,
    pub horizon_status_reason: Option<String>,
    pub horizon_status_message: Option<String>,
    pub capacity_group: String,
    pub desired_machines: u32,
    pub assigned_machines: u32,
    pub healthy_instances: u32,
    pub unavailable_instances: u32,
    pub command_supported: bool,
    #[serde(default)]
    pub observed_image: Option<String>,
    pub latest_update_timestamp: String,
    pub daemon_instances: Vec<ManagedRuntimeUnitStatus>,
    pub events: Vec<ManagedRuntimeEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureDaemonHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub horizon_cluster_id: String,
    #[serde(default)]
    pub daemon_name: String,
    pub horizon_status: String,
    pub horizon_status_reason: Option<String>,
    pub horizon_status_message: Option<String>,
    pub capacity_group: String,
    pub desired_machines: u32,
    pub assigned_machines: u32,
    pub healthy_instances: u32,
    pub unavailable_instances: u32,
    pub command_supported: bool,
    #[serde(default)]
    pub observed_image: Option<String>,
    pub latest_update_timestamp: String,
    pub daemon_instances: Vec<ManagedRuntimeUnitStatus>,
    pub events: Vec<ManagedRuntimeEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MachinesDaemonHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub horizon_cluster_id: String,
    #[serde(default)]
    pub daemon_name: String,
    pub horizon_status: String,
    pub horizon_status_reason: Option<String>,
    pub horizon_status_message: Option<String>,
    pub capacity_group: String,
    pub desired_machines: u32,
    pub assigned_machines: u32,
    pub healthy_instances: u32,
    pub unavailable_instances: u32,
    pub command_supported: bool,
    #[serde(default)]
    pub observed_image: Option<String>,
    pub latest_update_timestamp: String,
    pub daemon_instances: Vec<ManagedRuntimeUnitStatus>,
    pub events: Vec<ManagedRuntimeEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeUnitStatus {
    pub replica_id: String,
    pub name: String,
    pub machine_id: Option<String>,
    pub node_name: Option<String>,
    pub ip: Option<String>,
    pub ready: bool,
    pub phase: Option<String>,
    pub status: Option<String>,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub restart_count: Option<u32>,
    pub waiting_reason: Option<String>,
    pub terminated_reason: Option<String>,
    pub metrics_healthy: Option<bool>,
    pub metrics_status: Option<String>,
    pub metrics_last_updated: Option<String>,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesDaemonHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    pub namespace: String,
    pub name: String,
    pub replicas: WorkloadReplicaStatus,
    pub restarts: Option<u32>,
    pub command_supported: bool,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
    pub workload: Option<KubernetesWorkloadStatus>,
    pub pods: Vec<KubernetesPodRuntimeUnitStatus>,
    pub events: Vec<KubernetesEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalDaemonHeartbeatData {
    pub status: WorkloadHeartbeatStatus,
    #[serde(default)]
    pub daemon_name: String,
    pub runtime_id: String,
    pub pid: Option<u32>,
    pub command_supported: bool,
    pub image_path_present: bool,
    pub restart_count: Option<u32>,
    pub exit_reason: Option<String>,
    pub daemon_instance: Option<LocalRuntimeUnitStatus>,
    pub events: Vec<LocalRuntimeEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalRuntimeUnitStatus {
    pub unit_id: String,
    pub name: String,
    pub kind: LocalRuntimeUnitKind,
    pub ready: bool,
    pub phase: Option<String>,
    pub pid: Option<u32>,
    pub restart_count: Option<u32>,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum LocalRuntimeUnitKind {
    Container,
    Process,
    Daemon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum KubernetesWorkloadKind {
    Deployment,
    StatefulSet,
    DaemonSet,
    ReplicaSet,
    Pod,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesWorkloadStatus {
    pub desired_replicas: Option<u32>,
    pub ready_replicas: Option<u32>,
    pub available_replicas: Option<u32>,
    pub updated_replicas: Option<u32>,
    pub observed_generation: Option<i64>,
    pub desired_generation: Option<i64>,
    pub rollout_reason: Option<String>,
    pub conditions: Vec<KubernetesWorkloadCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesWorkloadCondition {
    #[serde(rename = "type")]
    pub condition_type: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodRuntimeUnitStatus {
    pub name: String,
    pub uid: Option<String>,
    pub phase: Option<String>,
    pub ready: bool,
    pub restart_count: u32,
    pub waiting_reason: Option<String>,
    pub terminated_reason: Option<String>,
    pub node_name: Option<String>,
    pub pod_ip: Option<String>,
    pub owner_references: Vec<KubernetesOwnerReference>,
    pub cpu: Option<MetricSample>,
    pub memory: Option<MetricSample>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesOwnerReference {
    pub kind: String,
    pub name: String,
    pub uid: String,
    pub controller: bool,
}
