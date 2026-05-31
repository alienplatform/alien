use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde_json::Value;

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Result of an Application Gateway create or update operation.
pub type ApplicationGatewayOperationResult = OperationResult<Value>;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ApplicationGatewayApi: Send + Sync + std::fmt::Debug {
    /// Create or update an Application Gateway.
    async fn create_or_update_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
        application_gateway: &Value,
    ) -> Result<ApplicationGatewayOperationResult>;

    /// Get an Application Gateway by name.
    async fn get_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
    ) -> Result<Value>;

    /// Delete an Application Gateway.
    async fn delete_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
    ) -> Result<OperationResult<()>>;
}

#[derive(Debug)]
pub struct AzureApplicationGatewayClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureApplicationGatewayClient {
    const API_VERSION: &'static str = "2024-05-01";

    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        let endpoint = token_cache.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
        }
    }

    fn resource_url(&self, resource_group_name: &str, application_gateway_name: &str) -> String {
        self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/applicationGateways/{}",
                &self.token_cache.config().subscription_id,
                resource_group_name,
                application_gateway_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        )
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ApplicationGatewayApi for AzureApplicationGatewayClient {
    async fn create_or_update_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
        application_gateway: &Value,
    ) -> Result<ApplicationGatewayOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.resource_url(resource_group_name, application_gateway_name);
        let body = serde_json::to_string(application_gateway)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize application gateway: {}",
                    application_gateway_name
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
                "CreateOrUpdateApplicationGateway",
                application_gateway_name,
            )
            .await
    }

    async fn get_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
    ) -> Result<Value> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.resource_url(resource_group_name, application_gateway_name);
        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetApplicationGateway", application_gateway_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetApplicationGateway: failed to read response body for {}",
                    application_gateway_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let application_gateway = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetApplicationGateway: JSON parse error for {}",
                    application_gateway_name
                ),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(application_gateway)
    }

    async fn delete_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.resource_url(resource_group_name, application_gateway_name);
        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(
                signed,
                "DeleteApplicationGateway",
                application_gateway_name,
            )
            .await
    }
}
