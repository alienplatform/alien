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

    /// Update an existing gateway only if its cloud ETag still matches.
    /// Read-modify-write callers must use this to preserve concurrent changes.
    async fn update_application_gateway_if_match(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
        application_gateway: &Value,
        etag: &str,
    ) -> Result<ApplicationGatewayOperationResult>;

    /// Get an Application Gateway by name.
    async fn get_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
    ) -> Result<Value>;

    /// Get backend health for an Application Gateway.
    async fn get_application_gateway_backend_health(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
    ) -> Result<OperationResult<Value>>;

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

    async fn put_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
        application_gateway: &Value,
        etag: Option<&str>,
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

        let builder = if let Some(etag) = etag {
            builder.header("If-Match", etag)
        } else {
            builder
        };
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

    async fn get_application_gateway_backend_health(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
    ) -> Result<OperationResult<Value>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/applicationGateways/{}/backendhealth",
                &self.token_cache.config().subscription_id,
                resource_group_name,
                application_gateway_name
            ),
            Some(vec![("api-version", "2025-07-01".into())]),
        );
        let req = AzureRequestBuilder::new(Method::POST, url)
            .content_length("")
            .build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        self.base
            .execute_request_with_long_running_support(
                signed,
                "GetApplicationGatewayBackendHealth",
                application_gateway_name,
            )
            .await
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ApplicationGatewayApi for AzureApplicationGatewayClient {
    async fn get_application_gateway_backend_health(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
    ) -> Result<OperationResult<Value>> {
        AzureApplicationGatewayClient::get_application_gateway_backend_health(
            self,
            resource_group_name,
            application_gateway_name,
        )
        .await
    }

    async fn create_or_update_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
        application_gateway: &Value,
    ) -> Result<ApplicationGatewayOperationResult> {
        self.put_application_gateway(
            resource_group_name,
            application_gateway_name,
            application_gateway,
            None,
        )
        .await
    }

    async fn update_application_gateway_if_match(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
        application_gateway: &Value,
        etag: &str,
    ) -> Result<ApplicationGatewayOperationResult> {
        self.put_application_gateway(
            resource_group_name,
            application_gateway_name,
            application_gateway,
            Some(etag),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AzureClientConfig, AzureCredentials};
    use alien_core::AzureServiceOverrides;
    use httpmock::prelude::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn conditional_gateway_update_rejects_a_stale_cloud_snapshot() {
        let server = MockServer::start_async().await;
        let body = serde_json::json!({"properties": {"backendAddressPools": [{"name": "backend", "properties": {"backendAddresses": [{"ipAddress": "10.0.0.10"}]}}]}});
        let write = server.mock_async(|when, then| {
            when.method(PUT)
                .path("/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/applicationGateways/gateway")
                .query_param("api-version", "2024-05-01")
                .header("If-Match", "W/\"snapshot\"")
                .json_body(body.clone());
            then.status(412).json_body(serde_json::json!({"error": {"code": "PreconditionFailed", "message": "A concurrent gateway update changed the ETag"}}));
        }).await;
        let config = AzureClientConfig {
            subscription_id: "sub".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: Some(AzureServiceOverrides {
                endpoints: HashMap::from([("management".to_string(), server.base_url())]),
            }),
        };
        let client =
            AzureApplicationGatewayClient::new(Client::new(), AzureTokenCache::new(config));
        let error = client
            .update_application_gateway_if_match("rg", "gateway", &body, "W/\"snapshot\"")
            .await
            .expect_err("stale cloud snapshots must fail");
        assert!(
            matches!(error.error, Some(ErrorData::RemoteResourceConflict { .. })),
            "conditional conflict must stay visible to the reconciler: {error}"
        );
        assert!(
            !error.retryable,
            "a stale snapshot must be refreshed before retrying"
        );
        write.assert_hits_async(1).await;
    }
}
