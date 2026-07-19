//! GCP Compute Engine client for VPC, networking, load balancing, instances, and disk operations.
//!
//! This module provides APIs for managing:
//! - VPC networks, subnetworks, routers, and firewalls
//! - Load balancing: health checks, backend services, URL maps, proxies, forwarding rules, NEGs
//! - Instance management: instance templates, instance group managers, instances
//! - Persistent disks
//!
//! See:
//! - Networks: https://cloud.google.com/compute/docs/reference/rest/v1/networks
//! - Subnetworks: https://cloud.google.com/compute/docs/reference/rest/v1/subnetworks
//! - Routers: https://cloud.google.com/compute/docs/reference/rest/v1/routers
//! - Firewalls: https://cloud.google.com/compute/docs/reference/rest/v1/firewalls
//! - Health Checks: https://cloud.google.com/compute/docs/reference/rest/v1/healthChecks
//! - Backend Services: https://cloud.google.com/compute/docs/reference/rest/v1/backendServices
//! - URL Maps: https://cloud.google.com/compute/docs/reference/rest/v1/urlMaps
//! - Target HTTP Proxies: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpProxies
//! - Global Addresses: https://cloud.google.com/compute/docs/reference/rest/v1/globalAddresses
//! - Global Forwarding Rules: https://cloud.google.com/compute/docs/reference/rest/v1/globalForwardingRules
//! - Network Endpoint Groups: https://cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups
//! - Instance Templates: https://cloud.google.com/compute/docs/reference/rest/v1/instanceTemplates
//! - Instance Group Managers: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers
//! - Instances: https://cloud.google.com/compute/docs/reference/rest/v1/instances
//! - Disks: https://cloud.google.com/compute/docs/reference/rest/v1/disks

use crate::gcp::api_client::GcpServiceConfig;

mod api;
mod client;
mod types;

#[cfg(test)]
mod tests;

pub use api::*;
pub use client::*;
pub use types::*;

// =============================================================================================
// Service Configuration
// =============================================================================================

/// Compute Engine service configuration
#[derive(Debug)]
pub struct ComputeServiceConfig;

impl GcpServiceConfig for ComputeServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://compute.googleapis.com/compute/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://compute.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Compute Engine"
    }

    fn service_key(&self) -> &'static str {
        "compute"
    }
}
