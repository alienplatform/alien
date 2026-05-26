use serde::{Deserialize, Serialize};

/// Azure KV ImportData — a table in the stack's shared Azure Storage
/// account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureKvImportData {
    /// Subscription ID containing the Storage account.
    pub subscription_id: String,
    /// Resource group containing the Storage account.
    pub resource_group: String,
    /// Storage account name.
    pub storage_account_name: String,
    /// Azure Table name.
    pub table_name: String,
    /// Primary Table endpoint URL.
    pub table_endpoint: String,
}
