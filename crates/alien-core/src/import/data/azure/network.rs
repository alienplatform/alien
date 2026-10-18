use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Azure Network ImportData — VNet + subnets + NAT topology.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureNetworkImportData {
    /// Subscription ID containing the VNet.
    pub subscription_id: String,
    /// Resource group containing the VNet.
    pub resource_group: String,
    /// VNet resource id (full ARM path). Absent when the controller
    /// will look up the default VNet at runtime.
    pub vnet_id: Option<String>,
    /// VNet short name.
    pub vnet_name: Option<String>,
    /// Subnet resource ids in this VNet used by workloads.
    pub subnet_ids: Vec<String>,
    /// Dedicated subnet for classic Azure Application Gateway ingress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_gateway_subnet_id: Option<String>,
    /// Dedicated subnet name for classic Azure Application Gateway ingress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_gateway_subnet_name: Option<String>,
    /// Dedicated subnet name for Private Endpoints (e.g. Postgres Flexible Server). Distinct from
    /// the Container Apps infrastructure ("private") subnet, which a Private Endpoint cannot share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_endpoint_subnet_name: Option<String>,
    /// Setup-owned sandbox and internal gateway subnet coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_network: Option<AzureSandboxNetwork>,
    /// NAT gateway resource id when one was created.
    pub nat_gateway_id: Option<String>,
    /// NAT public addresses owned by setup; empty for an unknown external egress path.
    #[serde(default)]
    pub egress_public_ips: Vec<String>,
    /// Network Security Group resource id attached to workload subnets.
    pub network_security_group_id: Option<String>,
    /// True when the VNet is owned outside this stack.
    pub is_byo_vnet: bool,
}

/// Setup-owned subnet coordinates consumed by an internal sandbox gateway load balancer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureSandboxNetwork {
    /// Dedicated delegated subnets whose sandboxes use the gateway.
    pub sandbox_subnet_ids: Vec<String>,
    /// Undelegated subnet reserved for the gateway load balancer's frontend.
    pub gateway_subnet_ids: Vec<String>,
    /// Sandbox source ranges allowed on the proxy listener.
    pub sandbox_cidrs: Vec<String>,
    /// Dedicated source ranges keyed by Sandbox resource id for exact linked-resource access.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sandbox_cidrs_by_resource: BTreeMap<String, Vec<String>>,
    /// Gateway ranges allowed by the sandbox subnet NSGs on TCP3128 and TCP3129.
    pub gateway_cidrs: Vec<String>,
    /// Workload source ranges allowed on the authenticated admin listener.
    pub workload_cidrs: Vec<String>,
    /// Every VNet range excluded from gateway destinations.
    pub vpc_cidrs: Vec<String>,
}
