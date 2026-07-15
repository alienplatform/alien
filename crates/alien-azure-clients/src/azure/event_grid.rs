use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

const EVENT_GRID_API_VERSION: &str = "2025-02-15";

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait EventGridApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_event_subscription(
        &self,
        source_resource_id: String,
        event_subscription_name: String,
        parameters: EventSubscriptionRequest,
    ) -> Result<EventSubscription>;

    async fn delete_event_subscription(
        &self,
        source_resource_id: String,
        event_subscription_name: String,
    ) -> Result<()>;
}

#[derive(Debug)]
pub struct AzureEventGridClient {
    base: AzureClientBase,
    token_cache: AzureTokenCache,
}

impl AzureEventGridClient {
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

    fn event_subscription_url(
        &self,
        source_resource_id: &str,
        event_subscription_name: &str,
    ) -> String {
        self.base.build_url(
            &format!(
                "{}/providers/Microsoft.EventGrid/eventSubscriptions/{}",
                source_resource_id.trim_end_matches('/'),
                event_subscription_name
            ),
            Some(vec![("api-version", EVENT_GRID_API_VERSION.to_string())]),
        )
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl EventGridApi for AzureEventGridClient {
    async fn create_or_update_event_subscription(
        &self,
        source_resource_id: String,
        event_subscription_name: String,
        parameters: EventSubscriptionRequest,
    ) -> Result<EventSubscription> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let url = self.event_subscription_url(&source_resource_id, &event_subscription_name);
        let body = serde_json::to_string(&parameters)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize Event Grid subscription '{}'",
                    event_subscription_name
                ),
            })?;
        let request = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone())
            .build()?;
        let signed = self.base.sign_request(request, &token).await?;
        let response = self
            .base
            .execute_request(
                signed,
                "CreateOrUpdateEventSubscription",
                &event_subscription_name,
            )
            .await?;
        let response_status = response.status().as_u16();
        let response_body =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: format!(
                "Event Grid CreateOrUpdateEventSubscription: failed to read response body for '{}'",
                event_subscription_name
            ),
                })?;

        serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Event Grid CreateOrUpdateEventSubscription: JSON parse error. Body: {}",
                    response_body
                ),
                url,
                http_status: response_status,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })
    }

    async fn delete_event_subscription(
        &self,
        source_resource_id: String,
        event_subscription_name: String,
    ) -> Result<()> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let url = self.event_subscription_url(&source_resource_id, &event_subscription_name);
        let request = AzureRequestBuilder::new(Method::DELETE, url)
            .content_length("")
            .build()?;
        let signed = self.base.sign_request(request, &token).await?;
        match self
            .base
            .execute_request(signed, "DeleteEventSubscription", &event_subscription_name)
            .await
        {
            Ok(_) => Ok(()),
            Err(error) if matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventSubscriptionRequest {
    pub properties: EventSubscriptionRequestProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventSubscriptionRequestProperties {
    pub destination: ServiceBusQueueDestination,
    pub filter: EventSubscriptionFilter,
    pub event_delivery_schema: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceBusQueueDestination {
    pub endpoint_type: String,
    pub properties: ServiceBusQueueDestinationProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceBusQueueDestinationProperties {
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventSubscriptionFilter {
    pub included_event_types: Vec<String>,
    pub subject_begins_with: String,
    pub is_subject_case_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventSubscription {
    pub id: Option<String>,
    pub name: Option<String>,
    pub properties: Option<EventSubscriptionProperties>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventSubscriptionProperties {
    pub provisioning_state: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use httpmock::{Method::DELETE, Method::PUT, MockServer};
    use serde_json::json;

    use super::*;
    use crate::azure::{AzureClientConfig, AzureClientConfigExt, ServiceOverrides};

    fn test_client(server: &MockServer) -> AzureEventGridClient {
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url())]),
        });
        AzureEventGridClient::new(Client::new(), AzureTokenCache::new(config))
    }

    fn storage_subscription_request() -> EventSubscriptionRequest {
        EventSubscriptionRequest {
            properties: EventSubscriptionRequestProperties {
                destination: ServiceBusQueueDestination {
                    endpoint_type: "ServiceBusQueue".to_string(),
                    properties: ServiceBusQueueDestinationProperties {
                        resource_id: "/subscriptions/sub/resourceGroups/bus-rg/providers/Microsoft.ServiceBus/namespaces/bus/queues/storage-events".to_string(),
                    },
                },
                filter: EventSubscriptionFilter {
                    included_event_types: vec!["Microsoft.Storage.BlobCreated".to_string()],
                    subject_begins_with:
                        "/blobServices/default/containers/uploads/blobs/".to_string(),
                    is_subject_case_sensitive: false,
                },
                event_delivery_schema: "CloudEventSchemaV1_0".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn creates_storage_subscription_with_service_bus_destination_and_filter() {
        let server = MockServer::start_async().await;
        let source = "/subscriptions/sub/resourceGroups/storage-rg/providers/Microsoft.Storage/storageAccounts/files";
        let expected_body = json!({
            "properties": {
                "destination": {
                    "endpointType": "ServiceBusQueue",
                    "properties": {
                        "resourceId": "/subscriptions/sub/resourceGroups/bus-rg/providers/Microsoft.ServiceBus/namespaces/bus/queues/storage-events"
                    }
                },
                "filter": {
                    "includedEventTypes": ["Microsoft.Storage.BlobCreated"],
                    "subjectBeginsWith": "/blobServices/default/containers/uploads/blobs/",
                    "isSubjectCaseSensitive": false
                },
                "eventDeliverySchema": "CloudEventSchemaV1_0"
            }
        });
        let request_mock = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!(
                        "{source}/providers/Microsoft.EventGrid/eventSubscriptions/storage-sub"
                    ))
                    .query_param("api-version", EVENT_GRID_API_VERSION)
                    .json_body(expected_body);
                then.status(201).json_body(json!({
                    "name": "storage-sub",
                    "properties": { "provisioningState": "Succeeded" }
                }));
            })
            .await;

        let created = test_client(&server)
            .create_or_update_event_subscription(
                source.to_string(),
                "storage-sub".to_string(),
                storage_subscription_request(),
            )
            .await
            .expect("storage event subscription should be created");

        request_mock.assert_async().await;
        assert_eq!(
            created
                .properties
                .and_then(|properties| properties.provisioning_state)
                .as_deref(),
            Some("Succeeded")
        );
    }

    #[tokio::test]
    async fn deleting_a_missing_subscription_is_idempotent() {
        let server = MockServer::start_async().await;
        let source = "/subscriptions/sub/resourceGroups/storage-rg/providers/Microsoft.Storage/storageAccounts/files";
        let delete_mock = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path(format!(
                        "{source}/providers/Microsoft.EventGrid/eventSubscriptions/storage-sub"
                    ))
                    .query_param("api-version", EVENT_GRID_API_VERSION);
                then.status(404).json_body(json!({
                    "error": { "code": "ResourceNotFound", "message": "not found" }
                }));
            })
            .await;

        test_client(&server)
            .delete_event_subscription(source.to_string(), "storage-sub".to_string())
            .await
            .expect("deleting a missing event subscription should succeed");

        assert!(delete_mock.hits_async().await >= 1);
    }
}
