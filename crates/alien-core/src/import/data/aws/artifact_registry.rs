use serde::{Deserialize, Serialize};

/// AWS ArtifactRegistry ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsArtifactRegistryImportData {
    /// AWS account ID that owns the ECR registry.
    pub account_id: String,
    /// AWS region for the registry endpoint.
    pub region: String,
    /// Registry identifier in `account:region` form.
    pub registry_id: String,
    /// Docker registry endpoint.
    pub registry_endpoint: String,
    /// Prefix used for repositories owned by this stack.
    pub repository_prefix: String,
    /// IAM role ARN with pull-only access.
    pub pull_role_arn: String,
    /// IAM role ARN with push and pull access.
    pub push_role_arn: String,
}
