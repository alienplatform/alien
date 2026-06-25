use serde::{Deserialize, Serialize};

/// AWS Postgres (Aurora Serverless v2) registration data: the handles by which a
/// setup-created Aurora cluster is registered as a Frozen Postgres resource. Setup owns
/// the cluster, Alien refreshes and heartbeats it. Carries the master password's Secrets
/// Manager ARN, never the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsPostgresImportData {
    /// Aurora DB cluster identifier.
    pub cluster_identifier: String,
    /// Writer instance identifier within the cluster.
    pub instance_identifier: String,
    /// DB subnet group spanning the private subnets.
    pub subnet_group_name: String,
    /// Dedicated security group admitting 5432 from the stack security group.
    pub security_group_id: String,
    /// Secrets Manager ARN of the master password (never the password itself).
    pub password_secret_arn: String,
    /// Cluster writer endpoint (the binding host).
    pub cluster_endpoint: String,
    /// Default database name.
    pub database: String,
    /// Master username.
    pub username: String,
    /// Engine version the cluster reports.
    pub engine_version: String,
}
