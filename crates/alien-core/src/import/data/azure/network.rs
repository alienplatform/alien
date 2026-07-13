use serde::{Deserialize, Serialize};

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
    /// NAT gateway resource id when one was created.
    pub nat_gateway_id: Option<String>,
    /// Network Security Group resource id attached to workload subnets.
    pub network_security_group_id: Option<String>,
    /// True when the VNet is owned outside this stack.
    pub is_byo_vnet: bool,
}
