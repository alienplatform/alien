use serde::{Deserialize, Serialize};

/// Azure ComputeCluster ImportData — AKS node pool identity / network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureComputeClusterImportData {
    /// Cluster identifier used by the controller.
    pub cluster_id: String,
    /// Resource ID of the user-assigned identity attached to cluster VMs.
    pub identity_id: String,
    /// AKS cluster identity principal id (system-assigned identity).
    pub cluster_identity_principal_id: String,
    /// kubelet UAMI client id used by node pools.
    pub kubelet_identity_client_id: String,
}
