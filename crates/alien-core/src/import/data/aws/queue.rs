use serde::{Deserialize, Serialize};

/// AWS Queue ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsQueueImportData {
    /// SQS queue name.
    pub queue_name: String,
    /// SQS queue URL.
    pub queue_url: String,
    /// SQS queue ARN.
    pub queue_arn: String,
}
