//! Markers on the `default-network` and `kubernetes` resources the preflight mutations add.
//!
//! A generator that only sees the prepared stack needs to tell a resource the source stack
//! declares (always expected) from one a mutation added because of the deployment's settings
//! (expected only for the settings the module is applied with).

use crate::ResourceEntry;

/// Id of the network the network mutation adds.
pub const PREPARED_NETWORK_ID: &str = "default-network";
/// Display name of the network the network mutation adds.
pub const PREPARED_NETWORK_DISPLAY_NAME: &str = "Network";
/// Description of the network the network mutation adds.
pub const PREPARED_NETWORK_DESCRIPTION: &str =
    "Provides the network and subnet connections used by application resources.";

/// Id of the cluster the Kubernetes cluster mutation adds.
pub const PREPARED_CLUSTER_ID: &str = "kubernetes";
/// Display name of the cluster the Kubernetes cluster mutation adds.
pub const PREPARED_CLUSTER_DISPLAY_NAME: &str = "Kubernetes cluster";
/// Description of the cluster the Kubernetes cluster mutation adds.
pub const PREPARED_CLUSTER_DESCRIPTION: &str =
    "Provides the Kubernetes cluster used by application workloads.";

/// True for a `default-network` the network mutation added, false for one the stack declares.
pub fn is_prepared_network(id: &str, entry: &ResourceEntry) -> bool {
    id == PREPARED_NETWORK_ID
        && entry.enabled_when.is_none()
        && entry.display_name.as_deref() == Some(PREPARED_NETWORK_DISPLAY_NAME)
        && entry.description.as_deref() == Some(PREPARED_NETWORK_DESCRIPTION)
}

/// True for a cluster the Kubernetes cluster mutation added, false for one the stack declares.
pub fn is_prepared_cluster(id: &str, entry: &ResourceEntry) -> bool {
    id == PREPARED_CLUSTER_ID
        && entry.enabled_when.is_none()
        && entry.display_name.as_deref() == Some(PREPARED_CLUSTER_DISPLAY_NAME)
        && entry.description.as_deref() == Some(PREPARED_CLUSTER_DESCRIPTION)
}
