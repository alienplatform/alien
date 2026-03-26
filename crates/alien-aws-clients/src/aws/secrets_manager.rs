use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, ContextError, IntoAlienError};
use bon::Builder;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait SecretsManagerApi: Send + Sync + std::fmt::Debug {
    async fn create_secret(&self, request: CreateSecretRequest) -> Result<CreateSecretResponse>;
    async fn update_secret(&self, request: UpdateSecretRequest) -> Result<UpdateSecretResponse>;
    async fn delete_secret(&self, request: DeleteSecretRequest) -> Result<DeleteSecretResponse>;
    async fn describe_secret(
        &self,
        request: DescribeSecretRequest,
    ) -> Result<DescribeSecretResponse>;
    async fn get_secret_value(
        &self,
        request: GetSecretValueRequest,
    ) -> Result<GetSecretValueResponse>;
    async fn put_secret_value(
        &self,
        request: PutSecretValueRequest,
    ) -> Result<PutSecretValueResponse>;
}

// ---------------------------------------------------------------------------
// Secrets Manager client using new request helpers.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct SecretsManagerClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl SecretsManagerClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "secretsmanager".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("secretsmanager") {
            override_url.to_string()
        } else {
            format!(
                "https://secretsmanager.{}.amazonaws.com",
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
                "secretsmanager.{}.amazonaws.com",
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
                    if let Some(mapped) = Self::map_secrets_manager_error(
                        status,
                        text,
                        operation,
                        resource,
                        request_body,
                    ) {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse Secrets Manager error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_secrets_manager_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<SecretsManagerErrorResponse, _> =
            serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let c = e.error_type.unwrap_or_else(|| "UnknownErrorCode".into());
                let m = e.message.unwrap_or_else(|| "Unknown error".into());
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
            | "UnauthorizedOperation"
            | "InvalidUserException"
            | "UnrecognizedClientException" => ErrorData::RemoteAccessDenied {
                resource_type: "AWS Secrets Manager Secret".into(),
                resource_name: resource.into(),
            },
            // Throttling
            "ThrottlingException" | "RequestLimitExceeded" => {
                ErrorData::RateLimitExceeded { message }
            }
            // Service unavailable
            "InternalServiceException" | "ServiceUnavailableException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Resource not found
            "ResourceNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "AWS Secrets Manager Secret".into(),
                resource_name: resource.into(),
            },
            // Conflict / already exists
            "ResourceExistsException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "AWS Secrets Manager Secret".into(),
                resource_name: resource.into(),
            },
            // Invalid input
            "InvalidParameterException"
            | "ValidationException"
            | "MalformedPolicyDocumentException" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "AWS Secrets Manager Secret".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "AWS Secrets Manager Secret".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "AWS Secrets Manager Secret".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("Secrets Manager operation failed: {}", message),
                    url: format!("secretsmanager.amazonaws.com"),
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
impl SecretsManagerApi for SecretsManagerClient {
    async fn create_secret(&self, request: CreateSecretRequest) -> Result<CreateSecretResponse> {
        // Ensure client_request_token is provided for idempotency
        let mut request = request;
        if request.client_request_token.is_none() {
            request.client_request_token = Some(Uuid::new_v4().to_string());
        }

        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateSecretRequest for secret '{}'",
                    request.name
                ),
            },
        )?;
        self.send_json(
            "secretsmanager.CreateSecret",
            body,
            "CreateSecret",
            &request.name,
        )
        .await
    }

    async fn update_secret(&self, request: UpdateSecretRequest) -> Result<UpdateSecretResponse> {
        // Ensure client_request_token is provided for idempotency
        let mut request = request;
        if request.client_request_token.is_none() {
            request.client_request_token = Some(Uuid::new_v4().to_string());
        }

        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize UpdateSecretRequest for secret '{}'",
                    request.secret_id
                ),
            },
        )?;
        self.send_json(
            "secretsmanager.UpdateSecret",
            body,
            "UpdateSecret",
            &request.secret_id,
        )
        .await
    }

    async fn delete_secret(&self, request: DeleteSecretRequest) -> Result<DeleteSecretResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DeleteSecretRequest for secret '{}'",
                    request.secret_id
                ),
            },
        )?;
        self.send_json(
            "secretsmanager.DeleteSecret",
            body,
            "DeleteSecret",
            &request.secret_id,
        )
        .await
    }

    async fn describe_secret(
        &self,
        request: DescribeSecretRequest,
    ) -> Result<DescribeSecretResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DescribeSecretRequest for secret '{}'",
                    request.secret_id
                ),
            },
        )?;
        self.send_json(
            "secretsmanager.DescribeSecret",
            body,
            "DescribeSecret",
            &request.secret_id,
        )
        .await
    }

    async fn get_secret_value(
        &self,
        request: GetSecretValueRequest,
    ) -> Result<GetSecretValueResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize GetSecretValueRequest for secret '{}'",
                    request.secret_id
                ),
            },
        )?;
        self.send_json(
            "secretsmanager.GetSecretValue",
            body,
            "GetSecretValue",
            &request.secret_id,
        )
        .await
    }

    async fn put_secret_value(
        &self,
        request: PutSecretValueRequest,
    ) -> Result<PutSecretValueResponse> {
        // Ensure client_request_token is provided for idempotency
        let mut request = request;
        if request.client_request_token.is_none() {
            request.client_request_token = Some(Uuid::new_v4().to_string());
        }

        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize PutSecretValueRequest for secret '{}'",
                    request.secret_id
                ),
            },
        )?;
        self.send_json(
            "secretsmanager.PutSecretValue",
            body,
            "PutSecretValue",
            &request.secret_id,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Error JSON mapping structs
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
struct SecretsManagerErrorResponse {
    #[serde(rename = "__type")]
    error_type: Option<String>,
    #[serde(rename = "message")]
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Request / response payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateSecretRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_binary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_overwrite_replica_secret: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replica_regions: Option<Vec<ReplicaRegionType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateSecretResponse {
    pub arn: Option<String>,
    pub name: Option<String>,
    pub version_id: Option<String>,
    pub replica_regions: Option<Vec<ReplicaRegionType>>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateSecretRequest {
    pub secret_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_binary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UpdateSecretResponse {
    pub arn: Option<String>,
    pub name: Option<String>,
    pub version_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteSecretRequest {
    pub secret_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_delete_without_recovery: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_window_in_days: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeleteSecretResponse {
    pub arn: Option<String>,
    pub name: Option<String>,
    pub deletion_date: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeSecretRequest {
    pub secret_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeSecretResponse {
    pub arn: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub kms_key_id: Option<String>,
    pub rotation_enabled: Option<bool>,
    pub rotation_lambda_arn: Option<String>,
    pub rotation_rules: Option<RotationRulesType>,
    pub last_rotated_date: Option<f64>,
    pub last_changed_date: Option<f64>,
    pub last_accessed_date: Option<f64>,
    pub deletion_date: Option<f64>,
    pub tags: Option<Vec<Tag>>,
    pub version_ids_to_stages: Option<HashMap<String, Vec<String>>>,
    pub owning_service: Option<String>,
    pub created_date: Option<f64>,
    pub primary_region: Option<String>,
    pub replica_regions: Option<Vec<ReplicaRegionType>>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct GetSecretValueRequest {
    pub secret_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_stage: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetSecretValueResponse {
    pub arn: Option<String>,
    pub name: Option<String>,
    pub version_id: Option<String>,
    pub secret_binary: Option<String>,
    pub secret_string: Option<String>,
    pub version_stages: Option<Vec<String>>,
    pub created_date: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct PutSecretValueRequest {
    pub secret_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_binary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_stages: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_request_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PutSecretValueResponse {
    pub arn: Option<String>,
    pub name: Option<String>,
    pub version_id: Option<String>,
    pub version_stages: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Supporting structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ReplicaRegionType {
    pub region: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct RotationRulesType {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatically_after_days: Option<i64>,
}
