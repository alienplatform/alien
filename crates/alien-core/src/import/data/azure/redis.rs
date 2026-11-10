use serde::{Deserialize, Serialize};

/// Azure Managed Redis registration data: the handles by which a setup-created cluster is
/// registered as a Frozen Redis resource. Setup owns the cluster, Alien refreshes and
/// heartbeats it. The access key is never carried; workloads list it from the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureManagedRedisImportData {
    /// Cluster name.
    pub cluster_name: String,
    /// Full ARM resource ID of the cluster.
    pub cluster_resource_id: String,
    /// Resource group of the cluster.
    pub resource_group: String,
    /// Private endpoint that fronts the cluster.
    pub private_endpoint_name: String,
    /// Endpoint host name (resolves to the private endpoint inside the VNet).
    pub host: String,
    /// Listening port.
    pub port: u16,
    /// SKU the cluster runs, such as `Balanced_B1`.
    pub sku: String,
    /// Whether high availability is on.
    #[serde(default)]
    pub high_availability: bool,
}
