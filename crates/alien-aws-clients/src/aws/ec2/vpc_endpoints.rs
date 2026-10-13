//! Interface endpoint query types. Collections retain pagination and per-resource failures.

use super::{Filter, GroupIdentifierSet, TagSet, TagSpecification};
use bon::Builder;
use serde::{Deserialize, Serialize};

/// Create a private interface endpoint with explicit DNS, policy, and replay choices.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateInterfaceVpcEndpointRequest {
    pub vpc_id: String,
    pub service_name: String,
    pub subnet_ids: Vec<String>,
    pub security_group_ids: Vec<String>,
    /// Persist this token before the first create; reuse it after an uncertain response.
    pub client_token: String,
    /// Disabling this avoids changing resolution of the service's public hostnames.
    pub private_dns_enabled: bool,
    pub policy_document: String,
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVpcEndpointResponse {
    pub vpc_endpoint: VpcEndpoint,
    pub client_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder, Default)]
#[serde(rename_all = "camelCase")]
pub struct DescribeVpcEndpointsRequest {
    pub vpc_endpoint_ids: Option<Vec<String>>,
    pub filters: Option<Vec<Filter>>,
    pub max_results: Option<i32>,
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeVpcEndpointsResponse {
    pub vpc_endpoint_set: VpcEndpointSet,
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcEndpointSet {
    #[serde(rename = "item", default)]
    pub items: Vec<VpcEndpoint>,
}

/// Native endpoint identity, attachment and ownership metadata.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcEndpoint {
    pub vpc_endpoint_id: String,
    pub vpc_endpoint_type: Option<String>,
    pub vpc_id: String,
    pub service_name: String,
    pub state: String,
    pub owner_id: Option<String>,
    pub requester_managed: Option<bool>,
    pub private_dns_enabled: Option<bool>,
    pub policy_document: Option<String>,
    pub subnet_id_set: Option<VpcEndpointIdSet>,
    pub network_interface_id_set: Option<VpcEndpointIdSet>,
    pub group_set: Option<GroupIdentifierSet>,
    pub dns_entry_set: Option<VpcEndpointDnsEntrySet>,
    pub tag_set: Option<TagSet>,
    pub last_error: Option<VpcEndpointError>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcEndpointIdSet {
    #[serde(rename = "item", default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcEndpointDnsEntrySet {
    #[serde(rename = "item", default)]
    pub items: Vec<VpcEndpointDnsEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcEndpointDnsEntry {
    pub dns_name: String,
    pub hosted_zone_id: String,
}

/// A successful batch HTTP response can still contain rejected endpoint deletions.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteVpcEndpointsResponse {
    pub unsuccessful: VpcEndpointUnsuccessfulSet,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcEndpointUnsuccessfulSet {
    #[serde(rename = "item", default)]
    pub items: Vec<VpcEndpointUnsuccessfulItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcEndpointUnsuccessfulItem {
    pub resource_id: String,
    pub error: VpcEndpointError,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcEndpointError {
    pub code: String,
    pub message: String,
}
