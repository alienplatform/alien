use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::models::blob::BlobContainer;
use crate::azure::AzureClientConfig;
use crate::azure::AzureClientConfigExt;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;

#[cfg(feature = "test-utils")]
use mockall::automock;

// -----------------------------------------------------------------------------
// Blob Container API trait
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait BlobContainerApi: Send + Sync + std::fmt::Debug {
    async fn create_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &BlobContainer,
    ) -> Result<BlobContainer>;

    async fn get_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<BlobContainer>;

    async fn delete_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<()>;

    async fn update_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &BlobContainer,
    ) -> Result<BlobContainer>;
}

// -----------------------------------------------------------------------------
// Blob Container client struct
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureBlobContainerClient {
    pub base: AzureClientBase,
    pub client_config: AzureClientConfig,
}

impl AzureBlobContainerClient {
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
impl BlobContainerApi for AzureBlobContainerClient {
    /// Create a blob container
    async fn create_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &BlobContainer,
    ) -> Result<BlobContainer> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}/blobServices/default/containers/{}",
                self.client_config.subscription_id, resource_group_name, storage_account_name, container_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let body = serde_json::to_string(blob_container)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize blob container '{}'.", container_name),
            })?;
        let request_body = body.clone(); // Store request body for error context

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "CreateBlobContainer", container_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure CreateBlobContainer: failed to read response body"),
            })?;

        let blob_container: BlobContainer = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure CreateBlobContainer: JSON parse error. Body: {}",
                    body
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: Some(request_body),
            })?;

        Ok(blob_container)
    }

    /// Get a blob container
    async fn get_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<BlobContainer> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}/blobServices/default/containers/{}",
                self.client_config.subscription_id, resource_group_name, storage_account_name, container_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetBlobContainer", container_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure GetBlobContainer: failed to read response body"),
            })?;

        let blob_container: BlobContainer = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!("Azure GetBlobContainer: JSON parse error. Body: {}", body),
                url: url.clone(),
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None, // GET request has no body
            })?;

        Ok(blob_container)
    }

    /// Delete a blob container
    async fn delete_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<()> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}/blobServices/default/containers/{}",
                self.client_config.subscription_id, resource_group_name, storage_account_name, container_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteBlobContainer", container_name)
            .await?;

        Ok(())
    }

    /// Update a blob container
    async fn update_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &BlobContainer,
    ) -> Result<BlobContainer> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}/blobServices/default/containers/{}",
                self.client_config.subscription_id, resource_group_name, storage_account_name, container_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let body = serde_json::to_string(blob_container)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize blob container '{}'.", container_name),
            })?;
        let request_body = body.clone(); // Store request body for error context

        let builder = AzureRequestBuilder::new(Method::PATCH, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "UpdateBlobContainer", container_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure UpdateBlobContainer: failed to read response body"),
            })?;

        let blob_container: BlobContainer = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure UpdateBlobContainer: JSON parse error. Body: {}",
                    body
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: Some(request_body),
            })?;

        Ok(blob_container)
    }
}
