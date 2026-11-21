//! Container resource controllers.

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::*;

#[cfg(feature = "kubernetes")]
pub(crate) mod kubernetes;
#[cfg(feature = "kubernetes")]
pub use kubernetes::*;

// Also the gateway declaration on GCP: both clouds run the gateway as setup-created VMs.
#[cfg(any(feature = "azure", feature = "gcp"))]
mod azure_gateway;
#[cfg(any(feature = "azure", feature = "gcp"))]
pub use azure_gateway::*;

#[cfg(feature = "local")]
mod local_utils;

#[cfg(feature = "kubernetes")]
mod kubernetes_network;

#[cfg(feature = "kubernetes")]
mod kubernetes_service;

#[cfg(feature = "kubernetes")]
pub(crate) use kubernetes_service::peer_service_name;
