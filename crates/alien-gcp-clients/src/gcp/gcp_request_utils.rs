use crate::gcp::GcpClientConfigExt;
use alien_client_core::RequestBuilderExt;
use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use reqwest::RequestBuilder;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::debug;

/// Configuration needed to add Bearer authentication to a GCP request.
#[derive(Debug, Clone)]
pub struct GcpAuthConfig {
    /// A valid OAuth 2.0 bearer token (e.g. generated from a service account JWT).
    pub bearer_token: String,
}

/// GCP API error response structure
#[derive(Debug, Deserialize)]
pub struct GcpErrorResponse {
    pub error: GcpErrorDetails,
}

#[derive(Debug, Deserialize)]
pub struct GcpErrorDetails {
    pub code: u16,
    pub message: String,
    #[serde(default)]
    pub errors: Vec<GcpErrorItem>,
    /// Optional status field that GCP APIs sometimes include
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GcpErrorItem {
    pub message: String,
    pub domain: String,
    pub reason: String,
}

/// Extension trait that enables adding `Authorization: Bearer <token>` to
/// `reqwest::RequestBuilder` instances in a fallible way (mirrors the AWS
/// signing trait so that call-sites look consistent).
pub trait GcpRequestAuthenticator: Sized {
    /// Attach the bearer token taken from `config` to the request and return a
    /// new builder so that further combinators (e.g. `with_retry`, `send_json`)
    /// can be chained.
    fn auth_gcp_request(self, config: &GcpAuthConfig) -> Result<Self>;
}

impl GcpRequestAuthenticator for reqwest::RequestBuilder {
    fn auth_gcp_request(self, config: &GcpAuthConfig) -> Result<Self> {
        // Building the request upfront allows us to surface errors (e.g. invalid
        // URL) at the authentication step just like we do for AWS. We immediately
        // turn the request back into a builder so that the caller keeps the same
        // ergonomic chaining pattern.
        let (client, req_result) = self.build_split();
        let mut reqwest_request =
            req_result
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Unable to build reqwest::Request prior to attaching bearer token"
                        .to_string(),
                })?;

        // Add/override the Authorization header.
        reqwest_request.headers_mut().insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", config.bearer_token))
                .into_alien_error()
                .context(ErrorData::InvalidClientConfig {
                    message: "Invalid bearer token provided".to_string(),
                    errors: None,
                })?,
        );

        // debug!(
        //     "GCP Request - Method: {}, URL: {}, Headers: {:?}, Body: {:?}",
        //     reqwest_request.method(),
        //     reqwest_request.url(),
        //     reqwest_request.headers(),
        //     reqwest_request.body().map(|body| {
        //         match body.as_bytes() {
        //             Some(bytes) => {
        //                 // Try to convert to string, fallback to byte count if not valid UTF-8
        //                 match std::str::from_utf8(bytes) {
        //                     Ok(text) => text.to_string(),
        //                     Err(_) => format!("<{} bytes (binary)>", bytes.len()),
        //                 }
        //             },
        //             None => "<no body>".to_string(),
        //         }
        //     })
        // );

        // Recreate a RequestBuilder so the caller can keep chaining.
        let new_builder = reqwest::RequestBuilder::from_parts(client, reqwest_request);
        Ok(new_builder)
    }
}

/// Parse GCP error response and map to appropriate ErrorData variant
pub fn map_gcp_error(
    http_status: u16,
    response_text: &str,
    url: &str,
    operation: &str,
    resource_name: &str,
    resource_type: &str,
    request_body: Option<String>,
) -> ErrorData {
    let resource_type = resource_type.to_string();
    let resource_name = resource_name.to_string();

    // Try to parse GCP error message if available
    let gcp_error_message = serde_json::from_str::<GcpErrorResponse>(response_text)
        .map(|gcp_error| gcp_error.error.message)
        .ok();

    match http_status {
        404 => ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
        },
        409 => ErrorData::RemoteResourceConflict {
            message: gcp_error_message.unwrap_or_else(|| {
                format!(
                    "Conflict during {} for {}: {}",
                    operation, resource_name, response_text
                )
            }),
            resource_type,
            resource_name,
        },
        403 => ErrorData::RemoteAccessDenied {
            resource_type,
            resource_name,
        },
        400 => ErrorData::InvalidInput {
            message: gcp_error_message.unwrap_or_else(|| response_text.to_string()),
            field_name: None,
        },
        429 => ErrorData::RateLimitExceeded {
            message: gcp_error_message
                .unwrap_or_else(|| format!("Rate limit exceeded for {}", resource_type)),
        },
        503 => ErrorData::RemoteServiceUnavailable {
            message: gcp_error_message
                .unwrap_or_else(|| format!("{} service unavailable", resource_type)),
        },
        _ => ErrorData::HttpResponseError {
            message: gcp_error_message.map_or_else(
                || format!("Request failed with HTTP {}: Unknown error", http_status),
                |msg| format!("Request failed with HTTP {}: {}", http_status, msg),
            ),
            url: url.to_string(),
            http_status,
            http_request_text: request_body,
            http_response_text: Some(response_text.to_string()),
        },
    }
}

/// Helper to map Result<T> from generic HTTP responses to GCP-specific errors
pub fn map_gcp_result<T>(
    result: Result<T>,
    operation: &str,
    resource_name: &str,
    resource_type: &str,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(e) => {
            if let Some(ErrorData::HttpResponseError {
                http_status,
                url,
                http_request_text,
                http_response_text: Some(ref text),
                ..
            }) = &e.error
            {
                let mapped = map_gcp_error(
                    *http_status,
                    text,
                    url,
                    operation,
                    resource_name,
                    resource_type,
                    http_request_text.clone(),
                );
                Err(e.context(mapped))
            } else {
                Err(e)
            }
        }
    }
}

// =============================================================================================
// Generic helpers: authenticate + retry + send (JSON / XML / no-body)
// Mirrors the helpers in `aws_request_utils.rs` so that call-sites between
// clouds stay as similar as possible.
// =============================================================================================

/// Attach the bearer token, apply the retry policy from [`RequestBuilderExt`]
/// and deserialize a JSON response into `T` with GCP-specific error mapping.
pub async fn auth_send_json<T: DeserializeOwned + Send + 'static>(
    builder: RequestBuilder,
    config: &GcpAuthConfig,
    operation: &str,
    resource_name: &str,
    resource_type: &str,
) -> Result<T> {
    let result = builder
        .auth_gcp_request(config)?
        .with_retry()
        .send_json::<T>()
        .await;
    map_gcp_result(result, operation, resource_name, resource_type)
}

/// Attach the bearer token, apply retries and expect no response body (return `()`
/// on HTTP success) with GCP-specific error mapping.
pub async fn auth_send_no_response(
    builder: RequestBuilder,
    config: &GcpAuthConfig,
    operation: &str,
    resource_name: &str,
    resource_type: &str,
) -> Result<()> {
    let result = builder
        .auth_gcp_request(config)?
        .with_retry()
        .send_no_response()
        .await;

    map_gcp_result(result, operation, resource_name, resource_type)
}
