use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::{LongRunningOperation, OperationResult};
use crate::azure::models::containerregistry::{
    GenerateCredentialsParameters, GenerateCredentialsResult, ImportImageParameters, Registry,
    RegistryListResult, RegistryUpdateParameters, ScopeMap, ScopeMapListResult, ScopeMapProperties,
    ScopeMapUpdateParameters, Token, TokenListResult, TokenProperties, TokenUpdateParameters,
};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use reqwest::{redirect::Policy, Client, Method, StatusCode};

#[cfg(feature = "test-utils")]
use mockall::automock;

// -------------------------------------------------------------------------
// Type aliases for operation results
// -------------------------------------------------------------------------

/// Type alias for Registry operations that can be either completed or long-running
pub type RegistryOperationResult = OperationResult<Registry>;

/// Type alias for ScopeMap operations that can be either completed or long-running
pub type ScopeMapOperationResult = OperationResult<ScopeMap>;

/// Type alias for Token operations that can be either completed or long-running
pub type TokenOperationResult = OperationResult<Token>;

/// Type alias for GenerateCredentials operations that can be either completed or long-running
pub type GenerateCredentialsOperationResult = OperationResult<GenerateCredentialsResult>;

// -------------------------------------------------------------------------
// Container Registry API trait
// -------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ContainerRegistryApi: Send + Sync + std::fmt::Debug {
    // Registry operations
    async fn create_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &Registry,
    ) -> Result<RegistryOperationResult>;

    async fn delete_registry(&self, resource_group_name: &str, registry_name: &str) -> Result<()>;

    async fn update_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &RegistryUpdateParameters,
    ) -> Result<RegistryOperationResult>;

    async fn get_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Registry>;

    async fn list_registries(&self, resource_group_name: Option<String>) -> Result<Vec<Registry>>;

    /// Import an immutable source image without retaining source credentials.
    async fn import_image(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &ImportImageParameters,
    ) -> Result<OperationResult<()>>;

    /// Poll an import without exposing provider response contents in errors.
    async fn poll_image_import(&self, operation: &LongRunningOperation) -> Result<bool>;

    // ScopeMap operations
    async fn create_scope_map(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        scope_map_name: &str,
        parameters: &ScopeMapProperties,
    ) -> Result<ScopeMapOperationResult>;

    async fn delete_scope_map(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        scope_map_name: &str,
    ) -> Result<()>;

    async fn update_scope_map(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        scope_map_name: &str,
        parameters: &ScopeMapUpdateParameters,
    ) -> Result<ScopeMapOperationResult>;

    async fn get_scope_map(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        scope_map_name: &str,
    ) -> Result<ScopeMap>;

    async fn list_scope_maps(
        &self,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Vec<ScopeMap>>;

    // Token operations
    async fn create_token(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        token_name: &str,
        parameters: &TokenProperties,
    ) -> Result<TokenOperationResult>;

    async fn delete_token(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        token_name: &str,
    ) -> Result<()>;

    async fn update_token(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        token_name: &str,
        parameters: &TokenUpdateParameters,
    ) -> Result<TokenOperationResult>;

    async fn get_token(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        token_name: &str,
    ) -> Result<Token>;

    async fn list_tokens(
        &self,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Vec<Token>>;

    // Credential generation
    async fn generate_credentials(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &GenerateCredentialsParameters,
    ) -> Result<GenerateCredentialsOperationResult>;
}

// -------------------------------------------------------------------------
// Container Registry client struct
// -------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureContainerRegistryClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureContainerRegistryClient {
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
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ContainerRegistryApi for AzureContainerRegistryClient {
    /// Create a container registry
    async fn create_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &Registry,
    ) -> Result<RegistryOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize registry create parameters for resource: {}",
                    registry_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(signed, "CreateRegistry", registry_name)
            .await
    }

    async fn import_image(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &ImportImageParameters,
    ) -> Result<OperationResult<()>> {
        let digest = parameters.source.source_image.split_once("@sha256:");
        if !digest.is_some_and(|(repository, digest)| {
            !repository.is_empty()
                && digest.len() == 64
                && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(AlienError::new(ErrorData::InvalidInput {
                message: "Image import requires an immutable SHA-256 source digest".into(),
                field_name: Some("source.sourceImage".into()),
            }));
        }
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/importImage",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );
        let body = serde_json::to_string(parameters).map_err(|_| {
            AlienError::new(ErrorData::SerializationError {
                message: "Cannot encode image import request".into(),
            })
        })?;
        let request = AzureRequestBuilder::new(Method::POST, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(request, &bearer_token).await?;
        let request_url = signed.url().clone();
        // Import carries a transient source password. The generic request helper
        // retains bodies in error context, and redirects could forward this body.
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|_| {
                AlienError::new(ErrorData::HttpRequestFailed {
                    message: "Cannot initialize image import transport".into(),
                })
            })?;
        let response = client.execute(signed).await.map_err(|_| {
            AlienError::new(ErrorData::HttpRequestFailed {
                message: "Image import transport failed".into(),
            })
        })?;
        let status = response.status();
        if status.is_success() {
            if let Some(operation) = LongRunningOperation::from_response_headers(&response)? {
                for value in std::iter::once(&operation.url).chain(operation.location_url.iter()) {
                    let same_origin = reqwest::Url::parse(value).is_ok_and(|url| {
                        url.origin() == request_url.origin()
                            && url.username().is_empty()
                            && url.password().is_none()
                            && url.fragment().is_none()
                            && url.query_pairs().all(|(key, value)| {
                                key == "api-version"
                                    && value.len() <= 32
                                    && value
                                        .bytes()
                                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                            })
                    });
                    if !same_origin {
                        return Err(AlienError::new(ErrorData::HttpRequestFailed {
                            message: "Image import returned an unauthorized polling endpoint"
                                .into(),
                        }));
                    }
                }
                return Ok(OperationResult::LongRunning(operation));
            }
            if status == StatusCode::OK || status == StatusCode::NO_CONTENT {
                return Ok(OperationResult::Completed(()));
            }
        }
        let error = match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorData::AuthenticationError {
                message: "Image import authorization was refused".into(),
            },
            StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                resource_type: "ContainerRegistry".into(),
                resource_name: registry_name.into(),
            },
            StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded {
                message: "Image import was rate limited".into(),
            },
            _ => ErrorData::HttpResponseError {
                message: "Image import returned an unexpected status or missing operation handle"
                    .into(),
                url: request_url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: None,
            },
        };
        Err(AlienError::new(error))
    }

    async fn poll_image_import(&self, operation: &LongRunningOperation) -> Result<bool> {
        let invalid = || {
            AlienError::new(ErrorData::HttpRequestFailed {
                message: "Image import polling endpoint is invalid".into(),
            })
        };
        let url = reqwest::Url::parse(&operation.url).map_err(|_| invalid())?;
        let expected =
            reqwest::Url::parse(self.token_cache.management_endpoint()).map_err(|_| invalid())?;
        if url.origin() != expected.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !url.query_pairs().all(|(key, value)| {
                key == "api-version"
                    && value.len() <= 32
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(invalid());
        }
        let token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|_| invalid())?;
        let mut response = client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| {
                AlienError::new(ErrorData::HttpRequestFailed {
                    message: "Image import polling transport failed".into(),
                })
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: "Image import polling failed".into(),
                url: self.token_cache.management_endpoint().to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: None,
            }));
        }
        if status == StatusCode::NO_CONTENT {
            return Ok(true);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| invalid())? {
            if bytes.len() + chunk.len() > 64 * 1024 {
                return Err(invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
            AlienError::new(ErrorData::SerializationError {
                message: "Image import polling returned invalid status data".into(),
            })
        })?;
        match value.get("status").and_then(serde_json::Value::as_str) {
            Some("Succeeded") => Ok(true),
            Some("Accepted" | "Running" | "InProgress") => Ok(false),
            _ => Err(AlienError::new(ErrorData::HttpRequestFailed {
                message: "Image import did not succeed".into(),
            })),
        }
    }

    /// Delete a container registry
    async fn delete_registry(&self, resource_group_name: &str, registry_name: &str) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteRegistry", registry_name)
            .await?;

        Ok(())
    }

    /// Update a container registry
    async fn update_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &RegistryUpdateParameters,
    ) -> Result<RegistryOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize registry update parameters for resource: {}",
                    registry_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PATCH, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(signed, "UpdateRegistry", registry_name)
            .await
    }

    /// Get container registry properties
    async fn get_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Registry> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );
        let url_string = url.to_string();

        let builder = AzureRequestBuilder::new(Method::GET, url);
        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetRegistry", registry_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure GetRegistry: failed to read response body".to_string(),
                })?;

        let registry: Registry = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetRegistry: JSON parse error. Body: {}",
                    response_body
                ),
                url: url_string,
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(registry)
    }

    /// List container registries
    async fn list_registries(&self, resource_group_name: Option<String>) -> Result<Vec<Registry>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let path = if let Some(rg_name) = resource_group_name {
            format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries",
                &self.token_cache.config().subscription_id,
                rg_name
            )
        } else {
            format!(
                "/subscriptions/{}/providers/Microsoft.ContainerRegistry/registries",
                &self.token_cache.config().subscription_id
            )
        };

        let url = self
            .base
            .build_url(&path, Some(vec![("api-version", "2025-04-01".into())]));
        let url_string = url.to_string();

        let builder = AzureRequestBuilder::new(Method::GET, url);
        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "ListRegistries", "")
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure ListRegistries: failed to read response body".to_string(),
                })?;

        let list_result: RegistryListResult = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ListRegistries: JSON parse error. Body: {}",
                    response_body
                ),
                url: url_string,
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(list_result.value)
    }

    /// Create a scope map
    async fn create_scope_map(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        scope_map_name: &str,
        parameters: &ScopeMapProperties,
    ) -> Result<ScopeMapOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/scopeMaps/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name, scope_map_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        // Wrap parameters in a ScopeMap structure
        let scope_map = ScopeMap {
            id: None,
            name: Some(scope_map_name.to_string()),
            properties: Some(parameters.clone()),
            system_data: None,
            type_: None,
        };

        let body = serde_json::to_string(&scope_map)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize scope map create parameters for resource: {}",
                    scope_map_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(signed, "CreateScopeMap", scope_map_name)
            .await
    }

    /// Delete a scope map
    async fn delete_scope_map(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        scope_map_name: &str,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/scopeMaps/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name, scope_map_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteScopeMap", scope_map_name)
            .await?;

        Ok(())
    }

    /// Update a scope map
    async fn update_scope_map(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        scope_map_name: &str,
        parameters: &ScopeMapUpdateParameters,
    ) -> Result<ScopeMapOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/scopeMaps/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name, scope_map_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize scope map update parameters for resource: {}",
                    scope_map_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PATCH, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(signed, "UpdateScopeMap", scope_map_name)
            .await
    }

    /// Get scope map properties
    async fn get_scope_map(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        scope_map_name: &str,
    ) -> Result<ScopeMap> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/scopeMaps/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name, scope_map_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );
        let url_string = url.to_string();

        let builder = AzureRequestBuilder::new(Method::GET, url);
        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetScopeMap", scope_map_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure GetScopeMap: failed to read response body".to_string(),
                })?;

        let scope_map: ScopeMap = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetScopeMap: JSON parse error. Body: {}",
                    response_body
                ),
                url: url_string,
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(scope_map)
    }

    /// List scope maps
    async fn list_scope_maps(
        &self,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Vec<ScopeMap>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/scopeMaps",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );
        let url_string = url.to_string();

        let builder = AzureRequestBuilder::new(Method::GET, url);
        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "ListScopeMaps", "")
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure ListScopeMaps: failed to read response body".to_string(),
                })?;

        let list_result: ScopeMapListResult = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ListScopeMaps: JSON parse error. Body: {}",
                    response_body
                ),
                url: url_string,
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(list_result.value)
    }

    /// Create a token
    async fn create_token(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        token_name: &str,
        parameters: &TokenProperties,
    ) -> Result<TokenOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/tokens/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name, token_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        // Wrap parameters in a Token structure
        let token = Token {
            id: None,
            name: Some(token_name.to_string()),
            properties: Some(parameters.clone()),
            system_data: None,
            type_: None,
        };

        let body = serde_json::to_string(&token).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize token create parameters for resource: {}",
                    token_name
                ),
            },
        )?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(signed, "CreateToken", token_name)
            .await
    }

    /// Delete a token
    async fn delete_token(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        token_name: &str,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/tokens/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name, token_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteToken", token_name)
            .await?;

        Ok(())
    }

    /// Update a token
    async fn update_token(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        token_name: &str,
        parameters: &TokenUpdateParameters,
    ) -> Result<TokenOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/tokens/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name, token_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize token update parameters for resource: {}",
                    token_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PATCH, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Use the generic long-running operation support
        self.base
            .execute_request_with_long_running_support(signed, "UpdateToken", token_name)
            .await
    }

    /// Get token properties
    async fn get_token(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        token_name: &str,
    ) -> Result<Token> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/tokens/{}",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name, token_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );
        let url_string = url.to_string();

        let builder = AzureRequestBuilder::new(Method::GET, url);
        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetToken", token_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure GetToken: failed to read response body".to_string(),
                })?;

        let token: Token = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!("Azure GetToken: JSON parse error. Body: {}", response_body),
                url: url_string,
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(token)
    }

    /// List tokens
    async fn list_tokens(
        &self,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Vec<Token>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/tokens",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );
        let url_string = url.to_string();

        let builder = AzureRequestBuilder::new(Method::GET, url);
        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self.base.execute_request(signed, "ListTokens", "").await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure ListTokens: failed to read response body".to_string(),
                })?;

        let list_result: TokenListResult = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ListTokens: JSON parse error. Body: {}",
                    response_body
                ),
                url: url_string,
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(list_result.value)
    }

    /// Generate credentials for a registry token.
    ///
    /// Calls `POST /registries/{registryName}/generateCredentials` to create
    /// a username/password pair tied to a token's scope map.
    async fn generate_credentials(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &GenerateCredentialsParameters,
    ) -> Result<GenerateCredentialsOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}/generateCredentials",
                &self.token_cache.config().subscription_id, resource_group_name, registry_name
            ),
            Some(vec![("api-version", "2025-04-01".into())]),
        );

        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize generateCredentials parameters for registry: {}",
                    registry_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::POST, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "GenerateCredentials", registry_name)
            .await
    }
}

#[cfg(test)]
#[path = "containerregistry_import_tests.rs"]
mod import_tests;
