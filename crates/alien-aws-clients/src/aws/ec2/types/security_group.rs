use super::common::{Filter, TagSet, TagSpecification};
use bon::Builder;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Security Group Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe security groups.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeSecurityGroupsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing security groups.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeSecurityGroupsResponse {
    #[serde(rename = "securityGroupInfo")]
    pub security_group_info: Option<SecurityGroupSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityGroupSet {
    #[serde(rename = "item", default)]
    pub items: Vec<SecurityGroup>,
}

/// Represents a security group.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityGroup {
    pub group_id: Option<String>,
    pub group_name: Option<String>,
    pub vpc_id: Option<String>,
    pub owner_id: Option<String>,
    pub group_description: Option<String>,
    #[serde(rename = "ipPermissions")]
    pub ip_permissions: Option<IpPermissionSet>,
    #[serde(rename = "ipPermissionsEgress")]
    pub ip_permissions_egress: Option<IpPermissionSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

/// Request to describe network interfaces.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeNetworkInterfacesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_interface_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing network interfaces.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeNetworkInterfacesResponse {
    #[serde(rename = "networkInterfaceSet")]
    pub network_interface_set: Option<NetworkInterfaceSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterfaceSet {
    #[serde(rename = "item", default)]
    pub items: Vec<NetworkInterface>,
}

/// Represents an EC2 network interface.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    pub network_interface_id: Option<String>,
    pub status: Option<String>,
    pub description: Option<String>,
    pub subnet_id: Option<String>,
    pub vpc_id: Option<String>,
    #[serde(rename = "groupSet")]
    pub group_set: Option<GroupIdentifierSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupIdentifierSet {
    #[serde(rename = "item", default)]
    pub items: Vec<GroupIdentifier>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupIdentifier {
    pub group_id: Option<String>,
    pub group_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpPermissionSet {
    #[serde(rename = "item", default)]
    pub items: Vec<IpPermissionResponse>,
}

/// IP permission in a response (from DescribeSecurityGroups).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpPermissionResponse {
    pub ip_protocol: Option<String>,
    pub from_port: Option<i32>,
    pub to_port: Option<i32>,
    #[serde(rename = "ipRanges")]
    pub ip_ranges: Option<IpRangeSet>,
    #[serde(rename = "ipv6Ranges")]
    pub ipv6_ranges: Option<Ipv6RangeSet>,
    #[serde(rename = "groups")]
    pub groups: Option<UserIdGroupPairSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpRangeSet {
    #[serde(rename = "item", default)]
    pub items: Vec<IpRangeResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpRangeResponse {
    pub cidr_ip: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ipv6RangeSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Ipv6RangeResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ipv6RangeResponse {
    pub cidr_ipv6: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserIdGroupPairSet {
    #[serde(rename = "item", default)]
    pub items: Vec<UserIdGroupPairResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserIdGroupPairResponse {
    pub group_id: Option<String>,
    pub user_id: Option<String>,
    pub description: Option<String>,
}

/// Request to create a security group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateSecurityGroupRequest {
    pub group_name: String,
    pub description: String,
    pub vpc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a security group.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSecurityGroupResponse {
    pub group_id: Option<String>,
}

/// IP permission for requests (authorize/revoke).
#[derive(Debug, Clone, Serialize, Builder)]
pub struct IpPermission {
    /// The IP protocol: tcp, udp, icmp, or -1 for all.
    pub ip_protocol: String,
    /// The start of port range (or ICMP type).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_port: Option<i32>,
    /// The end of port range (or ICMP code).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_port: Option<i32>,
    /// The IPv4 CIDR ranges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_ranges: Option<Vec<IpRange>>,
    /// The IPv6 CIDR ranges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_ranges: Option<Vec<Ipv6Range>>,
    /// The security group and AWS account ID pairs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id_group_pairs: Option<Vec<UserIdGroupPair>>,
}

/// IPv4 CIDR range for security group rules.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct IpRange {
    pub cidr_ip: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// IPv6 CIDR range for security group rules.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Ipv6Range {
    pub cidr_ipv6: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Security group and AWS account ID pair for security group rules.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct UserIdGroupPair {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request to authorize security group ingress.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AuthorizeSecurityGroupIngressRequest {
    pub group_id: String,
    pub ip_permissions: Vec<IpPermission>,
}

/// Request to authorize security group egress.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AuthorizeSecurityGroupEgressRequest {
    pub group_id: String,
    pub ip_permissions: Vec<IpPermission>,
}

/// Request to revoke security group ingress.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct RevokeSecurityGroupIngressRequest {
    pub group_id: String,
    pub ip_permissions: Vec<IpPermission>,
}

/// Request to revoke security group egress.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct RevokeSecurityGroupEgressRequest {
    pub group_id: String,
    pub ip_permissions: Vec<IpPermission>,
}
