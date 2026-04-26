use std::time::Instant;

use alien_core::{MessagePayload, QueueMessage};
use alien_error::{AlienError, Context, IntoAlienError};
use tracing::{debug, info};

use crate::{
    error::{ErrorData, Result},
    types::{BodySpec, CommandResponse, Envelope},
    PROTOCOL_VERSION,
};

/// Parse a QueueMessage to extract a command envelope if present
pub fn parse_envelope(message: &QueueMessage) -> Result<Option<Envelope>> {
    let envelope_data = match &message.payload {
        MessagePayload::Json(value) => {
            // Try to parse as command envelope
            match serde_json::from_value::<Envelope>(value.clone()) {
                Ok(envelope) => envelope,
                Err(_) => return Ok(None), // Not a command envelope
            }
        }
        MessagePayload::Text(text) => {
            // Try to parse JSON text as command envelope
            match serde_json::from_str::<Envelope>(text) {
                Ok(envelope) => envelope,
                Err(_) => return Ok(None), // Not a command envelope
            }
        }
    };

    // Validate it's a valid command envelope
    if envelope_data.protocol != PROTOCOL_VERSION {
        return Ok(None);
    }

    envelope_data
        .validate()
        .context(ErrorData::InvalidEnvelope {
            message: "Envelope validation failed".to_string(),
            field: None,
        })?;
    Ok(Some(envelope_data))
}

/// Decode params from an envelope to JSON
///
/// For inline params, decodes the base64 and parses as JSON.
/// For storage params, fetches from storage using the presigned request.
pub async fn decode_params(envelope: &Envelope) -> Result<serde_json::Value> {
    match &envelope.params {
        BodySpec::Inline { inline_base64 } => {
            use base64::{engine::general_purpose, Engine as _};

            let bytes = general_purpose::STANDARD
                .decode(inline_base64)
                .into_alien_error()
                .context(ErrorData::InvalidEnvelope {
                    message: "Failed to decode base64 params".to_string(),
                    field: Some("params.inlineBase64".to_string()),
                })?;

            serde_json::from_slice(&bytes)
                .into_alien_error()
                .context(ErrorData::InvalidEnvelope {
                    message: "Failed to parse params JSON".to_string(),
                    field: Some("params".to_string()),
                })
        }
        BodySpec::Storage {
            storage_get_request,
            ..
        } => {
            let presigned_request = storage_get_request.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::InvalidEnvelope {
                    message: "Storage params missing storage_get_request".to_string(),
                    field: Some("params.storageGetRequest".to_string()),
                })
            })?;

            let response = presigned_request.execute(None).await.context(
                ErrorData::StorageOperationFailed {
                    message: "Failed to fetch params from storage".to_string(),
                    operation: Some("get".to_string()),
                    path: Some(presigned_request.path.clone()),
                },
            )?;

            let body = response.body.ok_or_else(|| {
                AlienError::new(ErrorData::StorageOperationFailed {
                    message: "Storage response has no body".to_string(),
                    operation: Some("get".to_string()),
                    path: Some(presigned_request.path.clone()),
                })
            })?;

            serde_json::from_slice(&body)
                .into_alien_error()
                .context(ErrorData::InvalidEnvelope {
                    message: "Failed to parse params JSON from storage".to_string(),
                    field: Some("params".to_string()),
                })
        }
    }
}

/// Decode params from an envelope to raw bytes
///
/// For inline params, decodes the base64.
/// For storage params, fetches from storage using the presigned request.
pub async fn decode_params_bytes(envelope: &Envelope) -> Result<Vec<u8>> {
    match &envelope.params {
        BodySpec::Inline { inline_base64 } => {
            use base64::{engine::general_purpose, Engine as _};

            general_purpose::STANDARD
                .decode(inline_base64)
                .into_alien_error()
                .context(ErrorData::InvalidEnvelope {
                    message: "Failed to decode base64 params".to_string(),
                    field: Some("params.inlineBase64".to_string()),
                })
        }
        BodySpec::Storage {
            storage_get_request,
            ..
        } => {
            let presigned_request = storage_get_request.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::InvalidEnvelope {
                    message: "Storage params missing storage_get_request".to_string(),
                    field: Some("params.storageGetRequest".to_string()),
                })
            })?;

            let response = presigned_request.execute(None).await.context(
                ErrorData::StorageOperationFailed {
                    message: "Failed to fetch params from storage".to_string(),
                    operation: Some("get".to_string()),
                    path: Some(presigned_request.path.clone()),
                },
            )?;

            response.body.map(|b| b.to_vec()).ok_or_else(|| {
                AlienError::new(ErrorData::StorageOperationFailed {
                    message: "Storage response has no body".to_string(),
                    operation: Some("get".to_string()),
                    path: Some(presigned_request.path.clone()),
                })
            })
        }
    }
}

/// Submit a command response back to the command server
///
/// This function implements the complete command response submission protocol:
/// - Small responses (≤ maxInlineBytes) are submitted inline as base64
/// - Large responses are uploaded to storage first, then submitted with storage reference
#[cfg(feature = "runtime")]
pub async fn submit_response(envelope: &Envelope, response: CommandResponse) -> Result<()> {
    use reqwest::Client;
    use std::time::Duration;

    let start_time = Instant::now();

    // Create client with connection pooling to prevent FD exhaustion
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(2)
        .pool_idle_timeout(Some(Duration::from_secs(60)))
        .build()
        .into_alien_error()
        .context(ErrorData::Other {
            message: "Failed to create HTTP client".to_string(),
        })?;

    // Check if response body needs storage upload
    let final_response = match &response {
        CommandResponse::Success { response: body } => {
            let body_size = body.size().unwrap_or(0);

            if body_size > envelope.response_handling.max_inline_bytes {
                // Large response: upload to storage first
                debug!(
                    command_id = %envelope.command_id,
                    body_size = body_size,
                    max_inline = envelope.response_handling.max_inline_bytes,
                    "Uploading large response body to storage"
                );

                // Get the bytes from the body
                let body_bytes = body.decode_inline().ok_or_else(|| {
                    AlienError::new(ErrorData::Other {
                        message: "Cannot upload storage body - expected inline body".to_string(),
                    })
                })?;

                // Upload to storage using the presigned request
                let upload_response = envelope
                    .response_handling
                    .storage_upload_request
                    .execute(Some(bytes::Bytes::from(body_bytes.clone())))
                    .await
                    .into_alien_error()
                    .context(ErrorData::StorageOperationFailed {
                        message: "Failed to upload response to storage".to_string(),
                        operation: Some("put".to_string()),
                        path: Some(
                            envelope
                                .response_handling
                                .storage_upload_request
                                .path
                                .clone(),
                        ),
                    })?;

                if upload_response.status_code < 200 || upload_response.status_code >= 300 {
                    return Err(AlienError::new(ErrorData::StorageOperationFailed {
                        message: format!(
                            "Storage upload failed with status {}",
                            upload_response.status_code
                        ),
                        operation: Some("put".to_string()),
                        path: Some(
                            envelope
                                .response_handling
                                .storage_upload_request
                                .path
                                .clone(),
                        ),
                    }));
                }

                debug!(
                    command_id = %envelope.command_id,
                    upload_status = upload_response.status_code,
                    "Response body uploaded to storage successfully"
                );

                // Create storage body spec
                CommandResponse::Success {
                    response: BodySpec::Storage {
                        size: Some(body_bytes.len() as u64),
                        storage_get_request: None, // Server will fill this in
                        storage_put_used: Some(true),
                    },
                }
            } else {
                response.clone()
            }
        }
        CommandResponse::Error { .. } => response.clone(),
    };

    // Submit response to command server using the URL from the envelope
    let submit_url = &envelope.response_handling.submit_response_url;

    debug!(
        command_id = %envelope.command_id,
        url = %submit_url,
        "Submitting command response"
    );

    let http_response = client
        .put(submit_url)
        .json(&crate::types::SubmitResponseRequest {
            response: final_response.clone(),
        })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpOperationFailed {
            message: "Failed to submit response".to_string(),
            method: Some("PUT".to_string()),
            url: Some(submit_url.clone()),
        })?;

    if !http_response.status().is_success() {
        let status = http_response.status();
        let error_body = http_response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::HttpOperationFailed {
            message: format!(
                "Response submission failed with status {}: {}",
                status, error_body
            ),
            method: Some("PUT".to_string()),
            url: Some(submit_url.clone()),
        }));
    }

    debug!(
        command_id = %envelope.command_id,
        processing_ms = start_time.elapsed().as_millis(),
        response_type = if final_response.is_success() { "success" } else { "error" },
        "Command response submitted successfully"
    );

    Ok(())
}

/// Create a simple success response for testing
pub fn create_test_response(data: &[u8]) -> CommandResponse {
    CommandResponse::success(data)
}

/// Create a simple error response for testing
pub fn create_test_error(code: &str, message: &str) -> CommandResponse {
    CommandResponse::error(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_bindings::presigned::PresignedRequest;
    use chrono::Utc;

    fn create_test_envelope() -> Envelope {
        Envelope {
            protocol: PROTOCOL_VERSION.to_string(),
            command_id: "cmd_123".to_string(),
            attempt: 1,
            deadline: None,
            command: "test-command".to_string(),
            params: BodySpec::inline(b"{}"),
            response_handling: crate::types::ResponseHandling {
                max_inline_bytes: 150000,
                submit_response_url: "https://commands.example.com/commands/cmd_123/response"
                    .to_string(),
                storage_upload_request: PresignedRequest::new_http(
                    "https://storage.example.com/upload".to_string(),
                    "PUT".to_string(),
                    std::collections::HashMap::new(),
                    alien_bindings::presigned::PresignedOperation::Put,
                    "test-path".to_string(),
                    Utc::now() + chrono::Duration::hours(1),
                ),
            },
            deployment_id: "dep_123".to_string(),
        }
    }

    #[test]
    fn test_parse_envelope_json() {
        let envelope = create_test_envelope();
        let envelope_json = serde_json::to_value(&envelope).unwrap();

        let queue_message = QueueMessage {
            id: "msg_123".to_string(),
            payload: MessagePayload::Json(envelope_json),
            receipt_handle: "handle_123".to_string(),
            timestamp: Utc::now(),
            source: "test-queue".to_string(),
            attributes: std::collections::HashMap::new(),
            attempt_count: Some(1),
        };

        let parsed = parse_envelope(&queue_message).unwrap();
        assert!(parsed.is_some());

        let parsed_envelope = parsed.unwrap();
        assert_eq!(parsed_envelope.command_id, "cmd_123");
        assert_eq!(parsed_envelope.command, "test-command");
        assert_eq!(parsed_envelope.protocol, PROTOCOL_VERSION);
    }

    #[test]
    fn test_parse_envelope_text() {
        let envelope = create_test_envelope();
        let envelope_text = serde_json::to_string(&envelope).unwrap();

        let queue_message = QueueMessage {
            id: "msg_456".to_string(),
            payload: MessagePayload::Text(envelope_text),
            receipt_handle: "handle_456".to_string(),
            timestamp: Utc::now(),
            source: "test-queue".to_string(),
            attributes: std::collections::HashMap::new(),
            attempt_count: Some(1),
        };

        let parsed = parse_envelope(&queue_message).unwrap();
        assert!(parsed.is_some());

        let parsed_envelope = parsed.unwrap();
        assert_eq!(parsed_envelope.command_id, "cmd_123");
    }

    #[test]
    fn test_parse_non_command_message() {
        let queue_message = QueueMessage {
            id: "msg_789".to_string(),
            payload: MessagePayload::Json(serde_json::json!({"regular": "message"})),
            receipt_handle: "handle_789".to_string(),
            timestamp: Utc::now(),
            source: "test-queue".to_string(),
            attributes: std::collections::HashMap::new(),
            attempt_count: Some(1),
        };

        let parsed = parse_envelope(&queue_message).unwrap();
        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_invalid_protocol() {
        let mut envelope = create_test_envelope();
        envelope.protocol = "invalid.v1".to_string();

        let envelope_json = serde_json::to_value(&envelope).unwrap();
        let queue_message = QueueMessage {
            id: "msg_invalid".to_string(),
            payload: MessagePayload::Json(envelope_json),
            receipt_handle: "handle_invalid".to_string(),
            timestamp: Utc::now(),
            source: "test-queue".to_string(),
            attributes: std::collections::HashMap::new(),
            attempt_count: Some(1),
        };

        let parsed = parse_envelope(&queue_message).unwrap();
        assert!(parsed.is_none());
    }

    #[test]
    fn test_create_test_response() {
        let response = create_test_response(b"Hello World");
        assert!(response.is_success());

        if let CommandResponse::Success { response: body } = response {
            assert_eq!(body.decode_inline().unwrap(), b"Hello World");
        } else {
            panic!("Expected success response");
        }
    }

    #[test]
    fn test_create_test_error() {
        let response = create_test_error("TEST_ERROR", "Something went wrong");
        assert!(response.is_error());

        if let CommandResponse::Error { code, message, .. } = response {
            assert_eq!(code, "TEST_ERROR");
            assert_eq!(message, "Something went wrong");
        } else {
            panic!("Expected error response");
        }
    }

    #[tokio::test]
    async fn test_decode_params_inline() {
        let params_json = serde_json::json!({"key": "value", "num": 42});
        let params_bytes = serde_json::to_vec(&params_json).unwrap();

        let envelope = Envelope {
            protocol: PROTOCOL_VERSION.to_string(),
            command_id: "cmd_decode".to_string(),
            attempt: 1,
            deadline: None,
            command: "test".to_string(),
            params: BodySpec::inline(&params_bytes),
            response_handling: crate::types::ResponseHandling {
                max_inline_bytes: 150000,
                submit_response_url: "https://commands.example.com/response".to_string(),
                storage_upload_request: PresignedRequest::new_http(
                    "https://storage.example.com/upload".to_string(),
                    "PUT".to_string(),
                    std::collections::HashMap::new(),
                    alien_bindings::presigned::PresignedOperation::Put,
                    "test-path".to_string(),
                    Utc::now() + chrono::Duration::hours(1),
                ),
            },
            deployment_id: "dep_123".to_string(),
        };

        let decoded = decode_params(&envelope).await.unwrap();
        assert_eq!(decoded["key"], "value");
        assert_eq!(decoded["num"], 42);
    }
}
