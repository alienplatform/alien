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
pub struct MonitoringServiceConfig;

impl GcpServiceConfig for MonitoringServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://monitoring.googleapis.com/v3"
    }

    fn default_audience(&self) -> &'static str {
        "https://monitoring.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Cloud Monitoring"
    }

    fn service_key(&self) -> &'static str {
        "monitoring"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait MonitoringApi: Send + Sync + Debug {
    async fn list_time_series(
        &self,
        request: ListTimeSeriesRequest,
    ) -> Result<ListTimeSeriesResponse>;
}

#[derive(Debug)]
pub struct MonitoringClient {
    base: GcpClientBase,
    project_id: String,
}

impl MonitoringClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(MonitoringServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl MonitoringApi for MonitoringClient {
    /// Lists time series matching a monitoring filter.
    /// See: https://cloud.google.com/monitoring/api/ref_v3/rest/v3/projects.timeSeries/list
    async fn list_time_series(
        &self,
        request: ListTimeSeriesRequest,
    ) -> Result<ListTimeSeriesResponse> {
        let name = request
            .name
            .clone()
            .unwrap_or_else(|| format!("projects/{}", self.project_id));
        let path = format!("{name}/timeSeries");
        let query_params = request.query_params();

        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(query_params).filter(|params| !params.is_empty()),
                Option::<()>::None,
                &name,
            )
            .await
    }
}

#[derive(Debug, Clone, Default, Builder)]
pub struct ListTimeSeriesRequest {
    pub name: Option<String>,
    pub filter: String,
    pub interval: TimeInterval,
    pub aggregation: Option<Aggregation>,
    pub secondary_aggregation: Option<Aggregation>,
    pub order_by: Option<String>,
    pub view: TimeSeriesView,
    pub page_size: Option<u32>,
    pub page_token: Option<String>,
}

impl ListTimeSeriesRequest {
    pub fn query_params(&self) -> Vec<(&'static str, String)> {
        let mut params = vec![
            ("filter", self.filter.clone()),
            ("interval.startTime", self.interval.start_time.clone()),
            ("interval.endTime", self.interval.end_time.clone()),
            ("view", self.view.to_string()),
        ];

        if let Some(aggregation) = &self.aggregation {
            params.extend(aggregation.query_params("aggregation"));
        }
        if let Some(aggregation) = &self.secondary_aggregation {
            params.extend(aggregation.query_params("secondaryAggregation"));
        }
        if let Some(order_by) = &self.order_by {
            params.push(("orderBy", order_by.clone()));
        }
        if let Some(page_size) = self.page_size {
            params.push(("pageSize", page_size.to_string()));
        }
        if let Some(page_token) = &self.page_token {
            params.push(("pageToken", page_token.clone()));
        }

        params
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TimeInterval {
    pub start_time: String,
    pub end_time: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Aggregation {
    pub alignment_period: Option<String>,
    pub per_series_aligner: Option<String>,
    pub cross_series_reducer: Option<String>,
    #[builder(default)]
    pub group_by_fields: Vec<String>,
}

impl Aggregation {
    fn query_params(&self, prefix: &'static str) -> Vec<(&'static str, String)> {
        match prefix {
            "aggregation" => self.aggregation_query_params(),
            "secondaryAggregation" => self.secondary_aggregation_query_params(),
            _ => Vec::new(),
        }
    }

    fn aggregation_query_params(&self) -> Vec<(&'static str, String)> {
        let mut params = Vec::new();
        if let Some(value) = &self.alignment_period {
            params.push(("aggregation.alignmentPeriod", value.clone()));
        }
        if let Some(value) = &self.per_series_aligner {
            params.push(("aggregation.perSeriesAligner", value.clone()));
        }
        if let Some(value) = &self.cross_series_reducer {
            params.push(("aggregation.crossSeriesReducer", value.clone()));
        }
        for value in &self.group_by_fields {
            params.push(("aggregation.groupByFields", value.clone()));
        }
        params
    }

    fn secondary_aggregation_query_params(&self) -> Vec<(&'static str, String)> {
        let mut params = Vec::new();
        if let Some(value) = &self.alignment_period {
            params.push(("secondaryAggregation.alignmentPeriod", value.clone()));
        }
        if let Some(value) = &self.per_series_aligner {
            params.push(("secondaryAggregation.perSeriesAligner", value.clone()));
        }
        if let Some(value) = &self.cross_series_reducer {
            params.push(("secondaryAggregation.crossSeriesReducer", value.clone()));
        }
        for value in &self.group_by_fields {
            params.push(("secondaryAggregation.groupByFields", value.clone()));
        }
        params
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimeSeriesView {
    #[default]
    Full,
    Headers,
}

impl std::fmt::Display for TimeSeriesView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => f.write_str("FULL"),
            Self::Headers => f.write_str("HEADERS"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTimeSeriesResponse {
    #[serde(default)]
    pub time_series: Vec<TimeSeries>,
    pub next_page_token: Option<String>,
    pub execution_errors: Option<Vec<Status>>,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSeries {
    pub metric: Option<Metric>,
    pub resource: Option<MonitoredResource>,
    pub metric_kind: Option<String>,
    pub value_type: Option<String>,
    #[serde(default)]
    pub points: Vec<Point>,
    pub unit: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metric {
    #[serde(rename = "type")]
    pub type_: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoredResource {
    #[serde(rename = "type")]
    pub type_: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub interval: Option<TimeInterval>,
    pub value: Option<TypedValue>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedValue {
    pub bool_value: Option<bool>,
    pub int64_value: Option<String>,
    pub double_value: Option<f64>,
    pub string_value: Option<String>,
    pub distribution_value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub code: Option<i32>,
    pub message: Option<String>,
    #[serde(default)]
    pub details: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_time_series_query_params_include_interval_and_aggregation() {
        let request = ListTimeSeriesRequest::builder()
            .filter("metric.type=\"run.googleapis.com/request_count\"".to_string())
            .interval(
                TimeInterval::builder()
                    .start_time("2026-06-21T00:00:00Z".to_string())
                    .end_time("2026-06-21T00:05:00Z".to_string())
                    .build(),
            )
            .aggregation(
                Aggregation::builder()
                    .alignment_period("60s".to_string())
                    .per_series_aligner("ALIGN_RATE".to_string())
                    .group_by_fields(vec!["resource.label.service_name".to_string()])
                    .build(),
            )
            .view(TimeSeriesView::Headers)
            .page_size(10)
            .build();

        assert_eq!(
            request.query_params(),
            vec![
                (
                    "filter",
                    "metric.type=\"run.googleapis.com/request_count\"".to_string()
                ),
                ("interval.startTime", "2026-06-21T00:00:00Z".to_string()),
                ("interval.endTime", "2026-06-21T00:05:00Z".to_string()),
                ("view", "HEADERS".to_string()),
                ("aggregation.alignmentPeriod", "60s".to_string()),
                ("aggregation.perSeriesAligner", "ALIGN_RATE".to_string()),
                (
                    "aggregation.groupByFields",
                    "resource.label.service_name".to_string()
                ),
                ("pageSize", "10".to_string()),
            ]
        );
    }

    #[test]
    fn list_time_series_response_deserializes_points() {
        let response: ListTimeSeriesResponse = serde_json::from_value(serde_json::json!({
            "timeSeries": [{
                "metric": {
                    "type": "run.googleapis.com/request_count",
                    "labels": { "response_code": "200" }
                },
                "resource": {
                    "type": "cloud_run_revision",
                    "labels": { "service_name": "api" }
                },
                "points": [{
                    "interval": {
                        "startTime": "2026-06-21T00:00:00Z",
                        "endTime": "2026-06-21T00:05:00Z"
                    },
                    "value": { "int64Value": "42" }
                }]
            }],
            "nextPageToken": "next"
        }))
        .unwrap();

        let series = response.time_series.first().unwrap();
        assert_eq!(
            series
                .metric
                .as_ref()
                .and_then(|metric| metric.type_.as_deref()),
            Some("run.googleapis.com/request_count")
        );
        assert_eq!(
            series
                .points
                .first()
                .and_then(|point| point.value.as_ref())
                .and_then(|value| value.int64_value.as_deref()),
            Some("42")
        );
        assert_eq!(response.next_page_token.as_deref(), Some("next"));
    }
}
