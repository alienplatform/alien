use super::network::NetworkTier;
use bon::Builder;
use serde::{Deserialize, Serialize};

mod routing;
pub use routing::*;

// =============================================================================================
// Data Structures - Health Check
// =============================================================================================

/// Represents a health check resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/healthChecks
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// How often (in seconds) to send a health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_interval_sec: Option<i32>,

    /// How long (in seconds) to wait before claiming failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_sec: Option<i32>,

    /// Number of consecutive failures before marking unhealthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unhealthy_threshold: Option<i32>,

    /// Number of consecutive successes before marking healthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy_threshold: Option<i32>,

    /// Type of health check (TCP, HTTP, HTTPS, HTTP2, GRPC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<HealthCheckType>,

    /// TCP health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_health_check: Option<TcpHealthCheck>,

    /// HTTP health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_health_check: Option<HttpHealthCheck>,

    /// HTTPS health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub https_health_check: Option<HttpsHealthCheck>,

    /// HTTP2 health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http2_health_check: Option<Http2HealthCheck>,

    /// GRPC health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grpc_health_check: Option<GrpcHealthCheck>,

    /// Log configuration for this health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<HealthCheckLogConfig>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#healthCheck").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Health check type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HealthCheckType {
    /// TCP health check.
    Tcp,
    /// HTTP health check.
    Http,
    /// HTTPS health check.
    Https,
    /// HTTP/2 health check.
    Http2,
    /// gRPC health check.
    Grpc,
}

/// TCP health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TcpHealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// Proxy header type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_header: Option<ProxyHeader>,

    /// Request data to send.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,

    /// Expected response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// HTTP health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HttpHealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// Host header for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    /// Request path for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,

    /// Proxy header type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_header: Option<ProxyHeader>,

    /// Expected response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// HTTPS health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HttpsHealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// Host header for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    /// Request path for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,

    /// Proxy header type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_header: Option<ProxyHeader>,

    /// Expected response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// HTTP/2 health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Http2HealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// Host header for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    /// Request path for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,

    /// Proxy header type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_header: Option<ProxyHeader>,

    /// Expected response data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// gRPC health check configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GrpcHealthCheck {
    /// Port number for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Port name for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Port specification type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_specification: Option<PortSpecification>,

    /// gRPC service name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grpc_service_name: Option<String>,
}

/// Port specification type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PortSpecification {
    /// Use a fixed port number.
    UseFixedPort,
    /// Use a named port.
    UseNamedPort,
    /// Use the serving port.
    UseServingPort,
}

/// Proxy header type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProxyHeader {
    /// No proxy header.
    None,
    /// PROXY_V1 header.
    ProxyV1,
}

/// Health check log configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckLogConfig {
    /// Whether to enable logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,
}

// =============================================================================================
// Data Structures - Backend Service
// =============================================================================================

/// Represents a backend service resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/backendServices
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BackendService {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// List of backends.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backends: Vec<Backend>,

    /// Health check URLs for this backend service.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub health_checks: Vec<String>,

    /// Timeout in seconds for backend responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_sec: Option<i32>,

    /// Port number used for communication with backends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Protocol used to communicate with backends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<BackendServiceProtocol>,

    /// Port name for backends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,

    /// Load balancing scheme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancing_scheme: Option<LoadBalancingScheme>,

    /// Session affinity configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_affinity: Option<SessionAffinity>,

    /// Affinity cookie TTL in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affinity_cookie_ttl_sec: Option<i32>,

    /// Connection draining configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_draining: Option<ConnectionDraining>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Whether to enable CDN for this backend service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_c_d_n: Option<bool>,

    /// CDN policy configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdn_policy: Option<BackendServiceCdnPolicy>,

    /// Log configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<BackendServiceLogConfig>,

    /// Security policy URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_policy: Option<String>,

    /// Locality load balancing policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locality_lb_policy: Option<LocalityLbPolicy>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#backendService").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Backend configuration for a backend service.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Backend {
    /// URL of the backend group (instance group or NEG).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,

    /// Balancing mode (UTILIZATION, RATE, CONNECTION).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balancing_mode: Option<BalancingMode>,

    /// Capacity scaler (0.0 to 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_scaler: Option<f64>,

    /// Maximum connections for this backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections: Option<i32>,

    /// Maximum connections per instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections_per_instance: Option<i32>,

    /// Maximum connections per endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections_per_endpoint: Option<i32>,

    /// Maximum rate for this backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rate: Option<i32>,

    /// Maximum rate per instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rate_per_instance: Option<f64>,

    /// Maximum rate per endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rate_per_endpoint: Option<f64>,

    /// Maximum CPU utilization for this backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_utilization: Option<f64>,

    /// Description of this backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Balancing mode for a backend.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BalancingMode {
    /// Balance by CPU utilization.
    Utilization,
    /// Balance by request rate.
    Rate,
    /// Balance by connection count.
    Connection,
}

/// Backend service protocol.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BackendServiceProtocol {
    /// HTTP protocol.
    Http,
    /// HTTPS protocol.
    Https,
    /// HTTP/2 protocol.
    Http2,
    /// TCP protocol.
    Tcp,
    /// SSL protocol.
    Ssl,
    /// gRPC protocol.
    Grpc,
    /// Unspecified protocol.
    Unspecified,
}

/// Load balancing scheme.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoadBalancingScheme {
    /// External load balancing.
    External,
    /// Internal load balancing.
    Internal,
    /// Internal self-managed load balancing.
    InternalSelfManaged,
    /// Internal managed load balancing.
    InternalManaged,
    /// External managed load balancing.
    ExternalManaged,
}

/// Session affinity type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionAffinity {
    /// No session affinity.
    None,
    /// Client IP affinity.
    ClientIp,
    /// Generated cookie affinity.
    GeneratedCookie,
    /// Client IP with proto affinity.
    ClientIpProto,
    /// Client IP and port affinity.
    ClientIpPortProto,
    /// HTTP cookie affinity.
    HttpCookie,
    /// Header field affinity.
    HeaderField,
}

/// Connection draining configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDraining {
    /// Time in seconds to wait for connections to drain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draining_timeout_sec: Option<i32>,
}

/// Backend service CDN policy.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BackendServiceCdnPolicy {
    /// Cache mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_mode: Option<CacheMode>,

    /// Signed URL cache max age in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_url_cache_max_age_sec: Option<i64>,

    /// Default TTL in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_ttl: Option<i32>,

    /// Maximum TTL in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ttl: Option<i32>,

    /// Client TTL in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ttl: Option<i32>,

    /// Whether to serve stale content while revalidating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serve_while_stale: Option<i32>,

    /// Negative caching policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative_caching: Option<bool>,
}

/// Cache mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CacheMode {
    /// Use origin headers.
    UseOriginHeaders,
    /// Force cache all.
    ForceCacheAll,
    /// Cache all static content.
    CacheAllStatic,
}

/// Backend service log configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BackendServiceLogConfig {
    /// Whether to enable logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Sample rate (0.0 to 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<f64>,
}

/// Locality load balancing policy.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LocalityLbPolicy {
    /// Round robin.
    RoundRobin,
    /// Least request.
    LeastRequest,
    /// Ring hash.
    RingHash,
    /// Random.
    Random,
    /// Original destination.
    OriginalDestination,
    /// Maglev.
    Maglev,
}
