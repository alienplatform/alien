//! One-token Bedrock `Converse` call, used to find out whether the account may invoke a model.
//!
//! The call costs one output token. It is only ever made with credentials the customer's admin
//! provides during setup; the management identity gets no invoke grant for it.

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsRequestSigner, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, RequestBuilderExt, Result};
use async_trait::async_trait;
use reqwest::Client;
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

/// What a one-token call says about invoking a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeCheck {
    /// The model answered.
    Verified,
    /// The cloud refused this model for this account: not enabled, no agreement, or no quota.
    Refused { code: String },
    /// The check says nothing about the model: the checker itself is not allowed to call Bedrock,
    /// the call was throttled (a new account can have a quota of 0, so a 429 is not a pass), or
    /// the service failed.
    NotChecked { reason: String },
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait BedrockRuntimeApi: Send + Sync + Debug {
    /// Ask `model_id` (a model or inference profile id) for one token.
    async fn check_invoke(&self, model_id: &str) -> InvokeCheck;
}

#[derive(Debug, Clone)]
pub struct BedrockRuntimeClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl BedrockRuntimeClient {
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn host(&self) -> String {
        format!("bedrock-runtime.{}.amazonaws.com", self.credentials.region())
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "bedrock".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn base_url(&self) -> String {
        self.credentials
            .get_service_endpoint_option("bedrock-runtime")
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("https://{}", self.host()))
    }
}

/// Reads the HTTP status and body of a failed call into an [`InvokeCheck`].
///
/// - 200: verified.
/// - 403 whose message says the caller "is not authorized to perform" an action: the checker's own
///   IAM is missing, so the model is not checked.
/// - 403 / `AccessDeniedException` otherwise ("You don't have access to the model ..."): refused.
/// - 404 and `ResourceNotFoundException` (model not offered in the region) and 400
///   `ValidationException` about the model id: refused.
/// - 429 / `ThrottlingException`: not checked, never a pass.
/// - anything else (5xx, network): not checked.
pub fn classify_converse_failure(http_status: u16, body: &str) -> InvokeCheck {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("message").or_else(|| v.get("Message")))
        .and_then(|m| m.as_str())
        .unwrap_or(body)
        .to_string();
    let code = parsed
        .as_ref()
        .and_then(|v| v.get("__type").or_else(|| v.get("code")))
        .and_then(|c| c.as_str())
        .map(|c| c.rsplit('#').next().unwrap_or(c).to_string())
        .unwrap_or_default();
    let lower = message.to_ascii_lowercase();
    let checker_not_allowed = lower.contains("is not authorized to perform")
        || lower.contains("explicit deny")
        || code == "UnrecognizedClientException"
        || code == "ExpiredTokenException";
    match http_status {
        429 => InvokeCheck::NotChecked {
            reason: "throttled".to_string(),
        },
        401 | 403 if checker_not_allowed => InvokeCheck::NotChecked {
            reason: "the checking credentials may not call Bedrock".to_string(),
        },
        403 => InvokeCheck::Refused {
            code: if code.is_empty() {
                "AccessDeniedException".to_string()
            } else {
                code
            },
        },
        404 => InvokeCheck::Refused {
            code: "ResourceNotFoundException".to_string(),
        },
        400 if code == "ValidationException"
            && (lower.contains("model") || lower.contains("inference profile")) =>
        {
            InvokeCheck::Refused { code }
        }
        _ if code == "ThrottlingException" => InvokeCheck::NotChecked {
            reason: "throttled".to_string(),
        },
        _ => InvokeCheck::NotChecked {
            reason: format!("HTTP {http_status}"),
        },
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl BedrockRuntimeApi for BedrockRuntimeClient {
    async fn check_invoke(&self, model_id: &str) -> InvokeCheck {
        if let Err(error) = self.credentials.ensure_fresh().await {
            return InvokeCheck::NotChecked {
                reason: format!("credentials: {error}"),
            };
        }
        let url = format!(
            "{}/model/{}/converse",
            self.base_url().trim_end_matches('/'),
            urlencoding::encode(model_id)
        );
        let body = serde_json::json!({
            "messages": [{"role": "user", "content": [{"text": "hi"}]}],
            "inferenceConfig": {"maxTokens": 1}
        })
        .to_string();
        let builder = self
            .client
            .post(url)
            .host(&self.host())
            .header("Content-Type", "application/json")
            .content_sha256(&body)
            .body(body);
        let signed = match builder.sign_aws_request(&self.sign_config()) {
            Ok(signed) => signed,
            Err(error) => {
                return InvokeCheck::NotChecked {
                    reason: format!("signing: {error}"),
                }
            }
        };
        let result: Result<serde_json::Value> = signed.send_json().await;
        match result {
            Ok(_) => InvokeCheck::Verified,
            Err(error) => match &error.error {
                Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text,
                    ..
                }) => classify_converse_failure(
                    *http_status,
                    http_response_text.as_deref().unwrap_or(""),
                ),
                _ => InvokeCheck::NotChecked {
                    reason: "no response".to_string(),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(code: &str) -> InvokeCheck {
        InvokeCheck::Refused {
            code: code.to_string(),
        }
    }

    #[test]
    fn a_model_the_account_may_not_use_is_refused() {
        let body = r#"{"message":"You don't have access to the model with the specified model ID."}"#;
        assert_eq!(
            classify_converse_failure(403, body),
            refused("AccessDeniedException")
        );
    }

    #[test]
    fn the_checkers_own_missing_iam_is_not_a_refusal() {
        let body = r#"{"message":"User: arn:aws:iam::1:user/x is not authorized to perform: bedrock:InvokeModel on resource: arn"}"#;
        assert!(matches!(
            classify_converse_failure(403, body),
            InvokeCheck::NotChecked { .. }
        ));
    }

    #[test]
    fn a_429_is_never_a_pass() {
        // A new account can have a quota of 0: Bedrock throttles instead of refusing.
        let check = classify_converse_failure(429, r#"{"message":"Too many requests"}"#);
        assert_eq!(
            check,
            InvokeCheck::NotChecked {
                reason: "throttled".to_string()
            }
        );
        assert_ne!(check, InvokeCheck::Verified);
    }

    #[test]
    fn a_model_not_offered_in_the_region_is_refused() {
        assert_eq!(
            classify_converse_failure(404, r#"{"message":"model not found"}"#),
            refused("ResourceNotFoundException")
        );
        let body = r#"{"__type":"com.amazon.bedrock#ValidationException","message":"The provided model identifier is invalid."}"#;
        assert_eq!(
            classify_converse_failure(400, body),
            refused("ValidationException")
        );
    }

    #[test]
    fn service_failures_say_nothing_about_the_model() {
        assert!(matches!(
            classify_converse_failure(503, "unavailable"),
            InvokeCheck::NotChecked { .. }
        ));
    }
}
