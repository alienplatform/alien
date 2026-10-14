//! Static configuration shared by setup-owned private worker gateways.
//!
//! The load balancer selects an allowed Host and prepends the API ID and endpoint
//! DNS name to the request path. The gateway restores the application path and
//! preserves Host while independently validating the upstream TLS identity.
//! Its security group must accept traffic only from that load balancer, and its
//! endpoint policy must allow only the associated API.

/// Envoy v1.39.4, pinned to its published multi-platform manifest.
pub const IMAGE: &str =
    "envoyproxy/envoy@sha256:74bd4eb5629708d3fadca1a5e6b85d208f338d7bb54e06b69565905c5294cc01";

/// Fixed bootstrap: no workload identity, IAM credentials or secrets are embedded.
pub const BOOTSTRAP: &str = include_str!("aws_worker_gateway/envoy.json");

/// Application traffic accepted only from the associated load balancer.
pub const HTTP_PORT: u16 = 8080;

/// Separate health listener so every application path remains available.
pub const HEALTH_PORT: u16 = 8081;

/// Gateway-authored application target, before REST API path/query decoding.
/// The gateway overwrites caller values; the private REST adapter consumes it.
pub const ORIGINAL_TARGET_HEADER: &str = "x-alien-original-target";
