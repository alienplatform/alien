use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

const RESOURCE_GRAPH_API_VERSION: &str = "2024-04-01";
const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ResourceGraphApi: Send + Sync + std::fmt::Debug {
    async fn resources(
        &self,
        request: ResourceGraphQueryRequest,
    ) -> Result<ResourceGraphQueryResponse>;
}

#[derive(Debug)]
pub struct AzureResourceGraphClient {
    base: AzureClientBase,
    token_cache: AzureTokenCache,
}

impl AzureResourceGraphClient {
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
impl ResourceGraphApi for AzureResourceGraphClient {
    async fn resources(
        &self,
        request: ResourceGraphQueryRequest,
    ) -> Result<ResourceGraphQueryResponse> {
        let url = self.base.build_url(
            "/providers/Microsoft.ResourceGraph/resources",
            Some(vec![(
                "api-version",
                RESOURCE_GRAPH_API_VERSION.to_string(),
            )]),
        );
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize Azure Resource Graph query".to_string(),
            },
        )?;
        let req = AzureRequestBuilder::new(Method::POST, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body.clone())
            .build()?;
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let signed_req = self.base.sign_request(req, &bearer_token).await?;
        let response = self
            .base
            .execute_request(signed_req, "resource_graph_resources", "resources")
            .await?;
        let status = response.status().as_u16();
        let response_body =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: "Failed to read Azure Resource Graph response body".to_string(),
                    url: url.clone(),
                    http_status: status,
                    http_request_text: Some(body),
                    http_response_text: None,
                })?;

        serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: "Failed to deserialize Azure Resource Graph response".to_string(),
                url,
                http_status: status,
                http_request_text: None,
                http_response_text: Some(response_body),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGraphQueryRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscriptions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub management_groups: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ResourceGraphQueryOptions>,
}

impl ResourceGraphQueryRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            subscriptions: Vec::new(),
            management_groups: Vec::new(),
            options: None,
        }
    }

    pub fn for_subscription(subscription_id: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            subscriptions: vec![subscription_id.into()],
            management_groups: Vec::new(),
            options: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGraphQueryOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_format: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGraphQueryResponse {
    pub count: Option<u64>,
    pub total_records: Option<u64>,
    pub result_truncated: Option<String>,
    pub skip_token: Option<String>,
    #[serde(default)]
    pub data: Vec<ResourceGraphResource>,
    #[serde(default)]
    pub facets: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGraphResource {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub location: Option<String>,
    pub resource_group: Option<String>,
    pub subscription_id: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    pub kind: Option<String>,
    pub sku: Option<serde_json::Value>,
    pub properties: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_graph_request_serializes_subscription_query() {
        let request =
            ResourceGraphQueryRequest::for_subscription("sub-1", "Resources | project id");

        let value = serde_json::to_value(&request).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "query": "Resources | project id",
                "subscriptions": ["sub-1"]
            })
        );
    }

    #[test]
    fn resource_graph_response_deserializes_resource_rows() {
        let response: ResourceGraphQueryResponse = serde_json::from_value(serde_json::json!({
            "count": 1,
            "totalRecords": 1,
            "resultTruncated": "false",
            "data": [{
                "id": "/subscriptions/sub-1/resourceGroups/rg/providers/Microsoft.Storage/storageAccounts/logs",
                "name": "logs",
                "type": "microsoft.storage/storageaccounts",
                "location": "eastus",
                "resourceGroup": "rg",
                "subscriptionId": "sub-1",
                "tags": { "env": "prod" }
            }]
        }))
        .unwrap();

        let resource = response.data.first().unwrap();
        assert_eq!(
            resource.type_.as_deref(),
            Some("microsoft.storage/storageaccounts")
        );
        assert_eq!(resource.tags.get("env").map(String::as_str), Some("prod"));
    }
}
