//! Alien Manager API
//!
//! Auto-generated from OpenAPI spec using Progenitor.
//! Provides a type-safe Rust client for the alien-manager API.
//!
//! ## Usage
//!
//! ```ignore
//! use alien_manager_api::Client;
//!
//! let client = Client::new("http://localhost:8080");
//!
//! // Create a deployment
//! let response = client
//!     .create_deployment()
//!     .body(&CreateDeploymentRequest {
//!         name: "my-deployment".into(),
//!         platform: Platform::Aws,
//!         ..Default::default()
//!     })
//!     .send()
//!     .await?;
//! ```

include!(concat!(env!("OUT_DIR"), "/codegen.rs"));

use alien_error::{AlienError, GenericError, HumanLayerPresentation};

/// Extension trait for converting manager SDK results to `AlienError`.
pub trait SdkResultExt<T> {
    /// Convert SDK result to `AlienError` result, preserving API error details.
    fn into_sdk_error(self) -> Result<T, AlienError<GenericError>>;
}

/// Async counterpart for operations whose OpenAPI schema does not describe
/// every error status. Progenitor leaves those response bodies unread, so an
/// async adapter is required to preserve the structured Alien error payload.
pub trait SdkResultExtReadingBody<T> {
    fn into_sdk_error_reading_body(
        self,
    ) -> impl std::future::Future<Output = Result<T, AlienError<GenericError>>> + Send;
}

impl<T: Send> SdkResultExtReadingBody<ResponseValue<T>> for Result<ResponseValue<T>, Error<()>> {
    fn into_sdk_error_reading_body(
        self,
    ) -> impl std::future::Future<Output = Result<ResponseValue<T>, AlienError<GenericError>>> + Send
    {
        async move {
            match self {
                Ok(response) => Ok(response),
                Err(error) => Err(convert_sdk_error_reading_body(error).await),
            }
        }
    }
}

impl<T> SdkResultExt<ResponseValue<T>> for Result<ResponseValue<T>, Error<()>> {
    fn into_sdk_error(self) -> Result<ResponseValue<T>, AlienError<GenericError>> {
        self.map_err(convert_sdk_error)
    }
}

/// Convert a progenitor SDK error to `AlienError`, reading the response body
/// of error statuses so structured Alien errors returned by the manager
/// (code, message, hint, retryable) survive the round-trip instead of
/// collapsing into a generic "Unexpected response" error.
///
/// Async because reading the response body requires awaiting; falls back to
/// [`convert_sdk_error`] semantics when the body is not an Alien error payload.
pub async fn convert_sdk_error_reading_body(err: Error<()>) -> AlienError<GenericError> {
    match err {
        Error::UnexpectedResponse(response) => {
            let status = response.status().as_u16();
            let canonical_reason = response
                .status()
                .canonical_reason()
                .unwrap_or("Unknown")
                .to_string();
            let url = response.url().to_string();
            let header_request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let body = response.text().await.unwrap_or_default();

            if let Ok(mut api_error) = serde_json::from_str::<AlienError<GenericError>>(&body) {
                if api_error.http_status_code.is_none() {
                    api_error.http_status_code = Some(status);
                }
                let body_request_id = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|value| value.get("requestId")?.as_str().map(str::to_string));
                api_error.context = context_with_request_id(
                    api_error.context,
                    header_request_id.as_deref().or(body_request_id.as_deref()),
                );
                return api_error;
            }

            AlienError {
                code: "UNEXPECTED_RESPONSE".to_string(),
                message: format!("Unexpected response: {} {}", status, canonical_reason),
                context: Some(serde_json::json!({
                    "status": status,
                    "url": url,
                })),
                hint: None,
                retryable: status >= 500,
                internal: false,
                http_status_code: Some(status),
                source: None,
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError {
                    message: format!("Unexpected response status: {}", status),
                }),
            }
        }
        other => convert_sdk_error(other),
    }
}

fn context_with_request_id(
    context: Option<serde_json::Value>,
    request_id: Option<&str>,
) -> Option<serde_json::Value> {
    let Some(request_id) = request_id else {
        return context;
    };

    match context {
        Some(serde_json::Value::Object(mut object)) => {
            object
                .entry("requestId")
                .or_insert_with(|| serde_json::Value::String(request_id.to_string()));
            Some(serde_json::Value::Object(object))
        }
        Some(value) => Some(serde_json::json!({
            "requestId": request_id,
            "details": value,
        })),
        None => Some(serde_json::json!({ "requestId": request_id })),
    }
}

/// Convert a progenitor SDK error to AlienError, preserving all details.
pub fn convert_sdk_error(err: Error<()>) -> AlienError<GenericError> {
    match err {
        Error::ErrorResponse(response) => {
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
                })),
                hint: None,
                retryable: status >= 500,
                internal: false,
                http_status_code: Some(status),
                source: None,
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError {
                    message: format!("Unexpected response status: {}", status),
                }),
            }
        }
        Error::CommunicationError(reqwest_err) => {
            let retryable =
                reqwest_err.is_connect() || reqwest_err.is_timeout() || reqwest_err.is_request();
            let message = reqwest_failure_message("HTTP request", &reqwest_err);

            AlienError {
                code: "COMMUNICATION_ERROR".to_string(),
                message: message.clone(),
                context: reqwest_failure_context(&reqwest_err),
                hint: None,
                retryable,
                internal: false,
                http_status_code: reqwest_err.status().map(|s| s.as_u16()),
                source: build_reqwest_source(&reqwest_err),
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError { message }),
            }
        }
        Error::InvalidRequest(msg) => AlienError {
            code: "INVALID_REQUEST".to_string(),
            message: format!("Invalid Request: {}", msg),
            context: None,
            hint: None,
            retryable: false,
            internal: false,
            http_status_code: Some(400),
            source: None,
            human_layer_presentation: HumanLayerPresentation::Normal,
            error: Some(GenericError {
                message: format!("Invalid Request: {}", msg),
            }),
        },
        Error::ResponseBodyError(reqwest_err) => {
            let message = reqwest_failure_message("HTTP response body read", &reqwest_err);

            AlienError {
                code: "RESPONSE_BODY_ERROR".to_string(),
                message: message.clone(),
                context: reqwest_failure_context(&reqwest_err),
                hint: None,
                retryable: true,
                internal: false,
                http_status_code: reqwest_err.status().map(|s| s.as_u16()),
                source: build_reqwest_source(&reqwest_err),
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError { message }),
            }
        }
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
                hint: None,
                retryable: false,
                internal: false,
                http_status_code: None,
                source: Some(Box::new(AlienError::new(GenericError {
                    message: json_err.to_string(),
                }))),
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError {
                    message: format!("Failed to parse response: {}", json_err),
                }),
            }
        }
        Error::InvalidUpgrade(reqwest_err) => {
            let message = reqwest_failure_message("HTTP connection upgrade", &reqwest_err);

            AlienError {
                code: "INVALID_UPGRADE".to_string(),
                message: message.clone(),
                context: reqwest_failure_context(&reqwest_err),
                hint: None,
                retryable: false,
                internal: false,
                http_status_code: reqwest_err.status().map(|s| s.as_u16()),
                source: build_reqwest_source(&reqwest_err),
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError { message }),
            }
        }
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
                hint: None,
                retryable: status >= 500,
                internal: false,
                http_status_code: Some(status),
                source: None,
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError {
                    message: format!("Unexpected response status: {}", status),
                }),
            }
        }
        Error::Custom(msg) => AlienError {
            code: "SDK_HOOK_ERROR".to_string(),
            message: msg.clone(),
            context: None,
            hint: None,
            retryable: false,
            internal: false,
            http_status_code: None,
            source: None,
            human_layer_presentation: HumanLayerPresentation::Normal,
            error: Some(GenericError { message: msg }),
        },
    }
}

fn reqwest_failure_message(operation: &str, err: &reqwest::Error) -> String {
    match err.url() {
        Some(url) => format!("{operation} {} failed: {err}", url),
        None => format!("{operation} failed: {err}"),
    }
}

fn reqwest_failure_context(err: &reqwest::Error) -> Option<serde_json::Value> {
    err.url().map(|url| {
        serde_json::json!({
            "url": url.to_string(),
        })
    })
}

fn build_reqwest_source(reqwest_err: &reqwest::Error) -> Option<Box<AlienError<GenericError>>> {
    use std::error::Error as _;

    reqwest_err.source().map(|source| {
        Box::new(AlienError {
            code: "GENERIC_ERROR".to_string(),
            message: source.to_string(),
            context: None,
            hint: None,
            retryable: false,
            internal: false,
            http_status_code: None,
            source: None,
            human_layer_presentation: HumanLayerPresentation::Transparent,
            error: Some(GenericError {
                message: source.to_string(),
            }),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unexpected_response(status: u16, body: &str) -> Error<()> {
        let response = http::Response::builder()
            .status(status)
            .body(body.to_string())
            .expect("test response should build");
        Error::UnexpectedResponse(reqwest::Response::from(response))
    }

    #[tokio::test]
    async fn reading_body_preserves_structured_alien_errors() {
        let body = serde_json::json!({
            "code": "PUBLIC_SUBDOMAIN_REQUIRES_CUSTOM_DOMAIN",
            "message": "Choosing a public subdomain requires a custom project domain",
            "hint": "Configure a custom domain first",
            "retryable": false,
            "internal": false,
            "httpStatusCode": 400,
            "requestId": "req_body_123",
        })
        .to_string();

        let error = convert_sdk_error_reading_body(unexpected_response(400, &body)).await;

        assert_eq!(error.code, "PUBLIC_SUBDOMAIN_REQUIRES_CUSTOM_DOMAIN");
        assert_eq!(
            error.message,
            "Choosing a public subdomain requires a custom project domain"
        );
        assert_eq!(error.http_status_code, Some(400));
        assert_eq!(
            error.hint.as_deref(),
            Some("Configure a custom domain first")
        );
        assert_eq!(error.context.as_ref().unwrap()["requestId"], "req_body_123");
        assert!(!error.retryable);
        assert!(!error.internal);
    }

    #[tokio::test]
    async fn reading_body_falls_back_to_generic_error_for_non_alien_payloads() {
        let error =
            convert_sdk_error_reading_body(unexpected_response(502, "<html>bad gateway</html>"))
                .await;

        assert_eq!(error.code, "UNEXPECTED_RESPONSE");
        assert_eq!(error.message, "Unexpected response: 502 Bad Gateway");
        assert_eq!(error.http_status_code, Some(502));
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn communication_error_includes_url_in_message_and_context() {
        let reqwest_err = reqwest::Client::new()
            .get("http://127.0.0.1:9/v1/initialize")
            .send()
            .await
            .expect_err("localhost discard port should refuse the connection");

        let error = super::convert_sdk_error(Error::CommunicationError(reqwest_err));

        assert_eq!(error.code, "COMMUNICATION_ERROR");
        assert!(error
            .message
            .starts_with("HTTP request http://127.0.0.1:9/v1/initialize failed:"));
        assert_eq!(
            error.context.as_ref().unwrap()["url"],
            "http://127.0.0.1:9/v1/initialize"
        );
    }
}
