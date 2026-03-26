use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsRequestSigner, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::RequestBuilderExt;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, ContextError, IntoAlienError};
use bon::Builder;
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CloudWatchLogsApi: Send + Sync + std::fmt::Debug {
    async fn create_log_group(&self, request: CreateLogGroupRequest) -> Result<()>;
    async fn delete_log_group(&self, log_group_name: &str) -> Result<()>;
    async fn create_log_stream(&self, request: CreateLogStreamRequest) -> Result<()>;
    async fn delete_log_stream(&self, log_group_name: &str, log_stream_name: &str) -> Result<()>;
    async fn put_log_events(&self, request: PutLogEventsRequest) -> Result<PutLogEventsResponse>;
    async fn get_log_events(&self, request: GetLogEventsRequest) -> Result<GetLogEventsResponse>;
}

// ---------------------------------------------------------------------------
// CloudWatch Logs client using new request helpers.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct CloudWatchLogsClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl CloudWatchLogsClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "logs".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("logs") {
            override_url.to_string()
        } else {
            format!("https://logs.{}.amazonaws.com", self.credentials.region())
        }
    }

    // ------------------------- internal helpers -------------------------

    fn map_result<T>(
        result: Result<T>,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Result<T> {
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(ref text),
                    ..
                }) = &e.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) = Self::map_cloudwatch_logs_error(
                        status,
                        text,
                        operation,
                        resource,
                        request_body,
                    ) {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse CloudWatch Logs error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_cloudwatch_logs_error(
        status: StatusCode,
        body: &str,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<CloudWatchLogsErrorResponse, _> =
            serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let c = e
                    .type_field_underscore
                    .or(e.type_field)
                    .unwrap_or_else(|| "UnknownErrorCode".into());
                let m = e
                    .message
                    .or(e.message_capital)
                    .unwrap_or_else(|| "Unknown error".into());
                (c, m)
            }
            Err(_) => {
                // If we can't parse the response, return None to use original error
                return None;
            }
        };

        Some(match code.as_str() {
            // Access / auth
            "AccessDeniedException"
            | "NotAuthorized"
            | "UnrecognizedClientException"
            | "ExpiredTokenException" => ErrorData::RemoteAccessDenied {
                resource_type: "LogGroup".into(),
                resource_name: resource.into(),
            },
            // Throttling
            "ThrottlingException" | "TooManyRequestsException" => {
                ErrorData::RateLimitExceeded { message }
            }
            // Service unavailable
            "ServiceUnavailable" | "InternalFailure" | "ServiceException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Timeout
            "RequestTimeoutException" => ErrorData::Timeout { message },
            // Resource not found
            "ResourceNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "LogGroup".into(),
                resource_name: resource.into(),
            },
            // Conflict / already exists
            "ResourceAlreadyExistsException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "LogGroup".into(),
                resource_name: resource.into(),
            },
            // Invalid parameter
            "InvalidParameterException" => ErrorData::GenericError { message },
            // Operation aborted
            "OperationAbortedException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "LogGroup".into(),
                resource_name: resource.into(),
            },
            // Limit exceeded
            "LimitExceededException" => ErrorData::RateLimitExceeded { message },
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "LogGroup".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "LogGroup".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "LogGroup".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("CloudWatch Logs operation failed: {}", message),
                    url: format!("logs.amazonaws.com"),
                    http_status: status.as_u16(),
                    http_response_text: Some(body.into()),
                    http_request_text: request_body.map(|s| s.to_string()),
                },
            },
        })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudWatchLogsApi for CloudWatchLogsClient {
    async fn create_log_group(&self, request: CreateLogGroupRequest) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateLogGroupRequest for log group '{}'",
                    request.log_group_name
                ),
            },
        )?;

        let builder = self
            .client
            .request(Method::POST, &self.get_base_url())
            .host(&format!("logs.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", "Logs_20140328.CreateLogGroup")
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result = builder
            .sign_aws_request(&self.sign_config())?
            .with_retry()
            .send_no_response()
            .await;
        Self::map_result(
            result,
            "CreateLogGroup",
            &request.log_group_name,
            Some(&body),
        )
    }

    async fn delete_log_group(&self, log_group_name: &str) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let request = DeleteLogGroupRequest {
            log_group_name: log_group_name.to_string(),
        };
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DeleteLogGroupRequest for log group '{}'",
                    log_group_name
                ),
            },
        )?;

        let builder = self
            .client
            .request(Method::POST, &self.get_base_url())
            .host(&format!("logs.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", "Logs_20140328.DeleteLogGroup")
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result = builder
            .sign_aws_request(&self.sign_config())?
            .with_retry()
            .send_no_response()
            .await;
        Self::map_result(result, "DeleteLogGroup", log_group_name, Some(&body))
    }

    async fn create_log_stream(&self, request: CreateLogStreamRequest) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateLogStreamRequest for log stream '{}' in group '{}'",
                    request.log_stream_name, request.log_group_name
                ),
            },
        )?;

        let builder = self
            .client
            .request(Method::POST, &self.get_base_url())
            .host(&format!("logs.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", "Logs_20140328.CreateLogStream")
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result = builder
            .sign_aws_request(&self.sign_config())?
            .with_retry()
            .send_no_response()
            .await;
        Self::map_result(
            result,
            "CreateLogStream",
            &request.log_stream_name,
            Some(&body),
        )
    }

    async fn delete_log_stream(&self, log_group_name: &str, log_stream_name: &str) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let request = DeleteLogStreamRequest {
            log_group_name: log_group_name.to_string(),
            log_stream_name: log_stream_name.to_string(),
        };
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DeleteLogStreamRequest for log stream '{}' in group '{}'",
                    log_stream_name, log_group_name
                ),
            },
        )?;

        let builder = self
            .client
            .request(Method::POST, &self.get_base_url())
            .host(&format!("logs.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", "Logs_20140328.DeleteLogStream")
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result = builder
            .sign_aws_request(&self.sign_config())?
            .with_retry()
            .send_no_response()
            .await;
        Self::map_result(result, "DeleteLogStream", log_stream_name, Some(&body))
    }

    async fn put_log_events(&self, request: PutLogEventsRequest) -> Result<PutLogEventsResponse> {
        self.credentials.ensure_fresh().await?;
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize PutLogEventsRequest for log stream '{}' in group '{}'",
                    request.log_stream_name, request.log_group_name
                ),
            },
        )?;

        let builder = self
            .client
            .request(Method::POST, &self.get_base_url())
            .host(&format!("logs.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", "Logs_20140328.PutLogEvents")
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result = builder
            .sign_aws_request(&self.sign_config())?
            .with_retry()
            .send_json::<PutLogEventsResponse>()
            .await;
        Self::map_result(
            result,
            "PutLogEvents",
            &request.log_stream_name,
            Some(&body),
        )
    }

    async fn get_log_events(&self, request: GetLogEventsRequest) -> Result<GetLogEventsResponse> {
        self.credentials.ensure_fresh().await?;
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize GetLogEventsRequest for log stream '{}' in group '{}'",
                    request.log_stream_name, request.log_group_name
                ),
            },
        )?;

        let builder = self
            .client
            .request(Method::POST, &self.get_base_url())
            .host(&format!("logs.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", "Logs_20140328.GetLogEvents")
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result = builder
            .sign_aws_request(&self.sign_config())?
            .with_retry()
            .send_json::<GetLogEventsResponse>()
            .await;
        Self::map_result(
            result,
            "GetLogEvents",
            &request.log_stream_name,
            Some(&body),
        )
    }
}

// ---------------------------------------------------------------------------
// Error JSON mapping structs
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
struct CloudWatchLogsErrorResponse {
    #[serde(rename = "Type")]
    type_field: Option<String>,
    #[serde(rename = "__type")]
    type_field_underscore: Option<String>,
    #[serde(rename = "message")]
    message: Option<String>,
    #[serde(rename = "Message")]
    message_capital: Option<String>,
    #[serde(rename = "Error")]
    error: Option<CloudWatchLogsErrorDetails>,
}

#[derive(Debug, Deserialize)]
struct CloudWatchLogsErrorDetails {
    #[serde(rename = "Code")]
    code: Option<String>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Request / response payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateLogGroupRequest {
    pub log_group_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_group_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteLogGroupRequest {
    pub log_group_name: String,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateLogStreamRequest {
    pub log_group_name: String,
    pub log_stream_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteLogStreamRequest {
    pub log_group_name: String,
    pub log_stream_name: String,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PutLogEventsRequest {
    pub log_group_name: String,
    pub log_stream_name: String,
    pub log_events: Vec<InputLogEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InputLogEvent {
    pub timestamp: i64,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutLogEventsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_sequence_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_log_events_info: Option<RejectedLogEventsInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectedLogEventsInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub too_new_log_event_start_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub too_old_log_event_end_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expired_log_event_end_index: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GetLogEventsRequest {
    pub log_group_name: String,
    pub log_stream_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_from_head: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLogEventsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<OutputLogEvent>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_forward_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_backward_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputLogEvent {
    pub timestamp: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingestion_time: Option<i64>,
}
