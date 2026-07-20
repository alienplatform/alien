use super::common::{Filter, TagSet, TagSpecification};
use bon::Builder;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// VPC Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe VPCs.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeVpcsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing VPCs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeVpcsResponse {
    #[serde(rename = "vpcSet")]
    pub vpc_set: Option<VpcSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Vpc>,
}

/// Represents a VPC.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vpc {
    pub vpc_id: Option<String>,
    pub state: Option<String>,
    pub cidr_block: Option<String>,
    pub owner_id: Option<String>,
    pub instance_tenancy: Option<String>,
    pub is_default: Option<bool>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

/// Request to describe a VPC attribute.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DescribeVpcAttributeRequest {
    pub vpc_id: String,
    /// The attribute to describe: "enableDnsSupport" or "enableDnsHostnames"
    pub attribute: String,
}

/// Response from describing a VPC attribute.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeVpcAttributeResponse {
    pub vpc_id: Option<String>,
    #[serde(rename = "enableDnsSupport")]
    pub enable_dns_support: Option<AttributeBooleanValue>,
    #[serde(rename = "enableDnsHostnames")]
    pub enable_dns_hostnames: Option<AttributeBooleanValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeBooleanValue {
    pub value: Option<bool>,
}

/// Request to create a VPC.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateVpcRequest {
    pub cidr_block: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_tenancy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amazon_provided_ipv6_cidr_block: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a VPC.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVpcResponse {
    pub vpc: Option<Vpc>,
}

/// Request to modify VPC attributes.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ModifyVpcAttributeRequest {
    pub vpc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_dns_support: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_dns_hostnames: Option<bool>,
}

// ---------------------------------------------------------------------------
// Subnet Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe subnets.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeSubnetsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing subnets.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeSubnetsResponse {
    #[serde(rename = "subnetSet")]
    pub subnet_set: Option<SubnetSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubnetSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Subnet>,
}

/// Represents a subnet.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subnet {
    pub subnet_id: Option<String>,
    pub vpc_id: Option<String>,
    pub state: Option<String>,
    pub cidr_block: Option<String>,
    pub availability_zone: Option<String>,
    pub availability_zone_id: Option<String>,
    pub available_ip_address_count: Option<i32>,
    pub default_for_az: Option<bool>,
    pub map_public_ip_on_launch: Option<bool>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

/// Request to create a subnet.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateSubnetRequest {
    pub vpc_id: String,
    pub cidr_block: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a subnet.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubnetResponse {
    pub subnet: Option<Subnet>,
}

// ---------------------------------------------------------------------------
// Internet Gateway Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create an internet gateway.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct CreateInternetGatewayRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating an internet gateway.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInternetGatewayResponse {
    #[serde(rename = "internetGateway")]
    pub internet_gateway: Option<InternetGateway>,
}

/// Represents an internet gateway.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternetGateway {
    pub internet_gateway_id: Option<String>,
    #[serde(rename = "attachmentSet")]
    pub attachment_set: Option<InternetGatewayAttachmentSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternetGatewayAttachmentSet {
    #[serde(rename = "item", default)]
    pub items: Vec<InternetGatewayAttachment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternetGatewayAttachment {
    pub vpc_id: Option<String>,
    pub state: Option<String>,
}

/// Request to attach an internet gateway to a VPC.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AttachInternetGatewayRequest {
    pub internet_gateway_id: String,
    pub vpc_id: String,
}

/// Request to detach an internet gateway from a VPC.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DetachInternetGatewayRequest {
    pub internet_gateway_id: String,
    pub vpc_id: String,
}

/// Request to describe internet gateways.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeInternetGatewaysRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internet_gateway_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing internet gateways.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeInternetGatewaysResponse {
    #[serde(rename = "internetGatewaySet")]
    pub internet_gateway_set: Option<InternetGatewaySet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternetGatewaySet {
    #[serde(rename = "item", default)]
    pub items: Vec<InternetGateway>,
}

// ---------------------------------------------------------------------------
// NAT Gateway Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a NAT gateway.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateNatGatewayRequest {
    /// The subnet in which to create the NAT gateway.
    pub subnet_id: String,
    /// [Public NAT only] The allocation ID of the Elastic IP address for the gateway.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation_id: Option<String>,
    /// The connectivity type: "public" (default) or "private".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connectivity_type: Option<String>,
    /// The private IPv4 address to assign to the NAT gateway.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a NAT gateway.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNatGatewayResponse {
    #[serde(rename = "natGateway")]
    pub nat_gateway: Option<NatGateway>,
}

/// Represents a NAT gateway.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGateway {
    pub nat_gateway_id: Option<String>,
    pub subnet_id: Option<String>,
    pub vpc_id: Option<String>,
    pub state: Option<String>,
    pub connectivity_type: Option<String>,
    #[serde(rename = "natGatewayAddressSet")]
    pub nat_gateway_address_set: Option<NatGatewayAddressSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGatewayAddressSet {
    #[serde(rename = "item", default)]
    pub items: Vec<NatGatewayAddress>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGatewayAddress {
    pub allocation_id: Option<String>,
    pub network_interface_id: Option<String>,
    pub private_ip: Option<String>,
    pub public_ip: Option<String>,
}

/// Response from deleting a NAT gateway.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteNatGatewayResponse {
    pub nat_gateway_id: Option<String>,
}

/// Request to describe NAT gateways.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeNatGatewaysRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_gateway_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing NAT gateways.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeNatGatewaysResponse {
    #[serde(rename = "natGatewaySet")]
    pub nat_gateway_set: Option<NatGatewaySet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGatewaySet {
    #[serde(rename = "item", default)]
    pub items: Vec<NatGateway>,
}

// ---------------------------------------------------------------------------
// Elastic IP Request/Response Types
// ---------------------------------------------------------------------------

/// Request to allocate an Elastic IP address.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct AllocateAddressRequest {
    /// The domain: "vpc" (default) or "standard".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from allocating an Elastic IP address.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocateAddressResponse {
    pub public_ip: Option<String>,
    pub allocation_id: Option<String>,
    pub domain: Option<String>,
}

// ---------------------------------------------------------------------------
// Route Table Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe route tables.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeRouteTablesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_table_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing route tables.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeRouteTablesResponse {
    #[serde(rename = "routeTableSet")]
    pub route_table_set: Option<RouteTableSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTableSet {
    #[serde(rename = "item", default)]
    pub items: Vec<RouteTable>,
}

/// Represents a route table.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTable {
    pub route_table_id: Option<String>,
    pub vpc_id: Option<String>,
    #[serde(rename = "routeSet")]
    pub route_set: Option<RouteSet>,
    #[serde(rename = "associationSet")]
    pub association_set: Option<RouteTableAssociationSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Route>,
}

/// Represents a route in a route table.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub destination_cidr_block: Option<String>,
    pub gateway_id: Option<String>,
    pub nat_gateway_id: Option<String>,
    pub instance_id: Option<String>,
    pub network_interface_id: Option<String>,
    pub vpc_peering_connection_id: Option<String>,
    pub transit_gateway_id: Option<String>,
    pub state: Option<String>,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTableAssociationSet {
    #[serde(rename = "item", default)]
    pub items: Vec<RouteTableAssociation>,
}

/// Represents an association between a route table and a subnet.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTableAssociation {
    pub route_table_association_id: Option<String>,
    pub route_table_id: Option<String>,
    pub subnet_id: Option<String>,
    pub main: Option<bool>,
}

/// Request to create a route table.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateRouteTableRequest {
    pub vpc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a route table.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRouteTableResponse {
    #[serde(rename = "routeTable")]
    pub route_table: Option<RouteTable>,
}

/// Request to create a route.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateRouteRequest {
    pub route_table_id: String,
    pub destination_cidr_block: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_gateway_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_interface_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_peering_connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transit_gateway_id: Option<String>,
}

/// Request to delete a route.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DeleteRouteRequest {
    pub route_table_id: String,
    pub destination_cidr_block: String,
}

/// Request to associate a route table with a subnet.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AssociateRouteTableRequest {
    pub route_table_id: String,
    pub subnet_id: String,
}

/// Response from associating a route table.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociateRouteTableResponse {
    pub association_id: Option<String>,
}
