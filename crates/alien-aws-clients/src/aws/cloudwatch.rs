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

    async fn send_query<T: DeserializeOwned + Send + 'static>(
        &self,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        let value: serde_json::Value = serde_json::from_str(&body).map_err(|error| {
            alien_error::AlienError::new(ErrorData::InvalidClientConfig {
                message: error.to_string(),
                errors: None,
            })
        })?;
        let mut params = vec![
            ("Action".to_string(), operation.to_string()),
            ("Version".to_string(), "2010-08-01".to_string()),
        ];
        query_parameters("", &value, &mut params);
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(params)
            .finish();
        self.credentials.ensure_fresh().await?;
        let url = format!("{}/", self.get_base_url().trim_end_matches('/'));

        let builder = self
            .client
            .post(&url)
            .host(&self.host())
            .content_type_form()
            .content_sha256(&body)
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await;

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
        let parsed: Option<CloudWatchErrorResponse> =
            serde_json::from_str(body).ok().or_else(|| {
                quick_xml::de::from_str::<QueryErrorEnvelope>(body)
                    .ok()
                    .map(|response| response.error)
            });
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

        let response: MetricDataEnvelope = self
            .send_query(body, "GetMetricData", "cloudwatch metrics")
            .await?;
        response.result.into_response()
    }

    async fn list_metrics(&self, request: ListMetricsRequest) -> Result<ListMetricsResponse> {
        let body = serde_json::to_string(&request).map_err(|error| {
            alien_error::AlienError::new(ErrorData::InvalidClientConfig {
                message: format!("Failed to serialize ListMetrics request: {}", error),
                errors: None,
            })
        })?;

        let response: ListMetricsEnvelope = self
            .send_query(
                body,
                "ListMetrics",
                request.namespace.as_deref().unwrap_or("all namespaces"),
            )
            .await?;
        Ok(ListMetricsResponse {
            metrics: response
                .result
                .metrics
                .map(|list| list.items)
                .unwrap_or_default()
                .into_iter()
                .map(|metric| Metric {
                    namespace: metric.namespace,
                    metric_name: metric.metric_name,
                    dimensions: metric.dimensions.map(|list| list.items).unwrap_or_default(),
                })
                .collect(),
            next_token: response.result.next_token,
            owning_accounts: response
                .result
                .owning_accounts
                .map(|list| list.items)
                .unwrap_or_default(),
        })
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

#[derive(Deserialize)]
struct QueryErrorEnvelope {
    #[serde(rename = "Error")]
    error: CloudWatchErrorResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CloudWatchErrorResponse {
    #[serde(rename = "__type", alias = "Code")]
    code: String,
    #[serde(rename = "Message", alias = "message")]
    message: Option<String>,
}

/// CloudWatch uses the AWS Query protocol, including member-indexed lists.
fn query_parameters(prefix: &str, value: &serde_json::Value, params: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, value) in fields {
                let key = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                query_parameters(&key, value, params);
            }
        }
        serde_json::Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                query_parameters(&format!("{prefix}.member.{}", index + 1), value, params);
            }
        }
        serde_json::Value::Null => {}
        serde_json::Value::Number(number) if prefix == "StartTime" || prefix == "EndTime" => {
            let time = number
                .as_i64()
                .and_then(|time| chrono::DateTime::from_timestamp(time, 0))
                .map(|time| time.to_rfc3339())
                .unwrap_or_else(|| number.to_string());
            params.push((prefix.into(), time));
        }
        serde_json::Value::String(value) => params.push((prefix.into(), value.clone())),
        _ => params.push((prefix.into(), value.to_string())),
    }
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct Members<T> {
    #[serde(rename = "member", default)]
    items: Vec<T>,
}

#[derive(Deserialize)]
struct MetricDataEnvelope {
    #[serde(rename = "GetMetricDataResult")]
    result: MetricDataWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MetricDataWire {
    metric_data_results: Option<Members<MetricResultWire>>,
    messages: Option<Members<MessageData>>,
    next_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MetricResultWire {
    id: String,
    label: Option<String>,
    status_code: Option<String>,
    timestamps: Option<Members<String>>,
    values: Option<Members<f64>>,
    messages: Option<Members<MessageData>>,
}

impl MetricDataWire {
    fn into_response(self) -> Result<GetMetricDataResponse> {
        let mut results = Vec::new();
        for result in self
            .metric_data_results
            .map(|list| list.items)
            .unwrap_or_default()
        {
            let mut timestamps = Vec::new();
            for timestamp in result.timestamps.map(|list| list.items).unwrap_or_default() {
                let parsed = chrono::DateTime::parse_from_rfc3339(&timestamp).map_err(|error| {
                    alien_error::AlienError::new(ErrorData::InvalidClientConfig {
                        message: format!("Invalid CloudWatch timestamp: {error}"),
                        errors: None,
                    })
                })?;
                timestamps.push(parsed.timestamp());
            }
            results.push(MetricDataResult {
                id: result.id,
                label: result.label,
                status_code: result.status_code,
                timestamps,
                values: result.values.map(|list| list.items).unwrap_or_default(),
                messages: result.messages.map(|list| list.items).unwrap_or_default(),
            });
        }
        Ok(GetMetricDataResponse {
            metric_data_results: results,
            messages: self.messages.map(|list| list.items).unwrap_or_default(),
            next_token: self.next_token,
        })
    }
}

#[derive(Deserialize)]
struct ListMetricsEnvelope {
    #[serde(rename = "ListMetricsResult")]
    result: ListMetricsWire,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ListMetricsWire {
    metrics: Option<Members<MetricWire>>,
    next_token: Option<String>,
    owning_accounts: Option<Members<String>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MetricWire {
    namespace: Option<String>,
    metric_name: Option<String>,
    dimensions: Option<Members<Dimension>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{AwsClientConfig, AwsCredentials, AwsServiceOverrides};
    use httpmock::{prelude::*, MockServer};
    use std::collections::HashMap;

    async fn client(server: &MockServer) -> CloudWatchClient {
        let credentials = AwsCredentialProvider::from_config(AwsClientConfig {
            account_id: "000000000000".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test".into(),
                secret_access_key: "test".into(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("monitoring".into(), server.base_url())]),
            }),
        })
        .await
        .unwrap();
        CloudWatchClient::new(Client::new(), credentials)
    }

    #[test]
    fn query_errors_preserve_provider_code_and_message() {
        let mapped = CloudWatchClient::map_error(StatusCode::BAD_REQUEST,
            "<ErrorResponse><Error><Code>InvalidNextToken</Code><Message>Expired next token</Message></Error></ErrorResponse>",
            "ListMetrics", "metrics", None).unwrap();
        assert!(
            matches!(mapped, ErrorData::InvalidClientConfig { message, .. } if message == "Expired next token")
        );
    }

    #[tokio::test]
    async fn get_metric_data_uses_query_protocol_and_decodes_real_xml_shape() {
        let server = MockServer::start_async().await;
        let mock = server.mock_async(|when, then| {
            when.method(POST).path("/").header("content-type", "application/x-www-form-urlencoded")
                .body_contains("Action=GetMetricData").body_contains("Version=2010-08-01")
                .body_contains("MetricDataQueries.member.1.MetricStat.Metric.Dimensions.member.1.Name=Class")
                .body_contains("StartTime=2026-10-13T00%3A00%3A00%2B00%3A00");
            then.status(200).body(r#"<GetMetricDataResponse xmlns="http://monitoring.amazonaws.com/doc/2010-08-01/"><GetMetricDataResult>
                <MetricDataResults><member><Id>usage</Id><Label>ResourceCount</Label><StatusCode>Complete</StatusCode>
                <Timestamps><member>2026-10-13T00:00:00Z</member></Timestamps><Values><member>12.0</member></Values>
                <Messages><member><Code>Notice</Code><Value>test message</Value></member></Messages></member></MetricDataResults>
                <NextToken>next-page</NextToken></GetMetricDataResult><ResponseMetadata><RequestId>test</RequestId></ResponseMetadata></GetMetricDataResponse>"#);
        }).await;
        let timestamp = chrono::DateTime::parse_from_rfc3339("2026-10-13T00:00:00Z")
            .unwrap()
            .timestamp();
        let response = client(&server)
            .await
            .get_metric_data(GetMetricDataRequest {
                start_time: timestamp,
                end_time: timestamp + 60,
                metric_data_queries: vec![MetricDataQuery {
                    id: "usage".into(),
                    expression: None,
                    label: None,
                    period: None,
                    return_data: Some(true),
                    metric_stat: Some(MetricStat {
                        metric: Metric {
                            namespace: Some("AWS/Usage".into()),
                            metric_name: Some("ResourceCount".into()),
                            dimensions: vec![Dimension {
                                name: "Class".into(),
                                value: "Standard/OnDemand".into(),
                            }],
                        },
                        period: 60,
                        stat: "Maximum".into(),
                        unit: None,
                    }),
                }],
                max_datapoints: None,
                next_token: None,
                scan_by: Some("TimestampDescending".into()),
            })
            .await
            .unwrap();
        assert_eq!(response.next_token.as_deref(), Some("next-page"));
        assert_eq!(response.metric_data_results.len(), 1);
        let result = &response.metric_data_results[0];
        assert_eq!(result.id, "usage");
        assert_eq!(result.timestamps, vec![timestamp]);
        assert_eq!(result.values, vec![12.0]);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].code.as_deref(), Some("Notice"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn list_metrics_uses_member_filters_and_decodes_dimensions_and_pagination() {
        let server = MockServer::start_async().await;
        let mock = server.mock_async(|when, then| {
            when.method(POST).path("/").body_contains("Action=ListMetrics")
                .body_contains("Dimensions.member.1.Name=Service")
                .body_contains("Dimensions.member.2.Name=Class");
            then.status(200).body(r#"<ListMetricsResponse><ListMetricsResult>
                <Metrics><member><Namespace>AWS/Usage</Namespace><MetricName>ResourceCount</MetricName>
                <Dimensions><member><Name>Service</Name><Value>EC2</Value></member>
                <member><Name>Class</Name><Value>Standard/OnDemand</Value></member></Dimensions></member>
                <member><Namespace>AWS/EC2</Namespace><MetricName>CPUUtilization</MetricName><Dimensions/></member></Metrics>
                <NextToken>next-page</NextToken><OwningAccounts><member>000000000000</member><member>111111111111</member></OwningAccounts>
                </ListMetricsResult></ListMetricsResponse>"#);
        }).await;
        let response = client(&server)
            .await
            .list_metrics(ListMetricsRequest {
                namespace: Some("AWS/Usage".into()),
                metric_name: None,
                dimensions: vec![
                    DimensionFilter {
                        name: "Service".into(),
                        value: Some("EC2".into()),
                    },
                    DimensionFilter {
                        name: "Class".into(),
                        value: Some("Standard/OnDemand".into()),
                    },
                ],
                next_token: None,
                recently_active: None,
                include_linked_accounts: None,
                owning_account: None,
            })
            .await
            .unwrap();
        assert_eq!(response.next_token.as_deref(), Some("next-page"));
        assert_eq!(response.metrics.len(), 2);
        assert_eq!(response.metrics[0].namespace.as_deref(), Some("AWS/Usage"));
        assert_eq!(response.metrics[0].dimensions.len(), 2);
        assert_eq!(response.metrics[0].dimensions[0].value, "EC2");
        assert_eq!(response.metrics[0].dimensions[1].name, "Class");
        assert_eq!(response.metrics[0].dimensions[1].value, "Standard/OnDemand");
        assert_eq!(
            response.metrics[1].metric_name.as_deref(),
            Some("CPUUtilization")
        );
        assert!(response.metrics[1].dimensions.is_empty());
        assert_eq!(
            response.owning_accounts,
            vec!["000000000000", "111111111111"]
        );
        mock.assert_async().await;
    }
    #[tokio::test]
    async fn query_lists_preserve_multiple_queries_samples_and_empty_series() {
        let server = MockServer::start_async().await;
        let mock = server.mock_async(|when, then| {
            when.method(POST).path("/")
                .body_contains("MetricDataQueries.member.1.Id=first")
                .body_contains("MetricDataQueries.member.2.Id=empty")
                .body_contains("MetricDataQueries.member.2.Expression=1%2B1");
            then.status(200).body(r#"<GetMetricDataResponse><GetMetricDataResult><MetricDataResults>
                <member><Id>first</Id><StatusCode>Complete</StatusCode><Timestamps>
                <member>2026-10-13T00:00:00Z</member><member>2026-10-13T00:01:00Z</member>
                </Timestamps><Values><member>12.0</member><member>13.0</member></Values></member>
                <member><Id>empty</Id><StatusCode>Complete</StatusCode><Timestamps/><Values/></member>
                </MetricDataResults></GetMetricDataResult></GetMetricDataResponse>"#);
        }).await;
        let timestamp = chrono::DateTime::parse_from_rfc3339("2026-10-13T00:00:00Z")
            .unwrap()
            .timestamp();
        let queries = ["first", "empty"]
            .into_iter()
            .map(|id| MetricDataQuery {
                id: id.into(),
                expression: Some("1+1".into()),
                label: None,
                period: None,
                return_data: Some(true),
                metric_stat: None,
            })
            .collect();
        let response = client(&server)
            .await
            .get_metric_data(GetMetricDataRequest {
                start_time: timestamp,
                end_time: timestamp + 120,
                metric_data_queries: queries,
                max_datapoints: None,
                next_token: None,
                scan_by: None,
            })
            .await
            .unwrap();
        assert_eq!(response.metric_data_results.len(), 2);
        assert_eq!(response.metric_data_results[0].id, "first");
        assert_eq!(
            response.metric_data_results[0].timestamps,
            vec![timestamp, timestamp + 60]
        );
        assert_eq!(response.metric_data_results[0].values, vec![12.0, 13.0]);
        assert_eq!(response.metric_data_results[1].id, "empty");
        assert_eq!(
            response.metric_data_results[1].status_code.as_deref(),
            Some("Complete")
        );
        assert!(response.metric_data_results[1].timestamps.is_empty());
        assert!(response.metric_data_results[1].values.is_empty());
        mock.assert_async().await;
    }
}
