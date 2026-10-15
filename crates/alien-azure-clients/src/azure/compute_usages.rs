use crate::azure::{
    common::{AzureClientBase, AzureRequestBuilder},
    models::compute_rp::Usage,
    token_cache::AzureTokenCache,
};
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::Deserialize;

#[cfg(feature = "test-utils")]
use mockall::automock;

/// One page of provider-reported applied limits and current usage.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComputeUsagesPage {
    value: Vec<Usage>,
    next_link: Option<String>,
}

/// Read-only Microsoft.Compute applied quota and usage operations.
#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ComputeUsagesApi: Send + Sync + std::fmt::Debug {
    /// Lists all usage pages returned for an Azure location.
    async fn list_compute_usages(&self, location: &str) -> Result<Vec<Usage>>;
}

/// Client for provider-reported regional compute quota usage.
#[derive(Debug)]
pub struct AzureComputeUsagesClient {
    base: AzureClientBase,
    token_cache: AzureTokenCache,
}

impl AzureComputeUsagesClient {
    const API_VERSION: &'static str = "2024-07-01";

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
impl ComputeUsagesApi for AzureComputeUsagesClient {
    async fn list_compute_usages(&self, location: &str) -> Result<Vec<Usage>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let mut next_url = Some(self.base.build_url(
            &format!(
                "/subscriptions/{}/providers/Microsoft.Compute/locations/{location}/usages",
                self.token_cache.config().subscription_id
            ),
            Some(vec![("api-version", Self::API_VERSION.to_string())]),
        ));
        let mut usages = Vec::new();

        while let Some(url) = next_url {
            let request = AzureRequestBuilder::new(Method::GET, url.clone())
                .content_length("")
                .build()?;
            let signed = self.base.sign_request(request, &bearer_token).await?;
            let response = self
                .base
                .execute_request(signed, "ListComputeUsages", location)
                .await?;
            let status = response.status().as_u16();
            let body =
                response
                    .text()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpResponseError {
                        message: format!(
                            "Azure ListComputeUsages: failed to read response body for {location}"
                        ),
                        url: url.clone(),
                        http_status: status,
                        http_response_text: None,
                        http_request_text: None,
                    })?;
            let page: ComputeUsagesPage = serde_json::from_str(&body).into_alien_error().context(
                ErrorData::HttpResponseError {
                    message: format!("Azure ListComputeUsages: JSON parse error for {location}"),
                    url,
                    http_status: status,
                    http_response_text: Some(body),
                    http_request_text: None,
                },
            )?;
            usages.extend(page.value);
            next_url = page.next_link;
        }

        Ok(usages)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::azure::{AzureClientConfig, AzureClientConfigExt, ServiceOverrides};
    use httpmock::{Method::GET, MockServer};
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn reads_authenticated_usage_pages_and_rejects_missing_usage() {
        let server = MockServer::start_async().await;
        let first = server.mock_async(|when, then| {
            when.method(GET).path("/subscriptions/12345678-1234-1234-1234-123456789012/providers/Microsoft.Compute/locations/eastus/usages")
                .query_param("api-version", "2024-07-01").header_exists("authorization");
            then.status(200).json_body(json!({"value":[{"name":{"value":"cores"},"currentValue":6,"limit":10,"unit":"Count"}],"nextLink":format!("{}/second",server.base_url())}));
        }).await;
        let second = server.mock_async(|when, then| {
            when.method(GET).path("/second").header_exists("authorization");
            then.status(200).json_body(json!({"value":[{"name":{"value":"standardDSv5Family"},"currentValue":0,"limit":0,"unit":"Count"}]}));
        }).await;
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("management".into(), server.base_url())]),
        });
        let rows = AzureComputeUsagesClient::new(Client::new(), AzureTokenCache::new(config))
            .list_compute_usages("eastus")
            .await
            .unwrap();
        first.assert_async().await;
        second.assert_async().await;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].current_value, 6);
        assert_eq!(rows[0].limit, 10);
        assert_eq!(rows[1].name.value.as_deref(), Some("standardDSv5Family"));
        assert_eq!(rows[1].limit, 0);
        for body in [
            json!({}),
            json!({"value":[{"name":{"value":"cores"},"limit":10,"unit":"Count"}]}),
        ] {
            assert!(serde_json::from_value::<ComputeUsagesPage>(body).is_err());
        }
    }
}
