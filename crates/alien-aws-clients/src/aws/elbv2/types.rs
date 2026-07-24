use super::*;

// ---------------------------------------------------------------------------
// Common Types
// ---------------------------------------------------------------------------

/// A tag for an ELB resource.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ElbTag {
    /// The tag key.
    pub key: String,
    /// The tag value.
    pub value: String,
}

/// A subnet mapping for a load balancer.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct SubnetMapping {
    /// The subnet ID.
    pub subnet_id: String,
    /// The allocation ID of the Elastic IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation_id: Option<String>,
    /// The private IPv4 address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ipv4_address: Option<String>,
}

/// A target description.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct TargetDescription {
    /// The ID of the target (instance ID, IP address, or Lambda ARN).
    pub id: String,
    /// The port on which the target is listening.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    /// The Availability Zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone: Option<String>,
}

/// A certificate for a listener.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Certificate {
    /// The ARN of the certificate.
    pub certificate_arn: String,
    /// Whether this is the default certificate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

/// A matcher for health checks.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Matcher {
    /// The HTTP codes for success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_code: Option<String>,
    /// The gRPC codes for success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grpc_code: Option<String>,
}

// ---------------------------------------------------------------------------
// Load Balancer Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a load balancer.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateLoadBalancerRequest {
    /// The name of the load balancer.
    pub name: String,
    /// The IDs of the subnets.
    pub subnets: Vec<String>,
    /// The subnet mappings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_mappings: Option<Vec<SubnetMapping>>,
    /// The IDs of the security groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_groups: Option<Vec<String>>,
    /// The scheme: internet-facing or internal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    /// The type: application, network, or gateway.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_type: Option<String>,
    /// The IP address type: ipv4 or dualstack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address_type: Option<String>,
    /// Tags for the load balancer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ElbTag>>,
}

/// Response from creating a load balancer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateLoadBalancerResponse {
    pub create_load_balancer_result: CreateLoadBalancerResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateLoadBalancerResult {
    pub load_balancers: Option<LoadBalancersWrapper>,
}

#[derive(Debug, Clone, Serialize, Builder)]
pub struct ModifyLoadBalancerAttributesRequest {
    pub load_balancer_arn: String,
    pub attributes: Vec<LoadBalancerAttribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyLoadBalancerAttributesResponse {
    pub modify_load_balancer_attributes_result: ModifyLoadBalancerAttributesResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyLoadBalancerAttributesResult {
    pub attributes: Option<LoadBalancerAttributesWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerAttributesWrapper {
    #[serde(rename = "member")]
    pub members: Vec<LoadBalancerAttribute>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancersWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<LoadBalancer>,
}

/// Request to describe load balancers.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeLoadBalancersRequest {
    /// The ARNs of the load balancers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_arns: Option<Vec<String>>,
    /// The names of the load balancers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    /// The marker for the next set of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// The maximum number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
}

/// Response from describing load balancers.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeLoadBalancersResponse {
    pub describe_load_balancers_result: DescribeLoadBalancersResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeLoadBalancersResult {
    pub load_balancers: Option<LoadBalancersWrapper>,
    pub next_marker: Option<String>,
}

/// A load balancer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancer {
    pub load_balancer_arn: Option<String>,
    pub load_balancer_name: Option<String>,
    #[serde(rename = "DNSName")]
    pub dns_name: Option<String>,
    pub canonical_hosted_zone_id: Option<String>,
    pub created_time: Option<String>,
    pub scheme: Option<String>,
    pub state: Option<LoadBalancerState>,
    #[serde(rename = "Type")]
    pub load_balancer_type: Option<String>,
    pub availability_zones: Option<AvailabilityZonesWrapper>,
    pub security_groups: Option<SecurityGroupsWrapper>,
    pub ip_address_type: Option<String>,
    pub vpc_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerState {
    pub code: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AvailabilityZonesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<AvailabilityZone>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AvailabilityZone {
    pub zone_name: Option<String>,
    pub subnet_id: Option<String>,
    pub outpost_id: Option<String>,
    pub load_balancer_addresses: Option<LoadBalancerAddressesWrapper>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerAddressesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<LoadBalancerAddress>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerAddress {
    pub ip_address: Option<String>,
    pub allocation_id: Option<String>,
    pub private_i_pv4_address: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SecurityGroupsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<String>,
}

// ---------------------------------------------------------------------------
// Target Group Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a target group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateTargetGroupRequest {
    /// The name of the target group.
    pub name: String,
    /// The protocol for the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// The protocol version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    /// The port for the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    /// The VPC ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_id: Option<String>,
    /// The health check protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_protocol: Option<String>,
    /// The health check port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_port: Option<String>,
    /// Whether health checks are enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_enabled: Option<bool>,
    /// The health check path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_path: Option<String>,
    /// The health check interval in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_interval_seconds: Option<i32>,
    /// The health check timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_timeout_seconds: Option<i32>,
    /// The number of consecutive successful health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy_threshold_count: Option<i32>,
    /// The number of consecutive failed health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unhealthy_threshold_count: Option<i32>,
    /// The matcher for health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<Matcher>,
    /// The target type: instance, ip, lambda, or alb.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_type: Option<String>,
    /// The IP address type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address_type: Option<String>,
    /// Tags for the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ElbTag>>,
}

/// Response from creating a target group.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateTargetGroupResponse {
    pub create_target_group_result: CreateTargetGroupResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateTargetGroupResult {
    pub target_groups: Option<TargetGroupsWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<TargetGroup>,
}

/// Request to describe target groups.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeTargetGroupsRequest {
    /// The ARN of the load balancer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_arn: Option<String>,
    /// The ARNs of the target groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_group_arns: Option<Vec<String>>,
    /// The names of the target groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    /// The marker for the next set of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// The maximum number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
}

/// Response from describing target groups.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTargetGroupsResponse {
    pub describe_target_groups_result: DescribeTargetGroupsResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTargetGroupsResult {
    pub target_groups: Option<TargetGroupsWrapper>,
    pub next_marker: Option<String>,
}

/// Request to modify a target group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ModifyTargetGroupRequest {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The health check protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_protocol: Option<String>,
    /// The health check port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_port: Option<String>,
    /// The health check path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_path: Option<String>,
    /// Whether health checks are enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_enabled: Option<bool>,
    /// The health check interval in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_interval_seconds: Option<i32>,
    /// The health check timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_timeout_seconds: Option<i32>,
    /// The number of consecutive successful health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy_threshold_count: Option<i32>,
    /// The number of consecutive failed health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unhealthy_threshold_count: Option<i32>,
    /// The matcher for health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<Matcher>,
}

/// Response from modifying a target group.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyTargetGroupResponse {
    pub modify_target_group_result: ModifyTargetGroupResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyTargetGroupResult {
    pub target_groups: Option<TargetGroupsWrapper>,
}

#[derive(Debug, Clone, Serialize, Builder)]
pub struct ModifyTargetGroupAttributesRequest {
    pub target_group_arn: String,
    pub attributes: Vec<TargetGroupAttribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyTargetGroupAttributesResponse {
    pub modify_target_group_attributes_result: ModifyTargetGroupAttributesResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyTargetGroupAttributesResult {
    pub attributes: Option<TargetGroupAttributesWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupAttributesWrapper {
    #[serde(rename = "member")]
    pub members: Vec<TargetGroupAttribute>,
}

/// A target group.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroup {
    pub target_group_arn: Option<String>,
    pub target_group_name: Option<String>,
    pub protocol: Option<String>,
    pub protocol_version: Option<String>,
    pub port: Option<i32>,
    pub vpc_id: Option<String>,
    pub health_check_protocol: Option<String>,
    pub health_check_port: Option<String>,
    pub health_check_enabled: Option<bool>,
    pub health_check_interval_seconds: Option<i32>,
    pub health_check_timeout_seconds: Option<i32>,
    pub healthy_threshold_count: Option<i32>,
    pub unhealthy_threshold_count: Option<i32>,
    pub health_check_path: Option<String>,
    pub matcher: Option<MatcherResponse>,
    pub load_balancer_arns: Option<LoadBalancerArnsWrapper>,
    pub target_type: Option<String>,
    pub ip_address_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MatcherResponse {
    pub http_code: Option<String>,
    pub grpc_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerArnsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<String>,
}

// ---------------------------------------------------------------------------
// Target Operations Request/Response Types
// ---------------------------------------------------------------------------

/// Request to register targets.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct RegisterTargetsRequest {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The targets to register.
    pub targets: Vec<TargetDescription>,
}

/// Request to deregister targets.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DeregisterTargetsRequest {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The targets to deregister.
    pub targets: Vec<TargetDescription>,
}

/// Request to describe target health.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DescribeTargetHealthRequest {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The targets to describe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<TargetDescription>>,
}

/// Response from describing target health.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTargetHealthResponse {
    pub describe_target_health_result: DescribeTargetHealthResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTargetHealthResult {
    pub target_health_descriptions: Option<TargetHealthDescriptionsWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetHealthDescriptionsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<TargetHealthDescription>,
}

/// A target health description.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetHealthDescription {
    pub target: Option<TargetDescriptionResponse>,
    pub health_check_port: Option<String>,
    pub target_health: Option<TargetHealth>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetDescriptionResponse {
    pub id: Option<String>,
    pub port: Option<i32>,
    pub availability_zone: Option<String>,
}

/// The health of a target.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetHealth {
    pub state: Option<String>,
    pub reason: Option<String>,
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Listener Request/Response Types
// ---------------------------------------------------------------------------

/// An action for a listener.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Action {
    /// The action type.
    pub action_type: String,
    /// The ARN of the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_group_arn: Option<String>,
    /// The order for the action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    /// The forward configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward_config: Option<ForwardActionConfig>,
    /// The redirect configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_config: Option<RedirectActionConfig>,
    /// The fixed response configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed_response_config: Option<FixedResponseActionConfig>,
}

/// Forward action configuration.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ForwardActionConfig {
    /// The target groups to forward to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_groups: Option<Vec<TargetGroupTuple>>,
    /// The target group stickiness configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_group_stickiness_config: Option<TargetGroupStickinessConfig>,
}

/// A target group tuple for forwarding.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct TargetGroupTuple {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The weight for the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<i32>,
}

/// Target group stickiness configuration.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct TargetGroupStickinessConfig {
    /// Whether stickiness is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// The duration in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<i32>,
}

/// Redirect action configuration.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct RedirectActionConfig {
    /// The status code: HTTP_301 or HTTP_302.
    pub status_code: String,
    /// The protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// The port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    /// The host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The query string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// Fixed response action configuration.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct FixedResponseActionConfig {
    /// The HTTP status code.
    pub status_code: String,
    /// The content type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// The message body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_body: Option<String>,
}

/// Request to create a listener.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateListenerRequest {
    /// The ARN of the load balancer.
    pub load_balancer_arn: String,
    /// The port for the listener.
    pub port: i32,
    /// The protocol for the listener.
    pub protocol: String,
    /// The default actions.
    pub default_actions: Vec<Action>,
    /// The SSL policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_policy: Option<String>,
    /// The certificates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificates: Option<Vec<Certificate>>,
    /// The ALPN policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpn_policy: Option<Vec<String>>,
    /// Tags for the listener.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ElbTag>>,
}

/// Response from creating a listener.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateListenerResponse {
    pub create_listener_result: CreateListenerResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateListenerResult {
    pub listeners: Option<ListenersWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListenersWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<Listener>,
}

/// Request to describe listeners.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeListenersRequest {
    /// The ARN of the load balancer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_arn: Option<String>,
    /// The ARNs of the listeners.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listener_arns: Option<Vec<String>>,
    /// The marker for the next set of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// The maximum number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
}

/// Response from describing listeners.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeListenersResponse {
    pub describe_listeners_result: DescribeListenersResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeListenersResult {
    pub listeners: Option<ListenersWrapper>,
    pub next_marker: Option<String>,
}

/// Request to modify a listener.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ModifyListenerRequest {
    /// The ARN of the listener.
    pub listener_arn: String,
    /// The new port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    /// The new protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// The new SSL policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_policy: Option<String>,
    /// The new certificates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificates: Option<Vec<Certificate>>,
    /// The new default actions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_actions: Option<Vec<Action>>,
}

/// Response from modifying a listener.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyListenerResponse {
    pub modify_listener_result: ModifyListenerResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyListenerResult {
    pub listeners: Option<ListenersWrapper>,
}

/// A listener.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Listener {
    pub listener_arn: Option<String>,
    pub load_balancer_arn: Option<String>,
    pub port: Option<i32>,
    pub protocol: Option<String>,
    pub certificates: Option<CertificatesWrapper>,
    pub ssl_policy: Option<String>,
    pub default_actions: Option<ActionsWrapper>,
    pub alpn_policy: Option<AlpnPolicyWrapper>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CertificatesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<CertificateResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CertificateResponse {
    pub certificate_arn: Option<String>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ActionsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<ActionResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ActionResponse {
    #[serde(rename = "Type")]
    pub action_type: Option<String>,
    pub target_group_arn: Option<String>,
    pub order: Option<i32>,
    pub forward_config: Option<ForwardActionConfigResponse>,
    pub redirect_config: Option<RedirectActionConfigResponse>,
    pub fixed_response_config: Option<FixedResponseActionConfigResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ForwardActionConfigResponse {
    pub target_groups: Option<TargetGroupTuplesWrapper>,
    pub target_group_stickiness_config: Option<TargetGroupStickinessConfigResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupTuplesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<TargetGroupTupleResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupTupleResponse {
    pub target_group_arn: Option<String>,
    pub weight: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupStickinessConfigResponse {
    pub enabled: Option<bool>,
    pub duration_seconds: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RedirectActionConfigResponse {
    pub protocol: Option<String>,
    pub port: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub query: Option<String>,
    pub status_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FixedResponseActionConfigResponse {
    pub status_code: Option<String>,
    pub content_type: Option<String>,
    pub message_body: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AlpnPolicyWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<String>,
}
