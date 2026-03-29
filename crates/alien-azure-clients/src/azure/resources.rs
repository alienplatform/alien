use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::models::resources::{
    Provider, ProviderRegistrationRequest, ResourceGroup, ResourceGroupPatchable,
};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method};
use tracing::{debug, trace};

#[cfg(feature = "test-utils")]
use mockall::automock;

// -----------------------------------------------------------------------------
// Resources API trait
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ResourcesApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_resource_group(
        &self,
        resource_group_name: &str,
        resource_group: &ResourceGroup,
    ) -> Result<ResourceGroup>;

    async fn delete_resource_group(
        &self,
        resource_group_name: &str,
    ) -> Result<crate::azure::long_running_operation::OperationResult<()>>;

    async fn update_resource_group(
        &self,
        resource_group_name: &str,
        resource_group_patch: &ResourceGroupPatchable,
    ) -> Result<ResourceGroup>;

    async fn get_resource_group(&self, resource_group_name: &str) -> Result<ResourceGroup>;

    async fn get_provider(&self, resource_provider_namespace: &str) -> Result<Provider>;

    async fn register_provider(
        &self,
        resource_provider_namespace: &str,
        registration_request: Option<ProviderRegistrationRequest>,
    ) -> Result<Provider>;

    async fn unregister_provider(&self, resource_provider_namespace: &str) -> Result<Provider>;
}

/// Azure Resources client for managing resource groups and other resources.
#[derive(Debug)]
pub struct AzureResourcesClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureResourcesClient {
    /// Create a new Azure Resources client.
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
impl ResourcesApi for AzureResourcesClient {
    /// Create or update a resource group.
    ///
    /// # Arguments
    /// * `resource_group_name` - The name of the resource group
    /// * `resource_group` - The resource group object to create or update
    ///
    /// # Returns
    /// The created or updated resource group
    async fn create_or_update_resource_group(
        &self,
        resource_group_name: &str,
        resource_group: &ResourceGroup,
    ) -> Result<ResourceGroup> {
        let path = format!(
            "/subscriptions/{}/resourcegroups/{}",
            self.token_cache.config().subscription_id,
            resource_group_name
        );
        let url = self
            .base
            .build_url(&path, Some(vec![("api-version", "2021-04-01".to_string())]));

        let body = serde_json::to_string(resource_group)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize ResourceGroup: {}", resource_group_name),
            })?;

        let req = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone())
            .build()?;

        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let signed_req = self.base.sign_request(req, &bearer_token).await?;

        debug!("Creating/updating resource group: {}", resource_group_name);
        let response = self
            .base
            .execute_request(
                signed_req,
                "create_or_update_resource_group",
                resource_group_name,
            )
            .await?;

        let status = response.status().as_u16();

        let response_text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: format!(
                        "Failed to read response body for resource group: {}",
                        resource_group_name
                    ),
                    url: url.clone(),
                    http_status: status,
                    http_request_text: Some(body.clone()),
                    http_response_text: None,
                })?;

        trace!("Resource group response: {}", response_text);

        let resource_group: ResourceGroup = serde_json::from_str(&response_text)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Failed to deserialize ResourceGroup response for {}",
                    resource_group_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_text),
            })?;

        Ok(resource_group)
    }

    /// Delete a resource group (long running operation).
    ///
    /// # Arguments
    /// * `resource_group_name` - The name of the resource group to delete
    ///
    /// # Returns
    /// The result of the long running operation
    async fn delete_resource_group(
        &self,
        resource_group_name: &str,
    ) -> Result<crate::azure::long_running_operation::OperationResult<()>> {
        let path = format!(
            "/subscriptions/{}/resourcegroups/{}",
            self.token_cache.config().subscription_id,
            resource_group_name
        );
        let url = self
            .base
            .build_url(&path, Some(vec![("api-version", "2021-04-01".to_string())]));

        let req = AzureRequestBuilder::new(Method::DELETE, url.clone()).build()?;

        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let signed_req = self.base.sign_request(req, &bearer_token).await?;

        debug!("Deleting resource group: {}", resource_group_name);
        let operation_result = self
            .base
            .execute_request_with_long_running_support(
                signed_req,
                "delete_resource_group",
                resource_group_name,
            )
            .await?;

        Ok(operation_result)
    }

    /// Update a resource group.
    ///
    /// # Arguments
    /// * `resource_group_name` - The name of the resource group to update
    /// * `resource_group_patch` - The resource group patch object
    ///
    /// # Returns
    /// The updated resource group
    async fn update_resource_group(
        &self,
        resource_group_name: &str,
        resource_group_patch: &ResourceGroupPatchable,
    ) -> Result<ResourceGroup> {
        let path = format!(
            "/subscriptions/{}/resourcegroups/{}",
            self.token_cache.config().subscription_id,
            resource_group_name
        );
        let url = self
            .base
            .build_url(&path, Some(vec![("api-version", "2021-04-01".to_string())]));

        let body = serde_json::to_string(resource_group_patch)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize ResourceGroupPatchable: {}",
                    resource_group_name
                ),
            })?;

        let req = AzureRequestBuilder::new(Method::PATCH, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone())
            .build()?;

        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let signed_req = self.base.sign_request(req, &bearer_token).await?;

        debug!("Updating resource group: {}", resource_group_name);
        let response = self
            .base
            .execute_request(signed_req, "update_resource_group", resource_group_name)
            .await?;

        let status = response.status().as_u16();

        let response_text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: format!(
                        "Failed to read response body for resource group: {}",
                        resource_group_name
                    ),
                    url: url.clone(),
                    http_status: status,
                    http_request_text: Some(body.clone()),
                    http_response_text: None,
                })?;

        trace!("Resource group update response: {}", response_text);

        let resource_group: ResourceGroup = serde_json::from_str(&response_text)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Failed to deserialize ResourceGroup response for {}",
                    resource_group_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_text),
            })?;

        Ok(resource_group)
    }

    /// Get a resource group.
    ///
    /// # Arguments
    /// * `resource_group_name` - The name of the resource group to retrieve
    ///
    /// # Returns
    /// The resource group information
    async fn get_resource_group(&self, resource_group_name: &str) -> Result<ResourceGroup> {
        let path = format!(
            "/subscriptions/{}/resourcegroups/{}",
            self.token_cache.config().subscription_id,
            resource_group_name
        );
        let url = self
            .base
            .build_url(&path, Some(vec![("api-version", "2021-04-01".to_string())]));

        let req = AzureRequestBuilder::new(Method::GET, url.clone()).build()?;

        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let signed_req = self.base.sign_request(req, &bearer_token).await?;

        debug!("Getting resource group: {}", resource_group_name);
        let response = self
            .base
            .execute_request(signed_req, "get_resource_group", resource_group_name)
            .await?;

        let status = response.status().as_u16();

        let response_text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: format!(
                        "Failed to read response body for resource group: {}",
                        resource_group_name
                    ),
                    url: url.clone(),
                    http_status: status,
                    http_request_text: None,
                    http_response_text: None,
                })?;

        trace!("Resource group get response: {}", response_text);

        let resource_group: ResourceGroup = serde_json::from_str(&response_text)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Failed to deserialize ResourceGroup response for {}",
                    resource_group_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_text),
            })?;

        Ok(resource_group)
    }

    /// Get a resource provider.
    ///
    /// # Arguments
    /// * `resource_provider_namespace` - The namespace of the resource provider
    ///
    /// # Returns
    /// The resource provider information
    async fn get_provider(&self, resource_provider_namespace: &str) -> Result<Provider> {
        let path = format!(
            "/subscriptions/{}/providers/{}",
            self.token_cache.config().subscription_id,
            resource_provider_namespace
        );
        let url = self
            .base
            .build_url(&path, Some(vec![("api-version", "2021-04-01".to_string())]));

        let req = AzureRequestBuilder::new(Method::GET, url.clone()).build()?;

        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let signed_req = self.base.sign_request(req, &bearer_token).await?;

        debug!("Getting provider: {}", resource_provider_namespace);
        let response = self
            .base
            .execute_request(signed_req, "get_provider", resource_provider_namespace)
            .await?;

        let status = response.status().as_u16();

        let response_text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: format!(
                        "Failed to read response body for provider: {}",
                        resource_provider_namespace
                    ),
                    url: url.clone(),
                    http_status: status,
                    http_request_text: None,
                    http_response_text: None,
                })?;

        trace!("Provider get response: {}", response_text);

        let provider: Provider = serde_json::from_str(&response_text)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Failed to deserialize Provider response for {}",
                    resource_provider_namespace
                ),
                url: url.clone(),
                http_status: status,
                http_request_text: None,
                http_response_text: Some(response_text),
            })?;

        Ok(provider)
    }

    /// Register a subscription with a resource provider.
    ///
    /// # Arguments
    /// * `resource_provider_namespace` - The namespace of the resource provider to register
    /// * `registration_request` - Optional provider registration request with consent
    ///
    /// # Returns
    /// The registered provider information
    async fn register_provider(
        &self,
        resource_provider_namespace: &str,
        registration_request: Option<ProviderRegistrationRequest>,
    ) -> Result<Provider> {
        let path = format!(
            "/subscriptions/{}/providers/{}/register",
            self.token_cache.config().subscription_id,
            resource_provider_namespace
        );
        let url = self
            .base
            .build_url(&path, Some(vec![("api-version", "2021-04-01".to_string())]));

        let body = if let Some(req) = registration_request {
            serde_json::to_string(&req).into_alien_error().context(
                ErrorData::SerializationError {
                    message: format!(
                        "Failed to serialize ProviderRegistrationRequest for: {}",
                        resource_provider_namespace
                    ),
                },
            )?
        } else {
            "{}".to_string()
        };

        let req = AzureRequestBuilder::new(Method::POST, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone())
            .build()?;

        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let signed_req = self.base.sign_request(req, &bearer_token).await?;

        debug!("Registering provider: {}", resource_provider_namespace);
        let response = self
            .base
            .execute_request(signed_req, "register_provider", resource_provider_namespace)
            .await?;

        let status = response.status().as_u16();

        let response_text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: format!(
                        "Failed to read response body for provider registration: {}",
                        resource_provider_namespace
                    ),
                    url: url.clone(),
                    http_status: status,
                    http_request_text: Some(body.clone()),
                    http_response_text: None,
                })?;

        trace!("Provider registration response: {}", response_text);

        let provider: Provider = serde_json::from_str(&response_text)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Failed to deserialize Provider response for registration of {}",
                    resource_provider_namespace
                ),
                url: url.clone(),
                http_status: status,
                http_request_text: Some(body),
                http_response_text: Some(response_text),
            })?;

        Ok(provider)
    }

    /// Unregister a subscription from a resource provider.
    ///
    /// # Arguments
    /// * `resource_provider_namespace` - The namespace of the resource provider to unregister
    ///
    /// # Returns
    /// The unregistered provider information
    async fn unregister_provider(&self, resource_provider_namespace: &str) -> Result<Provider> {
        let path = format!(
            "/subscriptions/{}/providers/{}/unregister",
            self.token_cache.config().subscription_id,
            resource_provider_namespace
        );
        let url = self
            .base
            .build_url(&path, Some(vec![("api-version", "2021-04-01".to_string())]));

        let req = AzureRequestBuilder::new(Method::POST, url.clone()).build()?;

        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let signed_req = self.base.sign_request(req, &bearer_token).await?;

        debug!("Unregistering provider: {}", resource_provider_namespace);
        let response = self
            .base
            .execute_request(
                signed_req,
                "unregister_provider",
                resource_provider_namespace,
            )
            .await?;

        let status = response.status().as_u16();

        let response_text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: format!(
                        "Failed to read response body for provider unregistration: {}",
                        resource_provider_namespace
                    ),
                    url: url.clone(),
                    http_status: status,
                    http_request_text: None,
                    http_response_text: None,
                })?;

        trace!("Provider unregistration response: {}", response_text);

        let provider: Provider = serde_json::from_str(&response_text)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Failed to deserialize Provider response for unregistration of {}",
                    resource_provider_namespace
                ),
                url: url.clone(),
                http_status: status,
                http_request_text: None,
                http_response_text: Some(response_text),
            })?;

        Ok(provider)
    }
}
