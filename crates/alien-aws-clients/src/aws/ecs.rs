//! JSON 1.1 API client for Amazon ECS and CloudWatch Logs. Calls are JSON in,
//! JSON out: the operator and its plugins read a few fields of a few
//! operations, so no full model is kept.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::Value;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait EcsApi: Send + Sync + std::fmt::Debug {
    /// Call one ECS operation (`ListServices`, `DescribeServices`, ...) with its
    /// JSON request and return its JSON response.
    async fn call(&self, operation: &str, request: Value) -> Result<Value>;
}

#[derive(Debug, Clone)]
pub struct EcsClient {
    client: Client,
    credentials: AwsCredentialProvider,
    service: Service,
}

/// The service a [`EcsClient`] talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Service {
    Ecs,
    Logs,
}

impl Service {
    /// Signing name, endpoint prefix and override key.
    fn name(self) -> &'static str {
        match self {
            Service::Ecs => "ecs",
            Service::Logs => "logs",
        }
    }

    fn target(self, operation: &str) -> String {
        match self {
            Service::Ecs => format!("AmazonEC2ContainerServiceV20141113.{operation}"),
            Service::Logs => format!("Logs_20140328.{operation}"),
        }
    }

    fn resource_type(self) -> &'static str {
        match self {
            Service::Ecs => "Amazon ECS",
            Service::Logs => "CloudWatch Logs",
        }
    }
}

impl EcsClient {
    /// A client for the Amazon ECS API.
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
            service: Service::Ecs,
        }
    }

    /// A client for the CloudWatch Logs API (`FilterLogEvents`, ...), same
    /// call shape as [`EcsClient::new`].
    pub fn new_logs(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
            service: Service::Logs,
        }
    }

    fn base_url(&self) -> String {
        let name = self.service.name();
        match self.credentials.get_service_endpoint_option(name) {
            Some(url) => url.to_string(),
            None => format!("https://{name}.{}.amazonaws.com", self.credentials.region()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct EcsError {
    #[serde(rename = "__type")]
    kind: Option<String>,
    #[serde(alias = "Message", alias = "message")]
    message: Option<String>,
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl EcsApi for EcsClient {
    async fn call(&self, operation: &str, request: Value) -> Result<Value> {
        self.credentials.ensure_fresh().await?;
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize the ECS {operation} request"),
            },
        )?;
        let base = self.base_url();
        let builder = self
            .client
            .post(format!("{}/", base.trim_end_matches('/')))
            .host(&format!(
                "{}.{}.amazonaws.com",
                self.service.name(),
                self.credentials.region()
            ))
            .header("X-Amz-Target", self.service.target(operation))
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());
        let sign = AwsSignConfig {
            service_name: self.service.name().into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
            interceptors: self.credentials.interceptors().clone(),
        };
        let result: Result<Value> =
            crate::aws::aws_request_utils::sign_send_json(builder, &sign).await;
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(text),
                    ..
                }) = &error.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) = map_error(self.service, status, text, operation) {
                        return Err(error.context(mapped));
                    }
                }
                Err(error)
            }
        }
    }
}

fn map_error(
    service: Service,
    status: StatusCode,
    text: &str,
    operation: &str,
) -> Option<ErrorData> {
    let parsed: EcsError = serde_json::from_str(text).ok()?;
    let kind = parsed.kind.unwrap_or_default();
    let kind = kind.rsplit('#').next().unwrap_or_default().to_string();
    let message = parsed.message.unwrap_or_else(|| "Unknown error".into());
    Some(match kind.as_str() {
        "AccessDeniedException" | "UnauthorizedOperation" => ErrorData::RemoteAccessDenied {
            resource_type: service.resource_type().into(),
            resource_name: operation.into(),
        },
        "ClusterNotFoundException" | "ServiceNotFoundException" | "ResourceNotFoundException" => {
            ErrorData::RemoteResourceNotFound {
                resource_type: service.resource_type().into(),
                resource_name: operation.into(),
            }
        }
        "ThrottlingException" | "RequestLimitExceeded" => ErrorData::RateLimitExceeded { message },
        "ServerException" | "ServiceUnavailableException" => {
            ErrorData::RemoteServiceUnavailable { message }
        }
        "InvalidParameterException" | "ClientException" => ErrorData::InvalidInput {
            message,
            field_name: None,
        },
        _ => ErrorData::HttpResponseError {
            message: format!("{} {operation} failed: {message}", service.resource_type()),
            url: format!("{}.amazonaws.com", service.name()),
            http_status: status.as_u16(),
            http_response_text: Some(text.into()),
            http_request_text: None,
        },
    })
}
