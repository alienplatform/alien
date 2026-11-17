use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::tables::ETag;
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
    ///
    /// With `etag` set (the `etag` of a prior GET), the request carries
    /// `If-Match` and Azure refuses it with a conflict error when the gateway
    /// changed in between, so a concurrent change is never overwritten.
    async fn create_or_update_application_gateway(
        &self,
        resource_group_name: &str,
        application_gateway_name: &str,
        application_gateway: &Value,
        etag: Option<ETag>,
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
        etag: Option<ETag>,
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

        let mut builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);
        if let Some(etag) = etag {
            builder = builder.header("If-Match", &etag.0);
        }

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use httpmock::{Method::PUT, MockServer};
    use serde_json::json;

    use super::*;
    use crate::azure::{AzureClientConfig, AzureClientConfigExt, ServiceOverrides};

    const SUBSCRIPTION_ID: &str = "12345678-1234-1234-1234-123456789012";

    fn test_client(server: &MockServer) -> AzureApplicationGatewayClient {
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url())]),
        });
        AzureApplicationGatewayClient::new(Client::new(), AzureTokenCache::new(config))
    }

    fn gateway_path() -> String {
        format!(
            "/subscriptions/{SUBSCRIPTION_ID}/resourceGroups/test-rg/providers/Microsoft.Network/applicationGateways/test-gw"
        )
    }

    #[tokio::test]
    async fn the_update_carries_the_etag_it_was_given() {
        let server = MockServer::start_async().await;
        let put = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(gateway_path())
                    .header("If-Match", "W/\"abc\"");
                then.status(200).json_body(json!({"name": "test-gw"}));
            })
            .await;

        test_client(&server)
            .create_or_update_application_gateway(
                "test-rg",
                "test-gw",
                &json!({"location": "eastus"}),
                Some(ETag("W/\"abc\"".to_string())),
            )
            .await
            .expect("the update should succeed");

        put.assert_async().await;
    }

    #[tokio::test]
    async fn the_update_without_an_etag_sends_no_precondition() {
        let server = MockServer::start_async().await;
        // A request with If-Match would hit this mock instead and fail the
        // assertion below.
        let with_precondition = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(gateway_path())
                    .header_exists("If-Match");
                then.status(500);
            })
            .await;
        let plain = server
            .mock_async(|when, then| {
                when.method(PUT).path(gateway_path());
                then.status(200).json_body(json!({"name": "test-gw"}));
            })
            .await;

        test_client(&server)
            .create_or_update_application_gateway(
                "test-rg",
                "test-gw",
                &json!({"location": "eastus"}),
                None,
            )
            .await
            .expect("the update should succeed");

        assert_eq!(with_precondition.hits_async().await, 0);
        plain.assert_async().await;
    }

    #[tokio::test]
    async fn a_changed_gateway_is_a_conflict_error() {
        let server = MockServer::start_async().await;
        let refused = server
            .mock_async(|when, then| {
                when.method(PUT).path(gateway_path());
                then.status(412).json_body(json!({"error": {
                    "code": "PreconditionFailed",
                    "message": "The resource has changed."
                }}));
            })
            .await;

        let error = test_client(&server)
            .create_or_update_application_gateway(
                "test-rg",
                "test-gw",
                &json!({"location": "eastus"}),
                Some(ETag("stale".to_string())),
            )
            .await
            .expect_err("a stale etag must be refused");

        assert!(
            matches!(
                error.error,
                Some(ErrorData::RemoteResourceConflict { .. })
            ),
            "got {error:?}"
        );
        // The same etag would be refused again, so the client must not resend it.
        assert_eq!(refused.hits_async().await, 1);
    }
}
