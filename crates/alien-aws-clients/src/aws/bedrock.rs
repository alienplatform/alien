//! AWS Bedrock control-plane client (model customization / fine-tuning).
//!
//! Hand-rolled reqwest + SigV4 client for the two model-customization-job REST
//! operations the fine-tuning controller needs. There is deliberately no
//! `aws-sdk-bedrock` dependency; this mirrors the other reqwest-based clients in
//! this crate (see `s3.rs` / `apigatewayv2.rs`) and keeps the dependency surface
//! small.
//!
//! Bedrock control plane host: `https://bedrock.{region}.amazonaws.com`
//! (SigV4 service signing name `bedrock`).
//! - CreateModelCustomizationJob: `POST /model-customization-jobs`
//! - GetModelCustomizationJob:    `GET  /model-customization-jobs/{jobIdentifier}`
//!
//! Request/response JSON field names follow the AWS Bedrock API reference
//! (CreateModelCustomizationJob / GetModelCustomizationJob, service version
//! 2023-04-20). Only the fields the controller uses are modeled; everything else
//! is ignored on deserialize.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use bon::Builder;
use form_urlencoded;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

// ---------------------------------------------------------------------------
// Bedrock API trait
// ---------------------------------------------------------------------------

/// Minimal Bedrock control-plane surface for model customization (fine-tuning).
#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait BedrockApi: Send + Sync + std::fmt::Debug {
    /// Submit a model customization (fine-tuning) job. Returns the created job's ARN,
    /// which is a valid `jobIdentifier` for [`get_model_customization_job`].
    async fn create_model_customization_job(
        &self,
        request: &CreateModelCustomizationJobRequest,
    ) -> Result<CreateModelCustomizationJobResponse>;

    /// Fetch a model customization job's current status and (once complete) the
    /// resulting custom-model ARN. `job_identifier` is the job ARN or job name.
    async fn get_model_customization_job(
        &self,
        job_identifier: &str,
    ) -> Result<GetModelCustomizationJobResponse>;
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BedrockClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl BedrockClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "bedrock".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    /// Host header value (always the real Bedrock host so the signature matches
    /// what AWS expects, even when the base URL is overridden for tests).
    fn host_header(&self) -> String {
        format!("bedrock.{}.amazonaws.com", self.credentials.region())
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("bedrock") {
            override_url.trim_end_matches('/').to_string()
        } else {
            format!("https://bedrock.{}.amazonaws.com", self.credentials.region())
        }
    }

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        method: Method,
        path: &str,
        body: Option<String>,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let url = format!("{}{}", self.get_base_url(), path);

        let mut builder = self
            .client
            .request(method, &url)
            .host(&self.host_header())
            .content_type_json();

        if let Some(body) = body {
            builder = builder.content_sha256(&body).body(body.clone());
            let result =
                crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;
            return Self::map_result(result, operation, resource, Some(&body));
        }

        builder = builder.content_sha256("");
        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;
        Self::map_result(result, operation, resource, None)
    }

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
                    if let Some(mapped) =
                        Self::map_bedrock_error(status, text, operation, resource, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Map a Bedrock error JSON body (`{"message": "...", "__type": "..."}`) to a
    /// structured client error. Bedrock control-plane errors use the standard AWS
    /// JSON error shape.
    fn map_bedrock_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<BedrockErrorResponse, _> = serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let code = e
                    .type_field
                    .or(e.code)
                    .unwrap_or_else(|| "UnknownError".into());
                // `__type` is often `com.amazonaws...#ValidationException`; keep the tail.
                let code = code.rsplit(['#', '.']).next().unwrap_or(&code).to_string();
                let message = e.message.unwrap_or_else(|| "Unknown error".into());
                (code, message)
            }
            Err(_) => return None,
        };

        Some(match code.as_str() {
            "AccessDeniedException" => ErrorData::RemoteAccessDenied {
                resource_type: "BedrockModelCustomizationJob".into(),
                resource_name: resource.into(),
            },
            "ResourceNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "BedrockModelCustomizationJob".into(),
                resource_name: resource.into(),
            },
            "ConflictException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "BedrockModelCustomizationJob".into(),
                resource_name: resource.into(),
            },
            "ThrottlingException" => ErrorData::RateLimitExceeded { message },
            "ServiceQuotaExceededException" => ErrorData::QuotaExceeded { message },
            "ValidationException" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "BedrockModelCustomizationJob".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "BedrockModelCustomizationJob".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("Bedrock operation failed: {}", message),
                    url: "bedrock.amazonaws.com".to_string(),
                    http_status: status.as_u16(),
                    http_response_text: Some(body.into()),
                    http_request_text: request_body.map(|s| s.to_string()),
                },
            },
        })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl BedrockApi for BedrockClient {
    async fn create_model_customization_job(
        &self,
        request: &CreateModelCustomizationJobRequest,
    ) -> Result<CreateModelCustomizationJobResponse> {
        let body = serde_json::to_string(request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize CreateModelCustomizationJobRequest".to_string(),
            },
        )?;
        self.send_json(
            Method::POST,
            "/model-customization-jobs",
            Some(body),
            "CreateModelCustomizationJob",
            &request.job_name,
        )
        .await
    }

    async fn get_model_customization_job(
        &self,
        job_identifier: &str,
    ) -> Result<GetModelCustomizationJobResponse> {
        // The identifier is a job ARN or name; it is a path segment and must be
        // percent-encoded (ARNs contain ':' and '/').
        let encoded = form_urlencoded::byte_serialize(job_identifier.as_bytes()).collect::<String>();
        let path = format!("/model-customization-jobs/{}", encoded);
        self.send_json(
            Method::GET,
            &path,
            None,
            "GetModelCustomizationJob",
            job_identifier,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Error struct
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BedrockErrorResponse {
    pub message: Option<String>,
    pub code: Option<String>,
    #[serde(rename = "__type")]
    pub type_field: Option<String>,
}

// ---------------------------------------------------------------------------
// Request / response types (subset of the Bedrock API)
// ---------------------------------------------------------------------------

/// S3 location for a job's input or output data.
/// Matches Bedrock's `TrainingDataConfig` / `OutputDataConfig` (`s3Uri`).
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct S3DataConfig {
    /// Fully-qualified S3 URI, e.g. `s3://bucket/key`.
    pub s3_uri: String,
}

/// Request body for CreateModelCustomizationJob.
/// Field names and casing follow the AWS Bedrock API reference; only the fields
/// the fine-tuning controller sets are modeled, and optional/None fields are
/// omitted so the wire shape stays minimal.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateModelCustomizationJobRequest {
    /// A name for the fine-tuning job.
    pub job_name: String,
    /// A name for the resulting custom model.
    pub custom_model_name: String,
    /// ARN of the IAM role Bedrock assumes to read training data / write output.
    pub role_arn: String,
    /// Base foundation-model identifier to tune.
    pub base_model_identifier: String,
    /// The customization type, e.g. `FINE_TUNING`.
    pub customization_type: String,
    /// Where the job reads its training dataset from.
    pub training_data_config: S3DataConfig,
    /// Where the job writes output/metrics.
    pub output_data_config: S3DataConfig,
    /// Optional tuning hyperparameters (string→string map).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyper_parameters: Option<HashMap<String, String>>,
}

/// Response body for CreateModelCustomizationJob.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateModelCustomizationJobResponse {
    /// ARN of the fine-tuning job (usable as `jobIdentifier` for polling).
    pub job_arn: String,
}

/// Terminal/non-terminal status of a model customization job, mirroring the
/// Bedrock `status` enum (`InProgress | Completed | Failed | Stopping | Stopped`).
/// An unrecognized value maps to [`Unknown`](ModelCustomizationJobStatus::Unknown)
/// so a future/unexpected status is surfaced rather than silently treated as terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCustomizationJobStatus {
    InProgress,
    Completed,
    Failed,
    Stopping,
    Stopped,
    /// Any status string Bedrock returns that we don't recognize.
    Unknown(String),
}

impl ModelCustomizationJobStatus {
    fn from_wire(raw: &str) -> Self {
        match raw {
            "InProgress" => Self::InProgress,
            "Completed" => Self::Completed,
            "Failed" => Self::Failed,
            "Stopping" => Self::Stopping,
            "Stopped" => Self::Stopped,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// True for a status the job will not move out of (success or failure).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }
}

/// Response body for GetModelCustomizationJob (subset).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetModelCustomizationJobResponse {
    /// Raw job status string from Bedrock. Prefer [`status`](Self::status) for the
    /// typed enum.
    pub status: String,
    /// ARN of the resulting custom model, present once the job completes. This is
    /// the artifact the OpenAI chat endpoint accepts for a custom model.
    #[serde(default)]
    pub output_model_arn: Option<String>,
    /// Name of the resulting custom model, present once the job completes.
    #[serde(default)]
    pub output_model_name: Option<String>,
    /// Human-readable reason the job failed, present when `status == Failed`.
    #[serde(default)]
    pub failure_message: Option<String>,
    /// ARN of the customization job itself.
    #[serde(default)]
    pub job_arn: Option<String>,
}

impl GetModelCustomizationJobResponse {
    /// The job's status as a typed enum.
    pub fn status(&self) -> ModelCustomizationJobStatus {
        ModelCustomizationJobStatus::from_wire(&self.status)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{AwsClientConfig, AwsCredentials, AwsServiceOverrides};
    use std::{
        collections::HashMap,
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };

    #[test]
    fn create_request_serializes_expected_json_shape() {
        let request = CreateModelCustomizationJobRequest::builder()
            .job_name("my-ai-job".to_string())
            .custom_model_name("my-ai-model".to_string())
            .role_arn("arn:aws:iam::123456789012:role/test-my-ai".to_string())
            .base_model_identifier("amazon.nova-lite-v1:0".to_string())
            .customization_type("FINE_TUNING".to_string())
            .training_data_config(
                S3DataConfig::builder()
                    .s3_uri("s3://test-bucket/training.jsonl".to_string())
                    .build(),
            )
            .output_data_config(
                S3DataConfig::builder()
                    .s3_uri("s3://test-bucket/output/my-ai/".to_string())
                    .build(),
            )
            .build();

        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["jobName"], "my-ai-job");
        assert_eq!(value["customModelName"], "my-ai-model");
        assert_eq!(value["roleArn"], "arn:aws:iam::123456789012:role/test-my-ai");
        assert_eq!(value["baseModelIdentifier"], "amazon.nova-lite-v1:0");
        assert_eq!(value["customizationType"], "FINE_TUNING");
        assert_eq!(
            value["trainingDataConfig"]["s3Uri"],
            "s3://test-bucket/training.jsonl"
        );
        assert_eq!(
            value["outputDataConfig"]["s3Uri"],
            "s3://test-bucket/output/my-ai/"
        );
        // Optional hyperParameters omitted when not set.
        assert!(value.get("hyperParameters").is_none());
    }

    #[test]
    fn get_response_maps_status_and_output_arn() {
        let body = r#"{
            "status": "Completed",
            "jobArn": "arn:aws:bedrock:us-east-1:123456789012:model-customization-job/amazon.nova-lite-v1:0/abcdef012345",
            "outputModelArn": "arn:aws:bedrock:us-east-1:123456789012:custom-model/amazon.nova-lite-v1:0/abcdef012345",
            "outputModelName": "my-ai-model"
        }"#;
        let parsed: GetModelCustomizationJobResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.status(), ModelCustomizationJobStatus::Completed);
        assert!(parsed.status().is_terminal());
        assert_eq!(
            parsed.output_model_arn.as_deref(),
            Some("arn:aws:bedrock:us-east-1:123456789012:custom-model/amazon.nova-lite-v1:0/abcdef012345")
        );

        let in_progress: GetModelCustomizationJobResponse =
            serde_json::from_str(r#"{"status":"InProgress"}"#).unwrap();
        assert_eq!(
            in_progress.status(),
            ModelCustomizationJobStatus::InProgress
        );
        assert!(!in_progress.status().is_terminal());
        assert!(in_progress.output_model_arn.is_none());
    }

    /// Read one full HTTP request (headers + body) off a socket, using the
    /// Content-Length header to know when the body is complete.
    fn read_http_request(stream: &mut std::net::TcpStream) -> (String, String) {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).expect("read request");
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header_end = end + 4;
                let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|v| v.trim().parse().ok())
                    })
                    .unwrap_or(0);
                if bytes.len() >= header_end + length {
                    let body =
                        String::from_utf8_lossy(&bytes[header_end..header_end + length]).to_string();
                    return (headers, body);
                }
            }
        }
        (String::from_utf8_lossy(&bytes).to_string(), String::new())
    }

    fn test_config(endpoint: String) -> AwsClientConfig {
        AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access".to_string(),
                secret_access_key: "test-secret".to_string(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("bedrock".to_string(), endpoint)]),
            }),
        }
    }

    #[tokio::test]
    async fn create_model_customization_job_signs_and_sends_expected_request() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
        let captured: Arc<Mutex<(String, String)>> =
            Arc::new(Mutex::new((String::new(), String::new())));
        let sink = captured.clone();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let (headers, body) = read_http_request(&mut stream);
            *sink.lock().expect("lock") = (headers, body);
            let resp_body = r#"{"jobArn":"arn:aws:bedrock:us-east-1:123456789012:model-customization-job/amazon.nova-lite-v1:0/abcdef012345"}"#;
            write!(
                stream,
                "HTTP/1.1 201 Created\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                resp_body.len(),
                resp_body
            )
            .expect("write response");
        });

        let client =
            BedrockClient::new(Client::new(), AwsCredentialProvider::from_config_sync(test_config(endpoint)));
        let request = CreateModelCustomizationJobRequest::builder()
            .job_name("my-ai-job".to_string())
            .custom_model_name("my-ai-model".to_string())
            .role_arn("arn:aws:iam::123456789012:role/test-my-ai".to_string())
            .base_model_identifier("amazon.nova-lite-v1:0".to_string())
            .customization_type("FINE_TUNING".to_string())
            .training_data_config(
                S3DataConfig::builder()
                    .s3_uri("s3://test-bucket/training.jsonl".to_string())
                    .build(),
            )
            .output_data_config(
                S3DataConfig::builder()
                    .s3_uri("s3://test-bucket/output/my-ai/".to_string())
                    .build(),
            )
            .build();

        let response = client
            .create_model_customization_job(&request)
            .await
            .expect("request should succeed");
        server.join().expect("server should finish");

        assert_eq!(
            response.job_arn,
            "arn:aws:bedrock:us-east-1:123456789012:model-customization-job/amazon.nova-lite-v1:0/abcdef012345"
        );

        let (headers, body) = captured.lock().expect("lock").clone();
        let request_line = headers.lines().next().unwrap_or_default();
        // Method + path.
        assert!(
            request_line.starts_with("POST /model-customization-jobs "),
            "unexpected request line: {request_line}"
        );
        // SigV4 signing headers must be present and name the bedrock service.
        let lower = headers.to_ascii_lowercase();
        assert!(
            lower.contains("authorization: aws4-hmac-sha256"),
            "missing SigV4 Authorization header:\n{headers}"
        );
        assert!(
            lower.contains("credential=test-access/")
                && lower.contains("/us-east-1/bedrock/aws4_request"),
            "Authorization must scope the signature to us-east-1/bedrock:\n{headers}"
        );
        assert!(lower.contains("x-amz-date:"), "missing x-amz-date header");
        assert!(
            lower.contains("x-amz-content-sha256:"),
            "missing x-amz-content-sha256 header"
        );
        assert!(
            lower.contains("host: bedrock.us-east-1.amazonaws.com"),
            "host header must be the real bedrock host for a valid signature:\n{headers}"
        );

        // Body must carry the exact JSON the API expects.
        let json: serde_json::Value = serde_json::from_str(&body).expect("body is json");
        assert_eq!(json["jobName"], "my-ai-job");
        assert_eq!(json["customModelName"], "my-ai-model");
        assert_eq!(json["baseModelIdentifier"], "amazon.nova-lite-v1:0");
        assert_eq!(json["customizationType"], "FINE_TUNING");
        assert_eq!(
            json["trainingDataConfig"]["s3Uri"],
            "s3://test-bucket/training.jsonl"
        );
        assert_eq!(json["roleArn"], "arn:aws:iam::123456789012:role/test-my-ai");
    }

    #[tokio::test]
    async fn get_model_customization_job_uses_get_and_encoded_path() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
        let captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let sink = captured.clone();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let (headers, _body) = read_http_request(&mut stream);
            *sink.lock().expect("lock") = headers;
            let resp_body = r#"{"status":"Completed","outputModelArn":"arn:aws:bedrock:us-east-1:123456789012:custom-model/amazon.nova-lite-v1:0/abcdef012345"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                resp_body.len(),
                resp_body
            )
            .expect("write response");
        });

        let client =
            BedrockClient::new(Client::new(), AwsCredentialProvider::from_config_sync(test_config(endpoint)));
        let job_arn =
            "arn:aws:bedrock:us-east-1:123456789012:model-customization-job/amazon.nova-lite-v1:0/abcdef012345";
        let response = client
            .get_model_customization_job(job_arn)
            .await
            .expect("request should succeed");
        server.join().expect("server should finish");

        assert_eq!(response.status(), ModelCustomizationJobStatus::Completed);
        assert_eq!(
            response.output_model_arn.as_deref(),
            Some("arn:aws:bedrock:us-east-1:123456789012:custom-model/amazon.nova-lite-v1:0/abcdef012345")
        );

        let headers = captured.lock().expect("lock").clone();
        let request_line = headers.lines().next().unwrap_or_default();
        assert!(
            request_line.starts_with("GET /model-customization-jobs/"),
            "unexpected request line: {request_line}"
        );
        // ARN separators must be percent-encoded in the path segment.
        assert!(
            request_line.contains("%3A") && request_line.contains("%2F"),
            "job identifier must be percent-encoded in the path: {request_line}"
        );
    }
}
