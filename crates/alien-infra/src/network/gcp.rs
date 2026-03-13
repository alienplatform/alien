//! GCP Network controller for VPC network and subnetwork management.
//!
//! This controller manages:
//! - VPC Networks (custom mode for subnet control)
//! - Subnetworks in specified regions
//! - Cloud Routers for NAT connectivity  
//! - Cloud NAT for private subnet internet access
//! - Firewall rules for ingress/egress control

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{Network, NetworkSettings, ResourceStatus};
use alien_error::{AlienError, Context};
use alien_gcp_clients::compute::{
    Firewall, FirewallAllowed, FirewallDirection, Network as GcpNetwork, NetworkRoutingConfig,
    Router, RouterNat, RouterNatSubnetworkToNat, RoutingMode, SourceIpRangesToNat, Subnetwork,
};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use tracing::{debug, info};

// =============================================================================================
// Controller
// =============================================================================================

/// GCP Network controller for managing VPC networks, subnetworks, routers, and NAT.
#[controller]
pub struct GcpNetworkController {
    // Configuration from Network resource
    pub(crate) desired_settings: Option<NetworkSettings>,

    // Created or imported network details
    pub(crate) network_name: Option<String>,
    pub(crate) network_self_link: Option<String>,
    pub(crate) subnetwork_name: Option<String>,
    pub(crate) subnetwork_self_link: Option<String>,
    pub(crate) router_name: Option<String>,
    pub(crate) cloud_nat_name: Option<String>,
    pub(crate) firewall_name: Option<String>,
    pub(crate) region: Option<String>,
    pub(crate) cidr_block: Option<String>,
    pub(crate) is_byo_vpc: bool,

    // Operation tracking
    pub(crate) pending_operation_name: Option<String>,
    pub(crate) pending_operation_region: Option<String>, // None = global, Some = regional
}

impl GcpNetworkController {
    /// Generate a deterministic CIDR block from the stack/network ID.
    /// Uses RFC 6598 (100.64.0.0/10) or RFC 1918 ranges to minimize conflicts.
    fn generate_cidr_from_id(network_id: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        network_id.hash(&mut hasher);
        let hash = hasher.finish();

        // Use second octet from hash (0-255), constraining to RFC 6598 range
        // RFC 6598: 100.64.0.0 - 100.127.255.255 (100.64.0.0/10)
        let second_octet = 64 + ((hash % 64) as u8);
        format!("100.{}.0.0/16", second_octet)
    }

    /// Get the network name for this controller
    fn get_network_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-vpc", resource_prefix, network_id)
    }

    /// Get the subnetwork name
    fn get_subnetwork_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-subnet", resource_prefix, network_id)
    }

    /// Get the router name
    fn get_router_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-router", resource_prefix, network_id)
    }

    /// Get the Cloud NAT name
    fn get_cloud_nat_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-nat", resource_prefix, network_id)
    }

    /// Get the firewall rule name
    fn get_firewall_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-allow-internal", resource_prefix, network_id)
    }
}

impl GcpNetworkController {
    /// Creates a controller in a ready state with mock values for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(network_id: &str) -> Self {
        let network_name = format!("test-{}-vpc", network_id);
        let subnetwork_name = format!("test-{}-subnet", network_id);
        let region = "us-central1".to_string();

        Self {
            state: GcpNetworkState::Ready,
            desired_settings: None,
            network_name: Some(network_name.clone()),
            network_self_link: Some(format!(
                "https://www.googleapis.com/compute/v1/projects/test/global/networks/{}",
                network_name
            )),
            subnetwork_name: Some(subnetwork_name.clone()),
            subnetwork_self_link: Some(format!(
                "https://www.googleapis.com/compute/v1/projects/test/regions/{}/subnetworks/{}",
                region, subnetwork_name
            )),
            router_name: Some(format!("test-{}-router", network_id)),
            cloud_nat_name: Some(format!("test-{}-nat", network_id)),
            firewall_name: Some(format!("test-{}-fw", network_id)),
            region: Some(region),
            cidr_block: Some("10.0.0.0/16".to_string()),
            is_byo_vpc: false,
            pending_operation_name: None,
            pending_operation_region: None,
            _internal_stay_count: None,
        }
    }
}

#[controller]
impl GcpNetworkController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;

        info!(network_id = %config.id, "Starting GCP Network creation");

        self.desired_settings = Some(config.settings.clone());

        match &config.settings {
            NetworkSettings::UseDefault => {
                // Use the cloud provider's default network — no provisioning needed
                let gcp_config = ctx.get_gcp_config()?;
                let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;
                let region = gcp_config.region.clone();

                info!("Using GCP default network in region {}", region);

                let network = compute_client
                    .get_network("default".to_string())
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: "Default network not found. It may have been deleted. \
                                  Use NetworkSettings::Create to create an isolated VPC, \
                                  or ByoVpcGcp to reference an existing one."
                            .to_string(),
                        operation: Some("discover_default_network".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                self.network_self_link = network.self_link;
                self.network_name = Some("default".to_string());

                let subnetwork = compute_client
                    .get_subnetwork(region.clone(), "default".to_string())
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: format!(
                            "Default subnet not found in region '{}'. \
                             Use NetworkSettings::Create or ByoVpcGcp instead.",
                            region
                        ),
                        operation: Some("discover_default_subnet".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                self.subnetwork_self_link = subnetwork.self_link;
                self.subnetwork_name = Some("default".to_string());
                self.cidr_block = subnetwork.ip_cidr_range;
                self.region = Some(region);
                self.is_byo_vpc = true;

                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }

            NetworkSettings::ByoVpcGcp {
                network_name,
                subnet_name,
                region,
            } => {
                // BYO-VPC: Just store the references
                info!(
                    network_name = %network_name,
                    subnet_name = %subnet_name,
                    region = %region,
                    "Using existing GCP VPC"
                );

                self.network_name = Some(network_name.clone());
                self.subnetwork_name = Some(subnet_name.clone());
                self.region = Some(region.clone());
                self.is_byo_vpc = true;

                // Verify the network exists
                let gcp_config = ctx.get_gcp_config()?;
                let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

                let network = compute_client
                    .get_network(network_name.clone())
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: format!("BYO-VPC network '{}' not found", network_name),
                        operation: Some("verify_byo_vpc".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                self.network_self_link = network.self_link;

                // Verify subnetwork exists
                let subnetwork = compute_client
                    .get_subnetwork(region.clone(), subnet_name.clone())
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: format!("BYO-VPC subnetwork '{}' not found", subnet_name),
                        operation: Some("verify_byo_vpc".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                self.subnetwork_self_link = subnetwork.self_link;
                self.cidr_block = subnetwork.ip_cidr_range;

                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }

            NetworkSettings::Create { cidr, .. } => {
                // Create new VPC network
                let gcp_config = ctx.get_gcp_config()?;
                let network_name = self.get_network_name(ctx.resource_prefix, &config.id);
                let cidr_block = cidr
                    .clone()
                    .unwrap_or_else(|| Self::generate_cidr_from_id(&config.id));

                // Get region from GCP config
                let region = gcp_config.region.clone();

                self.network_name = Some(network_name);
                self.cidr_block = Some(cidr_block);
                self.region = Some(region);
                self.is_byo_vpc = false;

                Ok(HandlerAction::Continue {
                    state: CreatingNetwork,
                    suggested_delay: None,
                })
            }

            _ => Err(AlienError::new(ErrorData::InfrastructureError {
                message: "Invalid network settings for GCP platform".to_string(),
                operation: Some("create_network".to_string()),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    #[handler(
        state = CreatingNetwork,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_network(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let network_name = self.network_name.clone().unwrap();

        info!(network_name = %network_name, "Creating GCP VPC network");

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        // Create custom-mode VPC (we control subnets)
        let network = GcpNetwork::builder()
            .name(network_name.clone())
            .description(format!("Alien-managed VPC for {}", ctx.resource_prefix))
            .auto_create_subnetworks(false)
            .routing_config(NetworkRoutingConfig {
                routing_mode: Some(RoutingMode::Regional),
            })
            .build();

        let operation = compute_client.insert_network(network).await.context(
            ErrorData::InfrastructureError {
                message: format!("Failed to create VPC network '{}'", network_name),
                operation: Some("insert_network".to_string()),
                resource_id: Some(config.id.clone()),
            },
        )?;

        self.pending_operation_name = operation.name;
        self.pending_operation_region = None; // Global operation

        Ok(HandlerAction::Continue {
            state: WaitingForNetwork,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForNetwork,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_network(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_global_operation(operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check network creation status".to_string(),
                operation: Some("get_global_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            debug!(operation_name = %operation_name, "Network creation still in progress");
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        if operation.has_error() {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::InfrastructureError {
                message: format!("Network creation failed: {}", error_msg),
                operation: Some("create_network".to_string()),
                resource_id: Some(config.id.clone()),
            }));
        }

        // Get the created network to store self_link
        let network_name = self.network_name.clone().unwrap();
        let network = compute_client
            .get_network(network_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to get created network '{}'", network_name),
                operation: Some("get_network".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.network_self_link = network.self_link;
        self.pending_operation_name = None;

        info!(network_name = %network_name, "VPC network created successfully");

        Ok(HandlerAction::Continue {
            state: CreatingSubnetwork,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingSubnetwork,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_subnetwork(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let subnetwork_name = self.get_subnetwork_name(ctx.resource_prefix, &config.id);
        let region = self.region.clone().unwrap();
        let cidr_block = self.cidr_block.clone().unwrap();
        let network_self_link = self.network_self_link.clone().unwrap();

        info!(
            subnetwork_name = %subnetwork_name,
            region = %region,
            cidr = %cidr_block,
            "Creating GCP subnetwork"
        );

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let subnetwork = Subnetwork::builder()
            .name(subnetwork_name.clone())
            .description(format!("Alien-managed subnet for {}", ctx.resource_prefix))
            .network(network_self_link)
            .ip_cidr_range(cidr_block)
            .private_ip_google_access(true)
            .build();

        let operation = compute_client
            .insert_subnetwork(region.clone(), subnetwork)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to create subnetwork '{}'", subnetwork_name),
                operation: Some("insert_subnetwork".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.subnetwork_name = Some(subnetwork_name);
        self.pending_operation_name = operation.name;
        self.pending_operation_region = Some(region);

        Ok(HandlerAction::Continue {
            state: WaitingForSubnetwork,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForSubnetwork,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_subnetwork(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();
        let region = self.pending_operation_region.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_region_operation(region.clone(), operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check subnetwork creation status".to_string(),
                operation: Some("get_region_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            debug!(operation_name = %operation_name, "Subnetwork creation still in progress");
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        if operation.has_error() {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::InfrastructureError {
                message: format!("Subnetwork creation failed: {}", error_msg),
                operation: Some("create_subnetwork".to_string()),
                resource_id: Some(config.id.clone()),
            }));
        }

        // Get the created subnetwork
        let subnetwork_name = self.subnetwork_name.clone().unwrap();
        let subnetwork = compute_client
            .get_subnetwork(region, subnetwork_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to get created subnetwork '{}'", subnetwork_name),
                operation: Some("get_subnetwork".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.subnetwork_self_link = subnetwork.self_link;
        self.pending_operation_name = None;
        self.pending_operation_region = None;

        info!(subnetwork_name = %subnetwork_name, "Subnetwork created successfully");

        // Create always provisions NAT; UseDefault and BYO rely on existing infra
        Ok(HandlerAction::Continue {
            state: CreatingRouter,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingRouter,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_router(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let router_name = self.get_router_name(ctx.resource_prefix, &config.id);
        let region = self.region.clone().unwrap();
        let network_self_link = self.network_self_link.clone().unwrap();

        info!(router_name = %router_name, region = %region, "Creating Cloud Router");

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let router = Router::builder()
            .name(router_name.clone())
            .description(format!("Alien-managed router for {}", ctx.resource_prefix))
            .network(network_self_link)
            .build();

        let operation = compute_client
            .insert_router(region.clone(), router)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to create router '{}'", router_name),
                operation: Some("insert_router".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.router_name = Some(router_name);
        self.pending_operation_name = operation.name;
        self.pending_operation_region = Some(region);

        Ok(HandlerAction::Continue {
            state: WaitingForRouter,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForRouter,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_router(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();
        let region = self.pending_operation_region.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_region_operation(region.clone(), operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check router creation status".to_string(),
                operation: Some("get_region_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            debug!(operation_name = %operation_name, "Router creation still in progress");
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        if operation.has_error() {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::InfrastructureError {
                message: format!("Router creation failed: {}", error_msg),
                operation: Some("create_router".to_string()),
                resource_id: Some(config.id.clone()),
            }));
        }

        self.pending_operation_name = None;
        self.pending_operation_region = None;

        info!(router_name = ?self.router_name, "Cloud Router created successfully");

        Ok(HandlerAction::Continue {
            state: CreatingCloudNat,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingCloudNat,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_cloud_nat(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let router_name = self.router_name.clone().unwrap();
        let cloud_nat_name = self.get_cloud_nat_name(ctx.resource_prefix, &config.id);
        let region = self.region.clone().unwrap();
        let subnetwork_self_link = self.subnetwork_self_link.clone().unwrap();

        info!(cloud_nat_name = %cloud_nat_name, router_name = %router_name, "Creating Cloud NAT");

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        // Get current router to add NAT config
        let mut router = compute_client
            .get_router(region.clone(), router_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: format!(
                    "Failed to get router '{}' for NAT configuration",
                    router_name
                ),
                operation: Some("get_router".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        // Add Cloud NAT configuration to router
        let nat_config = RouterNat::builder()
            .name(cloud_nat_name.clone())
            .nat_ip_allocate_option(alien_gcp_clients::compute::NatIpAllocateOption::AutoOnly)
            .source_subnetwork_ip_ranges_to_nat(
                alien_gcp_clients::compute::SourceSubnetworkIpRangesToNat::ListOfSubnetworks,
            )
            .subnetworks(vec![RouterNatSubnetworkToNat::builder()
                .name(subnetwork_self_link)
                .source_ip_ranges_to_nat(vec![SourceIpRangesToNat::AllIpRanges])
                .build()])
            .build();

        router.nats = vec![nat_config];

        let operation = compute_client
            .patch_router(region.clone(), router_name.clone(), router)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to configure Cloud NAT on router '{}'", router_name),
                operation: Some("patch_router".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.cloud_nat_name = Some(cloud_nat_name);
        self.pending_operation_name = operation.name;
        self.pending_operation_region = Some(region);

        Ok(HandlerAction::Continue {
            state: WaitingForCloudNat,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForCloudNat,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_cloud_nat(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();
        let region = self.pending_operation_region.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_region_operation(region.clone(), operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check Cloud NAT creation status".to_string(),
                operation: Some("get_region_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            debug!(operation_name = %operation_name, "Cloud NAT creation still in progress");
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        if operation.has_error() {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::InfrastructureError {
                message: format!("Cloud NAT creation failed: {}", error_msg),
                operation: Some("create_cloud_nat".to_string()),
                resource_id: Some(config.id.clone()),
            }));
        }

        self.pending_operation_name = None;
        self.pending_operation_region = None;

        info!(cloud_nat_name = ?self.cloud_nat_name, "Cloud NAT created successfully");

        Ok(HandlerAction::Continue {
            state: CreatingFirewall,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingFirewall,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_firewall(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let firewall_name = self.get_firewall_name(ctx.resource_prefix, &config.id);
        let network_self_link = self.network_self_link.clone().unwrap();
        let cidr_block = self.cidr_block.clone().unwrap();

        info!(firewall_name = %firewall_name, "Creating firewall rule for internal traffic");

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        // Create firewall rule allowing internal traffic
        let firewall = Firewall::builder()
            .name(firewall_name.clone())
            .description(format!(
                "Allow internal traffic for {}",
                ctx.resource_prefix
            ))
            .network(network_self_link)
            .direction(FirewallDirection::Ingress)
            .source_ranges(vec![cidr_block])
            .allowed(vec![
                FirewallAllowed::builder()
                    .ip_protocol("tcp".to_string())
                    .build(),
                FirewallAllowed::builder()
                    .ip_protocol("udp".to_string())
                    .build(),
                FirewallAllowed::builder()
                    .ip_protocol("icmp".to_string())
                    .build(),
            ])
            .priority(1000)
            .build();

        let operation = compute_client.insert_firewall(firewall).await.context(
            ErrorData::InfrastructureError {
                message: format!("Failed to create firewall rule '{}'", firewall_name),
                operation: Some("insert_firewall".to_string()),
                resource_id: Some(config.id.clone()),
            },
        )?;

        self.firewall_name = Some(firewall_name);
        self.pending_operation_name = operation.name;
        self.pending_operation_region = None; // Firewall is global

        Ok(HandlerAction::Continue {
            state: WaitingForFirewall,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForFirewall,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_firewall(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_global_operation(operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check firewall creation status".to_string(),
                operation: Some("get_global_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            debug!(operation_name = %operation_name, "Firewall creation still in progress");
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        if operation.has_error() {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::InfrastructureError {
                message: format!("Firewall creation failed: {}", error_msg),
                operation: Some("create_firewall".to_string()),
                resource_id: Some(config.id.clone()),
            }));
        }

        self.pending_operation_name = None;

        info!(
            firewall_name = ?self.firewall_name,
            network_name = ?self.network_name,
            "GCP Network infrastructure created successfully"
        );

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;

        // For BYO-VPC, we don't need to verify
        if self.is_byo_vpc {
            debug!(network_id = %config.id, "BYO-VPC network ready");
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(std::time::Duration::from_secs(60)),
            });
        }

        // For created networks, verify network still exists
        if let Some(network_name) = &self.network_name {
            let gcp_config = ctx.get_gcp_config()?;
            let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

            let _ = compute_client
                .get_network(network_name.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to verify network during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            debug!(network_name = %network_name, "Network exists and is accessible");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(std::time::Duration::from_secs(60)),
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
        info!(network_id = %config.id, "Network update requested - GCP Network updates are mostly immutable");
        // GCP Network updates are mostly immutable after creation
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────────

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        info!(network_id = %config.id, "Starting GCP Network deletion");

        if self.is_byo_vpc {
            info!("BYO-VPC mode - skipping deletion of external resources");
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        // Start deletion flow - firewall first
        if self.firewall_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingFirewall,
                suggested_delay: None,
            })
        } else if self.cloud_nat_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingCloudNat,
                suggested_delay: None,
            })
        } else if self.router_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingRouter,
                suggested_delay: None,
            })
        } else if self.subnetwork_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingSubnetwork,
                suggested_delay: None,
            })
        } else if self.network_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingNetwork,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = DeletingFirewall,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_firewall(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let firewall_name = self.firewall_name.clone().unwrap();

        info!(firewall_name = %firewall_name, "Deleting firewall rule");

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .delete_firewall(firewall_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to delete firewall '{}'", firewall_name),
                operation: Some("delete_firewall".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.pending_operation_name = operation.name;

        Ok(HandlerAction::Continue {
            state: WaitingForFirewallDeletion,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForFirewallDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_firewall_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_global_operation(operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check firewall deletion status".to_string(),
                operation: Some("get_global_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        self.firewall_name = None;
        self.pending_operation_name = None;

        if self.cloud_nat_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingCloudNat,
                suggested_delay: None,
            })
        } else if self.router_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingRouter,
                suggested_delay: None,
            })
        } else if self.subnetwork_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingSubnetwork,
                suggested_delay: None,
            })
        } else if self.network_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingNetwork,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            })
        }
    }

    #[handler(
        state = DeletingCloudNat,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_cloud_nat(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let router_name = self.router_name.clone().unwrap();
        let region = self.region.clone().unwrap();

        info!(router_name = %router_name, "Removing Cloud NAT from router");

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        // Get router and remove NAT config
        let mut router = compute_client
            .get_router(region.clone(), router_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to get router '{}' for NAT removal", router_name),
                operation: Some("get_router".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        router.nats = vec![]; // Remove all NAT configs

        let operation = compute_client
            .patch_router(region.clone(), router_name.clone(), router)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to remove Cloud NAT from router '{}'", router_name),
                operation: Some("patch_router".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.pending_operation_name = operation.name;
        self.pending_operation_region = Some(region);

        Ok(HandlerAction::Continue {
            state: WaitingForCloudNatDeletion,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForCloudNatDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_cloud_nat_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();
        let region = self.pending_operation_region.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_region_operation(region, operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check Cloud NAT deletion status".to_string(),
                operation: Some("get_region_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        self.cloud_nat_name = None;
        self.pending_operation_name = None;
        self.pending_operation_region = None;

        Ok(HandlerAction::Continue {
            state: DeletingRouter,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRouter,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_router(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let router_name = self.router_name.clone().unwrap();
        let region = self.region.clone().unwrap();

        info!(router_name = %router_name, "Deleting Cloud Router");

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .delete_router(region.clone(), router_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to delete router '{}'", router_name),
                operation: Some("delete_router".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.pending_operation_name = operation.name;
        self.pending_operation_region = Some(region);

        Ok(HandlerAction::Continue {
            state: WaitingForRouterDeletion,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForRouterDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_router_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();
        let region = self.pending_operation_region.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_region_operation(region, operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check router deletion status".to_string(),
                operation: Some("get_region_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        self.router_name = None;
        self.pending_operation_name = None;
        self.pending_operation_region = None;

        Ok(HandlerAction::Continue {
            state: DeletingSubnetwork,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingSubnetwork,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_subnetwork(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let subnetwork_name = self.subnetwork_name.clone().unwrap();
        let region = self.region.clone().unwrap();

        info!(subnetwork_name = %subnetwork_name, "Deleting subnetwork");

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .delete_subnetwork(region.clone(), subnetwork_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to delete subnetwork '{}'", subnetwork_name),
                operation: Some("delete_subnetwork".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.pending_operation_name = operation.name;
        self.pending_operation_region = Some(region);

        Ok(HandlerAction::Continue {
            state: WaitingForSubnetworkDeletion,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForSubnetworkDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_subnetwork_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();
        let region = self.pending_operation_region.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_region_operation(region, operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check subnetwork deletion status".to_string(),
                operation: Some("get_region_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        self.subnetwork_name = None;
        self.subnetwork_self_link = None;
        self.pending_operation_name = None;
        self.pending_operation_region = None;

        Ok(HandlerAction::Continue {
            state: DeletingNetwork,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingNetwork,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_network(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let network_name = self.network_name.clone().unwrap();

        info!(network_name = %network_name, "Deleting VPC network");

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .delete_network(network_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to delete network '{}'", network_name),
                operation: Some("delete_network".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.pending_operation_name = operation.name;

        Ok(HandlerAction::Continue {
            state: WaitingForNetworkDeletion,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForNetworkDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_network_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let operation_name = self.pending_operation_name.clone().unwrap();

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = compute_client
            .get_global_operation(operation_name.clone())
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check network deletion status".to_string(),
                operation: Some("get_global_operation".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if !operation.is_done() {
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            });
        }

        self.network_name = None;
        self.network_self_link = None;
        self.pending_operation_name = None;

        info!("GCP Network infrastructure deleted successfully");

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
}
