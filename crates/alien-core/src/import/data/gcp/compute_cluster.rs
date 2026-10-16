use serde::{Deserialize, Serialize};

/// GCP ComputeCluster ImportData — GKE node pool identity + network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpComputeClusterImportData {
    /// Exact custom-role namespace selected by setup for the node identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_role_namespace: Option<String>,
    /// Cluster identifier used by the controller.
    pub cluster_id: String,
    /// Service account email attached to cluster nodes.
    pub node_service_account_email: String,
    /// Tag applied to firewall rules targeting cluster nodes.
    pub network_tag: String,
}
