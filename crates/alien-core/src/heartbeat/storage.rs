use super::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
