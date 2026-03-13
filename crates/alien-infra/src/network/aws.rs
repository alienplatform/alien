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
    DeleteRouteRequest, DescribeAvailabilityZonesRequest, DescribeNatGatewaysRequest,
    DescribeRouteTablesRequest, DescribeSubnetsRequest, DescribeVpcsRequest,
    DetachInternetGatewayRequest, Ec2Api, Filter, IpPermission, IpRange, ModifyVpcAttributeRequest,
    Tag, TagSpecification,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Network, NetworkOutputs, NetworkSettings, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashSet;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult};
use crate::error::{ErrorData, Result};

/// AWS Network Controller state machine.
///
/// This controller manages the lifecycle of AWS VPC networking infrastructure.
#[controller]
pub struct AwsNetworkController {
    // VPC resources
    pub(crate) vpc_id: Option<String>,
    pub(crate) cidr_block: Option<String>,

    // Internet Gateway
    pub(crate) internet_gateway_id: Option<String>,

    // NAT Gateway
    pub(crate) nat_gateway_id: Option<String>,
    pub(crate) eip_allocation_id: Option<String>,

    // Subnets
    pub(crate) public_subnet_ids: Vec<String>,
    pub(crate) private_subnet_ids: Vec<String>,

    // Route Tables
    pub(crate) public_route_table_id: Option<String>,
    pub(crate) private_route_table_id: Option<String>,
    pub(crate) route_table_association_ids: Vec<String>,

    // Security Group
    pub(crate) security_group_id: Option<String>,

    // Metadata
    pub(crate) availability_zones: Vec<String>,
    pub(crate) is_byo_vpc: bool,
}

impl AwsNetworkController {
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;

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

    /// Create tags for AWS resources.
    fn create_tags(&self, resource_prefix: &str, resource_type: &str) -> Vec<TagSpecification> {
        vec![TagSpecification {
            resource_type: resource_type.to_string(),
            tags: vec![
                Tag {
                    key: "Name".to_string(),
                    value: format!("{}-{}", resource_prefix, resource_type.to_lowercase()),
                },
                Tag {
                    key: "ManagedBy".to_string(),
                    value: "Alien".to_string(),
                },
            ],
        }]
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
                let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_config)?;

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

                let subnet_ids: Vec<String> = subnets_response
                    .subnet_set
                    .map(|set| set.items.into_iter().filter_map(|s| s.subnet_id).collect())
                    .unwrap_or_default();

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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
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
        let create_response = client
            .create_vpc(
                CreateVpcRequest::builder()
                    .cidr_block(vpc_cidr.clone())
                    .tag_specifications(self.create_tags(ctx.resource_prefix, "vpc"))
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
        let config = ctx.desired_resource_config::<Network>()?;

        let vpc_id = self.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!("Creating Internet Gateway");

        // Create Internet Gateway
        let igw_response = client
            .create_internet_gateway(
                CreateInternetGatewayRequest::builder()
                    .tag_specifications(self.create_tags(ctx.resource_prefix, "internet-gateway"))
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

        info!(igw_id = %igw_id, "Internet Gateway created, attaching to VPC");

        // Attach Internet Gateway to VPC
        client
            .attach_internet_gateway(
                AttachInternetGatewayRequest::builder()
                    .internet_gateway_id(igw_id.clone())
                    .vpc_id(vpc_id.clone())
                    .build(),
            )
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
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

            let subnet_response = client
                .create_subnet(
                    CreateSubnetRequest::builder()
                        .vpc_id(vpc_id.clone())
                        .cidr_block(cidr.clone())
                        .availability_zone(az.clone())
                        .tag_specifications(vec![TagSpecification {
                            resource_type: "subnet".to_string(),
                            tags: vec![
                                Tag {
                                    key: "Name".to_string(),
                                    value: subnet_name,
                                },
                                Tag {
                                    key: "ManagedBy".to_string(),
                                    value: "Alien".to_string(),
                                },
                                Tag {
                                    key: "Type".to_string(),
                                    value: "Public".to_string(),
                                },
                            ],
                        }])
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create public subnet in {}", az),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(subnet_id) = subnet_response.subnet.and_then(|s| s.subnet_id) {
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

            let subnet_response = client
                .create_subnet(
                    CreateSubnetRequest::builder()
                        .vpc_id(vpc_id.clone())
                        .cidr_block(cidr.clone())
                        .availability_zone(az.clone())
                        .tag_specifications(vec![TagSpecification {
                            resource_type: "subnet".to_string(),
                            tags: vec![
                                Tag {
                                    key: "Name".to_string(),
                                    value: subnet_name,
                                },
                                Tag {
                                    key: "ManagedBy".to_string(),
                                    value: "Alien".to_string(),
                                },
                                Tag {
                                    key: "Type".to_string(),
                                    value: "Private".to_string(),
                                },
                            ],
                        }])
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create private subnet in {}", az),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(subnet_id) = subnet_response.subnet.and_then(|s| s.subnet_id) {
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
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
                    .tag_specifications(vec![TagSpecification {
                        resource_type: "route-table".to_string(),
                        tags: vec![
                            Tag {
                                key: "Name".to_string(),
                                value: format!("{}-public-rt", ctx.resource_prefix),
                            },
                            Tag {
                                key: "ManagedBy".to_string(),
                                value: "Alien".to_string(),
                            },
                        ],
                    }])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create public route table".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let public_rt_id = public_rt_response
            .route_table
            .and_then(|rt| rt.route_table_id)
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
                    .build(),
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
                self.route_table_association_ids.push(assoc_id);
            }
        }

        self.public_route_table_id = Some(public_rt_id);

        info!("Creating private route table");

        // Create private route table
        let private_rt_response = client
            .create_route_table(
                CreateRouteTableRequest::builder()
                    .vpc_id(vpc_id.clone())
                    .tag_specifications(vec![TagSpecification {
                        resource_type: "route-table".to_string(),
                        tags: vec![
                            Tag {
                                key: "Name".to_string(),
                                value: format!("{}-private-rt", ctx.resource_prefix),
                            },
                            Tag {
                                key: "ManagedBy".to_string(),
                                value: "Alien".to_string(),
                            },
                        ],
                    }])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create private route table".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let private_rt_id = private_rt_response
            .route_table
            .and_then(|rt| rt.route_table_id)
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
                self.route_table_association_ids.push(assoc_id);
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
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
                    .domain("vpc".to_string())
                    .tag_specifications(self.create_tags(ctx.resource_prefix, "elastic-ip"))
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

        self.eip_allocation_id = Some(allocation_id.clone());

        info!(allocation_id = %allocation_id, "Creating NAT Gateway");

        // Create NAT Gateway
        let nat_response = client
            .create_nat_gateway(
                CreateNatGatewayRequest::builder()
                    .subnet_id(public_subnet_id.clone())
                    .allocation_id(allocation_id)
                    .connectivity_type("public".to_string())
                    .tag_specifications(self.create_tags(ctx.resource_prefix, "natgateway"))
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
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

                client
                    .create_route(
                        CreateRouteRequest::builder()
                            .route_table_id(private_rt_id.clone())
                            .destination_cidr_block("0.0.0.0/0".to_string())
                            .nat_gateway_id(nat_gateway_id.clone())
                            .build(),
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
                    message: format!("NAT Gateway in failed state: {:?}", nat_gateway.state),
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
        let config = ctx.desired_resource_config::<Network>()?;

        let vpc_id = self.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!("Creating security group");

        // Create security group
        let sg_response = client
            .create_security_group(
                CreateSecurityGroupRequest::builder()
                    .group_name(format!("{}-sg", ctx.resource_prefix))
                    .description(
                        "Alien managed security group for VPC internal communication".to_string(),
                    )
                    .vpc_id(vpc_id.clone())
                    .tag_specifications(self.create_tags(ctx.resource_prefix, "security-group"))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create security group".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let sg_id = sg_response.group_id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Security group created but no ID returned".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(sg_id = %sg_id, "Security group created, adding ingress rules");

        // Add ingress rule: allow all traffic from within the VPC
        let cidr_block = self.cidr_block.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "CIDR block not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        client
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
            .context(ErrorData::CloudPlatformError {
                message: "Failed to add ingress rule to security group".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        // Add egress rule: allow all outbound traffic (default rule, but we make it explicit)
        client
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
            .ok(); // Ignore error if egress rule already exists (default rule)

        self.security_group_id = Some(sg_id);

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
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(60)),
            });
        }

        // For created VPCs, verify VPC still exists
        if let Some(vpc_id) = &self.vpc_id {
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;

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

            if vpc_response
                .vpc_set
                .map(|set| set.items)
                .unwrap_or_default()
                .is_empty()
            {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "VPC no longer exists".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }

            debug!(vpc_id = %vpc_id, "VPC exists and is accessible");
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

        // For BYO-VPC, nothing to delete
        if self.is_byo_vpc {
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
        let config = ctx.desired_resource_config::<Network>()?;

        // Delete NAT Gateway if it exists
        if let Some(nat_gateway_id) = &self.nat_gateway_id {
            info!(nat_gateway_id = %nat_gateway_id, "Deleting NAT Gateway");

            match client.delete_nat_gateway(nat_gateway_id).await {
                Ok(_) => {
                    info!(nat_gateway_id = %nat_gateway_id, "NAT Gateway deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
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
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;

        // Delete security group if it exists
        if let Some(sg_id) = &self.security_group_id {
            info!(sg_id = %sg_id, "Deleting security group");

            match client.delete_security_group(sg_id).await {
                Ok(_) => {
                    info!(sg_id = %sg_id, "Security group deleted");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;

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
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;

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
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
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
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;

        // Detach and delete Internet Gateway
        if let Some(igw_id) = &self.internet_gateway_id {
            if let Some(vpc_id) = &self.vpc_id {
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
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
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
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;

        // Delete VPC
        if let Some(vpc_id) = &self.vpc_id {
            info!(vpc_id = %vpc_id, "Deleting VPC");

            match client.delete_vpc(vpc_id).await {
                Ok(_) => {
                    info!(vpc_id = %vpc_id, "VPC deleted");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
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

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        // Network doesn't have bindings - other resources access it via require_dependency
        None
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
