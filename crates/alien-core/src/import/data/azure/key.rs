use serde::{Deserialize, Serialize};

/// Azure Key Vault key created by the customer-installed setup stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureKeyImportData {
    /// ARM resource ID of the dedicated Key Vault.
    pub vault_resource_id: String,
    /// Key name within the vault.
    pub key_name: String,
    /// Version ID that identifies the original key lineage.
    pub lineage_version_id: String,
    /// Full versioned Key Vault key ID used for new wrapping operations.
    pub key_id: String,
}
