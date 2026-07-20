use std::collections::BTreeMap;

use crate::{Platform, ResourceType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

mod azure_platform;
mod cluster;
mod data_services;
mod platform;
mod registry_build;
mod storage;
mod workload;

#[cfg(test)]
mod tests;

pub use azure_platform::*;
pub use cluster::*;
pub use data_services::*;
pub use platform::*;
pub use registry_build::*;
pub use storage::*;
pub use workload::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceHeartbeat {
    pub deployment_id: Option<String>,
    /// Alien resource id, such as the `alien.Container` or `alien.Storage`
    /// resource id from the stack.
    pub resource_id: String,
    pub resource_type: ResourceType,
    pub controller_platform: Platform,
    pub backend: HeartbeatBackend,
    pub observed_at: DateTime<Utc>,
    pub data: ResourceHeartbeatData,
    pub raw: Vec<RawHeartbeatSnippet>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ObservedInventoryBatch {
    /// Writer/source for this inventory pass, such as `operator` or
    /// `manager-observer`.
    pub source_kind: String,
    /// Stable scope for the provider list operation that produced this batch.
    pub inventory_scope: String,
    /// Platform whose observer produced this snapshot.
    pub controller_platform: Platform,
    /// Backend whose observer produced this snapshot.
    pub backend: HeartbeatBackend,
    /// Time the inventory scope was observed.
    pub observed_at: DateTime<Utc>,
    /// Whether this batch is a complete replacement for the scope. Complete
    /// batches tombstone previously observed rows in the same scope when they
    /// are absent from `resources`.
    pub complete: bool,
    pub resources: Vec<ObservedResourceSample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ObservedResourceSample {
    pub deployment_id: Option<String>,
    /// Provider-native stable identity: Kubernetes object identity, cloud ARN,
    /// GCP full resource name, Azure resource id, etc.
    pub raw_identity: String,
    /// Provider-native kind, such as `apps/v1/Deployment`,
    /// `AWS::S3::Bucket`, `storage.googleapis.com/Bucket`, or an Azure
    /// resource type.
    pub provider_kind: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_type_hint: Option<ResourceType>,
    /// Release/version identity observed from the provider resource, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alien_resource_id: Option<String>,
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub partial: bool,
    pub provider_stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<ObservedCounts>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, JsonValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw: Vec<RawHeartbeatSnippet>,
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
    #[serde(rename = "postgres")]
    Postgres(PostgresHeartbeatData),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct NamedResourceDetail {
    pub name: String,
    pub region: Option<String>,
}
