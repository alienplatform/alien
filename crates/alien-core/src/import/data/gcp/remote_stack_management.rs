use serde::{Deserialize, Serialize};

/// GCP RemoteStackManagement ImportData — cross-project service account
/// the manager impersonates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpRemoteStackManagementImportData {
    /// Project ID containing the management service account.
    pub project_id: String,
    /// Numeric project number containing the management service account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_number: Option<String>,
    /// Service account email the manager impersonates.
    pub service_account_email: String,
    /// Stable unique id Google assigns to the service account.
    pub service_account_unique_id: String,
    /// Whether the management role binding was attached by the
    /// generated stack.
    #[serde(deserialize_with = "crate::import::data::deserialize_bool_from_bool_or_string")]
    pub management_permissions_applied: bool,
}
