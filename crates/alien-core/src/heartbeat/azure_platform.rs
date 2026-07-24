use std::collections::BTreeMap;

use super::*;
use serde::{Deserialize, Serialize};

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
