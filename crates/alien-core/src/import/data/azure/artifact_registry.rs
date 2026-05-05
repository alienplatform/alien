use serde::{Deserialize, Serialize};

/// Azure ArtifactRegistry ImportData — an Azure Container Registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureArtifactRegistryImportData {
    /// Subscription ID hosting the registry.
    pub subscription_id: String,
    /// Resource group hosting the registry.
    pub resource_group: String,
    /// ACR name (must be globally unique, alphanumeric).
    pub registry_name: String,
    /// Login server URL (`<name>.azurecr.io`).
    pub login_server: String,
    /// UAMI principal id with `AcrPull` access.
    pub pull_principal_id: String,
    /// UAMI principal id with `AcrPush` access.
    pub push_principal_id: String,
}
