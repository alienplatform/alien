use serde::{Deserialize, Serialize};

/// AWS AI ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsAiImportData {
    /// AWS region where Bedrock is accessed.
    pub region: String,
}
