use serde::{Deserialize, Serialize};

/// GCP ComputeCluster ImportData — GKE node pool identity + network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpComputeClusterImportData {
    /// Cluster identifier used by the controller.
    pub cluster_id: String,
    /// Service account email attached to cluster nodes.
    pub node_service_account_email: String,
    /// Tag applied to firewall rules targeting cluster nodes.
    pub network_tag: String,
}
