//! Alien Client SDK
//!
//! Auto-generated from OpenAPI spec with custom error conversion support.
//!
//! ## Error Handling
//!
//! For SDK API calls, use `SdkResultExt::into_sdk_error()` instead of
//! `.into_alien_error()` to preserve structured API error information:
//!
//! ```ignore
//! use alien_client_sdk::SdkResultExt;
//!
//! // ✅ Good: preserves API error code, message, retryable flag
//! client.some_method().send().await.into_sdk_error().context(...)?
//!
//! // ❌ Bad: loses structured error information
//! client.some_method().send().await.into_alien_error().context(...)?
//! ```
//!
//! For non-SDK errors (serde, std, etc.), continue using `.into_alien_error()`.

include!(concat!(env!("OUT_DIR"), "/codegen.rs"));

use alien_error::{AlienError, GenericError};

/// Extension trait for converting SDK API results to `AlienError`.
///
/// This properly extracts error information from progenitor's error types,
/// preserving API error details that would be lost with `.into_alien_error()`.
///
/// ## When to use
///
/// Use `into_sdk_error()` for SDK API calls:
/// ```ignore
/// client.sync_acquire().send().await.into_sdk_error().context(...)?
/// ```
///
/// Continue using `into_alien_error()` for non-SDK errors (serde, std, etc.):
/// ```ignore
/// serde_json::to_value(&data).into_alien_error().context(...)?
/// ```
///
/// ## What it preserves
///
/// When the API returns an error response, `into_sdk_error()` preserves:
/// - `code`: The API error code (e.g., "AGENT_NOT_FOUND")
/// - `message`: The error message
/// - `retryable`: Whether the operation can be retried
/// - `context`: Additional error context as JSON
/// - `source`: Nested error chain
/// - HTTP status code
pub trait SdkResultExt<T> {
    /// Convert SDK result to `AlienError` result, preserving API error details.
    fn into_sdk_error(self) -> Result<T, AlienError<GenericError>>;
}

impl<T> SdkResultExt<ResponseValue<T>> for Result<ResponseValue<T>, Error<types::ApiError>> {
    fn into_sdk_error(self) -> Result<ResponseValue<T>, AlienError<GenericError>> {
        self.map_err(convert_sdk_error)
    }
}

/// Convert a progenitor SDK error to AlienError, preserving all details.
pub fn convert_sdk_error(err: Error<types::ApiError>) -> AlienError<GenericError> {
    match err {
        // API returned a documented error response with ApiError body
        // This is the main case where we gain value over .into_alien_error()
        Error::ErrorResponse(response) => {
            let status = response.status().as_u16();
            let api_error = response.into_inner();

            AlienError {
                code: api_error.code.to_string(),
                message: api_error.message.to_string(),
                context: api_error.context,
                retryable: api_error.retryable,
                internal: false, // API errors sent to clients are external by nature
                http_status_code: Some(status),
                source: api_error.source.and_then(parse_source_error),
                error: Some(GenericError {
                    message: api_error.message.to_string(),
                }),
            }
        }

        // Network/connection error - typically retryable
        Error::CommunicationError(reqwest_err) => {
            let retryable =
                reqwest_err.is_connect() || reqwest_err.is_timeout() || reqwest_err.is_request();

            AlienError {
                code: "COMMUNICATION_ERROR".to_string(),
                message: format!("Communication Error: {}", reqwest_err),
                context: None,
                retryable,
                internal: false,
                http_status_code: reqwest_err.status().map(|s| s.as_u16()),
                source: build_reqwest_source(&reqwest_err),
                error: Some(GenericError {
                    message: format!("Communication Error: {}", reqwest_err),
                }),
            }
        }

        // Request validation failed (client-side, before sending)
        Error::InvalidRequest(msg) => AlienError {
            code: "INVALID_REQUEST".to_string(),
            message: format!("Invalid Request: {}", msg),
            context: None,
            retryable: false,
            internal: false,
            http_status_code: Some(400),
            source: None,
            error: Some(GenericError {
                message: format!("Invalid Request: {}", msg),
            }),
        },

        // Failed to read response body
        Error::ResponseBodyError(reqwest_err) => AlienError {
            code: "RESPONSE_BODY_ERROR".to_string(),
            message: format!("Error reading response body: {}", reqwest_err),
            context: None,
            retryable: true, // Transient network issue
            internal: false,
            http_status_code: reqwest_err.status().map(|s| s.as_u16()),
            source: build_reqwest_source(&reqwest_err),
            error: Some(GenericError {
                message: format!("Error reading response body: {}", reqwest_err),
            }),
        },

        // Response body couldn't be parsed as expected type
        // Include raw body in context for debugging
        Error::InvalidResponsePayload(bytes, json_err) => {
            let raw_body = String::from_utf8_lossy(&bytes);
            let truncated = if raw_body.len() > 1000 {
                format!(
                    "{}...(truncated {} bytes)",
                    &raw_body[..1000],
                    raw_body.len() - 1000
                )
            } else {
                raw_body.to_string()
            };

            AlienError {
                code: "INVALID_RESPONSE_PAYLOAD".to_string(),
                message: format!("Failed to parse response: {}", json_err),
                context: Some(serde_json::json!({
                    "parseError": json_err.to_string(),
                    "responseBody": truncated,
                })),
                retryable: false,
                internal: false,
                http_status_code: None,
                source: Some(Box::new(AlienError::new(GenericError {
                    message: json_err.to_string(),
                }))),
                error: Some(GenericError {
                    message: format!("Failed to parse response: {}", json_err),
                }),
            }
        }

        // WebSocket upgrade error
        Error::InvalidUpgrade(reqwest_err) => AlienError {
            code: "INVALID_UPGRADE".to_string(),
            message: format!("Connection upgrade failed: {}", reqwest_err),
            context: None,
            retryable: false,
            internal: false,
            http_status_code: reqwest_err.status().map(|s| s.as_u16()),
            source: build_reqwest_source(&reqwest_err),
            error: Some(GenericError {
                message: format!("Connection upgrade failed: {}", reqwest_err),
            }),
        },

        // Response with status code not in OpenAPI spec
        Error::UnexpectedResponse(response) => {
            let status = response.status().as_u16();
            AlienError {
                code: "UNEXPECTED_RESPONSE".to_string(),
                message: format!(
                    "Unexpected response: {} {}",
                    status,
                    response.status().canonical_reason().unwrap_or("Unknown")
                ),
                context: Some(serde_json::json!({
                    "status": status,
                    "url": response.url().to_string(),
                })),
                retryable: status >= 500, // Server errors are typically retryable
                internal: false,
                http_status_code: Some(status),
                source: None,
                error: Some(GenericError {
                    message: format!("Unexpected response status: {}", status),
                }),
            }
        }

        // Custom hook error
        Error::Custom(msg) => AlienError {
            code: "SDK_HOOK_ERROR".to_string(),
            message: msg.clone(),
            context: None,
            retryable: false,
            internal: false,
            http_status_code: None,
            source: None,
            error: Some(GenericError { message: msg }),
        },
    }
}

/// Build a source error chain from a reqwest error
fn build_reqwest_source(err: &reqwest::Error) -> Option<Box<AlienError<GenericError>>> {
    // Walk the error chain and build AlienError source chain
    use std::error::Error;

    let mut sources = Vec::new();
    let mut current: Option<&(dyn Error + 'static)> = err.source();

    while let Some(src) = current {
        sources.push(src.to_string());
        current = src.source();
    }

    if sources.is_empty() {
        return None;
    }

    // Build chain from innermost to outermost
    let mut result: Option<Box<AlienError<GenericError>>> = None;
    for msg in sources.into_iter().rev() {
        let error = AlienError {
            code: "GENERIC_ERROR".to_string(),
            message: msg.clone(),
            context: None,
            retryable: false,
            internal: false,
            http_status_code: None,
            source: result,
            error: Some(GenericError { message: msg }),
        };
        result = Some(Box::new(error));
    }

    result
}

/// Try to parse a JSON value as a nested AlienError source chain.
fn parse_source_error(value: serde_json::Value) -> Option<Box<AlienError<GenericError>>> {
    let obj = value.as_object()?;

    let code = obj
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or("NESTED_ERROR")
        .to_string();

    let message = obj
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("Nested error")
        .to_string();

    let context = obj.get("context").cloned();
    let retryable = obj
        .get("retryable")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Recursively parse nested source
    let nested_source = obj.get("source").cloned().and_then(parse_source_error);

    Some(Box::new(AlienError {
        code,
        message: message.clone(),
        context,
        retryable,
        internal: false,
        http_status_code: None,
        source: nested_source,
        error: Some(GenericError { message }),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_code_deref() {
        // Verify generated types work as expected
        let code = types::ApiErrorCode::try_from("TEST_ERROR").unwrap();
        assert_eq!(code.as_str(), "TEST_ERROR");
    }
}
