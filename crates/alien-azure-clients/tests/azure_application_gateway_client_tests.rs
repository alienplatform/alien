use alien_azure_clients::application_gateways::{
    ApplicationGatewayApi, AzureApplicationGatewayClient,
};
use alien_azure_clients::long_running_operation::{
    LongRunningOperationApi, LongRunningOperationClient, OperationResult,
};
use alien_azure_clients::models::public_ip_address::{
    IpAllocationMethod, PublicIpAddress, PublicIpAddressPropertiesFormat, PublicIpAddressSku,
    PublicIpAddressSkuName, PublicIpAddressSkuTier,
};
use alien_azure_clients::models::virtual_network::{
    AddressSpace, Subnet, SubnetPropertiesFormat, VirtualNetwork, VirtualNetworkPropertiesFormat,
};
use alien_azure_clients::network::{AzureNetworkClient, NetworkApi};
use alien_azure_clients::{AzureClientConfig, AzureCredentials, AzureTokenCache};
use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};
use std::{env, path::PathBuf};
use test_context::{test_context, AsyncTestContext};
use tracing::info;
use uuid::Uuid;

struct ApplicationGatewayTestContext {
    gateway: AzureApplicationGatewayClient,
    network: AzureNetworkClient,
    operations: LongRunningOperationClient,
    resource_group: String,
    subscription_id: String,
    gateway_name: Option<String>,
    public_ip_name: Option<String>,
    vnet_name: Option<String>,
}

impl AsyncTestContext for ApplicationGatewayTestContext {
    async fn setup() -> Self {
        let root: PathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();
        let config = AzureClientConfig {
            subscription_id: env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID").unwrap(),
            tenant_id: env::var("AZURE_MANAGEMENT_TENANT_ID").unwrap(),
            region: Some("eastus".into()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: env::var("AZURE_MANAGEMENT_CLIENT_ID").unwrap(),
                client_secret: env::var("AZURE_MANAGEMENT_CLIENT_SECRET").unwrap(),
            },
            service_overrides: None,
        };
        let token = || AzureTokenCache::new(config.clone());
        Self {
            gateway: AzureApplicationGatewayClient::new(Client::new(), token()),
            network: AzureNetworkClient::new(Client::new(), token()),
            operations: LongRunningOperationClient::new(Client::new(), token()),
            resource_group: env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP").unwrap(),
            subscription_id: config.subscription_id,
            gateway_name: None,
            public_ip_name: None,
            vnet_name: None,
        }
    }

    async fn teardown(self) {
        info!("Cleaning up Application Gateway integration test resources");
        if let Some(name) = &self.gateway_name {
            if let Ok(result) = self
                .gateway
                .delete_application_gateway(&self.resource_group, name)
                .await
            {
                let _ = result
                    .wait_for_operation_completion(
                        &self.operations,
                        "DeleteApplicationGateway",
                        name,
                    )
                    .await;
            }
        }
        if let Some(name) = &self.public_ip_name {
            if let Ok(result) = self
                .network
                .delete_public_ip_address(&self.resource_group, name)
                .await
            {
                let _ = result
                    .wait_for_operation_completion(&self.operations, "DeletePublicIpAddress", name)
                    .await;
            }
        }
        if let Some(name) = &self.vnet_name {
            if let Ok(result) = self
                .network
                .delete_virtual_network(&self.resource_group, name)
                .await
            {
                let _ = result
                    .wait_for_operation_completion(&self.operations, "DeleteVirtualNetwork", name)
                    .await;
            }
        }
    }
}

async fn operation_value(
    result: OperationResult<Value>,
    operations: &LongRunningOperationClient,
    operation_name: &str,
    resource_name: &str,
) -> Result<Value> {
    Ok(match result {
        OperationResult::Completed(value) => value,
        OperationResult::LongRunning(operation) => {
            let body = operations
                .wait_for_completion(&operation, operation_name, resource_name)
                .await?;
            if operation.location_url.is_some() {
                operations
                    .fetch_location_result(&operation, operation_name, resource_name)
                    .await?
            } else {
                serde_json::from_str(&body)?
            }
        }
    })
}

#[test_context(ApplicationGatewayTestContext)]
#[tokio::test]
async fn application_gateway_reports_a_real_healthy_backend(
    ctx: &mut ApplicationGatewayTestContext,
) -> Result<()> {
    let suffix = Uuid::new_v4().simple().to_string()[..8].to_string();
    let vnet_name = format!("alien-test-appgw-vnet-{suffix}");
    let subnet_name = format!("alien-test-appgw-subnet-{suffix}");
    let public_ip_name = format!("alien-test-appgw-pip-{suffix}");
    let gateway_name = format!("alien-test-appgw-{suffix}");
    ctx.vnet_name = Some(vnet_name.clone());

    let vnet = VirtualNetwork {
        location: Some("eastus".into()),
        properties: Some(VirtualNetworkPropertiesFormat {
            address_space: Some(AddressSpace {
                address_prefixes: vec!["10.91.0.0/16".into()],
                ipam_pool_prefix_allocations: vec![],
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    ctx.network
        .create_or_update_virtual_network(&ctx.resource_group, &vnet_name, &vnet)
        .await?
        .wait_for_operation_completion(&ctx.operations, "CreateVirtualNetwork", &vnet_name)
        .await?;
    let subnet = Subnet {
        name: Some(subnet_name.clone()),
        properties: Some(SubnetPropertiesFormat {
            address_prefix: Some("10.91.1.0/24".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    ctx.network
        .create_or_update_subnet(&ctx.resource_group, &vnet_name, &subnet_name, &subnet)
        .await?
        .wait_for_operation_completion(&ctx.operations, "CreateSubnet", &subnet_name)
        .await?;
    let subnet_id = ctx
        .network
        .get_subnet(&ctx.resource_group, &vnet_name, &subnet_name)
        .await?
        .id
        .unwrap();

    let public_ip = PublicIpAddress {
        location: Some("eastus".into()),
        sku: Some(PublicIpAddressSku {
            name: Some(PublicIpAddressSkuName::Standard),
            tier: Some(PublicIpAddressSkuTier::Regional),
        }),
        properties: Some(PublicIpAddressPropertiesFormat {
            public_ip_allocation_method: Some(IpAllocationMethod::Static),
            ..Default::default()
        }),
        ..Default::default()
    };
    ctx.public_ip_name = Some(public_ip_name.clone());
    ctx.network
        .create_or_update_public_ip_address(&ctx.resource_group, &public_ip_name, &public_ip)
        .await?
        .wait_for_operation_completion(&ctx.operations, "CreatePublicIpAddress", &public_ip_name)
        .await?;
    let public_ip_id = ctx
        .network
        .get_public_ip_address(&ctx.resource_group, &public_ip_name)
        .await?
        .id
        .unwrap();

    let base = format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/applicationGateways/{gateway_name}", ctx.subscription_id, ctx.resource_group);
    let id = |kind: &str, name: &str| format!("{base}/{kind}/{name}");
    let gateway = json!({
        "location":"eastus",
        "properties":{
            "sku":{"name":"Standard_v2","tier":"Standard_v2","capacity":1},
            "gatewayIPConfigurations":[{"name":"gateway","id":id("gatewayIPConfigurations","gateway"),"properties":{"subnet":{"id":subnet_id}}}],
            "frontendIPConfigurations":[{"name":"frontend","id":id("frontendIPConfigurations","frontend"),"properties":{"publicIPAddress":{"id":public_ip_id}}}],
            "frontendPorts":[{"name":"http","id":id("frontendPorts","http"),"properties":{"port":80}}],
            "backendAddressPools":[{"name":"backend","id":id("backendAddressPools","backend"),"properties":{"backendAddresses":[{"fqdn":"example.com"}]}}],
            "probes":[{"name":"probe","id":id("probes","probe"),"properties":{"protocol":"Http","path":"/","interval":10,"timeout":10,"unhealthyThreshold":2,"pickHostNameFromBackendHttpSettings":true}}],
            "backendHttpSettingsCollection":[{"name":"settings","id":id("backendHttpSettingsCollection","settings"),"properties":{"port":80,"protocol":"Http","cookieBasedAffinity":"Disabled","requestTimeout":20,"pickHostNameFromBackendAddress":true,"probe":{"id":id("probes","probe")}}}],
            "httpListeners":[{"name":"listener","id":id("httpListeners","listener"),"properties":{"frontendIPConfiguration":{"id":id("frontendIPConfigurations","frontend")},"frontendPort":{"id":id("frontendPorts","http")},"protocol":"Http"}}],
            "requestRoutingRules":[{"name":"rule","id":id("requestRoutingRules","rule"),"properties":{"ruleType":"Basic","priority":100,"httpListener":{"id":id("httpListeners","listener")},"backendAddressPool":{"id":id("backendAddressPools","backend")},"backendHttpSettings":{"id":id("backendHttpSettingsCollection","settings")}}}]
        }
    });
    ctx.gateway_name = Some(gateway_name.clone());
    ctx.gateway
        .create_or_update_application_gateway(&ctx.resource_group, &gateway_name, &gateway)
        .await?
        .wait_for_operation_completion(&ctx.operations, "CreateApplicationGateway", &gateway_name)
        .await?;

    for _ in 0..30 {
        let health = operation_value(
            ctx.gateway
                .get_application_gateway_backend_health(&ctx.resource_group, &gateway_name)
                .await?,
            &ctx.operations,
            "GetApplicationGatewayBackendHealth",
            &gateway_name,
        )
        .await?;
        let output = health.pointer("/properties/output").unwrap_or(&health);
        let healthy = output
            .get("backendAddressPools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|pool| {
                pool.pointer("/backendAddressPool/id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id.ends_with("/backendAddressPools/backend"))
            })
            .flat_map(|pool| {
                pool.get("backendHttpSettingsCollection")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .flat_map(|settings| {
                settings
                    .get("servers")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .any(|server| server.get("health").and_then(Value::as_str) == Some("Healthy"));
        if healthy {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
    anyhow::bail!("Application Gateway never reported the reachable backend as Healthy")
}
