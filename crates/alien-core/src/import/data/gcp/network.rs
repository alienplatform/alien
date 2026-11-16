use serde::{Deserialize, Serialize};

/// GCP Network ImportData — VPC + subnetwork + Cloud NAT topology.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpNetworkImportData {
    /// Project ID owning the network.
    pub project_id: String,
    /// VPC self-link. Absent when the controller will look up the
    /// default network at runtime.
    pub vpc_self_link: Option<String>,
    /// VPC short name.
    pub vpc_name: Option<String>,
    /// Subnetwork self-links across the configured regions.
    pub subnet_self_links: Vec<String>,
    /// Primary subnetwork CIDR block used for internal firewall rules.
    pub cidr_block: Option<String>,
    /// Cloud Router self-link backing Cloud NAT (when created).
    pub router_self_link: Option<String>,
    /// Cloud NAT name (when created).
    pub nat_name: Option<String>,
    /// True when the VPC is owned outside this stack.
    pub is_byo_vpc: bool,
    /// Setup-owned network attachment and egress gateway VMs for Internet sandboxes. Absent for a
    /// deployment without them, and for setup releases that predate them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_network: Option<GcpSandboxNetwork>,
}

/// What setup builds so sandbox egress can reach the deployment's egress gateway: a network
/// attachment whose subnet Agent Platform draws sandbox interfaces from, and the gateway VMs.
///
/// Only tcp/22 crosses from Agent Platform's tenant into the VPC (measured; Google's docs do
/// not say so), so the sandboxes reach the gateway's proxy on `proxy_port`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpSandboxNetwork {
    /// Full path of the network attachment sandbox templates name:
    /// `projects/{project}/regions/{region}/networkAttachments/{name}`.
    pub network_attachment: String,
    /// Range of the attachment's own subnet. Sandbox traffic enters the VPC from it.
    pub attachment_cidr: String,
    /// Range of the gateway subnet, the only one with Cloud NAT.
    pub gateway_cidr: String,
    /// Static private IPs of the gateway VMs, one per replica.
    pub gateway_ips: Vec<String>,
    /// The tcp port the gateway's proxy answers sandboxes on. The admin port is never reachable
    /// from the attachment range.
    pub proxy_port: u16,
    /// Every VPC range, excluded from gateway destinations.
    pub vpc_cidrs: Vec<String>,
}
