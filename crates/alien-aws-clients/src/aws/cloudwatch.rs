use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::ContextError;
use bon::Builder;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

pub const GET_METRIC_DATA_TARGET: &str = "GraniteServiceVersion20100801.GetMetricData";
pub const LIST_METRICS_TARGET: &str = "GraniteServiceVersion20100801.ListMetrics";

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CloudWatchApi: Send + Sync + std::fmt::Debug {
    async fn get_metric_data(&self, request: GetMetricDataRequest)
        -> Result<GetMetricDataResponse>;

    async fn list_metrics(&self, request: ListMetricsRequest) -> Result<ListMetricsResponse>;
}

#[derive(Debug, Clone)]
pub struct CloudWatchClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl CloudWatchClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "monitoring".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn host(&self) -> String {
        format!("monitoring.{}.amazonaws.com", self.credentials.region())
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self
            .credentials
            .get_service_endpoint_option("monitoring")
            .or_else(|| self.credentials.get_service_endpoint_option("cloudwatch"))
        {
            override_url.to_string()
        } else {
            format!("https://{}", self.host())
        }
    }

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &str,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let url = format!("{}/", self.get_base_url().trim_end_matches('/'));

        let builder = self
            .client
            .post(&url)
            .host(&self.host())
            .header("X-Amz-Target", target)
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(&body))
    }

    fn map_result<T>(
        result: Result<T>,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(ref text),
                    ..
                }) = &error.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) =
                        Self::map_error(status, text, operation, resource, request_body)
                    {
                        return Err(error.context(mapped));
                    }
                }
                Err(error)
            }
        }
    }

    fn map_error(
        status: StatusCode,
        body: &str,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: Option<CloudWatchErrorResponse> = serde_json::from_str(body).ok();
        let code = parsed
            .as_ref()
            .map(|error| error.code.trim_start_matches('#'))
            .unwrap_or_default();
        let message = parsed
            .as_ref()
            .and_then(|error| error.message.clone())
            .unwrap_or_else(|| body.to_string());

        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Some(ErrorData::AuthenticationError { message })
            }
            StatusCode::TOO_MANY_REQUESTS => Some(ErrorData::RateLimitExceeded { message }),
            StatusCode::BAD_REQUEST
                if matches!(code, "InvalidParameterValue" | "InvalidNextToken") =>
            {
                Some(ErrorData::InvalidClientConfig {
                    message,
                    errors: None,
                })
            }
            StatusCode::INTERNAL_SERVER_ERROR if code == "InternalServiceError" => {
                Some(ErrorData::RemoteServiceUnavailable { message })
            }
            _ if !body.trim().is_empty() => Some(ErrorData::HttpResponseError {
                message: format!("{} failed for '{}': {}", operation, resource, message),
                url: String::new(),
                http_status: status.as_u16(),
                http_request_text: request_body.map(ToOwned::to_owned),
                http_response_text: Some(body.to_string()),
            }),
            _ => None,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudWatchApi for CloudWatchClient {
    async fn get_metric_data(
        &self,
        request: GetMetricDataRequest,
    ) -> Result<GetMetricDataResponse> {
        let body = serde_json::to_string(&request).map_err(|error| {
            alien_error::AlienError::new(ErrorData::InvalidClientConfig {
                message: format!("Failed to serialize GetMetricData request: {}", error),
                errors: None,
            })
        })?;

        self.send_json(
            GET_METRIC_DATA_TARGET,
            body,
            "GetMetricData",
            "cloudwatch metrics",
        )
        .await
    }

    async fn list_metrics(&self, request: ListMetricsRequest) -> Result<ListMetricsResponse> {
        let body = serde_json::to_string(&request).map_err(|error| {
            alien_error::AlienError::new(ErrorData::InvalidClientConfig {
                message: format!("Failed to serialize ListMetrics request: {}", error),
                errors: None,
            })
        })?;

        self.send_json(
            LIST_METRICS_TARGET,
            body,
            "ListMetrics",
            request.namespace.as_deref().unwrap_or("all namespaces"),
        )
        .await
    }
}

#[derive(Debug, Clone, Builder, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetMetricDataRequest {
    pub start_time: i64,
    pub end_time: i64,
    pub metric_data_queries: Vec<MetricDataQuery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_datapoints: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_by: Option<String>,
}

#[derive(Debug, Clone, Builder, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct MetricDataQuery {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric_stat: Option<MetricStat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_data: Option<bool>,
}

#[derive(Debug, Clone, Builder, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct MetricStat {
    pub metric: Metric,
    pub period: i32,
    pub stat: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Metric {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub dimensions: Vec<Dimension>,
}

#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Dimension {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Builder, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListMetricsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub dimensions: Vec<DimensionFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recently_active: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_linked_accounts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owning_account: Option<String>,
}

#[derive(Debug, Clone, Builder, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DimensionFilter {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetMetricDataResponse {
    #[serde(default)]
    pub metric_data_results: Vec<MetricDataResult>,
    #[serde(default)]
    pub messages: Vec<MessageData>,
    #[serde(default)]
    pub next_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MetricDataResult {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub status_code: Option<String>,
    #[serde(default)]
    pub timestamps: Vec<i64>,
    #[serde(default)]
    pub values: Vec<f64>,
    #[serde(default)]
    pub messages: Vec<MessageData>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MessageData {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListMetricsResponse {
    #[serde(default)]
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub next_token: Option<String>,
    #[serde(default)]
    pub owning_accounts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CloudWatchErrorResponse {
    #[serde(rename = "__type", alias = "Code")]
    code: String,
    #[serde(rename = "Message", alias = "message")]
    message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_metric_data_request_matches_aws_json_shape() {
        let request = GetMetricDataRequest::builder()
            .start_time(1_637_061_900)
            .end_time(1_637_074_500)
            .metric_data_queries(vec![MetricDataQuery::builder()
                .id("m1".to_string())
                .label("CPU".to_string())
                .metric_stat(
                    MetricStat::builder()
                        .metric(
                            Metric::builder()
                                .namespace("AWS/EC2".to_string())
                                .metric_name("CPUUtilization".to_string())
                                .dimensions(vec![Dimension::builder()
                                    .name("InstanceId".to_string())
                                    .value("i-123".to_string())
                                    .build()])
                                .build(),
                        )
                        .period(300)
                        .stat("Average".to_string())
                        .build(),
                )
                .return_data(true)
                .build()])
            .build();

        let encoded = serde_json::to_value(request).unwrap();

        assert_eq!(
            encoded,
            json!({
                "StartTime": 1637061900,
                "EndTime": 1637074500,
                "MetricDataQueries": [{
                    "Id": "m1",
                    "Label": "CPU",
                    "MetricStat": {
                        "Metric": {
                            "Namespace": "AWS/EC2",
                            "MetricName": "CPUUtilization",
                            "Dimensions": [{ "Name": "InstanceId", "Value": "i-123" }]
                        },
                        "Period": 300,
                        "Stat": "Average"
                    },
                    "ReturnData": true
                }]
            })
        );
    }

    #[test]
    fn list_metrics_request_matches_aws_json_shape() {
        let request = ListMetricsRequest::builder()
            .namespace("AWS/EC2".to_string())
            .dimensions(vec![DimensionFilter::builder()
                .name("InstanceId".to_string())
                .build()])
            .build();

        let encoded = serde_json::to_value(request).unwrap();

        assert_eq!(
            encoded,
            json!({
                "Namespace": "AWS/EC2",
                "Dimensions": [{ "Name": "InstanceId" }]
            })
        );
    }

    #[test]
    fn get_metric_data_response_parses_values() {
        let response: GetMetricDataResponse = serde_json::from_value(json!({
            "NextToken": "next",
            "MetricDataResults": [{
                "Id": "m1",
                "Label": "CPU",
                "StatusCode": "Complete",
                "Timestamps": [1637074200],
                "Values": [0.5]
            }]
        }))
        .unwrap();

        assert_eq!(response.next_token.as_deref(), Some("next"));
        assert_eq!(response.metric_data_results[0].id, "m1");
        assert_eq!(
            response.metric_data_results[0].timestamps,
            vec![1_637_074_200]
        );
        assert_eq!(response.metric_data_results[0].values, vec![0.5]);
    }

    #[test]
    fn list_metrics_response_parses_metrics() {
        let response: ListMetricsResponse = serde_json::from_value(json!({
            "Metrics": [{
                "Namespace": "AWS/EC2",
                "MetricName": "CPUUtilization",
                "Dimensions": [{ "Name": "InstanceId", "Value": "i-123" }]
            }],
            "OwningAccounts": ["111111111111"]
        }))
        .unwrap();

        assert_eq!(response.metrics[0].namespace.as_deref(), Some("AWS/EC2"));
        assert_eq!(
            response.metrics[0].metric_name.as_deref(),
            Some("CPUUtilization")
        );
        assert_eq!(response.owning_accounts, vec!["111111111111"]);
    }
}
