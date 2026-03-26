//! AWS Container Controller
//!
//! This module implements the AWS-specific controller for managing Container resources.
//! A Container represents a deployable workload that runs on a ContainerCluster.
//!
//! The controller:
//! - Creates ALB target groups for public containers
//! - Creates EBS volumes for persistent storage
//! - Calls Horizon API to create/update/delete containers
//! - Monitors container status via Horizon
//!
//! Container scheduling and replica management is handled by Horizon, not this controller.

use alien_aws_clients::elbv2::CreateTargetGroupRequest;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    CertificateStatus, Container, ContainerCluster, ContainerCode, ContainerOutputs,
    ContainerStatus, DnsRecordStatus, ExposeProtocol, HorizonClusterConfig, ResourceOutputs,
    ResourceRef, ResourceStatus,
};
use alien_error::{AlienError, Context as ContextError, IntoAlienError};
use alien_macros::controller;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::num::NonZeroU64;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::container_cluster::AwsContainerClusterController;
use crate::core::{environment_variables::EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use crate::horizon::{create_horizon_client, horizon_container_status_to_alien};
use crate::network::AwsNetworkController;

/// Tracks an EBS volume created for a stateful container.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EbsVolumeState {
    /// The volume ID (e.g., vol-0abc123...)
    pub volume_id: String,
    /// The availability zone (e.g., us-east-1a)
    pub zone: String,
    /// The ordinal this volume is for (for stateful containers)
    pub ordinal: u32,
    /// Size in GB
    pub size_gb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerEndpoint {
    pub dns_name: String,
    pub hosted_zone_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerState {
    pub arn: String,
    pub listener_arn: Option<String>,
    pub security_group_id: Option<String>,
    pub endpoint: Option<LoadBalancerEndpoint>,
    pub port: u16,
    pub protocol: ExposeProtocol,
}

/// AWS Container Controller state machine.
///
/// This controller manages the lifecycle of containers via Horizon:
/// - Creates ALB/NLB target groups and load balancers for each exposed port
/// - Creates EBS volumes for persistent storage
/// - Creates containers in Horizon when the ContainerCluster is ready
/// - Updates container configuration via Horizon API
/// - Deletes containers from Horizon during cleanup
#[controller]
pub struct AwsContainerController {
    /// Horizon container name (derived from resource ID)
    pub(crate) container_name: Option<String>,

    /// Current status from Horizon
    pub(crate) horizon_status: Option<ContainerStatus>,

    /// Number of running replicas
    pub(crate) current_replicas: u32,

    /// Public URL for the exposed port (if any)
    pub(crate) public_url: Option<String>,

    /// Fully qualified domain name (custom or generated, for HTTP ports only)
    pub(crate) fqdn: Option<String>,

    /// Certificate ID for auto-managed domains (HTTP ports only)
    pub(crate) certificate_id: Option<String>,

    /// ACM certificate ARN (auto-imported or custom, for HTTP ports only)
    pub(crate) certificate_arn: Option<String>,

    /// Load balancer state (single exposed port only, enforced by preflight)
    pub(crate) load_balancer: Option<LoadBalancerState>,

    /// Whether this resource uses a customer-managed domain (HTTP ports only)
    pub(crate) uses_custom_domain: bool,

    /// Target group ARN for the exposed port (single port only, enforced by preflight)
    pub(crate) target_group_arn: Option<String>,

    /// EBS volumes created for persistent storage
    pub(crate) ebs_volumes: Vec<EbsVolumeState>,

    /// Timestamp when certificate was imported (for renewal detection, HTTP only)
    pub(crate) certificate_issued_at: Option<String>,

    /// Number of iterations spent waiting for replicas to become healthy
    #[serde(default)]
    pub(crate) wait_for_replicas_iterations: u32,
}

/// Context for interacting with Horizon API for a specific cluster.
struct HorizonContext<'a> {
    /// Cluster configuration (contains cluster_id)
    cluster: &'a HorizonClusterConfig,
    /// Pre-authenticated Horizon client
    client: horizon_client_sdk::Client,
}

impl AwsContainerController {
    /// Get Horizon context for the given cluster.
    /// Returns the cluster config and an authenticated client.
    fn horizon<'a>(
        ctx: &'a ResourceControllerContext<'_>,
        cluster_resource_id: &str,
    ) -> Result<HorizonContext<'a>> {
        let horizon_config = match &ctx.deployment_config.compute_backend {
            Some(alien_core::ComputeBackend::Horizon(h)) => h,
            None => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Container resources require a Horizon compute backend".to_string(),
                    resource_id: Some(cluster_resource_id.to_string()),
                }))
            }
        };

        let cluster = horizon_config
            .clusters
            .get(cluster_resource_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("No Horizon cluster config for '{}'", cluster_resource_id),
                    resource_id: Some(cluster_resource_id.to_string()),
                })
            })?;

        let client = create_horizon_client(&horizon_config.url, &cluster.management_token)
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to create Horizon client: {}", e),
                    resource_id: Some(cluster_resource_id.to_string()),
                })
            })?;

        Ok(HorizonContext { cluster, client })
    }

    /// Parse storage size string (e.g., "100Gi", "500GB") to GB.
    fn parse_storage_size_gb(size: &str) -> Result<u32> {
        let size = size.trim();
        let (num_str, unit) = if size.ends_with("Gi") || size.ends_with("GiB") {
            (size.trim_end_matches("GiB").trim_end_matches("Gi"), "Gi")
        } else if size.ends_with("GB") {
            (size.trim_end_matches("GB"), "GB")
        } else if size.ends_with("Ti") || size.ends_with("TiB") {
            (size.trim_end_matches("TiB").trim_end_matches("Ti"), "Ti")
        } else if size.ends_with("TB") {
            (size.trim_end_matches("TB"), "TB")
        } else {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Invalid storage size format: {}. Expected format like '100Gi' or '500GB'",
                    size
                ),
                resource_id: None,
            }));
        };

        let num: u32 = num_str.parse().map_err(|_| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("Invalid storage size number: {}", num_str),
                resource_id: None,
            })
        })?;

        let gb = match unit {
            "Gi" | "GiB" => num, // GiB is close enough to GB for EBS
            "GB" => num,
            "Ti" | "TiB" => num * 1024,
            "TB" => num * 1000,
            _ => num,
        };

        Ok(gb)
    }
}

struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    certificate_arn: Option<String>,
    uses_custom_domain: bool,
}

impl AwsContainerController {
    fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<DomainInfo> {
        let stack_settings = &ctx.deployment_config.stack_settings;
        if let Some(custom) = stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id))
        {
            let cert_arn = custom
                .certificate
                .aws
                .as_ref()
                .map(|cert| cert.certificate_arn.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires an AWS certificate ARN".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                certificate_arn: Some(cert_arn),
                uses_custom_domain: true,
            });
        }

        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for public resource".to_string(),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

        let resource = metadata.resources.get(resource_id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain metadata missing for resource".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        Ok(DomainInfo {
            fqdn: resource.fqdn.clone(),
            certificate_id: Some(resource.certificate_id.clone()),
            certificate_arn: None,
            uses_custom_domain: false,
        })
    }
}

#[controller]
impl AwsContainerController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(container_id = %config.id, cluster = %cluster, "Starting Container provisioning");

        self.container_name = Some(config.id.clone());

        // Determine next step based on what infrastructure we need to create
        // With preflight validation, we know there's at most one exposed port
        let exposed_port = config.ports.iter().find(|p| p.expose.is_some());

        if let Some(port_config) = exposed_port {
            let protocol = port_config.expose.as_ref().unwrap();

            // If HTTP protocol, need domain/certificate setup
            if matches!(protocol, ExposeProtocol::Http) {
                let domain_info = Self::resolve_domain_info(ctx, &config.id)?;
                self.fqdn = Some(domain_info.fqdn.clone());
                self.certificate_id = domain_info.certificate_id;
                self.certificate_arn = domain_info.certificate_arn;
                self.uses_custom_domain = domain_info.uses_custom_domain;

                // Store the public URL
                self.public_url = Some(
                    ctx.deployment_config
                        .public_urls
                        .as_ref()
                        .and_then(|urls| urls.get(&config.id).cloned())
                        .unwrap_or_else(|| format!("https://{}", domain_info.fqdn)),
                );
            }

            Ok(HandlerAction::Continue {
                state: CreatingTargetGroup,
                suggested_delay: None,
            })
        } else if config.persistent_storage.is_some() {
            Ok(HandlerAction::Continue {
                state: CreatingEbsVolumes,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: CreatingHorizonContainer,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = CreatingTargetGroup,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_target_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let elbv2_client = ctx.service_provider.get_aws_elbv2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Container>()?;

        // Get VPC from network dependency
        let network_ref = ResourceRef::new(
            alien_core::Network::RESOURCE_TYPE,
            "default-network".to_string(),
        );
        let network = ctx.require_dependency::<AwsNetworkController>(&network_ref)?;
        let vpc_id = network.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Network VPC ID not available".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Create target group for the exposed port (preflight ensures at most one)
        let exposed_port = config.ports.iter().find(|p| p.expose.is_some());

        if let Some(port_config) = exposed_port {
            let port = port_config.port;
            let target_group_name = format!("{}-{}-tg", ctx.resource_prefix, config.id);

            info!(
                container_id = %config.id,
                    port = port,
                target_group_name = %target_group_name,
                    "Creating target group for exposed port"
            );

            // Create target group for this port
            let create_response = elbv2_client
                .create_target_group(
                    CreateTargetGroupRequest::builder()
                        .name(target_group_name.clone())
                        .protocol("HTTP".to_string())
                        .port(port as i32)
                        .vpc_id(vpc_id.clone())
                        .target_type("instance".to_string())
                        .health_check_enabled(true)
                        .health_check_path(
                            config
                                .health_check
                                .as_ref()
                                .map(|h| h.path.clone())
                                .unwrap_or_else(|| "/".to_string()),
                        )
                        .health_check_interval_seconds(30)
                        .health_check_timeout_seconds(5)
                        .healthy_threshold_count(2)
                        .unhealthy_threshold_count(3)
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create target group for port {}", port),
                    resource_id: Some(config.id.clone()),
                })?;

            let target_group_arn = create_response
                .create_target_group_result
                .target_groups
                .as_ref()
                .and_then(|w| w.members.first())
                .and_then(|tg| tg.target_group_arn.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: "Target group created but no ARN returned".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

            self.target_group_arn = Some(target_group_arn.clone());

            info!(
                container_id = %config.id,
                    port = port,
                target_group_arn = %target_group_arn,
                    "Target group created for port"
            );
        }

        // Check if we also need to create EBS volumes
        if config.persistent_storage.is_some() {
            Ok(HandlerAction::Continue {
                state: CreatingEbsVolumes,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: CreatingHorizonContainer,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = CreatingEbsVolumes,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_ebs_volumes(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Container>()?;

        let persistent_storage = match &config.persistent_storage {
            Some(ps) => ps,
            None => {
                return Ok(HandlerAction::Continue {
                    state: CreatingHorizonContainer,
                    suggested_delay: None,
                })
            }
        };

        // Parse storage size from string to GB
        let size_gb = Self::parse_storage_size_gb(&persistent_storage.size)?;

        // Get availability zone from network (use first private subnet's zone)
        let network_ref = ResourceRef::new(
            alien_core::Network::RESOURCE_TYPE,
            "default-network".to_string(),
        );
        let network = ctx.require_dependency::<AwsNetworkController>(&network_ref)?;

        // For stateful containers, we need to know what zone to create volumes in
        let zone = network
            .private_subnet_ids
            .first()
            .and_then(|_| Some(format!("{}a", aws_cfg.region.clone())))
            .unwrap_or_else(|| format!("{}a", aws_cfg.region));

        // For stateful containers, create one volume per potential replica
        let replica_count = config
            .replicas
            .or(config.autoscaling.as_ref().map(|a| a.desired))
            .unwrap_or(1);

        info!(
            container_id = %config.id,
            size_gb = size_gb,
            replica_count = replica_count,
            zone = %zone,
            "Creating EBS volumes for persistent storage"
        );

        // Map storage_type string to EBS volume type
        let volume_type = persistent_storage.storage_type.as_deref().unwrap_or("gp3");

        for ordinal in 0..replica_count {
            let volume_name = format!("{}-{}-vol-{}", ctx.resource_prefix, config.id, ordinal);

            let tags = vec![alien_aws_clients::ec2::TagSpecification {
                resource_type: "volume".to_string(),
                tags: vec![
                    alien_aws_clients::ec2::Tag {
                        key: "Name".to_string(),
                        value: volume_name,
                    },
                    alien_aws_clients::ec2::Tag {
                        key: "ManagedBy".to_string(),
                        value: "Alien".to_string(),
                    },
                    alien_aws_clients::ec2::Tag {
                        key: "Container".to_string(),
                        value: config.id.clone(),
                    },
                ],
            }];

            // Build request based on optional fields
            // Note: bon builder uses type states, so we handle combinations explicitly
            let request = match (persistent_storage.iops, persistent_storage.throughput) {
                (Some(iops), Some(throughput)) => {
                    alien_aws_clients::ec2::CreateVolumeRequest::builder()
                        .availability_zone(zone.clone())
                        .size(size_gb as i32)
                        .volume_type(volume_type.to_string())
                        .encrypted(true)
                        .iops(iops as i32)
                        .throughput(throughput as i32)
                        .tag_specifications(tags)
                        .build()
                }
                (Some(iops), None) => alien_aws_clients::ec2::CreateVolumeRequest::builder()
                    .availability_zone(zone.clone())
                    .size(size_gb as i32)
                    .volume_type(volume_type.to_string())
                    .encrypted(true)
                    .iops(iops as i32)
                    .tag_specifications(tags)
                    .build(),
                (None, Some(throughput)) => alien_aws_clients::ec2::CreateVolumeRequest::builder()
                    .availability_zone(zone.clone())
                    .size(size_gb as i32)
                    .volume_type(volume_type.to_string())
                    .encrypted(true)
                    .throughput(throughput as i32)
                    .tag_specifications(tags)
                    .build(),
                (None, None) => alien_aws_clients::ec2::CreateVolumeRequest::builder()
                    .availability_zone(zone.clone())
                    .size(size_gb as i32)
                    .volume_type(volume_type.to_string())
                    .encrypted(true)
                    .tag_specifications(tags)
                    .build(),
            };

            let create_response =
                ec2_client
                    .create_volume(request)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create EBS volume for ordinal {}", ordinal),
                        resource_id: Some(config.id.clone()),
                    })?;

            let volume_id = create_response.volume_id.ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Volume created but no ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            self.ebs_volumes.push(EbsVolumeState {
                volume_id: volume_id.clone(),
                zone: zone.clone(),
                ordinal,
                size_gb,
            });

            info!(
                container_id = %config.id,
                volume_id = %volume_id,
                ordinal = ordinal,
                "EBS volume created"
            );
        }

        Ok(HandlerAction::Continue {
            state: WaitingForVolumes,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForVolumes,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_volumes(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Container>()?;

        if self.ebs_volumes.is_empty() {
            return Ok(HandlerAction::Continue {
                state: CreatingHorizonContainer,
                suggested_delay: None,
            });
        }

        let volume_ids: Vec<String> = self
            .ebs_volumes
            .iter()
            .map(|v| v.volume_id.clone())
            .collect();

        let describe_response = ec2_client
            .describe_volumes(
                alien_aws_clients::ec2::DescribeVolumesRequest::builder()
                    .volume_ids(volume_ids)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe EBS volumes".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let volumes = describe_response
            .volume_set
            .map(|vs| vs.items)
            .unwrap_or_default();

        let all_available = volumes
            .iter()
            .all(|v| v.state.as_deref() == Some("available"));

        if all_available {
            info!(
                container_id = %config.id,
                volume_count = self.ebs_volumes.len(),
                "All EBS volumes are available"
            );
            Ok(HandlerAction::Continue {
                state: CreatingHorizonContainer,
                suggested_delay: None,
            })
        } else {
            debug!(
                container_id = %config.id,
                "Waiting for EBS volumes to become available"
            );
            Ok(HandlerAction::Stay {
                max_times: 30,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        }
    }

    #[handler(
        state = CreatingHorizonContainer,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_horizon_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Get the ContainerCluster to verify it's ready and get cluster ID
        let cluster_ref = ResourceRef::new(ContainerCluster::RESOURCE_TYPE, cluster_id.clone());
        let _cluster = ctx.require_dependency::<AwsContainerClusterController>(&cluster_ref)?;

        // Get Horizon context
        let horizon = Self::horizon(ctx, cluster_id)?;

        info!(
            container_id = %config.id,
            cluster_id = %horizon.cluster.cluster_id,
            target_group = ?self.target_group_arn,
            volume_count = self.ebs_volumes.len(),
            "Creating container in Horizon"
        );

        // Get image from container code
        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Container is configured with source code, but only pre-built images are supported".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        // Build resource requirements using SDK types
        let cpu: horizon_client_sdk::types::ResourceRequirementsCpu =
            horizon_client_sdk::types::ResourceRequirementsCpu::builder()
                .min(&config.cpu.min)
                .desired(&config.cpu.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid CPU config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let memory: horizon_client_sdk::types::ResourceRequirementsMemory =
            horizon_client_sdk::types::ResourceRequirementsMemory::builder()
                .min(&config.memory.min)
                .desired(&config.memory.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid memory config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let mut resources_builder = horizon_client_sdk::types::ResourceRequirements::builder()
            .cpu(cpu)
            .memory(memory);

        if let Some(ephemeral) = &config.ephemeral_storage {
            let ephemeral_storage: horizon_client_sdk::types::ResourceRequirementsEphemeralStorage =
                ephemeral.as_str().try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid ephemeral storage config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            resources_builder = resources_builder.ephemeral_storage(ephemeral_storage);
        }

        if let Some(gpu) = &config.gpu {
            let gpu_spec: horizon_client_sdk::types::GpuSpec =
                horizon_client_sdk::types::GpuSpec::builder()
                    .type_(gpu.gpu_type.clone())
                    .count(NonZeroU64::new(gpu.count as u64).unwrap_or(NonZeroU64::new(1).unwrap()))
                    .try_into()
                    .map_err(|e| {
                        AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!("Invalid GPU config: {:?}", e),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
            resources_builder = resources_builder.gpu(gpu_spec);
        }

        let resources: horizon_client_sdk::types::ResourceRequirements =
            resources_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid resources config: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // Build ports from config
        let ports: Vec<NonZeroU64> = config
            .ports
            .iter()
            .filter_map(|p| NonZeroU64::new(p.port as u64))
            .collect();

        // Build capacity group
        let capacity_group = config.pool.clone().unwrap_or_else(|| "general".to_string());

        // Build environment variables using EnvironmentVariableBuilder
        //
        // IMPORTANT: Start with config.environment which includes:
        // - User-defined variables from the Container config
        // - Injected variables from DeploymentConfig (OTLP, ALIEN_AGENT_ID, etc.)
        //   added by inject_environment_variables() during Provisioning phase
        //
        // Then layer on:
        // - Standard Alien vars (ALIEN_DEPLOYMENT_TYPE, platform IDs)
        // - ALIEN_TRANSPORT=passthrough (containers own their HTTP port; Horizon manages networking)
        // - Linked resources (bindings)
        let env_vars = EnvironmentVariableBuilder::new(&config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_container_transport_env_vars()
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        // Start building request
        let mut request_builder = horizon_client_sdk::types::CreateContainerRequest::builder()
            .name(&config.id)
            .capacity_group(&capacity_group)
            .image(&image)
            .resources(resources)
            .stateful(config.stateful)
            .ports(ports)
            .env(env_vars);

        // Add replicas or autoscaling
        if config.stateful {
            if let Some(replicas) = config.replicas {
                if let Some(nz) = NonZeroU64::new(replicas as u64) {
                    request_builder = request_builder.replicas(nz);
                }
            }
        } else if let Some(autoscaling) = &config.autoscaling {
            let mut autoscaling_builder =
                horizon_client_sdk::types::CreateContainerRequestAutoscaling::builder()
                    .min(
                        NonZeroU64::new(autoscaling.min as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .desired(
                        NonZeroU64::new(autoscaling.desired as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .max(
                        NonZeroU64::new(autoscaling.max as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    );

            if let Some(cpu_pct) = autoscaling.target_cpu_percent {
                autoscaling_builder = autoscaling_builder.target_cpu_percent(cpu_pct as f64);
            }
            if let Some(mem_pct) = autoscaling.target_memory_percent {
                autoscaling_builder = autoscaling_builder.target_memory_percent(mem_pct as f64);
            }
            if let Some(http_inflight) = autoscaling.target_http_in_flight_per_replica {
                if let Some(nz) = NonZeroU64::new(http_inflight as u64) {
                    autoscaling_builder = autoscaling_builder.target_http_in_flight_per_replica(nz);
                }
            }

            let autoscaling_config: horizon_client_sdk::types::CreateContainerRequestAutoscaling =
                autoscaling_builder.try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid autoscaling config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            request_builder = request_builder.autoscaling(autoscaling_config);
        }

        // Add command if present
        if let Some(cmd) = &config.command {
            request_builder = request_builder.command(cmd.clone());
        }

        // Wire per-container cloud identity so horizond can vend credentials via the IMDS proxy.
        // The SA resource ID follows the convention: {permissions_profile}-sa
        {
            let sa_resource_id = format!("{}-sa", config.get_permissions());
            if let Some(sa_resource) = ctx.desired_stack.resources.get(&sa_resource_id) {
                let sa_ctrl = ctx
                    .require_dependency::<crate::service_account::AwsServiceAccountController>(
                        &(&sa_resource.config).into(),
                    )?;
                if let Some(role_arn) = &sa_ctrl.role_arn {
                    let sa = horizon_client_sdk::types::ServiceAccountTarget::from(
                        horizon_client_sdk::types::AwsServiceAccountTarget {
                            role_arn: role_arn.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid role ARN '{}'", role_arn),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                            type_: horizon_client_sdk::types::AwsServiceAccountTargetType::Aws,
                        },
                    );
                    request_builder = request_builder.service_account(sa);
                    info!(
                        container_id = %config.id,
                        role_arn = %role_arn,
                        "Wired AWS service account to container for IMDS credential vending"
                    );
                }
            }
        }

        // Add load balancer target for exposed port (single port enforced by preflight)
        if let Some(tg_arn) = &self.target_group_arn {
            let lb_target = horizon_client_sdk::types::LoadBalancerTarget::Aws {
                target_group_arn: tg_arn.parse().into_alien_error().context(
                    ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid target group ARN '{}'", tg_arn),
                        resource_id: Some(config.id.clone()),
                    },
                )?,
            };
            request_builder = request_builder.load_balancer_target(lb_target);
        }

        // Add volumes for persistent storage
        if !self.ebs_volumes.is_empty() {
            let volumes = self
                .ebs_volumes
                .iter()
                .map(|v| {
                    Ok::<_, AlienError<ErrorData>>(horizon_client_sdk::types::VolumeRegistration {
                        ordinal: v.ordinal as u64,
                        volume: horizon_client_sdk::types::VolumeTarget::Aws {
                            volume_id: v.volume_id.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid EBS volume ID '{}'", v.volume_id),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                            zone: v.zone.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid zone '{}'", v.zone),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                        },
                    })
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            request_builder = request_builder.volumes(volumes);
        }

        // Build the final request
        let request: horizon_client_sdk::types::CreateContainerRequest =
            request_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Failed to build container request: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // Create container via Horizon SDK
        let response = horizon
            .client
            .create_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .body(&request)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to create container in Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.horizon_status = Some(horizon_container_status_to_alien(response.status));

        info!(
            container_id = %config.id,
            "Container created in Horizon, waiting for replicas"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForReplicas,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = WaitingForReplicas,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_replicas(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Get Horizon context
        let horizon = Self::horizon(ctx, cluster_id)?;

        let container_response = horizon
            .client
            .get_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to get container status from Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.horizon_status = Some(horizon_container_status_to_alien(container_response.status));

        let healthy_replicas = container_response
            .replicas_info
            .iter()
            .filter(|r| r.healthy)
            .count() as u32;

        self.current_replicas = healthy_replicas;

        let desired = config
            .replicas
            .or(config.autoscaling.as_ref().map(|a| a.desired))
            .unwrap_or(1);

        debug!(
            container_id = %config.id,
            healthy = healthy_replicas,
            desired = desired,
            status = %container_response.status,
            "Container replica status"
        );

        if healthy_replicas >= desired.min(1) {
            info!(
                container_id = %config.id,
                healthy = healthy_replicas,
                "Container has healthy replicas"
            );

            // Check if we have an exposed port that needs load balancer
            let exposed_port = config.ports.iter().find(|p| p.expose.is_some());

            if let Some(port_config) = exposed_port {
                let is_http = matches!(port_config.expose.as_ref().unwrap(), ExposeProtocol::Http);

                let next_state = if is_http && !self.uses_custom_domain {
                    WaitingForCertificate
                } else {
                    CreatingLoadBalancer
                };
                Ok(HandlerAction::Continue {
                    state: next_state,
                    suggested_delay: None,
                })
            } else {
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
        } else {
            self.wait_for_replicas_iterations += 1;
            if self.wait_for_replicas_iterations >= 30 {
                // If the parent cluster is mid-update/provision, replica disruption is expected:
                // the rolling update will bring fresh VMs with updated horizond. Reset and wait.
                let cluster_is_updating = ctx.state.resources.get(cluster_id).map_or(false, |s| {
                    matches!(
                        s.status,
                        ResourceStatus::Updating | ResourceStatus::Provisioning
                    )
                });
                if cluster_is_updating {
                    info!(
                        container_id = %config.id,
                        cluster_id = %cluster_id,
                        "Parent cluster is updating, resetting health check counter"
                    );
                    self.wait_for_replicas_iterations = 0;
                } else {
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "Container replicas did not become healthy after 30 iterations (~5 min). \
                             Last Horizon status: {:?}, healthy replicas: {}/{}",
                            self.horizon_status, self.current_replicas, desired
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
            debug!(
                container_id = %config.id,
                iteration = self.wait_for_replicas_iterations,
                "Waiting for more healthy replicas"
            );
            Ok(HandlerAction::Stay {
                max_times: 35, // safety backstop; manual check above fires first
                suggested_delay: Some(Duration::from_secs(10)),
            })
        }
    }

    #[handler(
        state = WaitingForCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&config.id));

        let status = metadata.map(|m| &m.certificate_status);

        match status {
            Some(CertificateStatus::Issued) => Ok(HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay: None,
            }),
            Some(CertificateStatus::Failed) => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "Certificate issuance failed".to_string(),
                    resource_id: Some(config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = ImportingCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let resource = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&config.id))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for certificate import".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // Certificate data is included in DeploymentConfig
        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let (leaf, chain) = crate::core::split_certificate_chain(certificate_chain);

        let aws_cfg = ctx.get_aws_config()?;
        let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
        let response = acm_client
            .import_certificate(
                alien_aws_clients::acm::ImportCertificateRequest::builder()
                    .certificate(leaf)
                    .private_key(private_key.clone())
                    .maybe_certificate_chain(chain)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import certificate to ACM".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.certificate_arn = Some(response.certificate_arn.clone());

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        Ok(HandlerAction::Continue {
            state: CreatingLoadBalancer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingLoadBalancer,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_load_balancer(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let elbv2_client = ctx.service_provider.get_aws_elbv2_client(aws_cfg).await?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Container>()?;

        let network_ref = ResourceRef::new(
            alien_core::Network::RESOURCE_TYPE,
            "default-network".to_string(),
        );
        let network = ctx.require_dependency::<AwsNetworkController>(&network_ref)?;

        let vpc_id = network.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Network VPC ID not available".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let subnets = if !network.public_subnet_ids.is_empty() {
            network.public_subnet_ids.clone()
        } else {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Public subnets are required for public containers".to_string(),
                resource_id: Some(config.id.clone()),
            }));
        };

        let existing_sg = self
            .load_balancer
            .as_ref()
            .and_then(|lb| lb.security_group_id.clone());

        let lb_security_group_id = if let Some(existing) = existing_sg {
            existing
        } else {
            let sg_response = ec2_client
                .create_security_group(
                    alien_aws_clients::ec2::CreateSecurityGroupRequest::builder()
                        .group_name(format!("{}-{}-alb-sg", ctx.resource_prefix, config.id))
                        .description("Alien managed ALB security group".to_string())
                        .vpc_id(vpc_id.clone())
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to create ALB security group".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            let sg_id = sg_response.group_id.ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Security group created but no ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            ec2_client
                .authorize_security_group_ingress(
                    alien_aws_clients::ec2::AuthorizeSecurityGroupIngressRequest::builder()
                        .group_id(sg_id.clone())
                        .ip_permissions(vec![alien_aws_clients::ec2::IpPermission {
                            ip_protocol: "tcp".to_string(),
                            from_port: Some(443),
                            to_port: Some(443),
                            ip_ranges: Some(vec![alien_aws_clients::ec2::IpRange {
                                cidr_ip: "0.0.0.0/0".to_string(),
                                description: Some("Allow HTTPS ingress".to_string()),
                            }]),
                            ipv6_ranges: None,
                            user_id_group_pairs: None,
                        }])
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to add ALB security group ingress".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            sg_id
        };

        let lb_name = format!("{}-{}-alb", ctx.resource_prefix, config.id);
        let lb_response = elbv2_client
            .create_load_balancer(
                alien_aws_clients::elbv2::CreateLoadBalancerRequest::builder()
                    .name(lb_name)
                    .subnets(subnets)
                    .security_groups(vec![lb_security_group_id.clone()])
                    .scheme("internet-facing".to_string())
                    .load_balancer_type("application".to_string())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create ALB".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let lb = lb_response
            .create_load_balancer_result
            .load_balancers
            .as_ref()
            .and_then(|wrapper| wrapper.members.first())
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Load balancer created but no details returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let lb_arn = lb.load_balancer_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Load balancer ARN missing".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let cert_arn = self.certificate_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate ARN missing for load balancer".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let listener_response = elbv2_client
            .create_listener(
                alien_aws_clients::elbv2::CreateListenerRequest::builder()
                    .load_balancer_arn(lb_arn.clone())
                    .protocol("HTTPS".to_string())
                    .port(443)
                    .certificates(vec![alien_aws_clients::elbv2::Certificate {
                        certificate_arn: cert_arn,
                        is_default: Some(true),
                    }])
                    .default_actions(vec![alien_aws_clients::elbv2::Action::builder()
                        .action_type("forward".to_string())
                        .maybe_target_group_arn(self.target_group_arn.clone())
                        .build()])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create ALB listener".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let listener_arn = listener_response
            .create_listener_result
            .listeners
            .as_ref()
            .and_then(|l| l.members.first())
            .and_then(|l| l.listener_arn.clone());

        let endpoint = match (&lb.dns_name, &lb.canonical_hosted_zone_id) {
            (Some(dns_name), Some(hosted_zone_id)) => Some(LoadBalancerEndpoint {
                dns_name: dns_name.clone(),
                hosted_zone_id: hosted_zone_id.clone(),
            }),
            _ => None,
        };

        // Store load balancer (single exposed port enforced by preflight)
        let exposed_port = config
            .ports
            .iter()
            .find(|p| p.expose.is_some())
            .expect("Should have exposed port if we created load balancer");

        let port = exposed_port.port;
        let protocol = exposed_port.expose.as_ref().unwrap().clone();

        self.load_balancer = Some(LoadBalancerState {
            arn: lb_arn,
            listener_arn,
            security_group_id: Some(lb_security_group_id),
            endpoint,
            port,
            protocol,
        });

        // Store public URL from fqdn if this is HTTP
        if let Some(fqdn) = &self.fqdn {
            self.public_url = Some(
                ctx.deployment_config
                    .public_urls
                    .as_ref()
                    .and_then(|urls| urls.get(&config.id).cloned())
                    .unwrap_or_else(|| format!("https://{}", fqdn)),
            );
        }

        if self.uses_custom_domain {
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        }
    }

    #[handler(
        state = WaitingForDns,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&config.id));

        let status = metadata.map(|m| &m.dns_status);

        match status {
            Some(DnsRecordStatus::Active) => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            }),
            Some(DnsRecordStatus::Failed) => {
                let fqdn = metadata.map(|m| m.fqdn.as_str()).unwrap_or("unknown");
                let detail = metadata
                    .and_then(|m| m.dns_error.as_deref())
                    .unwrap_or("unknown error");
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("DNS record creation failed for {fqdn}: {detail}"),
                    resource_id: Some(config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
    }

    // ─────────────── READY STATE ────────────────────────────────

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        debug!(container_id = %config.id, "Container ready, checking health");

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Periodic health check - query Horizon for current status
        let horizon = Self::horizon(ctx, cluster_id)?;

        let container = horizon
            .client
            .get_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to get container status from Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.horizon_status = Some(horizon_container_status_to_alien(container.status));
        self.current_replicas = container.replicas_info.iter().filter(|r| r.healthy).count() as u32;

        // Check if certificate was renewed (for containers with HTTP exposed port and auto-managed domains)
        let has_http_port = config
            .ports
            .iter()
            .any(|p| p.expose == Some(ExposeProtocol::Http));

        if has_http_port {
            if let Some(domain_metadata) = &ctx.deployment_config.domain_metadata {
                if let Some(resource_info) = domain_metadata.resources.get(&config.id) {
                    if let Some(new_issued_at) = &resource_info.issued_at {
                        match &self.certificate_issued_at {
                            Some(stored) if new_issued_at != stored => {
                                // Certificate renewed! Trigger update flow to re-import
                                info!(
                                    name = %config.id,
                                    old_issued_at = %stored,
                                    new_issued_at = %new_issued_at,
                                    "Certificate renewed, triggering update"
                                );
                                return Ok(HandlerAction::Continue {
                                    state: UpdateStart,
                                    suggested_delay: None,
                                });
                            }
                            None => {
                                // First heartbeat after deployment, store the timestamp
                                self.certificate_issued_at = Some(new_issued_at.clone());
                            }
                            _ => {} // Same timestamp, no renewal
                        }
                    }
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
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
        let config = ctx.desired_resource_config::<Container>()?;

        info!(container_id = %config.id, "Container update requested");

        Ok(HandlerAction::Continue {
            state: UpdatingHorizonContainer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingHorizonContainer,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_horizon_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let horizon = Self::horizon(ctx, cluster_id)?;

        info!(
            container_id = %config.id,
            cluster_id = %horizon.cluster.cluster_id,
            "Updating container in Horizon"
        );

        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Container is configured with source code, but only pre-built images are supported".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        let image_typed: horizon_client_sdk::types::UpdateContainerRequestImage =
            image.as_str().try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid image: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let env_vars = EnvironmentVariableBuilder::new(&config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_container_transport_env_vars()
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        let cpu: horizon_client_sdk::types::UpdateContainerRequestResourcesCpu =
            horizon_client_sdk::types::UpdateContainerRequestResourcesCpu::builder()
                .min(&config.cpu.min)
                .desired(&config.cpu.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid CPU config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let memory: horizon_client_sdk::types::UpdateContainerRequestResourcesMemory =
            horizon_client_sdk::types::UpdateContainerRequestResourcesMemory::builder()
                .min(&config.memory.min)
                .desired(&config.memory.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid memory config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let mut resources_builder =
            horizon_client_sdk::types::UpdateContainerRequestResources::builder()
                .cpu(cpu)
                .memory(memory);

        if let Some(ephemeral) = &config.ephemeral_storage {
            let ephemeral_storage: horizon_client_sdk::types::UpdateContainerRequestResourcesEphemeralStorage =
                ephemeral.as_str().try_into().map_err(|e| AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid ephemeral storage config: {:?}", e),
                    resource_id: Some(config.id.clone()),
                }))?;
            resources_builder = resources_builder.ephemeral_storage(ephemeral_storage);
        }

        if let Some(gpu) = &config.gpu {
            let gpu_spec: horizon_client_sdk::types::GpuSpec =
                horizon_client_sdk::types::GpuSpec::builder()
                    .type_(gpu.gpu_type.clone())
                    .count(NonZeroU64::new(gpu.count as u64).unwrap_or(NonZeroU64::new(1).unwrap()))
                    .try_into()
                    .map_err(|e| {
                        AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!("Invalid GPU config: {:?}", e),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
            resources_builder = resources_builder.gpu(gpu_spec);
        }

        let resources: horizon_client_sdk::types::UpdateContainerRequestResources =
            resources_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid resources config: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let mut request_builder = horizon_client_sdk::types::UpdateContainerRequest::builder()
            .image(image_typed)
            .env(env_vars)
            .resources(resources);

        if let Some(cmd) = &config.command {
            request_builder = request_builder.command(cmd.clone());
        }

        if config.stateful {
            if let Some(replicas) = config.replicas {
                if let Some(nz) = NonZeroU64::new(replicas as u64) {
                    request_builder = request_builder.replicas(nz);
                }
            }
        } else if let Some(autoscaling) = &config.autoscaling {
            let mut autoscaling_builder =
                horizon_client_sdk::types::UpdateContainerRequestAutoscaling::builder()
                    .min(
                        NonZeroU64::new(autoscaling.min as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .desired(
                        NonZeroU64::new(autoscaling.desired as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .max(
                        NonZeroU64::new(autoscaling.max as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    );

            if let Some(cpu_pct) = autoscaling.target_cpu_percent {
                autoscaling_builder = autoscaling_builder.target_cpu_percent(cpu_pct as f64);
            }
            if let Some(mem_pct) = autoscaling.target_memory_percent {
                autoscaling_builder = autoscaling_builder.target_memory_percent(mem_pct as f64);
            }
            if let Some(http_inflight) = autoscaling.target_http_in_flight_per_replica {
                if let Some(nz) = NonZeroU64::new(http_inflight as u64) {
                    autoscaling_builder = autoscaling_builder.target_http_in_flight_per_replica(nz);
                }
            }
            if let Some(p95_latency) = autoscaling.max_http_p95_latency_ms {
                autoscaling_builder = autoscaling_builder.max_http_p95_latency_ms(p95_latency);
            }

            let autoscaling_config: horizon_client_sdk::types::UpdateContainerRequestAutoscaling =
                autoscaling_builder.try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid autoscaling config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            request_builder = request_builder.autoscaling(autoscaling_config);
        }

        // Wire SA identity on updates (SA role ARN may be updated e.g. after trust policy change)
        {
            let sa_resource_id = format!("{}-sa", config.get_permissions());
            if let Some(sa_resource) = ctx.desired_stack.resources.get(&sa_resource_id) {
                let sa_ctrl = ctx
                    .require_dependency::<crate::service_account::AwsServiceAccountController>(
                        &(&sa_resource.config).into(),
                    )?;
                if let Some(role_arn) = &sa_ctrl.role_arn {
                    let sa = horizon_client_sdk::types::NullableServiceAccountTarget::from(
                        horizon_client_sdk::types::AwsServiceAccountTarget {
                            role_arn: role_arn.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid role ARN '{}'", role_arn),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                            type_: horizon_client_sdk::types::AwsServiceAccountTargetType::Aws,
                        },
                    );
                    request_builder = request_builder.service_account(sa);
                }
            }
        }

        let request: horizon_client_sdk::types::UpdateContainerRequest =
            request_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Failed to build update request: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        horizon
            .client
            .update_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .body(&request)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to update container in Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        info!(container_id = %config.id, "Container updated in Horizon");

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
        let config = ctx.desired_resource_config::<Container>()?;

        info!(container_id = %config.id, "Starting Container deletion");

        Ok(HandlerAction::Continue {
            state: DeletingHorizonContainer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingHorizonContainer,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_horizon_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let horizon = Self::horizon(ctx, cluster_id)?;

        info!(
            container_id = %config.id,
            cluster_id = %horizon.cluster.cluster_id,
            "Deleting container from Horizon"
        );

        // Best-effort deletion - continue even if API call fails (container may already be gone)
        match horizon
            .client
            .delete_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .send()
            .await
        {
            Ok(_) => info!(container_id = %config.id, "Container deleted from Horizon"),
            Err(e) => {
                warn!(
                    container_id = %config.id,
                    error = ?e,
                    "Failed to delete container from Horizon, continuing with cleanup"
                );
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingLoadBalancer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingLoadBalancer,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_load_balancer(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        if let Some(load_balancer) = &self.load_balancer {
            let aws_cfg = ctx.get_aws_config()?;
            let elbv2_client = ctx.service_provider.get_aws_elbv2_client(aws_cfg).await?;
            let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

            if let Some(listener_arn) = &load_balancer.listener_arn {
                let _ = elbv2_client.delete_listener(listener_arn).await;
            }

            match elbv2_client.delete_load_balancer(&load_balancer.arn).await {
                Ok(_) => info!(container_id = %config.id, "Load balancer deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    warn!(container_id = %config.id, "Load balancer already deleted");
                }
                Err(e) => {
                    warn!(
                        container_id = %config.id,
                        error = ?e,
                        "Failed to delete load balancer, continuing"
                    );
                }
            }

            if let Some(sg_id) = &load_balancer.security_group_id {
                let _ = ec2_client.delete_security_group(sg_id).await;
            }

            self.load_balancer = None;
        }

        if !self.uses_custom_domain {
            if let Some(cert_arn) = &self.certificate_arn {
                let aws_cfg = ctx.get_aws_config()?;
                let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
                let _ = acm_client.delete_certificate(cert_arn).await;
            }
        }

        self.certificate_arn = None;

        Ok(HandlerAction::Continue {
            state: DeletingTargetGroup,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingTargetGroup,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_target_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        if let Some(target_group_arn) = &self.target_group_arn {
            let aws_cfg = ctx.get_aws_config()?;
            let elbv2_client = ctx.service_provider.get_aws_elbv2_client(aws_cfg).await?;

            info!(
                container_id = %config.id,
                target_group_arn = %target_group_arn,
                "Deleting target group"
            );

            match elbv2_client.delete_target_group(target_group_arn).await {
                Ok(_) => {
                    info!(
                        container_id = %config.id,
                        "Target group deleted"
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    warn!(
                        container_id = %config.id,
                        "Target group already deleted"
                    );
                }
                Err(e) => {
                    warn!(
                        container_id = %config.id,
                        error = ?e,
                        "Failed to delete target group, continuing"
                    );
                }
            }

            self.target_group_arn = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingEbsVolumes,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingEbsVolumes,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_ebs_volumes(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        if self.ebs_volumes.is_empty() {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        info!(
            container_id = %config.id,
            volume_count = self.ebs_volumes.len(),
            "Deleting EBS volumes"
        );

        for volume_state in &self.ebs_volumes {
            info!(
                container_id = %config.id,
                volume_id = %volume_state.volume_id,
                "Deleting EBS volume"
            );

            match ec2_client.delete_volume(&volume_state.volume_id).await {
                Ok(_) => {
                    info!(
                        volume_id = %volume_state.volume_id,
                        "Volume deleted"
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    warn!(
                        volume_id = %volume_state.volume_id,
                        "Volume already deleted"
                    );
                }
                Err(e) => {
                    warn!(
                        volume_id = %volume_state.volume_id,
                        error = ?e,
                        "Failed to delete volume, continuing"
                    );
                }
            }
        }

        self.ebs_volumes.clear();

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
        let container_name = self.container_name.as_ref()?;

        // Map the load balancer endpoint for DNS management (HTTP only)
        let load_balancer_endpoint = self
            .load_balancer
            .as_ref()
            .filter(|lb| lb.protocol == ExposeProtocol::Http)
            .and_then(|lb| lb.endpoint.as_ref())
            .map(|endpoint| alien_core::LoadBalancerEndpoint {
                dns_name: endpoint.dns_name.clone(),
                hosted_zone_id: Some(endpoint.hosted_zone_id.clone()),
            });

        Some(ResourceOutputs::new(ContainerOutputs {
            name: container_name.clone(),
            status: self.horizon_status.unwrap_or(ContainerStatus::Pending),
            current_replicas: self.current_replicas,
            desired_replicas: self.current_replicas, // TODO: Get from config
            internal_dns: format!("{}.svc", container_name),
            url: self.public_url.clone(),
            replicas: vec![], // TODO: Populate from Horizon
            load_balancer_endpoint,
        }))
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, ContainerBinding};

        self.container_name.as_ref().map(|name| {
            // Internal URL uses Horizon service discovery (container_name.svc)
            let internal_url = format!("http://{}.svc:8080", name);

            let binding = if let Some(url) = &self.public_url {
                ContainerBinding::horizon_with_public_url(
                    BindingValue::value(name.clone()),
                    BindingValue::value(internal_url),
                    BindingValue::value(url.clone()),
                )
            } else {
                ContainerBinding::horizon(
                    BindingValue::value(name.clone()),
                    BindingValue::value(internal_url),
                )
            };

            serde_json::to_value(binding).unwrap_or_default()
        })
    }
}

impl AwsContainerController {
    /// Creates a controller in ready state with mock values for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(container_name: &str, replicas: u32) -> Self {
        Self {
            state: AwsContainerState::Ready,
            container_name: Some(container_name.to_string()),
            horizon_status: Some(ContainerStatus::Running),
            current_replicas: replicas,
            public_url: None,
            fqdn: None,
            certificate_id: None,
            certificate_arn: None,
            load_balancer: None,
            uses_custom_domain: false,
            target_group_arn: None,
            ebs_volumes: Vec::new(),
            certificate_issued_at: None,
            wait_for_replicas_iterations: 0,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container_cluster::AwsContainerClusterController;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use crate::network::AwsNetworkController;
    use alien_core::NetworkSettings;
    use alien_core::{
        CapacityGroup, ComputeBackend, ContainerAutoscaling, EnvironmentVariablesSnapshot,
        HorizonClusterConfig, HorizonConfig, Network, ResourceSpec,
    };
    use httpmock::MockServer;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn setup_horizon_server(
        cluster_id: &str,
        container_name: &str,
        healthy_replicas: u32,
    ) -> MockServer {
        let server = MockServer::start();

        let replica_infos: Vec<serde_json::Value> = (0..healthy_replicas)
            .map(|idx| {
                json!({
                    "replicaId": format!("{}-{}", container_name, idx),
                    "machineId": format!("machine-{}", idx),
                    "ip": format!("10.0.2.{}", idx + 10),
                    "status": "running",
                    "healthy": true,
                    "consecutiveFailures": 0
                })
            })
            .collect();

        let container_response = json!({
            "name": container_name,
            "capacityGroup": "general",
            "image": "nginx:latest",
            "resources": {
                "cpu": { "min": "1", "desired": "1" },
                "memory": { "min": "1Gi", "desired": "1Gi" }
            },
            "stateful": false,
            "ports": [8080],
            "status": "running",
            "clusterId": cluster_id,
            "replicasInfo": replica_infos,
            "createdAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        });

        let create_response = json!({
            "name": container_name,
            "clusterId": cluster_id,
            "status": "running",
            "createdAt": "2024-01-01T00:00:00Z"
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path(format!("/clusters/{}/containers", cluster_id));
            then.status(200).json_body(create_response.clone());
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(container_response.clone());
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::PATCH).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(create_response.clone());
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::DELETE).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(json!({ "success": true }));
        });

        server
    }

    fn setup_mock_provider() -> Arc<MockPlatformServiceProvider> {
        Arc::new(MockPlatformServiceProvider::new())
    }

    fn test_container(cluster_id: &str) -> Container {
        Container::new("api".to_string())
            .cluster(cluster_id.to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .autoscaling(ContainerAutoscaling {
                min: 1,
                desired: 1,
                max: 2,
                target_cpu_percent: Some(70.0),
                target_memory_percent: Some(80.0),
                target_http_in_flight_per_replica: None,
                max_http_p95_latency_ms: None,
            })
            .permissions("execution".to_string())
            .build()
    }

    fn test_horizon_config(server: &MockServer, cluster_id: &str) -> ComputeBackend {
        let mut clusters = HashMap::new();
        clusters.insert(
            "compute".to_string(),
            HorizonClusterConfig {
                cluster_id: cluster_id.to_string(),
                management_token: "hm_test".to_string(),
            },
        );
        ComputeBackend::Horizon(HorizonConfig {
            url: server.base_url(),
            horizond_download_base_url: "http://releases.test".to_string(),
            horizond_binary_hash: None,
            clusters,
        })
    }

    fn test_network() -> Network {
        Network::new("default-network".to_string())
            .settings(NetworkSettings::Create {
                cidr: Some("10.0.0.0/16".to_string()),
                availability_zones: 2,
            })
            .build()
    }

    fn test_cluster_resource() -> ContainerCluster {
        ContainerCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("t3.medium".to_string()),
                profile: None,
                min_size: 1,
                max_size: 1,
            })
            .build()
    }

    #[tokio::test]
    async fn test_update_flow_succeeds() {
        let cluster_id = "test-cluster";
        let container_name = "api";
        let server = setup_horizon_server(cluster_id, container_name, 1);
        let mock_provider = setup_mock_provider();

        let updated_container = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "2".to_string(),
                desired: "4".to_string(),
            })
            .memory(ResourceSpec {
                min: "2Gi".to_string(),
                desired: "2Gi".to_string(),
            })
            .port(8080)
            .autoscaling(ContainerAutoscaling {
                min: 1,
                desired: 2,
                max: 5,
                target_cpu_percent: Some(70.0),
                target_memory_percent: Some(80.0),
                target_http_in_flight_per_replica: None,
                max_http_p95_latency_ms: None,
            })
            .permissions("execution".to_string())
            .build();

        let ready_controller = AwsContainerController::mock_ready(container_name, 1);

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(ready_controller)
            .platform(alien_core::Platform::Aws)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AwsNetworkController::mock_ready("default-network", 2),
            )
            .with_dependency(
                test_cluster_resource(),
                AwsContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.update(updated_container).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Verifies that updates with target_http_in_flight_per_replica and max_http_p95_latency_ms complete.
    #[tokio::test]
    async fn test_update_with_http_in_flight() {
        let cluster_id = "test-cluster";
        let container_name = "api";

        let server = MockServer::start();
        let create_response = json!({
            "name": container_name,
            "clusterId": cluster_id,
            "status": "running",
            "createdAt": "2024-01-01T00:00:00Z"
        });
        let container_response = json!({
            "name": container_name,
            "capacityGroup": "general",
            "image": "nginx:latest",
            "resources": { "cpu": { "min": "1", "desired": "1" }, "memory": { "min": "1Gi", "desired": "1Gi" } },
            "stateful": false,
            "ports": [8080],
            "status": "running",
            "clusterId": cluster_id,
            "replicasInfo": [{ "replicaId": "api-0", "machineId": "m-0", "ip": "10.0.2.10", "status": "running", "healthy": true, "consecutiveFailures": 0 }],
            "createdAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path(format!(
                    "/clusters/{}/containers/{}",
                    cluster_id, container_name
                ))
                .body_contains("targetHttpInFlightPerReplica")
                .body_contains("maxHttpP95LatencyMs");
            then.status(200).json_body(create_response.clone());
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(container_response.clone());
        });

        let mock_provider = setup_mock_provider();

        let updated_container = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .autoscaling(ContainerAutoscaling {
                min: 1,
                desired: 1,
                max: 5,
                target_cpu_percent: Some(70.0),
                target_memory_percent: Some(80.0),
                target_http_in_flight_per_replica: Some(10),
                max_http_p95_latency_ms: Some(200.0),
            })
            .permissions("execution".to_string())
            .build();

        let ready_controller = AwsContainerController::mock_ready(container_name, 1);

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(ready_controller)
            .platform(alien_core::Platform::Aws)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AwsNetworkController::mock_ready("default-network", 2),
            )
            .with_dependency(
                test_cluster_resource(),
                AwsContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.update(updated_container).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}
