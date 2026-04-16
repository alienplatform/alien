use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, ContextError, IntoAlienError};
use bon::Builder;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait EventBridgeApi: Send + Sync + std::fmt::Debug {
    async fn put_rule(&self, request: PutRuleRequest) -> Result<PutRuleResponse>;
    async fn put_targets(&self, request: PutTargetsRequest) -> Result<()>;
    async fn remove_targets(&self, rule_name: &str, target_ids: Vec<String>) -> Result<()>;
    async fn delete_rule(&self, rule_name: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// EventBridge client using AWS JSON 1.1 protocol.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct EventBridgeClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl EventBridgeClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "events".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("events") {
            override_url.to_string()
        } else {
            format!(
                "https://events.{}.amazonaws.com",
                self.credentials.region()
            )
        }
    }

    // ------------------------- internal helpers -------------------------

    async fn send_json<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &str,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));

        let builder = self
            .client
            .post(&url)
            .host(&format!(
                "events.{}.amazonaws.com",
                self.credentials.region()
            ))
            .header("X-Amz-Target", target)
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(&body))
    }

    async fn send_json_no_response(
        &self,
        target: &str,
        body: String,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));

        let builder = self
            .client
            .post(&url)
            .host(&format!(
                "events.{}.amazonaws.com",
                self.credentials.region()
            ))
            .header("X-Amz-Target", target)
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, operation, resource, Some(&body))
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
                        Self::map_eventbridge_error(status, text, operation, resource, request_body)
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

    fn map_eventbridge_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<EventBridgeErrorResponse, _> =
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
            Err(_) => return None,
        };

        Some(match code.as_str() {
            // Access / auth
            "AccessDeniedException" | "UnrecognizedClientException" | "ExpiredTokenException" => {
                ErrorData::RemoteAccessDenied {
                    resource_type: "Rule".into(),
                    resource_name: resource.into(),
                }
            }
            // Throttling
            "ThrottlingException" | "LimitExceededException" => {
                ErrorData::RateLimitExceeded { message }
            }
            // Service unavailable
            "InternalException" | "ServiceUnavailableException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Resource not found
            "ResourceNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "Rule".into(),
                resource_name: resource.into(),
            },
            // Resource already exists
            "ResourceAlreadyExistsException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "Rule".into(),
                resource_name: resource.into(),
            },
            // Invalid input
            "ValidationException" | "InvalidEventPatternException" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "Rule".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "Rule".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "Rule".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("EventBridge operation failed: {}", message),
                    url: "events.amazonaws.com".into(),
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
impl EventBridgeApi for EventBridgeClient {
    async fn put_rule(&self, request: PutRuleRequest) -> Result<PutRuleResponse> {
        let body = serde_json::to_string(&request)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                message: "Failed to serialize PutRule request".into(),
                field_name: None,
            })?;

        self.send_json("AWSEvents.PutRule", body, "PutRule", &request.name)
            .await
    }

    async fn put_targets(&self, request: PutTargetsRequest) -> Result<()> {
        let body = serde_json::to_string(&request)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                message: "Failed to serialize PutTargets request".into(),
                field_name: None,
            })?;

        self.send_json_no_response("AWSEvents.PutTargets", body, "PutTargets", &request.rule)
            .await
    }

    async fn remove_targets(&self, rule_name: &str, target_ids: Vec<String>) -> Result<()> {
        let body = serde_json::to_string(&RemoveTargetsRequest {
            rule: rule_name.to_string(),
            ids: target_ids,
        })
        .into_alien_error()
        .context(ErrorData::InvalidInput {
            message: "Failed to serialize RemoveTargets request".into(),
            field_name: None,
        })?;

        self.send_json_no_response("AWSEvents.RemoveTargets", body, "RemoveTargets", rule_name)
            .await
    }

    async fn delete_rule(&self, rule_name: &str) -> Result<()> {
        let body = serde_json::to_string(&DeleteRuleRequest {
            name: rule_name.to_string(),
        })
        .into_alien_error()
        .context(ErrorData::InvalidInput {
            message: "Failed to serialize DeleteRule request".into(),
            field_name: None,
        })?;

        self.send_json_no_response("AWSEvents.DeleteRule", body, "DeleteRule", rule_name)
            .await
    }
}

// ---------------------------------------------------------------------------
// Request / response payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct PutRuleRequest {
    pub name: String,
    pub schedule_expression: String,
    pub state: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutRuleResponse {
    pub rule_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutTargetsRequest {
    pub rule: String,
    pub targets: Vec<EventBridgeTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EventBridgeTarget {
    pub id: String,
    pub arn: String,
}

// Internal request types (not exposed publicly)

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct RemoveTargetsRequest {
    pub rule: String,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct DeleteRuleRequest {
    pub name: String,
}

// ---------------------------------------------------------------------------
// Error JSON mapping structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EventBridgeErrorResponse {
    #[serde(rename = "Type")]
    type_field: Option<String>,
    #[serde(rename = "__type")]
    type_field_underscore: Option<String>,
    #[serde(rename = "message")]
    message: Option<String>,
    #[serde(rename = "Message")]
    message_capital: Option<String>,
}
