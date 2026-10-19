use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// AWS subnets grouped by their real availability zone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsFailureDomainSubnets {
    /// Public subnet IDs in this availability zone.
    #[serde(default)]
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids")]
    pub public_subnet_ids: Vec<String>,
    /// Private subnet IDs in this availability zone.
    #[serde(default)]
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids")]
    pub private_subnet_ids: Vec<String>,
}

/// Setup network coordinates; the deployment controller verifies actual EC2 routes and CIDRs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsSandboxNetwork {
    /// Route-less connector subnet identifiers.
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids_or_csv")]
    pub sandbox_subnet_ids: Vec<String>,
    /// Dedicated NAT-routed subnets for the gateway's internal load balancer.
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids_or_csv")]
    pub gateway_subnet_ids: Vec<String>,
    /// Source ranges permitted on the gateway proxy port.
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids_or_csv")]
    pub sandbox_cidrs: Vec<String>,
    /// Only destinations permitted by the connector security group.
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids_or_csv")]
    pub gateway_cidrs: Vec<String>,
    /// Source ranges permitted on the gateway admin port.
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids_or_csv")]
    pub workload_cidrs: Vec<String>,
    /// Every associated VPC IPv4 range, denied by the gateway.
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids_or_csv")]
    pub vpc_cidrs: Vec<String>,
}

/// AWS Network ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
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
    /// Public addresses allocated to this deployment's NAT; empty when unknown.
    #[serde(default)]
    pub egress_public_ips: Vec<String>,
    /// Dedicated sandbox networking to verify after setup; absent on older imports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_network: Option<AwsSandboxNetwork>,
    /// Public subnet IDs.
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids")]
    pub public_subnet_ids: Vec<String>,
    /// Private subnet IDs.
    #[serde(deserialize_with = "crate::network_ids::deserialize_ids")]
    pub private_subnet_ids: Vec<String>,
    /// Public route table ID.
    pub public_route_table_id: Option<String>,
    /// Private route table ID.
    pub private_route_table_id: Option<String>,
    /// Security group ID for private workloads.
    // Keeps the published schema unchanged: without it, `default` adds `"default": null`.
    #[cfg_attr(
        feature = "jsonschema",
        schemars(skip_serializing_if = "Option::is_none")
    )]
    #[serde(
        default,
        deserialize_with = "crate::network_ids::deserialize_optional_id"
    )]
    pub security_group_id: Option<String>,
    /// Availability zone names used by created or BYO subnets.
    pub availability_zones: Vec<String>,
    /// Exact subnet membership keyed by real availability zone.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub subnets_by_failure_domain: BTreeMap<String, AwsFailureDomainSubnets>,
    /// True when the VPC is owned outside this stack.
    #[serde(deserialize_with = "crate::import::data::deserialize_bool_from_bool_or_string")]
    pub is_byo_vpc: bool,
}
