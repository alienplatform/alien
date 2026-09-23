//! AWS Cloud Control API: create, read and delete any CloudFormation-registry resource type.
//!
//! AWS JSON 1.0 (`X-Amz-Target: CloudApiService.<Operation>`, signing name `cloudcontrolapi`).
//! Resource documents (`DesiredState`, `Properties`, `ResourceModel`) travel as JSON strings, not
//! nested objects. Mutations are asynchronous: they return a [`ProgressEvent`] whose
//! `RequestToken` is polled with `GetResourceRequestStatus`.

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
pub trait CloudControlApi: Send + Sync + std::fmt::Debug {
    async fn create_resource(&self, request: CreateResourceRequest) -> Result<ProgressEvent>;
    async fn get_resource(&self, type_name: &str, identifier: &str) -> Result<ResourceDescription>;
    async fn delete_resource(&self, type_name: &str, identifier: &str) -> Result<ProgressEvent>;
    async fn get_resource_request_status(&self, request_token: &str) -> Result<ProgressEvent>;
    async fn list_resources(
        &self,
        type_name: &str,
        next_token: Option<String>,
    ) -> Result<ListResourcesResponse>;
}

#[derive(Debug, Clone)]
pub struct CloudControlClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl CloudControlClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    async fn send<B: Serialize, T: DeserializeOwned + Send + 'static>(
        &self,
        operation: &str,
        request: &B,
        resource: &str,
    ) -> Result<T> {
        let body = serde_json::to_string(request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Cloud Control {operation} request"),
            },
        )?;
        self.credentials.ensure_fresh().await?;
        let region = self.credentials.region();
        let endpoint = self
            .credentials
            .get_service_endpoint_option("cloudcontrolapi")
            .map(str::to_string)
            .unwrap_or_else(|| format!("https://cloudcontrolapi.{region}.amazonaws.com"));
        let builder = self
            .client
            .post(endpoint.trim_end_matches('/'))
            .host(&format!("cloudcontrolapi.{region}.amazonaws.com"))
            .header("X-Amz-Target", format!("CloudApiService.{operation}"))
            .header("content-type", "application/x-amz-json-1.0")
            .content_sha256(&body)
            .body(body.clone());
        let result = crate::aws::aws_request_utils::sign_send_json(
            builder,
            &AwsSignConfig {
                service_name: "cloudcontrolapi".into(),
                region: region.to_string(),
                credentials: self.credentials.get_credentials(),
                signing_region: None,
            },
        )
        .await;
        map_result(result, resource)
    }
}

fn map_result<T>(result: Result<T>, resource: &str) -> Result<T> {
    let error = match result {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };
    let Some(ErrorData::HttpResponseError {
        http_status,
        http_response_text: Some(text),
        ..
    }) = &error.error
    else {
        return Err(error);
    };
    let Ok(parsed) = serde_json::from_str::<CloudControlErrorResponse>(text) else {
        return Err(error);
    };
    let Some(raw_code) = parsed.error_type else {
        return Err(error);
    };
    let code = raw_code.rsplit('#').next().unwrap_or(&raw_code).to_string();
    let message = parsed
        .message
        .or(parsed.message_upper)
        .unwrap_or_else(|| code.clone());
    let status = StatusCode::from_u16(*http_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mapped = match code.as_str() {
        "ResourceNotFoundException" | "RequestTokenNotFoundException" => {
            ErrorData::RemoteResourceNotFound {
                resource_type: "Cloud Control resource".into(),
                resource_name: resource.into(),
            }
        }
        "AlreadyExistsException"
        | "ConcurrentOperationException"
        | "ResourceConflictException"
        | "ClientTokenConflictException" => ErrorData::RemoteResourceConflict {
            message,
            resource_type: "Cloud Control resource".into(),
            resource_name: resource.into(),
        },
        "InvalidCredentialsException" | "AccessDeniedException" => ErrorData::RemoteAccessDenied {
            resource_type: "Cloud Control resource".into(),
            resource_name: resource.into(),
        },
        "ThrottlingException" => ErrorData::RateLimitExceeded { message },
        "ServiceLimitExceededException" => ErrorData::QuotaExceeded { message },
        "ServiceInternalErrorException" | "NetworkFailureException" => {
            ErrorData::RemoteServiceUnavailable { message }
        }
        "InvalidRequestException" | "TypeNotFoundException" | "UnsupportedActionException" => {
            ErrorData::InvalidInput {
                message,
                field_name: None,
            }
        }
        _ if status == StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
        _ => ErrorData::GenericError {
            message: format!("Cloud Control {code} for '{resource}': {message}"),
        },
    };
    Err(error.context(mapped))
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudControlApi for CloudControlClient {
    async fn create_resource(&self, request: CreateResourceRequest) -> Result<ProgressEvent> {
        // A fresh token per call: the transport's retry of this one request must not create a
        // second resource, while a later call after a FAILED create must not replay the failure,
        // which a token reused within its 36-hour window would.
        let request = CreateResourceRequest {
            client_token: Some(
                request
                    .client_token
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            ),
            ..request
        };
        let response: ProgressEventResponse = self
            .send("CreateResource", &request, &request.type_name)
            .await?;
        Ok(response.progress_event)
    }

    async fn get_resource(&self, type_name: &str, identifier: &str) -> Result<ResourceDescription> {
        let response: GetResourceResponse = self
            .send(
                "GetResource",
                &IdentifiedRequest {
                    type_name,
                    identifier,
                    client_token: None,
                },
                identifier,
            )
            .await?;
        Ok(response.resource_description)
    }

    async fn delete_resource(&self, type_name: &str, identifier: &str) -> Result<ProgressEvent> {
        let client_token = uuid::Uuid::new_v4().to_string();
        let response: ProgressEventResponse = self
            .send(
                "DeleteResource",
                &IdentifiedRequest {
                    type_name,
                    identifier,
                    client_token: Some(&client_token),
                },
                identifier,
            )
            .await?;
        Ok(response.progress_event)
    }

    async fn get_resource_request_status(&self, request_token: &str) -> Result<ProgressEvent> {
        let response: ProgressEventResponse = self
            .send(
                "GetResourceRequestStatus",
                &RequestStatusRequest { request_token },
                request_token,
            )
            .await?;
        Ok(response.progress_event)
    }

    async fn list_resources(
        &self,
        type_name: &str,
        next_token: Option<String>,
    ) -> Result<ListResourcesResponse> {
        self.send(
            "ListResources",
            &ListResourcesRequest {
                type_name,
                next_token,
            },
            type_name,
        )
        .await
    }
}

/// A request to create one resource.
#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateResourceRequest {
    /// The registry type, e.g. `AWS::Lambda::NetworkConnector`.
    pub type_name: String,
    /// The resource's properties as one JSON document, sent as a string.
    pub desired_state: String,
    /// Idempotency token; the client generates one per call when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct IdentifiedRequest<'a> {
    type_name: &'a str,
    identifier: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_token: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct RequestStatusRequest<'a> {
    request_token: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct ListResourcesRequest<'a> {
    type_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ProgressEventResponse {
    progress_event: ProgressEvent,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GetResourceResponse {
    resource_description: ResourceDescription,
}

#[derive(Deserialize)]
struct CloudControlErrorResponse {
    #[serde(rename = "__type")]
    error_type: Option<String>,
    message: Option<String>,
    #[serde(rename = "Message")]
    message_upper: Option<String>,
}

/// Where an asynchronous resource operation stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationStatus {
    Pending,
    InProgress,
    Success,
    Failed,
    CancelInProgress,
    CancelComplete,
}

/// The state of one resource operation request.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProgressEvent {
    pub type_name: Option<String>,
    /// The primary identifier; may be present before the operation succeeds.
    pub identifier: Option<String>,
    pub request_token: String,
    pub operation: Option<String>,
    pub operation_status: OperationStatus,
    pub status_message: Option<String>,
    /// A handler error code such as `NotFound` or `AlreadyExists`, set when the status is FAILED.
    pub error_code: Option<String>,
}

impl ProgressEvent {
    /// Whether the request has stopped moving.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.operation_status,
            OperationStatus::Success | OperationStatus::Failed | OperationStatus::CancelComplete
        )
    }

    /// The error a FAILED or cancelled request stands for, carrying AWS's code and message.
    ///
    /// `NotFound` and `AlreadyExists` map to the same variants a synchronous refusal does, so a
    /// caller handles "already gone" and "already there" the same way whichever path reports it.
    pub fn failure(&self) -> Option<AlienError<ErrorData>> {
        if !matches!(
            self.operation_status,
            OperationStatus::Failed | OperationStatus::CancelComplete
        ) {
            return None;
        }
        let resource = self
            .identifier
            .clone()
            .or_else(|| self.type_name.clone())
            .unwrap_or_else(|| self.request_token.clone());
        let code = self.error_code.as_deref().unwrap_or("Unknown");
        let message = format!(
            "Cloud Control {} request '{}' ended {:?} with {code}: {}",
            self.operation.as_deref().unwrap_or("resource"),
            self.request_token,
            self.operation_status,
            self.status_message
                .as_deref()
                .unwrap_or("no status message")
        );
        let reported = AlienError::new(ErrorData::GenericError {
            message: message.clone(),
        });
        let typed = match code {
            "NotFound" => ErrorData::RemoteResourceNotFound {
                resource_type: "Cloud Control resource".into(),
                resource_name: resource,
            },
            "AlreadyExists" | "ResourceConflict" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "Cloud Control resource".into(),
                resource_name: resource,
            },
            "Throttling" => ErrorData::RateLimitExceeded { message },
            "AccessDenied" | "InvalidCredentials" | "UnauthorizedTaggingOperation" => {
                ErrorData::RemoteAccessDenied {
                    resource_type: "Cloud Control resource".into(),
                    resource_name: resource,
                }
            }
            _ => return Some(reported),
        };
        Some(reported.context(typed))
    }
}

/// A provisioned resource: its primary identifier and its properties as a JSON string.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ResourceDescription {
    pub identifier: String,
    #[serde(default)]
    pub properties: Option<String>,
}

/// One page of `ListResources`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListResourcesResponse {
    #[serde(default)]
    pub resource_descriptions: Vec<ResourceDescription>,
    pub next_token: Option<String>,
}
