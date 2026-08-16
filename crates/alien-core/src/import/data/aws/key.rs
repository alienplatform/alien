use serde::{Deserialize, Serialize};

/// AWS KMS key created by the customer-installed setup stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsKeyImportData {
    /// Immutable KMS key ARN. Alias ARNs are not accepted.
    pub key_arn: String,
}
