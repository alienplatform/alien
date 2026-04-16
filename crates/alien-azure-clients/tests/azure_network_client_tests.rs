use alien_azure_clients::long_running_operation::LongRunningOperationClient;
use alien_azure_clients::models::nat_gateway::{
    NatGateway, NatGatewayPropertiesFormat, NatGatewaySku, NatGatewaySkuName,
    SubResource as NatSubResource,
};
use alien_azure_clients::models::network_security_group::{
    NetworkSecurityGroup, NetworkSecurityGroupPropertiesFormat, SecurityRule, SecurityRuleAccess,
    SecurityRuleDirection, SecurityRulePropertiesFormat, SecurityRulePropertiesFormatProtocol,
};
use alien_azure_clients::models::public_ip_address::{
    IpAllocationMethod, PublicIpAddress, PublicIpAddressPropertiesFormat, PublicIpAddressSku,
    PublicIpAddressSkuName, PublicIpAddressSkuTier,
};
use alien_azure_clients::models::virtual_network::{
    AddressSpace,
    // Use virtual_network's NetworkSecurityGroup for subnet NSG reference
    NetworkSecurityGroup as VnetNetworkSecurityGroup,
    ProvisioningState,
    // Use virtual_network's SubResource for subnet references
    SubResource as VnetSubResource,
    Subnet,
    SubnetPropertiesFormat,
    VirtualNetwork,
    VirtualNetworkPropertiesFormat,
};
use alien_azure_clients::network::{AzureNetworkClient, NetworkApi};
use alien_azure_clients::AzureTokenCache;
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::ErrorData;
use anyhow::{bail, Result};
use reqwest::Client;
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

// -------------------------------------------------------------------------
// Tracked resources for cleanup
// -------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TrackedVirtualNetwork {
    name: String,
}

#[derive(Debug, Clone)]
struct TrackedSubnet {
    vnet_name: String,
    subnet_name: String,
}

#[derive(Debug, Clone)]
struct TrackedNatGateway {
    name: String,
}

#[derive(Debug, Clone)]
struct TrackedPublicIpAddress {
    name: String,
}

#[derive(Debug, Clone)]
struct TrackedNetworkSecurityGroup {
    name: String,
}

// -------------------------------------------------------------------------
// Test context
// -------------------------------------------------------------------------

struct NetworkTestContext {
    client: AzureNetworkClient,
    long_running_operation_client: LongRunningOperationClient,
    resource_group_name: String,
    location: String,
    created_virtual_networks: Mutex<Vec<TrackedVirtualNetwork>>,
    created_subnets: Mutex<Vec<TrackedSubnet>>,
    created_nat_gateways: Mutex<Vec<TrackedNatGateway>>,
    created_public_ip_addresses: Mutex<Vec<TrackedPublicIpAddress>>,
    created_network_security_groups: Mutex<Vec<TrackedNetworkSecurityGroup>>,
}

impl AsyncTestContext for NetworkTestContext {
    async fn setup() -> NetworkTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID not set");
        let tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let client_id =
            env::var("AZURE_MANAGEMENT_CLIENT_ID").expect("AZURE_MANAGEMENT_CLIENT_ID not set");
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET not set");
        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP not set");

        let client_config = AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id,
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            },
            service_overrides: None,
        };

        let client =
            AzureNetworkClient::new(Client::new(), AzureTokenCache::new(client_config.clone()));

        info!(
            "🔧 Using subscription: {} and resource group: {} for network testing",
            subscription_id, resource_group_name
        );

        NetworkTestContext {
            client,
            long_running_operation_client: LongRunningOperationClient::new(
                Client::new(),
                AzureTokenCache::new(client_config),
            ),
            resource_group_name,
            location: "eastus".to_string(),
            created_virtual_networks: Mutex::new(Vec::new()),
            created_subnets: Mutex::new(Vec::new()),
            created_nat_gateways: Mutex::new(Vec::new()),
            created_public_ip_addresses: Mutex::new(Vec::new()),
            created_network_security_groups: Mutex::new(Vec::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Network test cleanup...");

        // Cleanup order matters due to dependencies:
        // 1. Subnets first (they reference VNets, NAT Gateways, NSGs)
        // 2. NAT Gateways (they reference Public IPs)
        // 3. Virtual Networks
        // 4. Public IP Addresses
        // 5. Network Security Groups

        // Cleanup subnets first
        let subnets_to_cleanup = {
            let subnets = self.created_subnets.lock().unwrap();
            subnets.clone()
        };

        for tracked_subnet in subnets_to_cleanup {
            self.cleanup_subnet(&tracked_subnet.vnet_name, &tracked_subnet.subnet_name)
                .await;
        }

        // Cleanup NAT gateways
        let nat_gateways_to_cleanup = {
            let nat_gateways = self.created_nat_gateways.lock().unwrap();
            nat_gateways.clone()
        };

        for tracked_nat_gateway in nat_gateways_to_cleanup {
            self.cleanup_nat_gateway(&tracked_nat_gateway.name).await;
        }

        // Cleanup virtual networks
        let vnets_to_cleanup = {
            let vnets = self.created_virtual_networks.lock().unwrap();
            vnets.clone()
        };

        for tracked_vnet in vnets_to_cleanup {
            self.cleanup_virtual_network(&tracked_vnet.name).await;
        }

        // Cleanup public IP addresses
        let public_ips_to_cleanup = {
            let public_ips = self.created_public_ip_addresses.lock().unwrap();
            public_ips.clone()
        };

        for tracked_public_ip in public_ips_to_cleanup {
            self.cleanup_public_ip_address(&tracked_public_ip.name)
                .await;
        }

        // Cleanup network security groups
        let nsgs_to_cleanup = {
            let nsgs = self.created_network_security_groups.lock().unwrap();
            nsgs.clone()
        };

        for tracked_nsg in nsgs_to_cleanup {
            self.cleanup_network_security_group(&tracked_nsg.name).await;
        }

        info!("✅ Network test cleanup completed");
    }
}

impl NetworkTestContext {
    // -------------------------------------------------------------------------
    // Resource tracking
    // -------------------------------------------------------------------------

    fn track_virtual_network(&self, name: &str) {
        let tracked = TrackedVirtualNetwork {
            name: name.to_string(),
        };
        let mut vnets = self.created_virtual_networks.lock().unwrap();
        vnets.push(tracked);
        info!("📝 Tracking virtual network for cleanup: {}", name);
    }

    fn untrack_virtual_network(&self, name: &str) {
        let mut vnets = self.created_virtual_networks.lock().unwrap();
        vnets.retain(|v| v.name != name);
        info!(
            "✅ Virtual network {} successfully cleaned up and untracked",
            name
        );
    }

    fn track_subnet(&self, vnet_name: &str, subnet_name: &str) {
        let tracked = TrackedSubnet {
            vnet_name: vnet_name.to_string(),
            subnet_name: subnet_name.to_string(),
        };
        let mut subnets = self.created_subnets.lock().unwrap();
        subnets.push(tracked);
        info!(
            "📝 Tracking subnet for cleanup: {}/{}",
            vnet_name, subnet_name
        );
    }

    fn untrack_subnet(&self, vnet_name: &str, subnet_name: &str) {
        let mut subnets = self.created_subnets.lock().unwrap();
        subnets.retain(|s| !(s.vnet_name == vnet_name && s.subnet_name == subnet_name));
        info!(
            "✅ Subnet {}/{} successfully cleaned up and untracked",
            vnet_name, subnet_name
        );
    }

    fn track_nat_gateway(&self, name: &str) {
        let tracked = TrackedNatGateway {
            name: name.to_string(),
        };
        let mut nat_gateways = self.created_nat_gateways.lock().unwrap();
        nat_gateways.push(tracked);
        info!("📝 Tracking NAT gateway for cleanup: {}", name);
    }

    fn untrack_nat_gateway(&self, name: &str) {
        let mut nat_gateways = self.created_nat_gateways.lock().unwrap();
        nat_gateways.retain(|n| n.name != name);
        info!(
            "✅ NAT gateway {} successfully cleaned up and untracked",
            name
        );
    }

    fn track_public_ip_address(&self, name: &str) {
        let tracked = TrackedPublicIpAddress {
            name: name.to_string(),
        };
        let mut public_ips = self.created_public_ip_addresses.lock().unwrap();
        public_ips.push(tracked);
        info!("📝 Tracking public IP address for cleanup: {}", name);
    }

    fn untrack_public_ip_address(&self, name: &str) {
        let mut public_ips = self.created_public_ip_addresses.lock().unwrap();
        public_ips.retain(|p| p.name != name);
        info!(
            "✅ Public IP address {} successfully cleaned up and untracked",
            name
        );
    }

    fn track_network_security_group(&self, name: &str) {
        let tracked = TrackedNetworkSecurityGroup {
            name: name.to_string(),
        };
        let mut nsgs = self.created_network_security_groups.lock().unwrap();
        nsgs.push(tracked);
        info!("📝 Tracking network security group for cleanup: {}", name);
    }

    fn untrack_network_security_group(&self, name: &str) {
        let mut nsgs = self.created_network_security_groups.lock().unwrap();
        nsgs.retain(|n| n.name != name);
        info!(
            "✅ Network security group {} successfully cleaned up and untracked",
            name
        );
    }

    // -------------------------------------------------------------------------
    // Cleanup helpers
    // -------------------------------------------------------------------------

    async fn cleanup_virtual_network(&self, name: &str) {
        info!("🧹 Cleaning up virtual network: {}", name);

        match self
            .client
            .delete_virtual_network(&self.resource_group_name, name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeleteVirtualNetwork",
                        name,
                    )
                    .await
                {
                    warn!("Failed to wait for virtual network deletion: {:?}", e);
                }
                info!("✅ Virtual network {} deleted successfully", name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Virtual network {} was already deleted", name);
            }
            Err(e) => {
                warn!(
                    "Failed to delete virtual network {} during cleanup: {:?}",
                    name, e
                );
            }
        }
    }

    async fn cleanup_subnet(&self, vnet_name: &str, subnet_name: &str) {
        info!("🧹 Cleaning up subnet: {}/{}", vnet_name, subnet_name);

        match self
            .client
            .delete_subnet(&self.resource_group_name, vnet_name, subnet_name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeleteSubnet",
                        subnet_name,
                    )
                    .await
                {
                    warn!("Failed to wait for subnet deletion: {:?}", e);
                }
                info!(
                    "✅ Subnet {}/{} deleted successfully",
                    vnet_name, subnet_name
                );
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!(
                    "🔍 Subnet {}/{} was already deleted",
                    vnet_name, subnet_name
                );
            }
            Err(e) => {
                warn!(
                    "Failed to delete subnet {}/{} during cleanup: {:?}",
                    vnet_name, subnet_name, e
                );
            }
        }
    }

    async fn cleanup_nat_gateway(&self, name: &str) {
        info!("🧹 Cleaning up NAT gateway: {}", name);

        match self
            .client
            .delete_nat_gateway(&self.resource_group_name, name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeleteNatGateway",
                        name,
                    )
                    .await
                {
                    warn!("Failed to wait for NAT gateway deletion: {:?}", e);
                }
                info!("✅ NAT gateway {} deleted successfully", name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 NAT gateway {} was already deleted", name);
            }
            Err(e) => {
                warn!(
                    "Failed to delete NAT gateway {} during cleanup: {:?}",
                    name, e
                );
            }
        }
    }

    async fn cleanup_public_ip_address(&self, name: &str) {
        info!("🧹 Cleaning up public IP address: {}", name);

        match self
            .client
            .delete_public_ip_address(&self.resource_group_name, name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeletePublicIpAddress",
                        name,
                    )
                    .await
                {
                    warn!("Failed to wait for public IP address deletion: {:?}", e);
                }
                info!("✅ Public IP address {} deleted successfully", name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Public IP address {} was already deleted", name);
            }
            Err(e) => {
                warn!(
                    "Failed to delete public IP address {} during cleanup: {:?}",
                    name, e
                );
            }
        }
    }

    async fn cleanup_network_security_group(&self, name: &str) {
        info!("🧹 Cleaning up network security group: {}", name);

        match self
            .client
            .delete_network_security_group(&self.resource_group_name, name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeleteNetworkSecurityGroup",
                        name,
                    )
                    .await
                {
                    warn!(
                        "Failed to wait for network security group deletion: {:?}",
                        e
                    );
                }
                info!("✅ Network security group {} deleted successfully", name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Network security group {} was already deleted", name);
            }
            Err(e) => {
                warn!(
                    "Failed to delete network security group {} during cleanup: {:?}",
                    name, e
                );
            }
        }
    }

    // -------------------------------------------------------------------------
    // Name generators
    // -------------------------------------------------------------------------

    fn generate_unique_vnet_name(&self) -> String {
        format!(
            "alien-test-vnet-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    fn generate_unique_subnet_name(&self) -> String {
        format!(
            "alien-test-subnet-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    fn generate_unique_nat_gateway_name(&self) -> String {
        format!(
            "alien-test-nat-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    fn generate_unique_public_ip_name(&self) -> String {
        format!(
            "alien-test-pip-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    fn generate_unique_nsg_name(&self) -> String {
        format!(
            "alien-test-nsg-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }

    // -------------------------------------------------------------------------
    // Wait helpers
    // -------------------------------------------------------------------------

    #[allow(dead_code)]
    async fn wait_for_provisioning_state<F, Fut>(
        &self,
        resource_name: &str,
        resource_type: &str,
        get_state: F,
        max_attempts: u32,
    ) -> Result<()>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<Option<ProvisioningState>>>,
    {
        info!(
            "⏳ Waiting for {} {} to be ready...",
            resource_type, resource_name
        );
        let mut attempts = 0;

        loop {
            attempts += 1;

            match get_state().await {
                Ok(Some(state)) => {
                    info!(
                        "📊 {} provisioning state: {:?} (attempt {}/{})",
                        resource_type, state, attempts, max_attempts
                    );

                    if state == ProvisioningState::Succeeded {
                        info!("✅ {} {} is ready!", resource_type, resource_name);
                        return Ok(());
                    }

                    if state == ProvisioningState::Failed {
                        bail!("❌ {} {} provisioning failed", resource_type, resource_name);
                    }
                }
                Ok(None) => {
                    info!(
                        "📊 {} provisioning state: unknown (attempt {}/{})",
                        resource_type, attempts, max_attempts
                    );
                }
                Err(e) => {
                    bail!("Failed to get {} status: {:?}", resource_type, e);
                }
            }

            if attempts >= max_attempts {
                bail!(
                    "⚠️  {} {} didn't become ready within {} attempts",
                    resource_type,
                    resource_name,
                    max_attempts
                );
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    }
}

// -------------------------------------------------------------------------
// Comprehensive lifecycle test
// -------------------------------------------------------------------------

/// This comprehensive test covers the full lifecycle of Azure networking resources:
/// 1. Create Network Security Group (NSG)
/// 2. Create Public IP Address
/// 3. Create NAT Gateway with Public IP
/// 4. Create Virtual Network
/// 5. Create Subnet with NSG and NAT Gateway
/// 6. Verify all resources
/// 7. Delete Subnet
/// 8. Delete Virtual Network
/// 9. Delete NAT Gateway
/// 10. Delete Public IP Address
/// 11. Delete Network Security Group
#[test_context(NetworkTestContext)]
#[tokio::test]
async fn test_comprehensive_network_lifecycle(ctx: &mut NetworkTestContext) -> Result<()> {
    info!("🏁 Starting comprehensive network lifecycle test");

    // Generate unique names for all resources
    let nsg_name = ctx.generate_unique_nsg_name();
    let public_ip_name = ctx.generate_unique_public_ip_name();
    let nat_gateway_name = ctx.generate_unique_nat_gateway_name();
    let vnet_name = ctx.generate_unique_vnet_name();
    let subnet_name = ctx.generate_unique_subnet_name();

    // -------------------------------------------------------------------------
    // Step 1: Create Network Security Group
    // -------------------------------------------------------------------------
    info!(
        "📦 Step 1/11: Creating network security group: {}",
        nsg_name
    );

    let nsg = NetworkSecurityGroup {
        location: Some(ctx.location.clone()),
        properties: Some(NetworkSecurityGroupPropertiesFormat {
            security_rules: vec![SecurityRule {
                name: Some("AllowHTTPS".to_string()),
                properties: Some(SecurityRulePropertiesFormat {
                    protocol: SecurityRulePropertiesFormatProtocol::Tcp,
                    source_port_range: Some("*".to_string()),
                    destination_port_range: Some("443".to_string()),
                    source_address_prefix: Some("*".to_string()),
                    destination_address_prefix: Some("*".to_string()),
                    access: SecurityRuleAccess::Allow,
                    priority: 100,
                    direction: SecurityRuleDirection::Inbound,
                    description: Some("Allow HTTPS inbound traffic".to_string()),
                    // Explicitly set remaining fields
                    source_port_ranges: vec![],
                    destination_port_ranges: vec![],
                    source_address_prefixes: vec![],
                    destination_address_prefixes: vec![],
                    source_application_security_groups: vec![],
                    destination_application_security_groups: vec![],
                    provisioning_state: None,
                }),
                id: None,
                etag: None,
                type_: None,
            }],
            default_security_rules: vec![],
            network_interfaces: vec![],
            subnets: vec![],
            flow_logs: vec![],
            flush_connection: None,
            provisioning_state: None,
            resource_guid: None,
        }),
        tags: [
            ("Purpose".to_string(), "AlienIntegrationTest".to_string()),
            ("TestType".to_string(), "NetworkLifecycle".to_string()),
        ]
        .iter()
        .cloned()
        .collect(),
        id: None,
        name: None,
        type_: None,
        etag: None,
    };

    let nsg_result = ctx
        .client
        .create_or_update_network_security_group(&ctx.resource_group_name, &nsg_name, &nsg)
        .await?;

    ctx.track_network_security_group(&nsg_name);

    nsg_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateNetworkSecurityGroup",
            &nsg_name,
        )
        .await?;

    // Verify NSG was created
    let created_nsg = ctx
        .client
        .get_network_security_group(&ctx.resource_group_name, &nsg_name)
        .await?;
    assert!(created_nsg.id.is_some(), "Created NSG should have an ID");
    info!("✅ Step 1/11: Network security group created successfully");

    // -------------------------------------------------------------------------
    // Step 2: Create Public IP Address
    // -------------------------------------------------------------------------
    info!(
        "📦 Step 2/11: Creating public IP address: {}",
        public_ip_name
    );

    let public_ip = PublicIpAddress {
        location: Some(ctx.location.clone()),
        sku: Some(PublicIpAddressSku {
            name: Some(PublicIpAddressSkuName::Standard),
            tier: Some(PublicIpAddressSkuTier::Regional),
        }),
        properties: Some(Box::new(PublicIpAddressPropertiesFormat {
            public_ip_allocation_method: Some(IpAllocationMethod::Static),
            idle_timeout_in_minutes: Some(4),
            ddos_settings: None,
            delete_option: None,
            dns_settings: None,
            ip_address: None,
            ip_configuration: None,
            ip_tags: vec![],
            linked_public_ip_address: None,
            migration_phase: None,
            nat_gateway: None,
            provisioning_state: None,
            public_ip_address_version: None,
            public_ip_prefix: None,
            resource_guid: None,
            service_public_ip_address: None,
        })),
        tags: [
            ("Purpose".to_string(), "AlienIntegrationTest".to_string()),
            ("TestType".to_string(), "NetworkLifecycle".to_string()),
        ]
        .iter()
        .cloned()
        .collect(),
        id: None,
        name: None,
        type_: None,
        etag: None,
        extended_location: None,
        zones: vec![],
    };

    let public_ip_result = ctx
        .client
        .create_or_update_public_ip_address(&ctx.resource_group_name, &public_ip_name, &public_ip)
        .await?;

    ctx.track_public_ip_address(&public_ip_name);

    public_ip_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreatePublicIpAddress",
            &public_ip_name,
        )
        .await?;

    // Verify Public IP was created and get its resource ID
    let created_public_ip = ctx
        .client
        .get_public_ip_address(&ctx.resource_group_name, &public_ip_name)
        .await?;
    let public_ip_id = created_public_ip
        .id
        .clone()
        .expect("Public IP should have an ID");
    info!(
        "✅ Step 2/11: Public IP address created successfully with ID: {}",
        public_ip_id
    );

    // -------------------------------------------------------------------------
    // Step 3: Create NAT Gateway with Public IP
    // -------------------------------------------------------------------------
    info!("📦 Step 3/11: Creating NAT gateway: {}", nat_gateway_name);

    let nat_gateway = NatGateway {
        location: Some(ctx.location.clone()),
        sku: Some(NatGatewaySku {
            name: Some(NatGatewaySkuName::Standard),
        }),
        properties: Some(NatGatewayPropertiesFormat {
            idle_timeout_in_minutes: Some(10),
            public_ip_addresses: vec![NatSubResource {
                id: Some(public_ip_id.clone()),
            }],
            public_ip_addresses_v6: vec![],
            public_ip_prefixes: vec![],
            public_ip_prefixes_v6: vec![],
            subnets: vec![],
            provisioning_state: None,
            resource_guid: None,
            source_virtual_network: None,
        }),
        tags: [
            ("Purpose".to_string(), "AlienIntegrationTest".to_string()),
            ("TestType".to_string(), "NetworkLifecycle".to_string()),
        ]
        .iter()
        .cloned()
        .collect(),
        id: None,
        name: None,
        type_: None,
        etag: None,
        zones: vec![],
    };

    let nat_gateway_result = ctx
        .client
        .create_or_update_nat_gateway(&ctx.resource_group_name, &nat_gateway_name, &nat_gateway)
        .await?;

    ctx.track_nat_gateway(&nat_gateway_name);

    nat_gateway_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateNatGateway",
            &nat_gateway_name,
        )
        .await?;

    // Verify NAT Gateway was created and get its resource ID
    let created_nat_gateway = ctx
        .client
        .get_nat_gateway(&ctx.resource_group_name, &nat_gateway_name)
        .await?;
    let nat_gateway_id = created_nat_gateway
        .id
        .clone()
        .expect("NAT Gateway should have an ID");
    info!(
        "✅ Step 3/11: NAT gateway created successfully with ID: {}",
        nat_gateway_id
    );

    // -------------------------------------------------------------------------
    // Step 4: Create Virtual Network
    // -------------------------------------------------------------------------
    info!("📦 Step 4/11: Creating virtual network: {}", vnet_name);

    let vnet = VirtualNetwork {
        location: Some(ctx.location.clone()),
        properties: Some(VirtualNetworkPropertiesFormat {
            address_space: Some(AddressSpace {
                address_prefixes: vec!["10.0.0.0/16".to_string()],
                ipam_pool_prefix_allocations: vec![],
            }),
            bgp_communities: None,
            ddos_protection_plan: None,
            default_public_nat_gateway: None,
            dhcp_options: None,
            enable_ddos_protection: false,
            enable_vm_protection: false,
            encryption: None,
            flow_logs: vec![],
            flow_timeout_in_minutes: None,
            ip_allocations: vec![],
            private_endpoint_v_net_policies: None,
            provisioning_state: None,
            resource_guid: None,
            subnets: vec![],
            virtual_network_peerings: vec![],
        }),
        tags: [
            ("Purpose".to_string(), "AlienIntegrationTest".to_string()),
            ("TestType".to_string(), "NetworkLifecycle".to_string()),
        ]
        .iter()
        .cloned()
        .collect(),
        id: None,
        name: None,
        type_: None,
        etag: None,
        extended_location: None,
    };

    let vnet_result = ctx
        .client
        .create_or_update_virtual_network(&ctx.resource_group_name, &vnet_name, &vnet)
        .await?;

    ctx.track_virtual_network(&vnet_name);

    vnet_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateVirtualNetwork",
            &vnet_name,
        )
        .await?;

    // Verify VNet was created
    let created_vnet = ctx
        .client
        .get_virtual_network(&ctx.resource_group_name, &vnet_name)
        .await?;
    assert!(created_vnet.id.is_some(), "Created VNet should have an ID");
    info!("✅ Step 4/11: Virtual network created successfully");

    // -------------------------------------------------------------------------
    // Step 5: Create Subnet with NSG and NAT Gateway
    // -------------------------------------------------------------------------
    info!(
        "📦 Step 5/11: Creating subnet: {} in VNet: {}",
        subnet_name, vnet_name
    );

    let nsg_id = created_nsg.id.clone().expect("NSG should have an ID");

    let subnet = Subnet {
        name: Some(subnet_name.clone()),
        properties: Some(SubnetPropertiesFormat {
            address_prefix: Some("10.0.1.0/24".to_string()),
            nat_gateway: Some(VnetSubResource {
                id: Some(nat_gateway_id.clone()),
            }),
            network_security_group: Some(VnetNetworkSecurityGroup {
                id: Some(nsg_id.clone()),
                location: None,
                tags: Default::default(),
                etag: None,
                name: None,
                type_: None,
                properties: None,
            }),
            address_prefixes: vec![],
            application_gateway_ip_configurations: vec![],
            default_outbound_access: None,
            delegations: vec![],
            ip_allocations: vec![],
            ip_configuration_profiles: vec![],
            ip_configurations: vec![],
            ipam_pool_prefix_allocations: vec![],
            private_endpoint_network_policies: Default::default(),
            private_endpoints: vec![],
            private_link_service_network_policies: Default::default(),
            provisioning_state: None,
            purpose: None,
            resource_navigation_links: vec![],
            route_table: None,
            service_association_links: vec![],
            service_endpoint_policies: vec![],
            service_endpoints: vec![],
            sharing_scope: None,
        }),
        id: None,
        etag: None,
        type_: None,
    };

    let subnet_result = ctx
        .client
        .create_or_update_subnet(&ctx.resource_group_name, &vnet_name, &subnet_name, &subnet)
        .await?;

    ctx.track_subnet(&vnet_name, &subnet_name);

    subnet_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateSubnet",
            &subnet_name,
        )
        .await?;

    // Verify Subnet was created
    let created_subnet = ctx
        .client
        .get_subnet(&ctx.resource_group_name, &vnet_name, &subnet_name)
        .await?;
    assert!(
        created_subnet.id.is_some(),
        "Created Subnet should have an ID"
    );
    info!("✅ Step 5/11: Subnet created successfully");

    // -------------------------------------------------------------------------
    // Step 6: Verify all resources
    // -------------------------------------------------------------------------
    info!("🔍 Step 6/11: Verifying all resources");

    // Verify NSG
    let verified_nsg = ctx
        .client
        .get_network_security_group(&ctx.resource_group_name, &nsg_name)
        .await?;
    assert!(verified_nsg.properties.is_some());
    if let Some(props) = &verified_nsg.properties {
        assert!(
            !props.security_rules.is_empty(),
            "NSG should have security rules"
        );
    }

    // Verify Public IP
    let verified_public_ip = ctx
        .client
        .get_public_ip_address(&ctx.resource_group_name, &public_ip_name)
        .await?;
    assert!(verified_public_ip.properties.is_some());

    // Verify NAT Gateway
    let verified_nat_gateway = ctx
        .client
        .get_nat_gateway(&ctx.resource_group_name, &nat_gateway_name)
        .await?;
    assert!(verified_nat_gateway.properties.is_some());
    if let Some(props) = &verified_nat_gateway.properties {
        assert!(
            !props.public_ip_addresses.is_empty(),
            "NAT Gateway should have public IP addresses"
        );
    }

    // Verify VNet
    let verified_vnet = ctx
        .client
        .get_virtual_network(&ctx.resource_group_name, &vnet_name)
        .await?;
    assert!(verified_vnet.properties.is_some());

    // Verify Subnet
    let verified_subnet = ctx
        .client
        .get_subnet(&ctx.resource_group_name, &vnet_name, &subnet_name)
        .await?;
    assert!(verified_subnet.properties.is_some());
    if let Some(props) = &verified_subnet.properties {
        assert!(
            props.nat_gateway.is_some(),
            "Subnet should have NAT gateway reference"
        );
        assert!(
            props.network_security_group.is_some(),
            "Subnet should have NSG reference"
        );
    }

    info!("✅ Step 6/11: All resources verified successfully");

    // -------------------------------------------------------------------------
    // Step 7: Delete Subnet
    // -------------------------------------------------------------------------
    info!("🗑️  Step 7/11: Deleting subnet: {}", subnet_name);

    let delete_subnet_result = ctx
        .client
        .delete_subnet(&ctx.resource_group_name, &vnet_name, &subnet_name)
        .await?;

    delete_subnet_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteSubnet",
            &subnet_name,
        )
        .await?;

    // Verify subnet was deleted
    let get_deleted_subnet_result = ctx
        .client
        .get_subnet(&ctx.resource_group_name, &vnet_name, &subnet_name)
        .await;
    assert!(
        get_deleted_subnet_result.is_err(),
        "Subnet should be deleted"
    );

    ctx.untrack_subnet(&vnet_name, &subnet_name);
    info!("✅ Step 7/11: Subnet deleted successfully");

    // -------------------------------------------------------------------------
    // Step 8: Delete Virtual Network
    // -------------------------------------------------------------------------
    info!("🗑️  Step 8/11: Deleting virtual network: {}", vnet_name);

    let delete_vnet_result = ctx
        .client
        .delete_virtual_network(&ctx.resource_group_name, &vnet_name)
        .await?;

    delete_vnet_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteVirtualNetwork",
            &vnet_name,
        )
        .await?;

    // Verify VNet was deleted
    let get_deleted_vnet_result = ctx
        .client
        .get_virtual_network(&ctx.resource_group_name, &vnet_name)
        .await;
    assert!(get_deleted_vnet_result.is_err(), "VNet should be deleted");

    ctx.untrack_virtual_network(&vnet_name);
    info!("✅ Step 8/11: Virtual network deleted successfully");

    // -------------------------------------------------------------------------
    // Step 9: Delete NAT Gateway
    // -------------------------------------------------------------------------
    info!("🗑️  Step 9/11: Deleting NAT gateway: {}", nat_gateway_name);

    let delete_nat_gateway_result = ctx
        .client
        .delete_nat_gateway(&ctx.resource_group_name, &nat_gateway_name)
        .await?;

    delete_nat_gateway_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteNatGateway",
            &nat_gateway_name,
        )
        .await?;

    // Verify NAT Gateway was deleted
    let get_deleted_nat_gateway_result = ctx
        .client
        .get_nat_gateway(&ctx.resource_group_name, &nat_gateway_name)
        .await;
    assert!(
        get_deleted_nat_gateway_result.is_err(),
        "NAT Gateway should be deleted"
    );

    ctx.untrack_nat_gateway(&nat_gateway_name);
    info!("✅ Step 9/11: NAT gateway deleted successfully");

    // -------------------------------------------------------------------------
    // Step 10: Delete Public IP Address
    // -------------------------------------------------------------------------
    info!(
        "🗑️  Step 10/11: Deleting public IP address: {}",
        public_ip_name
    );

    let delete_public_ip_result = ctx
        .client
        .delete_public_ip_address(&ctx.resource_group_name, &public_ip_name)
        .await?;

    delete_public_ip_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeletePublicIpAddress",
            &public_ip_name,
        )
        .await?;

    // Verify Public IP was deleted
    let get_deleted_public_ip_result = ctx
        .client
        .get_public_ip_address(&ctx.resource_group_name, &public_ip_name)
        .await;
    assert!(
        get_deleted_public_ip_result.is_err(),
        "Public IP should be deleted"
    );

    ctx.untrack_public_ip_address(&public_ip_name);
    info!("✅ Step 10/11: Public IP address deleted successfully");

    // -------------------------------------------------------------------------
    // Step 11: Delete Network Security Group
    // -------------------------------------------------------------------------
    info!(
        "🗑️  Step 11/11: Deleting network security group: {}",
        nsg_name
    );

    let delete_nsg_result = ctx
        .client
        .delete_network_security_group(&ctx.resource_group_name, &nsg_name)
        .await?;

    delete_nsg_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteNetworkSecurityGroup",
            &nsg_name,
        )
        .await?;

    // Verify NSG was deleted
    let get_deleted_nsg_result = ctx
        .client
        .get_network_security_group(&ctx.resource_group_name, &nsg_name)
        .await;
    assert!(get_deleted_nsg_result.is_err(), "NSG should be deleted");

    ctx.untrack_network_security_group(&nsg_name);
    info!("✅ Step 11/11: Network security group deleted successfully");

    // -------------------------------------------------------------------------
    // Summary
    // -------------------------------------------------------------------------
    info!("🎉 Comprehensive network lifecycle test completed successfully!");
    info!("   ✓ Created Network Security Group with security rules");
    info!("   ✓ Created Public IP Address (Standard SKU, Static allocation)");
    info!("   ✓ Created NAT Gateway with Public IP");
    info!("   ✓ Created Virtual Network with address space 10.0.0.0/16");
    info!("   ✓ Created Subnet with NSG and NAT Gateway associations");
    info!("   ✓ Verified all resources and their associations");
    info!("   ✓ Deleted all resources in correct dependency order");

    Ok(())
}

// -------------------------------------------------------------------------
// Individual resource tests (for faster feedback during development)
// -------------------------------------------------------------------------

#[test_context(NetworkTestContext)]
#[tokio::test]
async fn test_get_virtual_network_not_found(ctx: &mut NetworkTestContext) {
    let non_existent_vnet = "alien-test-non-existent-vnet";

    let result = ctx
        .client
        .get_virtual_network(&ctx.resource_group_name, non_existent_vnet)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            let Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) = &err.error else {
                unreachable!()
            };
            assert_eq!(resource_name, non_existent_vnet);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(NetworkTestContext)]
#[tokio::test]
async fn test_get_subnet_not_found(ctx: &mut NetworkTestContext) {
    let non_existent_vnet = "alien-test-non-existent-vnet";
    let non_existent_subnet = "alien-test-non-existent-subnet";

    let result = ctx
        .client
        .get_subnet(
            &ctx.resource_group_name,
            non_existent_vnet,
            non_existent_subnet,
        )
        .await;

    assert!(result.is_err());
    // The error could be either VNet not found or Subnet not found
    match result.unwrap_err() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            // Expected
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(NetworkTestContext)]
#[tokio::test]
async fn test_get_nat_gateway_not_found(ctx: &mut NetworkTestContext) {
    let non_existent_nat = "alien-test-non-existent-nat";

    let result = ctx
        .client
        .get_nat_gateway(&ctx.resource_group_name, non_existent_nat)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            let Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) = &err.error else {
                unreachable!()
            };
            assert_eq!(resource_name, non_existent_nat);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(NetworkTestContext)]
#[tokio::test]
async fn test_get_public_ip_address_not_found(ctx: &mut NetworkTestContext) {
    let non_existent_pip = "alien-test-non-existent-pip";

    let result = ctx
        .client
        .get_public_ip_address(&ctx.resource_group_name, non_existent_pip)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            let Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) = &err.error else {
                unreachable!()
            };
            assert_eq!(resource_name, non_existent_pip);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(NetworkTestContext)]
#[tokio::test]
async fn test_get_network_security_group_not_found(ctx: &mut NetworkTestContext) {
    let non_existent_nsg = "alien-test-non-existent-nsg";

    let result = ctx
        .client
        .get_network_security_group(&ctx.resource_group_name, non_existent_nsg)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            let Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) = &err.error else {
                unreachable!()
            };
            assert_eq!(resource_name, non_existent_nsg);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}
