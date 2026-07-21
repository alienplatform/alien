use crate::aws::AwsClientConfigExt;
use std::fmt::Debug;

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::{AwsClientConfig, AwsCredentials};
use alien_client_core::{redact_request_body, ErrorData, RequestBuilderExt, Result};
use alien_error::ContextError;
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
    async fn get_session_token(
        &self,
        duration_seconds: Option<i32>,
    ) -> Result<GetSessionTokenResponse>;
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

    async fn sign_config(&self) -> Result<AwsSignConfig> {
        let config = if matches!(self.config.credentials, AwsCredentials::WebIdentity { .. }) {
            self.config.get_web_identity_credentials().await?
        } else {
            self.config.clone()
        };

        Ok(AwsSignConfig {
            service_name: "sts".into(),
            region: config.region.clone(),
            credentials: config.get_credentials(),
            signing_region: None,
        })
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

        // AssumeRoleWithWebIdentity is authenticated by its OIDC token and
        // explicitly does not require AWS credentials. Signing it with dummy
        // credentials makes the otherwise valid exchange fail at AWS.
        let result = if operation_name == "AssumeRoleWithWebIdentity" {
            builder.with_retry().send_xml::<T>().await
        } else {
            let sign_config = self.sign_config().await?;
            crate::aws::aws_request_utils::sign_send_xml(builder, &sign_config).await
        };
        // Web-identity request bodies contain the projected identity token.
        // Strip it from every error-chain layer before adding STS context.
        let result = if operation_name == "AssumeRoleWithWebIdentity" {
            redact_request_body(result)
        } else {
            result
        };

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
                    if let Some(mapped) = Self::map_sts_error(
                        status,
                        text,
                        operation_name,
                        resource_name,
                        (operation_name != "AssumeRoleWithWebIdentity").then_some(body.as_str()),
                    ) {
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
        request_body: Option<&str>,
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
                    message: format!("STS {operation} failed: {error_message}"),
                    url: "sts.amazonaws.com".into(),
                    http_status: status.as_u16(),
                    http_response_text: Some(error_body.into()),
                    http_request_text: request_body.map(str::to_string),
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

    async fn get_session_token(
        &self,
        duration_seconds: Option<i32>,
    ) -> Result<GetSessionTokenResponse> {
        let params = duration_seconds
            .map(|duration| vec![("DurationSeconds".to_string(), duration.to_string())])
            .unwrap_or_default();
        let body = Self::build_form_body("GetSessionToken", "2011-06-15", params);
        self.post_xml(body, "GetSessionToken", "caller").await
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

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetSessionTokenResponse {
    pub get_session_token_result: GetSessionTokenResult,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GetSessionTokenResult {
    pub credentials: Credentials,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{AwsServiceOverrides, AwsWebIdentityConfig};
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn get_session_token_exchanges_static_keys_for_expiring_credentials() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test STS server");
        let endpoint = format!("http://{}", listener.local_addr().expect("local addr"));
        let server_observed = observed.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept STS request");
            let (headers, body) = read_http_request(&mut stream);
            let authorization = headers
                .lines()
                .find(|line| line.to_ascii_lowercase().starts_with("authorization:"))
                .unwrap_or_default()
                .to_string();
            server_observed
                .lock()
                .expect("observed requests lock")
                .push(ObservedRequest {
                    body,
                    authorization,
                });
            write_xml_response(&mut stream, get_session_token_response());
        });
        let config = AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIATESTACCESS".to_string(),
                secret_access_key: "test-secret".to_string(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("sts".to_string(), endpoint)]),
            }),
        };

        let response = StsClient::new(Client::new(), config)
            .get_session_token(Some(3600))
            .await
            .expect("get session token should succeed");
        assert_eq!(
            response.get_session_token_result.credentials.access_key_id,
            "ASIASESSIONACCESS"
        );
        server.join().expect("server thread should finish");
        let observed = observed.lock().expect("observed requests lock");
        assert!(observed[0].body.contains("Action=GetSessionToken"));
        assert!(observed[0].body.contains("DurationSeconds=3600"));
        assert!(observed[0].authorization.contains("AKIATESTACCESS"));
    }

    #[tokio::test]
    async fn get_caller_identity_exchanges_web_identity_before_signing() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let (endpoint, server) = start_sts_test_server(observed.clone());

        let token_file = tempfile::NamedTempFile::new().expect("create token file");
        std::fs::write(token_file.path(), "test-web-identity-token").expect("write token file");

        let config = AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
            credentials: AwsCredentials::WebIdentity {
                config: AwsWebIdentityConfig {
                    role_arn: "arn:aws:iam::123456789012:role/test-role".to_string(),
                    session_name: Some("test-session".to_string()),
                    web_identity_token_file: token_file.path().display().to_string(),
                    duration_seconds: Some(900),
                },
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("sts".to_string(), endpoint)]),
            }),
        };

        let response = StsClient::new(Client::new(), config)
            .get_caller_identity()
            .await
            .expect("get caller identity should use exchanged credentials");

        assert_eq!(
            response.get_caller_identity_result.account.as_deref(),
            Some("123456789012")
        );

        server.join().expect("server thread should finish");
        let observed = observed.lock().expect("observed requests lock");
        assert_eq!(observed.len(), 2);
        assert!(observed[0]
            .body
            .contains("Action=AssumeRoleWithWebIdentity"));
        assert!(
            observed[0].authorization.is_empty(),
            "AssumeRoleWithWebIdentity must not be SigV4 signed"
        );
        assert!(observed[1].body.contains("Action=GetCallerIdentity"));
        assert!(
            observed[1].authorization.contains("ASIATESTACCESS"),
            "GetCallerIdentity must be signed with credentials returned by AssumeRoleWithWebIdentity, got: {}",
            observed[1].authorization
        );
    }

    #[tokio::test]
    async fn assume_role_sends_the_exact_inline_session_policy() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test STS server");
        let endpoint = format!("http://{}", listener.local_addr().expect("local addr"));
        let server_observed = observed.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept STS request");
            let (headers, body) = read_http_request(&mut stream);
            server_observed
                .lock()
                .expect("observed requests lock")
                .push(ObservedRequest {
                    body,
                    authorization: headers,
                });
            write_xml_response(&mut stream, assume_role_response());
        });
        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Action": ["s3:ListBucket"],
                "Resource": ["arn:aws:s3:::requested-bucket"]
            }]
        })
        .to_string();
        let config = AwsClientConfig {
            account_id: "111122223333".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIATESTACCESS".to_string(),
                secret_access_key: "test-secret".to_string(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("sts".to_string(), endpoint)]),
            }),
        };

        StsClient::new(Client::new(), config)
            .assume_role(
                AssumeRoleRequest::builder()
                    .role_arn("arn:aws:iam::123456789012:role/remote-management".to_string())
                    .role_session_name("remote-storage-session".to_string())
                    .duration_seconds(3600)
                    .policy(policy.clone())
                    .build(),
            )
            .await
            .expect("AssumeRole should succeed");
        server.join().expect("server thread should finish");

        let observed = observed.lock().expect("observed requests lock");
        let form = form_urlencoded::parse(observed[0].body.as_bytes())
            .into_owned()
            .collect::<HashMap<_, _>>();
        assert_eq!(form.get("Action").map(String::as_str), Some("AssumeRole"));
        assert_eq!(form.get("Policy"), Some(&policy));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(form.get("Policy").unwrap()).unwrap(),
            serde_json::json!({
                "Version": "2012-10-17",
                "Statement": [{
                    "Effect": "Allow",
                    "Action": ["s3:ListBucket"],
                    "Resource": ["arn:aws:s3:::requested-bucket"]
                }]
            })
        );
    }

    #[tokio::test]
    async fn web_identity_errors_never_retain_the_request_token() {
        const SENTINEL: &str = "secret-web-identity-sentinel";
        const POLICY: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:GetObject"],"Resource":["arn:aws:s3:::requested-bucket/*"]}]}"#;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test STS server");
        let endpoint = format!("http://{}", listener.local_addr().expect("local addr"));
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept STS request");
            let (headers, body) = read_http_request(&mut stream);
            assert!(body.contains(SENTINEL), "server should receive the token");
            let form = form_urlencoded::parse(body.as_bytes())
                .into_owned()
                .collect::<HashMap<_, _>>();
            assert_eq!(form.get("Policy").map(String::as_str), Some(POLICY));
            assert!(
                !headers
                    .lines()
                    .any(|line| line.to_ascii_lowercase().starts_with("authorization:")),
                "AssumeRoleWithWebIdentity must not be SigV4 signed"
            );
            let response_body = "upstream failure";
            let response = format!(
                "HTTP/1.1 500 Internal Server Error\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write error response");
        });
        let config = AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "UNSIGNED".to_string(),
                secret_access_key: "UNSIGNED".to_string(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("sts".to_string(), endpoint)]),
            }),
        };
        let error = StsClient::new(Client::new(), config)
            .assume_role_with_web_identity(
                AssumeRoleWithWebIdentityRequest::builder()
                    .role_arn("arn:aws:iam::123456789012:role/test".to_string())
                    .role_session_name("test".to_string())
                    .web_identity_token(SENTINEL.to_string())
                    .policy(POLICY.to_string())
                    .build(),
            )
            .await
            .expect_err("upstream error should propagate");
        server.join().expect("server thread should finish");
        let serialized = serde_json::to_string(&error).expect("serialize error chain");
        assert!(!serialized.contains(SENTINEL));
        assert!(!serialized.contains("WebIdentityToken"));
    }

    #[derive(Debug)]
    struct ObservedRequest {
        body: String,
        authorization: String,
    }

    fn start_sts_test_server(
        observed: Arc<Mutex<Vec<ObservedRequest>>>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test STS server");
        let endpoint = format!("http://{}", listener.local_addr().expect("local addr"));
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept STS request");
                let (headers, body) = read_http_request(&mut stream);
                let authorization = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("authorization")
                                .then(|| value.trim())
                        })
                    })
                    .unwrap_or_default()
                    .to_string();
                observed
                    .lock()
                    .expect("observed requests lock")
                    .push(ObservedRequest {
                        body: body.clone(),
                        authorization: authorization.clone(),
                    });

                if body.contains("Action=AssumeRoleWithWebIdentity") {
                    write_xml_response(&mut stream, assume_role_with_web_identity_response());
                } else if body.contains("Action=GetCallerIdentity")
                    && authorization.contains("ASIATESTACCESS")
                {
                    write_xml_response(&mut stream, get_caller_identity_response());
                } else {
                    write_forbidden_response(&mut stream);
                }
            }
        });
        (endpoint, server)
    }

    fn read_http_request(stream: &mut TcpStream) -> (String, String) {
        let mut buffer = Vec::new();
        let mut scratch = [0_u8; 4096];
        let header_end;
        loop {
            let read = stream.read(&mut scratch).expect("read request");
            assert!(read > 0, "connection closed before headers");
            buffer.extend_from_slice(&scratch[..read]);
            if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = position + 4;
                break;
            }
        }

        let headers = String::from_utf8(buffer[..header_end].to_vec()).expect("headers utf8");
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .expect("content-length header");

        while buffer.len() < header_end + content_length {
            let read = stream.read(&mut scratch).expect("read request body");
            assert!(read > 0, "connection closed before body");
            buffer.extend_from_slice(&scratch[..read]);
        }

        let body = String::from_utf8(buffer[header_end..header_end + content_length].to_vec())
            .expect("body utf8");
        (headers, body)
    }

    fn write_xml_response(stream: &mut TcpStream, body: String) {
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write XML response");
    }

    fn write_forbidden_response(stream: &mut TcpStream) {
        let body = r#"<ErrorResponse><Error><Code>AccessDenied</Code><Message>denied</Message></Error></ErrorResponse>"#;
        let response = format!(
            "HTTP/1.1 403 Forbidden\r\ncontent-type: text/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write forbidden response");
    }

    fn assume_role_with_web_identity_response() -> String {
        r#"<AssumeRoleWithWebIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleWithWebIdentityResult>
    <SubjectFromWebIdentityToken>system:serviceaccount:test:agent</SubjectFromWebIdentityToken>
    <Audience>sts.amazonaws.com</Audience>
    <Provider>provider</Provider>
    <AssumedRoleUser>
      <Arn>arn:aws:sts::123456789012:assumed-role/test-role/test-session</Arn>
      <AssumedRoleId>AROA:test-session</AssumedRoleId>
    </AssumedRoleUser>
    <Credentials>
      <AccessKeyId>ASIATESTACCESS</AccessKeyId>
      <SecretAccessKey>test-secret</SecretAccessKey>
      <SessionToken>test-session-token</SessionToken>
      <Expiration>2026-05-27T11:00:00Z</Expiration>
    </Credentials>
  </AssumeRoleWithWebIdentityResult>
  <ResponseMetadata><RequestId>request-1</RequestId></ResponseMetadata>
</AssumeRoleWithWebIdentityResponse>"#
            .to_string()
    }

    fn assume_role_response() -> String {
        r#"<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleResult>
    <AssumedRoleUser>
      <Arn>arn:aws:sts::123456789012:assumed-role/remote-management/remote-storage-session</Arn>
      <AssumedRoleId>AROA:remote-storage-session</AssumedRoleId>
    </AssumedRoleUser>
    <Credentials>
      <AccessKeyId>ASIAREMOTEACCESS</AccessKeyId>
      <SecretAccessKey>remote-secret</SecretAccessKey>
      <SessionToken>remote-session-token</SessionToken>
      <Expiration>2030-01-01T01:00:00Z</Expiration>
    </Credentials>
  </AssumeRoleResult>
  <ResponseMetadata><RequestId>request-assume-role</RequestId></ResponseMetadata>
</AssumeRoleResponse>"#
            .to_string()
    }

    fn get_caller_identity_response() -> String {
        r#"<GetCallerIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <GetCallerIdentityResult>
    <Arn>arn:aws:sts::123456789012:assumed-role/test-role/test-session</Arn>
    <UserId>AROA:test-session</UserId>
    <Account>123456789012</Account>
  </GetCallerIdentityResult>
  <ResponseMetadata><RequestId>request-2</RequestId></ResponseMetadata>
</GetCallerIdentityResponse>"#
            .to_string()
    }

    fn get_session_token_response() -> String {
        r#"<GetSessionTokenResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <GetSessionTokenResult>
    <Credentials>
      <AccessKeyId>ASIASESSIONACCESS</AccessKeyId>
      <SecretAccessKey>session-secret</SecretAccessKey>
      <SessionToken>session-token</SessionToken>
      <Expiration>2030-01-01T01:00:00Z</Expiration>
    </Credentials>
  </GetSessionTokenResult>
  <ResponseMetadata><RequestId>request-session</RequestId></ResponseMetadata>
</GetSessionTokenResponse>"#
            .to_string()
    }
}
