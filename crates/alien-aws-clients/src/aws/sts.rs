use crate::aws::AwsClientConfigExt;
use std::fmt::Debug;

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::AwsClientConfig;
use alien_client_core::{ErrorData, Result};
use alien_error::ContextError;
use async_trait::async_trait;
use bon::Builder;
use form_urlencoded;

#[cfg(feature = "test-utils")]
use mockall::automock;
use quick_xml;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait StsApi: Send + Sync + Debug {
    async fn assume_role(&self, request: AssumeRoleRequest) -> Result<AssumeRoleResponse>;
    async fn assume_role_with_web_identity(
        &self,
        request: AssumeRoleWithWebIdentityRequest,
    ) -> Result<AssumeRoleWithWebIdentityResponse>;
    async fn get_caller_identity(&self) -> Result<GetCallerIdentityResponse>;
}

/// AWS STS client using the new request/error abstractions.
#[derive(Debug, Clone)]
pub struct StsClient {
    client: Client,
    config: AwsClientConfig,
}

impl StsClient {
    pub fn new(client: Client, config: AwsClientConfig) -> Self {
        Self { client, config }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "sts".into(),
            region: self.config.region.clone(),
            credentials: self.config.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.config.get_service_endpoint_option("sts") {
            override_url.to_string()
        } else {
            format!("https://sts.{}.amazonaws.com", self.config.region)
        }
    }

    fn build_form_body(action: &str, version: &str, params: Vec<(String, String)>) -> String {
        let mut all = vec![
            ("Action".to_string(), action.to_string()),
            ("Version".to_string(), version.to_string()),
        ];
        all.extend(params);

        all.into_iter()
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    k,
                    form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
                )
            })
            .collect::<Vec<String>>()
            .join("&")
    }

    // ---- Internal helpers ------------------------------------------------
    async fn post_xml<T: DeserializeOwned + Send + 'static>(
        &self,
        body: String,
        operation_name: &str,
        resource_name: &str,
    ) -> Result<T> {
        let base_url = self.get_base_url();
        let url = format!("{}/", base_url.trim_end_matches('/'));
        let builder = self
            .client
            .post(&url)
            .content_type_form()
            .body(body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await;

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
                        Self::map_sts_error(status, text, operation_name, resource_name, &body)
                    {
                        Err(e.context(mapped))
                    } else {
                        // Couldn't parse STS error, use original error
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_sts_error(
        status: StatusCode,
        error_body: &str,
        operation: &str,
        resource_name: &str,
        request_body: &str,
    ) -> Option<ErrorData> {
        // Attempt to parse the canonical AWS error XML.
        let parsed_error: std::result::Result<StsErrorResponse, _> =
            quick_xml::de::from_str(error_body);

        let (error_code, error_message) = match parsed_error {
            Ok(e) => (
                e.error.code.unwrap_or_else(|| "UnknownErrorCode".into()),
                e.error.message.unwrap_or_else(|| "Unknown error".into()),
            ),
            Err(_) => {
                // If we can't parse the response, return None to use original error
                return None;
            }
        };

        Some(match error_code.as_str() {
            // Access & auth
            "AccessDenied"
            | "AccessDeniedException"
            | "UnauthorizedOperation"
            | "InvalidUserID.NotFound"
            | "AuthFailure"
            | "SignatureDoesNotMatch"
            | "TokenRefreshRequired"
            | "NotAuthorized"
            | "InvalidClientTokenId"
            | "MissingAuthenticationToken"
            | "OptInRequired" => ErrorData::RemoteAccessDenied {
                resource_type: "STS Resource".into(),
                resource_name: resource_name.into(),
            },

            // Rate limiting / throttling
            "Throttling" | "ThrottlingException" | "RequestLimitExceeded" => {
                ErrorData::RateLimitExceeded {
                    message: error_message,
                }
            }

            // Service unavailable
            "ServiceUnavailable" | "InternalFailure" | "ServiceFailure" => {
                ErrorData::RemoteServiceUnavailable {
                    message: error_message,
                }
            }

            // STS-specific errors
            "ExpiredToken" => ErrorData::AuthenticationError {
                message: "Security token has expired".into(),
            },

            "MalformedPolicyDocument" => ErrorData::InvalidInput {
                message: format!("Malformed policy document: {}", error_message),
                field_name: Some("PolicyDocument".into()),
            },

            "PackedPolicyTooLarge" => ErrorData::InvalidInput {
                message: format!("Policy document is too large: {}", error_message),
                field_name: Some("PolicyDocument".into()),
            },

            "RegionDisabled" => ErrorData::RemoteServiceUnavailable {
                message: format!("STS is not enabled in this region: {}", error_message),
            },

            "InvalidParameterValue" => ErrorData::InvalidInput {
                message: error_message,
                field_name: None,
            },

            // Generic fallback categories
            _ => match status {
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message: error_message,
                    resource_type: "STS Resource".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "STS Resource".into(),
                    resource_name: resource_name.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "STS Resource".into(),
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
                    message: format!("STS operation failed: {}", error_message),
                    url: "sts.amazonaws.com".into(),
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
impl StsApi for StsClient {
    async fn assume_role(&self, request: AssumeRoleRequest) -> Result<AssumeRoleResponse> {
        let mut params: Vec<(String, String)> = vec![
            ("RoleArn".to_string(), request.role_arn.clone()),
            (
                "RoleSessionName".to_string(),
                request.role_session_name.clone(),
            ),
        ];

        if let Some(duration) = request.duration_seconds {
            params.push(("DurationSeconds".to_string(), duration.to_string()));
        }
        if let Some(ref external_id) = request.external_id {
            params.push(("ExternalId".to_string(), external_id.clone()));
        }
        if let Some(ref policy) = request.policy {
            params.push(("Policy".to_string(), policy.clone()));
        }
        if let Some(ref serial_number) = request.serial_number {
            params.push(("SerialNumber".to_string(), serial_number.clone()));
        }
        if let Some(ref token_code) = request.token_code {
            params.push(("TokenCode".to_string(), token_code.clone()));
        }
        if let Some(ref source_identity) = request.source_identity {
            params.push(("SourceIdentity".to_string(), source_identity.clone()));
        }

        // Handle policy ARNs array
        if let Some(ref policy_arns) = request.policy_arns {
            for (i, policy_arn) in policy_arns.iter().enumerate() {
                params.push((
                    format!("PolicyArns.member.{}.arn", i + 1),
                    policy_arn.clone(),
                ));
            }
        }

        // Handle tags array
        if let Some(ref tags) = request.tags {
            for (i, tag) in tags.iter().enumerate() {
                params.push((format!("Tags.member.{}.Key", i + 1), tag.key.clone()));
                params.push((format!("Tags.member.{}.Value", i + 1), tag.value.clone()));
            }
        }

        // Handle transitive tag keys array
        if let Some(ref transitive_tag_keys) = request.transitive_tag_keys {
            for (i, key) in transitive_tag_keys.iter().enumerate() {
                params.push((format!("TransitiveTagKeys.member.{}", i + 1), key.clone()));
            }
        }

        let body = Self::build_form_body("AssumeRole", "2011-06-15", params);
        self.post_xml(body, "AssumeRole", &request.role_arn).await
    }

    async fn assume_role_with_web_identity(
        &self,
        request: AssumeRoleWithWebIdentityRequest,
    ) -> Result<AssumeRoleWithWebIdentityResponse> {
        let mut params: Vec<(String, String)> = vec![
            ("RoleArn".to_string(), request.role_arn.clone()),
            (
                "RoleSessionName".to_string(),
                request.role_session_name.clone(),
            ),
            (
                "WebIdentityToken".to_string(),
                request.web_identity_token.clone(),
            ),
        ];

        if let Some(duration) = request.duration_seconds {
            params.push(("DurationSeconds".to_string(), duration.to_string()));
        }
        if let Some(ref policy) = request.policy {
            params.push(("Policy".to_string(), policy.clone()));
        }
        if let Some(ref provider_id) = request.provider_id {
            params.push(("ProviderId".to_string(), provider_id.clone()));
        }

        // Handle policy ARNs array
        if let Some(ref policy_arns) = request.policy_arns {
            for (i, policy_arn) in policy_arns.iter().enumerate() {
                params.push((
                    format!("PolicyArns.member.{}.arn", i + 1),
                    policy_arn.clone(),
                ));
            }
        }

        let body = Self::build_form_body("AssumeRoleWithWebIdentity", "2011-06-15", params);
        self.post_xml(body, "AssumeRoleWithWebIdentity", &request.role_arn)
            .await
    }

    async fn get_caller_identity(&self) -> Result<GetCallerIdentityResponse> {
        let params = vec![];
        let body = Self::build_form_body("GetCallerIdentity", "2011-06-15", params);
        self.post_xml(body, "GetCallerIdentity", "caller").await
    }
}

// -------------------------------------------------------------------------
// Error XML structs (PascalCase matching AWS STS)
// -------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct StsErrorResponse {
    pub error: StsErrorDetails,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct StsErrorDetails {
    pub code: Option<String>,
    pub message: Option<String>,
}

// -------------------------------------------------------------------------
// Request / response payloads
// -------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct AssumeRoleRequest {
    /// The ARN of the role to assume
    pub role_arn: String,
    /// An identifier for the assumed role session
    pub role_session_name: String,
    /// The duration, in seconds, of the role session (900-43200)
    pub duration_seconds: Option<i32>,
    /// A unique identifier used when assuming a role in another account
    pub external_id: Option<String>,
    /// An IAM policy in JSON format to use as an inline session policy
    pub policy: Option<String>,
    /// The Amazon Resource Names (ARNs) of IAM managed policies to use as managed session policies
    pub policy_arns: Option<Vec<String>>,
    /// The identification number of the MFA device
    pub serial_number: Option<String>,
    /// The value provided by the MFA device
    pub token_code: Option<String>,
    /// The source identity specified by the principal
    pub source_identity: Option<String>,
    /// A list of session tags
    pub tags: Option<Vec<Tag>>,
    /// A list of keys for session tags that you want to set as transitive
    pub transitive_tag_keys: Option<Vec<String>>,
}

#[derive(Serialize, Debug, Clone, Builder)]
#[serde(rename_all = "PascalCase")]
pub struct AssumeRoleWithWebIdentityRequest {
    /// The ARN of the role to assume
    pub role_arn: String,
    /// An identifier for the assumed role session
    pub role_session_name: String,
    /// The OAuth 2.0 access token or OpenID Connect ID token
    pub web_identity_token: String,
    /// The duration, in seconds, of the role session (900-43200)
    pub duration_seconds: Option<i32>,
    /// An IAM policy in JSON format to use as an inline session policy
    pub policy: Option<String>,
    /// The Amazon Resource Names (ARNs) of IAM managed policies to use as managed session policies
    pub policy_arns: Option<Vec<String>>,
    /// The fully qualified host component of the domain name of the OAuth 2.0 identity provider
    pub provider_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct AssumeRoleResponse {
    pub assume_role_result: AssumeRoleResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct AssumeRoleWithWebIdentityResponse {
    pub assume_role_with_web_identity_result: AssumeRoleWithWebIdentityResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct AssumeRoleWithWebIdentityResult {
    pub assumed_role_user: AssumedRoleUser,
    pub credentials: Credentials,
    pub packed_policy_size: Option<i32>,
    pub provider: Option<String>,
    pub audience: Option<String>,
    pub source_identity: Option<String>,
    pub subject_from_web_identity_token: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct AssumeRoleResult {
    pub assumed_role_user: AssumedRoleUser,
    pub credentials: Credentials,
    pub packed_policy_size: Option<i32>,
    pub source_identity: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct AssumedRoleUser {
    pub arn: String,
    pub assumed_role_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    pub expiration: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetCallerIdentityResponse {
    pub get_caller_identity_result: GetCallerIdentityResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetCallerIdentityResult {
    pub arn: Option<String>,
    pub user_id: Option<String>,
    pub account: Option<String>,
}
