use super::network::NetworkTier;
use bon::Builder;
use serde::{Deserialize, Serialize};

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

// =============================================================================================
// Data Structures - URL Map
// =============================================================================================

/// Represents a URL map resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/urlMaps
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UrlMap {
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

    /// Default backend service URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_service: Option<String>,

    /// Host rules for this URL map.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_rules: Vec<HostRule>,

    /// Path matchers for this URL map.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_matchers: Vec<PathMatcher>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#urlMap").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Host rule for URL map.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HostRule {
    /// Description of this rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// List of hosts to match.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,

    /// Name of the path matcher to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_matcher: Option<String>,
}

/// Path matcher for URL map.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PathMatcher {
    /// Name of this path matcher.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Description of this path matcher.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Default backend service URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_service: Option<String>,

    /// Path rules for this matcher.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_rules: Vec<PathRule>,
}

/// Path rule for URL map.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PathRule {
    /// Paths to match.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,

    /// Backend service URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

// =============================================================================================
// Data Structures - Target HTTP Proxy
// =============================================================================================

/// Represents a target HTTP proxy resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpProxies
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TargetHttpProxy {
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

    /// URL of the URL map associated with this proxy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_map: Option<String>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Whether to proxy WebSocket requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_bind: Option<bool>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#targetHttpProxy").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

// =============================================================================================
// Data Structures - Target TCP/HTTPS Proxy
// =============================================================================================

/// Global target TCP proxy.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetTcpProxies
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TargetTcpProxy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_header: Option<String>,
}

/// Represents a target HTTPS proxy resource (with SSL certificate support).
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/targetHttpsProxies
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TargetHttpsProxy {
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

    /// URL of the URL map associated with this proxy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_map: Option<String>,

    /// URLs of SSL certificates associated with this proxy.
    /// At least one SSL certificate must be specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_certificates: Option<Vec<String>>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Whether to proxy WebSocket requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_bind: Option<bool>,

    /// Minimum TLS version (e.g., "TLS_1_2").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_policy: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#targetHttpsProxy").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// QUIC protocol override (e.g., "NONE", "ENABLE", "DISABLE").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quic_override: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SetSslCertificatesRequest {
    pub ssl_certificates: Vec<String>,
}

// =============================================================================================
// Data Structures - SSL Certificate
// =============================================================================================

/// Self-managed SSL certificate details.
/// Used when SslCertificate.type = "SELF_MANAGED".
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/sslCertificates#SslCertificateSelfManagedSslCertificate
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SslCertificateSelfManaged {
    /// PEM-encoded X.509 certificate chain.
    /// The chain must be no greater than 5 certificates long.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate: Option<String>,

    /// PEM-encoded private key. Write-only; never returned in GET responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

/// Represents an SSL certificate resource for load balancers.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/sslCertificates
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SslCertificate {
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

    /// Type of certificate ("SELF_MANAGED" or "MANAGED").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// Self-managed certificate details.
    /// Must be populated when type = "SELF_MANAGED".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_managed: Option<SslCertificateSelfManaged>,

    /// Domains covered by this certificate (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_alternative_names: Option<Vec<String>>,

    /// Expiration timestamp (RFC3339, output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,

    /// Creation timestamp (RFC3339, output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#sslCertificate").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

// =============================================================================================
// Data Structures - Global Address
// =============================================================================================

/// Represents a global address resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalAddresses
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Address {
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

    /// The static IP address represented by this resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,

    /// The type of address (EXTERNAL or INTERNAL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_type: Option<AddressType>,

    /// IP version (IPV4 or IPV6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_version: Option<IpVersion>,

    /// Purpose of the address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<AddressPurpose>,

    /// Network tier for this address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_tier: Option<NetworkTier>,

    /// Status of the address (RESERVED, IN_USE, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<AddressStatus>,

    /// URL of the resource using this address.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<String>,

    /// Prefix length for IPv6 addresses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_length: Option<i32>,

    /// Network URL for internal addresses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Subnetwork URL for internal addresses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#address").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Address type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AddressType {
    /// External address.
    External,
    /// Internal address.
    Internal,
}

/// IP version.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IpVersion {
    /// IPv4.
    Ipv4,
    /// IPv6.
    Ipv6,
}

/// Address purpose.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AddressPurpose {
    /// GCE endpoint.
    GceEndpoint,
    /// VPC peering.
    VpcPeering,
    /// Private service connect.
    PrivateServiceConnect,
    /// NAT auto.
    NatAuto,
    /// Shared loadbalancer VIP.
    SharedLoadbalancerVip,
    /// DNS resolver.
    DnsResolver,
}

/// Address status.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AddressStatus {
    /// Address is reserved.
    Reserved,
    /// Address is reserved but being used.
    Reserving,
    /// Address is in use.
    InUse,
}

// =============================================================================================
// Data Structures - Global Forwarding Rule
// =============================================================================================

/// Represents a forwarding rule resource (global or regional).
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalForwardingRules
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/forwardingRules
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ForwardingRule {
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

    /// IP address for this forwarding rule.
    #[serde(rename = "IPAddress", skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,

    /// IP protocol for this forwarding rule.
    #[serde(rename = "IPProtocol", skip_serializing_if = "Option::is_none")]
    pub ip_protocol: Option<ForwardingRuleProtocol>,

    /// Port range for this forwarding rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_range: Option<String>,

    /// List of ports for this forwarding rule.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,

    /// URL of the target resource. For a Private Service Connect consumer
    /// endpoint this is the producer's service-attachment URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

    /// URL of the network this forwarding rule belongs to. Required for a
    /// Private Service Connect consumer endpoint, which lives in the consumer VPC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// URL of the subnetwork this forwarding rule draws its internal IP from.
    /// Used by internal forwarding rules such as Private Service Connect endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// Load balancing scheme. Left unset for a Private Service Connect consumer
    /// endpoint, which is not a load balancer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancing_scheme: Option<LoadBalancingScheme>,

    /// Network tier for this forwarding rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_tier: Option<NetworkTier>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#forwardingRule").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Forwarding rule IP protocol.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForwardingRuleProtocol {
    /// TCP protocol.
    Tcp,
    /// UDP protocol.
    Udp,
    /// ESP protocol.
    Esp,
    /// AH protocol.
    Ah,
    /// SCTP protocol.
    Sctp,
    /// ICMP protocol.
    Icmp,
    /// L3 default protocol.
    L3Default,
}

// =============================================================================================
// Data Structures - Network Endpoint Group (NEG)
// =============================================================================================

/// Represents a network endpoint group resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroup {
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

    /// Type of network endpoint group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_endpoint_type: Option<NetworkEndpointType>,

    /// Size of the network endpoint group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,

    /// URL of the network to which this NEG belongs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// URL of the subnetwork to which this NEG belongs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// URL of the zone where the NEG is located.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// Default port for endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_port: Option<i32>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#networkEndpointGroup").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Cloud Run service configuration for serverless NEG.
    /// Only valid when network_endpoint_type is SERVERLESS.
    /// Only one of cloud_run, app_engine, or cloud_function may be set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_run: Option<NetworkEndpointGroupCloudRun>,

    /// App Engine service configuration for serverless NEG.
    /// Only valid when network_endpoint_type is SERVERLESS.
    /// Only one of cloud_run, app_engine, or cloud_function may be set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_engine: Option<NetworkEndpointGroupAppEngine>,

    /// Cloud Function configuration for serverless NEG.
    /// Only valid when network_endpoint_type is SERVERLESS.
    /// Only one of cloud_run, app_engine, or cloud_function may be set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_function: Option<NetworkEndpointGroupCloudFunction>,
}

/// Cloud Run service configuration for a serverless NEG.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupCloudRun {
    /// Cloud Run service name.
    /// Example: "my-service"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    /// Cloud Run service tag (optional).
    /// Example: "v1", "production"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,

    /// URL mask for routing to multiple Cloud Run services.
    /// Example: "<tag>.domain.com/<service>" allows routing based on URL patterns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_mask: Option<String>,
}

/// App Engine service configuration for a serverless NEG.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupAppEngine {
    /// App Engine service name (optional).
    /// The service name is case-sensitive and must be 1-63 characters long.
    /// Example: "default", "my-service"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    /// App Engine version (optional).
    /// The version name is case-sensitive and must be 1-100 characters long.
    /// Example: "v1", "v2"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// URL mask for routing to multiple App Engine services.
    /// Example: "<service>-dot-appname.appspot.com/<version>"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_mask: Option<String>,
}

/// Cloud Function configuration for a serverless NEG.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupCloudFunction {
    /// Cloud Function name.
    /// The function name is case-sensitive and must be 1-63 characters long.
    /// Example: "func1"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,

    /// URL mask for routing to multiple Cloud Functions.
    /// Example: "/<function>" allows routing based on URL patterns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_mask: Option<String>,
}

/// Network endpoint type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NetworkEndpointType {
    /// GCE VM IP port endpoint.
    GceVmIpPort,
    /// Non-GCP private IP port endpoint.
    NonGcpPrivateIpPort,
    /// Internet IP port endpoint.
    InternetIpPort,
    /// Internet FQDN port endpoint.
    InternetFqdnPort,
    /// Serverless endpoint.
    Serverless,
    /// Private service connect endpoint.
    PrivateServiceConnect,
}

/// Request to attach network endpoints.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupsAttachEndpointsRequest {
    /// Network endpoints to attach.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_endpoints: Vec<NetworkEndpoint>,
}

/// Request to detach network endpoints.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupsDetachEndpointsRequest {
    /// Network endpoints to detach.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_endpoints: Vec<NetworkEndpoint>,
}

/// Network endpoint.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpoint {
    /// IP address of the endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,

    /// Port number for the endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Instance that the endpoint belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,

    /// FQDN of the endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fqdn: Option<String>,
}
