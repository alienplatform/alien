use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

#[derive(Debug)]
pub struct CloudAssetServiceConfig;

impl GcpServiceConfig for CloudAssetServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://cloudasset.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://cloudasset.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Cloud Asset Inventory"
    }

    fn service_key(&self) -> &'static str {
        "cloudasset"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CloudAssetApi: Send + Sync + Debug {
    async fn search_all_resources(
        &self,
        request: SearchAllResourcesRequest,
    ) -> Result<SearchAllResourcesResponse>;
}

#[derive(Debug)]
pub struct CloudAssetClient {
    base: GcpClientBase,
    project_id: String,
}

impl CloudAssetClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(CloudAssetServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudAssetApi for CloudAssetClient {
    /// Searches resources in a project, folder, or organization scope.
    /// See: https://cloud.google.com/asset-inventory/docs/reference/rest/v1/TopLevel/searchAllResources
    async fn search_all_resources(
        &self,
        request: SearchAllResourcesRequest,
    ) -> Result<SearchAllResourcesResponse> {
        let scope = request
            .scope
            .clone()
            .unwrap_or_else(|| format!("projects/{}", self.project_id));
        let path = format!("{scope}:searchAllResources");
        let query_params = request.query_params();

        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(query_params).filter(|params| !params.is_empty()),
                Option::<()>::None,
                &scope,
            )
            .await
    }
}

#[derive(Debug, Clone, Default, Builder)]
pub struct SearchAllResourcesRequest {
    pub scope: Option<String>,
    pub query: Option<String>,
    #[builder(default)]
    pub asset_types: Vec<String>,
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
    pub order_by: Option<String>,
    #[builder(default)]
    pub read_mask: Vec<String>,
}

impl SearchAllResourcesRequest {
    pub fn query_params(&self) -> Vec<(&'static str, String)> {
        let mut params = Vec::new();
        if let Some(query) = &self.query {
            params.push(("query", query.clone()));
        }
        for asset_type in &self.asset_types {
            params.push(("assetTypes", asset_type.clone()));
        }
        if let Some(page_size) = self.page_size {
            params.push(("pageSize", page_size.to_string()));
        }
        if let Some(page_token) = &self.page_token {
            params.push(("pageToken", page_token.clone()));
        }
        if let Some(order_by) = &self.order_by {
            params.push(("orderBy", order_by.clone()));
        }
        if !self.read_mask.is_empty() {
            params.push(("readMask", self.read_mask.join(",")));
        }
        params
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAllResourcesResponse {
    #[serde(default)]
    pub results: Vec<ResourceSearchResult>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSearchResult {
    pub name: Option<String>,
    pub asset_type: Option<String>,
    pub project: Option<String>,
    #[serde(default)]
    pub folders: Vec<String>,
    pub organization: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub network_tags: Vec<String>,
    pub kms_key: Option<String>,
    #[serde(default)]
    pub kms_keys: Vec<String>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
    pub state: Option<String>,
    pub parent_full_resource_name: Option<String>,
    pub parent_asset_type: Option<String>,
    pub additional_attributes: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<ResourceTag>,
    #[serde(default)]
    pub effective_tags: Vec<ResourceTag>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTag {
    pub tag_key: Option<String>,
    pub tag_key_id: Option<String>,
    pub tag_value: Option<String>,
    pub tag_value_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_all_resources_query_params_include_repeated_asset_types() {
        let request = SearchAllResourcesRequest::builder()
            .query("state:ACTIVE".to_string())
            .asset_types(vec![
                "storage.googleapis.com/Bucket".to_string(),
                "run.googleapis.com/Service".to_string(),
            ])
            .page_size(500)
            .page_token("next".to_string())
            .read_mask(vec![
                "name".to_string(),
                "assetType".to_string(),
                "labels".to_string(),
            ])
            .build();

        assert_eq!(
            request.query_params(),
            vec![
                ("query", "state:ACTIVE".to_string()),
                ("assetTypes", "storage.googleapis.com/Bucket".to_string()),
                ("assetTypes", "run.googleapis.com/Service".to_string()),
                ("pageSize", "500".to_string()),
                ("pageToken", "next".to_string()),
                ("readMask", "name,assetType,labels".to_string()),
            ]
        );
    }

    #[test]
    fn search_all_resources_response_deserializes_resource_fields() {
        let response: SearchAllResourcesResponse = serde_json::from_value(serde_json::json!({
            "results": [{
                "name": "//storage.googleapis.com/projects/_/buckets/logs",
                "assetType": "storage.googleapis.com/Bucket",
                "displayName": "logs",
                "location": "US",
                "labels": {
                    "env": "prod"
                },
                "effectiveTags": [{
                    "tagKey": "tagKeys/123",
                    "tagValue": "tagValues/456"
                }]
            }],
            "nextPageToken": "next"
        }))
        .unwrap();

        let resource = response.results.first().unwrap();
        assert_eq!(
            resource.asset_type.as_deref(),
            Some("storage.googleapis.com/Bucket")
        );
        assert_eq!(resource.labels.get("env").map(String::as_str), Some("prod"));
        assert_eq!(response.next_page_token.as_deref(), Some("next"));
    }
}
