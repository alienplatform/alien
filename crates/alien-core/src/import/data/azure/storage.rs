use serde::{Deserialize, Serialize};

/// Azure Storage ImportData — a Blob container inside a Storage Account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureStorageImportData {
    /// Subscription ID containing the storage account.
    pub subscription_id: String,
    /// Resource group containing the storage account.
    pub resource_group: String,
    /// Storage Account name (globally unique).
    pub storage_account_name: String,
    /// Blob container name owned by this stack.
    pub container_name: String,
}
