//! AWS CloudWatch Logs client: the log group operations Alien needs to own a
//! Lambda function's log group (create it, set its retention, delete it).

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait CloudWatchLogsApi: Send + Sync + std::fmt::Debug {
    /// Creates a log group. A group that already exists is a
    /// `RemoteResourceConflict`.
    async fn create_log_group(&self, request: CreateLogGroupRequest) -> Result<()>;

    /// Sets how many days a log group keeps its events.
    async fn put_retention_policy(&self, request: PutRetentionPolicyRequest) -> Result<()>;

    /// Deletes a log group and its events. A group that does not exist is a
    /// `RemoteResourceNotFound`.
    async fn delete_log_group(&self, log_group_name: &str) -> Result<()>;

    /// The tags of a log group, by its ARN without the trailing `:*`. A group that does not
    /// exist is a `RemoteResourceNotFound`.
    async fn list_tags_for_resource(&self, resource_arn: &str) -> Result<HashMap<String, String>>;
}

/// `CreateLogGroup` request.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateLogGroupRequest {
    pub log_group_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,
}

/// `DeleteLogGroup` request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteLogGroupRequest<'a> {
    log_group_name: &'a str,
}

/// `ListTagsForResource` request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListTagsForResourceRequest<'a> {
    resource_arn: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListTagsForResourceResponse {
    #[serde(default)]
    tags: HashMap<String, String>,
}

/// `PutRetentionPolicy` request.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PutRetentionPolicyRequest {
    pub log_group_name: String,
    /// One of the values CloudWatch Logs accepts (1, 3, 5, 7, 14, 30, ...).
    pub retention_in_days: i32,
}

#[derive(Debug, Deserialize)]
struct CloudWatchLogsErrorResponse {
    #[serde(rename = "__type")]
    type_field: Option<String>,
    /// AccessDeniedException spells it `Message`; the other errors `message`.
    #[serde(alias = "Message")]
    message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CloudWatchLogsClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl CloudWatchLogsClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
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

    fn get_host(&self) -> String {
        format!("logs.{}.amazonaws.com", self.credentials.region())
    }

    /// Sends a JSON 1.1 request whose successful response has no body.
    /// `retry` is off for a create, whose "already exists" answer is final.
    async fn send(
        &self,
        target: &str,
        body: String,
        log_group_name: &str,
        retry: bool,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let builder = self.request(target, &body);

        let result = if retry {
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config()).await
        } else {
            crate::aws::aws_request_utils::sign_send_no_response_once(builder, &self.sign_config())
                .await
        };
        Self::map_result(result, log_group_name)
    }

    fn request(&self, target: &str, body: &str) -> reqwest::RequestBuilder {
        self.client
            .request(Method::POST, self.get_base_url())
            .host(&self.get_host())
            .header("X-Amz-Target", format!("Logs_20140328.{target}"))
            .header("Content-Type", "application/x-amz-json-1.1")
            .content_sha256(body)
            .body(body.to_string())
    }

    fn map_result<T>(result: Result<T>, log_group_name: &str) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let mapped = match &error.error {
                    Some(ErrorData::HttpResponseError {
                        http_status,
                        http_response_text: Some(text),
                        ..
                    }) => Self::map_error(text, log_group_name, *http_status),
                    _ => None,
                };
                Err(mapped
                    .into_iter()
                    .flatten()
                    .fold(error, |error, layer| error.context(layer)))
            }
        }
    }

    /// The errors to wrap the HTTP error in, innermost first.
    fn map_error(body: &str, log_group_name: &str, http_status: u16) -> Option<Vec<ErrorData>> {
        let parsed: CloudWatchLogsErrorResponse = serde_json::from_str(body).ok()?;
        let code = parsed.type_field?;
        let code = code.rsplit('#').next().unwrap_or(&code).to_string();
        let message = parsed.message.unwrap_or_else(|| code.clone());
        let resource = || {
            (
                String::from("CloudWatch Logs log group"),
                log_group_name.to_string(),
            )
        };
        let mapped = match code.as_str() {
            "ResourceAlreadyExistsException" => {
                let (resource_type, resource_name) = resource();
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type,
                    resource_name,
                }
            }
            "ResourceNotFoundException" => {
                let (resource_type, resource_name) = resource();
                ErrorData::RemoteResourceNotFound {
                    resource_type,
                    resource_name,
                }
            }
            // AWS's message names the denied action and the caller, which a warning can show.
            "AccessDeniedException" => {
                let (resource_type, resource_name) = resource();
                return Some(vec![
                    ErrorData::RemoteRequestRejected {
                        service: "CloudWatch Logs".to_string(),
                        http_status,
                        provider_code: code,
                        message,
                    },
                    ErrorData::RemoteAccessDenied {
                        resource_type,
                        resource_name,
                    },
                ]);
            }
            "ThrottlingException" => ErrorData::RateLimitExceeded { message },
            "LimitExceededException" => ErrorData::QuotaExceeded { message },
            "ServiceUnavailableException" => ErrorData::RemoteServiceUnavailable { message },
            "InvalidParameterException" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            _ => return None,
        };
        Some(vec![mapped])
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl CloudWatchLogsApi for CloudWatchLogsClient {
    async fn create_log_group(&self, request: CreateLogGroupRequest) -> Result<()> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateLogGroupRequest for '{}'",
                    request.log_group_name
                ),
            },
        )?;
        self.send("CreateLogGroup", body, &request.log_group_name, false)
            .await
    }

    async fn put_retention_policy(&self, request: PutRetentionPolicyRequest) -> Result<()> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize PutRetentionPolicyRequest for '{}'",
                    request.log_group_name
                ),
            },
        )?;
        self.send("PutRetentionPolicy", body, &request.log_group_name, true)
            .await
    }

    async fn delete_log_group(&self, log_group_name: &str) -> Result<()> {
        let body = serde_json::to_string(&DeleteLogGroupRequest { log_group_name })
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DeleteLogGroupRequest for '{log_group_name}'"
                ),
            })?;
        self.send("DeleteLogGroup", body, log_group_name, true)
            .await
    }

    async fn list_tags_for_resource(&self, resource_arn: &str) -> Result<HashMap<String, String>> {
        let body = serde_json::to_string(&ListTagsForResourceRequest { resource_arn })
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize ListTagsForResourceRequest for '{resource_arn}'"
                ),
            })?;
        self.credentials.ensure_fresh().await?;
        let builder = self.request("ListTagsForResource", &body);
        let result: Result<ListTagsForResourceResponse> =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;
        Self::map_result(result, resource_arn).map(|response| response.tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{AwsClientConfig, AwsCredentials, AwsServiceOverrides};
    use httpmock::{Method::POST, MockServer};

    fn client(server: &MockServer) -> CloudWatchLogsClient {
        let credentials = AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access".into(),
                secret_access_key: "test-secret".into(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("logs".into(), server.base_url())]),
            }),
        });
        CloudWatchLogsClient::new(Client::new(), credentials)
    }

    #[tokio::test]
    async fn creates_a_log_group_and_sets_its_retention() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "Logs_20140328.CreateLogGroup")
                    .header("content-type", "application/x-amz-json-1.1")
                    .json_body(serde_json::json!({ "logGroupName": "/aws/lambda/app-api" }));
                then.status(200);
            })
            .await;
        let retention = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "Logs_20140328.PutRetentionPolicy")
                    .json_body(serde_json::json!({
                        "logGroupName": "/aws/lambda/app-api",
                        "retentionInDays": 30,
                    }));
                then.status(200);
            })
            .await;
        let client = client(&server);

        client
            .create_log_group(
                CreateLogGroupRequest::builder()
                    .log_group_name("/aws/lambda/app-api".to_string())
                    .build(),
            )
            .await
            .expect("create log group");
        client
            .put_retention_policy(
                PutRetentionPolicyRequest::builder()
                    .log_group_name("/aws/lambda/app-api".to_string())
                    .retention_in_days(30)
                    .build(),
            )
            .await
            .expect("put retention policy");

        create.assert_hits_async(1).await;
        retention.assert_hits_async(1).await;
    }

    /// A response recorded from AWS for a role without the grant (account id replaced).
    const RECORDED_ACCESS_DENIED: &str = r#"{"__type":"AccessDeniedException","Message":"User: arn:aws:sts::123456789012:assumed-role/app-management/session is not authorized to perform: logs:CreateLogGroup on resource: arn:aws:logs:us-east-1:123456789012:log-group:/aws/lambda/app-api because no identity-based policy allows the logs:CreateLogGroup action"}"#;

    #[tokio::test]
    async fn a_denied_call_keeps_the_aws_message() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "Logs_20140328.CreateLogGroup");
                then.status(400).body(RECORDED_ACCESS_DENIED);
            })
            .await;

        let error = client(&server)
            .create_log_group(
                CreateLogGroupRequest::builder()
                    .log_group_name("/aws/lambda/app-api".to_string())
                    .build(),
            )
            .await
            .expect_err("denied");

        assert!(
            matches!(
                error.error,
                Some(ErrorData::RemoteAccessDenied { ref resource_name, .. })
                    if resource_name == "/aws/lambda/app-api"
            ),
            "{error:?}"
        );
        let rejection = error.source.as_deref().expect("the AWS rejection");
        assert_eq!(rejection.code, "REMOTE_REQUEST_REJECTED");
        assert!(!rejection.internal);
        assert_eq!(
            rejection.message,
            "CloudWatch Logs rejected the request (AccessDeniedException): User: \
             arn:aws:sts::123456789012:assumed-role/app-management/session is not authorized to \
             perform: logs:CreateLogGroup on resource: \
             arn:aws:logs:us-east-1:123456789012:log-group:/aws/lambda/app-api because no \
             identity-based policy allows the logs:CreateLogGroup action"
        );
    }

    #[tokio::test]
    async fn an_existing_log_group_is_a_conflict() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "Logs_20140328.CreateLogGroup");
                then.status(400).body(
                    r#"{"__type":"ResourceAlreadyExistsException","message":"The specified log group already exists"}"#,
                );
            })
            .await;

        let error = client(&server)
            .create_log_group(
                CreateLogGroupRequest::builder()
                    .log_group_name("/aws/lambda/app-api".to_string())
                    .build(),
            )
            .await
            .expect_err("existing log group");

        assert!(
            matches!(
                error.error,
                Some(ErrorData::RemoteResourceConflict { ref resource_name, .. })
                    if resource_name == "/aws/lambda/app-api"
            ),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn lists_a_log_groups_tags_and_reports_a_missing_one_as_not_found() {
        let server = MockServer::start_async().await;
        let tags = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "Logs_20140328.ListTagsForResource")
                    .json_body(serde_json::json!({
                        "resourceArn": "arn:aws:logs:us-east-1:123456789012:log-group:app-net-flow-logs"
                    }));
                then.status(200)
                    .body(r#"{"tags":{"deployment":"app","managed-by":"runtime"}}"#);
            })
            .await;
        let missing = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "Logs_20140328.ListTagsForResource")
                    .json_body(serde_json::json!({
                        "resourceArn": "arn:aws:logs:us-east-1:123456789012:log-group:gone"
                    }));
                then.status(400).body(
                    r#"{"__type":"com.amazonaws.logs#ResourceNotFoundException","message":"The specified log group does not exist."}"#,
                );
            })
            .await;
        let client = client(&server);

        let found = client
            .list_tags_for_resource(
                "arn:aws:logs:us-east-1:123456789012:log-group:app-net-flow-logs",
            )
            .await
            .expect("tags");
        let error = client
            .list_tags_for_resource("arn:aws:logs:us-east-1:123456789012:log-group:gone")
            .await
            .expect_err("missing log group");

        assert_eq!(
            found,
            HashMap::from([
                ("deployment".to_string(), "app".to_string()),
                ("managed-by".to_string(), "runtime".to_string()),
            ])
        );
        tags.assert_hits_async(1).await;
        assert!(missing.hits_async().await >= 1);
        assert!(
            matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn deletes_a_log_group_and_reports_a_missing_one_as_not_found() {
        let server = MockServer::start_async().await;
        let delete = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "Logs_20140328.DeleteLogGroup")
                    .json_body(serde_json::json!({ "logGroupName": "/aws/lambda/app-api" }));
                then.status(200);
            })
            .await;
        let missing = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("x-amz-target", "Logs_20140328.DeleteLogGroup")
                    .json_body(serde_json::json!({ "logGroupName": "/aws/lambda/app-gone" }));
                then.status(400).body(
                    r#"{"__type":"com.amazonaws.logs#ResourceNotFoundException","message":"The specified log group does not exist."}"#,
                );
            })
            .await;
        let client = client(&server);

        client
            .delete_log_group("/aws/lambda/app-api")
            .await
            .expect("delete log group");
        let error = client
            .delete_log_group("/aws/lambda/app-gone")
            .await
            .expect_err("missing log group");

        delete.assert_hits_async(1).await;
        assert!(missing.hits_async().await >= 1);
        assert!(
            matches!(
                error.error,
                Some(ErrorData::RemoteResourceNotFound { ref resource_name, .. })
                    if resource_name == "/aws/lambda/app-gone"
            ),
            "{error:?}"
        );
    }
}
