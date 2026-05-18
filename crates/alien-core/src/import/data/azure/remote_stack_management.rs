use serde::{Deserialize, Serialize};

/// Azure RemoteStackManagement ImportData — UAMI + Federated Identity
/// Credential the manager uses to act on this stack via Azure AD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureRemoteStackManagementImportData {
    /// Subscription ID containing the management identity.
    pub subscription_id: String,
    /// Resource group containing the management identity.
    pub resource_group: String,
    /// Tenant ID containing the management identity.
    pub tenant_id: String,
    /// Management UAMI resource id.
    pub identity_id: String,
    /// Management UAMI principal id.
    pub principal_id: String,
    /// Management UAMI client id (used by the manager to call AAD).
    pub client_id: String,
    /// Whether the management role assignments were applied by the
    /// generated stack.
    #[serde(deserialize_with = "crate::import::data::deserialize_bool_from_bool_or_string")]
    pub management_permissions_applied: bool,
}
