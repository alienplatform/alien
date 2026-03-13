use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::AwsClientConfig;
use crate::aws::AwsClientConfigExt;
use alien_client_core::RequestBuilderExt;
use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use bon::Builder;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait EcrApi: Send + Sync + Debug {
    async fn create_repository(
        &self,
        request: CreateRepositoryRequest,
    ) -> Result<CreateRepositoryResponse>;
    async fn delete_repository(
        &self,
        request: DeleteRepositoryRequest,
    ) -> Result<DeleteRepositoryResponse>;
    async fn describe_repositories(
        &self,
        request: DescribeRepositoriesRequest,
    ) -> Result<DescribeRepositoriesResponse>;
    async fn set_repository_policy(
        &self,
        request: SetRepositoryPolicyRequest,
    ) -> Result<SetRepositoryPolicyResponse>;
    async fn get_repository_policy(
        &self,
        request: GetRepositoryPolicyRequest,
    ) -> Result<GetRepositoryPolicyResponse>;
    async fn delete_repository_policy(
        &self,
        request: DeleteRepositoryPolicyRequest,
    ) -> Result<DeleteRepositoryPolicyResponse>;
    async fn get_authorization_token(
        &self,
        request: GetAuthorizationTokenRequest,
    ) -> Result<GetAuthorizationTokenResponse>;
}

/// AWS ECR client using the new request/error abstractions.
#[derive(Debug, Clone)]
pub struct EcrClient {
    client: Client,
    config: AwsClientConfig,
}

impl EcrClient {
    pub fn new(client: Client, config: AwsClientConfig) -> Self {
        Self { client, config }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "ecr".into(),
            region: self.config.region.clone(),
            credentials: self.config.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.config.get_service_endpoint_option("ecr") {
            override_url.to_string()
        } else {
            format!("https://ecr.{}.amazonaws.com", self.config.region)
        }
    }

    // ---- Internal helpers ------------------------------------------------
    async fn post_json<T: serde::de::DeserializeOwned + Send + 'static>(
        &self,
        operation: &str,
        body: String,
        resource_name: &str,
    ) -> Result<T> {
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));
        let target = format!("AmazonEC2ContainerRegistry_V20150921.{}", operation);

        let builder = self
            .client
            .post(&url)
            .host(&format!("ecr.{}.amazonaws.com", self.config.region))
            .header("X-Amz-Target", target)
            .header("Content-Type", "application/x-amz-json-1.1")
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;

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
                        Self::map_ecr_error(status, text, operation, resource_name, &body, &url)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse ECR error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_ecr_error(
        status: StatusCode,
        error_body: &str,
        operation: &str,
        resource_name: &str,
        request_body: &str,
        url: &str,
    ) -> Option<ErrorData> {
        // Attempt to parse the canonical AWS error JSON.
        let parsed_error: std::result::Result<EcrErrorResponse, _> =
            serde_json::from_str(error_body);

        let (error_code, error_message) = match parsed_error {
            Ok(e) => {
                let code = e
                    .type_field
                    .or(e.type_field_underscore)
                    .unwrap_or_else(|| "UnknownErrorCode".into());
                let message = e
                    .message
                    .or(e.message_capital)
                    .unwrap_or_else(|| "Unknown error".into());
                (code, message)
            }
            Err(_) => {
                // If we can't parse the response, return None to use original error
                return None;
            }
        };

        Some(match error_code.as_str() {
            // Access & auth
            "AccessDeniedException"
            | "UnauthorizedOperation"
            | "InvalidUserID.NotFound"
            | "AuthFailure"
            | "SignatureDoesNotMatch"
            | "TokenRefreshRequired"
            | "NotAuthorized"
            | "InvalidClientTokenId"
            | "MissingAuthenticationToken"
            | "UnrecognizedClientException"
            | "InvalidSignatureException" => ErrorData::RemoteAccessDenied {
                resource_type: "ECR Repository".into(),
                resource_name: resource_name.into(),
            },

            // Unknown operation (usually indicates wrong Content-Type or X-Amz-Target)
            "UnknownOperationException" => ErrorData::InvalidInput {
                message: format!("Unknown operation: {}", error_message),
                field_name: None,
            },

            // Rate limiting / throttling
            "ThrottlingException" | "TooManyRequestsException" | "RequestLimitExceeded" => {
                ErrorData::RateLimitExceeded {
                    message: error_message,
                }
            }

            // Service unavailable
            "ServiceUnavailableException" | "InternalServerException" | "ServerException" => {
                ErrorData::RemoteServiceUnavailable {
                    message: error_message,
                }
            }

            // Not found
            "RepositoryNotFoundException"
            | "ImageNotFoundException"
            | "RegistryPolicyNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "ECR Repository".into(),
                resource_name: resource_name.into(),
            },

            // Repository not empty (DeleteRepository specific)
            "RepositoryNotEmptyException" => ErrorData::RemoteResourceConflict {
                message: error_message,
                resource_type: "ECR Repository".into(),
                resource_name: resource_name.into(),
            },

            // Already exists
            "RepositoryAlreadyExistsException" | "ImageAlreadyExistsException" => {
                ErrorData::RemoteResourceConflict {
                    message: error_message,
                    resource_type: "ECR Repository".into(),
                    resource_name: resource_name.into(),
                }
            }

            // Quota / limit exceeded
            "LimitExceededException" | "RepositoryPolicyNotFoundException" => {
                ErrorData::QuotaExceeded {
                    message: error_message,
                }
            }

            // Invalid parameter / validation errors
            "InvalidParameterException" | "ValidationException" => ErrorData::InvalidInput {
                message: error_message,
                field_name: None,
            },

            // Tag-specific errors
            "InvalidTagParameterException" => ErrorData::InvalidInput {
                message: error_message,
                field_name: Some("tags".into()),
            },

            "TooManyTagsException" => ErrorData::QuotaExceeded {
                message: error_message,
            },

            // KMS-related errors
            "KmsException" => ErrorData::InvalidInput {
                message: error_message,
                field_name: Some("encryptionConfiguration".into()),
            },

            // Generic fallback categories
            _ => match status {
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message: error_message,
                    resource_type: "ECR Repository".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "ECR Repository".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "ECR Repository".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded {
                    message: error_message,
                },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable {
                    message: error_message,
                },
                _ => ErrorData::HttpResponseError {
                    message: format!("ECR operation failed: {}", error_message),
                    url: url.to_string(),
                    http_status: status.as_u16(),
                    http_response_text: Some(error_body.into()),
                    http_request_text: Some(request_body.into()),
                },
            },
        })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl EcrApi for EcrClient {
    async fn create_repository(
        &self,
        request: CreateRepositoryRequest,
    ) -> Result<CreateRepositoryResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize CreateRepositoryRequest for repository '{}'",
                    request.repository_name
                ),
            },
        )?;

        self.post_json("CreateRepository", body, &request.repository_name)
            .await
    }

    async fn delete_repository(
        &self,
        request: DeleteRepositoryRequest,
    ) -> Result<DeleteRepositoryResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DeleteRepositoryRequest for repository '{}'",
                    request.repository_name
                ),
            },
        )?;

        self.post_json("DeleteRepository", body, &request.repository_name)
            .await
    }

    async fn describe_repositories(
        &self,
        request: DescribeRepositoriesRequest,
    ) -> Result<DescribeRepositoriesResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize DescribeRepositoriesRequest".to_string(),
            },
        )?;

        let resource_name = request
            .repository_names
            .as_ref()
            .and_then(|names| names.first())
            .map(|name| name.as_str())
            .unwrap_or("*");

        self.post_json("DescribeRepositories", body, resource_name)
            .await
    }

    async fn set_repository_policy(
        &self,
        request: SetRepositoryPolicyRequest,
    ) -> Result<SetRepositoryPolicyResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize SetRepositoryPolicyRequest for repository '{}'",
                    request.repository_name
                ),
            },
        )?;

        self.post_json("SetRepositoryPolicy", body, &request.repository_name)
            .await
    }

    async fn get_repository_policy(
        &self,
        request: GetRepositoryPolicyRequest,
    ) -> Result<GetRepositoryPolicyResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize GetRepositoryPolicyRequest for repository '{}'",
                    request.repository_name
                ),
            },
        )?;

        self.post_json("GetRepositoryPolicy", body, &request.repository_name)
            .await
    }

    async fn delete_repository_policy(
        &self,
        request: DeleteRepositoryPolicyRequest,
    ) -> Result<DeleteRepositoryPolicyResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DeleteRepositoryPolicyRequest for repository '{}'",
                    request.repository_name
                ),
            },
        )?;

        self.post_json("DeleteRepositoryPolicy", body, &request.repository_name)
            .await
    }

    async fn get_authorization_token(
        &self,
        request: GetAuthorizationTokenRequest,
    ) -> Result<GetAuthorizationTokenResponse> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize GetAuthorizationTokenRequest".to_string(),
            },
        )?;

        self.post_json("GetAuthorizationToken", body, "authorization-token")
            .await
    }
}

// -------------------------------------------------------------------------
// Error JSON structs
// -------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct EcrErrorResponse {
    #[serde(rename = "__type")]
    pub type_field_underscore: Option<String>,
    #[serde(rename = "Type")]
    pub type_field: Option<String>,
    #[serde(rename = "message")]
    pub message: Option<String>,
    #[serde(rename = "Message")]
    pub message_capital: Option<String>,
}

// -------------------------------------------------------------------------
// Request / response payloads
// -------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateRepositoryRequest {
    pub repository_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_tag_mutability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_scanning_configuration: Option<ImageScanningConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_configuration: Option<EncryptionConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateRepositoryResponse {
    pub repository: Repository,
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRepositoryRequest {
    pub repository_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRepositoryResponse {
    pub repository: Repository,
}

#[derive(Serialize, Debug, Clone, Builder, Default)]
#[serde(rename_all = "camelCase")]
pub struct DescribeRepositoriesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DescribeRepositoriesResponse {
    pub repositories: Vec<Repository>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SetRepositoryPolicyRequest {
    pub repository_name: String,
    pub policy_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SetRepositoryPolicyResponse {
    pub repository_name: String,
    pub policy_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GetRepositoryPolicyRequest {
    pub repository_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetRepositoryPolicyResponse {
    pub repository_name: String,
    pub policy_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRepositoryPolicyRequest {
    pub repository_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRepositoryPolicyResponse {
    pub repository_name: String,
    pub policy_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
}

#[derive(Serialize, Debug, Clone, Builder, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetAuthorizationTokenRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_ids: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetAuthorizationTokenResponse {
    pub authorization_data: Vec<AuthorizationData>,
}

// -------------------------------------------------------------------------
// Shared data structures
// -------------------------------------------------------------------------

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub repository_arn: String,
    pub registry_id: String,
    pub repository_name: String,
    pub repository_uri: String,
    pub created_at: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_tag_mutability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_scanning_configuration: Option<ImageScanningConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_configuration: Option<EncryptionConfiguration>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImageScanningConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_on_push: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionConfiguration {
    pub encryption_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tag {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "Value")]
    pub value: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationData {
    pub authorization_token: String,
    pub expires_at: f64,
    pub proxy_endpoint: String,
}
