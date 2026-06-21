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

use crate::aws_sdk::{
    AllocateAddressRequest, AssociateRouteTableRequest, AttachInternetGatewayRequest,
    AuthorizeSecurityGroupEgressRequest, AuthorizeSecurityGroupIngressRequest, ConnectivityType,
    CreateInternetGatewayRequest, CreateNatGatewayRequest, CreateRouteRequest,
    CreateRouteTableRequest, CreateSecurityGroupRequest, CreateSubnetRequest, CreateVpcRequest,
    DeleteNatGatewayRequest, DescribeAvailabilityZonesRequest, DescribeNatGatewaysRequest,
    DescribeSecurityGroupsRequest, DescribeSubnetsRequest, DescribeVpcsRequest,
    DetachInternetGatewayRequest, DomainType, Ec2ResourceType, Ec2Tag as Tag, Filter, IpPermission,
    IpRange, ModifyVpcAttributeRequest, SecurityGroup, TagSpecification,
};
use alien_core::{
    standard_resource_tags, AwsVpcNetworkHeartbeatData, HeartbeatBackend, Network,
    NetworkHeartbeatData, NetworkHeartbeatStatus, NetworkOutputs, NetworkSettings, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;
use std::collections::HashSet;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

fn is_security_group_duplicate(error: &AlienError<ErrorData>) -> bool {
    matches!(
        &error.error,
        Some(ErrorData::CloudResourceConflict { message, .. })
            if message.contains("InvalidGroup.Duplicate")
                || message.contains("already exists")
    )
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

fn is_security_group_rule_duplicate(error: &AlienError<ErrorData>) -> bool {
    matches!(
        &error.error,
        Some(ErrorData::CloudResourceConflict { message, .. })
            if message.contains("InvalidPermission.Duplicate")
                || message.contains("specified rule")
                || message.contains("already exists")
    )
}

fn has_ipv4_all_protocol_rule(permissions: Option<&[IpPermission]>, cidr_block: &str) -> bool {
    permissions.unwrap_or_default().iter().any(|permission| {
        permission.ip_protocol() == Some("-1")
            && permission
                .ip_ranges()
                .iter()
                .any(|range| range.cidr_ip() == Some(cidr_block))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_existing_all_protocol_ipv4_rule() {
        let permissions = [IpPermission::builder()
            .ip_protocol("-1")
            .ip_ranges(IpRange::builder().cidr_ip("10.0.0.0/16").build())
            .build()];

        assert!(has_ipv4_all_protocol_rule(
            Some(&permissions),
            "10.0.0.0/16"
        ));
    }

    #[test]
    fn ignores_rules_for_other_cidrs_or_protocols() {
        let permissions = [
            IpPermission::builder()
                .ip_protocol("tcp")
                .from_port(443)
                .to_port(443)
                .ip_ranges(IpRange::builder().cidr_ip("10.0.0.0/16").build())
                .build(),
            IpPermission::builder()
                .ip_protocol("-1")
                .ip_ranges(IpRange::builder().cidr_ip("192.168.0.0/16").build())
                .build(),
        ];

        assert!(!has_ipv4_all_protocol_rule(
            Some(&permissions),
            "10.0.0.0/16"
        ));
    }

    #[test]
    fn handles_empty_permission_sets() {
        let permissions: [IpPermission; 0] = [];

        assert!(!has_ipv4_all_protocol_rule(Some(&permissions), "0.0.0.0/0"));
        assert!(!has_ipv4_all_protocol_rule(None, "0.0.0.0/0"));
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

    // Security Group
    pub(crate) security_group_id: Option<String>,

    // Metadata
    pub(crate) availability_zones: Vec<String>,
    pub is_byo_vpc: bool,
}

impl AwsNetworkController {
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
                    .set_filters(Some(vec![
                        Filter::builder().name("vpc-id").values(vpc_id).build(),
                        Filter::builder()
                            .name("group-name")
                            .values(group_name)
                            .build(),
                    ]))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build security group lookup request".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })?,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe existing security group".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;

        Ok(response
            .security_groups()
            .iter()
            .find_map(|sg| sg.group_id().map(ToString::to_string)))
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
                    .group_ids(group_id.to_string())
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build security group describe request".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })?,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe security group".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;

        response.security_groups().first().cloned().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Security group '{group_id}' was not found"),
                resource_id: Some(resource_id.to_string()),
            })
        })
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
        let describe_vpcs_request = DescribeVpcsRequest::builder()
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to build DescribeVpcs request for CIDR allocation".to_string(),
                resource_id: Some(stack_id.to_string()),
            })?;

        let existing_vpcs = client.describe_vpcs(describe_vpcs_request).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to describe existing VPCs for CIDR allocation".to_string(),
                resource_id: Some(stack_id.to_string()),
            },
        )?;

        let used_cidrs: HashSet<String> = existing_vpcs
            .vpcs()
            .iter()
            .filter_map(|vpc| vpc.cidr_block().map(ToString::to_string))
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
        let mut tags = vec![Tag::builder().key("Name").value(name.into()).build()];

        tags.extend(
            standard_resource_tags(resource_prefix, resource_id)
                .into_iter()
                .chain(extra_tags)
                .map(|(key, value)| Tag::builder().key(key).value(value).build()),
        );

        TagSpecification::builder()
            .resource_type(Ec2ResourceType::from(resource_type))
            .set_tags(Some(tags))
            .build()
    }

    /// Create tags for AWS resources.
    fn create_tags(
        &self,
        resource_prefix: &str,
        resource_id: &str,
        resource_type: &str,
    ) -> Vec<TagSpecification> {
        vec![self.create_tag_specification(
            resource_prefix,
            resource_id,
            resource_type,
            format!("{}-{}", resource_prefix, resource_type.to_lowercase()),
            [],
        )]
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

                let describe_vpcs_request = DescribeVpcsRequest::builder()
                    .filters(Filter::builder().name("is-default").values("true").build())
                    .build()
                    .into_alien_error()
                    .context(ErrorData::InfrastructureError {
                        message: "Failed to build default VPC discovery request".to_string(),
                        operation: Some("discover_default_vpc".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                let vpcs_response = ec2_client
                    .describe_vpcs(describe_vpcs_request)
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: "Failed to discover default VPC".to_string(),
                        operation: Some("discover_default_vpc".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                let default_vpc = vpcs_response.vpcs().first().ok_or_else(|| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: "Default VPC not found. It may have been deleted. \
                                      Use NetworkSettings::Create to create an isolated VPC, \
                                      or ByoVpcAws to reference an existing one."
                            .to_string(),
                        operation: Some("discover_default_vpc".to_string()),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

                let vpc_id = default_vpc
                    .vpc_id()
                    .map(ToString::to_string)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::InfrastructureError {
                            message: "Default VPC has no ID".to_string(),
                            operation: Some("discover_default_vpc".to_string()),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;

                // List subnets in the default VPC
                let describe_subnets_request = DescribeSubnetsRequest::builder()
                    .filters(
                        Filter::builder()
                            .name("vpc-id")
                            .values(vpc_id.clone())
                            .build(),
                    )
                    .build()
                    .into_alien_error()
                    .context(ErrorData::InfrastructureError {
                        message: "Failed to build default subnet discovery request".to_string(),
                        operation: Some("discover_default_subnets".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                let subnets_response = ec2_client
                    .describe_subnets(describe_subnets_request)
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: "Failed to list subnets in default VPC".to_string(),
                        operation: Some("discover_default_subnets".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                let subnet_ids: Vec<String> = subnets_response
                    .subnets()
                    .iter()
                    .filter_map(|subnet| subnet.subnet_id().map(ToString::to_string))
                    .collect();

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

                self.availability_zones = (0..self.public_subnet_ids.len())
                    .map(|i| format!("az-{}", i))
                    .collect();

                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            NetworkSettings::Create { .. } => {
                // Continue to VPC creation flow
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

                // Determine availability zones from subnet count
                self.availability_zones = (0..private_subnet_ids.len())
                    .map(|i| format!("az-{}", i))
                    .collect();

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
        state = CreatingVpc,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_vpc(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let (cidr, availability_zones) = match &config.settings {
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

        // Determine CIDR block
        let vpc_cidr = match cidr {
            Some(c) => c,
            None => self.find_available_cidr(ctx, &config.id).await?,
        };

        info!(cidr = %vpc_cidr, "Creating VPC");

        // Create the VPC
        let create_vpc_request = CreateVpcRequest::builder()
            .cidr_block(vpc_cidr.clone())
            .set_tag_specifications(Some(self.create_tags(
                ctx.resource_prefix,
                &config.id,
                "vpc",
            )))
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to build CreateVpc request".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let create_response =
            client
                .create_vpc(create_vpc_request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to create VPC".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

        let vpc_id = create_response
            .vpc()
            .and_then(|vpc| vpc.vpc_id())
            .map(ToString::to_string)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "VPC created but no VPC ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        info!(vpc_id = %vpc_id, "VPC created, enabling DNS support");

        // Enable DNS support
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

        // Enable DNS hostnames
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

        self.vpc_id = Some(vpc_id);
        self.cidr_block = Some(vpc_cidr);

        // Get availability zones
        let az_response = client
            .describe_availability_zones(DescribeAvailabilityZonesRequest::builder().build())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe availability zones".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        // Take the requested number of AZs
        self.availability_zones = az_response
            .availability_zone_info
            .map(|set| set.items)
            .unwrap_or_default()
            .iter()
            .take(availability_zones as usize)
            .filter_map(|az| az.zone_name.clone())
            .collect();

        if self.availability_zones.is_empty() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: "No availability zones found in region".to_string(),
                resource_id: Some(config.id.clone()),
            }));
        }

        info!(azs = ?self.availability_zones, "VPC created successfully, proceeding to create Internet Gateway");

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

        let vpc_id = self.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!("Creating Internet Gateway");

        // Create Internet Gateway
        let create_gateway_request = CreateInternetGatewayRequest::builder()
            .set_tag_specifications(Some(self.create_tags(
                ctx.resource_prefix,
                &config.id,
                "internet-gateway",
            )))
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to build CreateInternetGateway request".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let igw_response = client
            .create_internet_gateway(create_gateway_request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Internet Gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let igw_id = igw_response
            .internet_gateway()
            .and_then(|igw| igw.internet_gateway_id())
            .map(ToString::to_string)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Internet Gateway created but no ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        info!(igw_id = %igw_id, "Internet Gateway created, attaching to VPC");

        // Attach Internet Gateway to VPC
        let attach_gateway_request = AttachInternetGatewayRequest::builder()
            .internet_gateway_id(igw_id.clone())
            .vpc_id(vpc_id.clone())
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to build AttachInternetGateway request".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        client
            .attach_internet_gateway(attach_gateway_request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to attach Internet Gateway to VPC".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.internet_gateway_id = Some(igw_id);

        info!("Internet Gateway attached, proceeding to create subnets");

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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Network>()?;

        let vpc_id = self.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let cidr_block = self.cidr_block.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "CIDR block not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // TODO: Determine if public subnets are needed based on stack resources
        // For now, always create public subnets
        let include_public = true;

        let (public_cidrs, private_cidrs) =
            self.calculate_subnet_cidrs(cidr_block, self.availability_zones.len(), include_public);

        info!(
            public_cidrs = ?public_cidrs,
            private_cidrs = ?private_cidrs,
            "Creating subnets"
        );

        // Create public subnets
        for (i, (cidr, az)) in public_cidrs
            .iter()
            .zip(&self.availability_zones)
            .enumerate()
        {
            let subnet_name = format!("{}-public-{}", ctx.resource_prefix, i + 1);

            let create_subnet_request = CreateSubnetRequest::builder()
                .vpc_id(vpc_id.clone())
                .cidr_block(cidr.clone())
                .availability_zone(az.clone())
                .set_tag_specifications(Some(vec![self.create_tag_specification(
                    ctx.resource_prefix,
                    &config.id,
                    "subnet",
                    subnet_name,
                    [("Type".to_string(), "Public".to_string())],
                )]))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to build public CreateSubnet request for {az}"),
                    resource_id: Some(config.id.clone()),
                })?;

            let subnet_response = client.create_subnet(create_subnet_request).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to create public subnet in {}", az),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            if let Some(subnet_id) = subnet_response
                .subnet()
                .and_then(|subnet| subnet.subnet_id())
                .map(ToString::to_string)
            {
                self.public_subnet_ids.push(subnet_id);
            }
        }

        // Create private subnets
        for (i, (cidr, az)) in private_cidrs
            .iter()
            .zip(&self.availability_zones)
            .enumerate()
        {
            let subnet_name = format!("{}-private-{}", ctx.resource_prefix, i + 1);

            let create_subnet_request = CreateSubnetRequest::builder()
                .vpc_id(vpc_id.clone())
                .cidr_block(cidr.clone())
                .availability_zone(az.clone())
                .set_tag_specifications(Some(vec![self.create_tag_specification(
                    ctx.resource_prefix,
                    &config.id,
                    "subnet",
                    subnet_name,
                    [("Type".to_string(), "Private".to_string())],
                )]))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to build private CreateSubnet request for {az}"),
                    resource_id: Some(config.id.clone()),
                })?;

            let subnet_response = client.create_subnet(create_subnet_request).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to create private subnet in {}", az),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            if let Some(subnet_id) = subnet_response
                .subnet()
                .and_then(|subnet| subnet.subnet_id())
                .map(ToString::to_string)
            {
                self.private_subnet_ids.push(subnet_id);
            }
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

        info!("Creating public route table");

        // Create public route table
        let public_rt_response = client
            .create_route_table(
                CreateRouteTableRequest::builder()
                    .vpc_id(vpc_id.clone())
                    .set_tag_specifications(Some(vec![self.create_tag_specification(
                        ctx.resource_prefix,
                        &config.id,
                        "route-table",
                        format!("{}-public-rt", ctx.resource_prefix),
                        [],
                    )]))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build public route table creation request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create public route table".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let public_rt_id = public_rt_response
            .route_table()
            .and_then(|rt| rt.route_table_id())
            .map(ToString::to_string)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Public route table created but no ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // Add route to Internet Gateway
        client
            .create_route(
                CreateRouteRequest::builder()
                    .route_table_id(public_rt_id.clone())
                    .destination_cidr_block("0.0.0.0/0".to_string())
                    .gateway_id(igw_id.clone())
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build Internet Gateway route request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create route to Internet Gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        // Associate public subnets with public route table
        for subnet_id in &self.public_subnet_ids {
            let assoc_response = client
                .associate_route_table(
                    AssociateRouteTableRequest::builder()
                        .route_table_id(public_rt_id.clone())
                        .subnet_id(subnet_id.clone())
                        .build()
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to build public route table association request for subnet {}",
                                subnet_id
                            ),
                            resource_id: Some(config.id.clone()),
                        })?,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to associate subnet {} with public route table",
                        subnet_id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(assoc_id) = assoc_response.association_id() {
                self.route_table_association_ids.push(assoc_id.to_string());
            }
        }

        self.public_route_table_id = Some(public_rt_id);

        info!("Creating private route table");

        // Create private route table
        let private_rt_response = client
            .create_route_table(
                CreateRouteTableRequest::builder()
                    .vpc_id(vpc_id.clone())
                    .set_tag_specifications(Some(vec![self.create_tag_specification(
                        ctx.resource_prefix,
                        &config.id,
                        "route-table",
                        format!("{}-private-rt", ctx.resource_prefix),
                        [],
                    )]))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build private route table creation request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create private route table".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let private_rt_id = private_rt_response
            .route_table()
            .and_then(|rt| rt.route_table_id())
            .map(ToString::to_string)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Private route table created but no ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // Associate private subnets with private route table
        for subnet_id in &self.private_subnet_ids {
            let assoc_response = client
                .associate_route_table(
                    AssociateRouteTableRequest::builder()
                        .route_table_id(private_rt_id.clone())
                        .subnet_id(subnet_id.clone())
                        .build()
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to build private route table association request for subnet {}",
                                subnet_id
                            ),
                            resource_id: Some(config.id.clone()),
                        })?,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to associate subnet {} with private route table",
                        subnet_id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(assoc_id) = assoc_response.association_id() {
                self.route_table_association_ids.push(assoc_id.to_string());
            }
        }

        self.private_route_table_id = Some(private_rt_id);

        info!("Route tables created, creating NAT gateway");

        // Create always provisions a NAT gateway for private subnet egress
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

        let public_subnet_id = self.public_subnet_ids.first().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "No public subnet available for NAT Gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!("Allocating Elastic IP for NAT Gateway");

        // Allocate Elastic IP
        let eip_response = client
            .allocate_address(
                AllocateAddressRequest::builder()
                    .domain(DomainType::Vpc)
                    .set_tag_specifications(Some(self.create_tags(
                        ctx.resource_prefix,
                        &config.id,
                        "elastic-ip",
                    )))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build Elastic IP allocation request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to allocate Elastic IP".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let allocation_id = eip_response
            .allocation_id()
            .map(ToString::to_string)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Elastic IP allocated but no allocation ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.eip_allocation_id = Some(allocation_id.clone());

        info!(allocation_id = %allocation_id, "Creating NAT Gateway");

        // Create NAT Gateway
        let nat_response = client
            .create_nat_gateway(
                CreateNatGatewayRequest::builder()
                    .subnet_id(public_subnet_id.clone())
                    .allocation_id(allocation_id)
                    .connectivity_type(ConnectivityType::Public)
                    .set_tag_specifications(Some(self.create_tags(
                        ctx.resource_prefix,
                        &config.id,
                        "natgateway",
                    )))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build NAT Gateway creation request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create NAT Gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let nat_gateway_id = nat_response
            .nat_gateway()
            .and_then(|ng| ng.nat_gateway_id())
            .map(ToString::to_string)
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
                    .nat_gateway_ids(nat_gateway_id.clone())
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build NAT Gateway describe request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe NAT Gateway".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let nat_gateway = nat_response.nat_gateways().first().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "NAT Gateway not found".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        match nat_gateway.state().map(|state| state.as_str()) {
            Some("available") => {
                info!(nat_gateway_id = %nat_gateway_id, "NAT Gateway is available, adding route");

                // Add route to NAT Gateway in private route table
                let private_rt_id = self.private_route_table_id.as_ref().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Private route table ID not set".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

                client
                    .create_route(
                        CreateRouteRequest::builder()
                            .route_table_id(private_rt_id.clone())
                            .destination_cidr_block("0.0.0.0/0".to_string())
                            .nat_gateway_id(nat_gateway_id.clone())
                            .build()
                            .into_alien_error()
                            .context(ErrorData::CloudPlatformError {
                                message: "Failed to build NAT Gateway route request".to_string(),
                                resource_id: Some(config.id.clone()),
                            })?,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to create route to NAT Gateway".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;

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
            Some("failed") | Some("deleted") => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "NAT Gateway in failed state: {:?}",
                        nat_gateway.state().map(|state| state.as_str())
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
                    .set_tag_specifications(Some(self.create_tags(
                        ctx.resource_prefix,
                        &config.id,
                        "security-group",
                    )))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build security group creation request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?,
            )
            .await;

        let sg_id = match sg_result {
            Ok(sg_response) => {
                sg_response
                    .group_id()
                    .map(ToString::to_string)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: "Security group created but no ID returned".to_string(),
                            resource_id: Some(config.id.clone()),
                        })
                    })?
            }
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
        if has_ipv4_all_protocol_rule(Some(security_group.ip_permissions()), cidr_block) {
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
                    .set_ip_permissions(Some(vec![IpPermission::builder()
                        .ip_protocol("-1") // All protocols
                        .ip_ranges(
                            IpRange::builder()
                                .cidr_ip(cidr_block.clone())
                                .description("Allow all traffic from VPC")
                                .build(),
                        )
                        .build()]))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build security group ingress request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?,
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
        if has_ipv4_all_protocol_rule(Some(security_group.ip_permissions_egress()), "0.0.0.0/0") {
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
                    .set_ip_permissions(Some(vec![IpPermission::builder()
                        .ip_protocol("-1")
                        .ip_ranges(
                            IpRange::builder()
                                .cidr_ip("0.0.0.0/0")
                                .description("Allow all outbound traffic")
                                .build(),
                        )
                        .build()]))
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build security group egress request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?,
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

            let describe_vpcs_request = DescribeVpcsRequest::builder()
                .vpc_ids(vpc_id.clone())
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build heartbeat DescribeVpcs request".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            let vpc_response = client.describe_vpcs(describe_vpcs_request).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to verify VPC during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            if vpc_response.vpcs().is_empty() {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "VPC no longer exists".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }

            let vpc_state = vpc_response
                .vpcs()
                .first()
                .and_then(|vpc| vpc.state())
                .map(|state| state.as_str().to_string());
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
            self.vpc_id = Some(vpc_id.clone());
            self.public_subnet_ids = public_subnet_ids.clone();
            self.private_subnet_ids = private_subnet_ids.clone();
            self.security_group_id = security_group_ids.first().cloned();
            self.availability_zones = (0..private_subnet_ids.len())
                .map(|i| format!("az-{}", i))
                .collect();

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
        let _config = ctx.desired_resource_config::<Network>()?;

        // Delete NAT Gateway if it exists
        if let Some(nat_gateway_id) = &self.nat_gateway_id {
            info!(nat_gateway_id = %nat_gateway_id, "Deleting NAT Gateway");

            let delete_request = DeleteNatGatewayRequest::builder()
                .nat_gateway_id(nat_gateway_id.clone())
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build NAT Gateway deletion request".to_string(),
                    resource_id: None,
                })?;

            match client.delete_nat_gateway(delete_request).await {
                Ok(_) => {
                    info!(nat_gateway_id = %nat_gateway_id, "NAT Gateway deletion initiated");
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    warn!(nat_gateway_id = %nat_gateway_id, "NAT Gateway already deleted");
                }
                Err(e) => {
                    warn!(nat_gateway_id = %nat_gateway_id, error = ?e, "Failed to delete NAT Gateway, continuing");
                }
            }

            self.nat_gateway_id = None;
        }

        // Release Elastic IP if it exists
        if let Some(allocation_id) = &self.eip_allocation_id {
            info!(allocation_id = %allocation_id, "Releasing Elastic IP");

            match client.release_address(allocation_id).await {
                Ok(_) => {
                    info!(allocation_id = %allocation_id, "Elastic IP released");
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    warn!(allocation_id = %allocation_id, "Elastic IP already released");
                }
                Err(e) => {
                    warn!(allocation_id = %allocation_id, error = ?e, "Failed to release Elastic IP, continuing");
                }
            }

            self.eip_allocation_id = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingSecurityGroup,
            suggested_delay: Some(Duration::from_secs(5)),
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

        // Delete security group if it exists
        if let Some(sg_id) = &self.security_group_id {
            info!(sg_id = %sg_id, "Deleting security group");

            match client.delete_security_group(sg_id).await {
                Ok(_) => {
                    info!(sg_id = %sg_id, "Security group deleted");
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    warn!(sg_id = %sg_id, "Security group already deleted");
                }
                Err(e) => {
                    warn!(sg_id = %sg_id, error = ?e, "Failed to delete security group, continuing");
                }
            }

            self.security_group_id = None;
        }

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

        // Delete all subnets
        for subnet_id in self
            .public_subnet_ids
            .drain(..)
            .chain(self.private_subnet_ids.drain(..))
        {
            info!(subnet_id = %subnet_id, "Deleting subnet");

            match client.delete_subnet(&subnet_id).await {
                Ok(_) => {
                    info!(subnet_id = %subnet_id, "Subnet deleted");
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    warn!(subnet_id = %subnet_id, "Subnet already deleted");
                }
                Err(e) => {
                    warn!(subnet_id = %subnet_id, error = ?e, "Failed to delete subnet, continuing");
                }
            }
        }

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

        // Disassociate and delete route tables
        for assoc_id in self.route_table_association_ids.drain(..) {
            info!(assoc_id = %assoc_id, "Disassociating route table");

            match client.disassociate_route_table(&assoc_id).await {
                Ok(_) => {
                    info!(assoc_id = %assoc_id, "Route table disassociated");
                }
                Err(e) => {
                    warn!(assoc_id = %assoc_id, error = ?e, "Failed to disassociate route table, continuing");
                }
            }
        }

        // Delete public route table
        if let Some(rt_id) = self.public_route_table_id.take() {
            info!(rt_id = %rt_id, "Deleting public route table");

            match client.delete_route_table(&rt_id).await {
                Ok(_) => {
                    info!(rt_id = %rt_id, "Public route table deleted");
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    warn!(rt_id = %rt_id, "Public route table already deleted");
                }
                Err(e) => {
                    warn!(rt_id = %rt_id, error = ?e, "Failed to delete public route table, continuing");
                }
            }
        }

        // Delete private route table
        if let Some(rt_id) = self.private_route_table_id.take() {
            info!(rt_id = %rt_id, "Deleting private route table");

            match client.delete_route_table(&rt_id).await {
                Ok(_) => {
                    info!(rt_id = %rt_id, "Private route table deleted");
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    warn!(rt_id = %rt_id, "Private route table already deleted");
                }
                Err(e) => {
                    warn!(rt_id = %rt_id, error = ?e, "Failed to delete private route table, continuing");
                }
            }
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

        // Detach and delete Internet Gateway
        if let Some(igw_id) = &self.internet_gateway_id {
            if let Some(vpc_id) = &self.vpc_id {
                info!(igw_id = %igw_id, vpc_id = %vpc_id, "Detaching Internet Gateway");

                let detach_gateway_request = DetachInternetGatewayRequest::builder()
                    .internet_gateway_id(igw_id.clone())
                    .vpc_id(vpc_id.clone())
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to build DetachInternetGateway request".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;

                match client.detach_internet_gateway(detach_gateway_request).await {
                    Ok(_) => {
                        info!(igw_id = %igw_id, "Internet Gateway detached");
                    }
                    Err(e) => {
                        warn!(igw_id = %igw_id, error = ?e, "Failed to detach Internet Gateway, continuing");
                    }
                }
            }

            info!(igw_id = %igw_id, "Deleting Internet Gateway");

            match client.delete_internet_gateway(igw_id).await {
                Ok(_) => {
                    info!(igw_id = %igw_id, "Internet Gateway deleted");
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    warn!(igw_id = %igw_id, "Internet Gateway already deleted");
                }
                Err(e) => {
                    warn!(igw_id = %igw_id, error = ?e, "Failed to delete Internet Gateway, continuing");
                }
            }

            self.internet_gateway_id = None;
        }

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

        // Delete VPC
        if let Some(vpc_id) = &self.vpc_id {
            info!(vpc_id = %vpc_id, "Deleting VPC");

            match client.delete_vpc(vpc_id).await {
                Ok(_) => {
                    info!(vpc_id = %vpc_id, "VPC deleted");
                }
                Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                    warn!(vpc_id = %vpc_id, "VPC already deleted");
                }
                Err(e) => {
                    warn!(vpc_id = %vpc_id, error = ?e, "Failed to delete VPC, continuing");
                }
            }

            self.vpc_id = None;
        }

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
            is_byo_vpc: false,
            _internal_stay_count: None,
        }
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
            is_byo_vpc: true,
            _internal_stay_count: None,
        }
    }
}
