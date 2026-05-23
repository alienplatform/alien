use serde::{Deserialize, Serialize};

/// AWS Network ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsNetworkImportData {
    /// VPC ID. Absent when default-VPC mode defers lookup to AWS at runtime.
    pub vpc_id: Option<String>,
    /// VPC CIDR block when the VPC is created by this stack.
    pub cidr_block: Option<String>,
    /// Internet gateway ID when created by this stack.
    pub internet_gateway_id: Option<String>,
    /// NAT gateway ID when created by this stack.
    pub nat_gateway_id: Option<String>,
    /// Elastic IP allocation ID backing the NAT gateway.
    pub eip_allocation_id: Option<String>,
    /// Public subnet IDs.
    pub public_subnet_ids: Vec<String>,
    /// Private subnet IDs.
    pub private_subnet_ids: Vec<String>,
    /// Public route table ID.
    pub public_route_table_id: Option<String>,
    /// Private route table ID.
    pub private_route_table_id: Option<String>,
    /// Security group ID for private workloads.
    pub security_group_id: Option<String>,
    /// Availability zone names used by created or BYO subnets.
    pub availability_zones: Vec<String>,
    /// True when the VPC is owned outside this stack.
    #[serde(deserialize_with = "crate::import::data::deserialize_bool_from_bool_or_string")]
    pub is_byo_vpc: bool,
}
