use serde::{Deserialize, Serialize};

/// AWS ComputeCluster ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsComputeClusterImportData {
    /// Cluster identifier used by container orchestration.
    pub cluster_id: String,
    /// IAM instance profile ARN for cluster machines.
    pub instance_profile_arn: String,
    /// Security group ID attached to cluster machines.
    pub security_group_id: String,
}
