use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::error::{safe_http_response_context, sanitized_diagnostic_url};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::Result;
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

const MONITOR_METRICS_API_VERSION: &str = "2023-10-01";
const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait MonitorApi: Send + Sync + std::fmt::Debug {
    async fn list_metrics(&self, request: ListMetricsRequest) -> Result<ListMetricsResponse>;
}

#[derive(Debug)]
pub struct AzureMonitorClient {
    base: AzureClientBase,
    token_cache: AzureTokenCache,
}

impl AzureMonitorClient {
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
impl MonitorApi for AzureMonitorClient {
    async fn list_metrics(&self, request: ListMetricsRequest) -> Result<ListMetricsResponse> {
        let path = format!(
            "{}/providers/Microsoft.Insights/metrics",
            request.resource_uri.trim_start_matches('/')
        );
        let query_params = request.query_params();
        let url = self.base.build_url(&format!("/{path}"), Some(query_params));
        let req = AzureRequestBuilder::new(Method::GET, url.clone()).build()?;
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let signed_req = self.base.sign_request(req, &bearer_token).await?;
        let response = self
            .base
            .execute_request(signed_req, "monitor_list_metrics", &request.resource_uri)
            .await?;
        let status = response.status();
        let diagnostic_url = sanitized_diagnostic_url(&url);
        let response_body = response
            .text()
            .await
            .map_err(reqwest::Error::without_url)
            .into_alien_error()
            .context(safe_http_response_context(
                "Failed to read Azure Monitor metrics response body",
                diagnostic_url.clone(),
                status,
            ))?;

        serde_json::from_str(&response_body)
            .into_alien_error()
            .context(safe_http_response_context(
                "Failed to deserialize Azure Monitor metrics response",
                diagnostic_url,
                status,
            ))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListMetricsRequest {
    pub resource_uri: String,
    pub timespan: Option<String>,
    pub interval: Option<String>,
    pub metric_names: Vec<String>,
    pub aggregation: Option<String>,
    pub top: Option<u32>,
    pub order_by: Option<String>,
    pub filter: Option<String>,
    pub result_type: Option<String>,
    pub metric_namespace: Option<String>,
    pub auto_adjust_timegrain: Option<bool>,
    pub validate_dimensions: Option<bool>,
    pub rollup_by: Option<String>,
}

impl ListMetricsRequest {
    pub fn new(resource_uri: impl Into<String>) -> Self {
        Self {
            resource_uri: resource_uri.into(),
            ..Default::default()
        }
    }

    pub fn query_params(&self) -> Vec<(&'static str, String)> {
        let mut params = vec![("api-version", MONITOR_METRICS_API_VERSION.to_string())];
        if let Some(timespan) = &self.timespan {
            params.push(("timespan", timespan.clone()));
        }
        if let Some(interval) = &self.interval {
            params.push(("interval", interval.clone()));
        }
        if !self.metric_names.is_empty() {
            params.push(("metricnames", self.metric_names.join(",")));
        }
        if let Some(aggregation) = &self.aggregation {
            params.push(("aggregation", aggregation.clone()));
        }
        if let Some(top) = self.top {
            params.push(("top", top.to_string()));
        }
        if let Some(order_by) = &self.order_by {
            params.push(("orderby", order_by.clone()));
        }
        if let Some(filter) = &self.filter {
            params.push(("$filter", filter.clone()));
        }
        if let Some(result_type) = &self.result_type {
            params.push(("resultType", result_type.clone()));
        }
        if let Some(metric_namespace) = &self.metric_namespace {
            params.push(("metricnamespace", metric_namespace.clone()));
        }
        if let Some(auto_adjust_timegrain) = self.auto_adjust_timegrain {
            params.push(("AutoAdjustTimegrain", auto_adjust_timegrain.to_string()));
        }
        if let Some(validate_dimensions) = self.validate_dimensions {
            params.push(("ValidateDimensions", validate_dimensions.to_string()));
        }
        if let Some(rollup_by) = &self.rollup_by {
            params.push(("rollupby", rollup_by.clone()));
        }
        params
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMetricsResponse {
    pub cost: Option<i32>,
    pub timespan: Option<String>,
    pub interval: Option<String>,
    pub namespace: Option<String>,
    #[serde(rename = "resourceregion")]
    pub resource_region: Option<String>,
    #[serde(default)]
    pub value: Vec<Metric>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metric {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub name: Option<LocalizableString>,
    pub display_description: Option<String>,
    pub unit: Option<String>,
    #[serde(default)]
    pub timeseries: Vec<TimeSeriesElement>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalizableString {
    pub value: Option<String>,
    pub localized_value: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSeriesElement {
    #[serde(default, rename = "metadatavalues")]
    pub metadata_values: Vec<MetadataValue>,
    #[serde(default)]
    pub data: Vec<MetricValue>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataValue {
    pub name: Option<LocalizableString>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricValue {
    pub time_stamp: Option<String>,
    pub average: Option<f64>,
    pub count: Option<f64>,
    pub maximum: Option<f64>,
    pub minimum: Option<f64>,
    pub total: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_metrics_query_params_include_required_api_version() {
        let mut request = ListMetricsRequest::new("/subscriptions/sub-1/resourceGroups/rg");
        request.timespan = Some("2026-06-21T00:00:00Z/2026-06-21T00:05:00Z".to_string());
        request.interval = Some("PT1M".to_string());
        request.metric_names = vec!["Requests".to_string(), "CpuPercentage".to_string()];
        request.aggregation = Some("average,maximum".to_string());
        request.metric_namespace = Some("Microsoft.App/containerApps".to_string());

        assert_eq!(
            request.query_params(),
            vec![
                ("api-version", "2023-10-01".to_string()),
                (
                    "timespan",
                    "2026-06-21T00:00:00Z/2026-06-21T00:05:00Z".to_string()
                ),
                ("interval", "PT1M".to_string()),
                ("metricnames", "Requests,CpuPercentage".to_string()),
                ("aggregation", "average,maximum".to_string()),
                ("metricnamespace", "Microsoft.App/containerApps".to_string()),
            ]
        );
    }

    #[test]
    fn list_metrics_response_deserializes_metric_points() {
        let response: ListMetricsResponse = serde_json::from_value(serde_json::json!({
            "timespan": "2026-06-21T00:00:00Z/2026-06-21T00:05:00Z",
            "interval": "PT1M",
            "namespace": "Microsoft.App/containerApps",
            "resourceregion": "eastus",
            "value": [{
                "name": { "value": "Requests", "localizedValue": "Requests" },
                "unit": "Count",
                "timeseries": [{
                    "data": [{
                        "timeStamp": "2026-06-21T00:00:00Z",
                        "total": 42
                    }]
                }],
                "errorCode": "Success"
            }]
        }))
        .unwrap();

        let metric = response.value.first().unwrap();
        assert_eq!(
            metric.name.as_ref().and_then(|name| name.value.as_deref()),
            Some("Requests")
        );
        assert_eq!(
            metric
                .timeseries
                .first()
                .and_then(|series| series.data.first())
                .and_then(|point| point.total),
            Some(42.0)
        );
    }

    #[test]
    fn monitor_diagnostic_url_redacts_query_and_userinfo() {
        const USER: &str = "MONITOR_USER_SECRET_0123456789";
        const PASSWORD: &str = "MONITOR_PASSWORD_SECRET_0123456789";
        const FILTER: &str = "MONITOR_FILTER_SECRET_0123456789";
        let diagnostic_url = sanitized_diagnostic_url(&format!(
            "https://{USER}:{PASSWORD}@management.azure.com/resource/providers/Microsoft.Insights/metrics?api-version=2023-10-01&$filter={FILTER}"
        ));

        assert_eq!(
            diagnostic_url,
            "https://management.azure.com/resource/providers/Microsoft.Insights/metrics"
        );
        assert!(!diagnostic_url.contains(USER));
        assert!(!diagnostic_url.contains(PASSWORD));
        assert!(!diagnostic_url.contains(FILTER));
    }
}
