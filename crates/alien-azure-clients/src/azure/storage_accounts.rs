use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::models::storage::{
    CheckNameAvailabilityResult, StorageAccount, StorageAccountCheckNameAvailabilityParameters,
    StorageAccountCreateParameters, StorageAccountListKeysResult, StorageAccountUpdateParameters,
};
use crate::azure::AzureClientConfig;
use crate::azure::AzureClientConfigExt;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;

#[cfg(feature = "test-utils")]
use mockall::automock;

// -------------------------------------------------------------------------
// Type aliases for operation results
// -------------------------------------------------------------------------

/// Type alias for Storage Account operations that can be either completed or long-running
pub type StorageAccountOperationResult = OperationResult<StorageAccount>;

// -------------------------------------------------------------------------
// Storage Accounts API trait
// -------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait StorageAccountsApi: Send + Sync + std::fmt::Debug {
    async fn create_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
        parameters: &StorageAccountCreateParameters,
    ) -> Result<StorageAccountOperationResult>;

    async fn delete_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<()>;

    async fn update_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
        parameters: &StorageAccountUpdateParameters,
    ) -> Result<StorageAccountOperationResult>;

    async fn get_storage_account_properties(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<StorageAccount>;

    async fn check_storage_account_name_availability(
        &self,
        parameters: &StorageAccountCheckNameAvailabilityParameters,
    ) -> Result<CheckNameAvailabilityResult>;

    async fn list_storage_account_keys(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<StorageAccountListKeysResult>;
}

// -------------------------------------------------------------------------
// Storage Accounts client struct
// -------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureStorageAccountsClient {
    pub base: AzureClientBase,
    pub client_config: AzureClientConfig,
}

impl AzureStorageAccountsClient {
    pub fn new(client: Client, client_config: AzureClientConfig) -> Self {
        let endpoint = client_config.management_endpoint().to_string();
        Self {
            base: AzureClientBase::with_client_config(client, endpoint, client_config.clone()),
            client_config,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl StorageAccountsApi for AzureStorageAccountsClient {
    /// Create a storage account
    async fn create_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
        parameters: &StorageAccountCreateParameters,
    ) -> Result<StorageAccountOperationResult> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}",
                &self.client_config.subscription_id, resource_group_name, account_name
            ),
            Some(vec![("api-version", "2023-01-01".into())]),
        );

        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize storage account create parameters for resource: {}",
                    account_name
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
            .execute_request_with_long_running_support(signed, "CreateStorageAccount", account_name)
            .await
    }

    /// Delete a storage account
    async fn delete_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<()> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}",
                &self.client_config.subscription_id, resource_group_name, account_name
            ),
            Some(vec![("api-version", "2023-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteStorageAccount", account_name)
            .await?;

        Ok(())
    }

    /// Update a storage account
    async fn update_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
        parameters: &StorageAccountUpdateParameters,
    ) -> Result<StorageAccountOperationResult> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}",
                &self.client_config.subscription_id, resource_group_name, account_name
            ),
            Some(vec![("api-version", "2023-01-01".into())]),
        );

        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize storage account update parameters for resource: {}",
                    account_name
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
            .execute_request_with_long_running_support(signed, "UpdateStorageAccount", account_name)
            .await
    }

    /// Get storage account properties
    async fn get_storage_account_properties(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<StorageAccount> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}",
                &self.client_config.subscription_id, resource_group_name, account_name
            ),
            Some(vec![("api-version", "2023-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetStorageAccount", account_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetStorageAccount: failed to read response body for {}",
                    account_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: None,
            })?;

        let storage_account: StorageAccount = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetStorageAccount: JSON parse error for {}",
                    account_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(body),
            })?;

        Ok(storage_account)
    }

    /// Check storage account name availability
    async fn check_storage_account_name_availability(
        &self,
        parameters: &StorageAccountCheckNameAvailabilityParameters,
    ) -> Result<CheckNameAvailabilityResult> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/providers/Microsoft.Storage/checkNameAvailability",
                &self.client_config.subscription_id
            ),
            Some(vec![("api-version", "2023-01-01".into())]),
        );

        let request_body: String = serde_json::to_string(parameters).into_alien_error().context(ErrorData::SerializationError {
            message: format!("Failed to serialize storage account check name availability parameters for resource: {}", parameters.name),
        })?;

        let builder = AzureRequestBuilder::new(Method::POST, url.clone())
            .content_type_json()
            .content_length(&request_body)
            .body(request_body.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(
                signed,
                "CheckStorageAccountNameAvailability",
                &parameters.name,
            )
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: format!(
                "Azure CheckStorageAccountNameAvailability: failed to read response body for {}",
                parameters.name
            ),
                    url: url.clone(),
                    http_status: 200,
                    http_request_text: Some(request_body.clone()),
                    http_response_text: None,
                })?;

        let result: CheckNameAvailabilityResult = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure CheckStorageAccountNameAvailability: JSON parse error for {}",
                    parameters.name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: Some(request_body),
                http_response_text: Some(response_body),
            })?;

        Ok(result)
    }

    /// List storage account access keys
    async fn list_storage_account_keys(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<StorageAccountListKeysResult> {
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}/listKeys",
                &self.client_config.subscription_id, resource_group_name, account_name
            ),
            Some(vec![("api-version", "2023-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::POST, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "ListStorageAccountKeys", account_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure ListStorageAccountKeys: failed to read response body for {}",
                    account_name
                ),
                url: url.clone(),
                http_status: 200,
                http_request_text: None,
                http_response_text: None,
            })?;

        let result: StorageAccountListKeysResult = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
            message: format!(
                "Azure ListStorageAccountKeys: JSON parse error for {}",
                account_name
            ),
            url: url.clone(),
            http_status: 200,
            http_request_text: None,
            http_response_text: Some(body),
        })?;

        Ok(result)
    }
}
