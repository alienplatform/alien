use serde::{Deserialize, Serialize};

/// Azure Worker ImportData — a Container App backed by the stack's
/// Container Apps Environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureWorkerImportData {
    /// Subscription ID hosting the Container App.
    pub subscription_id: String,
    /// Resource group hosting the Container App.
    pub resource_group: String,
    /// Container App name.
    pub container_app_name: String,
    /// Public FQDN when ingress is enabled.
    pub fqdn: Option<String>,
}
