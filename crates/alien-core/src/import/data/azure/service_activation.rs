use serde::{Deserialize, Serialize};

/// Azure ServiceActivation ImportData — names an Azure resource provider the
/// subscription must have registered.
///
/// Added by the `AzureServiceActivationMutation` preflight when the
/// stack contains resources that depend on a specific Azure RP
/// (Microsoft.App, Microsoft.Storage, Microsoft.KeyVault, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureServiceActivationImportData {
    /// Subscription ID the provider was registered on.
    pub subscription_id: String,
    /// Resource provider namespace (e.g. `Microsoft.App`).
    pub provider_namespace: String,
    /// Whether setup established the registration. False from a Terraform package: it registers
    /// nothing, and the runtime reads the real state on its first refresh.
    pub registered: bool,
}
