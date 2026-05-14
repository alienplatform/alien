use serde::{Deserialize, Serialize};

/// Azure StorageAccount ImportData — the parent storage account that
/// holds Blob containers + queues + tables for this stack. Realized
/// once per stack and shared across `Storage` resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureStorageAccountImportData {
    /// Subscription ID containing the storage account.
    pub subscription_id: String,
    /// Resource group containing the storage account.
    pub resource_group: String,
    /// Storage account name (globally unique).
    pub storage_account_name: String,
    /// Primary blob endpoint URL.
    pub blob_endpoint: String,
    /// Primary queue endpoint URL.
    pub queue_endpoint: String,
}
