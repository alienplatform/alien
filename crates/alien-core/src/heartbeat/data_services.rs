use std::collections::BTreeMap;

use super::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
pub enum PostgresHeartbeatData {
    /// AWS Aurora Serverless v2 backend.
    Aurora(AuroraPostgresHeartbeatData),
    /// GCP Cloud SQL backend.
    CloudSql(GcpCloudSqlPostgresHeartbeatData),
    /// Azure Flexible Server backend.
    FlexibleServer(AzureFlexibleServerPostgresHeartbeatData),
    /// Local embedded Postgres backend.
    Local(LocalPostgresHeartbeatData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PostgresHeartbeatStatus {
    pub health: ObservedHealth,
    pub lifecycle: ProviderLifecycleState,
    pub message: Option<String>,
    pub stale: bool,
    pub partial: bool,
    pub collection_issues: Vec<HeartbeatCollectionIssue>,
}

impl Default for PostgresHeartbeatStatus {
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
pub struct LocalPostgresHeartbeatData {
    pub status: PostgresHeartbeatStatus,
    pub name: String,
    pub port: Option<u16>,
    pub version: String,
    pub process_running: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AuroraPostgresHeartbeatData {
    pub status: PostgresHeartbeatStatus,
    pub cluster_identifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Latest sampled `ServerlessDatabaseCapacity` (ACU).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serverless_capacity: Option<f64>,
    /// True when a `minCapacity: 0` instance has not reached 0 ACU over the observation
    /// window — it is silently paying always-on prices (auto-pause verification).
    pub never_pauses: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpCloudSqlPostgresHeartbeatData {
    pub status: PostgresHeartbeatStatus,
    pub instance_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureFlexibleServerPostgresHeartbeatData {
    pub status: PostgresHeartbeatStatus,
    pub server_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
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
