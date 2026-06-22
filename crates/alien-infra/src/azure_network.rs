use crate::core::{
    map_azure_core_021_delete_lro_response, map_azure_core_021_lro_response,
    map_azure_core_021_sdk_error, OperationResult,
};
use crate::error::Result;
use alien_core::AzureClientConfig;
use azure_mgmt_network::package_2024_03 as azure_network_2024_03;
use azure_mgmt_network::package_2024_03::models::{
    NatGateway, NetworkSecurityGroup, PublicIpAddress, Subnet, VirtualNetwork,
};

pub(crate) async fn create_or_update_virtual_network(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    virtual_network_name: &str,
    virtual_network: &VirtualNetwork,
) -> Result<OperationResult<VirtualNetwork>> {
    let result = client
        .virtual_networks_client()
        .create_or_update(
            resource_group_name.to_string(),
            virtual_network_name.to_string(),
            virtual_network.clone(),
            config.subscription_id.clone(),
        )
        .send()
        .await;
    map_azure_core_021_lro_response(
        "Azure Network",
        result,
        "virtual network create or update",
        "Azure virtual network",
        virtual_network_name,
        |response| response.into_body(),
    )
    .await
}

pub(crate) async fn get_virtual_network(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    virtual_network_name: &str,
) -> Result<VirtualNetwork> {
    let result = client
        .virtual_networks_client()
        .get(
            resource_group_name.to_string(),
            virtual_network_name.to_string(),
            config.subscription_id.clone(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Network",
        result,
        "virtual network get",
        "Azure virtual network",
        virtual_network_name,
    )
}

pub(crate) async fn delete_virtual_network(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    virtual_network_name: &str,
) -> Result<OperationResult<()>> {
    let result = client
        .virtual_networks_client()
        .delete(
            resource_group_name.to_string(),
            virtual_network_name.to_string(),
            config.subscription_id.clone(),
        )
        .send()
        .await;
    map_azure_core_021_delete_lro_response(
        "Azure Network",
        result,
        "virtual network delete",
        "Azure virtual network",
        virtual_network_name,
    )
    .await
}

pub(crate) async fn create_or_update_subnet(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    virtual_network_name: &str,
    subnet_name: &str,
    subnet: &Subnet,
) -> Result<OperationResult<Subnet>> {
    let result = client
        .subnets_client()
        .create_or_update(
            resource_group_name.to_string(),
            virtual_network_name.to_string(),
            subnet_name.to_string(),
            subnet.clone(),
            config.subscription_id.clone(),
        )
        .send()
        .await;
    map_azure_core_021_lro_response(
        "Azure Network",
        result,
        "subnet create or update",
        "Azure subnet",
        subnet_name,
        |response| response.into_body(),
    )
    .await
}

pub(crate) async fn get_subnet(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    virtual_network_name: &str,
    subnet_name: &str,
) -> Result<Subnet> {
    let result = client
        .subnets_client()
        .get(
            resource_group_name.to_string(),
            virtual_network_name.to_string(),
            subnet_name.to_string(),
            config.subscription_id.clone(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Network",
        result,
        "subnet get",
        "Azure subnet",
        subnet_name,
    )
}

pub(crate) async fn create_or_update_nat_gateway(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    nat_gateway_name: &str,
    nat_gateway: &NatGateway,
) -> Result<OperationResult<NatGateway>> {
    let result = client
        .nat_gateways_client()
        .create_or_update(
            resource_group_name.to_string(),
            nat_gateway_name.to_string(),
            nat_gateway.clone(),
            config.subscription_id.clone(),
        )
        .send()
        .await;
    map_azure_core_021_lro_response(
        "Azure Network",
        result,
        "NAT gateway create or update",
        "Azure NAT gateway",
        nat_gateway_name,
        |response| response.into_body(),
    )
    .await
}

pub(crate) async fn get_nat_gateway(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    nat_gateway_name: &str,
) -> Result<NatGateway> {
    let result = client
        .nat_gateways_client()
        .get(
            resource_group_name.to_string(),
            nat_gateway_name.to_string(),
            config.subscription_id.clone(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Network",
        result,
        "NAT gateway get",
        "Azure NAT gateway",
        nat_gateway_name,
    )
}

pub(crate) async fn delete_nat_gateway(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    nat_gateway_name: &str,
) -> Result<OperationResult<()>> {
    let result = client
        .nat_gateways_client()
        .delete(
            resource_group_name.to_string(),
            nat_gateway_name.to_string(),
            config.subscription_id.clone(),
        )
        .send()
        .await;
    map_azure_core_021_delete_lro_response(
        "Azure Network",
        result,
        "NAT gateway delete",
        "Azure NAT gateway",
        nat_gateway_name,
    )
    .await
}

pub(crate) async fn create_or_update_public_ip_address(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    public_ip_address_name: &str,
    public_ip_address: &PublicIpAddress,
) -> Result<OperationResult<PublicIpAddress>> {
    let result = client
        .public_ip_addresses_client()
        .create_or_update(
            resource_group_name.to_string(),
            public_ip_address_name.to_string(),
            public_ip_address.clone(),
            config.subscription_id.clone(),
        )
        .send()
        .await;
    map_azure_core_021_lro_response(
        "Azure Network",
        result,
        "public IP address create or update",
        "Azure public IP address",
        public_ip_address_name,
        |response| response.into_body(),
    )
    .await
}

pub(crate) async fn get_public_ip_address(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    public_ip_address_name: &str,
) -> Result<PublicIpAddress> {
    let result = client
        .public_ip_addresses_client()
        .get(
            resource_group_name.to_string(),
            public_ip_address_name.to_string(),
            config.subscription_id.clone(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Network",
        result,
        "public IP address get",
        "Azure public IP address",
        public_ip_address_name,
    )
}

pub(crate) async fn delete_public_ip_address(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    public_ip_address_name: &str,
) -> Result<OperationResult<()>> {
    let result = client
        .public_ip_addresses_client()
        .delete(
            resource_group_name.to_string(),
            public_ip_address_name.to_string(),
            config.subscription_id.clone(),
        )
        .send()
        .await;
    map_azure_core_021_delete_lro_response(
        "Azure Network",
        result,
        "public IP address delete",
        "Azure public IP address",
        public_ip_address_name,
    )
    .await
}

pub(crate) async fn create_or_update_network_security_group(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    network_security_group_name: &str,
    network_security_group: &NetworkSecurityGroup,
) -> Result<OperationResult<NetworkSecurityGroup>> {
    let result = client
        .network_security_groups_client()
        .create_or_update(
            resource_group_name.to_string(),
            network_security_group_name.to_string(),
            network_security_group.clone(),
            config.subscription_id.clone(),
        )
        .send()
        .await;
    map_azure_core_021_lro_response(
        "Azure Network",
        result,
        "network security group create or update",
        "Azure network security group",
        network_security_group_name,
        |response| response.into_body(),
    )
    .await
}

pub(crate) async fn get_network_security_group(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    network_security_group_name: &str,
) -> Result<NetworkSecurityGroup> {
    let result = client
        .network_security_groups_client()
        .get(
            resource_group_name.to_string(),
            network_security_group_name.to_string(),
            config.subscription_id.clone(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Network",
        result,
        "network security group get",
        "Azure network security group",
        network_security_group_name,
    )
}

pub(crate) async fn delete_network_security_group(
    client: &azure_network_2024_03::Client,
    config: &AzureClientConfig,
    resource_group_name: &str,
    network_security_group_name: &str,
) -> Result<OperationResult<()>> {
    let result = client
        .network_security_groups_client()
        .delete(
            resource_group_name.to_string(),
            network_security_group_name.to_string(),
            config.subscription_id.clone(),
        )
        .send()
        .await;
    map_azure_core_021_delete_lro_response(
        "Azure Network",
        result,
        "network security group delete",
        "Azure network security group",
        network_security_group_name,
    )
    .await
}
