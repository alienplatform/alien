use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::models::containerregistry::{
    GenerateCredentialsParameters, GenerateCredentialsResult, Registry, RegistryListResult,
    RegistryUpdateParameters, ScopeMap, ScopeMapListResult, ScopeMapProperties,
    ScopeMapUpdateParameters, Token, TokenListResult, TokenProperties, TokenUpdateParameters,
};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method};

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
                &self.token_cache.config().subscription_id, rg_name
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
            .execute_request_with_long_running_support(
                signed,
                "GenerateCredentials",
                registry_name,
            )
            .await
    }
}
