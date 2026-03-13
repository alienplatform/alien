use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::AzureClientConfig;
use crate::azure::AzureClientConfigExt;
use alien_client_core::{Error, ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use reqwest::{Client, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

// -----------------------------------------------------------------------------
// Azure async operation status response
// -----------------------------------------------------------------------------

/// Response from polling Azure async operation URLs
/// Based on: https://learn.microsoft.com/en-us/azure/azure-resource-manager/management/async-operations
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AsyncOperationStatus {
    /// The status of the operation
    status: String,
    /// Optional error information if the operation failed
    #[serde(default)]
    error: Option<serde_json::Value>,
}

// -----------------------------------------------------------------------------
// Long-running operation data structure
// -----------------------------------------------------------------------------

/// Represents a long-running operation in Azure that can be polled for completion.
/// This is a pure data structure that contains the information needed to poll an operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LongRunningOperation {
    /// The URL to poll for operation status
    pub url: String,
    /// Optional retry delay as suggested by the server
    pub retry_after: Option<Duration>,
}

impl LongRunningOperation {
    /// Creates a new LongRunningOperation from response headers.
    ///
    /// Looks for Azure-AsyncOperation header first, then falls back to Location header.
    /// Also extracts Retry-After header if present.
    pub fn from_response_headers(response: &Response) -> Result<Option<LongRunningOperation>> {
        let headers = response.headers();

        // First, look for Azure-AsyncOperation header
        let url = if let Some(async_op) = headers.get("azure-asyncoperation") {
            async_op
                .to_str()
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: "Failed to serialize Azure-AsyncOperation header".to_string(),
                })?
                .to_string()
        } else if let Some(location) = headers.get("location") {
            // Fall back to Location header
            location
                .to_str()
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: "Failed to serialize Location header".to_string(),
                })?
                .to_string()
        } else {
            // No long-running operation headers found
            return Ok(None);
        };

        // Parse Retry-After header if present
        let retry_after =
            if let Some(retry_header) = headers.get("retry-after") {
                let retry_str = retry_header.to_str().into_alien_error().context(
                    ErrorData::SerializationError {
                        message: "Failed to serialize Retry-After header".to_string(),
                    },
                )?;

                let seconds: u64 = retry_str.parse().into_alien_error().context(
                    ErrorData::SerializationError {
                        message: "Failed to serialize Retry-After header as seconds".to_string(),
                    },
                )?;

                Some(Duration::from_secs(seconds))
            } else {
                None
            };

        Ok(Some(LongRunningOperation { url, retry_after }))
    }
}

// -----------------------------------------------------------------------------
// Long-running operation API trait
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait LongRunningOperationApi: Send + Sync + Debug {
    /// Checks the status of the long-running operation.
    ///
    /// Returns:
    /// - `Ok(Some(body))` if the operation is complete (200 OK)
    /// - `Ok(None)` if the operation is still running (202 Accepted)
    /// - `Err(Error)` if there was an error
    async fn check_status(
        &self,
        operation: &LongRunningOperation,
        operation_name: &str,
        resource_name: &str,
    ) -> Result<Option<String>>;

    /// Polls the long-running operation until completion with automatic retry delay.
    ///
    /// Returns the final response body when the operation completes.
    /// Uses the retry_after duration if available, otherwise defaults to 5 seconds between polls.
    #[cfg(not(target_arch = "wasm32"))]
    async fn wait_for_completion(
        &self,
        operation: &LongRunningOperation,
        operation_name: &str,
        resource_name: &str,
    ) -> Result<String>;
}

// -----------------------------------------------------------------------------
// Long-running operation client implementation
// -----------------------------------------------------------------------------

/// Client for handling Azure long-running operations.
/// This provides the implementation for checking status and waiting for completion.
#[derive(Debug)]
pub struct LongRunningOperationClient {
    pub base: AzureClientBase,
    pub client_config: AzureClientConfig,
}

impl LongRunningOperationClient {
    /// Creates a new LongRunningOperationClient
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
impl LongRunningOperationApi for LongRunningOperationClient {
    async fn check_status(
        &self,
        operation: &LongRunningOperation,
        operation_name: &str,
        resource_name: &str,
    ) -> Result<Option<String>> {
        debug!(operation = %operation_name, resource = %resource_name, url = %operation.url, "Checking Azure async operation status");
        let bearer_token = self
            .client_config
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let builder =
            AzureRequestBuilder::new(Method::GET, operation.url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        // Execute request with custom error handling for long-running operations
        let resp = self
            .base
            .execute_request(signed, operation_name, resource_name)
            .await?;

        let status = resp.status();
        match status {
            StatusCode::OK => {
                // Got 200 OK - need to check the JSON status field
                let body =
                    resp.text()
                        .await
                        .into_alien_error()
                        .context(ErrorData::HttpRequestFailed {
                            message: format!(
                                "Azure {operation_name}: failed to read response body"
                            ),
                        })?;

                // Try to parse as async operation status first
                if let Ok(operation_status) = serde_json::from_str::<AsyncOperationStatus>(&body) {
                    // This is an async operation status response
                    match operation_status.status.to_lowercase().as_str() {
                        "succeeded" => {
                            // Operation truly completed successfully
                            info!(operation = %operation_name, resource = %resource_name, "✅ Azure async operation completed successfully");
                            Ok(Some(body))
                        }
                        "failed" | "canceled" => {
                            // Operation failed or was canceled
                            let error_msg = if let Some(error) = &operation_status.error {
                                format!("Operation {}: {}", operation_status.status, error)
                            } else {
                                format!("Operation {}", operation_status.status)
                            };
                            warn!(operation = %operation_name, resource = %resource_name, status = %operation_status.status, error = ?operation_status.error, "❌ Azure async operation failed");
                            Err(AlienError::new(ErrorData::GenericError {
                                message: format!(
                                    "Azure {operation_name} for '{resource_name}' {}: {error_msg}",
                                    operation_status.status.to_lowercase()
                                ),
                            }))
                        }
                        _ => {
                            // Operation still in progress (e.g., "InProgress", "Accepted", etc.)
                            debug!(operation = %operation_name, resource = %resource_name, status = %operation_status.status, "⏳ Azure async operation still in progress");
                            Ok(None)
                        }
                    }
                } else {
                    // Handle empty response body (which can happen on successful deletes)
                    if body.trim().is_empty() {
                        info!(operation = %operation_name, resource = %resource_name, "✅ Azure operation completed (200 OK, empty body)");
                        return Ok(Some(body));
                    }

                    // This is likely a resource response (e.g., storage account), check for provisioningState
                    let parsed_json: serde_json::Value = serde_json::from_str(&body)
                        .into_alien_error().context(ErrorData::HttpResponseError {
                            message: format!("Azure {operation_name}: failed to parse response JSON. Body: {body}"),
                            url: operation.url.clone(),
                            http_status: 200,
                            http_request_text: None,
                            http_response_text: Some(body.clone()),
                        })?;

                    // Look for provisioningState in properties
                    if let Some(properties) = parsed_json.get("properties") {
                        if let Some(provisioning_state) = properties.get("provisioningState") {
                            if let Some(state_str) = provisioning_state.as_str() {
                                match state_str.to_lowercase().as_str() {
                                    "succeeded" => {
                                        info!(operation = %operation_name, resource = %resource_name, "✅ Azure resource operation completed successfully (provisioningState: Succeeded)");
                                        Ok(Some(body))
                                    }
                                    "failed" | "canceled" => {
                                        warn!(operation = %operation_name, resource = %resource_name, provisioning_state = %state_str, "❌ Azure resource operation failed");
                                        Err(AlienError::new(ErrorData::GenericError {
                                            message: format!("Azure {operation_name} for '{resource_name}' failed with provisioningState: {state_str}"),
                                        }))
                                    }
                                    _ => {
                                        // Operation still in progress (e.g., "Creating", "Updating", etc.)
                                        debug!(operation = %operation_name, resource = %resource_name, provisioning_state = %state_str, "⏳ Azure resource operation still in progress");
                                        Ok(None)
                                    }
                                }
                            } else {
                                // provisioningState is not a string, assume completed
                                info!(operation = %operation_name, resource = %resource_name, "✅ Azure operation completed (200 OK, non-string provisioningState)");
                                Ok(Some(body))
                            }
                        } else {
                            // No provisioningState found, assume completed since we got 200 OK
                            info!(operation = %operation_name, resource = %resource_name, "✅ Azure operation completed (200 OK, no provisioningState)");
                            Ok(Some(body))
                        }
                    } else {
                        // No properties found, assume completed since we got 200 OK
                        info!(operation = %operation_name, resource = %resource_name, "✅ Azure operation completed (200 OK, no properties)");
                        Ok(Some(body))
                    }
                }
            }
            StatusCode::NO_CONTENT => {
                // 204 No Content typically means operation completed successfully
                info!(operation = %operation_name, resource = %resource_name, "✅ Azure async operation completed (204 No Content)");
                Ok(Some(String::new()))
            }
            StatusCode::ACCEPTED => {
                // Operation still running
                debug!(operation = %operation_name, resource = %resource_name, "⏳ Azure async operation still running (202 Accepted)");
                Ok(None)
            }
            _ => {
                // Unexpected status code
                let body = resp.text().await.unwrap_or_default();
                Err(crate::azure::common::create_azure_http_error_with_context(
                    status,
                    operation_name,
                    "LongRunningOperation",
                    resource_name,
                    &body,
                    &operation.url,
                    None, // GET request has no body
                ))
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn wait_for_completion(
        &self,
        operation: &LongRunningOperation,
        operation_name: &str,
        resource_name: &str,
    ) -> Result<String> {
        let default_delay = Duration::from_secs(5);
        info!(operation = %operation_name, resource = %resource_name, url = %operation.url, "🚀 Starting Azure async operation polling");

        loop {
            match self
                .check_status(operation, operation_name, resource_name)
                .await?
            {
                Some(result) => {
                    // Operation completed
                    info!(operation = %operation_name, resource = %resource_name, "🎉 Azure async operation polling completed");
                    return Ok(result);
                }
                None => {
                    // Operation still running, wait before next poll
                    let delay = operation.retry_after.unwrap_or(default_delay);
                    debug!(operation = %operation_name, resource = %resource_name, delay_seconds = %delay.as_secs(), "💤 Waiting before next poll");

                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Generic operation result for Azure services with long-running operations
// -----------------------------------------------------------------------------

/// Generic result of an Azure operation that may be completed synchronously or asynchronously
#[derive(Debug)]
pub enum OperationResult<T> {
    /// The operation completed synchronously and returned the result
    Completed(T),
    /// The operation is running asynchronously and can be polled for completion
    LongRunning(LongRunningOperation),
}

impl<T> OperationResult<T> {
    /// Waits for the ARM operation to complete without returning the resource.
    ///
    /// If the operation was completed synchronously, returns immediately.
    /// If the operation is long-running, polls until the ARM operation completes.
    /// Note: This only waits for the Azure ARM operation to complete, not for the
    /// resource to be fully provisioned. Callers should make a separate GET request
    /// to retrieve the final resource state.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn wait_for_operation_completion(
        self,
        client: &dyn LongRunningOperationApi,
        operation_name: &str,
        resource_name: &str,
    ) -> Result<()> {
        match self {
            OperationResult::Completed(_) => Ok(()),
            OperationResult::LongRunning(long_running_op) => {
                // Just wait for the ARM operation to complete, don't try to parse the response
                client
                    .wait_for_completion(&long_running_op, operation_name, resource_name)
                    .await?;
                Ok(())
            }
        }
    }

    /// Gets the resource if the operation completed synchronously.
    ///
    /// Returns the resource if it was completed synchronously, or None if it's a long-running operation.
    /// This is useful when you want to handle synchronous and asynchronous operations differently.
    pub fn get_if_completed(self) -> Option<T> {
        match self {
            OperationResult::Completed(result) => Some(result),
            OperationResult::LongRunning(_) => None,
        }
    }
}
