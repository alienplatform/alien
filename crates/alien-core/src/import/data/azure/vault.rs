use serde::{Deserialize, Serialize};

/// Azure Vault ImportData — a Key Vault namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureVaultImportData {
    /// Subscription ID containing the Key Vault.
    pub subscription_id: String,
    /// Resource group containing the Key Vault.
    pub resource_group: String,
    /// Key Vault name.
    pub vault_name: String,
    /// Vault DNS-style URI (`https://<name>.vault.azure.net/`).
    pub vault_uri: String,
}
