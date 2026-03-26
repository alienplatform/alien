use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::models::{
    nat_gateway::NatGateway,
    network_security_group::NetworkSecurityGroup,
    public_ip_address::PublicIpAddress,
    virtual_network::{Subnet, VirtualNetwork},
};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Result of a virtual network create or update operation
pub type VirtualNetworkOperationResult = OperationResult<VirtualNetwork>;

/// Result of a subnet create or update operation
pub type SubnetOperationResult = OperationResult<Subnet>;

/// Result of a NAT gateway create or update operation
pub type NatGatewayOperationResult = OperationResult<NatGateway>;

/// Result of a public IP address create or update operation
pub type PublicIpAddressOperationResult = OperationResult<PublicIpAddress>;

/// Result of a network security group create or update operation
pub type NetworkSecurityGroupOperationResult = OperationResult<NetworkSecurityGroup>;

// -------------------------------------------------------------------------
// Azure Network API trait
// -------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait NetworkApi: Send + Sync + std::fmt::Debug {
    // -------------------------------------------------------------------------
    // Virtual Network Operations
    // -------------------------------------------------------------------------

    /// Create or update a virtual network
    ///
    /// This method handles the Azure Virtual Network API for both creating new VNets
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    ///
    /// The operation may complete synchronously (201/200 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    async fn create_or_update_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        virtual_network: &VirtualNetwork,
    ) -> Result<VirtualNetworkOperationResult>;

    /// Get a virtual network by name
    async fn get_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<VirtualNetwork>;

    /// Delete a virtual network
    ///
    /// This method deletes a Virtual Network. The operation may complete synchronously with
    /// a 204 status code if the deletion is immediate, or asynchronously returning
    /// a 202 status code if the deletion is in progress.
    async fn delete_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<OperationResult<()>>;

    // -------------------------------------------------------------------------
    // Subnet Operations
    // -------------------------------------------------------------------------

    /// Create or update a subnet within a virtual network
    ///
    /// This method handles the Azure Subnet API for both creating new subnets
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    async fn create_or_update_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
        subnet: &Subnet,
    ) -> Result<SubnetOperationResult>;

    /// Get a subnet by name
    async fn get_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<Subnet>;

    /// Delete a subnet from a virtual network
    async fn delete_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<OperationResult<()>>;

    // -------------------------------------------------------------------------
    // NAT Gateway Operations
    // -------------------------------------------------------------------------

    /// Create or update a NAT gateway
    ///
    /// This method handles the Azure NAT Gateway API for both creating new NAT gateways
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    async fn create_or_update_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
        nat_gateway: &NatGateway,
    ) -> Result<NatGatewayOperationResult>;

    /// Get a NAT gateway by name
    async fn get_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<NatGateway>;

    /// Delete a NAT gateway
    async fn delete_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<OperationResult<()>>;

    // -------------------------------------------------------------------------
    // Public IP Address Operations
    // -------------------------------------------------------------------------

    /// Create or update a public IP address
    ///
    /// This method handles the Azure Public IP Address API for both creating new public IPs
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    async fn create_or_update_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
        public_ip_address: &PublicIpAddress,
    ) -> Result<PublicIpAddressOperationResult>;

    /// Get a public IP address by name
    async fn get_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<PublicIpAddress>;

    /// Delete a public IP address
    async fn delete_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<OperationResult<()>>;

    // -------------------------------------------------------------------------
    // Network Security Group Operations
    // -------------------------------------------------------------------------

    /// Create or update a network security group
    ///
    /// This method handles the Azure Network Security Group API for both creating new NSGs
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    async fn create_or_update_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
        network_security_group: &NetworkSecurityGroup,
    ) -> Result<NetworkSecurityGroupOperationResult>;

    /// Get a network security group by name
    async fn get_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<NetworkSecurityGroup>;

    /// Delete a network security group
    async fn delete_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<OperationResult<()>>;
}

// -------------------------------------------------------------------------
// Azure Network client struct
// -------------------------------------------------------------------------

/// Azure Network client for managing Virtual Networks, Subnets, NAT Gateways,
/// Public IP Addresses, and Network Security Groups.
#[derive(Debug)]
pub struct AzureNetworkClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureNetworkClient {
    /// API version for Azure Network resources
    const API_VERSION: &'static str = "2024-05-01";

    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        // Azure Resource Manager endpoint
        let endpoint = token_cache.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(client, endpoint, token_cache.config().clone()),
            token_cache,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl NetworkApi for AzureNetworkClient {
    // -------------------------------------------------------------------------
    // Virtual Network Operations
    // -------------------------------------------------------------------------

    async fn create_or_update_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        virtual_network: &VirtualNetwork,
    ) -> Result<VirtualNetworkOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, virtual_network_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(virtual_network)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize virtual network: {}",
                    virtual_network_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdateVirtualNetwork",
                virtual_network_name,
            )
            .await
    }

    async fn get_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<VirtualNetwork> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, virtual_network_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetVirtualNetwork", virtual_network_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetVirtualNetwork: failed to read response body for {}",
                    virtual_network_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let virtual_network: VirtualNetwork = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetVirtualNetwork: JSON parse error for {}",
                    virtual_network_name
                ),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            })?;

        Ok(virtual_network)
    }

    async fn delete_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, virtual_network_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "DeleteVirtualNetwork",
                virtual_network_name,
            )
            .await
    }

    // -------------------------------------------------------------------------
    // Subnet Operations
    // -------------------------------------------------------------------------

    async fn create_or_update_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
        subnet: &Subnet,
    ) -> Result<SubnetOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}/subnets/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, virtual_network_name, subnet_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(subnet).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize subnet: {}", subnet_name),
            },
        )?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "CreateOrUpdateSubnet", subnet_name)
            .await
    }

    async fn get_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<Subnet> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}/subnets/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, virtual_network_name, subnet_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetSubnet", subnet_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetSubnet: failed to read response body for {}",
                    subnet_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let subnet: Subnet = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!("Azure GetSubnet: JSON parse error for {}", subnet_name),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(subnet)
    }

    async fn delete_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}/subnets/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, virtual_network_name, subnet_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "DeleteSubnet", subnet_name)
            .await
    }

    // -------------------------------------------------------------------------
    // NAT Gateway Operations
    // -------------------------------------------------------------------------

    async fn create_or_update_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
        nat_gateway: &NatGateway,
    ) -> Result<NatGatewayOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/natGateways/{}",
                &self.token_cache.config().subscription_id, resource_group_name, nat_gateway_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(nat_gateway)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize NAT gateway: {}", nat_gateway_name),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdateNatGateway",
                nat_gateway_name,
            )
            .await
    }

    async fn get_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<NatGateway> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/natGateways/{}",
                &self.token_cache.config().subscription_id, resource_group_name, nat_gateway_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetNatGateway", nat_gateway_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetNatGateway: failed to read response body for {}",
                    nat_gateway_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let nat_gateway: NatGateway = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetNatGateway: JSON parse error for {}",
                    nat_gateway_name
                ),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(nat_gateway)
    }

    async fn delete_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/natGateways/{}",
                &self.token_cache.config().subscription_id, resource_group_name, nat_gateway_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "DeleteNatGateway", nat_gateway_name)
            .await
    }

    // -------------------------------------------------------------------------
    // Public IP Address Operations
    // -------------------------------------------------------------------------

    async fn create_or_update_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
        public_ip_address: &PublicIpAddress,
    ) -> Result<PublicIpAddressOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/publicIPAddresses/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, public_ip_address_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(public_ip_address)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize public IP address: {}",
                    public_ip_address_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdatePublicIpAddress",
                public_ip_address_name,
            )
            .await
    }

    async fn get_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<PublicIpAddress> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/publicIPAddresses/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, public_ip_address_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetPublicIpAddress", public_ip_address_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetPublicIpAddress: failed to read response body for {}",
                    public_ip_address_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let public_ip_address: PublicIpAddress = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetPublicIpAddress: JSON parse error for {}",
                    public_ip_address_name
                ),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            })?;

        Ok(public_ip_address)
    }

    async fn delete_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/publicIPAddresses/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, public_ip_address_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "DeletePublicIpAddress",
                public_ip_address_name,
            )
            .await
    }

    // -------------------------------------------------------------------------
    // Network Security Group Operations
    // -------------------------------------------------------------------------

    async fn create_or_update_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
        network_security_group: &NetworkSecurityGroup,
    ) -> Result<NetworkSecurityGroupOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/networkSecurityGroups/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, network_security_group_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(network_security_group)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize network security group: {}",
                    network_security_group_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateOrUpdateNetworkSecurityGroup",
                network_security_group_name,
            )
            .await
    }

    async fn get_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<NetworkSecurityGroup> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/networkSecurityGroups/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, network_security_group_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(
                signed,
                "GetNetworkSecurityGroup",
                network_security_group_name,
            )
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetNetworkSecurityGroup: failed to read response body for {}",
                    network_security_group_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let network_security_group: NetworkSecurityGroup = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetNetworkSecurityGroup: JSON parse error for {}",
                    network_security_group_name
                ),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            })?;

        Ok(network_security_group)
    }

    async fn delete_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/networkSecurityGroups/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, network_security_group_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "DeleteNetworkSecurityGroup",
                network_security_group_name,
            )
            .await
    }
}
