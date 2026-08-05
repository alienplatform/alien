//! Read-only Vertex AI Model Garden availability client.

use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[derive(Debug)]
struct ModelGardenServiceConfig;

impl GcpServiceConfig for ModelGardenServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://aiplatform.googleapis.com/v1beta1"
    }

    fn default_audience(&self) -> &'static str {
        "https://aiplatform.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Vertex AI Model Garden"
    }

    fn service_key(&self) -> &'static str {
        "aiplatform"
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckPublisherModelEulaRequest {
    publisher_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherModelEulaAcceptance {
    pub publisher_model: Option<String>,
    #[serde(default)]
    pub publisher_model_eula_acked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherModel {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListPublisherModelsResponse {
    #[serde(default)]
    publisher_models: Vec<PublisherModel>,
    next_page_token: Option<String>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ModelGardenApi: Send + Sync + Debug {
    async fn list_publisher_models(&self, publisher: &str) -> Result<Vec<PublisherModel>>;

    /// Check, but never mutate, the project's EULA acceptance for a publisher model.
    async fn check_publisher_model_eula(
        &self,
        publisher_model: &str,
    ) -> Result<PublisherModelEulaAcceptance>;
}

#[derive(Debug)]
pub struct ModelGardenClient {
    base: GcpClientBase,
    project_id: String,
}

impl ModelGardenClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(ModelGardenServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ModelGardenApi for ModelGardenClient {
    async fn list_publisher_models(&self, publisher: &str) -> Result<Vec<PublisherModel>> {
        let mut page_token: Option<String> = None;
        let mut models = Vec::new();
        loop {
            let mut query = vec![("pageSize", "1000".to_string())];
            if let Some(token) = page_token.as_ref() {
                query.push(("pageToken", token.clone()));
            }
            let page: ListPublisherModelsResponse = self
                .base
                .execute_request(
                    Method::GET,
                    &format!("publishers/{publisher}/models"),
                    Some(query),
                    Option::<()>::None,
                    publisher,
                )
                .await?;
            models.extend(page.publisher_models);
            page_token = page.next_page_token.filter(|token| !token.is_empty());
            if page_token.is_none() {
                return Ok(models);
            }
        }
    }

    async fn check_publisher_model_eula(
        &self,
        publisher_model: &str,
    ) -> Result<PublisherModelEulaAcceptance> {
        self.base
            .execute_request(
                Method::POST,
                &format!("projects/{}/modelGardenEula:check", self.project_id),
                None,
                Some(CheckPublisherModelEulaRequest {
                    publisher_model: publisher_model.to_string(),
                }),
                publisher_model,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_eula_acceptance() {
        let parsed: PublisherModelEulaAcceptance = serde_json::from_str(
            r#"{"publisherModel":"publishers/anthropic/models/claude-sonnet-4-5","publisherModelEulaAcked":true}"#,
        )
        .unwrap();
        assert!(parsed.publisher_model_eula_acked);
    }
}
