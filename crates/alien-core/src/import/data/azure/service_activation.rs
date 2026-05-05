use serde::{Deserialize, Serialize};

/// Azure ServiceActivation ImportData — registers an Azure resource
/// provider on the customer's subscription.
///
/// Added by the `AzureServiceActivationMutation` preflight when the
/// stack contains resources that depend on a specific Azure RP
/// (Microsoft.App, Microsoft.Storage, Microsoft.KeyVault, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureServiceActivationImportData {
    /// Subscription ID the provider was registered on.
    pub subscription_id: String,
    /// Resource provider namespace (e.g. `Microsoft.App`).
    pub provider_namespace: String,
    /// True once the provider is reported `Registered`.
    pub registered: bool,
}
