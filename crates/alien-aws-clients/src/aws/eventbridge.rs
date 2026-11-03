use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, ContextError, IntoAlienError};
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
            format!("https://events.{}.amazonaws.com", self.credentials.region())
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
        let parsed: std::result::Result<EventBridgeErrorResponse, _> = serde_json::from_str(body);
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
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::InvalidInput {
                message: "Failed to serialize PutRule request".into(),
                field_name: None,
            },
        )?;

        self.send_json("AWSEvents.PutRule", body, "PutRule", &request.name)
            .await
    }

    async fn put_targets(&self, request: PutTargetsRequest) -> Result<()> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::InvalidInput {
                message: "Failed to serialize PutTargets request".into(),
                field_name: None,
            },
        )?;

        let response: TargetsResponse = self
            .send_json("AWSEvents.PutTargets", body, "PutTargets", &request.rule)
            .await?;
        response.into_result("PutTargets", &request.rule)
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

        let response: TargetsResponse = self
            .send_json("AWSEvents.RemoveTargets", body, "RemoveTargets", rule_name)
            .await?;
        response.into_result("RemoveTargets", rule_name)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<EventBridgeTag>>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EventBridgeTag {
    pub key: String,
    pub value: String,
}

// Internal request types (not exposed publicly)

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
struct RemoveTargetsRequest {
    pub rule: String,
    pub ids: Vec<String>,
}

/// The body of a PutTargets or RemoveTargets response. AWS answers HTTP 200 when some or all
/// targets failed, and lists them here.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TargetsResponse {
    #[serde(default)]
    failed_entries: Vec<FailedTargetEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FailedTargetEntry {
    target_id: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
}

impl TargetsResponse {
    /// An error for the first failed target, if any target failed. Its retryability follows
    /// the entry's error code.
    fn into_result(self, operation: &str, rule_name: &str) -> Result<()> {
        let Some(entry) = self.failed_entries.into_iter().next() else {
            return Ok(());
        };
        let code = entry.error_code.unwrap_or_else(|| "UnknownErrorCode".into());
        let message = format!(
            "{operation} failed for target '{}' of rule '{rule_name}': {}",
            entry.target_id.as_deref().unwrap_or("unknown"),
            entry.error_message.as_deref().unwrap_or("no message")
        );
        Err(AlienError::new(match code.as_str() {
            "ConcurrentModificationException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "Rule".into(),
                resource_name: rule_name.into(),
            },
            "ThrottlingException" | "LimitExceededException" => {
                ErrorData::RateLimitExceeded { message }
            }
            "InternalException" | "InternalFailure" | "ServiceUnavailableException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            _ => ErrorData::RemoteRequestRejected {
                service: "EventBridge".into(),
                http_status: 200,
                provider_code: code,
                message,
            },
        }))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{AwsClientConfig, AwsCredentials, AwsServiceOverrides};
    use httpmock::{Method::POST, MockServer};
    use std::collections::HashMap;

    fn client(server: &MockServer) -> EventBridgeClient {
        let credentials = AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access".into(),
                secret_access_key: "test-secret".into(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("events".into(), server.base_url())]),
            }),
        });
        EventBridgeClient::new(Client::new(), credentials)
    }

    async fn answer(server: &MockServer, target: &str, body: &str) {
        let target = target.to_string();
        let body = body.to_string();
        server
            .mock_async(move |when, then| {
                when.method(POST).header("X-Amz-Target", target);
                then.status(200)
                    .header("content-type", "application/x-amz-json-1.1")
                    .body(body);
            })
            .await;
    }

    #[tokio::test]
    async fn remove_targets_with_a_concurrent_modification_entry_is_a_retryable_error() {
        let server = MockServer::start_async().await;
        answer(
            &server,
            "AWSEvents.RemoveTargets",
            r#"{"FailedEntryCount":1,"FailedEntries":[{"TargetId":"1","ErrorCode":"ConcurrentModificationException","ErrorMessage":"Too many requests"}]}"#,
        )
        .await;

        let error = client(&server)
            .remove_targets("app-worker-cron-0", vec!["1".into()])
            .await
            .expect_err("a target AWS failed to remove must not be reported as removed");

        assert_eq!(error.code, "REMOTE_RESOURCE_CONFLICT");
        assert!(error.retryable);
        assert!(error.message.contains("target '1'"), "{}", error.message);
        assert!(error.message.contains("Too many requests"), "{}", error.message);
    }

    #[tokio::test]
    async fn remove_targets_with_an_unknown_entry_error_is_not_retryable() {
        let server = MockServer::start_async().await;
        answer(
            &server,
            "AWSEvents.RemoveTargets",
            r#"{"FailedEntryCount":1,"FailedEntries":[{"TargetId":"1","ErrorCode":"ValidationException","ErrorMessage":"bad target"}]}"#,
        )
        .await;

        let error = client(&server)
            .remove_targets("app-worker-cron-0", vec!["1".into()])
            .await
            .expect_err("a failed entry is an error");

        assert_eq!(error.code, "REMOTE_REQUEST_REJECTED");
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn remove_targets_without_failed_entries_succeeds() {
        let server = MockServer::start_async().await;
        answer(
            &server,
            "AWSEvents.RemoveTargets",
            r#"{"FailedEntryCount":0,"FailedEntries":[]}"#,
        )
        .await;

        client(&server)
            .remove_targets("app-worker-cron-0", vec!["1".into()])
            .await
            .expect("no failed entries means every target was removed");
    }

    #[tokio::test]
    async fn put_targets_with_a_failed_entry_is_an_error() {
        let server = MockServer::start_async().await;
        answer(
            &server,
            "AWSEvents.PutTargets",
            r#"{"FailedEntryCount":1,"FailedEntries":[{"TargetId":"1","ErrorCode":"InternalException","ErrorMessage":"internal"}]}"#,
        )
        .await;

        let error = client(&server)
            .put_targets(PutTargetsRequest {
                rule: "app-worker-cron-0".into(),
                targets: vec![EventBridgeTarget {
                    id: "1".into(),
                    arn: "arn:aws:lambda:us-east-1:123456789012:function:app-worker".into(),
                }],
            })
            .await
            .expect_err("a target AWS failed to add must not be reported as added");

        assert_eq!(error.code, "REMOTE_SERVICE_UNAVAILABLE");
        assert!(error.retryable);
    }
}
