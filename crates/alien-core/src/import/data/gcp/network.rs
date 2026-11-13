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
    /// Self-link of the network attachment sandboxes use for Private Service Connect egress.
    /// Present only when the stack has a sandbox with `internet` egress or preview ports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_psc_attachment_self_link: Option<String>,
}
