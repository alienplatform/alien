//! AWS Network Controller
//!
//! This module implements the AWS-specific network controller for managing VPC infrastructure
//! including VPCs, subnets, Internet Gateways, NAT Gateways, route tables, and security groups.
//!
//! # Create Mode
//!
//! When `NetworkSettings::Create` is configured, the controller creates:
//! - VPC with the specified or auto-generated CIDR block
//! - Public subnets (if any resource needs public ingress)
//! - Private subnets
//! - Internet Gateway (if public subnets exist)
//! - NAT Gateway (if configured)
//! - Route tables for public and private subnets
//! - Security group for internal VPC communication
//!
//! # BYO-VPC Mode
//!
//! When `NetworkSettings::ByoVpcAws` is configured, the controller:
//! - Stores the provided VPC ID, subnet IDs, and security group IDs
//! - Validates the infrastructure exists (via preflights)
//! - Transitions directly to Ready state

use alien_aws_clients::ec2::{
    AllocateAddressRequest, AssociateRouteTableRequest, AttachInternetGatewayRequest,
    AuthorizeSecurityGroupEgressRequest, AuthorizeSecurityGroupIngressRequest,
    CreateInternetGatewayRequest, CreateNatGatewayRequest, CreateRouteRequest,
    CreateRouteTableRequest, CreateSecurityGroupRequest, CreateSubnetRequest, CreateVpcRequest,
    DescribeAddressesResponse, DescribeAvailabilityZonesRequest, DescribeInternetGatewaysRequest,
    DescribeNatGatewaysRequest, DescribeNetworkInterfacesRequest, DescribeRouteTablesRequest,
    DescribeSecurityGroupsRequest, DescribeSubnetsRequest, DescribeVpcsRequest,
    DetachInternetGatewayRequest, Filter, IpPermission, IpPermissionResponse, IpRange,
    ModifyVpcAttributeRequest, NatGateway, NetworkInterface, RouteTable, RouteTableAssociation,
    SecurityGroup, Subnet, Tag, TagSet, TagSpecification,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::aws::AwsFailureDomainSubnets;
use alien_core::{
    standard_resource_tags, AwsVpcNetworkHeartbeatData, HeartbeatBackend, Network,
    NetworkHeartbeatData, NetworkHeartbeatStatus, NetworkOutputs, NetworkSettings, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

fn is_security_group_duplicate(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { message, .. })
            if message.contains("InvalidGroup.Duplicate")
                || message.contains("already exists")
    )
}

fn is_not_found(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

/// EC2 reports `DependencyViolation`, `ResourceInUse`, `InvalidIPAddress.InUse` and
/// `Resource.AlreadyAssociated` as conflicts.
fn is_conflict(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { .. })
    )
}

fn is_access_denied(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteAccessDenied { .. })
    )
}

/// `Gateway.NotAttached`: the internet gateway is already detached from the VPC.
fn is_gateway_not_attached(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { resource_type, .. })
            if resource_type == "InternetGateway"
    )
}

/// Polls of a NAT gateway in `deleting` (15 s apart). Deletion usually takes a few minutes.
const NAT_GATEWAY_DELETION_MAX_POLLS: u32 = 60;
/// Polls of an Elastic IP still associated after its NAT gateway is gone (15 s apart).
const ELASTIC_IP_RELEASE_MAX_POLLS: u32 = 20;
/// Polls of a security group or subnet that still holds network interfaces (30 s apart).
/// Lambda network interfaces can take 20-40 minutes to go away after the function.
const NETWORK_INTERFACE_DRAIN_MAX_POLLS: u32 = 90;
/// Polls of a route table, internet gateway or VPC that still has dependents (15 s apart).
const DEPENDENCY_DRAIN_MAX_POLLS: u32 = 40;

/// Idempotency token for `CreateNatGateway`. EC2 allows up to 64 ASCII characters.
fn nat_gateway_client_token(allocation_id: &str) -> String {
    format!("alien-nat-{allocation_id}")
}

fn nat_gateway_failure_reason(nat_gateway: &NatGateway) -> String {
    match (&nat_gateway.failure_code, &nat_gateway.failure_message) {
        (Some(code), Some(message)) => format!("{code}: {message}"),
        (Some(reason), None) | (None, Some(reason)) => reason.clone(),
        (None, None) => "AWS reported no failure reason".to_string(),
    }
}

/// Filters matching the ownership tags `create_tag_specification` puts on every object
/// this controller creates for a resource.
fn owned_tag_filters(resource_prefix: &str, resource_id: &str) -> Vec<Filter> {
    let mut filters: Vec<Filter> = standard_resource_tags(resource_prefix, resource_id)
        .into_iter()
        .map(|(key, value)| Filter {
            name: format!("tag:{key}"),
            values: vec![value],
        })
        .collect();
    filters.sort_by(|a, b| a.name.cmp(&b.name));
    filters
}

/// Tag that carries the create token of the controller that made an object.
///
/// Each object gets its token once, recorded in controller state before its first create
/// call, and every retry reuses it. After a lost response the object is therefore found by
/// that exact token, never by a match on name or CIDR alone. The token never changes while the
/// object is outstanding: a read that briefly misses a just-created object (EC2 reads are
/// eventually consistent) must not leave that object under a token nobody looks for. Objects
/// left by an earlier controller of the resource carry other tokens and are never adopted.
const CREATE_ATTEMPT_TAG: &str = "CreateAttempt";

fn new_create_attempt_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The one ID among `ids`, or an error when a token matched several objects. A token is
/// only ever reused for the same object, so several matches mean a create was repeated
/// while a read missed the first object; a human decides which one stays.
fn single_create_attempt_match(
    ids: Vec<String>,
    object: &str,
    token: &str,
    resource_id: &str,
) -> Result<Option<String>> {
    match ids.as_slice() {
        [] => Ok(None),
        [id] => Ok(Some(id.clone())),
        _ => Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Found {} {object}s with create token {token} ({}); delete the extra ones and retry",
                ids.len(),
                ids.join(", ")
            ),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

fn create_attempt_tag(token: &str) -> (String, String) {
    (CREATE_ATTEMPT_TAG.to_string(), token.to_string())
}

/// Ownership-tag filters plus the create-attempt token.
fn create_attempt_filters(resource_prefix: &str, resource_id: &str, token: &str) -> Vec<Filter> {
    let mut filters = owned_tag_filters(resource_prefix, resource_id);
    filters.push(Filter {
        name: format!("tag:{CREATE_ATTEMPT_TAG}"),
        values: vec![token.to_string()],
    });
    filters
}

fn has_create_attempt_tags(
    tag_set: Option<&TagSet>,
    resource_prefix: &str,
    resource_id: &str,
    token: &str,
) -> bool {
    has_owned_tags(tag_set, resource_prefix, resource_id)
        && tag_set.is_some_and(|set| {
            set.items
                .iter()
                .any(|tag| tag.key == CREATE_ATTEMPT_TAG && tag.value == token)
        })
}

fn has_owned_tags(tag_set: Option<&TagSet>, resource_prefix: &str, resource_id: &str) -> bool {
    let tags = tag_set.map(|set| set.items.as_slice()).unwrap_or_default();
    standard_resource_tags(resource_prefix, resource_id)
        .iter()
        .all(|(key, value)| {
            tags.iter()
                .any(|tag| &tag.key == key && &tag.value == value)
        })
}

/// What keeps a network object from being deleted.
#[derive(Debug, Default)]
struct DeleteBlockers {
    items: Vec<String>,
    /// A blocker the runtime role cannot remove or even list; waiting cannot help.
    missing_permission: bool,
}

fn describe_network_interface(interface: &NetworkInterface) -> String {
    let mut detail = format!(
        "network interface {} ({}",
        interface.network_interface_id.as_deref().unwrap_or("?"),
        interface.status.as_deref().unwrap_or("unknown status")
    );
    if let Some(kind) = &interface.interface_type {
        detail.push_str(&format!(", type {kind}"));
    }
    if let Some(description) = interface.description.as_deref().filter(|d| !d.is_empty()) {
        detail.push_str(&format!(", '{description}'"));
    }
    if let Some(requester) = &interface.requester_id {
        detail.push_str(&format!(", requested by {requester}"));
    }
    detail.push(')');
    detail
}

/// One subnet of the managed network, as `ensure_subnet` finds or creates it.
#[derive(bon::Builder)]
struct SubnetSpec<'a> {
    vpc_id: &'a str,
    cidr: &'a str,
    availability_zone: &'a str,
    /// Value of the subnet's `Name` tag.
    name: String,
    /// `Public` or `Private`, recorded in the subnet's `Type` tag.
    subnet_type: &'a str,
    resource_id: &'a str,
}

fn emit_aws_network_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    controller: &AwsNetworkController,
    vpc_state: Option<String>,
) {
    let route_table_count = [
        controller.public_route_table_id.as_ref(),
        controller.private_route_table_id.as_ref(),
    ]
    .into_iter()
    .filter(|route_table_id| route_table_id.is_some())
    .count() as u32;

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Network::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Network(NetworkHeartbeatData::AwsVpc(
            AwsVpcNetworkHeartbeatData {
                status: NetworkHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: controller
                        .vpc_id
                        .as_ref()
                        .map(|vpc_id| format!("AWS VPC '{}' is reachable", vpc_id)),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                vpc_id: controller.vpc_id.clone(),
                vpc_state,
                cidr_block: controller.cidr_block.clone(),
                public_subnet_ids: controller.public_subnet_ids.clone(),
                private_subnet_ids: controller.private_subnet_ids.clone(),
                availability_zones: controller.availability_zones.clone(),
                internet_gateway_id: controller.internet_gateway_id.clone(),
                nat_gateway_id: controller.nat_gateway_id.clone(),
                route_table_count,
                security_group_id: controller.security_group_id.clone(),
                is_byo_vpc: controller.is_byo_vpc,
            },
        )),
        raw: vec![],
    });
}

fn is_security_group_rule_duplicate(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { message, .. })
            if message.contains("InvalidPermission.Duplicate")
                || message.contains("specified rule")
                || message.contains("already exists")
    )
}

fn has_ipv4_all_protocol_rule(
    permissions: Option<&[IpPermissionResponse]>,
    cidr_block: &str,
) -> bool {
    permissions.unwrap_or_default().iter().any(|permission| {
        permission.ip_protocol.as_deref() == Some("-1")
            && permission.ip_ranges.as_ref().is_some_and(|ranges| {
                ranges
                    .items
                    .iter()
                    .any(|range| range.cidr_ip.as_deref() == Some(cidr_block))
            })
    })
}

/// Return the subset of `desired` subnet IDs that are not already attached
/// via `existing` associations. Used when reusing a route table found by
/// `find_existing_route_table_by_name` so we don't double-associate subnets
/// that survived a crash mid-way through `creating_route_tables`.
fn subnets_needing_association<'a>(
    desired: &'a [String],
    existing: &[RouteTableAssociation],
) -> Vec<&'a String> {
    let already: HashSet<&str> = existing
        .iter()
        .filter_map(|a| a.subnet_id.as_deref())
        .collect();
    desired
        .iter()
        .filter(|s| !already.contains(s.as_str()))
        .collect()
}

const EC2_VPC_EIP_QUOTA_CODE: &str = "L-0263D0A3";

#[derive(Debug, PartialEq)]
enum EipQuotaPreflight {
    Available { used: usize, limit: f64 },
    Exhausted { used: usize, limit: f64 },
    Unknown(&'static str),
}

fn assess_eip_quota(used: Option<usize>, limit: Option<f64>) -> EipQuotaPreflight {
    match (
        used,
        limit.filter(|value| value.is_finite() && *value >= 0.0),
    ) {
        (Some(used), Some(limit)) if used as f64 + 1.0 > limit => {
            EipQuotaPreflight::Exhausted { used, limit }
        }
        (Some(used), Some(limit)) => EipQuotaPreflight::Available { used, limit },
        (None, _) => EipQuotaPreflight::Unknown("current Elastic IP usage is unavailable"),
        (_, None) => EipQuotaPreflight::Unknown("the applied Elastic IP quota is unavailable"),
    }
}

fn quota_consuming_eip_usage(response: DescribeAddressesResponse) -> Option<usize> {
    let addresses = response
        .addresses_set
        .map(|set| set.items)
        .unwrap_or_default();

    // `domain` is required to distinguish the EC2-VPC quota from legacy EC2-Classic
    // addresses. Do not turn incomplete provider data into a false quota failure.
    if addresses.iter().any(|address| address.domain.is_none()) {
        return None;
    }

    Some(
        addresses
            .into_iter()
            .filter(|address| {
                address.domain.as_deref() == Some("vpc")
                    && matches!(address.public_ipv4_pool.as_deref(), None | Some("amazon"))
            })
            .count(),
    )
}

async fn preflight_aws_eip_quota(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<()> {
    let aws_config = ctx.get_aws_config()?;
    let ec2 = ctx.service_provider.get_aws_ec2_client(aws_config).await?;
    let quotas = match ctx
        .service_provider
        .get_aws_service_quotas_client(aws_config)
        .await
    {
        Ok(client) => Some(client),
        Err(error) => {
            warn!(error = %error, "Could not initialize the optional AWS Service Quotas client; continuing because quota preflight is best-effort");
            None
        }
    };

    let used = match ec2.describe_addresses().await {
        Ok(response) => quota_consuming_eip_usage(response),
        Err(error) => {
            warn!(error = %error, "Could not verify current AWS Elastic IP usage; continuing because quota preflight is best-effort");
            None
        }
    };
    let limit = match quotas {
        Some(quotas) => match quotas
            .get_service_quota("ec2", EC2_VPC_EIP_QUOTA_CODE)
            .await
        {
            Ok(response) => response.quota.and_then(|quota| quota.value),
            Err(error) => {
                warn!(error = %error, "Could not read the applied AWS Elastic IP quota; continuing because quota preflight is best-effort");
                None
            }
        },
        None => None,
    };

    match assess_eip_quota(used, limit) {
        EipQuotaPreflight::Available { used, limit } => {
            info!(used, limit, required = 1, "AWS Elastic IP quota preflight passed");
            Ok(())
        }
        EipQuotaPreflight::Exhausted { used, limit } => {
            Err(AlienError::new(ErrorData::InfrastructureError {
                message: format!(
                    "Cannot create the managed AWS network: its NAT gateway requires 1 Elastic IP, but this Region already uses {used} of the applied {limit} EC2-VPC Elastic IP quota. Release an owned Elastic IP, request a quota increase, or configure NetworkSettings::ByoVpcAws. No network resources were created."
                ),
                operation: Some("preflight_elastic_ip_quota".to_string()),
                resource_id: Some(resource_id.to_string()),
            }))
        }
        EipQuotaPreflight::Unknown(reason) => {
            warn!(reason, required = 1, "AWS managed network requires one Elastic IP, but quota preflight is uncertain");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_aws_clients::ec2::{
        Address, AddressSet, IpPermissionSet, IpRangeResponse, IpRangeSet, MockEc2Api,
    };
    use alien_aws_clients::service_quotas::{
        GetServiceQuotaResponse, MockServiceQuotasApi, ServiceQuota,
    };
    use alien_core::{Network, Platform};

    use super::*;
    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};

    #[test]
    fn detects_existing_all_protocol_ipv4_rule() {
        let permissions = [IpPermissionResponse {
            ip_protocol: Some("-1".to_string()),
            from_port: None,
            to_port: None,
            ip_ranges: Some(IpRangeSet {
                items: vec![IpRangeResponse {
                    cidr_ip: Some("10.0.0.0/16".to_string()),
                    description: None,
                }],
            }),
            ipv6_ranges: None,
            groups: None,
            prefix_list_ids: None,
        }];

        assert!(has_ipv4_all_protocol_rule(
            Some(&permissions),
            "10.0.0.0/16"
        ));
    }

    #[test]
    fn ignores_rules_for_other_cidrs_or_protocols() {
        let permissions = [
            IpPermissionResponse {
                ip_protocol: Some("tcp".to_string()),
                from_port: Some(443),
                to_port: Some(443),
                ip_ranges: Some(IpRangeSet {
                    items: vec![IpRangeResponse {
                        cidr_ip: Some("10.0.0.0/16".to_string()),
                        description: None,
                    }],
                }),
                ipv6_ranges: None,
                groups: None,
                prefix_list_ids: None,
            },
            IpPermissionResponse {
                ip_protocol: Some("-1".to_string()),
                from_port: None,
                to_port: None,
                ip_ranges: Some(IpRangeSet {
                    items: vec![IpRangeResponse {
                        cidr_ip: Some("192.168.0.0/16".to_string()),
                        description: None,
                    }],
                }),
                ipv6_ranges: None,
                groups: None,
                prefix_list_ids: None,
            },
        ];

        assert!(!has_ipv4_all_protocol_rule(
            Some(&permissions),
            "10.0.0.0/16"
        ));
    }

    #[test]
    fn handles_empty_permission_sets() {
        let set = IpPermissionSet { items: Vec::new() };

        assert!(!has_ipv4_all_protocol_rule(Some(&set.items), "0.0.0.0/0"));
        assert!(!has_ipv4_all_protocol_rule(None, "0.0.0.0/0"));
    }

    fn assoc(rtb_assoc_id: &str, subnet_id: &str) -> RouteTableAssociation {
        RouteTableAssociation {
            route_table_association_id: Some(rtb_assoc_id.to_string()),
            route_table_id: None,
            subnet_id: Some(subnet_id.to_string()),
            main: Some(false),
        }
    }

    #[test]
    fn associates_all_subnets_when_route_table_has_none() {
        let desired = vec!["subnet-a".to_string(), "subnet-b".to_string()];
        let existing: Vec<RouteTableAssociation> = vec![];

        let to_attach: Vec<_> = subnets_needing_association(&desired, &existing)
            .into_iter()
            .map(String::as_str)
            .collect();

        assert_eq!(to_attach, vec!["subnet-a", "subnet-b"]);
    }

    #[test]
    fn skips_subnets_already_attached_to_reused_route_table() {
        // Scenario: a prior iteration crashed after associating subnet-a but
        // before associating subnet-b. The retry finds the same RT (by tag)
        // and should only associate subnet-b — re-associating subnet-a would
        // either fail or leak a duplicate association_id we already hold.
        let desired = vec!["subnet-a".to_string(), "subnet-b".to_string()];
        let existing = vec![assoc("rtbassoc-001", "subnet-a")];

        let to_attach: Vec<_> = subnets_needing_association(&desired, &existing)
            .into_iter()
            .map(String::as_str)
            .collect();

        assert_eq!(to_attach, vec!["subnet-b"]);
    }

    #[test]
    fn skips_nothing_when_all_already_attached() {
        // If the prior iteration finished associations before crashing, the
        // retry should be a no-op for the association step.
        let desired = vec!["subnet-a".to_string(), "subnet-b".to_string()];
        let existing = vec![
            assoc("rtbassoc-001", "subnet-a"),
            assoc("rtbassoc-002", "subnet-b"),
        ];

        let to_attach: Vec<_> = subnets_needing_association(&desired, &existing)
            .into_iter()
            .map(String::as_str)
            .collect();

        assert!(to_attach.is_empty());
    }

    #[test]
    fn ignores_main_route_table_associations_in_existing_set() {
        // A real RouteTableAssociationSet from AWS includes the implicit
        // "main" association (no subnet_id, main=true). The helper must skip
        // those (filter_map drops them when `subnet_id` is None) so the
        // desired subnets still get processed.
        let desired = vec!["subnet-a".to_string()];
        let existing = vec![RouteTableAssociation {
            route_table_association_id: Some("rtbassoc-main".to_string()),
            route_table_id: Some("rtb-xxx".to_string()),
            subnet_id: None,
            main: Some(true),
        }];

        let to_attach: Vec<_> = subnets_needing_association(&desired, &existing)
            .into_iter()
            .map(String::as_str)
            .collect();

        assert_eq!(to_attach, vec!["subnet-a"]);
    }

    #[test]
    fn handles_empty_desired() {
        let desired: Vec<String> = vec![];
        let existing = vec![assoc("rtbassoc-001", "subnet-a")];

        let to_attach = subnets_needing_association(&desired, &existing);

        assert!(to_attach.is_empty());
    }

    #[test]
    fn eip_preflight_blocks_when_required_address_exceeds_quota() {
        assert_eq!(
            assess_eip_quota(Some(5), Some(5.0)),
            EipQuotaPreflight::Exhausted {
                used: 5,
                limit: 5.0
            }
        );
    }

    #[test]
    fn eip_preflight_accepts_last_available_address() {
        assert_eq!(
            assess_eip_quota(Some(4), Some(5.0)),
            EipQuotaPreflight::Available {
                used: 4,
                limit: 5.0
            }
        );
    }

    #[test]
    fn eip_preflight_is_uncertain_without_usage_or_applied_limit() {
        assert!(matches!(
            assess_eip_quota(None, Some(5.0)),
            EipQuotaPreflight::Unknown(_)
        ));
        assert!(matches!(
            assess_eip_quota(Some(0), None),
            EipQuotaPreflight::Unknown(_)
        ));
    }

    #[test]
    fn eip_usage_excludes_byoip_addresses_from_the_vpc_quota() {
        let response = DescribeAddressesResponse {
            addresses_set: Some(AddressSet {
                items: vec![
                    Address {
                        allocation_id: Some("eipalloc-amazon".to_string()),
                        public_ip: None,
                        domain: Some("vpc".to_string()),
                        association_id: None,
                        network_interface_id: None,
                        public_ipv4_pool: Some("amazon".to_string()),
                        tag_set: None,
                    },
                    Address {
                        allocation_id: Some("eipalloc-amazon-legacy".to_string()),
                        public_ip: None,
                        domain: Some("vpc".to_string()),
                        association_id: None,
                        network_interface_id: None,
                        public_ipv4_pool: None,
                        tag_set: None,
                    },
                    Address {
                        allocation_id: Some("eipalloc-byoip".to_string()),
                        public_ip: None,
                        domain: Some("vpc".to_string()),
                        association_id: None,
                        network_interface_id: None,
                        public_ipv4_pool: Some("ipv4pool-ec2-customer".to_string()),
                        tag_set: None,
                    },
                ],
            }),
        };

        assert_eq!(quota_consuming_eip_usage(response), Some(2));
    }

    #[test]
    fn eip_usage_is_unknown_when_address_domain_is_missing() {
        let response = DescribeAddressesResponse {
            addresses_set: Some(AddressSet {
                items: vec![Address {
                    allocation_id: Some("eipalloc-unknown".to_string()),
                    public_ip: None,
                    domain: None,
                    association_id: None,
                    network_interface_id: None,
                    public_ipv4_pool: None,
                    tag_set: None,
                }],
            }),
        };

        assert_eq!(quota_consuming_eip_usage(response), None);
    }

    #[tokio::test]
    async fn create_start_does_not_reject_byoip_addresses_as_quota_usage() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_addresses().return_once(|| {
            Ok(DescribeAddressesResponse {
                addresses_set: Some(AddressSet {
                    items: vec![
                        Address {
                            allocation_id: Some("eipalloc-amazon-1".to_string()),
                            public_ip: None,
                            domain: Some("vpc".to_string()),
                            association_id: None,
                            network_interface_id: None,
                            public_ipv4_pool: Some("amazon".to_string()),
                            tag_set: None,
                        },
                        Address {
                            allocation_id: Some("eipalloc-byoip".to_string()),
                            public_ip: None,
                            domain: Some("vpc".to_string()),
                            association_id: None,
                            network_interface_id: None,
                            public_ipv4_pool: Some("ipv4pool-ec2-customer".to_string()),
                            tag_set: None,
                        },
                    ],
                }),
            })
        });
        let ec2 = Arc::new(ec2);

        let mut quotas = MockServiceQuotasApi::new();
        quotas.expect_get_service_quota().return_once(|_, _| {
            Ok(GetServiceQuotaResponse {
                quota: Some(ServiceQuota {
                    quota_code: Some(EC2_VPC_EIP_QUOTA_CODE.to_string()),
                    service_code: Some("ec2".to_string()),
                    quota_name: None,
                    value: Some(2.0),
                }),
            })
        });
        let quotas = Arc::new(quotas);

        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        provider
            .expect_get_aws_service_quotas_client()
            .returning(move |_| Ok(quotas.clone()));

        let network = Network::new("quota-test".to_string())
            .settings(NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            })
            .build();
        let mut executor = SingleControllerExecutor::builder()
            .resource(network)
            .controller(AwsNetworkController::default())
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .with_test_dependencies()
            .build()
            .await
            .expect("executor should build");

        executor
            .step()
            .await
            .expect("BYOIP address must not produce a false quota failure");
        assert_eq!(
            executor
                .internal_state::<AwsNetworkController>()
                .expect("network controller state")
                .state,
            AwsNetworkState::CreatingVpc
        );
    }
}

/// AWS Network Controller state machine.
///
/// This controller manages the lifecycle of AWS VPC networking infrastructure.
#[controller]
pub struct AwsNetworkController {
    // VPC resources
    pub vpc_id: Option<String>,
    pub cidr_block: Option<String>,

    // Internet Gateway
    pub(crate) internet_gateway_id: Option<String>,

    // NAT Gateway
    pub nat_gateway_id: Option<String>,
    pub(crate) eip_allocation_id: Option<String>,

    // Subnets
    pub public_subnet_ids: Vec<String>,
    pub private_subnet_ids: Vec<String>,

    // Route Tables
    pub(crate) public_route_table_id: Option<String>,
    pub(crate) private_route_table_id: Option<String>,
    pub(crate) route_table_association_ids: Vec<String>,

    // Security Group. Public so resource controllers wiring a dedicated database
    // security group (which admits 5432 only from this stack SG — see
    // permission-sets/postgres/provision.jsonc) can reference it without
    // re-discovering it by tag (mirrors the already-public GCP `subnetwork_self_link`).
    pub security_group_id: Option<String>,

    // Metadata
    pub(crate) availability_zones: Vec<String>,
    /// Exact subnet membership keyed by real availability zone.
    #[serde(default)]
    pub subnets_by_failure_domain: BTreeMap<String, AwsFailureDomainSubnets>,
    pub is_byo_vpc: bool,

    /// Token of the latest `create_vpc` call, recorded before the call and tagged on the VPC.
    #[serde(default)]
    pub(crate) vpc_create_token: Option<String>,
    /// The subnet whose `create_subnet` call is in flight: recorded before the call, cleared
    /// once its ID is recorded.
    #[serde(default)]
    pub(crate) subnet_create_attempt: Option<SubnetCreateAttempt>,
    /// Token tagged on the NAT gateway, recorded before `create_nat_gateway`.
    #[serde(default)]
    pub(crate) nat_gateway_create_token: Option<String>,
    /// Token of the latest `create_internet_gateway` call, recorded before the call.
    #[serde(default)]
    pub(crate) internet_gateway_create_token: Option<String>,
    /// Token of the latest `allocate_address` call, recorded before the call.
    #[serde(default)]
    pub(crate) eip_create_token: Option<String>,
    /// Polls of the current delete step while AWS reports its object in use. Reset when the
    /// step moves on, and by a manual retry.
    #[serde(default)]
    pub(crate) wait_for_delete_dependencies_iterations: u32,
}

/// A `create_subnet` call that may have created a subnet whose ID is not recorded yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubnetCreateAttempt {
    pub(crate) token: String,
    pub(crate) cidr: String,
    /// `Public` or `Private`.
    pub(crate) subnet_type: String,
}

impl AwsNetworkController {
    fn map_subnets_to_failure_domains(
        subnets: Vec<Subnet>,
        public_subnet_ids: &[String],
        private_subnet_ids: &[String],
        resource_id: &str,
    ) -> Result<BTreeMap<String, AwsFailureDomainSubnets>> {
        let expected: HashSet<&str> = public_subnet_ids
            .iter()
            .chain(private_subnet_ids)
            .map(String::as_str)
            .collect();
        let mut mapped_ids = HashSet::new();
        let mut by_domain = BTreeMap::<String, AwsFailureDomainSubnets>::new();

        for subnet in subnets {
            let Some(subnet_id) = subnet.subnet_id else {
                continue;
            };
            if !expected.contains(subnet_id.as_str()) {
                continue;
            }
            let domain = subnet.availability_zone.ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Subnet '{subnet_id}' has no availability zone"),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;
            let entry = by_domain.entry(domain).or_default();
            if public_subnet_ids.contains(&subnet_id) {
                entry.public_subnet_ids.push(subnet_id.clone());
            }
            if private_subnet_ids.contains(&subnet_id) {
                entry.private_subnet_ids.push(subnet_id.clone());
            }
            mapped_ids.insert(subnet_id);
        }

        let mut missing: Vec<&str> = expected
            .into_iter()
            .filter(|subnet_id| !mapped_ids.contains(*subnet_id))
            .collect();
        missing.sort_unstable();
        if !missing.is_empty() {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Could not resolve failure domains for subnets: {}",
                    missing.join(", ")
                ),
                resource_id: Some(resource_id.to_string()),
            }));
        }
        for subnets in by_domain.values_mut() {
            subnets.public_subnet_ids.sort();
            subnets.private_subnet_ids.sort();
        }
        Ok(by_domain)
    }

    pub fn private_subnets_ready_for_runtime(&self) -> bool {
        self.is_byo_vpc || matches!(self.state, AwsNetworkState::Ready)
    }

    async fn find_security_group_id_by_name(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vpc_id: &str,
        group_name: &str,
        resource_id: &str,
    ) -> Result<Option<String>> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let response = client
            .describe_security_groups(
                DescribeSecurityGroupsRequest::builder()
                    .filters(vec![
                        Filter {
                            name: "vpc-id".to_string(),
                            values: vec![vpc_id.to_string()],
                        },
                        Filter {
                            name: "group-name".to_string(),
                            values: vec![group_name.to_string()],
                        },
                    ])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe existing security group".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;

        Ok(response
            .security_group_info
            .and_then(|set| set.items.into_iter().find_map(|sg| sg.group_id)))
    }

    /// Look up an existing route table in this VPC by its `Name` tag.
    ///
    /// Returns `Ok(None)` when no matching table exists. The result carries
    /// the full `RouteTable` so callers can inspect `association_set` and
    /// avoid re-associating subnets already attached.
    ///
    /// Used by `creating_route_tables` to make a retried state-machine
    /// iteration reuse the table from a prior partial attempt instead of
    /// leaking a fresh one. The reference pattern is `create_route`'s
    /// `RemoteResourceConflict` short-circuit; that one can't apply to
    /// `CreateRouteTable` because AWS doesn't surface a conflict when a
    /// tagged duplicate already exists.
    async fn find_existing_route_table_by_name(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vpc_id: &str,
        name: &str,
        resource_id: &str,
    ) -> Result<Option<RouteTable>> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let response = client
            .describe_route_tables(
                DescribeRouteTablesRequest::builder()
                    .filters(vec![
                        Filter {
                            name: "vpc-id".to_string(),
                            values: vec![vpc_id.to_string()],
                        },
                        Filter {
                            name: "tag:Name".to_string(),
                            values: vec![name.to_string()],
                        },
                    ])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe existing route tables".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;

        Ok(response
            .route_table_set
            .and_then(|set| set.items.into_iter().next()))
    }

    async fn find_security_group_by_id(
        &self,
        ctx: &ResourceControllerContext<'_>,
        group_id: &str,
        resource_id: &str,
    ) -> Result<SecurityGroup> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let response = client
            .describe_security_groups(
                DescribeSecurityGroupsRequest::builder()
                    .group_ids(vec![group_id.to_string()])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe security group".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;

        response
            .security_group_info
            .and_then(|set| set.items.into_iter().next())
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Security group '{group_id}' was not found"),
                    resource_id: Some(resource_id.to_string()),
                })
            })
    }

    /// Find the VPC the `create_vpc` call with this attempt token made: one with this
    /// resource's ownership tags, the token tag and `cidr`.
    ///
    /// More than one match is an error, because one call creates at most one VPC.
    async fn find_vpc_by_create_attempt(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        token: &str,
        cidr: &str,
    ) -> Result<Option<String>> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let resource_prefix = ctx.resource_prefix;

        let response = client
            .describe_vpcs(
                DescribeVpcsRequest::builder()
                    .filters(create_attempt_filters(resource_prefix, resource_id, token))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to look up the VPC of an earlier create attempt".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;

        let vpc_ids: Vec<String> = response
            .vpc_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter(|vpc| {
                vpc.cidr_block.as_deref() == Some(cidr)
                    && has_create_attempt_tags(
                        vpc.tag_set.as_ref(),
                        resource_prefix,
                        resource_id,
                        token,
                    )
            })
            .map(|vpc| {
                vpc.vpc_id.ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: format!("VPC of create attempt {token} has no ID"),
                        resource_id: Some(resource_id.to_string()),
                    })
                })
            })
            .collect::<Result<_>>()?;

        match vpc_ids.as_slice() {
            [] => Ok(None),
            [vpc_id] => Ok(Some(vpc_id.clone())),
            _ => Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Found {} VPCs for create attempt {token} ({}); delete the extra VPCs and retry",
                    vpc_ids.len(),
                    vpc_ids.join(", ")
                ),
                resource_id: Some(resource_id.to_string()),
            })),
        }
    }

    /// Find the subnet the `create_subnet` call of `attempt` made in `vpc_id`.
    async fn find_subnet_by_create_attempt(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vpc_id: &str,
        attempt: &SubnetCreateAttempt,
        resource_id: &str,
    ) -> Result<Option<String>> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let mut filters = create_attempt_filters(ctx.resource_prefix, resource_id, &attempt.token);
        filters.push(Filter {
            name: "vpc-id".to_string(),
            values: vec![vpc_id.to_string()],
        });
        filters.push(Filter {
            name: "cidr-block".to_string(),
            values: vec![attempt.cidr.clone()],
        });

        let subnets = client
            .describe_subnets(DescribeSubnetsRequest::builder().filters(filters).build())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to look up subnet {} of an earlier create attempt",
                    attempt.cidr
                ),
                resource_id: Some(resource_id.to_string()),
            })?
            .subnet_set
            .map(|set| set.items)
            .unwrap_or_default();

        // A CIDR is unique within a VPC, so at most one subnet can match.
        Ok(subnets
            .into_iter()
            .filter(|subnet| {
                subnet.cidr_block.as_deref() == Some(attempt.cidr.as_str())
                    && has_create_attempt_tags(
                        subnet.tag_set.as_ref(),
                        ctx.resource_prefix,
                        resource_id,
                        &attempt.token,
                    )
            })
            .filter_map(|subnet| subnet.subnet_id)
            .collect::<Vec<_>>())
        .and_then(|ids| single_create_attempt_match(ids, "subnet", &attempt.token, resource_id))
    }

    /// Find the internet gateway the `create_internet_gateway` call with this token made.
    async fn find_internet_gateway_by_create_attempt(
        &self,
        ctx: &ResourceControllerContext<'_>,
        token: &str,
        resource_id: &str,
    ) -> Result<Option<String>> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let gateways = client
            .describe_internet_gateways(
                DescribeInternetGatewaysRequest::builder()
                    .filters(create_attempt_filters(
                        ctx.resource_prefix,
                        resource_id,
                        token,
                    ))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to look up the Internet Gateway of an earlier create attempt"
                    .to_string(),
                resource_id: Some(resource_id.to_string()),
            })?
            .internet_gateway_set
            .map(|set| set.items)
            .unwrap_or_default();

        Ok(gateways
            .into_iter()
            .filter(|gateway| {
                has_create_attempt_tags(
                    gateway.tag_set.as_ref(),
                    ctx.resource_prefix,
                    resource_id,
                    token,
                )
            })
            .filter_map(|gateway| gateway.internet_gateway_id)
            .collect::<Vec<_>>())
        .and_then(|ids| single_create_attempt_match(ids, "Internet Gateway", token, resource_id))
    }

    /// Find the Elastic IP the `allocate_address` call with this token allocated.
    ///
    /// DescribeAddresses here takes no filters, so the token is matched on the returned tags.
    async fn find_elastic_ip_by_create_attempt(
        &self,
        ctx: &ResourceControllerContext<'_>,
        token: &str,
        resource_id: &str,
    ) -> Result<Option<String>> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let addresses = client
            .describe_addresses()
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to look up the Elastic IP of an earlier allocation".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?
            .addresses_set
            .map(|set| set.items)
            .unwrap_or_default();

        Ok(addresses
            .into_iter()
            .filter(|address| {
                has_create_attempt_tags(
                    address.tag_set.as_ref(),
                    ctx.resource_prefix,
                    resource_id,
                    token,
                )
            })
            .filter_map(|address| address.allocation_id)
            .collect::<Vec<_>>())
        .and_then(|ids| single_create_attempt_match(ids, "Elastic IP", token, resource_id))
    }

    /// Deletes the detached network interfaces `filter` matches and describes those that still
    /// hold the object a delete step is waiting on.
    ///
    /// An `available` interface is attached to nothing and sits in this network, which is
    /// being deleted, so nothing can use it again; Lambda leaves such interfaces behind when
    /// its cleanup does not run. Interfaces in use are left alone and reported.
    async fn release_detached_network_interfaces(
        &self,
        ctx: &ResourceControllerContext<'_>,
        filter: Filter,
        resource_id: &str,
    ) -> Result<DeleteBlockers> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let mut blockers = DeleteBlockers::default();

        let interfaces = match client
            .describe_network_interfaces(
                DescribeNetworkInterfacesRequest::builder()
                    .filters(vec![filter])
                    .build(),
            )
            .await
        {
            Ok(response) => response
                .network_interface_set
                .map(|set| set.items)
                .unwrap_or_default(),
            // Access denied must not leave this handler: it would end the whole delete. The
            // interfaces may still drain on their own, so the step keeps waiting.
            Err(error) if is_access_denied(&error) => {
                blockers.items.push(
                    "network interfaces that could not be listed (ec2:DescribeNetworkInterfaces is not granted)"
                        .to_string(),
                );
                return Ok(blockers);
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to list the network interfaces holding a network object"
                        .to_string(),
                    resource_id: Some(resource_id.to_string()),
                }))
            }
        };

        for interface in interfaces {
            let Some(interface_id) = interface.network_interface_id.clone() else {
                continue;
            };
            let detail = describe_network_interface(&interface);
            if interface.status.as_deref() != Some("available") {
                blockers.items.push(detail);
                continue;
            }
            match client.delete_network_interface(&interface_id).await {
                Ok(()) => {
                    info!(network_interface_id = %interface_id, description = ?interface.description, "Deleted a detached network interface left in the network");
                }
                Err(error) if is_not_found(&error) => {}
                Err(error) if is_conflict(&error) => blockers.items.push(detail),
                Err(error) if is_access_denied(&error) => {
                    blockers.missing_permission = true;
                    blockers.items.push(format!(
                        "{detail}; it is detached but this role cannot delete it (ec2:DeleteNetworkInterface is not granted): rerun setup to grant it, or delete it"
                    ));
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete detached network interface '{interface_id}'"
                        ),
                        resource_id: Some(resource_id.to_string()),
                    }))
                }
            }
        }
        Ok(blockers)
    }

    /// The security groups other than the default one that keep `vpc_id` from being deleted.
    async fn vpc_security_group_blockers(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vpc_id: &str,
        resource_id: &str,
    ) -> Result<Vec<String>> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let groups = match client
            .describe_security_groups(
                DescribeSecurityGroupsRequest::builder()
                    .filters(vec![Filter {
                        name: "vpc-id".to_string(),
                        values: vec![vpc_id.to_string()],
                    }])
                    .build(),
            )
            .await
        {
            Ok(response) => response
                .security_group_info
                .map(|set| set.items)
                .unwrap_or_default(),
            Err(error) if is_access_denied(&error) => return Ok(Vec::new()),
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to list the security groups of VPC '{vpc_id}'"),
                    resource_id: Some(resource_id.to_string()),
                }))
            }
        };
        Ok(groups
            .into_iter()
            .filter(|group| group.group_name.as_deref() != Some("default"))
            .map(|group| {
                format!(
                    "security group {} ('{}')",
                    group.group_id.unwrap_or_default(),
                    group.group_name.unwrap_or_default()
                )
            })
            .collect())
    }

    /// Counts one more poll of a delete step whose object is still in use. Fails with what
    /// holds it once `max_polls` is reached, or at once when only a missing permission keeps
    /// the step from making progress.
    fn wait_for_delete_dependencies(
        &mut self,
        resource_id: &str,
        object: String,
        blockers: DeleteBlockers,
        max_polls: u32,
    ) -> Result<()> {
        self.wait_for_delete_dependencies_iterations += 1;
        if blockers.missing_permission || self.wait_for_delete_dependencies_iterations >= max_polls
        {
            let listed = if blockers.items.is_empty() {
                "dependencies AWS does not list here (for example VPC endpoints or other services' interfaces)".to_string()
            } else {
                blockers.items.join("; ")
            };
            return Err(AlienError::new(ErrorData::ResourceDeleteBlocked {
                resource_id: resource_id.to_string(),
                object,
                blockers: listed,
            }));
        }
        Ok(())
    }

    /// Whether the Elastic IP with this allocation ID is still associated with a network
    /// interface. An address that no longer exists is not associated.
    async fn elastic_ip_associated(
        &self,
        ctx: &ResourceControllerContext<'_>,
        allocation_id: &str,
        resource_id: &str,
    ) -> Result<bool> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let addresses = client
            .describe_addresses()
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to describe Elastic IP '{allocation_id}'"),
                resource_id: Some(resource_id.to_string()),
            })?
            .addresses_set
            .map(|set| set.items)
            .unwrap_or_default();

        Ok(addresses.iter().any(|address| {
            address.allocation_id.as_deref() == Some(allocation_id)
                && (address.association_id.is_some() || address.network_interface_id.is_some())
        }))
    }

    /// Find the NAT gateway tagged with this create-attempt token that is not yet deleted.
    async fn find_nat_gateway_by_create_attempt(
        &self,
        ctx: &ResourceControllerContext<'_>,
        token: &str,
        resource_id: &str,
    ) -> Result<Option<String>> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let nat_gateways = client
            .describe_nat_gateways(
                DescribeNatGatewaysRequest::builder()
                    .filters(create_attempt_filters(
                        ctx.resource_prefix,
                        resource_id,
                        token,
                    ))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to look up the NAT Gateway of an earlier create attempt"
                    .to_string(),
                resource_id: Some(resource_id.to_string()),
            })?
            .nat_gateway_set
            .map(|set| set.items)
            .unwrap_or_default();

        Ok(nat_gateways
            .into_iter()
            .filter(|nat_gateway| {
                nat_gateway.state.as_deref() != Some("deleted")
                    && has_create_attempt_tags(
                        nat_gateway.tag_set.as_ref(),
                        ctx.resource_prefix,
                        resource_id,
                        token,
                    )
            })
            .find_map(|nat_gateway| nat_gateway.nat_gateway_id))
    }

    /// Whether the internet gateway is attached (or attaching) to `vpc_id`.
    async fn internet_gateway_attached_to(
        &self,
        ctx: &ResourceControllerContext<'_>,
        igw_id: &str,
        vpc_id: &str,
        resource_id: &str,
    ) -> Result<bool> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let response = client
            .describe_internet_gateways(
                DescribeInternetGatewaysRequest::builder()
                    .internet_gateway_ids(vec![igw_id.to_string()])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to describe Internet Gateway '{igw_id}'"),
                resource_id: Some(resource_id.to_string()),
            })?;

        Ok(response
            .internet_gateway_set
            .map(|set| set.items)
            .unwrap_or_default()
            .iter()
            .filter(|igw| igw.internet_gateway_id.as_deref() == Some(igw_id))
            .flat_map(|igw| igw.attachment_set.iter().flat_map(|set| set.items.iter()))
            .any(|attachment| {
                attachment.vpc_id.as_deref() == Some(vpc_id)
                    && !matches!(
                        attachment.state.as_deref(),
                        Some("detaching") | Some("detached")
                    )
            }))
    }

    /// Return the subnet with `cidr` in this network's VPC, creating it if it does not
    /// exist. Subnet CIDRs are derived from the VPC CIDR, so a subnet with that CIDR in our
    /// VPC is the one an earlier attempt created; it must also carry our ownership tags.
    async fn ensure_subnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        subnet: SubnetSpec<'_>,
    ) -> Result<String> {
        let SubnetSpec {
            vpc_id,
            cidr,
            availability_zone,
            name,
            subnet_type,
            resource_id,
        } = subnet;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        // A recorded attempt for this CIDR means `create_subnet` was called and its response
        // may have been lost. Only the subnet carrying that attempt's token is ours.
        if let Some(attempt) = self
            .subnet_create_attempt
            .clone()
            .filter(|attempt| attempt.cidr == cidr)
        {
            if let Some(subnet_id) = self
                .find_subnet_by_create_attempt(ctx, vpc_id, &attempt, resource_id)
                .await?
            {
                info!(subnet_id = %subnet_id, cidr = %cidr, "Found the subnet created by an earlier attempt");
                self.subnet_create_attempt = None;
                return Ok(subnet_id);
            }
        }

        // Subnets are created one at a time and an attempt is cleared only once its ID is
        // recorded, so an attempt for this CIDR is this subnet's: its token is reused.
        let attempt = match self.subnet_create_attempt.clone() {
            Some(attempt) if attempt.cidr == cidr => attempt,
            _ => SubnetCreateAttempt {
                token: new_create_attempt_token(),
                cidr: cidr.to_string(),
                subnet_type: subnet_type.to_string(),
            },
        };
        self.subnet_create_attempt = Some(attempt.clone());
        let token = attempt.token.clone();

        let created = client
            .create_subnet(
                CreateSubnetRequest::builder()
                    .vpc_id(vpc_id.to_string())
                    .cidr_block(cidr.to_string())
                    .availability_zone(availability_zone.to_string())
                    .tag_specifications(vec![self.create_tag_specification(
                        ctx.resource_prefix,
                        resource_id,
                        "subnet",
                        name,
                        [
                            ("Type".to_string(), subnet_type.to_string()),
                            create_attempt_tag(&token),
                        ],
                    )])
                    .build(),
            )
            .await;
        let response = match created {
            Ok(response) => response,
            // The CIDR is taken. If the earlier lookup missed our own subnet (eventual
            // consistency), it is visible by now; anything else is a real conflict.
            Err(error) if is_conflict(&error) => {
                if let Some(subnet_id) = self
                    .find_subnet_by_create_attempt(ctx, vpc_id, &attempt, resource_id)
                    .await?
                {
                    info!(subnet_id = %subnet_id, cidr = %cidr, "Found the subnet an earlier attempt created");
                    self.subnet_create_attempt = None;
                    return Ok(subnet_id);
                }
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Subnet {cidr} conflicts with an existing subnet"),
                    resource_id: Some(resource_id.to_string()),
                }));
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create {} subnet in {availability_zone}",
                        subnet_type.to_lowercase()
                    ),
                    resource_id: Some(resource_id.to_string()),
                }));
            }
        };

        let subnet_id = response.subnet.and_then(|s| s.subnet_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Subnet {cidr} created but no subnet ID returned"),
                resource_id: Some(resource_id.to_string()),
            })
        })?;
        // The caller records the ID before its next call.
        self.subnet_create_attempt = None;
        Ok(subnet_id)
    }

    /// Record the objects that create calls made but whose responses were lost: the VPC, an
    /// in-flight subnet and the NAT gateway. Each is looked up by its create-attempt token;
    /// nothing found means that call created nothing.
    async fn recover_lost_creates(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<()> {
        if self.vpc_id.is_none() {
            if let (Some(token), Some(cidr)) =
                (self.vpc_create_token.clone(), self.cidr_block.clone())
            {
                if let Some(vpc_id) = self
                    .find_vpc_by_create_attempt(ctx, resource_id, &token, &cidr)
                    .await?
                {
                    info!(vpc_id = %vpc_id, "Recovered the VPC of a create whose response was lost");
                    self.vpc_id = Some(vpc_id);
                }
            }
        }

        if let (Some(attempt), Some(vpc_id)) =
            (self.subnet_create_attempt.clone(), self.vpc_id.clone())
        {
            if let Some(subnet_id) = self
                .find_subnet_by_create_attempt(ctx, &vpc_id, &attempt, resource_id)
                .await?
            {
                info!(subnet_id = %subnet_id, "Recovered the subnet of a create whose response was lost");
                let recorded = if attempt.subnet_type == "Public" {
                    &mut self.public_subnet_ids
                } else {
                    &mut self.private_subnet_ids
                };
                if !recorded.contains(&subnet_id) {
                    recorded.push(subnet_id);
                }
            }
            self.subnet_create_attempt = None;
        }

        if self.internet_gateway_id.is_none() {
            if let Some(token) = self.internet_gateway_create_token.clone() {
                if let Some(igw_id) = self
                    .find_internet_gateway_by_create_attempt(ctx, &token, resource_id)
                    .await?
                {
                    info!(igw_id = %igw_id, "Recovered the Internet Gateway of a create whose response was lost");
                    self.internet_gateway_id = Some(igw_id);
                }
            }
        }

        if self.eip_allocation_id.is_none() {
            if let Some(token) = self.eip_create_token.clone() {
                if let Some(allocation_id) = self
                    .find_elastic_ip_by_create_attempt(ctx, &token, resource_id)
                    .await?
                {
                    info!(allocation_id = %allocation_id, "Recovered the Elastic IP of an allocation whose response was lost");
                    self.eip_allocation_id = Some(allocation_id);
                }
            }
        }

        // Route tables and the security group carry fixed names unique within our VPC, so the
        // create handlers already rediscover them by name; delete does the same here.
        if let Some(vpc_id) = self.vpc_id.clone() {
            for (name, recorded) in [
                (
                    format!("{}-public-rt", ctx.resource_prefix),
                    self.public_route_table_id.is_some(),
                ),
                (
                    format!("{}-private-rt", ctx.resource_prefix),
                    self.private_route_table_id.is_some(),
                ),
            ] {
                if recorded {
                    continue;
                }
                let route_table_id = self
                    .find_existing_route_table_by_name(ctx, &vpc_id, &name, resource_id)
                    .await?
                    .and_then(|route_table| route_table.route_table_id);
                if let Some(route_table_id) = route_table_id {
                    info!(rt_id = %route_table_id, name = %name, "Recovered a route table whose ID was not recorded");
                    if name.ends_with("-public-rt") {
                        self.public_route_table_id = Some(route_table_id);
                    } else {
                        self.private_route_table_id = Some(route_table_id);
                    }
                }
            }

            if self.security_group_id.is_none() {
                let group_name = format!("{}-sg", ctx.resource_prefix);
                if let Some(sg_id) = self
                    .find_security_group_id_by_name(ctx, &vpc_id, &group_name, resource_id)
                    .await?
                {
                    info!(sg_id = %sg_id, "Recovered a security group whose ID was not recorded");
                    self.security_group_id = Some(sg_id);
                }
            }
        }

        if self.nat_gateway_id.is_none() {
            if let Some(token) = self.nat_gateway_create_token.clone() {
                if let Some(nat_gateway_id) = self
                    .find_nat_gateway_by_create_attempt(ctx, &token, resource_id)
                    .await?
                {
                    info!(nat_gateway_id = %nat_gateway_id, "Recovered the NAT Gateway of a create whose response was lost");
                    self.nat_gateway_id = Some(nat_gateway_id);
                }
            }
        }

        Ok(())
    }

    fn forget_subnet(&mut self, subnet_id: &str) {
        self.public_subnet_ids.retain(|id| id != subnet_id);
        self.private_subnet_ids.retain(|id| id != subnet_id);
        for subnets in self.subnets_by_failure_domain.values_mut() {
            subnets.public_subnet_ids.retain(|id| id != subnet_id);
            subnets.private_subnet_ids.retain(|id| id != subnet_id);
        }
        self.subnets_by_failure_domain.retain(|_, subnets| {
            !subnets.public_subnet_ids.is_empty() || !subnets.private_subnet_ids.is_empty()
        });
    }

    /// Find an available CIDR block for the VPC.
    ///
    /// This method queries existing VPCs and finds a non-overlapping CIDR block.
    /// Priority order:
    /// 1. 100.64.0.0/10 range (RFC 6598 - rarely conflicts with enterprise networks)
    /// 2. 172.16.0.0/12 range
    /// 3. 10.0.0.0/8 range (commonly used, last resort)
    async fn find_available_cidr(
        &self,
        ctx: &ResourceControllerContext<'_>,
        stack_id: &str,
    ) -> Result<String> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        // Get all existing VPC CIDRs in the account
        let existing_vpcs = client
            .describe_vpcs(DescribeVpcsRequest::builder().build())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe existing VPCs for CIDR allocation".to_string(),
                resource_id: Some(stack_id.to_string()),
            })?;

        let used_cidrs: HashSet<String> = existing_vpcs
            .vpc_set
            .map(|set| set.items)
            .unwrap_or_default()
            .iter()
            .filter_map(|vpc| vpc.cidr_block.clone())
            .collect();

        // Start with hash-based offset for determinism
        let stack_hash = stack_id.bytes().fold(0u8, |acc, b| acc.wrapping_add(b)) % 64;

        // Primary: 100.64.0.0/10 range (RFC 6598 - rarely used in enterprise)
        // 64 possible /16 ranges: 100.64.0.0 - 100.127.0.0
        for attempt in 0..64 {
            let octet = 64 + ((stack_hash + attempt) % 64);
            let candidate = format!("100.{}.0.0/16", octet);

            if !self.cidr_overlaps_any(&candidate, &used_cidrs) {
                info!(cidr = %candidate, "Found available CIDR in RFC 6598 range");
                return Ok(candidate);
            }
        }

        // Fallback: 172.16.0.0/12 range (16 possible /16)
        for octet in 16..32 {
            let candidate = format!("172.{}.0.0/16", octet);
            if !self.cidr_overlaps_any(&candidate, &used_cidrs) {
                info!(cidr = %candidate, "Found available CIDR in 172.16.0.0/12 range");
                return Ok(candidate);
            }
        }

        // Last resort: 10.x (commonly used, but 256 options)
        for octet in 0..=255u16 {
            let candidate = format!("10.{}.0.0/16", octet);
            if !self.cidr_overlaps_any(&candidate, &used_cidrs) {
                info!(cidr = %candidate, "Found available CIDR in 10.0.0.0/8 range");
                return Ok(candidate);
            }
        }

        Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "No available CIDR block found. All 336 possible /16 ranges are in use."
                .to_string(),
            resource_id: Some(stack_id.to_string()),
        }))
    }

    /// Check if a CIDR block overlaps with any existing CIDR blocks.
    ///
    /// This is a simplified check that only handles /16 blocks.
    fn cidr_overlaps_any(&self, candidate: &str, used_cidrs: &HashSet<String>) -> bool {
        // Simple overlap check for /16 blocks
        // Extract the network portion (first two octets for /16)
        let candidate_prefix = candidate.split('/').next().unwrap_or("");

        for used_cidr in used_cidrs {
            let used_prefix = used_cidr.split('/').next().unwrap_or("");

            // For simplicity, check if the prefixes are the same
            // A more robust implementation would do proper CIDR math
            if candidate_prefix == used_prefix {
                return true;
            }

            // Also check for overlapping ranges (simplified)
            // This could be enhanced with proper CIDR overlap detection
        }

        false
    }

    /// Calculate subnet CIDRs based on VPC CIDR and availability zones.
    ///
    /// For a /16 VPC CIDR, creates /20 subnets to allow for growth.
    fn calculate_subnet_cidrs(
        &self,
        vpc_cidr: &str,
        az_count: usize,
        include_public: bool,
    ) -> (Vec<String>, Vec<String>) {
        // Extract the first two octets from the VPC CIDR (assuming /16)
        let parts: Vec<&str> = vpc_cidr.split('.').collect();
        let octet1 = parts.get(0).unwrap_or(&"10");
        let octet2 = parts.get(1).unwrap_or(&"0");

        let mut public_cidrs = Vec::new();
        let mut private_cidrs = Vec::new();

        // Create /20 subnets (4096 IPs each)
        // Public subnets: 0-16, 16-32, 32-48 (third octet in multiples of 16)
        // Private subnets: 128-144, 144-160, 160-176
        for i in 0..az_count {
            if include_public {
                let public_third_octet = i * 16;
                public_cidrs.push(format!("{}.{}.{}.0/20", octet1, octet2, public_third_octet));
            }

            let private_third_octet = 128 + (i * 16);
            private_cidrs.push(format!(
                "{}.{}.{}.0/20",
                octet1, octet2, private_third_octet
            ));
        }

        (public_cidrs, private_cidrs)
    }

    fn create_tag_specification(
        &self,
        resource_prefix: &str,
        resource_id: &str,
        resource_type: &str,
        name: impl Into<String>,
        extra_tags: impl IntoIterator<Item = (String, String)>,
    ) -> TagSpecification {
        let mut tags = vec![Tag {
            key: "Name".to_string(),
            value: name.into(),
        }];

        tags.extend(
            standard_resource_tags(resource_prefix, resource_id)
                .into_iter()
                .chain(extra_tags)
                .map(|(key, value)| Tag { key, value }),
        );

        TagSpecification {
            resource_type: resource_type.to_string(),
            tags,
        }
    }

    /// Create tags for AWS resources.
    fn create_tags(
        &self,
        resource_prefix: &str,
        resource_id: &str,
        resource_type: &str,
    ) -> Vec<TagSpecification> {
        vec![TagSpecification {
            resource_type: resource_type.to_string(),
            tags: self
                .create_tag_specification(
                    resource_prefix,
                    resource_id,
                    resource_type,
                    format!("{}-{}", resource_prefix, resource_type.to_lowercase()),
                    [],
                )
                .tags,
        }]
    }
}

#[cfg(test)]
mod failure_domain_compatibility_tests {
    use super::*;

    fn subnet(id: &str, availability_zone: Option<&str>) -> Subnet {
        Subnet {
            subnet_id: Some(id.to_string()),
            vpc_id: None,
            state: None,
            cidr_block: None,
            availability_zone: availability_zone.map(str::to_string),
            availability_zone_id: None,
            available_ip_address_count: None,
            default_for_az: None,
            map_public_ip_on_launch: None,
            tag_set: None,
        }
    }

    #[test]
    fn maps_public_and_private_subnets_to_their_real_failure_domains() {
        let mapped = AwsNetworkController::map_subnets_to_failure_domains(
            vec![
                subnet("subnet-private-b", Some("us-east-1b")),
                subnet("subnet-public-a", Some("us-east-1a")),
                subnet("subnet-private-a", Some("us-east-1a")),
            ],
            &["subnet-public-a".to_string()],
            &[
                "subnet-private-a".to_string(),
                "subnet-private-b".to_string(),
            ],
            "network",
        )
        .expect("all requested subnets have an availability zone");

        assert_eq!(
            mapped["us-east-1a"],
            AwsFailureDomainSubnets {
                public_subnet_ids: vec!["subnet-public-a".to_string()],
                private_subnet_ids: vec!["subnet-private-a".to_string()],
            }
        );
        assert_eq!(
            mapped["us-east-1b"],
            AwsFailureDomainSubnets {
                public_subnet_ids: Vec::new(),
                private_subnet_ids: vec!["subnet-private-b".to_string()],
            }
        );
    }

    #[test]
    fn rejects_incomplete_subnet_discovery_before_using_the_network() {
        let error = AwsNetworkController::map_subnets_to_failure_domains(
            vec![subnet("subnet-present", Some("us-east-1a"))],
            &["subnet-present".to_string()],
            &["subnet-missing".to_string()],
            "network",
        )
        .expect_err("an unresolved requested subnet must fail");

        assert!(error.to_string().contains("subnet-missing"));
    }

    #[test]
    fn rejects_subnets_without_a_failure_domain() {
        let error = AwsNetworkController::map_subnets_to_failure_domains(
            vec![subnet("subnet-without-zone", None)],
            &["subnet-without-zone".to_string()],
            &[],
            "network",
        )
        .expect_err("a subnet without an availability zone must fail");

        assert!(error.to_string().contains("has no availability zone"));
    }

    #[test]
    fn existing_ready_state_remains_aggregate_and_ready() {
        let controller = AwsNetworkController::mock_ready("vpc-legacy", 2);
        let mut value = serde_json::to_value(controller).expect("controller should serialize");
        value
            .as_object_mut()
            .expect("controller state should be an object")
            .remove("subnetsByFailureDomain");

        let restored: AwsNetworkController =
            serde_json::from_value(value).expect("legacy controller should deserialize");
        assert_eq!(restored.state, AwsNetworkState::Ready);
        assert!(restored.subnets_by_failure_domain.is_empty());
    }
}

#[controller]
impl AwsNetworkController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;

        info!(network_id = %config.id, "Starting network provisioning");

        match &config.settings {
            NetworkSettings::UseDefault => {
                // Discover the account's default VPC — no provisioning needed
                let aws_config = ctx.get_aws_config()?;
                let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_config).await?;

                info!("Discovering AWS default VPC");

                let vpcs_response = ec2_client
                    .describe_vpcs(
                        DescribeVpcsRequest::builder()
                            .filters(vec![Filter::builder()
                                .name("is-default".to_string())
                                .values(vec!["true".to_string()])
                                .build()])
                            .build(),
                    )
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: "Failed to discover default VPC".to_string(),
                        operation: Some("discover_default_vpc".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                let default_vpc = vpcs_response
                    .vpc_set
                    .and_then(|set| set.items.into_iter().next())
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::InfrastructureError {
                            message: "Default VPC not found. It may have been deleted. \
                                      Use NetworkSettings::Create to create an isolated VPC, \
                                      or ByoVpcAws to reference an existing one."
                                .to_string(),
                            operation: Some("discover_default_vpc".to_string()),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;

                let vpc_id = default_vpc.vpc_id.ok_or_else(|| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: "Default VPC has no ID".to_string(),
                        operation: Some("discover_default_vpc".to_string()),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

                // List subnets in the default VPC
                let subnets_response = ec2_client
                    .describe_subnets(
                        DescribeSubnetsRequest::builder()
                            .filters(vec![Filter::builder()
                                .name("vpc-id".to_string())
                                .values(vec![vpc_id.clone()])
                                .build()])
                            .build(),
                    )
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: "Failed to list subnets in default VPC".to_string(),
                        operation: Some("discover_default_subnets".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                let discovered_subnets = subnets_response
                    .subnet_set
                    .map(|set| set.items)
                    .unwrap_or_default();
                let subnet_ids: Vec<String> = discovered_subnets
                    .iter()
                    .filter_map(|subnet| subnet.subnet_id.clone())
                    .collect();
                let subnets_by_failure_domain = Self::map_subnets_to_failure_domains(
                    discovered_subnets,
                    &subnet_ids,
                    &[],
                    &config.id,
                )?;

                info!(
                    vpc_id = %vpc_id,
                    subnet_count = subnet_ids.len(),
                    "Using AWS default VPC"
                );

                // Default VPC subnets are all public (have auto-assign public IP)
                self.vpc_id = Some(vpc_id);
                self.public_subnet_ids = subnet_ids;
                self.private_subnet_ids = Vec::new();
                self.is_byo_vpc = true;
                self.availability_zones = subnets_by_failure_domain.keys().cloned().collect();
                self.subnets_by_failure_domain = subnets_by_failure_domain;

                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            NetworkSettings::Create { .. } => {
                // A managed AWS network needs one EIP for its NAT gateway. Check before
                // creating the VPC so a known quota exhaustion cannot leave partial resources.
                preflight_aws_eip_quota(ctx, &config.id).await?;
                Ok(HandlerAction::Continue {
                    state: CreatingVpc,
                    suggested_delay: None,
                })
            }
            NetworkSettings::ByoVpcAws {
                vpc_id,
                public_subnet_ids,
                private_subnet_ids,
                security_group_ids,
            } => {
                let aws_config = ctx.get_aws_config()?;
                let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_config).await?;
                let requested_subnet_ids: Vec<String> = public_subnet_ids
                    .iter()
                    .chain(private_subnet_ids)
                    .cloned()
                    .collect();
                let response = ec2_client
                    .describe_subnets(
                        DescribeSubnetsRequest::builder()
                            .subnet_ids(requested_subnet_ids)
                            .build(),
                    )
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: "Failed to resolve existing subnet failure domains".to_string(),
                        operation: Some("discover_existing_subnet_domains".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;
                let subnets_by_failure_domain = Self::map_subnets_to_failure_domains(
                    response.subnet_set.map(|set| set.items).unwrap_or_default(),
                    public_subnet_ids,
                    private_subnet_ids,
                    &config.id,
                )?;
                // BYO-VPC mode: store the provided IDs and transition to Ready
                info!(
                    vpc_id = %vpc_id,
                    public_subnets = ?public_subnet_ids,
                    private_subnets = ?private_subnet_ids,
                    "Using existing VPC infrastructure"
                );

                self.vpc_id = Some(vpc_id.clone());
                self.public_subnet_ids = public_subnet_ids.clone();
                self.private_subnet_ids = private_subnet_ids.clone();
                self.security_group_id = security_group_ids.first().cloned();
                self.is_byo_vpc = true;

                self.availability_zones = subnets_by_failure_domain.keys().cloned().collect();
                self.subnets_by_failure_domain = subnets_by_failure_domain;

                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            _ => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Invalid network settings for AWS platform".to_string(),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    #[handler(
        state = DiscoveringSubnetFailureDomains,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn discovering_subnet_failure_domains(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let aws_config = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_config).await?;
        let requested_subnet_ids = self
            .public_subnet_ids
            .iter()
            .chain(&self.private_subnet_ids)
            .cloned()
            .collect();
        let response = ec2_client
            .describe_subnets(
                DescribeSubnetsRequest::builder()
                    .subnet_ids(requested_subnet_ids)
                    .build(),
            )
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to resolve imported subnet failure domains".to_string(),
                operation: Some("discover_imported_subnet_domains".to_string()),
                resource_id: Some(config.id.clone()),
            })?;
        self.subnets_by_failure_domain = Self::map_subnets_to_failure_domains(
            response.subnet_set.map(|set| set.items).unwrap_or_default(),
            &self.public_subnet_ids,
            &self.private_subnet_ids,
            &config.id,
        )?;
        self.availability_zones = self.subnets_by_failure_domain.keys().cloned().collect();
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingVpc,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_vpc(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let (requested_cidr, availability_zones) = match &config.settings {
            NetworkSettings::Create {
                cidr,
                availability_zones,
            } => (cidr.clone(), *availability_zones),
            _ => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Expected Create settings in CreatingVpc state".to_string(),
                    resource_id: Some(config.id.clone()),
                }))
            }
        };

        // Reads first: everything this handler learns before `create_vpc` is recorded, so a
        // retry never repeats the lookups with different answers.
        if self.availability_zones.is_empty() {
            let az_response = client
                .describe_availability_zones(DescribeAvailabilityZonesRequest::builder().build())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to describe availability zones".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            let zones: Vec<String> = az_response
                .availability_zone_info
                .map(|set| set.items)
                .unwrap_or_default()
                .into_iter()
                .take(availability_zones as usize)
                .filter_map(|az| az.zone_name)
                .collect();

            if zones.is_empty() {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "No availability zones found in region".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
            self.availability_zones = zones;
        }

        if let Some(vpc_id) = &self.vpc_id {
            info!(vpc_id = %vpc_id, "VPC already recorded, configuring DNS");
            return Ok(HandlerAction::Continue {
                state: ConfiguringVpcDns,
                suggested_delay: None,
            });
        }

        // A recorded attempt token without a VPC ID means `create_vpc` was called and its
        // response may have been lost. Only the VPC carrying that token (and the attempted
        // CIDR) is ours; a VPC merely tagged for this network, such as one left by an earlier
        // instance of the resource, is never adopted. When the token finds nothing, the create
        // is repeated under the same token. State persisted before tokens existed has none, so
        // it creates a new VPC.
        if let (Some(token), Some(attempted_cidr)) =
            (self.vpc_create_token.clone(), self.cidr_block.clone())
        {
            if let Some(vpc_id) = self
                .find_vpc_by_create_attempt(ctx, &config.id, &token, &attempted_cidr)
                .await?
            {
                info!(vpc_id = %vpc_id, cidr = %attempted_cidr, "Found the VPC created by an earlier attempt");
                self.vpc_id = Some(vpc_id);
                return Ok(HandlerAction::Continue {
                    state: ConfiguringVpcDns,
                    suggested_delay: None,
                });
            }
        }

        let vpc_cidr = match &self.cidr_block {
            Some(cidr) => cidr.clone(),
            None => {
                let cidr = match requested_cidr {
                    Some(cidr) => cidr,
                    None => self.find_available_cidr(ctx, &config.id).await?,
                };
                self.cidr_block = Some(cidr.clone());
                cidr
            }
        };

        let token = self
            .vpc_create_token
            .get_or_insert_with(new_create_attempt_token)
            .clone();
        info!(cidr = %vpc_cidr, "Creating VPC");

        let create_response = client
            .create_vpc(
                CreateVpcRequest::builder()
                    .cidr_block(vpc_cidr.clone())
                    .tag_specifications(vec![self.create_tag_specification(
                        ctx.resource_prefix,
                        &config.id,
                        "vpc",
                        format!("{}-vpc", ctx.resource_prefix),
                        [create_attempt_tag(&token)],
                    )])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create VPC".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let vpc_id = create_response.vpc.and_then(|v| v.vpc_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "VPC created but no VPC ID returned".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(vpc_id = %vpc_id, azs = ?self.availability_zones, "VPC created");
        self.vpc_id = Some(vpc_id);

        Ok(HandlerAction::Continue {
            state: ConfiguringVpcDns,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ConfiguringVpcDns,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_vpc_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let vpc_id = self.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Setting an attribute to the value it already has succeeds, so a retry may repeat
        // both calls.
        client
            .modify_vpc_attribute(
                ModifyVpcAttributeRequest::builder()
                    .vpc_id(vpc_id.clone())
                    .enable_dns_support(true)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to enable DNS support on VPC".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        client
            .modify_vpc_attribute(
                ModifyVpcAttributeRequest::builder()
                    .vpc_id(vpc_id.clone())
                    .enable_dns_hostnames(true)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to enable DNS hostnames on VPC".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(vpc_id = %vpc_id, "VPC DNS configured, proceeding to create Internet Gateway");

        Ok(HandlerAction::Continue {
            state: CreatingInternetGateway,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingInternetGateway,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_internet_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        if let Some(igw_id) = &self.internet_gateway_id {
            info!(igw_id = %igw_id, "Internet Gateway already recorded");
            return Ok(HandlerAction::Continue {
                state: AttachingInternetGateway,
                suggested_delay: None,
            });
        }

        // A recorded token without an ID: the create may have succeeded and its response
        // been lost. Only the gateway carrying that token is ours.
        if let Some(token) = self.internet_gateway_create_token.clone() {
            if let Some(igw_id) = self
                .find_internet_gateway_by_create_attempt(ctx, &token, &config.id)
                .await?
            {
                info!(igw_id = %igw_id, "Found the Internet Gateway created by an earlier attempt");
                self.internet_gateway_id = Some(igw_id);
                return Ok(HandlerAction::Continue {
                    state: AttachingInternetGateway,
                    suggested_delay: None,
                });
            }
        }

        let token = self
            .internet_gateway_create_token
            .get_or_insert_with(new_create_attempt_token)
            .clone();
        info!("Creating Internet Gateway");

        let igw_response = client
            .create_internet_gateway(
                CreateInternetGatewayRequest::builder()
                    .tag_specifications(vec![self.create_tag_specification(
                        ctx.resource_prefix,
                        &config.id,
                        "internet-gateway",
                        format!("{}-internet-gateway", ctx.resource_prefix),
                        [create_attempt_tag(&token)],
                    )])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Internet Gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let igw_id = igw_response
            .internet_gateway
            .and_then(|igw| igw.internet_gateway_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Internet Gateway created but no ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        info!(igw_id = %igw_id, "Internet Gateway created");
        self.internet_gateway_id = Some(igw_id);

        Ok(HandlerAction::Continue {
            state: AttachingInternetGateway,
            suggested_delay: None,
        })
    }

    #[handler(
        state = AttachingInternetGateway,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn attaching_internet_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let vpc_id = self.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let igw_id = self.internet_gateway_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Internet Gateway ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        match client
            .attach_internet_gateway(
                AttachInternetGatewayRequest::builder()
                    .internet_gateway_id(igw_id.clone())
                    .vpc_id(vpc_id.clone())
                    .build(),
            )
            .await
        {
            Ok(()) => {
                info!(igw_id = %igw_id, vpc_id = %vpc_id, "Internet Gateway attached");
            }
            // An earlier attempt may have attached it before its response was lost. That is
            // only success when the gateway is attached to this network's VPC.
            Err(error) if is_conflict(&error) => {
                if !self
                    .internet_gateway_attached_to(ctx, igw_id, vpc_id, &config.id)
                    .await?
                {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Internet Gateway '{igw_id}' could not be attached to VPC '{vpc_id}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                info!(igw_id = %igw_id, vpc_id = %vpc_id, "Internet Gateway was already attached");
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to attach Internet Gateway to VPC".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: CreatingSubnets,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingSubnets,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_subnets(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;

        let vpc_id = self.vpc_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let cidr_block = self.cidr_block.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "CIDR block not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // TODO: Determine if public subnets are needed based on stack resources
        // For now, always create public subnets
        let include_public = true;

        let availability_zones = self.availability_zones.clone();
        let (public_cidrs, private_cidrs) =
            self.calculate_subnet_cidrs(&cidr_block, availability_zones.len(), include_public);

        info!(
            public_cidrs = ?public_cidrs,
            private_cidrs = ?private_cidrs,
            "Creating subnets"
        );

        // Each subnet ID is recorded before the next call, and subnets are created in a
        // fixed order, so index `i` already in state means that subnet exists.
        for (i, (cidr, az)) in public_cidrs.iter().zip(&availability_zones).enumerate() {
            if i < self.public_subnet_ids.len() {
                continue;
            }
            let subnet_id = self
                .ensure_subnet(
                    ctx,
                    SubnetSpec::builder()
                        .vpc_id(&vpc_id)
                        .cidr(cidr)
                        .availability_zone(az)
                        .name(format!("{}-public-{}", ctx.resource_prefix, i + 1))
                        .subnet_type("Public")
                        .resource_id(&config.id)
                        .build(),
                )
                .await?;
            self.public_subnet_ids.push(subnet_id.clone());
            self.subnets_by_failure_domain
                .entry(az.clone())
                .or_default()
                .public_subnet_ids
                .push(subnet_id);
        }

        for (i, (cidr, az)) in private_cidrs.iter().zip(&availability_zones).enumerate() {
            if i < self.private_subnet_ids.len() {
                continue;
            }
            let subnet_id = self
                .ensure_subnet(
                    ctx,
                    SubnetSpec::builder()
                        .vpc_id(&vpc_id)
                        .cidr(cidr)
                        .availability_zone(az)
                        .name(format!("{}-private-{}", ctx.resource_prefix, i + 1))
                        .subnet_type("Private")
                        .resource_id(&config.id)
                        .build(),
                )
                .await?;
            self.private_subnet_ids.push(subnet_id.clone());
            self.subnets_by_failure_domain
                .entry(az.clone())
                .or_default()
                .private_subnet_ids
                .push(subnet_id);
        }

        info!(
            public_subnets = ?self.public_subnet_ids,
            private_subnets = ?self.private_subnet_ids,
            "Subnets created, proceeding to create route tables"
        );

        Ok(HandlerAction::Continue {
            state: CreatingRouteTables,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingRouteTables,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_route_tables(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let vpc_id = self.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let igw_id = self.internet_gateway_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Internet Gateway ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let public_rt_name = format!("{}-public-rt", ctx.resource_prefix);
        let private_rt_name = format!("{}-private-rt", ctx.resource_prefix);

        // Look up an existing public route table by tag:Name first. AWS
        // CreateRouteTable doesn't enforce tag uniqueness, so without this
        // lookup every retried iteration of the state machine creates a fresh
        // RT, leaking orphans in the VPC. Mirrors the `create_route`
        // idempotency pattern in `waiting_for_nat_gateway`.
        let existing_public = self
            .find_existing_route_table_by_name(ctx, vpc_id, &public_rt_name, &config.id)
            .await?;

        let (public_rt_id, public_existing_assocs) = match existing_public {
            Some(rt) => {
                let id = rt.route_table_id.ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: "Existing public route table has no ID".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
                let assocs = rt.association_set.map(|set| set.items).unwrap_or_default();
                info!(
                    route_table_id = %id,
                    "Public route table already exists from a prior attempt — reusing"
                );
                (id, assocs)
            }
            None => {
                info!("Creating public route table");
                let public_rt_response = client
                    .create_route_table(
                        CreateRouteTableRequest::builder()
                            .vpc_id(vpc_id.clone())
                            .tag_specifications(vec![self.create_tag_specification(
                                ctx.resource_prefix,
                                &config.id,
                                "route-table",
                                public_rt_name.clone(),
                                [],
                            )])
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to create public route table".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;
                let id = public_rt_response
                    .route_table
                    .and_then(|rt| rt.route_table_id)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: "Public route table created but no ID returned".to_string(),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
                (id, Vec::new())
            }
        };

        // Add route to Internet Gateway. Idempotent: when reusing an existing
        // RT the route may already be there.
        match client
            .create_route(
                CreateRouteRequest::builder()
                    .route_table_id(public_rt_id.clone())
                    .destination_cidr_block("0.0.0.0/0".to_string())
                    .gateway_id(igw_id.clone())
                    .build(),
            )
            .await
        {
            Ok(_) => {}
            Err(err)
                if matches!(
                    &err.error,
                    Some(CloudClientErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                info!(
                    route_table_id = %public_rt_id,
                    "0.0.0.0/0 → IGW route already exists from a prior attempt — reusing"
                );
            }
            Err(err) => {
                return Err(err).context(ErrorData::CloudPlatformError {
                    message: "Failed to create route to Internet Gateway".to_string(),
                    resource_id: Some(config.id.clone()),
                });
            }
        }

        // Associate only public subnets that aren't already attached. AWS
        // returns the existing association_id on a duplicate associate call,
        // but skipping the call keeps the log clean and avoids needless API
        // traffic per retry.
        for subnet_id in
            subnets_needing_association(&self.public_subnet_ids, &public_existing_assocs)
        {
            let assoc_response = client
                .associate_route_table(
                    AssociateRouteTableRequest::builder()
                        .route_table_id(public_rt_id.clone())
                        .subnet_id(subnet_id.clone())
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to associate subnet {} with public route table",
                        subnet_id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(assoc_id) = assoc_response.association_id {
                if !self.route_table_association_ids.contains(&assoc_id) {
                    self.route_table_association_ids.push(assoc_id);
                }
            }
        }

        // Preserve any association IDs the existing RT was already carrying,
        // so cleanup on destroy can disassociate them too.
        for assoc in &public_existing_assocs {
            if let Some(assoc_id) = assoc.route_table_association_id.clone() {
                if !self.route_table_association_ids.contains(&assoc_id) {
                    self.route_table_association_ids.push(assoc_id);
                }
            }
        }

        self.public_route_table_id = Some(public_rt_id);

        // Same lookup-or-create dance for the private route table.
        let existing_private = self
            .find_existing_route_table_by_name(ctx, vpc_id, &private_rt_name, &config.id)
            .await?;

        let (private_rt_id, private_existing_assocs) = match existing_private {
            Some(rt) => {
                let id = rt.route_table_id.ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: "Existing private route table has no ID".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
                let assocs = rt.association_set.map(|set| set.items).unwrap_or_default();
                info!(
                    route_table_id = %id,
                    "Private route table already exists from a prior attempt — reusing"
                );
                (id, assocs)
            }
            None => {
                info!("Creating private route table");
                let private_rt_response = client
                    .create_route_table(
                        CreateRouteTableRequest::builder()
                            .vpc_id(vpc_id.clone())
                            .tag_specifications(vec![self.create_tag_specification(
                                ctx.resource_prefix,
                                &config.id,
                                "route-table",
                                private_rt_name.clone(),
                                [],
                            )])
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to create private route table".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;
                let id = private_rt_response
                    .route_table
                    .and_then(|rt| rt.route_table_id)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: "Private route table created but no ID returned".to_string(),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
                (id, Vec::new())
            }
        };

        // Associate only private subnets that aren't already attached.
        for subnet_id in
            subnets_needing_association(&self.private_subnet_ids, &private_existing_assocs)
        {
            let assoc_response = client
                .associate_route_table(
                    AssociateRouteTableRequest::builder()
                        .route_table_id(private_rt_id.clone())
                        .subnet_id(subnet_id.clone())
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to associate subnet {} with private route table",
                        subnet_id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(assoc_id) = assoc_response.association_id {
                if !self.route_table_association_ids.contains(&assoc_id) {
                    self.route_table_association_ids.push(assoc_id);
                }
            }
        }

        for assoc in &private_existing_assocs {
            if let Some(assoc_id) = assoc.route_table_association_id.clone() {
                if !self.route_table_association_ids.contains(&assoc_id) {
                    self.route_table_association_ids.push(assoc_id);
                }
            }
        }

        self.private_route_table_id = Some(private_rt_id);

        info!("Route tables created, creating NAT gateway");

        // Create always provisions a NAT gateway for private subnet egress
        Ok(HandlerAction::Continue {
            state: AllocatingElasticIp,
            suggested_delay: None,
        })
    }

    #[handler(
        state = AllocatingElasticIp,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn allocating_elastic_ip(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        if let Some(allocation_id) = &self.eip_allocation_id {
            info!(allocation_id = %allocation_id, "Elastic IP already allocated");
            return Ok(HandlerAction::Continue {
                state: CreatingNatGateway,
                suggested_delay: None,
            });
        }

        // A recorded token without an allocation ID: the allocation may have succeeded and its
        // response been lost. Only the address carrying that token is ours.
        if let Some(token) = self.eip_create_token.clone() {
            if let Some(allocation_id) = self
                .find_elastic_ip_by_create_attempt(ctx, &token, &config.id)
                .await?
            {
                info!(allocation_id = %allocation_id, "Found the Elastic IP allocated by an earlier attempt");
                self.eip_allocation_id = Some(allocation_id);
                return Ok(HandlerAction::Continue {
                    state: CreatingNatGateway,
                    suggested_delay: None,
                });
            }
        }

        let token = self
            .eip_create_token
            .get_or_insert_with(new_create_attempt_token)
            .clone();
        info!("Allocating Elastic IP for NAT Gateway");

        let eip_response = client
            .allocate_address(
                AllocateAddressRequest::builder()
                    .domain("vpc".to_string())
                    .tag_specifications(vec![self.create_tag_specification(
                        ctx.resource_prefix,
                        &config.id,
                        "elastic-ip",
                        format!("{}-elastic-ip", ctx.resource_prefix),
                        [create_attempt_tag(&token)],
                    )])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to allocate Elastic IP".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let allocation_id = eip_response.allocation_id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Elastic IP allocated but no allocation ID returned".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(allocation_id = %allocation_id, "Elastic IP allocated");
        self.eip_allocation_id = Some(allocation_id);

        Ok(HandlerAction::Continue {
            state: CreatingNatGateway,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingNatGateway,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_nat_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        if let Some(nat_gateway_id) = &self.nat_gateway_id {
            info!(nat_gateway_id = %nat_gateway_id, "NAT Gateway already recorded");
            return Ok(HandlerAction::Continue {
                state: WaitingForNatGateway,
                suggested_delay: None,
            });
        }

        let Some(allocation_id) = self.eip_allocation_id.clone() else {
            return Ok(HandlerAction::Continue {
                state: AllocatingElasticIp,
                suggested_delay: None,
            });
        };

        let public_subnet_id = self.public_subnet_ids.first().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "No public subnet available for NAT Gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(allocation_id = %allocation_id, "Creating NAT Gateway");

        // An Elastic IP backs at most one NAT gateway, so a client token derived from it is
        // unique to this gateway. When a retry repeats the call after a lost response, AWS
        // returns the gateway the first call created, so the attempt token (which lets delete
        // find a gateway whose ID was never recorded) stays the same across retries too.
        let attempt_token = self
            .nat_gateway_create_token
            .get_or_insert_with(new_create_attempt_token)
            .clone();
        let nat_response = client
            .create_nat_gateway(
                CreateNatGatewayRequest::builder()
                    .subnet_id(public_subnet_id.clone())
                    .allocation_id(allocation_id.clone())
                    .connectivity_type("public".to_string())
                    .tag_specifications(vec![self.create_tag_specification(
                        ctx.resource_prefix,
                        &config.id,
                        "natgateway",
                        format!("{}-natgateway", ctx.resource_prefix),
                        [create_attempt_tag(&attempt_token)],
                    )])
                    .client_token(nat_gateway_client_token(&allocation_id))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create NAT Gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let nat_gateway_id = nat_response
            .nat_gateway
            .and_then(|ng| ng.nat_gateway_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "NAT Gateway created but no ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.nat_gateway_id = Some(nat_gateway_id.clone());

        info!(nat_gateway_id = %nat_gateway_id, "NAT Gateway created, waiting for it to become available");

        Ok(HandlerAction::Continue {
            state: WaitingForNatGateway,
            suggested_delay: Some(Duration::from_secs(15)),
        })
    }

    #[handler(
        state = WaitingForNatGateway,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_nat_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let nat_gateway_id = self.nat_gateway_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "NAT Gateway ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Check NAT Gateway status
        let nat_response = client
            .describe_nat_gateways(
                DescribeNatGatewaysRequest::builder()
                    .nat_gateway_ids(vec![nat_gateway_id.clone()])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe NAT Gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let nat_gateway = nat_response
            .nat_gateway_set
            .and_then(|set| set.items.into_iter().next())
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "NAT Gateway not found".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        match nat_gateway.state.as_deref() {
            Some("available") => {
                info!(nat_gateway_id = %nat_gateway_id, "NAT Gateway is available, adding route");

                // Add route to NAT Gateway in private route table
                let private_rt_id = self.private_route_table_id.as_ref().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Private route table ID not set".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

                // Idempotent: a retried iteration of the state machine may have
                // already added the 0.0.0.0/0 route in a prior partial attempt;
                // AWS surfaces that as `RemoteResourceConflict` /
                // `RouteAlreadyExists`. Treat as success so the state machine
                // can advance.
                match client
                    .create_route(
                        CreateRouteRequest::builder()
                            .route_table_id(private_rt_id.clone())
                            .destination_cidr_block("0.0.0.0/0".to_string())
                            .nat_gateway_id(nat_gateway_id.clone())
                            .build(),
                    )
                    .await
                {
                    Ok(_) => {}
                    Err(err)
                        if matches!(
                            &err.error,
                            Some(CloudClientErrorData::RemoteResourceConflict { .. })
                        ) =>
                    {
                        info!(
                            route_table_id = %private_rt_id,
                            nat_gateway_id = %nat_gateway_id,
                            "0.0.0.0/0 → NAT route already exists from a prior attempt — reusing"
                        );
                    }
                    Err(err) => {
                        return Err(err).context(ErrorData::CloudPlatformError {
                            message: "Failed to create route to NAT Gateway".to_string(),
                            resource_id: Some(config.id.clone()),
                        });
                    }
                }

                Ok(HandlerAction::Continue {
                    state: CreatingSecurityGroup,
                    suggested_delay: None,
                })
            }
            Some("pending") => {
                debug!(nat_gateway_id = %nat_gateway_id, "NAT Gateway still pending");
                Ok(HandlerAction::Continue {
                    state: WaitingForNatGateway,
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
            // Terminal for this gateway, so retrying the wait cannot help. The ID stays in
            // state so delete cleans the gateway and its Elastic IP up.
            Some(state @ ("failed" | "deleted")) => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "NAT Gateway '{nat_gateway_id}' is {state}: {}",
                        nat_gateway_failure_reason(&nat_gateway)
                    ),
                    resource_id: Some(config.id.clone()),
                }))
            }
            state => {
                debug!(nat_gateway_id = %nat_gateway_id, state = ?state, "NAT Gateway in unknown state");
                Ok(HandlerAction::Continue {
                    state: WaitingForNatGateway,
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
        }
    }

    #[handler(
        state = CreatingSecurityGroup,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_security_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let vpc_id = self.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!("Creating security group");

        let group_name = format!("{}-sg", ctx.resource_prefix);
        if let Some(sg_id) = &self.security_group_id {
            info!(sg_id = %sg_id, "Security group already recorded");
            return Ok(HandlerAction::Continue {
                state: AuthorizingSecurityGroupIngress,
                suggested_delay: None,
            });
        }

        if let Some(sg_id) = self
            .find_security_group_id_by_name(ctx, vpc_id, &group_name, &config.id)
            .await?
        {
            info!(
                sg_id = %sg_id,
                group_name = %group_name,
                "Security group already exists"
            );
            self.security_group_id = Some(sg_id);
            return Ok(HandlerAction::Continue {
                state: AuthorizingSecurityGroupIngress,
                suggested_delay: None,
            });
        }

        // Create security group
        let sg_result = client
            .create_security_group(
                CreateSecurityGroupRequest::builder()
                    .group_name(group_name.clone())
                    .description(
                        "Alien managed security group for VPC internal communication".to_string(),
                    )
                    .vpc_id(vpc_id.clone())
                    .tag_specifications(self.create_tags(
                        ctx.resource_prefix,
                        &config.id,
                        "security-group",
                    ))
                    .build(),
            )
            .await;

        let sg_id = match sg_result {
            Ok(sg_response) => sg_response.group_id.ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Security group created but no ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?,
            Err(error) if is_security_group_duplicate(&error) => {
                let sg_id = self
                    .find_security_group_id_by_name(ctx, vpc_id, &group_name, &config.id)
                    .await?
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "Security group '{}' already exists but could not be resolved",
                                group_name
                            ),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;

                info!(
                    sg_id = %sg_id,
                    group_name = %group_name,
                    "Security group already exists; continuing create flow"
                );
                sg_id
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to create security group".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        info!(sg_id = %sg_id, "Security group created");
        self.security_group_id = Some(sg_id);

        Ok(HandlerAction::Continue {
            state: AuthorizingSecurityGroupIngress,
            suggested_delay: None,
        })
    }

    #[handler(
        state = AuthorizingSecurityGroupIngress,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn authorizing_security_group_ingress(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let sg_id = self.security_group_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Security group ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Add ingress rule: allow all traffic from within the VPC
        let cidr_block = self.cidr_block.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "CIDR block not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let security_group = self
            .find_security_group_by_id(ctx, sg_id, &config.id)
            .await?;
        if has_ipv4_all_protocol_rule(
            security_group
                .ip_permissions
                .as_ref()
                .map(|permissions| permissions.items.as_slice()),
            cidr_block,
        ) {
            debug!(sg_id = %sg_id, cidr_block = %cidr_block, "Security group ingress rule already exists");
            return Ok(HandlerAction::Continue {
                state: AuthorizingSecurityGroupEgress,
                suggested_delay: None,
            });
        }

        if let Err(e) = client
            .authorize_security_group_ingress(
                AuthorizeSecurityGroupIngressRequest::builder()
                    .group_id(sg_id.clone())
                    .ip_permissions(vec![IpPermission {
                        ip_protocol: "-1".to_string(), // All protocols
                        from_port: None,
                        to_port: None,
                        ip_ranges: Some(vec![IpRange {
                            cidr_ip: cidr_block.clone(),
                            description: Some("Allow all traffic from VPC".to_string()),
                        }]),
                        ipv6_ranges: None,
                        user_id_group_pairs: None,
                    }])
                    .build(),
            )
            .await
        {
            if is_security_group_rule_duplicate(&e) {
                debug!("Ingress rule already exists (duplicate), skipping");
            } else {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to add ingress rule to security group".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: AuthorizingSecurityGroupEgress,
            suggested_delay: None,
        })
    }

    #[handler(
        state = AuthorizingSecurityGroupEgress,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn authorizing_security_group_egress(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let sg_id = self.security_group_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Security group ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Add egress rule: allow all outbound traffic (default rule, but we make it explicit).
        // Only ignore duplicate-rule errors; propagate everything else.
        let security_group = self
            .find_security_group_by_id(ctx, sg_id, &config.id)
            .await?;
        if has_ipv4_all_protocol_rule(
            security_group
                .ip_permissions_egress
                .as_ref()
                .map(|permissions| permissions.items.as_slice()),
            "0.0.0.0/0",
        ) {
            debug!(sg_id = %sg_id, "Security group egress rule already exists");
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        if let Err(e) = client
            .authorize_security_group_egress(
                AuthorizeSecurityGroupEgressRequest::builder()
                    .group_id(sg_id.clone())
                    .ip_permissions(vec![IpPermission {
                        ip_protocol: "-1".to_string(),
                        from_port: None,
                        to_port: None,
                        ip_ranges: Some(vec![IpRange {
                            cidr_ip: "0.0.0.0/0".to_string(),
                            description: Some("Allow all outbound traffic".to_string()),
                        }]),
                        ipv6_ranges: None,
                        user_id_group_pairs: None,
                    }])
                    .build(),
            )
            .await
        {
            if is_security_group_rule_duplicate(&e) {
                debug!("Egress rule already exists (duplicate), skipping");
            } else {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to add egress rule to security group".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        info!("Security group configured, network provisioning complete");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;

        // For BYO-VPC, we don't need to verify - preflights already validated
        if self.is_byo_vpc {
            debug!(network_id = %config.id, "BYO-VPC network ready");
            emit_aws_network_heartbeat(ctx, &config.id, self, None);
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(60)),
            });
        }

        // For created VPCs, verify VPC still exists
        if let Some(vpc_id) = &self.vpc_id {
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

            let vpc_response = client
                .describe_vpcs(
                    DescribeVpcsRequest::builder()
                        .vpc_ids(vec![vpc_id.clone()])
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to verify VPC during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            let vpcs = vpc_response
                .vpc_set
                .map(|set| set.items)
                .unwrap_or_default();
            if vpcs.is_empty() {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "VPC no longer exists".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }

            let vpc_state = vpcs.first().and_then(|vpc| vpc.state.clone());
            debug!(vpc_id = %vpc_id, "VPC exists and is accessible");
            emit_aws_network_heartbeat(ctx, &config.id, self, vpc_state);
        } else {
            emit_aws_network_heartbeat(ctx, &config.id, self, None);
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;

        info!(network_id = %config.id, "Network update requested");

        // For BYO-VPC, update the stored VPC/subnet/SG references so downstream
        // resources (functions, containers) pick up the new values on their next update.
        if let NetworkSettings::ByoVpcAws {
            vpc_id,
            public_subnet_ids,
            private_subnet_ids,
            security_group_ids,
        } = &config.settings
        {
            let aws_config = ctx.get_aws_config()?;
            let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_config).await?;
            let requested_subnet_ids = public_subnet_ids
                .iter()
                .chain(private_subnet_ids)
                .cloned()
                .collect();
            let response = ec2_client
                .describe_subnets(
                    DescribeSubnetsRequest::builder()
                        .subnet_ids(requested_subnet_ids)
                        .build(),
                )
                .await
                .context(ErrorData::InfrastructureError {
                    message: "Failed to resolve updated subnet failure domains".to_string(),
                    operation: Some("discover_updated_subnet_domains".to_string()),
                    resource_id: Some(config.id.clone()),
                })?;
            let subnets_by_failure_domain = Self::map_subnets_to_failure_domains(
                response.subnet_set.map(|set| set.items).unwrap_or_default(),
                public_subnet_ids,
                private_subnet_ids,
                &config.id,
            )?;
            self.vpc_id = Some(vpc_id.clone());
            self.public_subnet_ids = public_subnet_ids.clone();
            self.private_subnet_ids = private_subnet_ids.clone();
            self.security_group_id = security_group_ids.first().cloned();
            self.availability_zones = subnets_by_failure_domain.keys().cloned().collect();
            self.subnets_by_failure_domain = subnets_by_failure_domain;

            info!(
                network_id = %config.id,
                vpc_id = %vpc_id,
                public_subnets = ?public_subnet_ids,
                private_subnets = ?private_subnet_ids,
                "Updated BYO-VPC references"
            );
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;

        let settings_is_setup_owned_vpc = matches!(
            &config.settings,
            NetworkSettings::UseDefault | NetworkSettings::ByoVpcAws { .. }
        );

        // For setup-owned VPCs, nothing to delete. The settings check protects
        // states imported before `is_byo_vpc` was derived from network mode.
        if self.is_byo_vpc || settings_is_setup_owned_vpc {
            self.is_byo_vpc = true;
            info!(network_id = %config.id, "BYO-VPC network - nothing to delete");
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        // A create whose response was lost left an object with no recorded ID. Find each one by
        // the exact token of its create call, so the steps below delete it.
        self.recover_lost_creates(ctx, &config.id).await?;

        // If no VPC was created, nothing to delete
        if self.vpc_id.is_none() {
            info!(network_id = %config.id, "No VPC created - nothing to delete");
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        info!(network_id = %config.id, "Starting network deletion");

        Ok(HandlerAction::Continue {
            state: DeletingNatGateway,
            suggested_delay: None,
        })
    }

    // Every delete handler below consumes NotFound itself: a NotFound returned from a
    // delete step ends the whole delete, which would skip the remaining children. An ID
    // leaves state only once AWS reports the object deleted or missing. While AWS reports
    // it in use, the ID stays and the handler polls, then fails with the ID still recorded.

    #[handler(
        state = DeletingNatGateway,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_nat_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let Some(nat_gateway_id) = self.nat_gateway_id.clone() else {
            return Ok(HandlerAction::Continue {
                state: ReleasingElasticIp,
                suggested_delay: None,
            });
        };

        info!(nat_gateway_id = %nat_gateway_id, "Deleting NAT Gateway");

        match client.delete_nat_gateway(&nat_gateway_id).await {
            Ok(_) => Ok(HandlerAction::Continue {
                state: WaitingForNatGatewayDeletion,
                suggested_delay: Some(Duration::from_secs(15)),
            }),
            Err(error) if is_not_found(&error) => {
                info!(nat_gateway_id = %nat_gateway_id, "NAT Gateway already deleted");
                self.nat_gateway_id = None;
                Ok(HandlerAction::Continue {
                    state: ReleasingElasticIp,
                    suggested_delay: None,
                })
            }
            Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete NAT Gateway '{nat_gateway_id}'"),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    #[handler(
        state = WaitingForNatGatewayDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_nat_gateway_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let Some(nat_gateway_id) = self.nat_gateway_id.clone() else {
            return Ok(HandlerAction::Continue {
                state: ReleasingElasticIp,
                suggested_delay: None,
            });
        };

        // The gateway holds its Elastic IP and its network interface in the public subnet
        // until it reaches `deleted`; releasing or deleting those earlier fails.
        let state = match client
            .describe_nat_gateways(
                DescribeNatGatewaysRequest::builder()
                    .nat_gateway_ids(vec![nat_gateway_id.clone()])
                    .build(),
            )
            .await
        {
            Ok(response) => response
                .nat_gateway_set
                .and_then(|set| set.items.into_iter().next())
                .and_then(|nat_gateway| nat_gateway.state),
            Err(error) if is_not_found(&error) => None,
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to describe NAT Gateway '{nat_gateway_id}'"),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        match state.as_deref() {
            None | Some("deleted") | Some("failed") => {
                info!(nat_gateway_id = %nat_gateway_id, "NAT Gateway deleted");
                self.nat_gateway_id = None;
                Ok(HandlerAction::Continue {
                    state: ReleasingElasticIp,
                    suggested_delay: None,
                })
            }
            Some(state) => {
                debug!(nat_gateway_id = %nat_gateway_id, state = %state, "Waiting for NAT Gateway deletion");
                Ok(HandlerAction::Stay {
                    max_times: Some(NAT_GATEWAY_DELETION_MAX_POLLS),
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
        }
    }

    #[handler(
        state = ReleasingElasticIp,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn releasing_elastic_ip(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let Some(allocation_id) = self.eip_allocation_id.clone() else {
            return Ok(HandlerAction::Continue {
                state: DeletingSecurityGroup,
                suggested_delay: None,
            });
        };

        info!(allocation_id = %allocation_id, "Releasing Elastic IP");

        match client.release_address(&allocation_id).await {
            Ok(()) => {
                info!(allocation_id = %allocation_id, "Elastic IP released");
            }
            Err(error) if is_not_found(&error) => {
                info!(allocation_id = %allocation_id, "Elastic IP already released");
            }
            Err(error) if is_conflict(&error) => {
                debug!(allocation_id = %allocation_id, "Elastic IP still in use");
                return Ok(HandlerAction::Stay {
                    max_times: Some(ELASTIC_IP_RELEASE_MAX_POLLS),
                    suggested_delay: Some(Duration::from_secs(15)),
                });
            }
            // EC2 answers `AuthFailure` (access denied), not `InvalidIPAddress.InUse`, while a
            // NAT gateway still holds the address. Access denied ends a delete as best-effort,
            // which would skip every later step, so it counts as a permission error only once
            // the address is no longer associated.
            Err(error) if is_access_denied(&error) => {
                if self
                    .elastic_ip_associated(ctx, &allocation_id, &config.id)
                    .await?
                {
                    debug!(allocation_id = %allocation_id, "Elastic IP still associated");
                    return Ok(HandlerAction::Stay {
                        max_times: Some(ELASTIC_IP_RELEASE_MAX_POLLS),
                        suggested_delay: Some(Duration::from_secs(15)),
                    });
                }
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Access denied releasing Elastic IP '{allocation_id}'"),
                    resource_id: Some(config.id.clone()),
                }));
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to release Elastic IP '{allocation_id}'"),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.eip_allocation_id = None;
        Ok(HandlerAction::Continue {
            state: DeletingSecurityGroup,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingSecurityGroup,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_security_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let Some(sg_id) = self.security_group_id.clone() else {
            return Ok(HandlerAction::Continue {
                state: DeletingSubnets,
                suggested_delay: None,
            });
        };

        info!(sg_id = %sg_id, "Deleting security group");

        match client.delete_security_group(&sg_id).await {
            Ok(()) => {
                info!(sg_id = %sg_id, "Security group deleted");
            }
            Err(error) if is_not_found(&error) => {
                info!(sg_id = %sg_id, "Security group already deleted");
            }
            // Network interfaces of dependents (Lambda, ECS, EC2) can stay in the group for
            // a long time after those dependents are deleted; detached ones are removed here.
            Err(error) if is_conflict(&error) => {
                debug!(sg_id = %sg_id, "Security group still in use");
                let blockers = self
                    .release_detached_network_interfaces(
                        ctx,
                        Filter {
                            name: "group-id".to_string(),
                            values: vec![sg_id.clone()],
                        },
                        &config.id,
                    )
                    .await?;
                self.wait_for_delete_dependencies(
                    &config.id,
                    format!("security group '{sg_id}'"),
                    blockers,
                    NETWORK_INTERFACE_DRAIN_MAX_POLLS,
                )?;
                return Ok(HandlerAction::Stay {
                    max_times: None,
                    suggested_delay: Some(Duration::from_secs(30)),
                });
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete security group '{sg_id}'"),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.security_group_id = None;
        self.wait_for_delete_dependencies_iterations = 0;
        Ok(HandlerAction::Continue {
            state: DeletingSubnets,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingSubnets,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_subnets(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let subnet_ids: Vec<String> = self
            .public_subnet_ids
            .iter()
            .chain(&self.private_subnet_ids)
            .cloned()
            .collect();
        let mut in_use = Vec::new();

        // Each deleted subnet leaves state immediately, so an error part-way keeps exactly
        // the subnets that still exist.
        for subnet_id in subnet_ids {
            info!(subnet_id = %subnet_id, "Deleting subnet");

            match client.delete_subnet(&subnet_id).await {
                Ok(()) => {
                    info!(subnet_id = %subnet_id, "Subnet deleted");
                }
                Err(error) if is_not_found(&error) => {
                    info!(subnet_id = %subnet_id, "Subnet already deleted");
                }
                Err(error) if is_conflict(&error) => {
                    debug!(subnet_id = %subnet_id, "Subnet still in use");
                    in_use.push(subnet_id);
                    continue;
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete subnet '{subnet_id}'"),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
            self.forget_subnet(&subnet_id);
        }

        if !in_use.is_empty() {
            let blockers = self
                .release_detached_network_interfaces(
                    ctx,
                    Filter {
                        name: "subnet-id".to_string(),
                        values: in_use.clone(),
                    },
                    &config.id,
                )
                .await?;
            self.wait_for_delete_dependencies(
                &config.id,
                format!("subnets {}", in_use.join(", ")),
                blockers,
                NETWORK_INTERFACE_DRAIN_MAX_POLLS,
            )?;
            return Ok(HandlerAction::Stay {
                max_times: None,
                suggested_delay: Some(Duration::from_secs(30)),
            });
        }

        self.wait_for_delete_dependencies_iterations = 0;
        Ok(HandlerAction::Continue {
            state: DeletingRouteTables,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRouteTables,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_route_tables(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        // Deleting a subnet removes its associations, so most of these are already gone.
        for assoc_id in self.route_table_association_ids.clone() {
            match client.disassociate_route_table(&assoc_id).await {
                Ok(()) => {
                    info!(assoc_id = %assoc_id, "Route table disassociated");
                }
                Err(error) if is_not_found(&error) => {
                    debug!(assoc_id = %assoc_id, "Route table association already removed");
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to disassociate route table '{assoc_id}'"),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
            self.route_table_association_ids
                .retain(|id| id != &assoc_id);
        }

        let mut in_use = false;
        for route_table_id in [
            self.public_route_table_id.clone(),
            self.private_route_table_id.clone(),
        ]
        .into_iter()
        .flatten()
        {
            info!(rt_id = %route_table_id, "Deleting route table");

            match client.delete_route_table(&route_table_id).await {
                Ok(()) => {
                    info!(rt_id = %route_table_id, "Route table deleted");
                }
                Err(error) if is_not_found(&error) => {
                    info!(rt_id = %route_table_id, "Route table already deleted");
                }
                Err(error) if is_conflict(&error) => {
                    debug!(rt_id = %route_table_id, "Route table still in use");
                    in_use = true;
                    continue;
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete route table '{route_table_id}'"),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
            if self.public_route_table_id.as_ref() == Some(&route_table_id) {
                self.public_route_table_id = None;
            }
            if self.private_route_table_id.as_ref() == Some(&route_table_id) {
                self.private_route_table_id = None;
            }
        }

        if in_use {
            return Ok(HandlerAction::Stay {
                max_times: Some(DEPENDENCY_DRAIN_MAX_POLLS),
                suggested_delay: Some(Duration::from_secs(15)),
            });
        }

        Ok(HandlerAction::Continue {
            state: DeletingInternetGateway,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingInternetGateway,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_internet_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let Some(igw_id) = self.internet_gateway_id.clone() else {
            return Ok(HandlerAction::Continue {
                state: DeletingVpc,
                suggested_delay: None,
            });
        };

        // Detach and delete in one handler: a repeated detach reports the gateway as not
        // attached, so a retry after either call is safe.
        if let Some(vpc_id) = self.vpc_id.clone() {
            info!(igw_id = %igw_id, vpc_id = %vpc_id, "Detaching Internet Gateway");

            match client
                .detach_internet_gateway(
                    DetachInternetGatewayRequest::builder()
                        .internet_gateway_id(igw_id.clone())
                        .vpc_id(vpc_id.clone())
                        .build(),
                )
                .await
            {
                Ok(()) => {
                    info!(igw_id = %igw_id, "Internet Gateway detached");
                }
                Err(error) if is_not_found(&error) || is_gateway_not_attached(&error) => {
                    debug!(igw_id = %igw_id, "Internet Gateway is not attached");
                }
                // AWS refuses the detach while the VPC still has mapped public addresses.
                Err(error) if is_conflict(&error) => {
                    debug!(igw_id = %igw_id, "Internet Gateway detach blocked by mapped public addresses");
                    return Ok(HandlerAction::Stay {
                        max_times: Some(DEPENDENCY_DRAIN_MAX_POLLS),
                        suggested_delay: Some(Duration::from_secs(15)),
                    });
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to detach Internet Gateway '{igw_id}'"),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }

        info!(igw_id = %igw_id, "Deleting Internet Gateway");

        match client.delete_internet_gateway(&igw_id).await {
            Ok(()) => {
                info!(igw_id = %igw_id, "Internet Gateway deleted");
            }
            Err(error) if is_not_found(&error) => {
                info!(igw_id = %igw_id, "Internet Gateway already deleted");
            }
            Err(error) if is_conflict(&error) => {
                debug!(igw_id = %igw_id, "Internet Gateway still in use");
                return Ok(HandlerAction::Stay {
                    max_times: Some(DEPENDENCY_DRAIN_MAX_POLLS),
                    suggested_delay: Some(Duration::from_secs(15)),
                });
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete Internet Gateway '{igw_id}'"),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.internet_gateway_id = None;
        Ok(HandlerAction::Continue {
            state: DeletingVpc,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingVpc,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_vpc(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let Some(vpc_id) = self.vpc_id.clone() else {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        };

        info!(vpc_id = %vpc_id, "Deleting VPC");

        match client.delete_vpc(&vpc_id).await {
            Ok(()) => {
                info!(vpc_id = %vpc_id, "VPC deleted");
            }
            Err(error) if is_not_found(&error) => {
                info!(vpc_id = %vpc_id, "VPC already deleted");
            }
            Err(error) if is_conflict(&error) => {
                debug!(vpc_id = %vpc_id, "VPC still has dependencies");
                let mut blockers = self
                    .release_detached_network_interfaces(
                        ctx,
                        Filter {
                            name: "vpc-id".to_string(),
                            values: vec![vpc_id.clone()],
                        },
                        &config.id,
                    )
                    .await?;
                blockers.items.extend(
                    self.vpc_security_group_blockers(ctx, &vpc_id, &config.id)
                        .await?,
                );
                self.wait_for_delete_dependencies(
                    &config.id,
                    format!("VPC '{vpc_id}'"),
                    blockers,
                    DEPENDENCY_DRAIN_MAX_POLLS,
                )?;
                return Ok(HandlerAction::Stay {
                    max_times: None,
                    suggested_delay: Some(Duration::from_secs(15)),
                });
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete VPC '{vpc_id}'"),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.vpc_id = None;
        self.wait_for_delete_dependencies_iterations = 0;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        // Only return outputs when VPC has been created or BYO-VPC is configured
        self.vpc_id.as_ref().map(|vpc_id| {
            ResourceOutputs::new(NetworkOutputs {
                network_id: vpc_id.clone(),
                availability_zones: self.availability_zones.len() as u8,
                has_public_subnets: !self.public_subnet_ids.is_empty(),
                has_nat_gateway: self.nat_gateway_id.is_some(),
                cidr: self.cidr_block.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // Network doesn't have bindings - other resources access it via require_dependency
        Ok(None)
    }
}

impl AwsNetworkController {
    /// Creates a controller in ready state with mock values for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(vpc_id: &str, az_count: usize) -> Self {
        Self {
            state: AwsNetworkState::Ready,
            vpc_id: Some(vpc_id.to_string()),
            cidr_block: Some("10.0.0.0/16".to_string()),
            internet_gateway_id: Some(format!("igw-{}", vpc_id)),
            nat_gateway_id: Some(format!("nat-{}", vpc_id)),
            eip_allocation_id: Some(format!("eipalloc-{}", vpc_id)),
            public_subnet_ids: (0..az_count)
                .map(|i| format!("subnet-pub-{}-{}", vpc_id, i))
                .collect(),
            private_subnet_ids: (0..az_count)
                .map(|i| format!("subnet-priv-{}-{}", vpc_id, i))
                .collect(),
            public_route_table_id: Some(format!("rtb-pub-{}", vpc_id)),
            private_route_table_id: Some(format!("rtb-priv-{}", vpc_id)),
            route_table_association_ids: vec![],
            security_group_id: Some(format!("sg-{}", vpc_id)),
            availability_zones: (0..az_count)
                .map(|i| format!("us-east-1{}", (b'a' + i as u8) as char))
                .collect(),
            subnets_by_failure_domain: BTreeMap::new(),
            is_byo_vpc: false,
            vpc_create_token: None,
            subnet_create_attempt: None,
            nat_gateway_create_token: None,
            internet_gateway_create_token: None,
            eip_create_token: None,
            wait_for_delete_dependencies_iterations: 0,
            _internal_stay_count: None,
        }
    }

    #[cfg(feature = "test-utils")]
    pub fn mock_waiting_for_private_subnet_egress(vpc_id: &str, az_count: usize) -> Self {
        let mut controller = Self::mock_ready(vpc_id, az_count);
        controller.state = AwsNetworkState::WaitingForNatGateway;
        controller.nat_gateway_id = None;
        controller
    }

    /// Creates a BYO-VPC controller in ready state for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_byo_vpc_ready(
        vpc_id: &str,
        public_subnet_ids: Vec<String>,
        private_subnet_ids: Vec<String>,
        security_group_id: Option<String>,
    ) -> Self {
        Self {
            state: AwsNetworkState::Ready,
            vpc_id: Some(vpc_id.to_string()),
            cidr_block: None,
            internet_gateway_id: None,
            nat_gateway_id: None,
            eip_allocation_id: None,
            public_subnet_ids,
            private_subnet_ids: private_subnet_ids.clone(),
            public_route_table_id: None,
            private_route_table_id: None,
            route_table_association_ids: vec![],
            security_group_id,
            availability_zones: (0..private_subnet_ids.len())
                .map(|i| format!("az-{}", i))
                .collect(),
            subnets_by_failure_domain: BTreeMap::new(),
            is_byo_vpc: true,
            vpc_create_token: None,
            subnet_create_attempt: None,
            nat_gateway_create_token: None,
            internet_gateway_create_token: None,
            eip_create_token: None,
            wait_for_delete_dependencies_iterations: 0,
            _internal_stay_count: None,
        }
    }
}

/// Retry, rediscovery and delete-ordering behavior of the managed (Create) network. Each
/// test drives the real handlers through `SingleControllerExecutor` with a mocked EC2 API.
/// `SingleControllerExecutor::step` keeps what a failed handler wrote to `self`, like the
/// real executor, so a failed step followed by another step is a retry.
#[cfg(test)]
mod controller_state_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use alien_aws_clients::ec2::{
        CreateNatGatewayRequest, DescribeNetworkInterfacesResponse, MockEc2Api,
    };
    use alien_aws_clients::service_quotas::{
        GetServiceQuotaResponse, MockServiceQuotasApi, ServiceQuota,
    };
    use alien_core::{Network, Platform};
    use serde::de::DeserializeOwned;
    use serde_json::json;

    use super::*;
    use crate::core::{
        controller_test::{assert_polling_delays, SingleControllerExecutor},
        MockPlatformServiceProvider, ResourceController,
    };

    const PREFIX: &str = "test";
    const NETWORK_ID: &str = "net";

    fn network(cidr: Option<&str>) -> Network {
        Network::new(NETWORK_ID.to_string())
            .settings(NetworkSettings::Create {
                cidr: cidr.map(str::to_string),
                availability_zones: 2,
            })
            .build()
    }

    fn parse<T: DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).expect("fixture should deserialize")
    }

    fn owned_tags_json() -> serde_json::Value {
        let tags: Vec<_> = standard_resource_tags(PREFIX, NETWORK_ID)
            .into_iter()
            .map(|(key, value)| json!({ "key": key, "value": value }))
            .collect();
        json!({ "item": tags })
    }

    /// Ownership tags plus the tag of the create call with this token.
    fn attempt_tags_json(token: &str) -> serde_json::Value {
        let mut tags = owned_tags_json();
        tags["item"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "key": CREATE_ATTEMPT_TAG, "value": token }));
        tags
    }

    fn filters_token(filters: Option<&Vec<Filter>>) -> Option<String> {
        filters?
            .iter()
            .find(|f| f.name == format!("tag:{CREATE_ATTEMPT_TAG}"))
            .map(|f| f.values[0].clone())
    }

    fn tag_value(specs: Option<&Vec<TagSpecification>>, key: &str) -> Option<String> {
        specs?
            .iter()
            .flat_map(|spec| &spec.tags)
            .find(|tag| tag.key == key)
            .map(|tag| tag.value.clone())
    }

    fn not_found() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "EC2 Resource".to_string(),
            resource_name: "x".to_string(),
        })
    }

    fn in_use() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceConflict {
            message: "DependencyViolation".to_string(),
            resource_type: "EC2 Resource".to_string(),
            resource_name: "x".to_string(),
        })
    }

    fn unavailable() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteServiceUnavailable {
            message: "connection reset".to_string(),
        })
    }

    async fn executor(
        ec2: MockEc2Api,
        controller: AwsNetworkController,
        cidr: Option<&str>,
    ) -> SingleControllerExecutor {
        let ec2 = Arc::new(ec2);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        SingleControllerExecutor::builder()
            .resource(network(cidr))
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .resource_prefix(PREFIX)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor should build")
    }

    fn controller(executor: &SingleControllerExecutor) -> &AwsNetworkController {
        executor
            .internal_state::<AwsNetworkController>()
            .expect("network controller state")
    }

    fn expect_two_zones(ec2: &mut MockEc2Api) {
        ec2.expect_describe_availability_zones().returning(|_| {
            Ok(parse(json!({
                "availabilityZoneInfo": { "item": [
                    { "zoneName": "us-east-1a" },
                    { "zoneName": "us-east-1b" },
                    { "zoneName": "us-east-1c" }
                ]}
            })))
        });
    }

    fn is_owned_tag_lookup(filters: Option<&Vec<Filter>>) -> bool {
        filters.is_some_and(|filters| {
            filters
                .iter()
                .any(|f| f.name == "tag:resource" && f.values == [NETWORK_ID])
                && filters
                    .iter()
                    .any(|f| f.name == "tag:deployment" && f.values == [PREFIX])
        })
    }

    // ─────────────── VPC ───────────────

    #[tokio::test]
    async fn vpc_id_survives_a_dns_failure_and_the_retry_does_not_create_another_vpc() {
        let mut ec2 = MockEc2Api::new();
        expect_two_zones(&mut ec2);
        // A first attempt has created nothing, so there is nothing to rediscover.
        ec2.expect_describe_vpcs().times(0);
        ec2.expect_create_vpc()
            .times(1)
            .withf(|request| request.cidr_block == "10.0.0.0/16")
            .returning(|_| Ok(parse(json!({ "vpc": { "vpcId": "vpc-1" } }))));
        let dns_calls = Arc::new(AtomicUsize::new(0));
        let calls = dns_calls.clone();
        ec2.expect_modify_vpc_attribute().returning(move |request| {
            assert_eq!(request.vpc_id, "vpc-1");
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(unavailable())
            } else {
                Ok(())
            }
        });

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::CreatingVpc,
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        executor.step().await.expect("VPC creation should succeed");
        assert_eq!(controller(&executor).vpc_id.as_deref(), Some("vpc-1"));
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::ConfiguringVpcDns
        );

        executor
            .step()
            .await
            .expect_err("the first DNS attribute call fails");
        assert_eq!(controller(&executor).vpc_id.as_deref(), Some("vpc-1"));
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::ConfiguringVpcDns
        );

        executor.step().await.expect("DNS retry should succeed");
        let state = controller(&executor);
        assert_eq!(state.state, AwsNetworkState::CreatingInternetGateway);
        assert_eq!(state.vpc_id.as_deref(), Some("vpc-1"));
        assert_eq!(state.cidr_block.as_deref(), Some("10.0.0.0/16"));
        assert_eq!(state.availability_zones, ["us-east-1a", "us-east-1b"]);
        assert_eq!(dns_calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn creating_vpc_records_the_owned_vpc_left_by_a_lost_response() {
        let mut ec2 = MockEc2Api::new();
        expect_two_zones(&mut ec2);
        // Only the attempt-token lookup runs: no CIDR search (it would see the VPC's range as
        // taken) and no create. A VPC with our ownership tags and the same CIDR but another
        // create's token is a leftover this attempt did not make.
        ec2.expect_describe_vpcs()
            .times(1)
            .withf(|request| {
                is_owned_tag_lookup(request.filters.as_ref())
                    && filters_token(request.filters.as_ref()).as_deref() == Some("attempt-1")
            })
            .returning(|_| {
                Ok(parse(json!({ "vpcSet": { "item": [
                    { "vpcId": "vpc-leftover", "cidrBlock": "100.70.0.0/16", "tagSet": attempt_tags_json("attempt-0") },
                    { "vpcId": "vpc-lost", "cidrBlock": "100.70.0.0/16", "tagSet": attempt_tags_json("attempt-1") }
                ]}})))
            });
        ec2.expect_create_vpc().times(0);

        // A recorded attempt without a VPC ID: `create_vpc` was called and its response lost.
        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::CreatingVpc,
                cidr_block: Some("100.70.0.0/16".to_string()),
                vpc_create_token: Some("attempt-1".to_string()),
                ..Default::default()
            },
            None,
        )
        .await;

        executor.step().await.expect("rediscovery should succeed");
        let state = controller(&executor);
        assert_eq!(state.state, AwsNetworkState::ConfiguringVpcDns);
        assert_eq!(state.vpc_id.as_deref(), Some("vpc-lost"));
        assert_eq!(state.cidr_block.as_deref(), Some("100.70.0.0/16"));
    }

    #[tokio::test]
    async fn creating_vpc_refuses_to_pick_between_several_owned_vpcs() {
        let mut ec2 = MockEc2Api::new();
        expect_two_zones(&mut ec2);
        ec2.expect_describe_vpcs().times(1).returning(|_| {
            Ok(parse(json!({ "vpcSet": { "item": [
                { "vpcId": "vpc-a", "cidrBlock": "100.70.0.0/16", "tagSet": attempt_tags_json("attempt-1") },
                { "vpcId": "vpc-b", "cidrBlock": "100.70.0.0/16", "tagSet": attempt_tags_json("attempt-1") }
            ]}})))
        });
        ec2.expect_create_vpc().times(0);

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::CreatingVpc,
                cidr_block: Some("100.70.0.0/16".to_string()),
                vpc_create_token: Some("attempt-1".to_string()),
                ..Default::default()
            },
            None,
        )
        .await;

        let error = executor
            .step()
            .await
            .expect_err("two owned VPCs must not be adopted");
        assert!(error.to_string().contains("vpc-a, vpc-b"), "{error}");
        assert_eq!(controller(&executor).vpc_id, None);
        assert_eq!(controller(&executor).state, AwsNetworkState::CreatingVpc);
    }

    #[tokio::test]
    async fn first_vpc_create_does_not_adopt_a_tagged_leftover_vpc() {
        let mut ec2 = MockEc2Api::new();
        expect_two_zones(&mut ec2);
        // A VPC carrying this network's tags, left by an earlier instance of the resource.
        // Only the CIDR search sees it, as a range already in use (its range is the first one
        // this network would otherwise pick).
        ec2.expect_describe_vpcs()
            .times(1)
            .withf(|request| request.filters.is_none())
            .returning(|_| {
                Ok(parse(json!({ "vpcSet": { "item": [{
                    "vpcId": "vpc-leftover",
                    "cidrBlock": "100.71.0.0/16",
                    "tagSet": owned_tags_json()
                }]}})))
            });
        ec2.expect_create_vpc()
            .times(1)
            .withf(|request| request.cidr_block != "100.71.0.0/16")
            .returning(|_| Ok(parse(json!({ "vpc": { "vpcId": "vpc-new" } }))));

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::CreatingVpc,
                ..Default::default()
            },
            None,
        )
        .await;

        executor.step().await.expect("VPC creation should succeed");
        let state = controller(&executor);
        assert_eq!(state.state, AwsNetworkState::ConfiguringVpcDns);
        assert_eq!(state.vpc_id.as_deref(), Some("vpc-new"));
        assert_ne!(state.cidr_block.as_deref(), Some("100.71.0.0/16"));
    }

    #[tokio::test]
    async fn retry_after_a_failed_vpc_create_reuses_the_chosen_cidr() {
        let mut ec2 = MockEc2Api::new();
        expect_two_zones(&mut ec2);
        let unfiltered_lookups = Arc::new(AtomicUsize::new(0));
        let lookups = unfiltered_lookups.clone();
        ec2.expect_describe_vpcs().returning(move |request| {
            if request.filters.is_none() {
                lookups.fetch_add(1, Ordering::SeqCst);
            }
            Ok(parse(json!({})))
        });
        let cidrs = Arc::new(Mutex::new(Vec::new()));
        let seen = cidrs.clone();
        let tokens = Arc::new(Mutex::new(Vec::new()));
        let seen_tokens = tokens.clone();
        ec2.expect_create_vpc().times(2).returning(move |request| {
            seen_tokens.lock().unwrap().push(
                tag_value(request.tag_specifications.as_ref(), CREATE_ATTEMPT_TAG)
                    .expect("the VPC carries its create-attempt token"),
            );
            let mut seen = seen.lock().unwrap();
            seen.push(request.cidr_block);
            if seen.len() == 1 {
                Err(unavailable())
            } else {
                Ok(parse(json!({ "vpc": { "vpcId": "vpc-1" } })))
            }
        });

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::CreatingVpc,
                ..Default::default()
            },
            None,
        )
        .await;

        executor.step().await.expect_err("first create fails");
        executor.step().await.expect("retry succeeds");

        let cidrs = cidrs.lock().unwrap();
        assert_eq!(cidrs.len(), 2);
        assert_eq!(cidrs[0], cidrs[1], "a retry must not move to another /16");
        let tokens = tokens.lock().unwrap();
        assert_eq!(
            tokens[0], tokens[1],
            "a retry reuses the token, so whatever the first call made stays findable"
        );
        assert_eq!(
            controller(&executor).vpc_create_token.as_ref(),
            Some(&tokens[0])
        );
        assert_eq!(unfiltered_lookups.load(Ordering::SeqCst), 1);
        assert_eq!(controller(&executor).vpc_id.as_deref(), Some("vpc-1"));
    }

    #[tokio::test]
    async fn vpc_missed_by_an_eventually_consistent_read_is_found_by_its_original_token() {
        let mut ec2 = MockEc2Api::new();
        expect_two_zones(&mut ec2);
        let lookups = Arc::new(AtomicUsize::new(0));
        let count = lookups.clone();
        ec2.expect_describe_vpcs()
            .times(2)
            .returning(move |request| {
                assert_eq!(
                    filters_token(request.filters.as_ref()).as_deref(),
                    Some("attempt-1")
                );
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    // The first read does not see the VPC the lost create made yet.
                    return Ok(parse(json!({})));
                }
                Ok(parse(json!({ "vpcSet": { "item": [{
                    "vpcId": "vpc-lost",
                    "cidrBlock": "100.70.0.0/16",
                    "tagSet": attempt_tags_json("attempt-1")
                }]}})))
            });
        // The repeated create carries the original token and fails (throttled).
        ec2.expect_create_vpc().times(1).returning(|request| {
            assert_eq!(
                tag_value(request.tag_specifications.as_ref(), CREATE_ATTEMPT_TAG).as_deref(),
                Some("attempt-1")
            );
            Err(unavailable())
        });

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::CreatingVpc,
                cidr_block: Some("100.70.0.0/16".to_string()),
                vpc_create_token: Some("attempt-1".to_string()),
                ..Default::default()
            },
            None,
        )
        .await;

        executor
            .step()
            .await
            .expect_err("the repeated create fails");
        assert_eq!(
            controller(&executor).vpc_create_token.as_deref(),
            Some("attempt-1")
        );
        executor.step().await.expect("the retry finds the VPC");
        let state = controller(&executor);
        assert_eq!(state.vpc_id.as_deref(), Some("vpc-lost"));
        assert_eq!(state.state, AwsNetworkState::ConfiguringVpcDns);
    }

    #[tokio::test]
    async fn delete_after_a_missed_read_still_finds_the_vpc_by_its_original_token() {
        let mut ec2 = MockEc2Api::new();
        expect_two_zones(&mut ec2);
        expect_no_named_leftovers(&mut ec2);
        let lookups = Arc::new(AtomicUsize::new(0));
        let count = lookups.clone();
        ec2.expect_describe_vpcs()
            .times(2)
            .returning(move |request| {
                assert_eq!(
                    filters_token(request.filters.as_ref()).as_deref(),
                    Some("attempt-1")
                );
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Ok(parse(json!({})));
                }
                Ok(parse(json!({ "vpcSet": { "item": [{
                    "vpcId": "vpc-lost",
                    "cidrBlock": "100.70.0.0/16",
                    "tagSet": attempt_tags_json("attempt-1")
                }]}})))
            });
        ec2.expect_create_vpc()
            .times(1)
            .returning(|_| Err(unavailable()));
        ec2.expect_delete_vpc()
            .times(1)
            .withf(|id| id == "vpc-lost")
            .returning(|_| Ok(()));

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::CreatingVpc,
                cidr_block: Some("100.70.0.0/16".to_string()),
                vpc_create_token: Some("attempt-1".to_string()),
                ..Default::default()
            },
            None,
        )
        .await;

        // The read misses the VPC and the repeated create fails; then the resource is deleted
        // (as a replace would do after the create gives up).
        executor
            .step()
            .await
            .expect_err("the repeated create fails");
        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    // ─────────────── Internet gateway ───────────────

    fn after_vpc(state: AwsNetworkState) -> AwsNetworkController {
        AwsNetworkController {
            state,
            vpc_id: Some("vpc-1".to_string()),
            cidr_block: Some("10.0.0.0/16".to_string()),
            availability_zones: vec!["us-east-1a".to_string(), "us-east-1b".to_string()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn internet_gateway_is_recorded_before_attach_and_a_failed_attach_only_retries_attach() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_create_internet_gateway()
            .times(1)
            .returning(|_| {
                Ok(parse(
                    json!({ "internetGateway": { "internetGatewayId": "igw-1" } }),
                ))
            });
        let attaches = Arc::new(AtomicUsize::new(0));
        let count = attaches.clone();
        ec2.expect_attach_internet_gateway()
            .returning(move |request| {
                assert_eq!(request.internet_gateway_id, "igw-1");
                assert_eq!(request.vpc_id, "vpc-1");
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(unavailable())
                } else {
                    Ok(())
                }
            });

        let mut executor = executor(
            ec2,
            after_vpc(AwsNetworkState::CreatingInternetGateway),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.step().await.expect("create succeeds");
        assert_eq!(
            controller(&executor).internet_gateway_id.as_deref(),
            Some("igw-1")
        );
        executor.step().await.expect_err("attach fails");
        assert_eq!(
            controller(&executor).internet_gateway_id.as_deref(),
            Some("igw-1")
        );
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::AttachingInternetGateway
        );
        executor.step().await.expect("attach retry succeeds");
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::CreatingSubnets
        );
        assert_eq!(attaches.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn already_associated_attach_is_success_only_when_attached_to_our_vpc() {
        for (attached_vpc, expect_ok) in [("vpc-1", true), ("vpc-other", false)] {
            let mut ec2 = MockEc2Api::new();
            ec2.expect_attach_internet_gateway()
                .times(1)
                .returning(|_| Err(in_use()));
            ec2.expect_describe_internet_gateways()
                .times(1)
                .withf(|request| {
                    request.internet_gateway_ids.as_deref() == Some(&["igw-1".to_string()][..])
                })
                .returning(move |_| {
                    Ok(parse(json!({ "internetGatewaySet": { "item": [{
                        "internetGatewayId": "igw-1",
                        "attachmentSet": { "item": [{ "vpcId": attached_vpc, "state": "available" }] }
                    }]}})))
                });

            let mut executor = executor(
                ec2,
                AwsNetworkController {
                    internet_gateway_id: Some("igw-1".to_string()),
                    ..after_vpc(AwsNetworkState::AttachingInternetGateway)
                },
                Some("10.0.0.0/16"),
            )
            .await;

            let result = executor.step().await;
            assert_eq!(result.is_ok(), expect_ok, "attached to {attached_vpc}");
            let expected_state = if expect_ok {
                AwsNetworkState::CreatingSubnets
            } else {
                AwsNetworkState::AttachingInternetGateway
            };
            assert_eq!(controller(&executor).state, expected_state);
        }
    }

    // ─────────────── Subnets ───────────────

    fn subnet_id_for(cidr: &str) -> String {
        format!("subnet-{}", cidr.split('.').nth(2).expect("third octet"))
    }

    #[tokio::test]
    async fn subnet_retry_creates_only_the_subnet_that_failed() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_subnets()
            .returning(|_| Ok(parse(json!({}))));
        let created = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = created.clone();
        let private_2_attempts = Arc::new(AtomicUsize::new(0));
        let attempts = private_2_attempts.clone();
        ec2.expect_create_subnet().returning(move |request| {
            // private-2 (10.0.144.0/20) fails on its first attempt.
            if request.cidr_block == "10.0.144.0/20" && attempts.fetch_add(1, Ordering::SeqCst) == 0
            {
                return Err(unavailable());
            }
            log.lock().unwrap().push(request.cidr_block.clone());
            Ok(parse(
                json!({ "subnet": { "subnetId": subnet_id_for(&request.cidr_block) } }),
            ))
        });

        let mut executor = executor(
            ec2,
            after_vpc(AwsNetworkState::CreatingSubnets),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.step().await.expect_err("private-2 fails");
        assert_eq!(
            controller(&executor).public_subnet_ids,
            ["subnet-0", "subnet-16"]
        );
        assert_eq!(controller(&executor).private_subnet_ids, ["subnet-128"]);

        executor.step().await.expect("retry succeeds");
        let state = controller(&executor);
        assert_eq!(state.state, AwsNetworkState::CreatingRouteTables);
        assert_eq!(state.public_subnet_ids, ["subnet-0", "subnet-16"]);
        assert_eq!(state.private_subnet_ids, ["subnet-128", "subnet-144"]);
        assert_eq!(
            *created.lock().unwrap(),
            [
                "10.0.0.0/20",
                "10.0.16.0/20",
                "10.0.128.0/20",
                "10.0.144.0/20"
            ],
            "each subnet is created exactly once"
        );
        assert_eq!(
            state.subnets_by_failure_domain["us-east-1a"],
            AwsFailureDomainSubnets {
                public_subnet_ids: vec!["subnet-0".to_string()],
                private_subnet_ids: vec!["subnet-128".to_string()],
            }
        );
        assert_eq!(
            state.subnets_by_failure_domain["us-east-1b"],
            AwsFailureDomainSubnets {
                public_subnet_ids: vec!["subnet-16".to_string()],
                private_subnet_ids: vec!["subnet-144".to_string()],
            }
        );
    }

    fn with_subnet_attempt(mut controller: AwsNetworkController) -> AwsNetworkController {
        controller.subnet_create_attempt = Some(SubnetCreateAttempt {
            token: "attempt-1".to_string(),
            cidr: "10.0.0.0/20".to_string(),
            subnet_type: "Public".to_string(),
        });
        controller
    }

    #[tokio::test]
    async fn subnet_created_by_a_lost_response_is_recorded_not_recreated() {
        let mut ec2 = MockEc2Api::new();
        // Only the subnet with a recorded attempt is looked up, by its exact token.
        ec2.expect_describe_subnets().times(1).returning(|request| {
            let filters = request.filters.expect("lookup by VPC, CIDR and token");
            assert!(filters
                .iter()
                .any(|f| f.name == "vpc-id" && f.values == ["vpc-1"]));
            assert!(filters
                .iter()
                .any(|f| f.name == "cidr-block" && f.values == ["10.0.0.0/20"]));
            assert_eq!(filters_token(Some(&filters)).as_deref(), Some("attempt-1"));
            Ok(parse(json!({ "subnetSet": { "item": [{
                "subnetId": "subnet-lost",
                "vpcId": "vpc-1",
                "cidrBlock": "10.0.0.0/20",
                "tagSet": attempt_tags_json("attempt-1")
            }]}})))
        });
        ec2.expect_create_subnet().times(3).returning(|request| {
            assert_ne!(request.cidr_block, "10.0.0.0/20");
            Ok(parse(
                json!({ "subnet": { "subnetId": subnet_id_for(&request.cidr_block) } }),
            ))
        });

        let mut executor = executor(
            ec2,
            with_subnet_attempt(after_vpc(AwsNetworkState::CreatingSubnets)),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.step().await.expect("subnets should be ensured");
        let state = controller(&executor);
        assert_eq!(state.public_subnet_ids, ["subnet-lost", "subnet-16"]);
        assert_eq!(state.private_subnet_ids, ["subnet-128", "subnet-144"]);
        assert_eq!(state.subnet_create_attempt, None);
    }

    #[tokio::test]
    async fn subnet_from_another_create_attempt_is_not_adopted() {
        let mut ec2 = MockEc2Api::new();
        // Same VPC, CIDR and ownership tags, but another controller's token. Looked up before
        // the create and again after its conflict.
        ec2.expect_describe_subnets().times(2).returning(|_| {
            Ok(parse(json!({ "subnetSet": { "item": [{
                "subnetId": "subnet-other",
                "vpcId": "vpc-1",
                "cidrBlock": "10.0.0.0/20",
                "tagSet": attempt_tags_json("attempt-0")
            }]}})))
        });
        let new_token = Arc::new(Mutex::new(None));
        let seen = new_token.clone();
        ec2.expect_create_subnet()
            .times(1)
            .returning(move |request| {
                *seen.lock().unwrap() =
                    tag_value(request.tag_specifications.as_ref(), CREATE_ATTEMPT_TAG);
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        message: "InvalidSubnet.Conflict".to_string(),
                        resource_type: "EC2 Resource".to_string(),
                        resource_name: request.cidr_block,
                    },
                ))
            });

        let mut executor = executor(
            ec2,
            with_subnet_attempt(after_vpc(AwsNetworkState::CreatingSubnets)),
            Some("10.0.0.0/16"),
        )
        .await;

        executor
            .step()
            .await
            .expect_err("AWS refuses the conflicting subnet");
        let state = controller(&executor);
        assert!(state.public_subnet_ids.is_empty());
        let attempt = state
            .subnet_create_attempt
            .as_ref()
            .expect("the attempt stays recorded");
        assert_eq!(attempt.token, "attempt-1", "the token is never rotated");
        assert_eq!(Some(&attempt.token), new_token.lock().unwrap().as_ref());
    }

    #[tokio::test]
    async fn subnet_missed_by_an_eventually_consistent_read_is_found_after_the_conflict() {
        let mut ec2 = MockEc2Api::new();
        let lookups = Arc::new(AtomicUsize::new(0));
        let count = lookups.clone();
        // The first lookup misses the subnet the lost create made; after the conflict it shows.
        ec2.expect_describe_subnets().returning(move |request| {
            assert_eq!(
                filters_token(request.filters.as_ref()).as_deref(),
                Some("attempt-1")
            );
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(parse(json!({})));
            }
            Ok(parse(json!({ "subnetSet": { "item": [{
                "subnetId": "subnet-lost",
                "vpcId": "vpc-1",
                "cidrBlock": "10.0.0.0/20",
                "tagSet": attempt_tags_json("attempt-1")
            }]}})))
        });
        ec2.expect_create_subnet().returning(|request| {
            if request.cidr_block == "10.0.0.0/20" {
                assert_eq!(
                    tag_value(request.tag_specifications.as_ref(), CREATE_ATTEMPT_TAG).as_deref(),
                    Some("attempt-1")
                );
                return Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        message: "InvalidSubnet.Conflict".to_string(),
                        resource_type: "Subnet".to_string(),
                        resource_name: request.cidr_block,
                    },
                ));
            }
            Ok(parse(
                json!({ "subnet": { "subnetId": subnet_id_for(&request.cidr_block) } }),
            ))
        });

        let mut executor = executor(
            ec2,
            with_subnet_attempt(after_vpc(AwsNetworkState::CreatingSubnets)),
            Some("10.0.0.0/16"),
        )
        .await;

        executor
            .step()
            .await
            .expect("the conflict resolves to our subnet");
        let state = controller(&executor);
        assert_eq!(state.public_subnet_ids, ["subnet-lost", "subnet-16"]);
        assert_eq!(lookups.load(Ordering::SeqCst), 2);
        assert_eq!(state.subnet_create_attempt, None);
    }

    #[tokio::test]
    async fn subnet_create_response_without_an_id_is_an_error() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_subnets()
            .returning(|_| Ok(parse(json!({}))));
        ec2.expect_create_subnet()
            .times(1)
            .returning(|_| Ok(parse(json!({ "subnet": {} }))));

        let mut executor = executor(
            ec2,
            after_vpc(AwsNetworkState::CreatingSubnets),
            Some("10.0.0.0/16"),
        )
        .await;

        let error = executor.step().await.expect_err("missing subnet ID");
        assert!(error.to_string().contains("no subnet ID"), "{error}");
        assert!(controller(&executor).public_subnet_ids.is_empty());
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::CreatingSubnets
        );
    }

    // ─────────────── NAT gateway ───────────────

    fn after_route_tables(state: AwsNetworkState) -> AwsNetworkController {
        AwsNetworkController {
            internet_gateway_id: Some("igw-1".to_string()),
            public_subnet_ids: vec!["subnet-pub-a".to_string(), "subnet-pub-b".to_string()],
            private_subnet_ids: vec!["subnet-priv-a".to_string(), "subnet-priv-b".to_string()],
            public_route_table_id: Some("rtb-pub".to_string()),
            private_route_table_id: Some("rtb-priv".to_string()),
            ..after_vpc(state)
        }
    }

    #[tokio::test]
    async fn nat_create_retry_reuses_the_elastic_ip_and_client_token() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_allocate_address()
            .times(1)
            .returning(|_| Ok(parse(json!({ "allocationId": "eipalloc-1" }))));
        let requests = Arc::new(Mutex::new(Vec::<CreateNatGatewayRequest>::new()));
        let seen = requests.clone();
        ec2.expect_create_nat_gateway()
            .times(2)
            .returning(move |request| {
                let mut seen = seen.lock().unwrap();
                seen.push(request);
                if seen.len() == 1 {
                    Err(unavailable())
                } else {
                    Ok(parse(json!({ "natGateway": { "natGatewayId": "nat-1" } })))
                }
            });

        let mut executor = executor(
            ec2,
            after_route_tables(AwsNetworkState::AllocatingElasticIp),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.step().await.expect("allocation succeeds");
        assert_eq!(
            controller(&executor).eip_allocation_id.as_deref(),
            Some("eipalloc-1")
        );
        executor.step().await.expect_err("first NAT create fails");
        assert_eq!(
            controller(&executor).eip_allocation_id.as_deref(),
            Some("eipalloc-1")
        );
        executor.step().await.expect("NAT create retry succeeds");

        let state = controller(&executor);
        assert_eq!(state.state, AwsNetworkState::WaitingForNatGateway);
        assert_eq!(state.nat_gateway_id.as_deref(), Some("nat-1"));
        let requests = requests.lock().unwrap();
        for request in requests.iter() {
            assert_eq!(request.allocation_id.as_deref(), Some("eipalloc-1"));
            assert_eq!(request.subnet_id, "subnet-pub-a");
            assert_eq!(
                request.client_token.as_deref(),
                Some("alien-nat-eipalloc-1")
            );
            // The tag delete uses to find a gateway whose ID was never recorded.
            assert_eq!(
                tag_value(request.tag_specifications.as_ref(), CREATE_ATTEMPT_TAG),
                state.nat_gateway_create_token
            );
        }
        assert!(state.nat_gateway_create_token.is_some());
    }

    #[tokio::test]
    async fn failed_nat_gateway_surfaces_the_aws_reason_and_keeps_ids_for_delete() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_nat_gateways().times(1).returning(|_| {
            Ok(parse(json!({ "natGatewaySet": { "item": [{
                "natGatewayId": "nat-1",
                "state": "failed",
                "failureCode": "InsufficientFreeAddressesInSubnet",
                "failureMessage": "Subnet has insufficient free addresses to create this NAT gateway"
            }]}})))
        });
        ec2.expect_create_route().times(0);

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                nat_gateway_id: Some("nat-1".to_string()),
                eip_allocation_id: Some("eipalloc-1".to_string()),
                ..after_route_tables(AwsNetworkState::WaitingForNatGateway)
            },
            Some("10.0.0.0/16"),
        )
        .await;

        let error = executor.step().await.expect_err("failed NAT");
        assert!(
            error.to_string().contains(
                "InsufficientFreeAddressesInSubnet: Subnet has insufficient free addresses"
            ),
            "{error}"
        );
        assert!(
            !error.retryable,
            "re-reading a failed NAT gateway cannot succeed"
        );
        assert_eq!(
            controller(&executor).nat_gateway_id.as_deref(),
            Some("nat-1")
        );
        assert_eq!(
            controller(&executor).eip_allocation_id.as_deref(),
            Some("eipalloc-1")
        );
    }

    // ─────────────── Delete ───────────────

    /// Delete looks up route tables and the security group by name when their IDs are not
    /// recorded; these tests have none to find.
    fn expect_no_named_leftovers(ec2: &mut MockEc2Api) {
        ec2.expect_describe_route_tables()
            .returning(|_| Ok(parse(json!({}))));
        ec2.expect_describe_security_groups()
            .returning(|_| Ok(parse(json!({}))));
    }

    /// A failed create, as the executor hands it to delete when it replaces the resource.
    fn failed_create(controller: AwsNetworkController) -> AwsNetworkController {
        AwsNetworkController {
            state: AwsNetworkState::CreateFailed,
            ..controller
        }
    }

    #[tokio::test]
    async fn delete_recovers_the_vpc_of_a_lost_create_response() {
        let mut ec2 = MockEc2Api::new();
        expect_no_named_leftovers(&mut ec2);
        ec2.expect_describe_vpcs()
            .times(1)
            .withf(|request| {
                filters_token(request.filters.as_ref()).as_deref() == Some("attempt-1")
            })
            .returning(|_| {
                Ok(parse(json!({ "vpcSet": { "item": [{
                    "vpcId": "vpc-lost",
                    "cidrBlock": "10.0.0.0/16",
                    "tagSet": attempt_tags_json("attempt-1")
                }]}})))
            });
        ec2.expect_delete_vpc()
            .times(1)
            .withf(|id| id == "vpc-lost")
            .returning(|_| Ok(()));

        let mut executor = executor(
            ec2,
            failed_create(AwsNetworkController {
                cidr_block: Some("10.0.0.0/16".to_string()),
                vpc_create_token: Some("attempt-1".to_string()),
                ..Default::default()
            }),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn delete_without_a_create_attempt_token_looks_nothing_up() {
        // State persisted before attempt tokens: a CIDR but no token says nothing about
        // which VPC, if any, the create made.
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_vpcs().times(0);
        ec2.expect_delete_vpc().times(0);

        let mut executor = executor(
            ec2,
            failed_create(AwsNetworkController {
                cidr_block: Some("10.0.0.0/16".to_string()),
                ..Default::default()
            }),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn delete_recovers_the_subnet_of_a_lost_create_response() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut ec2 = MockEc2Api::new();
        expect_no_named_leftovers(&mut ec2);
        ec2.expect_describe_subnets().times(1).returning(|request| {
            let filters = request.filters.expect("lookup by VPC, CIDR and token");
            assert!(filters
                .iter()
                .any(|f| f.name == "vpc-id" && f.values == ["vpc-1"]));
            assert_eq!(filters_token(Some(&filters)).as_deref(), Some("attempt-1"));
            Ok(parse(json!({ "subnetSet": { "item": [{
                "subnetId": "subnet-lost",
                "vpcId": "vpc-1",
                "cidrBlock": "10.0.0.0/20",
                "tagSet": attempt_tags_json("attempt-1")
            }]}})))
        });
        let log = calls.clone();
        ec2.expect_delete_subnet().times(1).returning(move |id| {
            log.lock().unwrap().push(format!("delete_subnet {id}"));
            Ok(())
        });
        let log = calls.clone();
        ec2.expect_delete_vpc().times(1).returning(move |id| {
            log.lock().unwrap().push(format!("delete_vpc {id}"));
            Ok(())
        });

        let mut executor = executor(
            ec2,
            failed_create(with_subnet_attempt(after_vpc(
                AwsNetworkState::CreatingSubnets,
            ))),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *calls.lock().unwrap(),
            ["delete_subnet subnet-lost", "delete_vpc vpc-1"],
            "the recovered subnet is deleted before the VPC it blocks"
        );
    }

    #[tokio::test]
    async fn delete_recovers_the_nat_gateway_of_a_lost_create_response() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut ec2 = MockEc2Api::new();
        expect_no_named_leftovers(&mut ec2);
        let log = calls.clone();
        ec2.expect_describe_nat_gateways().returning(move |request| {
            if let Some(token) = filters_token(request.filters.as_ref()) {
                assert_eq!(token, "attempt-1");
                return Ok(parse(json!({ "natGatewaySet": { "item": [{
                    "natGatewayId": "nat-lost",
                    "state": "available",
                    "tagSet": attempt_tags_json("attempt-1")
                }]}})));
            }
            log.lock().unwrap().push("describe_nat_gateways deleted".to_string());
            Ok(parse(json!({ "natGatewaySet": { "item": [{ "natGatewayId": "nat-lost", "state": "deleted" }] } })))
        });
        let log = calls.clone();
        ec2.expect_delete_nat_gateway()
            .times(1)
            .returning(move |id| {
                log.lock().unwrap().push(format!("delete_nat_gateway {id}"));
                Ok(parse(json!({ "natGatewayId": id })))
            });
        let log = calls.clone();
        ec2.expect_release_address().times(1).returning(move |id| {
            log.lock().unwrap().push(format!("release_address {id}"));
            Ok(())
        });
        let log = calls.clone();
        ec2.expect_delete_vpc().times(1).returning(move |id| {
            log.lock().unwrap().push(format!("delete_vpc {id}"));
            Ok(())
        });

        let mut executor = executor(
            ec2,
            failed_create(AwsNetworkController {
                eip_allocation_id: Some("eipalloc-1".to_string()),
                nat_gateway_create_token: Some("attempt-1".to_string()),
                ..after_vpc(AwsNetworkState::CreatingNatGateway)
            }),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "delete_nat_gateway nat-lost",
                "describe_nat_gateways deleted",
                "release_address eipalloc-1",
                "delete_vpc vpc-1",
            ],
            "the recovered NAT gateway is deleted before its Elastic IP is released"
        );
    }

    // ─────────────── Internet gateway and Elastic IP create attempts ───────────────

    #[tokio::test]
    async fn internet_gateway_retry_adopts_only_its_own_create_attempt() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_internet_gateways()
            .times(1)
            .withf(|request| {
                filters_token(request.filters.as_ref()).as_deref() == Some("attempt-1")
            })
            .returning(|_| {
                Ok(parse(json!({ "internetGatewaySet": { "item": [
                    { "internetGatewayId": "igw-other", "tagSet": attempt_tags_json("attempt-0") },
                    { "internetGatewayId": "igw-lost", "tagSet": attempt_tags_json("attempt-1") }
                ]}})))
            });
        ec2.expect_create_internet_gateway().times(0);

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                internet_gateway_create_token: Some("attempt-1".to_string()),
                ..after_vpc(AwsNetworkState::CreatingInternetGateway)
            },
            Some("10.0.0.0/16"),
        )
        .await;

        executor.step().await.expect("rediscovery succeeds");
        let state = controller(&executor);
        assert_eq!(state.internet_gateway_id.as_deref(), Some("igw-lost"));
        assert_eq!(state.state, AwsNetworkState::AttachingInternetGateway);
    }

    #[tokio::test]
    async fn internet_gateway_retry_without_a_match_reuses_its_token() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_internet_gateways()
            .times(1)
            .returning(|_| {
                Ok(parse(json!({ "internetGatewaySet": { "item": [
                    { "internetGatewayId": "igw-other", "tagSet": attempt_tags_json("attempt-0") }
                ]}})))
            });
        let token = Arc::new(Mutex::new(None));
        let seen = token.clone();
        ec2.expect_create_internet_gateway()
            .times(1)
            .returning(move |request| {
                *seen.lock().unwrap() =
                    tag_value(request.tag_specifications.as_ref(), CREATE_ATTEMPT_TAG);
                Ok(parse(
                    json!({ "internetGateway": { "internetGatewayId": "igw-new" } }),
                ))
            });

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                internet_gateway_create_token: Some("attempt-0-of-mine".to_string()),
                ..after_vpc(AwsNetworkState::CreatingInternetGateway)
            },
            Some("10.0.0.0/16"),
        )
        .await;

        executor.step().await.expect("create succeeds");
        let state = controller(&executor);
        assert_eq!(state.internet_gateway_id.as_deref(), Some("igw-new"));
        let token = token
            .lock()
            .unwrap()
            .clone()
            .expect("tagged with its token");
        assert_eq!(token, "attempt-0-of-mine", "the token is never rotated");
        assert_eq!(state.internet_gateway_create_token, Some(token));
    }

    #[tokio::test]
    async fn elastic_ip_retry_adopts_only_its_own_allocation() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_addresses().times(1).returning(|| {
            Ok(parse(json!({ "addressesSet": { "item": [
                { "allocationId": "eipalloc-other", "tagSet": attempt_tags_json("attempt-0") },
                { "allocationId": "eipalloc-untagged" },
                { "allocationId": "eipalloc-lost", "tagSet": attempt_tags_json("attempt-1") }
            ]}})))
        });
        ec2.expect_allocate_address().times(0);

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                eip_create_token: Some("attempt-1".to_string()),
                ..after_route_tables(AwsNetworkState::AllocatingElasticIp)
            },
            Some("10.0.0.0/16"),
        )
        .await;

        executor.step().await.expect("rediscovery succeeds");
        let state = controller(&executor);
        assert_eq!(state.eip_allocation_id.as_deref(), Some("eipalloc-lost"));
        assert_eq!(state.state, AwsNetworkState::CreatingNatGateway);
    }

    #[tokio::test]
    async fn recorded_elastic_ip_is_used_without_a_lookup() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_addresses().times(0);
        ec2.expect_allocate_address().times(0);

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                eip_allocation_id: Some("eipalloc-1".to_string()),
                eip_create_token: Some("attempt-1".to_string()),
                ..after_route_tables(AwsNetworkState::AllocatingElasticIp)
            },
            Some("10.0.0.0/16"),
        )
        .await;

        executor
            .step()
            .await
            .expect("the recorded allocation is reused");
        let state = controller(&executor);
        assert_eq!(state.eip_allocation_id.as_deref(), Some("eipalloc-1"));
        assert_eq!(state.state, AwsNetworkState::CreatingNatGateway);
    }

    #[tokio::test]
    async fn delete_recovers_the_internet_gateway_and_elastic_ip_of_lost_responses() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut ec2 = MockEc2Api::new();
        expect_no_named_leftovers(&mut ec2);
        ec2.expect_describe_internet_gateways()
            .times(1)
            .returning(|_| {
                Ok(parse(json!({ "internetGatewaySet": { "item": [
                    { "internetGatewayId": "igw-lost", "tagSet": attempt_tags_json("igw-attempt") }
                ]}})))
            });
        ec2.expect_describe_addresses().times(1).returning(|| {
            Ok(parse(json!({ "addressesSet": { "item": [
                { "allocationId": "eipalloc-lost", "tagSet": attempt_tags_json("eip-attempt") }
            ]}})))
        });
        let log = calls.clone();
        ec2.expect_release_address().times(1).returning(move |id| {
            log.lock().unwrap().push(format!("release_address {id}"));
            Ok(())
        });
        // Never attached: the lost response was the create's.
        ec2.expect_detach_internet_gateway()
            .times(1)
            .returning(|_| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        message: "Gateway.NotAttached".to_string(),
                        resource_type: "InternetGateway".to_string(),
                        resource_name: "igw-lost".to_string(),
                    },
                ))
            });
        let log = calls.clone();
        ec2.expect_delete_internet_gateway()
            .times(1)
            .returning(move |id| {
                log.lock()
                    .unwrap()
                    .push(format!("delete_internet_gateway {id}"));
                Ok(())
            });
        let log = calls.clone();
        ec2.expect_delete_vpc().times(1).returning(move |id| {
            log.lock().unwrap().push(format!("delete_vpc {id}"));
            Ok(())
        });

        let mut executor = executor(
            ec2,
            failed_create(AwsNetworkController {
                internet_gateway_create_token: Some("igw-attempt".to_string()),
                eip_create_token: Some("eip-attempt".to_string()),
                ..after_vpc(AwsNetworkState::CreatingInternetGateway)
            }),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "release_address eipalloc-lost",
                "delete_internet_gateway igw-lost",
                "delete_vpc vpc-1",
            ]
        );
    }

    #[tokio::test]
    async fn delete_finds_route_tables_and_security_group_by_name_when_ids_were_not_recorded() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_route_tables()
            .times(2)
            .returning(|request| {
                let filters = request.filters.expect("lookup by VPC and name");
                assert!(filters
                    .iter()
                    .any(|f| f.name == "vpc-id" && f.values == ["vpc-1"]));
                let name = filters
                    .iter()
                    .find(|f| f.name == "tag:Name")
                    .map(|f| f.values[0].clone())
                    .expect("name filter");
                let id = if name == format!("{PREFIX}-public-rt") {
                    "rtb-public"
                } else {
                    assert_eq!(name, format!("{PREFIX}-private-rt"));
                    "rtb-private"
                };
                Ok(parse(
                    json!({ "routeTableSet": { "item": [{ "routeTableId": id }] } }),
                ))
            });
        ec2.expect_describe_security_groups()
            .times(1)
            .returning(|request| {
                let filters = request.filters.expect("lookup by VPC and name");
                assert!(filters
                    .iter()
                    .any(|f| f.name == "group-name" && f.values == [format!("{PREFIX}-sg")]));
                Ok(parse(
                    json!({ "securityGroupInfo": { "item": [{ "groupId": "sg-lost" }] } }),
                ))
            });
        let log = calls.clone();
        ec2.expect_delete_security_group()
            .times(1)
            .returning(move |id| {
                log.lock()
                    .unwrap()
                    .push(format!("delete_security_group {id}"));
                Ok(())
            });
        let log = calls.clone();
        ec2.expect_delete_route_table()
            .times(2)
            .returning(move |id| {
                log.lock().unwrap().push(format!("delete_route_table {id}"));
                Ok(())
            });
        let log = calls.clone();
        ec2.expect_delete_vpc().times(1).returning(move |id| {
            log.lock().unwrap().push(format!("delete_vpc {id}"));
            Ok(())
        });

        let mut executor = executor(
            ec2,
            failed_create(after_vpc(AwsNetworkState::CreatingRouteTables)),
            Some("10.0.0.0/16"),
        )
        .await;

        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "delete_security_group sg-lost",
                "delete_route_table rtb-public",
                "delete_route_table rtb-private",
                "delete_vpc vpc-1",
            ]
        );
    }

    #[tokio::test]
    async fn delete_waits_for_nat_gateway_deletion_before_releasing_its_elastic_ip() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut ec2 = MockEc2Api::new();
        expect_no_named_leftovers(&mut ec2);
        let log = calls.clone();
        ec2.expect_delete_nat_gateway()
            .times(1)
            .returning(move |id| {
                log.lock().unwrap().push(format!("delete_nat_gateway {id}"));
                Ok(parse(json!({ "natGatewayId": id })))
            });
        let log = calls.clone();
        ec2.expect_describe_nat_gateways().returning(move |_| {
            let mut log = log.lock().unwrap();
            let polls = log.iter().filter(|c| c.starts_with("describe")).count();
            let state = if polls < 2 { "deleting" } else { "deleted" };
            log.push(format!("describe_nat_gateways {state}"));
            Ok(parse(json!({ "natGatewaySet": { "item": [{ "natGatewayId": "nat-1", "state": state }] } })))
        });
        let log = calls.clone();
        ec2.expect_release_address().times(1).returning(move |id| {
            log.lock().unwrap().push(format!("release_address {id}"));
            Ok(())
        });
        let log = calls.clone();
        ec2.expect_delete_vpc().times(1).returning(move |id| {
            log.lock().unwrap().push(format!("delete_vpc {id}"));
            Ok(())
        });

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::Ready,
                vpc_id: Some("vpc-1".to_string()),
                nat_gateway_id: Some("nat-1".to_string()),
                eip_allocation_id: Some("eipalloc-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");

        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "delete_nat_gateway nat-1",
                "describe_nat_gateways deleting",
                "describe_nat_gateways deleting",
                "describe_nat_gateways deleted",
                "release_address eipalloc-1",
                "delete_vpc vpc-1",
            ]
        );
        assert_polling_delays(
            &executor.take_suggested_delays(),
            Duration::from_secs(15),
            "delete",
        );
        let state = controller(&executor);
        assert_eq!(state.nat_gateway_id, None);
        assert_eq!(state.eip_allocation_id, None);
        assert_eq!(state.vpc_id, None);
    }

    fn auth_failure(id: &str) -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: "EC2 Resource".to_string(),
            resource_name: id.to_string(),
        })
    }

    fn addresses(associated: bool) -> DescribeAddressesResponse {
        let mut address = json!({ "allocationId": "eipalloc-1", "domain": "vpc" });
        if associated {
            address["associationId"] = json!("eipassoc-1");
            address["networkInterfaceId"] = json!("eni-nat");
        }
        parse(json!({ "addressesSet": { "item": [address] } }))
    }

    /// EC2 answers AuthFailure while a NAT gateway still holds the address. That must stay a
    /// wait: as access denied it would end the whole delete and leave the rest of the network.
    #[tokio::test]
    async fn auth_failure_on_an_associated_elastic_ip_keeps_its_id_and_polls() {
        let mut ec2 = MockEc2Api::new();
        let releases = Arc::new(AtomicUsize::new(0));
        let count = releases.clone();
        ec2.expect_release_address().returning(move |id| {
            assert_eq!(id, "eipalloc-1");
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(auth_failure(id))
            } else {
                Ok(())
            }
        });
        ec2.expect_describe_addresses()
            .times(1)
            .returning(|| Ok(addresses(true)));

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::ReleasingElasticIp,
                vpc_id: Some("vpc-1".to_string()),
                eip_allocation_id: Some("eipalloc-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        let result = executor
            .step()
            .await
            .expect("an associated address is a wait, not a failure");
        assert_eq!(result.suggested_delay, Some(Duration::from_secs(15)));
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::ReleasingElasticIp
        );
        assert_eq!(
            controller(&executor).eip_allocation_id.as_deref(),
            Some("eipalloc-1")
        );

        executor.step().await.expect("release succeeds");
        assert_eq!(releases.load(Ordering::SeqCst), 2);
        assert_eq!(controller(&executor).eip_allocation_id, None);
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::DeletingSecurityGroup
        );
    }

    /// Access denied on an address that is not associated is a real permission error and is
    /// returned, so the executor's best-effort delete rule applies to it.
    #[tokio::test]
    async fn auth_failure_on_an_unassociated_elastic_ip_is_returned() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_release_address()
            .times(1)
            .returning(|id| Err(auth_failure(id)));
        ec2.expect_describe_addresses()
            .times(1)
            .returning(|| Ok(addresses(false)));

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::ReleasingElasticIp,
                vpc_id: Some("vpc-1".to_string()),
                eip_allocation_id: Some("eipalloc-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        let error = executor
            .step()
            .await
            .expect_err("access denied is returned");
        let source = error
            .source
            .as_deref()
            .expect("the EC2 error is the source");
        assert_eq!(source.code, "REMOTE_ACCESS_DENIED");
        assert_eq!(
            controller(&executor).eip_allocation_id.as_deref(),
            Some("eipalloc-1")
        );
    }

    #[tokio::test]
    async fn elastic_ip_still_in_use_keeps_its_id_and_polls() {
        let mut ec2 = MockEc2Api::new();
        let releases = Arc::new(AtomicUsize::new(0));
        let count = releases.clone();
        ec2.expect_release_address().returning(move |id| {
            assert_eq!(id, "eipalloc-1");
            if count.fetch_add(1, Ordering::SeqCst) < 2 {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        message: "InvalidIPAddress.InUse".to_string(),
                        resource_type: "ElasticIP".to_string(),
                        resource_name: id.to_string(),
                    },
                ))
            } else {
                Ok(())
            }
        });

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::ReleasingElasticIp,
                vpc_id: Some("vpc-1".to_string()),
                eip_allocation_id: Some("eipalloc-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        for _ in 0..2 {
            let result = executor
                .step()
                .await
                .expect("in-use is a wait, not a failure");
            assert_eq!(result.suggested_delay, Some(Duration::from_secs(15)));
            assert_eq!(
                controller(&executor).state,
                AwsNetworkState::ReleasingElasticIp
            );
            assert_eq!(
                controller(&executor).eip_allocation_id.as_deref(),
                Some("eipalloc-1")
            );
        }
        executor.step().await.expect("release succeeds");
        assert_eq!(controller(&executor).eip_allocation_id, None);
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::DeletingSecurityGroup
        );
    }

    #[tokio::test]
    async fn subnet_in_use_stays_in_state_while_the_others_are_forgotten() {
        let mut ec2 = MockEc2Api::new();
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = calls.clone();
        ec2.expect_delete_subnet().returning(move |id| {
            let mut log = log.lock().unwrap();
            let first_try_of_b = id == "subnet-b" && !log.iter().any(|c| c == "subnet-b");
            log.push(id.to_string());
            if first_try_of_b {
                Err(in_use())
            } else {
                Ok(())
            }
        });
        ec2.expect_describe_network_interfaces()
            .times(1)
            .withf(|request| {
                request.filters.as_ref().is_some_and(|filters| {
                    filters[0].name == "subnet-id" && filters[0].values == ["subnet-b"]
                })
            })
            .returning(|_| Ok(parse(json!({}))));

        let mut subnets_by_failure_domain = BTreeMap::new();
        subnets_by_failure_domain.insert(
            "us-east-1a".to_string(),
            AwsFailureDomainSubnets {
                public_subnet_ids: vec!["subnet-a".to_string()],
                private_subnet_ids: vec!["subnet-c".to_string()],
            },
        );
        subnets_by_failure_domain.insert(
            "us-east-1b".to_string(),
            AwsFailureDomainSubnets {
                public_subnet_ids: vec!["subnet-b".to_string()],
                private_subnet_ids: vec![],
            },
        );
        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::DeletingSubnets,
                vpc_id: Some("vpc-1".to_string()),
                public_subnet_ids: vec!["subnet-a".to_string(), "subnet-b".to_string()],
                private_subnet_ids: vec!["subnet-c".to_string()],
                subnets_by_failure_domain,
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        let result = executor.step().await.expect("in-use subnet is a wait");
        assert_eq!(result.suggested_delay, Some(Duration::from_secs(30)));
        let state = controller(&executor);
        assert_eq!(state.state, AwsNetworkState::DeletingSubnets);
        assert_eq!(state.public_subnet_ids, ["subnet-b"]);
        assert!(state.private_subnet_ids.is_empty());
        assert_eq!(
            state.subnets_by_failure_domain.keys().collect::<Vec<_>>(),
            ["us-east-1b"]
        );

        executor.step().await.expect("subnet-b drains");
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::DeletingRouteTables
        );
        assert!(controller(&executor).public_subnet_ids.is_empty());
        assert_eq!(
            *calls.lock().unwrap(),
            ["subnet-a", "subnet-b", "subnet-c", "subnet-b"]
        );
    }

    #[tokio::test]
    async fn vpc_with_dependencies_polls_then_fails_naming_them_with_its_id_still_recorded() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_delete_vpc()
            .times(DEPENDENCY_DRAIN_MAX_POLLS as usize)
            .returning(|_| Err(in_use()));
        ec2.expect_describe_network_interfaces()
            .returning(|_| Ok(interfaces(json!([busy_lambda_interface()]))));
        ec2.expect_describe_security_groups().returning(|_| {
            Ok(parse(json!({ "securityGroupInfo": { "item": [
                { "groupId": "sg-default", "groupName": "default" },
                { "groupId": "sg-stray", "groupName": "someone-elses" }
            ]}})))
        });
        ec2.expect_delete_network_interface().times(0);

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::DeletingVpc,
                vpc_id: Some("vpc-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        for _ in 1..DEPENDENCY_DRAIN_MAX_POLLS {
            executor.step().await.expect("dependency drain is a wait");
            assert_eq!(executor.status(), ResourceStatus::Deleting);
        }
        let error = executor
            .step()
            .await
            .expect_err("the poll budget is bounded");
        assert_eq!(error.code, "RESOURCE_DELETE_BLOCKED");
        assert!(!error.retryable);
        assert!(error.message.contains("VPC 'vpc-1'"), "{}", error.message);
        assert!(
            error.message.contains("eni-busy")
                && error.message.contains("in-use")
                && error.message.contains("AWS Lambda VPC ENI-fn")
                && error.message.contains("requested by 123456789012:fn"),
            "{}",
            error.message
        );
        assert!(error.message.contains("sg-stray"), "{}", error.message);
        assert!(!error.message.contains("sg-default"), "{}", error.message);
        assert_eq!(controller(&executor).vpc_id.as_deref(), Some("vpc-1"));

        // A manual retry resets the poll count, so the delete waits again instead of failing.
        let mut failed = controller(&executor).clone();
        failed.transition_to_failure();
        assert_eq!(failed.state, AwsNetworkState::DeleteFailed);
        assert_eq!(failed.vpc_id.as_deref(), Some("vpc-1"));
        let mut resumed = controller(&executor).clone();
        resumed.reset_stay_count();
        assert_eq!(resumed.wait_for_delete_dependencies_iterations, 0);
    }

    fn interfaces(items: serde_json::Value) -> DescribeNetworkInterfacesResponse {
        parse(json!({ "networkInterfaceSet": { "item": items } }))
    }

    fn busy_lambda_interface() -> serde_json::Value {
        json!({
            "networkInterfaceId": "eni-busy",
            "status": "in-use",
            "interfaceType": "lambda",
            "description": "AWS Lambda VPC ENI-fn",
            "requesterId": "123456789012:fn",
            "requesterManaged": true
        })
    }

    fn orphaned_lambda_interface() -> serde_json::Value {
        json!({
            "networkInterfaceId": "eni-orphan",
            "status": "available",
            "interfaceType": "lambda",
            "description": "AWS Lambda VPC ENI-gone-fn",
            "requesterId": "123456789012:gone-fn",
            "requesterManaged": true
        })
    }

    /// Lambda can leave its interfaces `available` (detached) after the function is gone; they
    /// never go away on their own. The security group step deletes them, leaves the ones in use
    /// alone, and waits.
    #[tokio::test]
    async fn detached_interfaces_holding_the_security_group_are_deleted_before_waiting() {
        let mut ec2 = MockEc2Api::new();
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        ec2.expect_delete_security_group().returning(move |id| {
            assert_eq!(id, "sg-1");
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(in_use())
            } else {
                Ok(())
            }
        });
        ec2.expect_describe_network_interfaces()
            .times(1)
            .withf(|request| {
                request.filters.as_ref().is_some_and(|filters| {
                    filters[0].name == "group-id" && filters[0].values == ["sg-1"]
                })
            })
            .returning(|_| {
                Ok(interfaces(json!([
                    orphaned_lambda_interface(),
                    busy_lambda_interface()
                ])))
            });
        ec2.expect_delete_network_interface()
            .times(1)
            .withf(|id| id == "eni-orphan")
            .returning(|_| Ok(()));

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::DeletingSecurityGroup,
                vpc_id: Some("vpc-1".to_string()),
                security_group_id: Some("sg-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        let result = executor.step().await.expect("an in-use group is a wait");
        assert_eq!(result.suggested_delay, Some(Duration::from_secs(30)));
        assert_eq!(
            controller(&executor).security_group_id.as_deref(),
            Some("sg-1")
        );
        assert_eq!(
            controller(&executor).wait_for_delete_dependencies_iterations,
            1
        );

        executor.step().await.expect("the group is deleted");
        let state = controller(&executor);
        assert_eq!(state.state, AwsNetworkState::DeletingSubnets);
        assert_eq!(state.security_group_id, None);
        assert_eq!(state.wait_for_delete_dependencies_iterations, 0);
    }

    /// A role set up before it could delete network interfaces cannot remove a detached one, so
    /// waiting cannot help. The delete fails at once with the interface named and what to do,
    /// and the error carries no access-denied cause, so it is not taken as "already deleted".
    #[tokio::test]
    async fn a_detached_interface_the_role_cannot_delete_fails_the_delete_at_once() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_delete_security_group()
            .times(1)
            .returning(|_| Err(in_use()));
        ec2.expect_describe_network_interfaces()
            .returning(|_| Ok(interfaces(json!([orphaned_lambda_interface()]))));
        ec2.expect_delete_network_interface()
            .times(1)
            .returning(|id| {
                Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "NetworkInterface".to_string(),
                    resource_name: id.to_string(),
                }))
            });

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::DeletingSecurityGroup,
                vpc_id: Some("vpc-1".to_string()),
                security_group_id: Some("sg-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        let error = executor.step().await.expect_err("waiting cannot help");
        assert_eq!(error.code, "RESOURCE_DELETE_BLOCKED");
        assert!(
            error.message.contains("eni-orphan")
                && error.message.contains("ec2:DeleteNetworkInterface")
                && error.message.contains("rerun setup"),
            "{}",
            error.message
        );
        assert!(error.source.is_none(), "no access-denied cause: {error:?}");
        assert_eq!(
            controller(&executor).security_group_id.as_deref(),
            Some("sg-1")
        );
    }

    /// A role that cannot list network interfaces keeps waiting, because the interfaces may
    /// still drain on their own; the denial does not end the delete.
    #[tokio::test]
    async fn a_role_that_cannot_list_interfaces_keeps_waiting() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_delete_security_group()
            .times(1)
            .returning(|_| Err(in_use()));
        ec2.expect_describe_network_interfaces()
            .times(1)
            .returning(|_| {
                Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "NetworkInterface".to_string(),
                    resource_name: "*".to_string(),
                }))
            });
        ec2.expect_delete_network_interface().times(0);

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::DeletingSecurityGroup,
                vpc_id: Some("vpc-1".to_string()),
                security_group_id: Some("sg-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        executor.step().await.expect("still a wait");
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::DeletingSecurityGroup
        );
        assert_eq!(
            controller(&executor).security_group_id.as_deref(),
            Some("sg-1")
        );
    }

    /// A delete checkpoint saved before the poll counter existed resumes polling; the old
    /// stay budget does not fail it at once.
    #[tokio::test]
    async fn persisted_deleting_vpc_mid_poll_keeps_polling() {
        let mut value =
            serde_json::to_value(AwsNetworkController::default()).expect("serialize default");
        let fields = value.as_object_mut().expect("object");
        assert!(fields
            .remove("waitForDeleteDependenciesIterations")
            .is_some());
        fields.insert("state".to_string(), json!("deletingVpc"));
        fields.insert("vpcId".to_string(), json!("vpc-1"));
        let mut controller_state: AwsNetworkController =
            serde_json::from_value(value).expect("an old checkpoint deserializes");
        // Near the end of the old stay budget (40 polls).
        controller_state._internal_stay_count = Some(39);
        let mut ec2 = MockEc2Api::new();
        ec2.expect_delete_vpc()
            .times(1)
            .returning(|_| Err(in_use()));
        ec2.expect_describe_network_interfaces()
            .returning(|_| Ok(interfaces(json!([]))));
        ec2.expect_describe_security_groups()
            .returning(|_| Ok(parse(json!({}))));

        let mut executor = executor(ec2, controller_state, Some("10.0.0.0/16")).await;
        executor.step().await.expect("still polling");
        assert_eq!(controller(&executor).state, AwsNetworkState::DeletingVpc);
        assert_eq!(
            controller(&executor).wait_for_delete_dependencies_iterations,
            1
        );
    }

    #[tokio::test]
    async fn not_found_from_every_child_walks_the_whole_chain_to_deleted() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_delete_nat_gateway()
            .times(1)
            .returning(|_| Err(not_found()));
        ec2.expect_describe_nat_gateways().times(0);
        ec2.expect_release_address()
            .times(1)
            .returning(|_| Err(not_found()));
        ec2.expect_delete_security_group()
            .times(1)
            .returning(|_| Err(not_found()));
        ec2.expect_delete_subnet()
            .times(4)
            .returning(|_| Err(not_found()));
        ec2.expect_disassociate_route_table()
            .times(1)
            .returning(|_| Err(not_found()));
        ec2.expect_delete_route_table()
            .times(2)
            .returning(|_| Err(not_found()));
        ec2.expect_detach_internet_gateway()
            .times(1)
            .returning(|_| Err(not_found()));
        ec2.expect_delete_internet_gateway()
            .times(1)
            .returning(|_| Err(not_found()));
        ec2.expect_delete_vpc()
            .times(1)
            .returning(|_| Err(not_found()));

        let mut ready = AwsNetworkController::mock_ready("vpc-1", 2);
        ready.route_table_association_ids = vec!["rtbassoc-1".to_string()];
        let mut executor = executor(ec2, ready, Some("10.0.0.0/16")).await;

        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");

        assert_eq!(executor.status(), ResourceStatus::Deleted);
        let state = controller(&executor);
        assert_eq!(state.vpc_id, None);
        assert_eq!(state.nat_gateway_id, None);
        assert_eq!(state.eip_allocation_id, None);
        assert_eq!(state.security_group_id, None);
        assert_eq!(state.internet_gateway_id, None);
        assert_eq!(state.public_route_table_id, None);
        assert_eq!(state.private_route_table_id, None);
        assert!(state.public_subnet_ids.is_empty());
        assert!(state.private_subnet_ids.is_empty());
        assert!(state.route_table_association_ids.is_empty());
    }

    #[tokio::test]
    async fn internet_gateway_that_was_never_attached_is_still_deleted() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_detach_internet_gateway()
            .times(1)
            .returning(|_| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        message: "resource igw-1 is not attached to network vpc-1".to_string(),
                        resource_type: "InternetGateway".to_string(),
                        resource_name: "igw-1".to_string(),
                    },
                ))
            });
        ec2.expect_delete_internet_gateway()
            .times(1)
            .withf(|id| id == "igw-1")
            .returning(|_| Ok(()));

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::DeletingInternetGateway,
                vpc_id: Some("vpc-1".to_string()),
                internet_gateway_id: Some("igw-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        executor
            .step()
            .await
            .expect("not-attached is not a failure");
        assert_eq!(controller(&executor).state, AwsNetworkState::DeletingVpc);
        assert_eq!(controller(&executor).internet_gateway_id, None);
    }

    #[tokio::test]
    async fn unexpected_delete_error_is_propagated_and_keeps_the_id() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_delete_security_group()
            .times(1)
            .returning(|_| Err(unavailable()));

        let mut executor = executor(
            ec2,
            AwsNetworkController {
                state: AwsNetworkState::DeletingSecurityGroup,
                vpc_id: Some("vpc-1".to_string()),
                security_group_id: Some("sg-1".to_string()),
                ..Default::default()
            },
            Some("10.0.0.0/16"),
        )
        .await;

        let error = executor
            .step()
            .await
            .expect_err("5xx must not be swallowed");
        assert!(error.retryable, "the executor retries transient failures");
        assert_eq!(
            controller(&executor).security_group_id.as_deref(),
            Some("sg-1")
        );
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::DeletingSecurityGroup
        );
    }

    // ─────────────── Persisted state ───────────────

    fn persisted(fields: serde_json::Value) -> AwsNetworkController {
        let mut value =
            serde_json::to_value(AwsNetworkController::default()).expect("serialize default");
        for (key, field) in fields.as_object().expect("object").clone() {
            value[key] = field;
        }
        serde_json::from_value(value).expect("persisted controller should deserialize")
    }

    #[tokio::test]
    async fn persisted_creating_vpc_with_a_recorded_vpc_does_not_create_another() {
        // Written by a version that recorded the VPC and then failed to list availability
        // zones in the same handler.
        let controller_state = persisted(json!({
            "state": "creatingVpc",
            "vpcId": "vpc-old",
            "cidrBlock": "10.0.0.0/16",
            "availabilityZones": []
        }));
        let mut ec2 = MockEc2Api::new();
        expect_two_zones(&mut ec2);
        ec2.expect_describe_vpcs().times(0);
        ec2.expect_create_vpc().times(0);

        let mut executor = executor(ec2, controller_state, Some("10.0.0.0/16")).await;

        executor.step().await.expect("resume should succeed");
        let state = controller(&executor);
        assert_eq!(state.state, AwsNetworkState::ConfiguringVpcDns);
        assert_eq!(state.vpc_id.as_deref(), Some("vpc-old"));
        assert_eq!(state.availability_zones, ["us-east-1a", "us-east-1b"]);
    }

    #[tokio::test]
    async fn persisted_creating_nat_gateway_without_an_elastic_ip_allocates_one_first() {
        let controller_state = persisted(json!({
            "state": "creatingNatGateway",
            "vpcId": "vpc-1",
            "publicSubnetIds": ["subnet-pub-a"]
        }));
        let mut ec2 = MockEc2Api::new();
        ec2.expect_allocate_address()
            .times(1)
            .returning(|_| Ok(parse(json!({ "allocationId": "eipalloc-1" }))));
        ec2.expect_create_nat_gateway()
            .times(1)
            .returning(|_| Ok(parse(json!({ "natGateway": { "natGatewayId": "nat-1" } }))));

        let mut executor = executor(ec2, controller_state, Some("10.0.0.0/16")).await;

        executor.step().await.expect("route to allocation");
        assert_eq!(
            controller(&executor).state,
            AwsNetworkState::AllocatingElasticIp
        );
        executor.step().await.expect("allocate");
        executor.step().await.expect("create NAT");
        assert_eq!(
            controller(&executor).nat_gateway_id.as_deref(),
            Some("nat-1")
        );
        assert_eq!(
            controller(&executor).eip_allocation_id.as_deref(),
            Some("eipalloc-1")
        );
    }

    #[tokio::test]
    async fn persisted_deleting_internet_gateway_still_detaches_before_deleting() {
        let controller_state = persisted(json!({
            "state": "deletingInternetGateway",
            "vpcId": "vpc-1",
            "internetGatewayId": "igw-1"
        }));
        let calls = Arc::new(Mutex::new(Vec::<&str>::new()));
        let mut ec2 = MockEc2Api::new();
        let log = calls.clone();
        ec2.expect_detach_internet_gateway()
            .times(1)
            .returning(move |_| {
                log.lock().unwrap().push("detach");
                Ok(())
            });
        let log = calls.clone();
        ec2.expect_delete_internet_gateway()
            .times(1)
            .returning(move |_| {
                log.lock().unwrap().push("delete");
                Ok(())
            });

        let mut executor = executor(ec2, controller_state, Some("10.0.0.0/16")).await;

        executor.step().await.expect("detach and delete");
        assert_eq!(*calls.lock().unwrap(), ["detach", "delete"]);
        assert_eq!(controller(&executor).state, AwsNetworkState::DeletingVpc);
    }

    // ─────────────── Full lifecycle ───────────────

    #[tokio::test]
    async fn full_create_then_delete_leaves_nothing_recorded() {
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_addresses()
            .returning(|| Ok(parse(json!({}))));
        expect_two_zones(&mut ec2);
        // The CIDR is configured and nothing was attempted before, so no VPC lookup runs.
        ec2.expect_describe_vpcs().times(0);
        ec2.expect_create_vpc()
            .times(1)
            .returning(|_| Ok(parse(json!({ "vpc": { "vpcId": "vpc-1" } }))));
        ec2.expect_modify_vpc_attribute()
            .times(2)
            .returning(|_| Ok(()));
        ec2.expect_create_internet_gateway()
            .times(1)
            .returning(|_| {
                Ok(parse(
                    json!({ "internetGateway": { "internetGatewayId": "igw-1" } }),
                ))
            });
        ec2.expect_attach_internet_gateway()
            .times(1)
            .returning(|_| Ok(()));
        // No create attempt is outstanding, so no subnet is looked up before its create.
        ec2.expect_describe_subnets().times(0);
        ec2.expect_create_subnet().times(4).returning(|request| {
            Ok(parse(
                json!({ "subnet": { "subnetId": subnet_id_for(&request.cidr_block) } }),
            ))
        });
        ec2.expect_describe_route_tables()
            .times(2)
            .returning(|_| Ok(parse(json!({}))));
        let route_tables = Arc::new(AtomicUsize::new(0));
        ec2.expect_create_route_table()
            .times(2)
            .returning(move |_| {
                let id = if route_tables.fetch_add(1, Ordering::SeqCst) == 0 {
                    "rtb-pub"
                } else {
                    "rtb-priv"
                };
                Ok(parse(json!({ "routeTable": { "routeTableId": id } })))
            });
        ec2.expect_create_route().times(2).returning(|_| Ok(()));
        let associations = Arc::new(AtomicUsize::new(0));
        ec2.expect_associate_route_table()
            .times(4)
            .returning(move |_| {
                let n = associations.fetch_add(1, Ordering::SeqCst);
                Ok(parse(json!({ "associationId": format!("rtbassoc-{n}") })))
            });
        ec2.expect_allocate_address()
            .times(1)
            .returning(|_| Ok(parse(json!({ "allocationId": "eipalloc-1" }))));
        ec2.expect_create_nat_gateway()
            .times(1)
            .returning(|_| Ok(parse(json!({ "natGateway": { "natGatewayId": "nat-1" } }))));
        let nat_states = Arc::new(Mutex::new(
            vec!["pending", "available", "deleting", "deleted"].into_iter(),
        ));
        ec2.expect_describe_nat_gateways()
            .times(4)
            .returning(move |_| {
                let state = nat_states.lock().unwrap().next().expect("NAT state");
                Ok(parse(json!({ "natGatewaySet": { "item": [{ "natGatewayId": "nat-1", "state": state }] } })))
            });
        ec2.expect_describe_security_groups().returning(|request| {
            if request.group_ids.is_some() {
                Ok(parse(
                    json!({ "securityGroupInfo": { "item": [{ "groupId": "sg-1" }] } }),
                ))
            } else {
                Ok(parse(json!({})))
            }
        });
        ec2.expect_create_security_group()
            .times(1)
            .returning(|_| Ok(parse(json!({ "groupId": "sg-1" }))));
        ec2.expect_authorize_security_group_ingress()
            .times(1)
            .returning(|_| Ok(()));
        ec2.expect_authorize_security_group_egress()
            .times(1)
            .returning(|_| Ok(()));

        let deleted = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = deleted.clone();
        ec2.expect_delete_nat_gateway()
            .times(1)
            .returning(move |id| {
                log.lock().unwrap().push(id.to_string());
                Ok(parse(json!({ "natGatewayId": id })))
            });
        let log = deleted.clone();
        ec2.expect_release_address().times(1).returning(move |id| {
            log.lock().unwrap().push(id.to_string());
            Ok(())
        });
        let log = deleted.clone();
        ec2.expect_delete_security_group()
            .times(1)
            .returning(move |id| {
                log.lock().unwrap().push(id.to_string());
                Ok(())
            });
        let log = deleted.clone();
        ec2.expect_delete_subnet().times(4).returning(move |id| {
            log.lock().unwrap().push(id.to_string());
            Ok(())
        });
        ec2.expect_disassociate_route_table()
            .times(4)
            .returning(|_| Err(not_found()));
        let log = deleted.clone();
        ec2.expect_delete_route_table()
            .times(2)
            .returning(move |id| {
                log.lock().unwrap().push(id.to_string());
                Ok(())
            });
        ec2.expect_detach_internet_gateway()
            .times(1)
            .returning(|_| Ok(()));
        let log = deleted.clone();
        ec2.expect_delete_internet_gateway()
            .times(1)
            .returning(move |id| {
                log.lock().unwrap().push(id.to_string());
                Ok(())
            });
        let log = deleted.clone();
        ec2.expect_delete_vpc().times(1).returning(move |id| {
            log.lock().unwrap().push(id.to_string());
            Ok(())
        });

        let ec2 = Arc::new(ec2);
        let mut quotas = MockServiceQuotasApi::new();
        quotas.expect_get_service_quota().returning(|_, _| {
            Ok(GetServiceQuotaResponse {
                quota: Some(ServiceQuota {
                    quota_code: Some(EC2_VPC_EIP_QUOTA_CODE.to_string()),
                    service_code: Some("ec2".to_string()),
                    quota_name: None,
                    value: Some(5.0),
                }),
            })
        });
        let quotas = Arc::new(quotas);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        provider
            .expect_get_aws_service_quotas_client()
            .returning(move |_| Ok(quotas.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(network(Some("10.0.0.0/16")))
            .controller(AwsNetworkController::default())
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .resource_prefix(PREFIX)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor should build");

        executor
            .run_until_terminal()
            .await
            .expect("create completes");
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_polling_delays(
            &executor.take_suggested_delays(),
            Duration::from_secs(15),
            "create",
        );
        let ready = controller(&executor).clone();
        assert_eq!(ready.vpc_id.as_deref(), Some("vpc-1"));
        assert_eq!(ready.internet_gateway_id.as_deref(), Some("igw-1"));
        assert_eq!(ready.nat_gateway_id.as_deref(), Some("nat-1"));
        assert_eq!(ready.eip_allocation_id.as_deref(), Some("eipalloc-1"));
        assert_eq!(ready.security_group_id.as_deref(), Some("sg-1"));
        assert_eq!(ready.public_subnet_ids, ["subnet-0", "subnet-16"]);
        assert_eq!(ready.private_subnet_ids, ["subnet-128", "subnet-144"]);
        assert_eq!(ready.route_table_association_ids.len(), 4);

        executor.delete().expect("delete transition");
        executor
            .run_until_terminal()
            .await
            .expect("delete completes");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_polling_delays(
            &executor.take_suggested_delays(),
            Duration::from_secs(15),
            "delete",
        );
        assert_eq!(
            *deleted.lock().unwrap(),
            [
                "nat-1",
                "eipalloc-1",
                "sg-1",
                "subnet-0",
                "subnet-16",
                "subnet-128",
                "subnet-144",
                "rtb-pub",
                "rtb-priv",
                "igw-1",
                "vpc-1",
            ]
        );
        let state = controller(&executor);
        assert_eq!(state.vpc_id, None);
        assert!(state.route_table_association_ids.is_empty());
    }
}
