use serde::{Deserialize, Serialize};

/// AWS RemoteStackManagement ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsRemoteStackManagementImportData {
    /// Cross-account management role name.
    pub role_name: String,
    /// Cross-account management role ARN.
    pub role_arn: String,
    /// Whether the management inline policy was attached by the generated stack.
    #[serde(deserialize_with = "crate::import::data::deserialize_bool_from_bool_or_string")]
    pub management_permissions_applied: bool,
    /// Setup-owned role used only for opted-in remote binding data access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_bindings_role_arn: Option<String>,
}
