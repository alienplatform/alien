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

impl<T> SdkResultExt<ResponseValue<T>> for Result<ResponseValue<T>, Error<()>> {
    fn into_sdk_error(self) -> Result<ResponseValue<T>, AlienError<GenericError>> {
        self.map_err(convert_sdk_error)
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

            AlienError {
                code: "COMMUNICATION_ERROR".to_string(),
                message: format!("Communication Error: {}", reqwest_err),
                context: None,
                hint: None,
                retryable,
                internal: false,
                http_status_code: reqwest_err.status().map(|s| s.as_u16()),
                source: build_reqwest_source(&reqwest_err),
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError {
                    message: format!("Communication Error: {}", reqwest_err),
                }),
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
        Error::ResponseBodyError(reqwest_err) => AlienError {
            code: "RESPONSE_BODY_ERROR".to_string(),
            message: format!("Error reading response body: {}", reqwest_err),
            context: None,
            hint: None,
            retryable: true,
            internal: false,
            http_status_code: reqwest_err.status().map(|s| s.as_u16()),
            source: build_reqwest_source(&reqwest_err),
            human_layer_presentation: HumanLayerPresentation::Normal,
            error: Some(GenericError {
                message: format!("Error reading response body: {}", reqwest_err),
            }),
        },
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
        Error::InvalidUpgrade(reqwest_err) => AlienError {
            code: "INVALID_UPGRADE".to_string(),
            message: format!("Connection upgrade failed: {}", reqwest_err),
            context: None,
            hint: None,
            retryable: false,
            internal: false,
            http_status_code: reqwest_err.status().map(|s| s.as_u16()),
            source: build_reqwest_source(&reqwest_err),
            human_layer_presentation: HumanLayerPresentation::Normal,
            error: Some(GenericError {
                message: format!("Connection upgrade failed: {}", reqwest_err),
            }),
        },
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
