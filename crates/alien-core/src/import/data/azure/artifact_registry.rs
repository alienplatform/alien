use serde::{Deserialize, Serialize};

/// Azure ArtifactRegistry ImportData — an Azure Container Registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
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
    /// Setup-owned pull identity ARM ID; absent in older setup payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_identity_resource_id: Option<String>,
    /// Setup-owned registry-scoped AcrPull assignment ARM ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_role_assignment_id: Option<String>,
    /// UAMI principal id with `AcrPush` access.
    pub push_principal_id: String,
}
