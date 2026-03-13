use alien_azure_clients::load_balancers::{AzureLoadBalancerClient, LoadBalancerApi};
use alien_azure_clients::long_running_operation::LongRunningOperationClient;
use alien_azure_clients::models::load_balancer::{
    BackendAddressPool, BackendAddressPoolPropertiesFormat, FrontendIpConfiguration,
    FrontendIpConfigurationPropertiesFormat, LoadBalancer, LoadBalancerPropertiesFormat,
    LoadBalancerSku, LoadBalancerSkuName, LoadBalancerSkuTier, LoadBalancingRule,
    LoadBalancingRulePropertiesFormat, Probe, ProbePropertiesFormat, ProbePropertiesFormatProtocol,
    PublicIpAddress as LbPublicIpAddress, SubResource, TransportProtocol,
};
use alien_azure_clients::models::public_ip_address::{
    IpAllocationMethod, PublicIpAddress, PublicIpAddressPropertiesFormat, PublicIpAddressSku,
    PublicIpAddressSkuName, PublicIpAddressSkuTier,
};
use alien_azure_clients::network::{AzureNetworkClient, NetworkApi};
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::ErrorData;
use anyhow::Result;
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
struct TrackedLoadBalancer {
    name: String,
}

#[derive(Debug, Clone)]
struct TrackedPublicIpAddress {
    name: String,
}

// -------------------------------------------------------------------------
// Test context
// -------------------------------------------------------------------------

struct LoadBalancerTestContext {
    client: AzureLoadBalancerClient,
    network_client: AzureNetworkClient,
    long_running_operation_client: LongRunningOperationClient,
    resource_group_name: String,
    subscription_id: String,
    location: String,
    created_load_balancers: Mutex<Vec<TrackedLoadBalancer>>,
    created_public_ip_addresses: Mutex<Vec<TrackedPublicIpAddress>>,
}

impl AsyncTestContext for LoadBalancerTestContext {
    async fn setup() -> LoadBalancerTestContext {
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

        let client = AzureLoadBalancerClient::new(Client::new(), client_config.clone());

        let network_client = AzureNetworkClient::new(Client::new(), client_config.clone());

        info!(
            "🔧 Using subscription: {} and resource group: {} for load balancer testing",
            subscription_id, resource_group_name
        );

        LoadBalancerTestContext {
            client,
            network_client,
            long_running_operation_client: LongRunningOperationClient::new(
                Client::new(),
                client_config,
            ),
            resource_group_name,
            subscription_id,
            location: "eastus".to_string(),
            created_load_balancers: Mutex::new(Vec::new()),
            created_public_ip_addresses: Mutex::new(Vec::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Load Balancer test cleanup...");

        // Cleanup load balancers first (they reference public IPs)
        let lbs_to_cleanup = {
            let lbs = self.created_load_balancers.lock().unwrap();
            lbs.clone()
        };

        for tracked_lb in lbs_to_cleanup {
            self.cleanup_load_balancer(&tracked_lb.name).await;
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

        info!("✅ Load Balancer test cleanup completed");
    }
}

impl LoadBalancerTestContext {
    // -------------------------------------------------------------------------
    // Resource tracking
    // -------------------------------------------------------------------------

    fn track_load_balancer(&self, name: &str) {
        let tracked = TrackedLoadBalancer {
            name: name.to_string(),
        };
        let mut lbs = self.created_load_balancers.lock().unwrap();
        lbs.push(tracked);
        info!("📝 Tracking load balancer for cleanup: {}", name);
    }

    fn untrack_load_balancer(&self, name: &str) {
        let mut lbs = self.created_load_balancers.lock().unwrap();
        lbs.retain(|l| l.name != name);
        info!(
            "✅ Load balancer {} successfully cleaned up and untracked",
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

    // -------------------------------------------------------------------------
    // Cleanup helpers
    // -------------------------------------------------------------------------

    async fn cleanup_load_balancer(&self, name: &str) {
        info!("🧹 Cleaning up load balancer: {}", name);

        match self
            .client
            .delete_load_balancer(&self.resource_group_name, name)
            .await
        {
            Ok(operation_result) => {
                if let Err(e) = operation_result
                    .wait_for_operation_completion(
                        &self.long_running_operation_client,
                        "DeleteLoadBalancer",
                        name,
                    )
                    .await
                {
                    warn!("Failed to wait for load balancer deletion: {:?}", e);
                }
                info!("✅ Load balancer {} deleted successfully", name);
            }
            Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                info!("🔍 Load balancer {} was already deleted", name);
            }
            Err(e) => {
                warn!(
                    "Failed to delete load balancer {} during cleanup: {:?}",
                    name, e
                );
            }
        }
    }

    async fn cleanup_public_ip_address(&self, name: &str) {
        info!("🧹 Cleaning up public IP address: {}", name);

        match self
            .network_client
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

    // -------------------------------------------------------------------------
    // Name generators
    // -------------------------------------------------------------------------

    fn generate_unique_lb_name(&self) -> String {
        format!(
            "alien-test-lb-{}",
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
            "alien-test-lb-pip-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        )
    }
}

// -------------------------------------------------------------------------
// Comprehensive lifecycle test
// -------------------------------------------------------------------------

/// This comprehensive test covers the full lifecycle of Azure Load Balancer resources:
/// 1. Create Public IP Address for the load balancer frontend
/// 2. Create Load Balancer with frontend IP, backend pool, health probe, and load balancing rule
/// 3. Verify the load balancer was created with all components
/// 4. Delete the load balancer
/// 5. Delete the public IP address
#[test_context(LoadBalancerTestContext)]
#[tokio::test]
async fn test_comprehensive_load_balancer_lifecycle(
    ctx: &mut LoadBalancerTestContext,
) -> Result<()> {
    info!("🏁 Starting comprehensive load balancer lifecycle test");

    // Generate unique names
    let public_ip_name = ctx.generate_unique_public_ip_name();
    let lb_name = ctx.generate_unique_lb_name();

    // -------------------------------------------------------------------------
    // Step 1: Create Public IP Address for frontend
    // -------------------------------------------------------------------------
    info!(
        "📦 Step 1/5: Creating public IP address: {}",
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
            ("TestType".to_string(), "LoadBalancerLifecycle".to_string()),
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
        .network_client
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

    // Get the public IP to obtain its resource ID
    let created_public_ip = ctx
        .network_client
        .get_public_ip_address(&ctx.resource_group_name, &public_ip_name)
        .await?;
    let public_ip_id = created_public_ip
        .id
        .clone()
        .expect("Public IP should have an ID");
    info!(
        "✅ Step 1/5: Public IP address created with ID: {}",
        public_ip_id
    );

    // -------------------------------------------------------------------------
    // Step 2: Create Load Balancer with all components
    // -------------------------------------------------------------------------
    info!("📦 Step 2/5: Creating load balancer: {}", lb_name);

    // Build resource IDs for referencing components
    let frontend_ip_config_name = "frontendIPConfig1";
    let backend_pool_name = "backendPool1";
    let probe_name = "healthProbe1";
    let lb_rule_name = "lbRule1";

    let frontend_ip_config_id = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/loadBalancers/{}/frontendIPConfigurations/{}",
        ctx.subscription_id, ctx.resource_group_name, lb_name, frontend_ip_config_name
    );
    let backend_pool_id = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/loadBalancers/{}/backendAddressPools/{}",
        ctx.subscription_id, ctx.resource_group_name, lb_name, backend_pool_name
    );
    let probe_id = format!(
        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/loadBalancers/{}/probes/{}",
        ctx.subscription_id, ctx.resource_group_name, lb_name, probe_name
    );

    let load_balancer = LoadBalancer {
        location: Some(ctx.location.clone()),
        sku: Some(LoadBalancerSku {
            name: Some(LoadBalancerSkuName::Standard),
            tier: Some(LoadBalancerSkuTier::Regional),
        }),
        properties: Some(LoadBalancerPropertiesFormat {
            frontend_ip_configurations: vec![FrontendIpConfiguration {
                name: Some(frontend_ip_config_name.to_string()),
                properties: Some(FrontendIpConfigurationPropertiesFormat {
                    public_ip_address: Some(LbPublicIpAddress {
                        id: Some(public_ip_id.clone()),
                        etag: None,
                        extended_location: None,
                        location: None,
                        name: None,
                        properties: None,
                        sku: None,
                        tags: Default::default(),
                        type_: None,
                        zones: vec![],
                    }),
                    private_ip_address: None,
                    private_ip_address_version: None,
                    private_ip_allocation_method: None,
                    subnet: None,
                    gateway_load_balancer: None,
                    inbound_nat_pools: vec![],
                    inbound_nat_rules: vec![],
                    load_balancing_rules: vec![],
                    outbound_rules: vec![],
                    provisioning_state: None,
                    public_ip_prefix: None,
                }),
                id: None,
                etag: None,
                type_: None,
                zones: vec![],
            }],
            backend_address_pools: vec![BackendAddressPool {
                name: Some(backend_pool_name.to_string()),
                properties: Some(BackendAddressPoolPropertiesFormat {
                    drain_period_in_seconds: None,
                    load_balancer_backend_addresses: vec![],
                    location: None,
                    sync_mode: None,
                    tunnel_interfaces: vec![],
                    virtual_network: None,
                    backend_ip_configurations: vec![],
                    inbound_nat_rules: vec![],
                    load_balancing_rules: vec![],
                    outbound_rule: None,
                    outbound_rules: vec![],
                    provisioning_state: None,
                }),
                id: None,
                etag: None,
                type_: None,
            }],
            probes: vec![Probe {
                name: Some(probe_name.to_string()),
                properties: Some(ProbePropertiesFormat {
                    protocol: ProbePropertiesFormatProtocol::Tcp,
                    port: 80,
                    interval_in_seconds: Some(15),
                    number_of_probes: Some(2),
                    probe_threshold: Some(1),
                    request_path: None,
                    load_balancing_rules: vec![],
                    no_healthy_backends_behavior: None,
                    provisioning_state: None,
                }),
                id: None,
                etag: None,
                type_: None,
            }],
            load_balancing_rules: vec![LoadBalancingRule {
                name: Some(lb_rule_name.to_string()),
                properties: Some(LoadBalancingRulePropertiesFormat {
                    frontend_ip_configuration: Some(SubResource {
                        id: Some(frontend_ip_config_id.clone()),
                    }),
                    backend_address_pool: Some(SubResource {
                        id: Some(backend_pool_id.clone()),
                    }),
                    backend_address_pools: vec![],
                    probe: Some(SubResource {
                        id: Some(probe_id.clone()),
                    }),
                    protocol: TransportProtocol::Tcp,
                    frontend_port: 80,
                    backend_port: Some(80),
                    load_distribution: None,
                    idle_timeout_in_minutes: Some(4),
                    enable_floating_ip: Some(false),
                    enable_tcp_reset: Some(true),
                    enable_connection_tracking: None,
                    disable_outbound_snat: Some(false),
                    provisioning_state: None,
                }),
                id: None,
                etag: None,
                type_: None,
            }],
            inbound_nat_pools: vec![],
            inbound_nat_rules: vec![],
            outbound_rules: vec![],
            provisioning_state: None,
            resource_guid: None,
            scope: None,
        }),
        tags: [
            ("Purpose".to_string(), "AlienIntegrationTest".to_string()),
            ("TestType".to_string(), "LoadBalancerLifecycle".to_string()),
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

    let lb_result = ctx
        .client
        .create_or_update_load_balancer(&ctx.resource_group_name, &lb_name, &load_balancer)
        .await?;

    ctx.track_load_balancer(&lb_name);

    lb_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "CreateLoadBalancer",
            &lb_name,
        )
        .await?;

    info!("✅ Step 2/5: Load balancer created successfully");

    // -------------------------------------------------------------------------
    // Step 3: Verify the load balancer
    // -------------------------------------------------------------------------
    info!("🔍 Step 3/5: Verifying load balancer: {}", lb_name);

    let verified_lb = ctx
        .client
        .get_load_balancer(&ctx.resource_group_name, &lb_name)
        .await?;

    assert!(verified_lb.id.is_some(), "Load balancer should have an ID");
    assert!(
        verified_lb.properties.is_some(),
        "Load balancer should have properties"
    );

    if let Some(props) = &verified_lb.properties {
        assert_eq!(
            props.frontend_ip_configurations.len(),
            1,
            "Load balancer should have 1 frontend IP config"
        );
        assert_eq!(
            props.backend_address_pools.len(),
            1,
            "Load balancer should have 1 backend pool"
        );
        assert_eq!(props.probes.len(), 1, "Load balancer should have 1 probe");
        assert_eq!(
            props.load_balancing_rules.len(),
            1,
            "Load balancer should have 1 load balancing rule"
        );
    }

    info!("✅ Step 3/5: Load balancer verified successfully");

    // -------------------------------------------------------------------------
    // Step 4: Delete the load balancer
    // -------------------------------------------------------------------------
    info!("🗑️  Step 4/5: Deleting load balancer: {}", lb_name);

    let delete_lb_result = ctx
        .client
        .delete_load_balancer(&ctx.resource_group_name, &lb_name)
        .await?;

    delete_lb_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeleteLoadBalancer",
            &lb_name,
        )
        .await?;

    // Verify load balancer was deleted
    let get_deleted_lb_result = ctx
        .client
        .get_load_balancer(&ctx.resource_group_name, &lb_name)
        .await;
    assert!(
        get_deleted_lb_result.is_err(),
        "Load balancer should be deleted"
    );

    ctx.untrack_load_balancer(&lb_name);
    info!("✅ Step 4/5: Load balancer deleted successfully");

    // -------------------------------------------------------------------------
    // Step 5: Delete the public IP address
    // -------------------------------------------------------------------------
    info!(
        "🗑️  Step 5/5: Deleting public IP address: {}",
        public_ip_name
    );

    let delete_pip_result = ctx
        .network_client
        .delete_public_ip_address(&ctx.resource_group_name, &public_ip_name)
        .await?;

    delete_pip_result
        .wait_for_operation_completion(
            &ctx.long_running_operation_client,
            "DeletePublicIpAddress",
            &public_ip_name,
        )
        .await?;

    // Verify public IP was deleted
    let get_deleted_pip_result = ctx
        .network_client
        .get_public_ip_address(&ctx.resource_group_name, &public_ip_name)
        .await;
    assert!(
        get_deleted_pip_result.is_err(),
        "Public IP should be deleted"
    );

    ctx.untrack_public_ip_address(&public_ip_name);
    info!("✅ Step 5/5: Public IP address deleted successfully");

    // -------------------------------------------------------------------------
    // Summary
    // -------------------------------------------------------------------------
    info!("🎉 Comprehensive load balancer lifecycle test completed successfully!");
    info!("   ✓ Created Public IP Address (Standard SKU, Static allocation)");
    info!("   ✓ Created Load Balancer with frontend IP, backend pool, health probe, and load balancing rule");
    info!("   ✓ Verified load balancer configuration");
    info!("   ✓ Deleted all resources");

    Ok(())
}

// -------------------------------------------------------------------------
// Not found test
// -------------------------------------------------------------------------

#[test_context(LoadBalancerTestContext)]
#[tokio::test]
async fn test_get_load_balancer_not_found(ctx: &mut LoadBalancerTestContext) {
    let non_existent_lb = "alien-test-non-existent-lb";

    let result = ctx
        .client
        .get_load_balancer(&ctx.resource_group_name, non_existent_lb)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        err if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            let Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) = &err.error else {
                unreachable!()
            };
            assert_eq!(resource_name, non_existent_lb);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}
