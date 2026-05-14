use serde::{Deserialize, Serialize};

/// AWS Storage ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsStorageImportData {
    /// S3 bucket name.
    pub bucket_name: String,
    /// S3 bucket ARN.
    pub bucket_arn: String,
}
