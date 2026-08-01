use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::models::disk_rp::Disk;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Result of a disk create or update operation
pub type DiskOperationResult = OperationResult<Disk>;

// -------------------------------------------------------------------------
// Azure Managed Disks API trait
// -------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ManagedDisksApi: Send + Sync + std::fmt::Debug {
    /// Create or update a managed disk
    ///
    /// This method handles the Azure Managed Disks API for both creating new disks
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    ///
    /// The operation may complete synchronously (201/200 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    async fn create_or_update_disk(
        &self,
        resource_group_name: &str,
        disk_name: &str,
        disk: &Disk,
    ) -> Result<DiskOperationResult>;

    /// Get a managed disk by name
    async fn get_disk(&self, resource_group_name: &str, disk_name: &str) -> Result<Disk>;

    /// Delete a managed disk
    ///
    /// This method deletes a Managed Disk. The operation may complete synchronously with
    /// a 204 status code if the deletion is immediate, or asynchronously returning
    /// a 202 status code if the deletion is in progress.
    async fn delete_disk(
        &self,
        resource_group_name: &str,
        disk_name: &str,
    ) -> Result<OperationResult<()>>;
}

// -------------------------------------------------------------------------
// Azure Managed Disks client struct
// -------------------------------------------------------------------------

/// Azure Managed Disks client for managing Managed Disks for stateful containers.
#[derive(Debug)]
pub struct AzureManagedDisksClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureManagedDisksClient {
    /// API version for Azure Managed Disks resources
    const API_VERSION: &'static str = "2024-03-02";

    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        // Azure Resource Manager endpoint
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
impl ManagedDisksApi for AzureManagedDisksClient {
    async fn create_or_update_disk(
        &self,
        resource_group_name: &str,
        disk_name: &str,
        disk: &Disk,
    ) -> Result<DiskOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/disks/{}",
                &self.token_cache.config().subscription_id,
                resource_group_name,
                disk_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(disk).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize disk: {}", disk_name),
            },
        )?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "CreateOrUpdateDisk", disk_name)
            .await
    }

    async fn get_disk(&self, resource_group_name: &str, disk_name: &str) -> Result<Disk> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/disks/{}",
                &self.token_cache.config().subscription_id,
                resource_group_name,
                disk_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetDisk", disk_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetDisk: failed to read response body for {}",
                    disk_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let disk: Disk = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!("Azure GetDisk: JSON parse error for {}", disk_name),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(disk)
    }

    async fn delete_disk(
        &self,
        resource_group_name: &str,
        disk_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/disks/{}",
                &self.token_cache.config().subscription_id,
                resource_group_name,
                disk_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "DeleteDisk", disk_name)
            .await
    }
}
