use serde::{Deserialize, Serialize};

/// AWS KV ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsKvImportData {
    /// DynamoDB table name.
    pub table_name: String,
    /// DynamoDB table ARN.
    pub table_arn: String,
}
