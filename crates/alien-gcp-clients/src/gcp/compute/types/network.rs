use bon::Builder;
use serde::{Deserialize, Serialize};

// =============================================================================================
// Data Structures - Network
// =============================================================================================

/// Represents a VPC network resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/networks
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource. Must be 1-63 characters, lowercase letters, numbers, or hyphens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// When true, VMs in this network without external IPs can access Google APIs using Private Google Access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_create_subnetworks: Option<bool>,

    /// Server-defined list of subnetwork URLs for this VPC network.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnetworks: Vec<String>,

    /// The network routing mode (REGIONAL or GLOBAL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_config: Option<NetworkRoutingConfig>,

    /// Maximum Transmission Unit in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtu: Option<i32>,

    /// Firewall policy enforced on the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firewall_policy: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Gateway IPv4 address (output only, for legacy networks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_i_pv4: Option<String>,

    /// Internal IPv6 range for this network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_ipv6_range: Option<String>,

    /// Type of resource (always "compute#network").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Network firewall policy enforcement order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_firewall_policy_enforcement_order: Option<NetworkFirewallPolicyEnforcementOrder>,
}

/// Routing configuration for a network.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRoutingConfig {
    /// The network-wide routing mode: REGIONAL or GLOBAL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_mode: Option<RoutingMode>,
}

/// Network routing mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RoutingMode {
    /// Regional routing: routes are only advertised to routers in the same region.
    Regional,
    /// Global routing: routes are advertised to all routers in the network.
    Global,
}

/// Network firewall policy enforcement order.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NetworkFirewallPolicyEnforcementOrder {
    /// Evaluate firewall policy before VPC firewall rules.
    BeforeClassicFirewall,
    /// Evaluate firewall policy after VPC firewall rules.
    AfterClassicFirewall,
}

// =============================================================================================
// Data Structures - Subnetwork
// =============================================================================================

/// Represents a subnetwork resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/subnetworks
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Subnetwork {
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

    /// URL of the network this subnetwork belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// IP CIDR range for this subnetwork (e.g., "10.0.0.0/24").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_cidr_range: Option<String>,

    /// URL of the region this subnetwork belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Gateway address for default routes to IPs within this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_address: Option<String>,

    /// Whether VMs in this subnetwork can access Google services without external IPs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip_google_access: Option<bool>,

    /// Purpose of the subnetwork (PRIVATE, INTERNAL_HTTPS_LOAD_BALANCER, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<SubnetworkPurpose>,

    /// Role of the subnetwork (ACTIVE or BACKUP for INTERNAL_HTTPS_LOAD_BALANCER).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<SubnetworkRole>,

    /// Secondary IP ranges for this subnetwork.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary_ip_ranges: Vec<SubnetworkSecondaryRange>,

    /// Fingerprint of this resource (for optimistic locking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Whether flow logs are enabled for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_flow_logs: Option<bool>,

    /// Log configuration for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<SubnetworkLogConfig>,

    /// Stack type for this subnetwork (IPV4_ONLY or IPV4_IPV6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_type: Option<StackType>,

    /// IPv6 access type for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_access_type: Option<Ipv6AccessType>,

    /// IPv6 CIDR range for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_cidr_range: Option<String>,

    /// External IPv6 prefix for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ipv6_prefix: Option<String>,

    /// Internal IPv6 prefix for this subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_ipv6_prefix: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#subnetwork").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Private IPv6 Google access type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ipv6_google_access: Option<PrivateIpv6GoogleAccess>,
}

/// Purpose of a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubnetworkPurpose {
    /// Regular user-created subnetwork.
    Private,
    /// Reserved for Internal HTTP(S) Load Balancer.
    InternalHttpsLoadBalancer,
    /// Reserved for Regional Internal HTTP(S) Load Balancer.
    RegionalManagedProxy,
    /// Reserved for Global Internal HTTP(S) Load Balancer.
    GlobalManagedProxy,
    /// Reserved for Private Service Connect.
    PrivateServiceConnect,
    /// Reserved for Private NAT.
    PrivateNat,
}

/// Role of a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubnetworkRole {
    /// Active role.
    Active,
    /// Backup role.
    Backup,
}

/// Secondary IP range for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SubnetworkSecondaryRange {
    /// Name of the secondary range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_name: Option<String>,

    /// IP CIDR range for the secondary range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_cidr_range: Option<String>,
}

/// Log configuration for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SubnetworkLogConfig {
    /// Whether to enable flow logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Aggregation interval for flow logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregation_interval: Option<AggregationInterval>,

    /// Sampling rate for flow logs (0.0-1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_sampling: Option<f64>,

    /// Metadata to include in flow logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SubnetworkLogConfigMetadata>,

    /// Custom metadata fields to include.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metadata_fields: Vec<String>,

    /// Filter expression for flow logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_expr: Option<String>,
}

/// Aggregation interval for flow logs.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AggregationInterval {
    /// 5 second interval.
    Interval5Sec,
    /// 30 second interval.
    Interval30Sec,
    /// 1 minute interval.
    Interval1Min,
    /// 5 minute interval.
    Interval5Min,
    /// 10 minute interval.
    Interval10Min,
    /// 15 minute interval.
    Interval15Min,
}

/// Metadata configuration for subnetwork logs.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubnetworkLogConfigMetadata {
    /// Exclude all metadata.
    ExcludeAllMetadata,
    /// Include all metadata.
    IncludeAllMetadata,
    /// Include custom metadata only.
    CustomMetadata,
}

/// Stack type for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StackType {
    /// IPv4 only.
    Ipv4Only,
    /// Dual-stack (IPv4 and IPv6).
    Ipv4Ipv6,
}

/// IPv6 access type for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Ipv6AccessType {
    /// External IPv6 access.
    External,
    /// Internal IPv6 access.
    Internal,
}

/// Private IPv6 Google access type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrivateIpv6GoogleAccess {
    /// Disable private IPv6 Google access.
    DisableGoogleAccess,
    /// Enable outbound VM access to Google services via IPv6.
    EnableOutboundVmAccessToGoogle,
    /// Enable bidirectional access to Google services via IPv6.
    EnableBidirectionalAccessToGoogle,
}

// =============================================================================================
// Data Structures - Router
// =============================================================================================

/// Represents a Cloud Router resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/routers
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Router {
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

    /// URL of the region this router belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// URL of the network this router belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// BGP information for this router.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bgp: Option<RouterBgp>,

    /// BGP peers for this router.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bgp_peers: Vec<RouterBgpPeer>,

    /// NAT configurations for this router.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nats: Vec<RouterNat>,

    /// Router interfaces.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<RouterInterface>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#router").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Encrypted interconnect router flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_interconnect_router: Option<bool>,
}

/// BGP configuration for a router.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterBgp {
    /// Local BGP Autonomous System Number (ASN).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn: Option<u32>,

    /// Advertise mode for this BGP speaker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advertise_mode: Option<AdvertiseMode>,

    /// Groups of prefixes to be advertised.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advertised_groups: Vec<AdvertisedGroup>,

    /// Individual prefixes to be advertised.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advertised_ip_ranges: Vec<RouterAdvertisedIpRange>,

    /// Keepalive interval in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keepalive_interval: Option<u32>,
}

/// BGP advertise mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdvertiseMode {
    /// Advertise default routes.
    Default,
    /// Advertise custom routes.
    Custom,
}

/// Groups of prefixes to advertise.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdvertisedGroup {
    /// Advertise all subnets.
    AllSubnets,
}

/// Individual IP range to advertise.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterAdvertisedIpRange {
    /// IP range to advertise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,

    /// Description of this IP range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// BGP peer configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterBgpPeer {
    /// Name of this BGP peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Name of the interface the BGP peer is associated with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_name: Option<String>,

    /// IP address of the peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_ip_address: Option<String>,

    /// Peer BGP ASN.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_asn: Option<u32>,

    /// Advertise mode for this BGP peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advertise_mode: Option<AdvertiseMode>,

    /// Advertised groups for this peer.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advertised_groups: Vec<AdvertisedGroup>,

    /// Advertised IP ranges for this peer.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advertised_ip_ranges: Vec<RouterAdvertisedIpRange>,

    /// BGP peer status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_type: Option<ManagementType>,

    /// Whether this peer is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Advertised route priority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advertised_route_priority: Option<u32>,
}

/// Management type for a BGP peer.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagementType {
    /// Peer is managed by the user.
    ManagedByUser,
    /// Peer is managed by an attachment.
    ManagedByAttachment,
}

/// Router interface configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterInterface {
    /// Name of this interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// IP range for this interface (CIDR format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_range: Option<String>,

    /// URL of the linked VPN tunnel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_vpn_tunnel: Option<String>,

    /// URL of the linked interconnect attachment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_interconnect_attachment: Option<String>,

    /// Management type for this interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_type: Option<ManagementType>,

    /// Subnetwork this interface is attached to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// Private IP address for this interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip_address: Option<String>,

    /// Redundant interface for this router interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redundant_interface: Option<String>,
}

/// Cloud NAT configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNat {
    /// Name of this NAT configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Type of NAT (endpoint-independent or endpoint-dependent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<NatType>,

    /// Source subnetwork IP ranges to NAT.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_subnetwork_ip_ranges_to_nat: Option<SourceSubnetworkIpRangesToNat>,

    /// Subnetworks to NAT.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnetworks: Vec<RouterNatSubnetworkToNat>,

    /// NAT IP allocation option.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_ip_allocate_option: Option<NatIpAllocateOption>,

    /// NAT IPs to use.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nat_ips: Vec<String>,

    /// Drain NAT IPs.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drain_nat_ips: Vec<String>,

    /// Minimum ports per VM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ports_per_vm: Option<i32>,

    /// Maximum ports per VM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ports_per_vm: Option<i32>,

    /// UDP idle timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udp_idle_timeout_sec: Option<i32>,

    /// ICMP idle timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icmp_idle_timeout_sec: Option<i32>,

    /// TCP established idle timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_established_idle_timeout_sec: Option<i32>,

    /// TCP transitory idle timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_transitory_idle_timeout_sec: Option<i32>,

    /// TCP time wait timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_time_wait_timeout_sec: Option<i32>,

    /// Log configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<RouterNatLogConfig>,

    /// Whether endpoint-independent mapping is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_endpoint_independent_mapping: Option<bool>,

    /// Whether dynamic port allocation is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_dynamic_port_allocation: Option<bool>,

    /// NAT rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RouterNatRule>,

    /// Auto network tier for this NAT.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_network_tier: Option<NetworkTier>,
}

/// NAT type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NatType {
    /// Public NAT.
    Public,
    /// Private NAT.
    Private,
}

/// Source subnetwork IP ranges to NAT.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceSubnetworkIpRangesToNat {
    /// NAT all primary and secondary IP ranges of all subnetworks.
    AllSubnetworksAllIpRanges,
    /// NAT only primary IP ranges of all subnetworks.
    AllSubnetworksAllPrimaryIpRanges,
    /// NAT only specific subnetworks.
    ListOfSubnetworks,
}

/// NAT IP allocation option.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NatIpAllocateOption {
    /// Allocate NAT IPs automatically.
    AutoOnly,
    /// Use manually specified NAT IPs.
    ManualOnly,
}

/// Subnetwork to NAT configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNatSubnetworkToNat {
    /// Name of the subnetwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Source IP ranges to NAT.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ip_ranges_to_nat: Vec<SourceIpRangesToNat>,

    /// Secondary IP range names to NAT.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary_ip_range_names: Vec<String>,
}

/// Source IP ranges to NAT for a subnetwork.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceIpRangesToNat {
    /// NAT all IP ranges.
    AllIpRanges,
    /// NAT primary IP range only.
    PrimaryIpRange,
    /// NAT only specified secondary IP ranges.
    ListOfSecondaryIpRanges,
}

/// NAT log configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNatLogConfig {
    /// Whether to enable logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Log filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<NatLogFilter>,
}

/// NAT log filter.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NatLogFilter {
    /// Log all events.
    All,
    /// Log errors only.
    ErrorsOnly,
    /// Log translations only.
    TranslationsOnly,
}

/// NAT rule.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNatRule {
    /// Rule number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_number: Option<u32>,

    /// Description of this rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Match condition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#match: Option<String>,

    /// Action to take when the rule matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<RouterNatRuleAction>,
}

/// NAT rule action.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterNatRuleAction {
    /// Source NAT active IPs.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_nat_active_ips: Vec<String>,

    /// Source NAT drain IPs.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_nat_drain_ips: Vec<String>,

    /// Source NAT active ranges.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_nat_active_ranges: Vec<String>,

    /// Source NAT drain ranges.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_nat_drain_ranges: Vec<String>,
}

/// Network tier.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NetworkTier {
    /// Premium tier.
    Premium,
    /// Standard tier.
    Standard,
}

/// List of routers.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RouterList {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// List of routers.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Router>,

    /// Server-defined URL for this resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Token for next page of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,

    /// Type of resource (always "compute#routerList").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

// =============================================================================================
// Data Structures - Firewall
// =============================================================================================

/// Represents a firewall rule resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/firewalls
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Firewall {
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

    /// URL of the network this firewall applies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Priority for this rule (0-65535, lower is higher priority).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,

    /// Direction of traffic (INGRESS or EGRESS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<FirewallDirection>,

    /// Action (allow or deny).
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<FirewallAllowed>,

    /// Denied traffic specifications.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied: Vec<FirewallDenied>,

    /// Source IP ranges for INGRESS rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ranges: Vec<String>,

    /// Destination IP ranges for EGRESS rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destination_ranges: Vec<String>,

    /// Source tags for INGRESS rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_tags: Vec<String>,

    /// Target tags for this rule.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_tags: Vec<String>,

    /// Source service accounts for INGRESS rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_service_accounts: Vec<String>,

    /// Target service accounts for this rule.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_service_accounts: Vec<String>,

    /// Whether the rule is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,

    /// Whether logging is enabled for this rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_config: Option<FirewallLogConfig>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#firewall").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Direction of a firewall rule.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FirewallDirection {
    /// Incoming traffic.
    Ingress,
    /// Outgoing traffic.
    Egress,
}

/// Allowed traffic specification.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FirewallAllowed {
    /// IP protocol (tcp, udp, icmp, esp, ah, sctp, ipip, all).
    #[serde(rename = "IPProtocol", skip_serializing_if = "Option::is_none")]
    pub ip_protocol: Option<String>,

    /// Ports to allow (e.g., "80", "8080-8090").
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
}

/// Denied traffic specification.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FirewallDenied {
    /// IP protocol (tcp, udp, icmp, esp, ah, sctp, ipip, all).
    #[serde(rename = "IPProtocol", skip_serializing_if = "Option::is_none")]
    pub ip_protocol: Option<String>,

    /// Ports to deny.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
}

/// Firewall log configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FirewallLogConfig {
    /// Whether to enable logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,

    /// Metadata to include in logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<FirewallLogConfigMetadata>,
}

/// Metadata configuration for firewall logs.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FirewallLogConfigMetadata {
    /// Exclude all metadata.
    ExcludeAllMetadata,
    /// Include all metadata.
    IncludeAllMetadata,
}

/// List of firewall rules.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FirewallList {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// List of firewall rules.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Firewall>,

    /// Server-defined URL for this resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Token for next page of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,

    /// Type of resource (always "compute#firewallList").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}
