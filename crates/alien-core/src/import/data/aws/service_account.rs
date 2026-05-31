use serde::{Deserialize, Serialize};

/// AWS ServiceAccount ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsServiceAccountImportData {
    /// IAM role name.
    pub role_name: String,
    /// IAM role ARN.
    pub role_arn: String,
    /// Whether stack-level permissions were attached by the generated stack.
    #[serde(deserialize_with = "crate::import::data::deserialize_bool_from_bool_or_string")]
    pub stack_permissions_applied: bool,
}
