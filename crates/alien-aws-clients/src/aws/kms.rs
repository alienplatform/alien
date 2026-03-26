use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use bon::Builder;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait KmsApi: Send + Sync + std::fmt::Debug {
    async fn create_key(&self, request: CreateKeyRequest) -> Result<KeyMetadata>;
    async fn describe_key(&self, key_id: &str) -> Result<KeyMetadata>;
    async fn disable_key(&self, key_id: &str) -> Result<()>;
    async fn enable_key(&self, key_id: &str) -> Result<()>;
    async fn schedule_key_deletion(
        &self,
        key_id: &str,
        pending_window_in_days: Option<i32>,
    ) -> Result<ScheduleKeyDeletionResponse>;
}

// ---------------------------------------------------------------------------
// KMS client using AWS JSON protocol.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct KmsClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl KmsClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "kms".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("kms") {
            override_url.to_string()
        } else {
            format!("https://kms.{}.amazonaws.com", self.credentials.region())
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
            .request(Method::POST, &url)
            .host(&format!("kms.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", format!("TrentService.{}", target))
            .content_type_amz_json()
            .content_sha256(&body)
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_json(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(&body))
    }

    async fn send_no_response(
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
            .request(Method::POST, &url)
            .host(&format!("kms.{}.amazonaws.com", self.credentials.region()))
            .header("X-Amz-Target", format!("TrentService.{}", target))
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
                        Self::map_kms_error(status, text, operation, resource, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse KMS error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_kms_error(
        status: StatusCode,
        body: &str,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        let parsed: std::result::Result<KmsErrorResponse, _> = serde_json::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => {
                let c = e
                    .type_field_underscore
                    .or(e.type_field)
                    .unwrap_or_else(|| "UnknownErrorCode".into());
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
            | "NotAuthorized"
            | "UnrecognizedClientException"
            | "ExpiredTokenException" => ErrorData::RemoteAccessDenied {
                resource_type: "KmsKey".into(),
                resource_name: resource.into(),
            },
            // Throttling
            "ThrottlingException" | "TooManyRequestsException" => {
                ErrorData::RateLimitExceeded { message }
            }
            // Service unavailable
            "ServiceUnavailable" | "InternalFailure" | "KMSInternalException" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Timeout
            "RequestTimeoutException" => ErrorData::Timeout { message },
            // Resource not found
            "NotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "KmsKey".into(),
                resource_name: resource.into(),
            },
            // Invalid state
            "KMSInvalidStateException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "KmsKey".into(),
                resource_name: resource.into(),
            },
            // Conflict / already exists - no direct KMS equivalent, but invalid request
            "InvalidRequestException" | "MalformedPolicyDocumentException" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "KmsKey".into(),
                    resource_name: resource.into(),
                }
            }
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "KmsKey".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "KmsKey".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "KmsKey".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("KMS operation failed: {}", message),
                    url: format!("kms.amazonaws.com"),
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
impl KmsApi for KmsClient {
    async fn create_key(&self, request: CreateKeyRequest) -> Result<KeyMetadata> {
        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize CreateKeyRequest".to_string(),
            },
        )?;

        let response: CreateKeyResponse = self
            .send_json("CreateKey", body, "CreateKey", "new-key")
            .await?;

        Ok(response.key_metadata)
    }

    async fn describe_key(&self, key_id: &str) -> Result<KeyMetadata> {
        let request = DescribeKeyRequest::builder()
            .key_id(key_id.to_string())
            .build();

        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize DescribeKeyRequest for key '{}'",
                    key_id
                ),
            },
        )?;

        let response: DescribeKeyResponse = self
            .send_json("DescribeKey", body, "DescribeKey", key_id)
            .await?;

        Ok(response.key_metadata)
    }

    async fn disable_key(&self, key_id: &str) -> Result<()> {
        let request = DisableKeyRequest::builder()
            .key_id(key_id.to_string())
            .build();

        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize DisableKeyRequest for key '{}'", key_id),
            },
        )?;

        self.send_no_response("DisableKey", body, "DisableKey", key_id)
            .await
    }

    async fn enable_key(&self, key_id: &str) -> Result<()> {
        let request = EnableKeyRequest::builder()
            .key_id(key_id.to_string())
            .build();

        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize EnableKeyRequest for key '{}'", key_id),
            },
        )?;

        self.send_no_response("EnableKey", body, "EnableKey", key_id)
            .await
    }

    async fn schedule_key_deletion(
        &self,
        key_id: &str,
        pending_window_in_days: Option<i32>,
    ) -> Result<ScheduleKeyDeletionResponse> {
        let request = ScheduleKeyDeletionRequest::builder()
            .key_id(key_id.to_string())
            .maybe_pending_window_in_days(pending_window_in_days)
            .build();

        let body = serde_json::to_string(&request).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize ScheduleKeyDeletionRequest for key '{}'",
                    key_id
                ),
            },
        )?;

        self.send_json("ScheduleKeyDeletion", body, "ScheduleKeyDeletion", key_id)
            .await
    }
}

// ---------------------------------------------------------------------------
// Error JSON mapping structs
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
struct KmsErrorResponse {
    #[serde(rename = "Type")]
    type_field: Option<String>,
    #[serde(rename = "__type")]
    type_field_underscore: Option<String>,
    #[serde(rename = "message")]
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Request / response payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct CreateKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_usage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_region: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<Tag>>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeKeyRequest {
    pub key_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_tokens: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct DisableKeyRequest {
    pub key_id: String,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct EnableKeyRequest {
    pub key_id: String,
}

#[derive(Debug, Clone, Serialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct ScheduleKeyDeletionRequest {
    pub key_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_window_in_days: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub tag_key: String,
    pub tag_value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateKeyResponse {
    pub key_metadata: KeyMetadata,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeKeyResponse {
    pub key_metadata: KeyMetadata,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct KeyMetadata {
    #[serde(rename = "AWSAccountId")]
    pub aws_account_id: Option<String>,
    pub key_id: String,
    pub arn: Option<String>,
    pub creation_date: Option<f64>,
    pub enabled: Option<bool>,
    pub description: Option<String>,
    pub key_usage: Option<String>,
    pub key_state: Option<String>,
    pub deletion_date: Option<f64>,
    pub valid_to: Option<f64>,
    pub origin: Option<String>,
    pub custom_key_store_id: Option<String>,
    pub cloud_hsm_cluster_id: Option<String>,
    pub expiration_model: Option<String>,
    pub key_manager: Option<String>,
    pub key_spec: Option<String>,
    pub encryption_algorithms: Option<Vec<String>>,
    pub signing_algorithms: Option<Vec<String>>,
    pub multi_region: Option<bool>,
    pub multi_region_configuration: Option<MultiRegionConfiguration>,
    pub pending_deletion_window_in_days: Option<i32>,
    pub mac_algorithms: Option<Vec<String>>,
    pub xks_key_configuration: Option<XksKeyConfigurationType>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MultiRegionConfiguration {
    pub multi_region_key_type: Option<String>,
    pub primary_key: Option<MultiRegionKey>,
    pub replica_keys: Option<Vec<MultiRegionKey>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MultiRegionKey {
    pub arn: Option<String>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct XksKeyConfigurationType {
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ScheduleKeyDeletionResponse {
    pub key_id: Option<String>,
    pub deletion_date: Option<f64>,
    pub key_state: Option<String>,
    pub pending_window_in_days: Option<i32>,
}
