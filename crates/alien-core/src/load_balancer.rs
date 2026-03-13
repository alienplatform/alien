//! Load balancer endpoint types for DNS management.

use serde::{Deserialize, Serialize};

/// Load balancer endpoint information for DNS management.
/// This is optional metadata used by the DNS controller to create domain mappings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerEndpoint {
    /// The DNS name of the load balancer endpoint (e.g., ALB DNS, API Gateway domain).
    pub dns_name: String,
    /// AWS Route53 hosted zone ID (for ALIAS records). Only set on AWS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosted_zone_id: Option<String>,
}
