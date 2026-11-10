use serde::{Deserialize, Serialize};

/// AWS Redis (ElastiCache for Valkey) registration data: the handles by which a
/// setup-created replication group is registered as a Frozen Redis resource. Setup owns
/// the group, Alien refreshes and heartbeats it. Carries the AUTH token's Secrets Manager
/// ARN, never the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsRedisImportData {
    /// Replication group id.
    pub replication_group_id: String,
    /// Cache subnet group.
    pub subnet_group_name: String,
    /// Dedicated security group admitting 6379 from the workload security groups.
    pub security_group_id: String,
    /// Cache parameter group holding `maxmemory-policy`.
    pub parameter_group_name: String,
    /// Secrets Manager ARN of the AUTH token (never the token itself).
    pub auth_secret_arn: String,
    /// Primary endpoint (the binding host).
    pub endpoint: String,
    /// Listening port.
    pub port: u16,
    /// Node type the group runs.
    pub node_type: String,
    /// Engine version the group reports.
    pub engine_version: String,
    /// Whether the group has a replica.
    #[serde(default)]
    pub high_availability: bool,
}
