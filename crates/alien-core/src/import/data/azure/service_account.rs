use serde::{Deserialize, Serialize};

/// Azure ServiceAccount ImportData — User-Assigned Managed Identity
/// (UAMI) plus optional Federated Identity Credentials trusting Alien's
/// OIDC issuer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureServiceAccountImportData {
    /// Subscription ID containing the UAMI.
    pub subscription_id: String,
    /// Resource group containing the UAMI.
    pub resource_group: String,
    /// UAMI resource id (full ARM path).
    pub identity_id: String,
    /// UAMI principal id (object id of the service principal).
    pub principal_id: String,
    /// UAMI client id used by `azure.workload.identity/client-id`
    /// annotations.
    pub client_id: String,
    /// Whether stack-level role assignments were attached by the
    /// generated stack.
    #[serde(deserialize_with = "crate::import::data::deserialize_bool_from_bool_or_string")]
    pub stack_permissions_applied: bool,
}
