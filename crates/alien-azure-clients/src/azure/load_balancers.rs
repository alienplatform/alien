use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::models::load_balancer::LoadBalancer;
use crate::azure::AzureClientConfig;
use crate::azure::AzureClientConfigExt;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Result of a load balancer create or update operation
pub type LoadBalancerOperationResult = OperationResult<LoadBalancer>;

// -------------------------------------------------------------------------
// Azure Load Balancer API trait
// -------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait LoadBalancerApi: Send + Sync + std::fmt::Debug {
    /// Create or update a load balancer
    ///
    /// This method handles the Azure Load Balancer API for both creating new load balancers
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    ///
    /// The operation may complete synchronously (201/200 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    async fn create_or_update_load_balancer(
        &self,
        resource_group_name: &str,
        load_balancer_name: &str,
        load_balancer: &LoadBalancer,
    ) -> Result<LoadBalancerOperationResult>;

    /// Get a load balancer by name
    async fn get_load_balancer(
        &self,
        resource_group_name: &str,
        load_balancer_name: &str,
    ) -> Result<LoadBalancer>;

    /// Delete a load balancer
    ///
    /// This method deletes a Load Balancer. The operation may complete synchronously with
    /// a 204 status code if the deletion is immediate, or asynchronously returning
    /// a 202 status code if the deletion is in progress.
    async fn delete_load_balancer(
        &self,
        resource_group_name: &str,
        load_balancer_name: &str,
    ) -> Result<OperationResult<()>>;
}

// -------------------------------------------------------------------------
// Azure Load Balancer client struct
// -------------------------------------------------------------------------

/// Azure Load Balancer client for managing Load Balancers with backend pools,
/// probes, load balancing rules, and outbound rules.
#[derive(Debug)]
pub struct AzureLoadBalancerClient {
    pub base: AzureClientBase,
    pub client_config: AzureClientConfig,
}

impl AzureLoadBalancerClient {
    /// API version for Azure Load Balancer resources
    const API_VERSION: &'static str = "2024-05-01";

    pub fn new(client: Client, client_config: AzureClientConfig) -> Self {
        // Azure Resource Manager endpoint
        let endpoint = client_config.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(client, endpoint, client_config.clone()),
            client_config,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl LoadBalancerApi for AzureLoadBalancerClient {
    async fn create_or_update_load_balancer(
        &self,
        resource_group_name: &str,
        load_balancer_name: &str,
        load_balancer: &LoadBalancer,
    ) -> Result<LoadBalancerOperationResult> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/loadBalancers/{}",
                &self.client_config.subscription_id, resource_group_name, load_balancer_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(load_balancer)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize load balancer: {}", load_balancer_name),
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
                "CreateOrUpdateLoadBalancer",
                load_balancer_name,
            )
            .await
    }

    async fn get_load_balancer(
        &self,
        resource_group_name: &str,
        load_balancer_name: &str,
    ) -> Result<LoadBalancer> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/loadBalancers/{}",
                &self.client_config.subscription_id, resource_group_name, load_balancer_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetLoadBalancer", load_balancer_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetLoadBalancer: failed to read response body for {}",
                    load_balancer_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let load_balancer: LoadBalancer = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetLoadBalancer: JSON parse error for {}",
                    load_balancer_name
                ),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(load_balancer)
    }

    async fn delete_load_balancer(
        &self,
        resource_group_name: &str,
        load_balancer_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/loadBalancers/{}",
                &self.client_config.subscription_id, resource_group_name, load_balancer_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "DeleteLoadBalancer",
                load_balancer_name,
            )
            .await
    }
}
