use serde::{Deserialize, Serialize};

/// AWS Vault ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsVaultImportData {
    /// AWS account ID that owns the Parameter Store namespace.
    pub account_id: String,
    /// AWS region containing the Parameter Store namespace.
    pub region: String,
    /// Prefix used for SecureString parameters in this vault.
    pub parameter_prefix: String,
}
