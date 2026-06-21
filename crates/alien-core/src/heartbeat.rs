use std::collections::BTreeMap;

use crate::{Platform, ResourceType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceHeartbeat {
    pub deployment_id: Option<String>,
    /// For managed heartbeats this is the Alien resource id. For observed heartbeats this is the
    /// raw provider identity, such as a Kubernetes object identity.
    pub resource_id: String,
    #[serde(default)]
    pub source: HeartbeatSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alien_resource_id: Option<String>,
    pub resource_type: ResourceType,
    pub controller_platform: Platform,
    pub backend: HeartbeatBackend,
    pub observed_at: DateTime<Utc>,
    pub data: ResourceHeartbeatData,
    pub raw: Vec<RawHeartbeatSnippet>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum HeartbeatSource {
    #[default]
    Managed,
    Observed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum HeartbeatBackend {
    Aws,
    Gcp,
    Azure,
    Kubernetes,
    Local,
    Managed,
    External,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum HeartbeatCollectionIssueReason {
    Forbidden,
    NotInstalled,
    ApiUnavailable,
    CollectionFailed,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "resourceType", content = "data")]
pub enum ResourceHeartbeatData {
    #[serde(rename = "storage")]
    Storage(StorageHeartbeatData),
    #[serde(rename = "worker")]
    Worker(WorkerHeartbeatData),
    #[serde(rename = "container")]
    Container(ContainerHeartbeatData),
    #[serde(rename = "daemon")]
    Daemon(DaemonHeartbeatData),
    #[serde(rename = "compute-cluster")]
    ComputeCluster(ComputeClusterHeartbeatData),
    #[serde(rename = "kubernetes-cluster")]
    KubernetesCluster(KubernetesClusterHeartbeatData),
    #[serde(rename = "queue")]
    Queue(QueueHeartbeatData),
    #[serde(rename = "kv")]
    Kv(KvHeartbeatData),
    #[serde(rename = "vault")]
    Vault(VaultHeartbeatData),
    #[serde(rename = "service-account")]
    ServiceAccount(ServiceAccountHeartbeatData),
    #[serde(rename = "network")]
    Network(NetworkHeartbeatData),
    #[serde(rename = "remote-stack-management")]
    RemoteStackManagement(RemoteStackManagementHeartbeatData),
    #[serde(rename = "artifact-registry")]
    ArtifactRegistry(ArtifactRegistryHeartbeatData),
    #[serde(rename = "build")]
    Build(BuildHeartbeatData),
    #[serde(rename = "service_activation")]
    ServiceActivation(ServiceActivationHeartbeatData),
    #[serde(rename = "azure_resource_group")]
    AzureResourceGroup(AzureResourceGroupHeartbeatData),
    #[serde(rename = "azure_storage_account")]
    AzureStorageAccount(AzureStorageAccountHeartbeatData),
    #[serde(rename = "azure_container_apps_environment")]
    AzureContainerAppsEnvironment(AzureContainerAppsEnvironmentHeartbeatData),
    #[serde(rename = "azure_service_bus_namespace")]
    AzureServiceBusNamespace(AzureServiceBusNamespaceHeartbeatData),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ObservedHealth {
    Unknown,
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ProviderLifecycleState {
    Unknown,
    Creating,
    Updating,
    Running,
    Scaling,
    Stopping,
    Stopped,
    Deleting,
    Deleted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatCollectionIssue {
    pub source: String,
    pub reason: HeartbeatCollectionIssueReason,
    pub severity: HeartbeatIssueSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum HeartbeatIssueSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesEventSnapshot {
    pub reason: String,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub message: String,
    pub count: Option<i32>,
    pub first_timestamp: Option<DateTime<Utc>>,
    pub last_timestamp: Option<DateTime<Utc>>,
    pub event_time: Option<DateTime<Utc>>,
    pub source: Option<KubernetesEventSource>,
    pub involved_object: Option<KubernetesEventInvolvedObject>,
    pub raw: Option<JsonValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesEventSource {
    pub component: Option<String>,
    pub host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesEventInvolvedObject {
    pub kind: Option<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub uid: Option<String>,
    pub api_version: Option<String>,
    pub resource_version: Option<String>,
    pub field_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeEventSnapshot {
    pub event_id: Option<String>,
    pub reason: String,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub message: String,
    pub count: Option<i32>,
    pub first_timestamp: Option<DateTime<Utc>>,
    pub last_timestamp: Option<DateTime<Utc>>,
    pub event_time: Option<DateTime<Utc>>,
    pub source: Option<ManagedRuntimeEventSource>,
    pub involved_object: Option<ManagedRuntimeEventInvolvedObject>,
    pub details: Option<JsonValue>,
    pub raw: Option<JsonValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeEventSource {
    pub component: Option<String>,
    pub host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeEventInvolvedObject {
    pub kind: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
    pub replica_id: Option<String>,
    pub machine_id: Option<String>,
    pub details: Option<JsonValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalRuntimeEventSnapshot {
    pub timestamp: DateTime<Utc>,
    pub severity: HeartbeatIssueSeverity,
    pub kind: String,
    pub message: String,
    pub subject: Option<LocalRuntimeEventSubject>,
    pub raw: Option<JsonValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalRuntimeEventSubject {
    pub kind: String,
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RawHeartbeatSnippet {
    pub source: String,
    pub format: RawHeartbeatSnippetFormat,
    pub collected_at: DateTime<Utc>,
    pub body: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RawHeartbeatSnippetFormat {
    Json,
    Yaml,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ObservedCounts {
    pub desired: Option<u32>,
    pub current: Option<u32>,
    pub ready: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MetricSample {
    pub value: f64,
    pub unit: MetricUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum MetricUnit {
    Count,
    Percent,
    Bytes,
    Cores,
    Milliseconds,
    RequestsPerSecond,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum StorageHeartbeatData {
    AwsS3(AwsS3StorageHeartbeatData),
    GcpCloudStorage(GcpCloudStorageHeartbeatData),
    AzureBlob(AzureBlobStorageHeartbeatData),
    Local(LocalStorageHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StorageHeartbeatStatus {
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
pub struct AwsS3StorageHeartbeatData {
    pub status: StorageHeartbeatStatus,
    pub name: String,
    pub region: Option<String>,
    pub bucket_location: Option<String>,
    pub versioning_status: Option<String>,
    pub versioning_enabled: Option<bool>,
    pub lifecycle_present: bool,
    pub lifecycle_rule_count: Option<u64>,
    pub encryption_config_present: bool,
    pub encryption_enabled: Option<bool>,
    pub public_access_block_present: bool,
    pub block_public_acls: Option<bool>,
    pub ignore_public_acls: Option<bool>,
    pub block_public_policy: Option<bool>,
    pub restrict_public_buckets: Option<bool>,
    pub bucket_policy_present: Option<bool>,
    pub bucket_acl_present: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureBlobStorageHeartbeatData {
    pub status: StorageHeartbeatStatus,
    pub name: String,
    pub storage_account_name: Option<String>,
    pub resource_group: Option<String>,
    pub location: Option<String>,
    pub account_kind: Option<String>,
    pub sku_name: Option<String>,
    pub sku_tier: Option<String>,
    pub access_tier: Option<String>,
    pub provisioning_state: Option<String>,
    pub primary_location: Option<String>,
    pub secondary_location: Option<String>,
    pub status_of_primary: Option<String>,
    pub status_of_secondary: Option<String>,
    pub public_network_access: Option<String>,
    pub allow_blob_public_access: Option<bool>,
    pub encryption_key_source: Option<String>,
    pub blob_encryption_enabled: Option<bool>,
    pub file_encryption_enabled: Option<bool>,
    pub queue_encryption_enabled: Option<bool>,
    pub table_encryption_enabled: Option<bool>,
    pub blob_versioning_enabled: Option<bool>,
    pub blob_delete_retention_enabled: Option<bool>,
    pub blob_delete_retention_days: Option<u64>,
    pub container_delete_retention_enabled: Option<bool>,
    pub container_delete_retention_days: Option<u64>,
    pub change_feed_enabled: Option<bool>,
    pub change_feed_retention_days: Option<u64>,
    pub container_public_access: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpCloudStorageHeartbeatData {
    pub status: StorageHeartbeatStatus,
    pub name: String,
    pub bucket_id: Option<String>,
    pub location: Option<String>,
    pub location_type: Option<String>,
    pub storage_class: Option<String>,
    pub versioning_enabled: Option<bool>,
    pub lifecycle_present: bool,
    pub lifecycle_rule_count: Option<u64>,
    pub retention_policy_effective_time: Option<String>,
    pub retention_policy_is_locked: Option<bool>,
    pub retention_period: Option<String>,
    pub soft_delete_retention_duration_seconds: Option<String>,
    pub soft_delete_effective_time: Option<String>,
    pub uniform_bucket_level_access_enabled: Option<bool>,
    pub uniform_bucket_level_access_locked_time: Option<String>,
    pub public_access_prevention: Option<String>,
    pub encryption_config_present: bool,
    pub default_kms_key_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalStorageHeartbeatData {
    pub status: StorageHeartbeatStatus,
    pub path: String,
    pub path_exists: bool,
    pub is_directory: Option<bool>,
    pub readonly: Option<bool>,
    pub modified_at: Option<DateTime<Utc>>,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum ComputeClusterHeartbeatData {
    Aws(AwsComputeClusterHeartbeatData),
    Gcp(GcpComputeClusterHeartbeatData),
    Azure(AzureComputeClusterHeartbeatData),
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
pub struct ComputeCapacityGroupStatus {
    pub group_id: String,
    pub current_machines: u32,
    pub desired_machines: u32,
    pub min_machines: Option<u32>,
    pub max_machines: Option<u32>,
    pub instance_type: Option<String>,
    pub recommendation: Option<ComputeCapacityRecommendation>,
    pub capacity_blocker: Option<ComputeCapacityBlocker>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum QueueHeartbeatData {
    AwsSqs(AwsSqsQueueHeartbeatData),
    GcpPubSub(GcpPubSubQueueHeartbeatData),
    AzureServiceBus(AzureServiceBusQueueHeartbeatData),
    Local(LocalQueueHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QueueHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

impl Default for QueueHeartbeatStatus {
    fn default() -> Self {
        Self {
            health: ObservedHealth::Unknown,
            lifecycle: ProviderLifecycleState::Unknown,
            message: None,
            stale: false,
            partial: false,
            collection_issues: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalQueueHeartbeatData {
    pub status: QueueHeartbeatStatus,
    pub name: String,
    pub path: Option<String>,
    pub service_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpPubSubQueueHeartbeatData {
    pub status: QueueHeartbeatStatus,
    pub topic_name: String,
    pub subscription_name: Option<String>,
    pub project_id: Option<String>,
    pub topic_full_name: Option<String>,
    pub subscription_full_name: Option<String>,
    pub endpoint: Option<String>,
    pub topic_labels: BTreeMap<String, String>,
    pub subscription_labels: BTreeMap<String, String>,
    pub message_storage_allowed_persistence_regions: Vec<String>,
    pub message_storage_enforce_in_transit: Option<bool>,
    pub kms_key_name: Option<String>,
    pub schema_name: Option<String>,
    pub schema_encoding: Option<String>,
    pub schema_first_revision_id: Option<String>,
    pub schema_last_revision_id: Option<String>,
    pub topic_message_retention_duration: Option<String>,
    pub topic_state: Option<String>,
    pub subscription_ack_deadline_seconds: Option<u32>,
    pub subscription_message_retention_duration: Option<String>,
    pub subscription_retain_acked_messages: Option<bool>,
    pub subscription_enable_message_ordering: Option<bool>,
    pub subscription_filter: Option<String>,
    pub subscription_detached: Option<bool>,
    pub subscription_state: Option<String>,
    pub subscription_push_config_present: Option<bool>,
    pub subscription_push_endpoint: Option<String>,
    pub subscription_push_attributes: BTreeMap<String, String>,
    pub subscription_push_oidc_service_account_email: Option<String>,
    pub subscription_push_oidc_audience: Option<String>,
    pub subscription_push_pubsub_wrapper_write_metadata: Option<bool>,
    pub subscription_push_no_wrapper_write_metadata: Option<bool>,
    pub subscription_dead_letter_topic: Option<String>,
    pub subscription_dead_letter_max_delivery_attempts: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureServiceBusQueueHeartbeatData {
    pub status: QueueHeartbeatStatus,
    pub name: String,
    pub namespace_name: String,
    pub resource_group: Option<String>,
    pub resource_id: Option<String>,
    pub endpoint: Option<String>,
    pub queue_status: Option<String>,
    pub lock_duration: Option<String>,
    pub max_delivery_count: Option<u32>,
    pub requires_duplicate_detection: Option<bool>,
    pub duplicate_detection_history_time_window: Option<String>,
    pub requires_session: Option<bool>,
    pub dead_lettering_on_message_expiration: Option<bool>,
    pub forward_dead_lettered_messages_to: Option<String>,
    pub forward_to: Option<String>,
    pub default_message_time_to_live: Option<String>,
    pub auto_delete_on_idle: Option<String>,
    pub enable_batched_operations: Option<bool>,
    pub enable_express: Option<bool>,
    pub enable_partitioning: Option<bool>,
    pub max_message_size_in_kilobytes: Option<u64>,
    pub max_size_in_megabytes: Option<u32>,
    pub message_count: Option<u64>,
    pub active_message_count: Option<u64>,
    pub dead_letter_message_count: Option<u64>,
    pub scheduled_message_count: Option<u64>,
    pub transfer_message_count: Option<u64>,
    pub transfer_dead_letter_message_count: Option<u64>,
    pub size_in_bytes: Option<u64>,
    pub accessed_at: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsSqsQueueHeartbeatData {
    pub status: QueueHeartbeatStatus,
    pub name: String,
    pub region: Option<String>,
    pub queue_url: Option<String>,
    pub queue_arn: Option<String>,
    pub visibility_timeout_seconds: Option<u32>,
    pub message_retention_period_seconds: Option<u32>,
    pub delay_seconds: Option<u32>,
    pub receive_message_wait_time_seconds: Option<u32>,
    pub maximum_message_size: Option<u32>,
    pub redrive_policy: Option<String>,
    pub redrive_allow_policy: Option<String>,
    pub fifo_queue: Option<bool>,
    pub content_based_deduplication: Option<bool>,
    pub deduplication_scope: Option<String>,
    pub fifo_throughput_limit: Option<String>,
    pub sse_enabled: Option<bool>,
    pub kms_master_key_id: Option<String>,
    pub kms_data_key_reuse_period_seconds: Option<u32>,
    pub sqs_managed_sse_enabled: Option<bool>,
    pub approximate_visible_messages: Option<u64>,
    pub approximate_in_flight_messages: Option<u64>,
    pub approximate_delayed_messages: Option<u64>,
    pub approximate_counts: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum KvHeartbeatData {
    AwsDynamoDb(AwsDynamoDbKvHeartbeatData),
    GcpFirestore(GcpFirestoreKvHeartbeatData),
    AzureTable(AzureTableKvHeartbeatData),
    Local(LocalKvHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KvHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

impl Default for KvHeartbeatStatus {
    fn default() -> Self {
        Self {
            health: ObservedHealth::Unknown,
            lifecycle: ProviderLifecycleState::Unknown,
            message: None,
            stale: false,
            partial: false,
            collection_issues: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsDynamoDbKvHeartbeatData {
    pub status: KvHeartbeatStatus,
    pub name: String,
    pub region: Option<String>,
    pub table_arn: Option<String>,
    pub table_status: Option<String>,
    pub billing_mode: Option<String>,
    pub key_schema: Vec<AwsDynamoDbKeySchemaElement>,
    pub global_secondary_index_count: Option<u32>,
    pub local_secondary_index_count: Option<u32>,
    pub item_count: Option<u64>,
    pub table_size_bytes: Option<u64>,
    pub stream_enabled: Option<bool>,
    pub stream_view_type: Option<String>,
    pub ttl_status: Option<String>,
    pub ttl_attribute_name: Option<String>,
    pub deletion_protection_enabled: Option<bool>,
    pub sse_status: Option<String>,
    pub sse_type: Option<String>,
    pub table_class: Option<String>,
    pub replica_count: Option<u32>,
    pub restore_in_progress: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsDynamoDbKeySchemaElement {
    pub attribute_name: String,
    pub key_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpFirestoreKvHeartbeatData {
    pub status: KvHeartbeatStatus,
    pub database_name: String,
    pub project_id: Option<String>,
    pub endpoint: Option<String>,
    pub location_id: Option<String>,
    pub database_type: Option<String>,
    pub concurrency_mode: Option<String>,
    pub app_engine_integration_mode: Option<String>,
    pub delete_protection_state: Option<String>,
    pub point_in_time_recovery_enablement: Option<String>,
    pub version_retention_period: Option<String>,
    pub earliest_version_time: Option<String>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
    pub delete_time: Option<String>,
    pub database_edition: Option<String>,
    pub cmek_enabled: bool,
    pub source_info_present: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureTableKvHeartbeatData {
    pub status: KvHeartbeatStatus,
    pub table_name: String,
    pub storage_account_name: String,
    pub resource_group: Option<String>,
    pub endpoint: Option<String>,
    pub storage_account_resource_id: Option<String>,
    pub storage_account_location: Option<String>,
    pub storage_account_kind: Option<String>,
    pub storage_account_provisioning_state: Option<String>,
    pub storage_account_primary_status: Option<String>,
    pub table_exists: bool,
    pub signed_identifier_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalKvHeartbeatData {
    pub status: KvHeartbeatStatus,
    pub name: String,
    pub path: String,
    pub path_exists: bool,
    pub is_directory: Option<bool>,
    pub cloud_metadata_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum VaultHeartbeatData {
    AwsParameterStore(AwsParameterStoreVaultHeartbeatData),
    GcpSecretManager(GcpSecretManagerVaultHeartbeatData),
    AzureKeyVault(AzureKeyVaultHeartbeatData),
    KubernetesSecret(KubernetesSecretVaultHeartbeatData),
    Local(LocalVaultHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VaultHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

impl Default for VaultHeartbeatStatus {
    fn default() -> Self {
        Self {
            health: ObservedHealth::Healthy,
            lifecycle: ProviderLifecycleState::Running,
            message: None,
            stale: false,
            partial: false,
            collection_issues: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsParameterStoreVaultHeartbeatData {
    pub status: VaultHeartbeatStatus,
    pub account_id: String,
    pub region: String,
    pub prefix: String,
    pub parameter_metadata_sampled: bool,
    pub sampled_parameter_count: Option<u32>,
    pub sampled_secure_string_count: Option<u32>,
    pub sampled_string_count: Option<u32>,
    pub sampled_string_list_count: Option<u32>,
    pub sampled_advanced_tier_count: Option<u32>,
    pub sampled_kms_key_metadata_present_count: Option<u32>,
    pub latest_modified_at: Option<DateTime<Utc>>,
    pub has_more_parameters: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpSecretManagerVaultHeartbeatData {
    pub status: VaultHeartbeatStatus,
    pub project_id: String,
    pub location: String,
    pub prefix: String,
    pub secret_metadata_listed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureKeyVaultHeartbeatData {
    pub status: VaultHeartbeatStatus,
    pub name: String,
    pub resource_group: Option<String>,
    pub resource_id: Option<String>,
    pub location: Option<String>,
    pub vault_uri: Option<String>,
    pub provisioning_state: Option<String>,
    pub sku_family: Option<String>,
    pub sku_name: Option<String>,
    pub soft_delete_enabled: bool,
    pub soft_delete_retention_days: i32,
    pub purge_protection_enabled: Option<bool>,
    pub rbac_authorization_enabled: bool,
    pub public_network_access: String,
    pub access_policy_count: u32,
    pub private_endpoint_connection_count: u32,
    pub secret_metadata_listed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesSecretVaultHeartbeatData {
    pub status: VaultHeartbeatStatus,
    pub namespace: String,
    pub prefix: String,
    pub secret_metadata_listed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalVaultHeartbeatData {
    pub status: VaultHeartbeatStatus,
    pub path: String,
    pub path_exists: bool,
    pub is_directory: Option<bool>,
    pub readonly: Option<bool>,
    pub modified_at: Option<DateTime<Utc>>,
    pub secret_metadata_listed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum ServiceAccountHeartbeatData {
    AwsIamRole(AwsIamRoleServiceAccountHeartbeatData),
    GcpServiceAccount(GcpServiceAccountHeartbeatData),
    AzureManagedIdentity(AzureManagedIdentityServiceAccountHeartbeatData),
    Local(LocalServiceAccountHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountHeartbeatStatus {
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
pub struct AwsIamRoleServiceAccountHeartbeatData {
    pub status: ServiceAccountHeartbeatStatus,
    pub role_name: String,
    pub role_arn: String,
    pub role_id: String,
    pub path: String,
    pub create_date: String,
    pub description: Option<String>,
    pub max_session_duration: Option<i32>,
    pub assume_role_policy_present: bool,
    pub permissions_boundary_type: Option<String>,
    pub permissions_boundary_arn: Option<String>,
    pub tag_count: u32,
    pub managed_tag_count: u32,
    pub attached_policy_count: u32,
    pub attached_policy_names: Vec<String>,
    pub inline_policy_count: u32,
    pub inline_policy_names: Vec<String>,
    pub stack_permissions_applied: bool,
    pub last_used_date: Option<String>,
    pub last_used_region: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpServiceAccountHeartbeatData {
    pub status: ServiceAccountHeartbeatStatus,
    pub name: Option<String>,
    pub project_id: Option<String>,
    pub unique_id: Option<String>,
    pub email: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub oauth2_client_id: Option<String>,
    pub disabled: Option<bool>,
    pub etag: Option<String>,
    pub project_binding_count: u32,
    pub project_roles: Vec<String>,
    pub service_account_binding_count: u32,
    pub service_account_roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureManagedIdentityServiceAccountHeartbeatData {
    pub status: ServiceAccountHeartbeatStatus,
    pub name: String,
    pub resource_id: String,
    pub resource_group: String,
    pub location: String,
    pub type_: Option<String>,
    pub client_id: Option<String>,
    pub principal_id: Option<String>,
    pub tenant_id: Option<String>,
    pub isolation_scope: Option<String>,
    pub managed_tag_count: u32,
    pub role_assignment_count: u32,
    pub role_assignment_ids: Vec<String>,
    pub custom_role_definition_count: u32,
    pub custom_role_definition_ids: Vec<String>,
    pub stack_permissions_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalServiceAccountHeartbeatData {
    pub status: ServiceAccountHeartbeatStatus,
    pub identity: String,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum NetworkHeartbeatData {
    AwsVpc(AwsVpcNetworkHeartbeatData),
    GcpVpc(GcpVpcNetworkHeartbeatData),
    AzureVnet(AzureVnetNetworkHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct NetworkHeartbeatStatus {
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
pub struct AwsVpcNetworkHeartbeatData {
    pub status: NetworkHeartbeatStatus,
    pub vpc_id: Option<String>,
    pub vpc_state: Option<String>,
    pub cidr_block: Option<String>,
    pub public_subnet_ids: Vec<String>,
    pub private_subnet_ids: Vec<String>,
    pub availability_zones: Vec<String>,
    pub internet_gateway_id: Option<String>,
    pub nat_gateway_id: Option<String>,
    pub route_table_count: u32,
    pub security_group_id: Option<String>,
    pub is_byo_vpc: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpVpcNetworkHeartbeatData {
    pub status: NetworkHeartbeatStatus,
    pub network_name: Option<String>,
    pub network_self_link: Option<String>,
    pub subnetwork_name: Option<String>,
    pub subnetwork_self_link: Option<String>,
    pub region: Option<String>,
    pub cidr_block: Option<String>,
    pub router_name: Option<String>,
    pub cloud_nat_name: Option<String>,
    pub firewall_name: Option<String>,
    pub is_byo_vpc: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureVnetNetworkHeartbeatData {
    pub status: NetworkHeartbeatStatus,
    pub vnet_name: Option<String>,
    pub vnet_resource_id: Option<String>,
    pub resource_group: Option<String>,
    pub location: Option<String>,
    pub cidr_block: Option<String>,
    pub public_subnet_name: Option<String>,
    pub private_subnet_name: Option<String>,
    pub application_gateway_subnet_name: Option<String>,
    pub nat_gateway_id: Option<String>,
    pub public_ip_id: Option<String>,
    pub nsg_id: Option<String>,
    pub is_byo_vnet: bool,
    pub last_byo_vnet_verification_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum RemoteStackManagementHeartbeatData {
    AwsIamRole(AwsRemoteStackManagementHeartbeatData),
    GcpServiceAccount(GcpRemoteStackManagementHeartbeatData),
    AzureManagedIdentity(AzureRemoteStackManagementHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RemoteStackManagementHeartbeatStatus {
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
pub struct AwsRemoteStackManagementHeartbeatData {
    pub status: RemoteStackManagementHeartbeatStatus,
    pub role_name: Option<String>,
    pub role_arn: Option<String>,
    pub management_permissions_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpRemoteStackManagementHeartbeatData {
    pub status: RemoteStackManagementHeartbeatStatus,
    pub service_account_email: Option<String>,
    pub service_account_unique_id: Option<String>,
    pub role_bound: bool,
    pub impersonation_granted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureRemoteStackManagementHeartbeatData {
    pub status: RemoteStackManagementHeartbeatStatus,
    pub uami_resource_id: Option<String>,
    pub uami_client_id: Option<String>,
    pub uami_principal_id: Option<String>,
    pub tenant_id: Option<String>,
    pub fic_name: Option<String>,
    pub role_definition_id: Option<String>,
    pub role_assignment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum ArtifactRegistryHeartbeatData {
    AwsEcr(AwsEcrArtifactRegistryHeartbeatData),
    GcpArtifactRegistry(GcpArtifactRegistryHeartbeatData),
    AzureContainerRegistry(AzureContainerRegistryHeartbeatData),
    Local(LocalArtifactRegistryHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRegistryHeartbeatStatus {
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
pub struct AwsEcrArtifactRegistryHeartbeatData {
    pub status: ArtifactRegistryHeartbeatStatus,
    pub registry_id: String,
    pub region: String,
    pub registry_uri: String,
    pub repository_prefix: String,
    pub pull_role_arn: Option<String>,
    pub push_role_arn: Option<String>,
    pub repository_count: u32,
    pub repositories_truncated: bool,
    pub repositories: Vec<AwsEcrRepositoryHeartbeatData>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsEcrRepositoryHeartbeatData {
    pub repository_arn: String,
    pub registry_id: String,
    pub repository_name: String,
    pub repository_uri: String,
    pub created_at: f64,
    pub image_tag_mutability: Option<String>,
    pub scan_on_push: Option<bool>,
    pub encryption_type: Option<String>,
    pub kms_key_present: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpArtifactRegistryHeartbeatData {
    pub status: ArtifactRegistryHeartbeatStatus,
    pub project_id: String,
    pub location: String,
    pub repository_id: String,
    pub name: Option<String>,
    pub format: Option<String>,
    pub mode: Option<String>,
    pub description: Option<String>,
    pub label_count: u32,
    pub cleanup_policy_count: u32,
    pub cleanup_policy_dry_run: Option<bool>,
    pub kms_key_name_present: bool,
    pub size_bytes: Option<String>,
    pub satisfies_pzs: Option<bool>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
    pub iam_policy_etag_present: bool,
    pub iam_binding_count: u32,
    pub iam_roles: Vec<String>,
    pub pull_service_account_email: Option<String>,
    pub push_service_account_email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureContainerRegistryHeartbeatData {
    pub status: ArtifactRegistryHeartbeatStatus,
    pub name: String,
    pub resource_id: Option<String>,
    pub resource_group: String,
    pub location: String,
    pub type_: Option<String>,
    pub login_server: Option<String>,
    pub sku_name: String,
    pub sku_tier: Option<String>,
    pub provisioning_state: Option<String>,
    pub admin_user_enabled: bool,
    pub anonymous_pull_enabled: bool,
    pub public_network_access: String,
    pub network_rule_bypass_options: String,
    pub network_rule_default_action: Option<String>,
    pub ip_rule_count: u32,
    pub encryption_status: Option<String>,
    pub encryption_key_vault_uri_present: bool,
    pub encryption_key_identifier_present: bool,
    pub policies_present: bool,
    pub policy_count: u32,
    pub private_endpoint_connection_count: u32,
    pub data_endpoint_enabled: Option<bool>,
    pub data_endpoint_host_names: Vec<String>,
    pub zone_redundancy: String,
    pub creation_date: Option<String>,
    pub managed_tag_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalArtifactRegistryHeartbeatData {
    pub status: ArtifactRegistryHeartbeatStatus,
    pub registry_url: String,
    pub reachable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum BuildHeartbeatData {
    AwsCodeBuild(AwsCodeBuildHeartbeatData),
    GcpCloudBuild(GcpCloudBuildHeartbeatData),
    AzureContainerApps(AzureContainerAppsBuildHeartbeatData),
    KubernetesJob(KubernetesBuildHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BuildHeartbeatStatus {
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
pub struct AwsCodeBuildHeartbeatData {
    pub status: BuildHeartbeatStatus,
    pub project_name: String,
    pub project_arn: Option<String>,
    pub description: Option<String>,
    pub source_type: Option<String>,
    pub artifacts_type: Option<String>,
    pub artifacts_encryption_disabled: Option<bool>,
    pub environment_type: Option<String>,
    pub environment_image: Option<String>,
    pub compute_type: Option<String>,
    pub image_pull_credentials_type: Option<String>,
    pub privileged_mode: Option<bool>,
    pub environment_variable_count: u32,
    pub service_role_present: bool,
    pub encryption_key_present: bool,
    pub cloud_watch_logs_status: Option<String>,
    pub s3_logs_status: Option<String>,
    pub timeout_in_minutes: Option<i32>,
    pub queued_timeout_in_minutes: Option<i32>,
    pub created: Option<f64>,
    pub last_modified: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpCloudBuildHeartbeatData {
    pub status: BuildHeartbeatStatus,
    pub project_id: String,
    pub location: String,
    pub build_config_id: String,
    pub service_account: Option<String>,
    pub environment_variable_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureContainerAppsBuildHeartbeatData {
    pub status: BuildHeartbeatStatus,
    pub managed_environment_id: String,
    pub resource_group_name: String,
    pub managed_identity_id: Option<String>,
    pub resource_prefix: Option<String>,
    pub environment_variable_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct KubernetesBuildHeartbeatData {
    pub status: BuildHeartbeatStatus,
    pub job_name: String,
    pub namespace: String,
    pub active: Option<i32>,
    pub succeeded: Option<i32>,
    pub failed: Option<i32>,
    pub start_time: Option<DateTime<Utc>>,
    pub completion_time: Option<DateTime<Utc>>,
    pub condition_count: u32,
    pub image_digest: Option<String>,
    pub events: Vec<KubernetesEventSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "backend", rename_all = "camelCase")]
pub enum ServiceActivationHeartbeatData {
    GcpServiceUsage(GcpServiceUsageActivationHeartbeatData),
    AzureResourceProvider(AzureResourceProviderActivationHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServiceActivationHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

impl Default for ServiceActivationHeartbeatStatus {
    fn default() -> Self {
        Self {
            health: ObservedHealth::Healthy,
            lifecycle: ProviderLifecycleState::Running,
            message: None,
            stale: false,
            partial: false,
            collection_issues: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpServiceUsageActivationHeartbeatData {
    pub status: ServiceActivationHeartbeatStatus,
    pub project_id: String,
    pub service_name: String,
    pub service_resource_name: Option<String>,
    pub title: Option<String>,
    pub state: Option<String>,
    pub enabled: bool,
    pub last_operation_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureResourceProviderActivationHeartbeatData {
    pub status: ServiceActivationHeartbeatStatus,
    pub namespace: String,
    pub provider_id: Option<String>,
    pub registration_state: Option<String>,
    pub registration_policy: Option<String>,
    pub resource_type_count: u32,
    pub registered: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureResourceGroupHeartbeatData {
    pub status: AzureResourceGroupHeartbeatStatus,
    pub name: String,
    pub resource_id: Option<String>,
    pub location: Option<String>,
    pub provisioning_state: Option<String>,
    pub managed_tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureResourceGroupHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

impl Default for AzureResourceGroupHeartbeatStatus {
    fn default() -> Self {
        Self {
            health: ObservedHealth::Healthy,
            lifecycle: ProviderLifecycleState::Running,
            message: None,
            stale: false,
            partial: false,
            collection_issues: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureStorageAccountHeartbeatData {
    pub status: StorageHeartbeatStatus,
    pub name: String,
    pub resource_id: Option<String>,
    pub resource_group: Option<String>,
    pub location: Option<String>,
    pub kind: Option<String>,
    pub sku_name: Option<String>,
    pub sku_tier: Option<String>,
    pub provisioning_state: Option<String>,
    pub primary_endpoints: AzureStorageAccountEndpoints,
    pub secondary_endpoints: AzureStorageAccountEndpoints,
    pub public_network_access: Option<String>,
    pub allow_blob_public_access: Option<bool>,
    pub allow_shared_key_access: Option<bool>,
    pub minimum_tls_version: Option<String>,
    pub supports_https_traffic_only: Option<bool>,
    pub encryption_key_source: Option<String>,
    pub require_infrastructure_encryption: Option<bool>,
    pub network_default_action: Option<String>,
    pub network_bypass: Option<String>,
    pub network_ip_rule_count: Option<u32>,
    pub network_virtual_network_rule_count: Option<u32>,
    pub network_resource_access_rule_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureStorageAccountEndpoints {
    pub blob: Option<String>,
    pub dfs: Option<String>,
    pub file: Option<String>,
    pub queue: Option<String>,
    pub table: Option<String>,
    pub web: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureContainerAppsEnvironmentHeartbeatData {
    pub status: AzureContainerAppsEnvironmentHeartbeatStatus,
    pub name: String,
    pub resource_id: Option<String>,
    pub resource_group: Option<String>,
    pub location: Option<String>,
    pub kind: Option<String>,
    pub provisioning_state: Option<String>,
    pub default_domain: Option<String>,
    pub static_ip: Option<String>,
    pub custom_domain_verification_id: Option<String>,
    pub infrastructure_resource_group: Option<String>,
    pub event_stream_endpoint: Option<String>,
    pub zone_redundant: Option<bool>,
    pub workload_profile_count: u32,
    pub workload_profiles: Vec<AzureContainerAppsEnvironmentWorkloadProfile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureContainerAppsEnvironmentHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureContainerAppsEnvironmentWorkloadProfile {
    pub name: String,
    pub workload_profile_type: String,
    pub minimum_count: Option<i32>,
    pub maximum_count: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureServiceBusNamespaceHeartbeatData {
    pub status: QueueHeartbeatStatus,
    pub name: String,
    pub resource_id: Option<String>,
    pub resource_group: Option<String>,
    pub location: Option<String>,
    pub sku_name: Option<String>,
    pub sku_tier: Option<String>,
    pub sku_capacity: Option<i32>,
    pub namespace_status: Option<String>,
    pub provisioning_state: Option<String>,
    pub service_bus_endpoint: Option<String>,
    pub metric_id: Option<String>,
    pub public_network_access: Option<String>,
    pub disable_local_auth: Option<bool>,
    pub minimum_tls_version: Option<String>,
    pub premium_messaging_partitions: Option<i32>,
    pub private_endpoint_connection_count: u32,
    pub zone_redundant: Option<bool>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct NamedResourceDetail {
    pub name: String,
    pub region: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;
    use serde_json::json;

    fn observed_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 28, 10, 30, 0).unwrap()
    }

    fn workload_status() -> WorkloadHeartbeatStatus {
        WorkloadHeartbeatStatus {
            health: ObservedHealth::Healthy,
            lifecycle: ProviderLifecycleState::Running,
            message: None,
            stale: false,
            partial: false,
            collection_issues: vec![],
        }
    }

    fn workload_replicas() -> WorkloadReplicaStatus {
        WorkloadReplicaStatus {
            desired: Some(2),
            current: Some(2),
            ready: Some(1),
            available: Some(1),
            updated: Some(2),
            misscheduled: None,
        }
    }

    fn heartbeat(data: ResourceHeartbeatData, resource_type: &str) -> ResourceHeartbeat {
        ResourceHeartbeat {
            deployment_id: Some("dep_123".to_string()),
            resource_id: "api".to_string(),
            resource_type: ResourceType::from(resource_type),
            controller_platform: Platform::Kubernetes,
            backend: HeartbeatBackend::Kubernetes,
            source: Default::default(),
            alien_resource_id: None,
            observed_at: observed_at(),
            data,
            raw: vec![RawHeartbeatSnippet {
                source: "kubernetes/apps/v1/deployments/api".to_string(),
                format: RawHeartbeatSnippetFormat::Json,
                collected_at: observed_at(),
                body: r#"{"readyReplicas":1}"#.to_string(),
                truncated: false,
            }],
        }
    }

    #[test]
    fn resource_heartbeat_defaults_missing_source_to_managed() {
        let mut heartbeat_json = serde_json::to_value(heartbeat(
            ResourceHeartbeatData::Container(ContainerHeartbeatData::Kubernetes(
                KubernetesContainerHeartbeatData {
                    status: workload_status(),
                    namespace: "default".to_string(),
                    name: "api".to_string(),
                    workload_kind: KubernetesWorkloadKind::Deployment,
                    replicas: workload_replicas(),
                    restarts: None,
                    cpu: None,
                    memory: None,
                    workload: None,
                    pods: vec![],
                    events: vec![],
                },
            )),
            "container",
        ))
        .unwrap();

        heartbeat_json
            .as_object_mut()
            .unwrap()
            .remove("source");
        heartbeat_json
            .as_object_mut()
            .unwrap()
            .remove("alienResourceId");

        let parsed: ResourceHeartbeat = serde_json::from_value(heartbeat_json).unwrap();
        assert_eq!(parsed.source, HeartbeatSource::Managed);
        assert_eq!(parsed.alien_resource_id, None);
    }

    #[test]
    fn container_heartbeat_serializes_resource_first_data() {
        let heartbeat = heartbeat(
            ResourceHeartbeatData::Container(ContainerHeartbeatData::Kubernetes(
                KubernetesContainerHeartbeatData {
                    status: workload_status(),
                    namespace: "default".to_string(),
                    name: "api".to_string(),
                    workload_kind: KubernetesWorkloadKind::Deployment,
                    replicas: workload_replicas(),
                    restarts: Some(1),
                    cpu: Some(MetricSample {
                        value: 0.5,
                        unit: MetricUnit::Cores,
                    }),
                    memory: None,
                    workload: None,
                    pods: vec![],
                    events: vec![],
                },
            )),
            "container",
        );

        let value = serde_json::to_value(&heartbeat).unwrap();

        assert_eq!(value["resourceType"], "container");
        assert_eq!(value["data"]["resourceType"], "container");
        assert_eq!(value["data"]["data"]["backend"], "kubernetes");
        assert_eq!(value["raw"][0]["body"], r#"{"readyReplicas":1}"#);
        assert!(value.get("collection").is_none());
        assert!(value["data"]["data"].get("summary").is_none());
        assert!(value["data"]["data"].get("detail").is_none());
    }

    #[test]
    fn representative_workload_data_has_stable_tags() {
        let daemon = serde_json::to_value(ResourceHeartbeatData::Daemon(
            DaemonHeartbeatData::Kubernetes(KubernetesDaemonHeartbeatData {
                status: workload_status(),
                namespace: "default".to_string(),
                name: "agent".to_string(),
                replicas: workload_replicas(),
                restarts: Some(0),
                command_supported: true,
                cpu: None,
                memory: None,
                workload: None,
                pods: vec![],
                events: vec![],
            }),
        ))
        .unwrap();
        let worker = serde_json::to_value(ResourceHeartbeatData::Worker(
            WorkerHeartbeatData::AwsLambda(AwsLambdaWorkerHeartbeatData {
                status: workload_status(),
                function_name: "handler".to_string(),
                runtime: Some("nodejs22.x".to_string()),
                package_type: None,
                memory_size_mb: None,
                timeout_seconds: None,
                version: None,
                revision_id: None,
                last_modified: None,
                state: None,
                state_reason: None,
                state_reason_code: None,
                last_update_status: None,
                last_update_status_reason: None,
                last_update_status_reason_code: None,
                code_sha256: None,
                layer_count: 0,
                function_url_auth_type: None,
                function_url_cors_present: false,
                trigger_count: 0,
            }),
        ))
        .unwrap();

        assert_eq!(daemon["resourceType"], "daemon");
        assert_eq!(daemon["data"]["backend"], "kubernetes");
        assert_eq!(worker["resourceType"], "worker");
        assert_eq!(worker["data"]["backend"], "awsLambda");
    }

    #[test]
    fn representative_cluster_and_data_variants_have_optional_counts() {
        let cluster = serde_json::to_value(ResourceHeartbeatData::KubernetesCluster(
            KubernetesClusterHeartbeatData {
                status: WorkloadHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: None,
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                node_counts: ObservedCounts {
                    desired: Some(3),
                    current: Some(3),
                    ready: None,
                },
                pod_counts: ObservedCounts {
                    desired: None,
                    current: Some(12),
                    ready: Some(11),
                },
                cpu: None,
                memory: None,
                name: "prod".to_string(),
                region: Some("us-east-1".to_string()),
                namespace: None,
                version: Some("1.33".to_string()),
                node_statuses: vec![],
                events: vec![],
            },
        ))
        .unwrap();
        let queue = serde_json::to_value(ResourceHeartbeatData::Queue(QueueHeartbeatData::AwsSqs(
            AwsSqsQueueHeartbeatData {
                status: QueueHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: None,
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: "jobs".to_string(),
                region: Some("us-east-1".to_string()),
                queue_url: Some("https://sqs.us-east-1.amazonaws.com/123/jobs".to_string()),
                queue_arn: Some("arn:aws:sqs:us-east-1:123:jobs".to_string()),
                visibility_timeout_seconds: Some(30),
                message_retention_period_seconds: Some(345600),
                delay_seconds: Some(0),
                receive_message_wait_time_seconds: Some(0),
                maximum_message_size: Some(262144),
                redrive_policy: None,
                redrive_allow_policy: None,
                fifo_queue: Some(false),
                content_based_deduplication: None,
                deduplication_scope: None,
                fifo_throughput_limit: None,
                sse_enabled: Some(false),
                kms_master_key_id: None,
                kms_data_key_reuse_period_seconds: None,
                sqs_managed_sse_enabled: Some(false),
                approximate_visible_messages: Some(42),
                approximate_in_flight_messages: Some(1),
                approximate_delayed_messages: Some(0),
                approximate_counts: true,
            },
        )))
        .unwrap();
        let storage = serde_json::to_value(ResourceHeartbeatData::Storage(
            StorageHeartbeatData::AwsS3(AwsS3StorageHeartbeatData {
                status: StorageHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: None,
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: "assets".to_string(),
                region: Some("us-east-1".to_string()),
                bucket_location: Some("us-east-1".to_string()),
                versioning_status: Some("Enabled".to_string()),
                versioning_enabled: Some(true),
                lifecycle_present: false,
                lifecycle_rule_count: Some(0),
                encryption_config_present: true,
                encryption_enabled: Some(true),
                public_access_block_present: true,
                block_public_acls: Some(true),
                ignore_public_acls: Some(true),
                block_public_policy: Some(true),
                restrict_public_buckets: Some(true),
                bucket_policy_present: Some(false),
                bucket_acl_present: Some(true),
            }),
        ))
        .unwrap();
        let gcp_storage = serde_json::to_value(ResourceHeartbeatData::Storage(
            StorageHeartbeatData::GcpCloudStorage(GcpCloudStorageHeartbeatData {
                status: StorageHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: None,
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: "assets".to_string(),
                bucket_id: Some("project/assets".to_string()),
                location: Some("US".to_string()),
                location_type: Some("multi-region".to_string()),
                storage_class: Some("STANDARD".to_string()),
                versioning_enabled: Some(true),
                lifecycle_present: false,
                lifecycle_rule_count: Some(0),
                retention_policy_effective_time: None,
                retention_policy_is_locked: None,
                retention_period: None,
                soft_delete_retention_duration_seconds: None,
                soft_delete_effective_time: None,
                uniform_bucket_level_access_enabled: Some(true),
                uniform_bucket_level_access_locked_time: None,
                public_access_prevention: Some("enforced".to_string()),
                encryption_config_present: true,
                default_kms_key_name: Some(
                    "projects/p/locations/l/keyRings/r/cryptoKeys/k".to_string(),
                ),
            }),
        ))
        .unwrap();
        let kv = serde_json::to_value(ResourceHeartbeatData::Kv(KvHeartbeatData::AwsDynamoDb(
            AwsDynamoDbKvHeartbeatData {
                status: KvHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: None,
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: "state".to_string(),
                region: Some("us-east-1".to_string()),
                table_arn: None,
                table_status: Some("ACTIVE".to_string()),
                billing_mode: Some("PAY_PER_REQUEST".to_string()),
                key_schema: vec![AwsDynamoDbKeySchemaElement {
                    attribute_name: "pk".to_string(),
                    key_type: "HASH".to_string(),
                }],
                global_secondary_index_count: Some(0),
                local_secondary_index_count: Some(0),
                item_count: None,
                table_size_bytes: None,
                stream_enabled: Some(false),
                stream_view_type: None,
                ttl_status: Some("ENABLED".to_string()),
                ttl_attribute_name: Some("ttl".to_string()),
                deletion_protection_enabled: Some(false),
                sse_status: Some("ENABLED".to_string()),
                sse_type: Some("KMS".to_string()),
                table_class: None,
                replica_count: Some(0),
                restore_in_progress: None,
            },
        )))
        .unwrap();

        assert_eq!(cluster["resourceType"], "kubernetes-cluster");
        assert!(cluster["data"].get("summary").is_none());
        assert!(cluster["data"].get("detail").is_none());
        assert_eq!(cluster["data"]["name"], "prod");
        assert_eq!(queue["data"]["backend"], "awsSqs");
        assert_eq!(queue["data"]["approximateVisibleMessages"], 42);
        assert!(queue["data"].get("summary").is_none());
        assert_eq!(storage["data"]["backend"], "awsS3");
        assert!(storage["data"].get("summary").is_none());
        assert_eq!(gcp_storage["data"]["backend"], "gcpCloudStorage");
        assert_eq!(gcp_storage["data"]["publicAccessPrevention"], "enforced");
        assert_eq!(kv["data"]["backend"], "awsDynamoDb");
        assert!(kv["data"].get("summary").is_none());
        assert_eq!(kv["data"]["itemCount"], json!(null));
    }
}
