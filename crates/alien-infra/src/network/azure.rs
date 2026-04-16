//! Azure Network controller for VNet and subnet management.
//!
//! This controller manages:
//! - Virtual Networks (VNets) with custom address spaces
//! - Subnets for workload isolation
//! - NAT Gateways for private subnet internet access
//! - Public IP addresses for NAT
//! - Network Security Groups (NSGs) for traffic control

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils;
use alien_azure_clients::azure::models::{
    nat_gateway::{
        NatGateway, NatGatewayPropertiesFormat, NatGatewaySku, NatGatewaySkuName,
        SubResource as NatSubResource,
    },
    network_security_group::{
        NetworkSecurityGroup, NetworkSecurityGroupPropertiesFormat, SecurityRule,
        SecurityRuleAccess, SecurityRuleDirection, SecurityRulePropertiesFormat,
        SecurityRulePropertiesFormatProtocol,
    },
    public_ip_address::{
        IpAllocationMethod, PublicIpAddress, PublicIpAddressPropertiesFormat, PublicIpAddressSku,
        PublicIpAddressSkuName,
    },
    virtual_network::{
        AddressSpace, SubResource as VnetSubResource, Subnet, SubnetPropertiesFormat,
        VirtualNetwork, VirtualNetworkPropertiesFormat,
    },
};
use alien_azure_clients::long_running_operation::OperationResult;
use alien_core::{Network, NetworkSettings, ResourceStatus};
use alien_error::{AlienError, Context};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use tracing::{debug, info};

// =============================================================================================
// Controller
// =============================================================================================

/// Azure Network controller for managing VNets, subnets, NAT Gateways, and NSGs.
#[controller]
pub struct AzureNetworkController {
    // Configuration from Network resource
    pub desired_settings: Option<NetworkSettings>,

    // Created or imported network details
    pub vnet_name: Option<String>,
    pub vnet_resource_id: Option<String>,
    pub public_subnet_name: Option<String>,
    pub private_subnet_name: Option<String>,
    pub(crate) nat_gateway_name: Option<String>,
    pub nat_gateway_id: Option<String>,
    pub(crate) public_ip_name: Option<String>,
    pub(crate) public_ip_id: Option<String>,
    pub(crate) nsg_name: Option<String>,
    pub(crate) nsg_id: Option<String>,
    pub resource_group: Option<String>,
    pub(crate) location: Option<String>,
    pub cidr_block: Option<String>,
    pub(crate) is_byo_vnet: bool,
}

impl AzureNetworkController {
    /// Generate a deterministic CIDR block from the stack/network ID.
    fn generate_cidr_from_id(network_id: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        network_id.hash(&mut hasher);
        let hash = hasher.finish();

        let second_octet = 64 + ((hash % 64) as u8);
        format!("100.{}.0.0/16", second_octet)
    }

    fn get_vnet_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-vnet", resource_prefix, network_id)
    }

    fn get_public_subnet_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-public-subnet", resource_prefix, network_id)
    }

    fn get_private_subnet_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-private-subnet", resource_prefix, network_id)
    }

    fn get_nat_gateway_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-nat", resource_prefix, network_id)
    }

    fn get_public_ip_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-pip", resource_prefix, network_id)
    }

    fn get_nsg_name(&self, resource_prefix: &str, network_id: &str) -> String {
        format!("{}-{}-nsg", resource_prefix, network_id)
    }

    fn calculate_public_subnet_cidr(cidr: &str) -> String {
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() == 2 {
            let prefix_len: u8 = parts[1].parse().unwrap_or(16);
            format!("{}/{}", parts[0], prefix_len + 1)
        } else {
            cidr.to_string()
        }
    }

    fn calculate_private_subnet_cidr(cidr: &str) -> String {
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() == 2 {
            let addr_parts: Vec<&str> = parts[0].split('.').collect();
            if addr_parts.len() == 4 {
                let third_octet: u8 = addr_parts[2].parse().unwrap_or(0);
                let prefix_len: u8 = parts[1].parse().unwrap_or(16);
                format!(
                    "{}.{}.{}.{}/{}",
                    addr_parts[0],
                    addr_parts[1],
                    third_octet + 128,
                    addr_parts[3],
                    prefix_len + 1
                )
            } else {
                cidr.to_string()
            }
        } else {
            cidr.to_string()
        }
    }
}

impl AzureNetworkController {
    /// Creates a controller in a ready state with mock values for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(network_id: &str) -> Self {
        let vnet_name = format!("test-{}-vnet", network_id);
        let resource_group = "test-resource-group".to_string();
        let subscription_id = "test-subscription".to_string();

        Self {
            state: AzureNetworkState::Ready,
            desired_settings: None,
            vnet_name: Some(vnet_name.clone()),
            vnet_resource_id: Some(format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}",
                subscription_id, resource_group, vnet_name
            )),
            public_subnet_name: Some(format!("test-{}-public-subnet", network_id)),
            private_subnet_name: Some(format!("test-{}-private-subnet", network_id)),
            nat_gateway_name: Some(format!("test-{}-nat", network_id)),
            nat_gateway_id: Some(format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/natGateways/test-{}-nat",
                subscription_id, resource_group, network_id
            )),
            public_ip_name: Some(format!("test-{}-pip", network_id)),
            public_ip_id: Some(format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/publicIPAddresses/test-{}-pip",
                subscription_id, resource_group, network_id
            )),
            nsg_name: Some(format!("test-{}-nsg", network_id)),
            nsg_id: Some(format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/networkSecurityGroups/test-{}-nsg",
                subscription_id, resource_group, network_id
            )),
            resource_group: Some(resource_group),
            location: Some("eastus".to_string()),
            cidr_block: Some("10.0.0.0/16".to_string()),
            is_byo_vnet: false,
            _internal_stay_count: None,
        }
    }
}

#[controller]
impl AzureNetworkController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;

        info!(network_id = %config.id, "Starting Azure Network creation");

        self.desired_settings = Some(config.settings.clone());

        match &config.settings {
            // Azure has no default VNet — UseDefault behaves like Create with defaults
            NetworkSettings::UseDefault | NetworkSettings::Create { .. } => {
                let cidr = match &config.settings {
                    NetworkSettings::Create { cidr, .. } => cidr.clone(),
                    _ => None,
                };

                let azure_config = ctx.get_azure_config()?;
                let resource_group = azure_utils::get_resource_group_name(ctx.state)?;
                let location = azure_config
                    .region
                    .clone()
                    .unwrap_or_else(|| "eastus".to_string());
                let vnet_name = self.get_vnet_name(ctx.resource_prefix, &config.id);
                let cidr_block = cidr.unwrap_or_else(|| Self::generate_cidr_from_id(&config.id));

                self.vnet_name = Some(vnet_name);
                self.cidr_block = Some(cidr_block);
                self.resource_group = Some(resource_group);
                self.location = Some(location);
                self.is_byo_vnet = false;

                Ok(HandlerAction::Continue {
                    state: CreatingVNet,
                    suggested_delay: None,
                })
            }

            NetworkSettings::ByoVnetAzure {
                vnet_resource_id,
                public_subnet_name,
                private_subnet_name,
            } => {
                info!(
                    vnet_resource_id = %vnet_resource_id,
                    public_subnet = %public_subnet_name,
                    private_subnet = %private_subnet_name,
                    "Using existing Azure VNet"
                );

                // Parse resource ID
                let parts: Vec<&str> = vnet_resource_id.split('/').collect();
                let rg_idx = parts.iter().position(|&p| p == "resourceGroups");
                let vnet_idx = parts.iter().position(|&p| p == "virtualNetworks");

                let (resource_group, vnet_name) = match (rg_idx, vnet_idx) {
                    (Some(rg), Some(vn)) if rg + 1 < parts.len() && vn + 1 < parts.len() => {
                        (parts[rg + 1].to_string(), parts[vn + 1].to_string())
                    }
                    _ => {
                        return Err(AlienError::new(ErrorData::InfrastructureError {
                            message: format!(
                                "Invalid VNet resource ID format: {}",
                                vnet_resource_id
                            ),
                            operation: Some("parse_vnet_resource_id".to_string()),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                };

                self.resource_group = Some(resource_group.clone());
                self.vnet_name = Some(vnet_name.clone());
                self.vnet_resource_id = Some(vnet_resource_id.clone());
                self.public_subnet_name = Some(public_subnet_name.clone());
                self.private_subnet_name = Some(private_subnet_name.clone());
                self.is_byo_vnet = true;

                let azure_config = ctx.get_azure_config()?;
                let network_client = ctx
                    .service_provider
                    .get_azure_network_client(azure_config)?;

                let vnet = network_client
                    .get_virtual_network(&resource_group, &vnet_name)
                    .await
                    .context(ErrorData::InfrastructureError {
                        message: format!("BYO-VNet '{}' not found", vnet_name),
                        operation: Some("verify_byo_vnet".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                self.location = vnet.location;
                if let Some(props) = &vnet.properties {
                    if let Some(addr_space) = &props.address_space {
                        self.cidr_block = addr_space.address_prefixes.first().cloned();
                    }
                }

                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }

            _ => Err(AlienError::new(ErrorData::InfrastructureError {
                message: "Invalid network settings for Azure platform".to_string(),
                operation: Some("create_network".to_string()),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    #[handler(
        state = CreatingVNet,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_vnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();
        let location = self.location.clone().unwrap();
        let cidr_block = self.cidr_block.clone().unwrap();

        info!(vnet_name = %vnet_name, cidr = %cidr_block, "Creating Azure VNet");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let vnet = VirtualNetwork {
            location: Some(location),
            properties: Some(VirtualNetworkPropertiesFormat {
                address_space: Some(AddressSpace {
                    address_prefixes: vec![cidr_block],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            tags: [("managed-by".to_string(), "alien".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let result = network_client
            .create_or_update_virtual_network(&resource_group, &vnet_name, &vnet)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to create VNet '{}'", vnet_name),
                operation: Some("create_or_update_virtual_network".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        match result {
            OperationResult::Completed(created_vnet) => {
                self.vnet_resource_id = created_vnet.id;
                Ok(HandlerAction::Continue {
                    state: CreatingPublicSubnet,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForVNet,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = WaitingForVNet,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_vnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let vnet = network_client
            .get_virtual_network(&resource_group, &vnet_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check VNet creation status".to_string(),
                operation: Some("get_virtual_network".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.vnet_resource_id = vnet.id;
        info!(vnet_name = %vnet_name, "VNet created successfully");

        Ok(HandlerAction::Continue {
            state: CreatingPublicSubnet,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPublicSubnet,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_public_subnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();
        let public_subnet_name = self.get_public_subnet_name(ctx.resource_prefix, &config.id);
        let cidr_block = self.cidr_block.clone().unwrap();
        let public_cidr = Self::calculate_public_subnet_cidr(&cidr_block);

        info!(subnet_name = %public_subnet_name, cidr = %public_cidr, "Creating public subnet");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let subnet = Subnet {
            properties: Some(SubnetPropertiesFormat {
                address_prefix: Some(public_cidr),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = network_client
            .create_or_update_subnet(&resource_group, &vnet_name, &public_subnet_name, &subnet)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to create public subnet '{}'", public_subnet_name),
                operation: Some("create_or_update_subnet".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.public_subnet_name = Some(public_subnet_name);

        match result {
            OperationResult::Completed(_) => Ok(HandlerAction::Continue {
                state: CreatingPrivateSubnet,
                suggested_delay: None,
            }),
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForPublicSubnet,
                suggested_delay: Some(std::time::Duration::from_secs(3)),
            }),
        }
    }

    #[handler(
        state = WaitingForPublicSubnet,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_public_subnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();
        let public_subnet_name = self.public_subnet_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let _ = network_client
            .get_subnet(&resource_group, &vnet_name, &public_subnet_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check public subnet creation status".to_string(),
                operation: Some("get_subnet".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        info!(subnet_name = %public_subnet_name, "Public subnet created");

        Ok(HandlerAction::Continue {
            state: CreatingPrivateSubnet,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPrivateSubnet,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_private_subnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();
        let private_subnet_name = self.get_private_subnet_name(ctx.resource_prefix, &config.id);
        let cidr_block = self.cidr_block.clone().unwrap();
        let private_cidr = Self::calculate_private_subnet_cidr(&cidr_block);

        info!(subnet_name = %private_subnet_name, cidr = %private_cidr, "Creating private subnet");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let subnet = Subnet {
            properties: Some(SubnetPropertiesFormat {
                address_prefix: Some(private_cidr),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = network_client
            .create_or_update_subnet(&resource_group, &vnet_name, &private_subnet_name, &subnet)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to create private subnet '{}'", private_subnet_name),
                operation: Some("create_or_update_subnet".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.private_subnet_name = Some(private_subnet_name);

        match result {
            OperationResult::Completed(_) => Ok(HandlerAction::Continue {
                state: CreatingPublicIp,
                suggested_delay: None,
            }),
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForPrivateSubnet,
                suggested_delay: Some(std::time::Duration::from_secs(3)),
            }),
        }
    }

    #[handler(
        state = WaitingForPrivateSubnet,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_private_subnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();
        let private_subnet_name = self.private_subnet_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let _ = network_client
            .get_subnet(&resource_group, &vnet_name, &private_subnet_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check private subnet creation status".to_string(),
                operation: Some("get_subnet".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        info!(subnet_name = %private_subnet_name, "Private subnet created");

        Ok(HandlerAction::Continue {
            state: CreatingPublicIp,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPublicIp,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_public_ip(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let location = self.location.clone().unwrap();
        let public_ip_name = self.get_public_ip_name(ctx.resource_prefix, &config.id);

        info!(public_ip_name = %public_ip_name, "Creating public IP for NAT Gateway");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let public_ip = PublicIpAddress {
            location: Some(location),
            sku: Some(PublicIpAddressSku {
                name: Some(PublicIpAddressSkuName::Standard),
                tier: None,
            }),
            properties: Some(Box::new(PublicIpAddressPropertiesFormat {
                public_ip_allocation_method: Some(IpAllocationMethod::Static),
                ..Default::default()
            })),
            tags: [("managed-by".to_string(), "alien".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let result = network_client
            .create_or_update_public_ip_address(&resource_group, &public_ip_name, &public_ip)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to create public IP '{}'", public_ip_name),
                operation: Some("create_or_update_public_ip_address".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.public_ip_name = Some(public_ip_name);

        match result {
            OperationResult::Completed(created_ip) => {
                self.public_ip_id = created_ip.id;
                Ok(HandlerAction::Continue {
                    state: CreatingNatGateway,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForPublicIp,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = WaitingForPublicIp,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_public_ip(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let public_ip_name = self.public_ip_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let public_ip = network_client
            .get_public_ip_address(&resource_group, &public_ip_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check public IP creation status".to_string(),
                operation: Some("get_public_ip_address".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.public_ip_id = public_ip.id;
        info!(public_ip_name = %public_ip_name, "Public IP created");

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
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let location = self.location.clone().unwrap();
        let nat_gateway_name = self.get_nat_gateway_name(ctx.resource_prefix, &config.id);
        let public_ip_id = self.public_ip_id.clone().unwrap();

        info!(nat_gateway_name = %nat_gateway_name, "Creating NAT Gateway");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let nat_gateway = NatGateway {
            location: Some(location),
            sku: Some(NatGatewaySku {
                name: Some(NatGatewaySkuName::Standard),
            }),
            properties: Some(NatGatewayPropertiesFormat {
                public_ip_addresses: vec![NatSubResource {
                    id: Some(public_ip_id),
                }],
                idle_timeout_in_minutes: Some(4),
                ..Default::default()
            }),
            tags: [("managed-by".to_string(), "alien".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let result = network_client
            .create_or_update_nat_gateway(&resource_group, &nat_gateway_name, &nat_gateway)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to create NAT Gateway '{}'", nat_gateway_name),
                operation: Some("create_or_update_nat_gateway".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.nat_gateway_name = Some(nat_gateway_name);

        match result {
            OperationResult::Completed(created_nat) => {
                self.nat_gateway_id = created_nat.id;
                Ok(HandlerAction::Continue {
                    state: AssociatingNatToSubnet,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForNatGateway,
                suggested_delay: Some(std::time::Duration::from_secs(10)),
            }),
        }
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
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let nat_gateway_name = self.nat_gateway_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let nat_gateway = network_client
            .get_nat_gateway(&resource_group, &nat_gateway_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check NAT Gateway creation status".to_string(),
                operation: Some("get_nat_gateway".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.nat_gateway_id = nat_gateway.id;
        info!(nat_gateway_name = %nat_gateway_name, "NAT Gateway created");

        Ok(HandlerAction::Continue {
            state: AssociatingNatToSubnet,
            suggested_delay: None,
        })
    }

    #[handler(
        state = AssociatingNatToSubnet,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn associating_nat_to_subnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();
        let private_subnet_name = self.private_subnet_name.clone().unwrap();
        let nat_gateway_id = self.nat_gateway_id.clone().unwrap();
        let cidr_block = self.cidr_block.clone().unwrap();

        info!(subnet_name = %private_subnet_name, nat_gateway_id = %nat_gateway_id, "Associating NAT Gateway with private subnet");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let subnet = Subnet {
            properties: Some(SubnetPropertiesFormat {
                address_prefix: Some(Self::calculate_private_subnet_cidr(&cidr_block)),
                nat_gateway: Some(VnetSubResource {
                    id: Some(nat_gateway_id),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = network_client
            .create_or_update_subnet(&resource_group, &vnet_name, &private_subnet_name, &subnet)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!(
                    "Failed to associate NAT Gateway with subnet '{}'",
                    private_subnet_name
                ),
                operation: Some("create_or_update_subnet".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        match result {
            OperationResult::Completed(_) => Ok(HandlerAction::Continue {
                state: CreatingNsg,
                suggested_delay: None,
            }),
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForNatAssociation,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = WaitingForNatAssociation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_nat_association(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();
        let private_subnet_name = self.private_subnet_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let subnet = network_client
            .get_subnet(&resource_group, &vnet_name, &private_subnet_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check NAT association status".to_string(),
                operation: Some("get_subnet".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if let Some(props) = subnet.properties {
            if props.nat_gateway.is_some() {
                info!("NAT Gateway associated with private subnet");
                return Ok(HandlerAction::Continue {
                    state: CreatingNsg,
                    suggested_delay: None,
                });
            }
        }

        debug!("NAT Gateway association still in progress");
        Ok(HandlerAction::Stay {
            max_times: 60,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
        })
    }

    #[handler(
        state = CreatingNsg,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_nsg(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let location = self.location.clone().unwrap();
        let nsg_name = self.get_nsg_name(ctx.resource_prefix, &config.id);
        let cidr_block = self.cidr_block.clone().unwrap();

        info!(nsg_name = %nsg_name, "Creating Network Security Group");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let nsg = NetworkSecurityGroup {
            location: Some(location),
            properties: Some(NetworkSecurityGroupPropertiesFormat {
                security_rules: vec![SecurityRule {
                    name: Some("AllowVNetInBound".to_string()),
                    properties: Some(SecurityRulePropertiesFormat {
                        priority: 100,
                        direction: SecurityRuleDirection::Inbound,
                        access: SecurityRuleAccess::Allow,
                        protocol: SecurityRulePropertiesFormatProtocol::X,
                        source_address_prefix: Some(cidr_block),
                        source_port_range: Some("*".to_string()),
                        destination_address_prefix: Some("*".to_string()),
                        destination_port_range: Some("*".to_string()),
                        description: None,
                        destination_address_prefixes: vec![],
                        destination_application_security_groups: vec![],
                        destination_port_ranges: vec![],
                        provisioning_state: None,
                        source_address_prefixes: vec![],
                        source_application_security_groups: vec![],
                        source_port_ranges: vec![],
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            tags: [("managed-by".to_string(), "alien".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        let result = network_client
            .create_or_update_network_security_group(&resource_group, &nsg_name, &nsg)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to create NSG '{}'", nsg_name),
                operation: Some("create_or_update_network_security_group".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.nsg_name = Some(nsg_name);

        match result {
            OperationResult::Completed(created_nsg) => {
                self.nsg_id = created_nsg.id;
                info!("Azure Network infrastructure created successfully");
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForNsg,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = WaitingForNsg,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_nsg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let nsg_name = self.nsg_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let nsg = network_client
            .get_network_security_group(&resource_group, &nsg_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check NSG creation status".to_string(),
                operation: Some("get_network_security_group".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        self.nsg_id = nsg.id;
        info!(nsg_name = %nsg_name, vnet_name = ?self.vnet_name, "Azure Network infrastructure created successfully");

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

        // For BYO-VNet, we don't need to verify
        if self.is_byo_vnet {
            debug!(network_id = %config.id, "BYO-VNet network ready");
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(std::time::Duration::from_secs(60)),
            });
        }

        // For created VNets, verify VNet still exists
        if let (Some(resource_group), Some(vnet_name)) = (&self.resource_group, &self.vnet_name) {
            let azure_config = ctx.get_azure_config()?;
            let network_client = ctx
                .service_provider
                .get_azure_network_client(azure_config)?;

            let _ = network_client
                .get_virtual_network(resource_group, vnet_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to verify VNet during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            debug!(vnet_name = %vnet_name, "VNet exists and is accessible");
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
        info!(network_id = %config.id, "Network update requested - Azure Network updates are mostly immutable");
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
        info!(network_id = %config.id, "Starting Azure Network deletion");

        if self.is_byo_vnet {
            info!("BYO-VNet mode - skipping deletion of external resources");
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        if self.nsg_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingNsg,
                suggested_delay: None,
            })
        } else if self.nat_gateway_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DissociatingNatFromSubnet,
                suggested_delay: None,
            })
        } else if self.vnet_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingVNet,
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
        state = DeletingNsg,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_nsg(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let nsg_name = self.nsg_name.clone().unwrap();

        info!(nsg_name = %nsg_name, "Deleting NSG");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let result = network_client
            .delete_network_security_group(&resource_group, &nsg_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to delete NSG '{}'", nsg_name),
                operation: Some("delete_network_security_group".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        match result {
            OperationResult::Completed(()) => {
                self.nsg_name = None;
                self.nsg_id = None;
                if self.nat_gateway_name.is_some() {
                    Ok(HandlerAction::Continue {
                        state: DissociatingNatFromSubnet,
                        suggested_delay: None,
                    })
                } else {
                    Ok(HandlerAction::Continue {
                        state: DeletingVNet,
                        suggested_delay: None,
                    })
                }
            }
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForNsgDeletion,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = WaitingForNsgDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_nsg_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let resource_group = self.resource_group.clone().unwrap();
        let nsg_name = self.nsg_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        match network_client
            .get_network_security_group(&resource_group, &nsg_name)
            .await
        {
            Ok(_) => {
                debug!("NSG deletion still in progress");
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(std::time::Duration::from_secs(5)),
                })
            }
            Err(_) => {
                self.nsg_name = None;
                self.nsg_id = None;
                if self.nat_gateway_name.is_some() {
                    Ok(HandlerAction::Continue {
                        state: DissociatingNatFromSubnet,
                        suggested_delay: None,
                    })
                } else {
                    Ok(HandlerAction::Continue {
                        state: DeletingVNet,
                        suggested_delay: None,
                    })
                }
            }
        }
    }

    #[handler(
        state = DissociatingNatFromSubnet,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn dissociating_nat_from_subnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();
        let private_subnet_name = self.private_subnet_name.clone().unwrap();
        let cidr_block = self.cidr_block.clone().unwrap();

        info!(subnet_name = %private_subnet_name, "Dissociating NAT Gateway from subnet");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let subnet = Subnet {
            properties: Some(SubnetPropertiesFormat {
                address_prefix: Some(Self::calculate_private_subnet_cidr(&cidr_block)),
                nat_gateway: None,
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = network_client
            .create_or_update_subnet(&resource_group, &vnet_name, &private_subnet_name, &subnet)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!(
                    "Failed to dissociate NAT Gateway from subnet '{}'",
                    private_subnet_name
                ),
                operation: Some("create_or_update_subnet".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        match result {
            OperationResult::Completed(_) => Ok(HandlerAction::Continue {
                state: DeletingNatGateway,
                suggested_delay: None,
            }),
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForNatDissociation,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = WaitingForNatDissociation,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_nat_dissociation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let vnet_name = self.vnet_name.clone().unwrap();
        let resource_group = self.resource_group.clone().unwrap();
        let private_subnet_name = self.private_subnet_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let subnet = network_client
            .get_subnet(&resource_group, &vnet_name, &private_subnet_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to check NAT dissociation status".to_string(),
                operation: Some("get_subnet".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        if let Some(props) = subnet.properties {
            if props.nat_gateway.is_none() {
                return Ok(HandlerAction::Continue {
                    state: DeletingNatGateway,
                    suggested_delay: None,
                });
            }
        }

        debug!("NAT Gateway dissociation still in progress");
        Ok(HandlerAction::Stay {
            max_times: 60,
            suggested_delay: Some(std::time::Duration::from_secs(5)),
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
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let nat_gateway_name = self.nat_gateway_name.clone().unwrap();

        info!(nat_gateway_name = %nat_gateway_name, "Deleting NAT Gateway");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let result = network_client
            .delete_nat_gateway(&resource_group, &nat_gateway_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to delete NAT Gateway '{}'", nat_gateway_name),
                operation: Some("delete_nat_gateway".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        match result {
            OperationResult::Completed(()) => {
                self.nat_gateway_name = None;
                self.nat_gateway_id = None;
                Ok(HandlerAction::Continue {
                    state: DeletingPublicIp,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForNatGatewayDeletion,
                suggested_delay: Some(std::time::Duration::from_secs(10)),
            }),
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
        let resource_group = self.resource_group.clone().unwrap();
        let nat_gateway_name = self.nat_gateway_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        match network_client
            .get_nat_gateway(&resource_group, &nat_gateway_name)
            .await
        {
            Ok(_) => {
                debug!("NAT Gateway deletion still in progress");
                Ok(HandlerAction::Stay {
                    max_times: 30,
                    suggested_delay: Some(std::time::Duration::from_secs(10)),
                })
            }
            Err(_) => {
                self.nat_gateway_name = None;
                self.nat_gateway_id = None;
                Ok(HandlerAction::Continue {
                    state: DeletingPublicIp,
                    suggested_delay: None,
                })
            }
        }
    }

    #[handler(
        state = DeletingPublicIp,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_public_ip(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let public_ip_name = match &self.public_ip_name {
            Some(name) => name.clone(),
            None => {
                return Ok(HandlerAction::Continue {
                    state: DeletingVNet,
                    suggested_delay: None,
                })
            }
        };

        info!(public_ip_name = %public_ip_name, "Deleting Public IP");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let result = network_client
            .delete_public_ip_address(&resource_group, &public_ip_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to delete Public IP '{}'", public_ip_name),
                operation: Some("delete_public_ip_address".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        match result {
            OperationResult::Completed(()) => {
                self.public_ip_name = None;
                self.public_ip_id = None;
                Ok(HandlerAction::Continue {
                    state: DeletingVNet,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForPublicIpDeletion,
                suggested_delay: Some(std::time::Duration::from_secs(5)),
            }),
        }
    }

    #[handler(
        state = WaitingForPublicIpDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_public_ip_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let resource_group = self.resource_group.clone().unwrap();
        let public_ip_name = self.public_ip_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        match network_client
            .get_public_ip_address(&resource_group, &public_ip_name)
            .await
        {
            Ok(_) => {
                debug!("Public IP deletion still in progress");
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(std::time::Duration::from_secs(5)),
                })
            }
            Err(_) => {
                self.public_ip_name = None;
                self.public_ip_id = None;
                Ok(HandlerAction::Continue {
                    state: DeletingVNet,
                    suggested_delay: None,
                })
            }
        }
    }

    #[handler(
        state = DeletingVNet,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_vnet(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Network>()?;
        let resource_group = self.resource_group.clone().unwrap();
        let vnet_name = self.vnet_name.clone().unwrap();

        info!(vnet_name = %vnet_name, "Deleting VNet");

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        let result = network_client
            .delete_virtual_network(&resource_group, &vnet_name)
            .await
            .context(ErrorData::InfrastructureError {
                message: format!("Failed to delete VNet '{}'", vnet_name),
                operation: Some("delete_virtual_network".to_string()),
                resource_id: Some(config.id.clone()),
            })?;

        match result {
            OperationResult::Completed(()) => {
                info!("Azure Network infrastructure deleted successfully");
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(_) => Ok(HandlerAction::Continue {
                state: WaitingForVNetDeletion,
                suggested_delay: Some(std::time::Duration::from_secs(10)),
            }),
        }
    }

    #[handler(
        state = WaitingForVNetDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_vnet_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let resource_group = self.resource_group.clone().unwrap();
        let vnet_name = self.vnet_name.clone().unwrap();

        let azure_config = ctx.get_azure_config()?;
        let network_client = ctx
            .service_provider
            .get_azure_network_client(azure_config)?;

        match network_client
            .get_virtual_network(&resource_group, &vnet_name)
            .await
        {
            Ok(_) => {
                debug!("VNet deletion still in progress");
                Ok(HandlerAction::Stay {
                    max_times: 30,
                    suggested_delay: Some(std::time::Duration::from_secs(10)),
                })
            }
            Err(_) => {
                info!("Azure Network infrastructure deleted successfully");
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
        }
    }

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
}
