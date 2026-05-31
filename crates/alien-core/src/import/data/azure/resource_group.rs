use serde::{Deserialize, Serialize};

/// Azure ResourceGroup ImportData — the parent resource group every
/// Azure resource in this stack lives in. Realized once per stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureResourceGroupImportData {
    /// Subscription ID containing the group.
    pub subscription_id: String,
    /// Resource group name.
    pub resource_group: String,
    /// Resource group location (e.g. `eastus`).
    pub location: String,
}
