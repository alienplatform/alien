use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
use alien_core::GcpClientConfig;
use alien_error::AlienError;
use bon::Builder;
use google_cloud_compute_v1::{
    client::{
        BackendServices as OfficialBackendServices, Firewalls as OfficialFirewalls,
        GlobalAddresses as OfficialGlobalAddresses,
        GlobalForwardingRules as OfficialGlobalForwardingRules,
        GlobalOperations as OfficialGlobalOperations, Networks as OfficialNetworks,
        RegionNetworkEndpointGroups as OfficialRegionNetworkEndpointGroups,
        RegionOperations as OfficialRegionOperations, Routers as OfficialRouters,
        SslCertificates as OfficialSslCertificates, Subnetworks as OfficialSubnetworks,
        TargetHttpsProxies as OfficialTargetHttpsProxies, UrlMaps as OfficialUrlMaps,
    },
    model::{
        Address as OfficialAddress, BackendService as OfficialBackendService,
        Firewall as OfficialFirewall, ForwardingRule as OfficialForwardingRule,
        Network as OfficialNetwork, NetworkEndpointGroup as OfficialNetworkEndpointGroup,
        Router as OfficialRouter, SslCertificate as OfficialSslCertificate,
        Subnetwork as OfficialSubnetwork,
        TargetHttpsProxiesSetSslCertificatesRequest as OfficialSetSslCertificatesRequest,
        TargetHttpsProxy as OfficialTargetHttpsProxy, UrlMap as OfficialUrlMap,
    },
};
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use http::StatusCode;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::OnceCell;

#[cfg(any(test, feature = "test-utils"))]
use mockall::automock;

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait GcpComputeApi: Send + Sync + std::fmt::Debug {
    async fn get_network(&self, network_name: String) -> CloudClientResult<Network>;
    async fn insert_network(&self, network: Network) -> CloudClientResult<Operation>;
    async fn delete_network(&self, network_name: String) -> CloudClientResult<Operation>;

    async fn get_subnetwork(
        &self,
        region: String,
        subnetwork_name: String,
    ) -> CloudClientResult<Subnetwork>;
    async fn insert_subnetwork(
        &self,
        region: String,
        subnetwork: Subnetwork,
    ) -> CloudClientResult<Operation>;
    async fn delete_subnetwork(
        &self,
        region: String,
        subnetwork_name: String,
    ) -> CloudClientResult<Operation>;

    async fn get_router(&self, region: String, router_name: String) -> CloudClientResult<Router>;
    async fn insert_router(&self, region: String, router: Router) -> CloudClientResult<Operation>;
    async fn patch_router(
        &self,
        region: String,
        router_name: String,
        router: Router,
    ) -> CloudClientResult<Operation>;
    async fn delete_router(
        &self,
        region: String,
        router_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_firewall(&self, firewall: Firewall) -> CloudClientResult<Operation>;
    async fn delete_firewall(&self, firewall_name: String) -> CloudClientResult<Operation>;

    async fn get_global_operation(&self, operation_name: String) -> CloudClientResult<Operation>;
    async fn get_region_operation(
        &self,
        region: String,
        operation_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_backend_service(
        &self,
        backend_service: BackendService,
    ) -> CloudClientResult<Operation>;
    async fn delete_backend_service(
        &self,
        backend_service_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_url_map(&self, url_map: UrlMap) -> CloudClientResult<Operation>;
    async fn delete_url_map(&self, url_map_name: String) -> CloudClientResult<Operation>;

    async fn insert_target_https_proxy(
        &self,
        target_https_proxy: TargetHttpsProxy,
    ) -> CloudClientResult<Operation>;
    async fn set_target_https_proxy_ssl_certificates(
        &self,
        target_https_proxy_name: String,
        ssl_certificates: Vec<String>,
    ) -> CloudClientResult<Operation>;
    async fn delete_target_https_proxy(
        &self,
        target_https_proxy_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_ssl_certificate(
        &self,
        ssl_certificate: SslCertificate,
    ) -> CloudClientResult<Operation>;
    async fn delete_ssl_certificate(
        &self,
        ssl_certificate_name: String,
    ) -> CloudClientResult<Operation>;

    async fn get_global_address(&self, address_name: String) -> CloudClientResult<Address>;
    async fn insert_global_address(&self, address: Address) -> CloudClientResult<Operation>;
    async fn delete_global_address(&self, address_name: String) -> CloudClientResult<Operation>;

    async fn insert_global_forwarding_rule(
        &self,
        forwarding_rule: ForwardingRule,
    ) -> CloudClientResult<Operation>;
    async fn delete_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> CloudClientResult<Operation>;

    async fn insert_region_network_endpoint_group(
        &self,
        region: String,
        network_endpoint_group: NetworkEndpointGroup,
    ) -> CloudClientResult<Operation>;
    async fn delete_region_network_endpoint_group(
        &self,
        region: String,
        network_endpoint_group_name: String,
    ) -> CloudClientResult<Operation>;
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Unique identifier defined by Compute Engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Name of the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Type of operation, such as insert or delete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_type: Option<String>,
    /// URL of the resource modified by this operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_link: Option<String>,
    /// Server-defined URL for this operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,
    /// User that requested the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Current operation status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<OperationStatus>,
    /// Progress from 0 to 100 when provided by Compute Engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<i32>,
    /// Operation start timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    /// Operation end timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    /// Operation insert timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_time: Option<String>,
    /// Zone URL for zonal operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// Region URL for regional operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Human-readable operation description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// HTTP status code for failed operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_error_status_code: Option<i32>,
    /// HTTP error message for failed operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_error_message: Option<String>,
    /// Structured operation error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<OperationError>,
    /// Resource kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl Operation {
    pub fn is_done(&self) -> bool {
        matches!(self.status, Some(OperationStatus::Done))
    }

    pub fn has_error(&self) -> bool {
        self.error
            .as_ref()
            .is_some_and(|error| !error.errors.is_empty())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationStatus {
    Pending,
    Running,
    Done,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    /// Individual errors returned by Compute Engine.
    #[builder(default)]
    #[serde(default)]
    pub errors: Vec<OperationErrorItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct OperationErrorItem {
    /// Compute Engine error code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Request location related to the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Human-readable error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    /// Unique identifier defined by Compute Engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Network name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Network description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Server-defined network URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,
    /// Whether Compute Engine should auto-create subnetworks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_create_subnetworks: Option<bool>,
    /// Network routing configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_config: Option<NetworkRoutingConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRoutingConfig {
    /// Routing mode for this VPC network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_mode: Option<RoutingMode>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoutingMode {
    Regional,
    Global,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Subnetwork {
    /// Unique identifier defined by Compute Engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Subnetwork name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Subnetwork description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Server-defined subnetwork URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,
    /// Parent network URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// Primary IPv4 CIDR range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_cidr_range: Option<String>,
    /// Whether private Google access is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip_google_access: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Router {
    /// Unique identifier defined by Compute Engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Router name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Router description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Server-defined router URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,
    /// Parent region URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Parent network URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// NAT configurations on this router.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nats: Vec<RouterNat>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNat {
    /// NAT configuration name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source subnetwork range selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_subnetwork_ip_ranges_to_nat: Option<SourceSubnetworkIpRangesToNat>,
    /// Subnetworks covered by this NAT configuration.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnetworks: Vec<RouterNatSubnetworkToNat>,
    /// NAT IP allocation option.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_ip_allocate_option: Option<NatIpAllocateOption>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceSubnetworkIpRangesToNat {
    AllSubnetworksAllIpRanges,
    AllSubnetworksAllPrimaryIpRanges,
    ListOfSubnetworks,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NatIpAllocateOption {
    AutoOnly,
    ManualOnly,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNatSubnetworkToNat {
    /// Subnetwork URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source IP ranges to NAT.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ip_ranges_to_nat: Vec<SourceIpRangesToNat>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceIpRangesToNat {
    AllIpRanges,
    PrimaryIpRange,
    ListOfSecondaryIpRanges,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Firewall {
    /// Unique identifier defined by Compute Engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Firewall rule name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Firewall rule description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Network URL this rule applies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// Rule priority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    /// Traffic direction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<FirewallDirection>,
    /// Allowed traffic.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<FirewallAllowed>,
    /// Source CIDR ranges.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ranges: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FirewallDirection {
    Ingress,
    Egress,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FirewallAllowed {
    /// IP protocol.
    #[serde(rename = "IPProtocol", skip_serializing_if = "Option::is_none")]
    pub ip_protocol: Option<String>,
    /// Ports to allow.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BackendService {
    /// Backend service name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Backend service description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Server-defined backend service URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,
    /// Backends attached to this service.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backends: Vec<Backend>,
    /// Protocol used by the backend service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<BackendServiceProtocol>,
    /// Load balancing scheme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancing_scheme: Option<LoadBalancingScheme>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Backend {
    /// Backend group URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Balancing mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balancing_mode: Option<BalancingMode>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BalancingMode {
    Utilization,
    Rate,
    Connection,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BackendServiceProtocol {
    Http,
    Https,
    Http2,
    Tcp,
    Ssl,
    Grpc,
    Unspecified,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoadBalancingScheme {
    External,
    Internal,
    InternalSelfManaged,
    InternalManaged,
    ExternalManaged,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UrlMap {
    /// URL map name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// URL map description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Default backend service URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_service: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TargetHttpsProxy {
    /// Target HTTPS proxy name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Target HTTPS proxy description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// URL map URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_map: Option<String>,
    /// SSL certificate URLs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_certificates: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SslCertificate {
    /// SSL certificate name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// SSL certificate description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Certificate type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Self-managed certificate data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_managed: Option<SslCertificateSelfManaged>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SslCertificateSelfManaged {
    /// PEM certificate chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate: Option<String>,
    /// PEM private key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    /// Address name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Address description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Reserved IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// Address type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_type: Option<AddressType>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AddressType {
    External,
    Internal,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ForwardingRule {
    /// Forwarding rule name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Forwarding rule description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// IP address for this forwarding rule.
    #[serde(rename = "IPAddress", skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    /// IP protocol.
    #[serde(rename = "IPProtocol", skip_serializing_if = "Option::is_none")]
    pub ip_protocol: Option<ForwardingRuleProtocol>,
    /// Port range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_range: Option<String>,
    /// Target URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Load balancing scheme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancing_scheme: Option<LoadBalancingScheme>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForwardingRuleProtocol {
    Tcp,
    Udp,
    Esp,
    Ah,
    Sctp,
    Icmp,
    L3Default,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroup {
    /// Network endpoint group name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Network endpoint group description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Endpoint group type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_endpoint_type: Option<NetworkEndpointType>,
    /// Cloud Run endpoint configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_run: Option<NetworkEndpointGroupCloudRun>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEndpointGroupCloudRun {
    /// Cloud Run service name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// Cloud Run service tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// URL mask for multi-service routing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_mask: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NetworkEndpointType {
    GceVmIpPort,
    NonGcpPrivateIpPort,
    InternetIpPort,
    InternetFqdnPort,
    Serverless,
    PrivateServiceConnect,
}

pub struct OfficialGcpComputeClient {
    config: GcpClientConfig,
    networks: OnceCell<OfficialNetworks>,
    subnetworks: OnceCell<OfficialSubnetworks>,
    routers: OnceCell<OfficialRouters>,
    firewalls: OnceCell<OfficialFirewalls>,
    global_operations: OnceCell<OfficialGlobalOperations>,
    region_operations: OnceCell<OfficialRegionOperations>,
    backend_services: OnceCell<OfficialBackendServices>,
    url_maps: OnceCell<OfficialUrlMaps>,
    target_https_proxies: OnceCell<OfficialTargetHttpsProxies>,
    ssl_certificates: OnceCell<OfficialSslCertificates>,
    global_addresses: OnceCell<OfficialGlobalAddresses>,
    global_forwarding_rules: OnceCell<OfficialGlobalForwardingRules>,
    region_network_endpoint_groups: OnceCell<OfficialRegionNetworkEndpointGroups>,
}

impl std::fmt::Debug for OfficialGcpComputeClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGcpComputeClient")
            .field("project_id", &self.config.project_id)
            .field("region", &self.config.region)
            .finish_non_exhaustive()
    }
}

impl OfficialGcpComputeClient {
    pub fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            networks: OnceCell::new(),
            subnetworks: OnceCell::new(),
            routers: OnceCell::new(),
            firewalls: OnceCell::new(),
            global_operations: OnceCell::new(),
            region_operations: OnceCell::new(),
            backend_services: OnceCell::new(),
            url_maps: OnceCell::new(),
            target_https_proxies: OnceCell::new(),
            ssl_certificates: OnceCell::new(),
            global_addresses: OnceCell::new(),
            global_forwarding_rules: OnceCell::new(),
            region_network_endpoint_groups: OnceCell::new(),
        }
    }

    async fn networks(&self) -> CloudClientResult<&OfficialNetworks> {
        self.networks
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialNetworks::builder).await
            })
            .await
    }

    async fn subnetworks(&self) -> CloudClientResult<&OfficialSubnetworks> {
        self.subnetworks
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialSubnetworks::builder).await
            })
            .await
    }

    async fn routers(&self) -> CloudClientResult<&OfficialRouters> {
        self.routers
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialRouters::builder).await
            })
            .await
    }

    async fn firewalls(&self) -> CloudClientResult<&OfficialFirewalls> {
        self.firewalls
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialFirewalls::builder).await
            })
            .await
    }

    async fn global_operations(&self) -> CloudClientResult<&OfficialGlobalOperations> {
        self.global_operations
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialGlobalOperations::builder).await
            })
            .await
    }

    async fn region_operations(&self) -> CloudClientResult<&OfficialRegionOperations> {
        self.region_operations
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialRegionOperations::builder).await
            })
            .await
    }

    async fn backend_services(&self) -> CloudClientResult<&OfficialBackendServices> {
        self.backend_services
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialBackendServices::builder).await
            })
            .await
    }

    async fn url_maps(&self) -> CloudClientResult<&OfficialUrlMaps> {
        self.url_maps
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialUrlMaps::builder).await
            })
            .await
    }

    async fn target_https_proxies(&self) -> CloudClientResult<&OfficialTargetHttpsProxies> {
        self.target_https_proxies
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialTargetHttpsProxies::builder).await
            })
            .await
    }

    async fn ssl_certificates(&self) -> CloudClientResult<&OfficialSslCertificates> {
        self.ssl_certificates
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialSslCertificates::builder).await
            })
            .await
    }

    async fn global_addresses(&self) -> CloudClientResult<&OfficialGlobalAddresses> {
        self.global_addresses
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialGlobalAddresses::builder).await
            })
            .await
    }

    async fn global_forwarding_rules(&self) -> CloudClientResult<&OfficialGlobalForwardingRules> {
        self.global_forwarding_rules
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialGlobalForwardingRules::builder).await
            })
            .await
    }

    async fn region_network_endpoint_groups(
        &self,
    ) -> CloudClientResult<&OfficialRegionNetworkEndpointGroups> {
        self.region_network_endpoint_groups
            .get_or_try_init(|| async {
                build_compute_client(&self.config, OfficialRegionNetworkEndpointGroups::builder)
                    .await
            })
            .await
    }
}

#[async_trait::async_trait]
impl GcpComputeApi for OfficialGcpComputeClient {
    async fn get_network(&self, network_name: String) -> CloudClientResult<Network> {
        self.networks()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_network(network_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "network", &network_name))
            .and_then(from_official)
    }

    async fn insert_network(&self, network: Network) -> CloudClientResult<Operation> {
        let name = resource_name(&network.name);
        self.networks()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(to_official::<_, OfficialNetwork>(network)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "network", &name))
            .and_then(from_official)
    }

    async fn delete_network(&self, network_name: String) -> CloudClientResult<Operation> {
        self.networks()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_network(network_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "network", &network_name))
            .and_then(from_official)
    }

    async fn get_subnetwork(
        &self,
        region: String,
        subnetwork_name: String,
    ) -> CloudClientResult<Subnetwork> {
        self.subnetworks()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_subnetwork(subnetwork_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "subnetwork", &subnetwork_name))
            .and_then(from_official)
    }

    async fn insert_subnetwork(
        &self,
        region: String,
        subnetwork: Subnetwork,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&subnetwork.name);
        self.subnetworks()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_body(to_official::<_, OfficialSubnetwork>(subnetwork)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "subnetwork", &name))
            .and_then(from_official)
    }

    async fn delete_subnetwork(
        &self,
        region: String,
        subnetwork_name: String,
    ) -> CloudClientResult<Operation> {
        self.subnetworks()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_subnetwork(subnetwork_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "subnetwork", &subnetwork_name))
            .and_then(from_official)
    }

    async fn get_router(&self, region: String, router_name: String) -> CloudClientResult<Router> {
        self.routers()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_router(router_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "router", &router_name))
            .and_then(from_official)
    }

    async fn insert_router(&self, region: String, router: Router) -> CloudClientResult<Operation> {
        let name = resource_name(&router.name);
        self.routers()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_body(to_official::<_, OfficialRouter>(router)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "router", &name))
            .and_then(from_official)
    }

    async fn patch_router(
        &self,
        region: String,
        router_name: String,
        router: Router,
    ) -> CloudClientResult<Operation> {
        self.routers()
            .await?
            .patch()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_router(router_name.clone())
            .set_body(to_official::<_, OfficialRouter>(router)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "router", &router_name))
            .and_then(from_official)
    }

    async fn delete_router(
        &self,
        region: String,
        router_name: String,
    ) -> CloudClientResult<Operation> {
        self.routers()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_router(router_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "router", &router_name))
            .and_then(from_official)
    }

    async fn insert_firewall(&self, firewall: Firewall) -> CloudClientResult<Operation> {
        let name = resource_name(&firewall.name);
        self.firewalls()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(to_official::<_, OfficialFirewall>(firewall)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "firewall", &name))
            .and_then(from_official)
    }

    async fn delete_firewall(&self, firewall_name: String) -> CloudClientResult<Operation> {
        self.firewalls()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_firewall(firewall_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "firewall", &firewall_name))
            .and_then(from_official)
    }

    async fn get_global_operation(&self, operation_name: String) -> CloudClientResult<Operation> {
        self.global_operations()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_operation(operation_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "globalOperation", &operation_name))
            .and_then(from_official)
    }

    async fn get_region_operation(
        &self,
        region: String,
        operation_name: String,
    ) -> CloudClientResult<Operation> {
        self.region_operations()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_operation(operation_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "regionOperation", &operation_name))
            .and_then(from_official)
    }

    async fn insert_backend_service(
        &self,
        backend_service: BackendService,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&backend_service.name);
        self.backend_services()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(to_official::<_, OfficialBackendService>(backend_service)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "backendService", &name))
            .and_then(from_official)
    }

    async fn delete_backend_service(
        &self,
        backend_service_name: String,
    ) -> CloudClientResult<Operation> {
        self.backend_services()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_backend_service(backend_service_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "backendService", &backend_service_name))
            .and_then(from_official)
    }

    async fn insert_url_map(&self, url_map: UrlMap) -> CloudClientResult<Operation> {
        let name = resource_name(&url_map.name);
        self.url_maps()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(to_official::<_, OfficialUrlMap>(url_map)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "urlMap", &name))
            .and_then(from_official)
    }

    async fn delete_url_map(&self, url_map_name: String) -> CloudClientResult<Operation> {
        self.url_maps()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_url_map(url_map_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "urlMap", &url_map_name))
            .and_then(from_official)
    }

    async fn insert_target_https_proxy(
        &self,
        target_https_proxy: TargetHttpsProxy,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&target_https_proxy.name);
        self.target_https_proxies()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(to_official::<_, OfficialTargetHttpsProxy>(
                target_https_proxy,
            )?)
            .send()
            .await
            .map_err(|error| compute_error(error, "targetHttpsProxy", &name))
            .and_then(from_official)
    }

    async fn set_target_https_proxy_ssl_certificates(
        &self,
        target_https_proxy_name: String,
        ssl_certificates: Vec<String>,
    ) -> CloudClientResult<Operation> {
        let request =
            OfficialSetSslCertificatesRequest::new().set_ssl_certificates(ssl_certificates);
        self.target_https_proxies()
            .await?
            .set_ssl_certificates()
            .set_project(self.config.project_id.clone())
            .set_target_https_proxy(target_https_proxy_name.clone())
            .set_body(request)
            .send()
            .await
            .map_err(|error| compute_error(error, "targetHttpsProxy", &target_https_proxy_name))
            .and_then(from_official)
    }

    async fn delete_target_https_proxy(
        &self,
        target_https_proxy_name: String,
    ) -> CloudClientResult<Operation> {
        self.target_https_proxies()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_target_https_proxy(target_https_proxy_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "targetHttpsProxy", &target_https_proxy_name))
            .and_then(from_official)
    }

    async fn insert_ssl_certificate(
        &self,
        ssl_certificate: SslCertificate,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&ssl_certificate.name);
        self.ssl_certificates()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(to_official::<_, OfficialSslCertificate>(ssl_certificate)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "sslCertificate", &name))
            .and_then(from_official)
    }

    async fn delete_ssl_certificate(
        &self,
        ssl_certificate_name: String,
    ) -> CloudClientResult<Operation> {
        self.ssl_certificates()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_ssl_certificate(ssl_certificate_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "sslCertificate", &ssl_certificate_name))
            .and_then(from_official)
    }

    async fn get_global_address(&self, address_name: String) -> CloudClientResult<Address> {
        self.global_addresses()
            .await?
            .get()
            .set_project(self.config.project_id.clone())
            .set_address(address_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "globalAddress", &address_name))
            .and_then(from_official)
    }

    async fn insert_global_address(&self, address: Address) -> CloudClientResult<Operation> {
        let name = resource_name(&address.name);
        self.global_addresses()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(to_official::<_, OfficialAddress>(address)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "globalAddress", &name))
            .and_then(from_official)
    }

    async fn delete_global_address(&self, address_name: String) -> CloudClientResult<Operation> {
        self.global_addresses()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_address(address_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "globalAddress", &address_name))
            .and_then(from_official)
    }

    async fn insert_global_forwarding_rule(
        &self,
        forwarding_rule: ForwardingRule,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&forwarding_rule.name);
        self.global_forwarding_rules()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_body(to_official::<_, OfficialForwardingRule>(forwarding_rule)?)
            .send()
            .await
            .map_err(|error| compute_error(error, "globalForwardingRule", &name))
            .and_then(from_official)
    }

    async fn delete_global_forwarding_rule(
        &self,
        forwarding_rule_name: String,
    ) -> CloudClientResult<Operation> {
        self.global_forwarding_rules()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_forwarding_rule(forwarding_rule_name.clone())
            .send()
            .await
            .map_err(|error| compute_error(error, "globalForwardingRule", &forwarding_rule_name))
            .and_then(from_official)
    }

    async fn insert_region_network_endpoint_group(
        &self,
        region: String,
        network_endpoint_group: NetworkEndpointGroup,
    ) -> CloudClientResult<Operation> {
        let name = resource_name(&network_endpoint_group.name);
        self.region_network_endpoint_groups()
            .await?
            .insert()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_body(to_official::<_, OfficialNetworkEndpointGroup>(
                network_endpoint_group,
            )?)
            .send()
            .await
            .map_err(|error| compute_error(error, "regionNetworkEndpointGroup", &name))
            .and_then(from_official)
    }

    async fn delete_region_network_endpoint_group(
        &self,
        region: String,
        network_endpoint_group_name: String,
    ) -> CloudClientResult<Operation> {
        self.region_network_endpoint_groups()
            .await?
            .delete()
            .set_project(self.config.project_id.clone())
            .set_region(region)
            .set_network_endpoint_group(network_endpoint_group_name.clone())
            .send()
            .await
            .map_err(|error| {
                compute_error(
                    error,
                    "regionNetworkEndpointGroup",
                    &network_endpoint_group_name,
                )
            })
            .and_then(from_official)
    }
}

async fn build_compute_client<T, B>(
    config: &GcpClientConfig,
    builder: impl FnOnce() -> B,
) -> CloudClientResult<T>
where
    B: ComputeClientBuilder<T>,
{
    let credentials = crate::core::gcp_credentials_from_alien_config(config).map_err(|error| {
        AlienError::new(CloudClientErrorData::AuthenticationError {
            message: error.to_string(),
        })
    })?;
    let mut builder = builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("compute"))
    {
        builder = builder.with_endpoint(compute_endpoint(endpoint));
    }

    builder.build().await.map_err(|error| {
        AlienError::new(CloudClientErrorData::GenericError {
            message: format!("Failed to build official GCP Compute client: {error}"),
        })
    })
}

#[async_trait::async_trait]
trait ComputeClientBuilder<T>: Sized {
    fn with_credentials(self, credentials: google_cloud_auth::credentials::Credentials) -> Self;
    fn with_endpoint(self, endpoint: String) -> Self;
    async fn build(self) -> Result<T, google_cloud_gax::client_builder::Error>;
}

macro_rules! impl_compute_client_builder {
    ($builder:ty, $client:ty) => {
        #[async_trait::async_trait]
        impl ComputeClientBuilder<$client> for $builder {
            fn with_credentials(
                self,
                credentials: google_cloud_auth::credentials::Credentials,
            ) -> Self {
                self.with_credentials(credentials)
            }

            fn with_endpoint(self, endpoint: String) -> Self {
                self.with_endpoint(endpoint)
            }

            async fn build(self) -> Result<$client, google_cloud_gax::client_builder::Error> {
                self.build().await
            }
        }
    };
}

impl_compute_client_builder!(
    google_cloud_compute_v1::builder::networks::ClientBuilder,
    OfficialNetworks
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::subnetworks::ClientBuilder,
    OfficialSubnetworks
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::routers::ClientBuilder,
    OfficialRouters
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::firewalls::ClientBuilder,
    OfficialFirewalls
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::global_operations::ClientBuilder,
    OfficialGlobalOperations
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::region_operations::ClientBuilder,
    OfficialRegionOperations
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::backend_services::ClientBuilder,
    OfficialBackendServices
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::url_maps::ClientBuilder,
    OfficialUrlMaps
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::target_https_proxies::ClientBuilder,
    OfficialTargetHttpsProxies
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::ssl_certificates::ClientBuilder,
    OfficialSslCertificates
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::global_addresses::ClientBuilder,
    OfficialGlobalAddresses
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::global_forwarding_rules::ClientBuilder,
    OfficialGlobalForwardingRules
);
impl_compute_client_builder!(
    google_cloud_compute_v1::builder::region_network_endpoint_groups::ClientBuilder,
    OfficialRegionNetworkEndpointGroups
);

fn to_official<T, U>(value: T) -> CloudClientResult<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_json::to_value(value).map_err(generic_compute_conversion_error)?;
    serde_json::from_value(value).map_err(generic_compute_conversion_error)
}

fn from_official<T, U>(value: T) -> CloudClientResult<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_json::to_value(value).map_err(generic_compute_conversion_error)?;
    serde_json::from_value(value).map_err(generic_compute_conversion_error)
}

fn generic_compute_conversion_error(error: serde_json::Error) -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::GenericError {
        message: format!("Failed to convert GCP Compute model: {error}"),
    })
}

fn compute_error(
    error: google_cloud_gax::error::Error,
    resource_type: &str,
    resource_name: &str,
) -> AlienError<CloudClientErrorData> {
    if gax_error_is_not_found(&error) {
        return AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        });
    }

    if gax_error_is_conflict(&error) {
        return AlienError::new(CloudClientErrorData::RemoteResourceConflict {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
            message: error.to_string(),
        });
    }

    if gax_error_is_permission_denied(&error) {
        return AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        });
    }

    AlienError::new(CloudClientErrorData::GenericError {
        message: error.to_string(),
    })
}

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == StatusCode::NOT_FOUND.as_u16())
}

fn gax_error_is_conflict(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::AlreadyExists)
        || error
            .http_status_code()
            .is_some_and(|code| code == StatusCode::CONFLICT.as_u16())
}

fn gax_error_is_permission_denied(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::PermissionDenied)
        || error
            .http_status_code()
            .is_some_and(|code| code == StatusCode::FORBIDDEN.as_u16())
}

fn compute_endpoint(endpoint: &str) -> String {
    endpoint
        .trim_end_matches('/')
        .trim_end_matches("/compute/v1")
        .to_string()
}

fn resource_name(name: &Option<String>) -> String {
    name.clone().unwrap_or_else(|| "<unnamed>".to_string())
}
