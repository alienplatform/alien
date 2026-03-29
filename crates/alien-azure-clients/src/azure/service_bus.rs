use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::models::queue::{SbQueue, SbQueueListResult, SbQueueProperties};
use crate::azure::models::queue_namespace::{
    SbNamespace, SbNamespaceListResult, SbNamespaceProperties,
};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

/// Check if a header name is a standard HTTP header that should not be treated as a custom property
fn is_standard_header(header_name: &str) -> bool {
    matches!(
        header_name,
        "authorization"
            | "content-type"
            | "content-length"
            | "host"
            | "user-agent"
            | "accept"
            | "accept-encoding"
            | "connection"
            | "date"
            | "server"
            | "transfer-encoding"
            | "brokerproperties"
            | "x-ms-request-id"
            | "x-ms-correlation-request-id"
            | "location"
            | "cache-control"
            | "pragma"
            | "expires"
            | "etag"
            | "last-modified"
            | "vary"
            | "strict-transport-security"
            | "x-content-type-options"
            | "x-frame-options"
    )
}

#[cfg(feature = "test-utils")]
use mockall::automock;

// -----------------------------------------------------------------------------
// Message structs for data plane operations
// -----------------------------------------------------------------------------

/// Parameters for sending a message to Service Bus
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageParameters {
    /// The message body content
    pub body: String,
    /// Message properties in BrokerProperties format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker_properties: Option<BrokerProperties>,
    /// Custom message properties
    #[serde(flatten)]
    pub custom_properties: HashMap<String, String>,
}

/// BrokerProperties for Service Bus messages (as per Azure Service Bus REST API)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BrokerProperties {
    /// Message label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Message correlation ID
    #[serde(rename = "CorrelationId", skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Session ID for session-aware messages
    #[serde(rename = "SessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Message ID
    #[serde(rename = "MessageId", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Reply to address
    #[serde(rename = "ReplyTo", skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// Time to live in seconds (Azure returns this as a float, e.g. 922337203685477.5)
    #[serde(rename = "TimeToLive", skip_serializing_if = "Option::is_none")]
    pub time_to_live: Option<f64>,
    /// Delivery count
    #[serde(rename = "DeliveryCount", skip_serializing_if = "Option::is_none")]
    pub delivery_count: Option<i64>,
    /// Lock token for peek-lock operations
    #[serde(rename = "LockToken", skip_serializing_if = "Option::is_none")]
    pub lock_token: Option<String>,
    /// Sequence number
    #[serde(rename = "SequenceNumber", skip_serializing_if = "Option::is_none")]
    pub sequence_number: Option<i64>,
    /// Enqueued time UTC
    #[serde(rename = "EnqueuedTimeUtc", skip_serializing_if = "Option::is_none")]
    pub enqueued_time_utc: Option<String>,
    /// Scheduled enqueue time UTC
    #[serde(
        rename = "ScheduledEnqueueTimeUtc",
        skip_serializing_if = "Option::is_none"
    )]
    pub scheduled_enqueue_time_utc: Option<String>,
}

/// Response from receiving a message from Service Bus
#[derive(Debug, Clone, Deserialize)]
pub struct ReceivedMessage {
    /// Message body
    pub body: String,
    /// Message properties
    pub broker_properties: Option<BrokerProperties>,
    /// Custom properties
    #[serde(flatten)]
    pub custom_properties: HashMap<String, String>,
}

/// Message settlement action after peek-lock
#[derive(Debug, Clone)]
pub enum MessageSettlement {
    Complete,
    Abandon,
    RenewLock,
}

// -----------------------------------------------------------------------------
// Service Bus Management API trait
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ServiceBusManagementApi: Send + Sync + std::fmt::Debug {
    // Namespace operations

    /// Create or update a Service Bus namespace
    async fn create_or_update_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
        parameters: SbNamespaceProperties,
    ) -> Result<SbNamespace>;

    /// Get a Service Bus namespace
    async fn get_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<SbNamespace>;

    /// List Service Bus namespaces in a resource group
    async fn list_namespaces_by_resource_group(
        &self,
        resource_group_name: String,
    ) -> Result<SbNamespaceListResult>;

    /// Delete a Service Bus namespace
    async fn delete_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<()>;

    // Queue operations

    /// Create or update a Service Bus queue
    async fn create_or_update_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
        parameters: SbQueueProperties,
    ) -> Result<SbQueue>;

    /// Get a Service Bus queue
    async fn get_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<SbQueue>;

    /// List Service Bus queues in a namespace
    async fn list_queues(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<SbQueueListResult>;

    /// Delete a Service Bus queue
    async fn delete_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<()>;
}

// -----------------------------------------------------------------------------
// Service Bus Data Plane API trait
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ServiceBusDataPlaneApi: Send + Sync + std::fmt::Debug {
    /// Send a message to a Service Bus queue
    async fn send_message(
        &self,
        namespace_name: String,
        queue_name: String,
        message: SendMessageParameters,
    ) -> Result<()>;

    /// Receive and delete a message (destructive read)
    async fn receive_and_delete(
        &self,
        namespace_name: String,
        queue_name: String,
        timeout_seconds: Option<i32>,
    ) -> Result<Option<ReceivedMessage>>;

    /// Peek-lock a message (non-destructive read)
    async fn peek_lock(
        &self,
        namespace_name: String,
        queue_name: String,
        timeout_seconds: Option<i32>,
    ) -> Result<Option<ReceivedMessage>>;

    /// Complete a message (delete after peek-lock)
    async fn complete_message(
        &self,
        namespace_name: String,
        queue_name: String,
        message_id: String,
        lock_token: String,
    ) -> Result<()>;

    /// Abandon a message (unlock after peek-lock)
    async fn abandon_message(
        &self,
        namespace_name: String,
        queue_name: String,
        message_id: String,
        lock_token: String,
    ) -> Result<()>;

    /// Renew lock on a message
    async fn renew_message_lock(
        &self,
        namespace_name: String,
        queue_name: String,
        message_id: String,
        lock_token: String,
    ) -> Result<()>;
}

// -----------------------------------------------------------------------------
// Service Bus Management client
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureServiceBusManagementClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureServiceBusManagementClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        // Azure Resource Manager endpoint
        let endpoint = token_cache.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(client, endpoint, token_cache.config().clone()),
            token_cache,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ServiceBusManagementApi for AzureServiceBusManagementClient {
    /// Create or update a Service Bus namespace
    async fn create_or_update_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
        parameters: SbNamespaceProperties,
    ) -> Result<SbNamespace> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}",
                self.token_cache.config().subscription_id, resource_group_name, namespace_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        // Get location from platform config, defaulting to "eastus" if not specified
        let location = self
            .token_cache
            .config()
            .region
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "eastus".to_string());

        // Create namespace payload with required location field
        let namespace_payload = serde_json::json!({
            "location": location,
            "properties": parameters
        });

        let body = serde_json::to_string(&namespace_payload)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize namespace parameters for '{}'",
                    namespace_name
                ),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "CreateOrUpdateNamespace", &namespace_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "Service Bus CreateOrUpdateNamespace: failed to read response body"
                    ),
                })?;

        let namespace: SbNamespace = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus CreateOrUpdateNamespace: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(namespace)
    }

    /// Get a Service Bus namespace
    async fn get_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<SbNamespace> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}",
                self.token_cache.config().subscription_id, resource_group_name, namespace_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetNamespace", &namespace_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Service Bus GetNamespace: failed to read response body"),
                })?;

        let namespace: SbNamespace = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus GetNamespace: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(namespace)
    }

    /// List Service Bus namespaces in a resource group
    async fn list_namespaces_by_resource_group(
        &self,
        resource_group_name: String,
    ) -> Result<SbNamespaceListResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces",
                self.token_cache.config().subscription_id, resource_group_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(
                signed,
                "ListNamespacesByResourceGroup",
                &resource_group_name,
            )
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "Service Bus ListNamespacesByResourceGroup: failed to read response body"
                    ),
                })?;

        let namespaces: SbNamespaceListResult = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus ListNamespacesByResourceGroup: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(namespaces)
    }

    /// Delete a Service Bus namespace
    async fn delete_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}",
                self.token_cache.config().subscription_id, resource_group_name, namespace_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteNamespace", &namespace_name)
            .await?;

        Ok(())
    }

    /// Create or update a Service Bus queue
    async fn create_or_update_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
        parameters: SbQueueProperties,
    ) -> Result<SbQueue> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}/queues/{}",
                self.token_cache.config().subscription_id, resource_group_name, namespace_name, queue_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        // Wrap properties in queue structure
        let queue_payload = serde_json::json!({
            "properties": parameters
        });

        let body = serde_json::to_string(&queue_payload)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize queue parameters for '{}'", queue_name),
            })?;

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "CreateOrUpdateQueue", &queue_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                        "Service Bus CreateOrUpdateQueue: failed to read response body"
                    ),
                })?;

        let queue: SbQueue = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus CreateOrUpdateQueue: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(queue)
    }

    /// Get a Service Bus queue
    async fn get_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<SbQueue> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}/queues/{}",
                self.token_cache.config().subscription_id, resource_group_name, namespace_name, queue_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetQueue", &queue_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Service Bus GetQueue: failed to read response body"),
                })?;

        let queue: SbQueue = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus GetQueue: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(queue)
    }

    /// List Service Bus queues in a namespace
    async fn list_queues(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<SbQueueListResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}/queues",
                self.token_cache.config().subscription_id, resource_group_name, namespace_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "ListQueues", &namespace_name)
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!("Service Bus ListQueues: failed to read response body"),
                })?;

        let queues: SbQueueListResult = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus ListQueues: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(queues)
    }

    /// Delete a Service Bus queue
    async fn delete_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}/queues/{}",
                self.token_cache.config().subscription_id, resource_group_name, namespace_name, queue_name
            ),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteQueue", &queue_name)
            .await?;

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Service Bus Data Plane client
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureServiceBusDataPlaneClient {
    pub client: Client,
    pub token_cache: AzureTokenCache,
}

impl AzureServiceBusDataPlaneClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        Self {
            client,
            token_cache,
        }
    }

    /// Build the full URL for Service Bus data plane operations
    fn build_data_plane_url(
        &self,
        namespace_name: &str,
        path: &str,
        query_params: Option<Vec<(&str, String)>>,
    ) -> Result<Url> {
        let base_url =
            if let Some(override_url) = self.token_cache.get_service_endpoint("servicebus") {
                override_url.trim_end_matches('/').to_string()
            } else {
                format!("https://{}.servicebus.windows.net", namespace_name)
            };

        let mut url = Url::parse(&format!("{}{}", base_url, path))
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: format!("Invalid Service Bus URL: {}{}", base_url, path),
                errors: None,
            })?;

        if let Some(params) = query_params {
            let mut qp = url.query_pairs_mut();
            for (k, v) in params {
                qp.append_pair(k, &v);
            }
        }

        Ok(url)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ServiceBusDataPlaneApi for AzureServiceBusDataPlaneClient {
    /// Send a message to a Service Bus queue
    async fn send_message(
        &self,
        namespace_name: String,
        queue_name: String,
        message: SendMessageParameters,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://servicebus.azure.net/.default")
            .await?;

        let url =
            self.build_data_plane_url(&namespace_name, &format!("/{}/messages", queue_name), None)?;

        let mut request = self
            .client
            .post(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .header(
                "Content-Type",
                "application/atom+xml;type=entry;charset=utf-8",
            );

        // Add BrokerProperties header if provided
        if let Some(broker_props) = &message.broker_properties {
            let broker_props_json = serde_json::to_string(broker_props)
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: "Failed to serialize BrokerProperties".to_string(),
                })?;
            request = request.header("BrokerProperties", broker_props_json);
        }

        // Add custom properties as headers
        for (key, value) in &message.custom_properties {
            request = request.header(key, value);
        }

        let resp = request
            .body(message.body.clone())
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Service Bus SendMessage: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus SendMessage failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: Some(message.body),
                http_response_text: Some(error_text),
            }));
        }

        Ok(())
    }

    /// Receive and delete a message (destructive read)
    async fn receive_and_delete(
        &self,
        namespace_name: String,
        queue_name: String,
        timeout_seconds: Option<i32>,
    ) -> Result<Option<ReceivedMessage>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://servicebus.azure.net/.default")
            .await?;

        let mut query_params = vec![];
        if let Some(timeout) = timeout_seconds {
            query_params.push(("timeout", timeout.to_string()));
        }

        let url = self.build_data_plane_url(
            &namespace_name,
            &format!("/{}/messages/head", queue_name),
            if query_params.is_empty() {
                None
            } else {
                Some(query_params)
            },
        )?;

        let resp = self
            .client
            .delete(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Service Bus ReceiveAndDelete: failed to execute request"),
            })?;

        if resp.status() == 204 {
            // No messages available
            return Ok(None);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus ReceiveAndDelete failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        }

        // Parse BrokerProperties header
        let broker_properties =
            if let Some(broker_props_header) = resp.headers().get("BrokerProperties") {
                let broker_props_str = broker_props_header
                    .to_str()
                    .into_alien_error()
                    .context(ErrorData::HttpResponseError {
                        message: "BrokerProperties header contains non-ASCII characters".to_string(),
                        url: url.to_string(),
                        http_status: 200,
                        http_request_text: None,
                        http_response_text: None,
                    })?;
                let broker_properties: BrokerProperties = serde_json::from_str(broker_props_str)
                    .into_alien_error()
                    .context(ErrorData::HttpResponseError {
                        message: format!("Failed to parse BrokerProperties header: {}", broker_props_str),
                        url: url.to_string(),
                        http_status: 200,
                        http_request_text: None,
                        http_response_text: Some(broker_props_str.to_string()),
                    })?;
                Some(broker_properties)
            } else {
                None
            };

        // Parse custom properties from headers
        let mut custom_properties = HashMap::new();
        for (name, value) in resp.headers() {
            let name_str = name.as_str().to_lowercase();
            // Skip standard HTTP headers and Service Bus specific headers
            if !is_standard_header(&name_str) {
                if let Ok(value_str) = value.to_str() {
                    // Remove surrounding quotes if present (Azure Service Bus returns quoted string values)
                    let cleaned_value = if value_str.starts_with('"')
                        && value_str.ends_with('"')
                        && value_str.len() >= 2
                    {
                        &value_str[1..value_str.len() - 1]
                    } else {
                        value_str
                    };
                    custom_properties.insert(name.as_str().to_string(), cleaned_value.to_string());
                }
            }
        }

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Service Bus ReceiveAndDelete: failed to read response body"),
            })?;

        Ok(Some(ReceivedMessage {
            body,
            broker_properties,
            custom_properties,
        }))
    }

    /// Peek-lock a message (non-destructive read)
    async fn peek_lock(
        &self,
        namespace_name: String,
        queue_name: String,
        timeout_seconds: Option<i32>,
    ) -> Result<Option<ReceivedMessage>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://servicebus.azure.net/.default")
            .await?;

        let mut query_params = vec![];
        if let Some(timeout) = timeout_seconds {
            query_params.push(("timeout", timeout.to_string()));
        }

        let url = self.build_data_plane_url(
            &namespace_name,
            &format!("/{}/messages/head", queue_name),
            if query_params.is_empty() {
                None
            } else {
                Some(query_params)
            },
        )?;

        let resp = self
            .client
            .post(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .header("Content-Length", "0")
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Service Bus PeekLock: failed to execute request"),
            })?;

        if resp.status() == 204 {
            // No messages available
            return Ok(None);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus PeekLock failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        }

        // Parse BrokerProperties header
        let broker_properties =
            if let Some(broker_props_header) = resp.headers().get("BrokerProperties") {
                let broker_props_str = broker_props_header
                    .to_str()
                    .into_alien_error()
                    .context(ErrorData::HttpResponseError {
                        message: "BrokerProperties header contains non-ASCII characters".to_string(),
                        url: url.to_string(),
                        http_status: 200,
                        http_request_text: None,
                        http_response_text: None,
                    })?;
                let broker_properties: BrokerProperties = serde_json::from_str(broker_props_str)
                    .into_alien_error()
                    .context(ErrorData::HttpResponseError {
                        message: format!("Failed to parse BrokerProperties header: {}", broker_props_str),
                        url: url.to_string(),
                        http_status: 200,
                        http_request_text: None,
                        http_response_text: Some(broker_props_str.to_string()),
                    })?;
                Some(broker_properties)
            } else {
                None
            };

        // Parse custom properties from headers
        let mut custom_properties = HashMap::new();
        for (name, value) in resp.headers() {
            let name_str = name.as_str().to_lowercase();
            // Skip standard HTTP headers and Service Bus specific headers
            if !is_standard_header(&name_str) {
                if let Ok(value_str) = value.to_str() {
                    // Remove surrounding quotes if present (Azure Service Bus returns quoted string values)
                    let cleaned_value = if value_str.starts_with('"')
                        && value_str.ends_with('"')
                        && value_str.len() >= 2
                    {
                        &value_str[1..value_str.len() - 1]
                    } else {
                        value_str
                    };
                    custom_properties.insert(name.as_str().to_string(), cleaned_value.to_string());
                }
            }
        }

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Service Bus PeekLock: failed to read response body"),
            })?;

        Ok(Some(ReceivedMessage {
            body,
            broker_properties,
            custom_properties,
        }))
    }

    /// Complete a message (delete after peek-lock)
    async fn complete_message(
        &self,
        namespace_name: String,
        queue_name: String,
        message_id: String,
        lock_token: String,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://servicebus.azure.net/.default")
            .await?;

        let url = self.build_data_plane_url(
            &namespace_name,
            &format!("/{}/messages/{}/{}", queue_name, message_id, lock_token),
            None,
        )?;

        let resp = self
            .client
            .delete(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Service Bus CompleteMessage: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus CompleteMessage failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        }

        Ok(())
    }

    /// Abandon a message (unlock after peek-lock)
    async fn abandon_message(
        &self,
        namespace_name: String,
        queue_name: String,
        message_id: String,
        lock_token: String,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://servicebus.azure.net/.default")
            .await?;

        let url = self.build_data_plane_url(
            &namespace_name,
            &format!("/{}/messages/{}/{}", queue_name, message_id, lock_token),
            None,
        )?;

        let resp = self
            .client
            .put(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Service Bus AbandonMessage: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus AbandonMessage failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        }

        Ok(())
    }

    /// Renew lock on a message
    async fn renew_message_lock(
        &self,
        namespace_name: String,
        queue_name: String,
        message_id: String,
        lock_token: String,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://servicebus.azure.net/.default")
            .await?;

        let url = self.build_data_plane_url(
            &namespace_name,
            &format!("/{}/messages/{}/{}", queue_name, message_id, lock_token),
            None,
        )?;

        let resp = self
            .client
            .post(url.to_string())
            .header("Authorization", format!("Bearer {}", bearer_token))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Service Bus RenewMessageLock: failed to execute request"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::HttpResponseError {
                message: format!(
                    "Service Bus RenewMessageLock failed with status {}: {}",
                    status, error_text
                ),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(error_text),
            }));
        }

        Ok(())
    }
}
