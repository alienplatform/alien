use std::time::{Duration, Instant};

use alien_core::{MessagePayload, QueueMessage};
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, Utc};
use tracing::debug;

use crate::{
    error::{ErrorData, Result},
    types::{BodySpec, CommandResponse, Envelope, LeaseInfo, LeaseRequest, LeaseResponse},
    PROTOCOL_VERSION,
};

/// Safety margin subtracted from a lease's expiry when computing a command's
/// execution budget. Stopping this far before the lease actually expires
/// guarantees the runtime finishes (or abandons) the command while the lease
/// is still held, so an expired lease is never redelivered by the manager
/// while a duplicate is still in flight. Shared by every pull-side poller —
/// the app-owned `Receiver` and the worker runtime's commands-polling
/// transport. Twin of the TypeScript receiver's `LEASE_SAFETY_MARGIN_MS`.
pub const LEASE_SAFETY_MARGIN: Duration = Duration::from_secs(5);

/// Per-command execution budget: `min(envelope.deadline, lease_expiry −
/// [`LEASE_SAFETY_MARGIN`])`, clamped so it never falls before now. There is
/// no lease-renew call in the protocol, so the safety-margined lease expiry
/// always bounds the budget. Shared by both pull-side pollers so the worker
/// runtime and the app-owned receiver enforce identical semantics. Twin of
/// the TypeScript receiver's `commandBudget`.
pub fn command_budget(
    deadline: Option<DateTime<Utc>>,
    lease_expires_at: DateTime<Utc>,
) -> DateTime<Utc> {
    let margin = chrono::Duration::from_std(LEASE_SAFETY_MARGIN)
        .unwrap_or_else(|_| chrono::Duration::seconds(5));
    let lease_bound = (lease_expires_at - margin).max(Utc::now());
    deadline.map_or(lease_bound, |d| d.min(lease_bound))
}

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
#[cfg(any(feature = "runtime", feature = "receiver"))]
pub async fn submit_response(envelope: &Envelope, response: CommandResponse) -> Result<()> {
    use reqwest::Client;

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

/// Shared lease-acquisition client for the pull-side pollers — the app-owned
/// `Receiver` and the worker runtime's commands-polling transport.
///
/// Holds the fully-qualified `…/commands/leases` endpoint, the bearer token,
/// and a pooled HTTP client. The endpoint is built **once** at construction
/// via [`LeaseClient::from_base`], so a base URL that cannot be a hierarchical
/// (HTTP(S)) URL fails at startup rather than being re-derived — and
/// misclassified as a transient error — on every poll.
///
/// [`LeaseClient::acquire`] returns this crate's [`Result`]; each caller maps
/// it to its own error enum at the boundary.
#[cfg(any(feature = "runtime", feature = "receiver"))]
#[derive(Debug, Clone)]
pub struct LeaseClient {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    token: String,
}

#[cfg(any(feature = "runtime", feature = "receiver"))]
impl LeaseClient {
    /// Build a lease client for a command-server base URL, appending the
    /// `commands/leases` path segments once.
    ///
    /// Returns `None` if `base` cannot be a hierarchical (HTTP(S)) URL —
    /// callers surface that as their own startup config error so the permanent
    /// misconfiguration fails fast instead of being retried on every poll.
    pub fn from_base(base: &reqwest::Url, token: String) -> Option<Self> {
        let mut endpoint = base.clone();
        endpoint
            .path_segments_mut()
            .ok()?
            .pop_if_empty()
            .push("commands")
            .push("leases");
        // A request timeout is load-bearing here: the poll loop awaits this
        // client serially, so a single hung acquire (half-open TCP, stalled
        // LB) with reqwest's no-timeout default would freeze command intake
        // for the whole process, not just one request.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self {
            client,
            endpoint,
            token,
        })
    }

    /// The fully-qualified lease endpoint this client POSTs to.
    pub fn endpoint(&self) -> &reqwest::Url {
        &self.endpoint
    }

    /// Acquire leases: POST `request` with the bearer token and parse the
    /// `LeaseResponse`. Errors as `HTTP_OPERATION_FAILED` (transport or
    /// non-success status) or `SERIALIZATION_FAILED` (unparseable body).
    pub async fn acquire(&self, request: &LeaseRequest) -> Result<Vec<LeaseInfo>> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .header("Authorization", format!("Bearer {}", self.token))
            .json(request)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpOperationFailed {
                message: "Failed to acquire leases".to_string(),
                method: Some("POST".to_string()),
                url: Some(self.endpoint.to_string()),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AlienError::new(ErrorData::HttpOperationFailed {
                message: format!("Lease request failed with status {status}: {body}"),
                method: Some("POST".to_string()),
                url: Some(self.endpoint.to_string()),
            }));
        }

        let mut lease_response: LeaseResponse =
            response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "Failed to parse lease response".to_string(),
                    data_type: Some("LeaseResponse".to_string()),
                })?;

        // Lease-served envelopes carry manager URLs as root-relative paths
        // (the manager cannot know an address reachable from behind this
        // consumer's network boundary; this endpoint — corrected by the
        // platform for exactly that — is the address to resolve against).
        // Absolute URLs pass through: cloud-presigned storage and envelopes
        // from managers that predate relative minting.
        for lease in &mut lease_response.leases {
            resolve_envelope_urls(&mut lease.envelope, &self.endpoint);
        }

        Ok(lease_response.leases)
    }
}

/// Resolve root-relative envelope URLs against the consumer's configured
/// commands endpoint origin. See [`LeaseClient::acquire`] — this is the
/// single ingestion point for lease-served envelopes, so downstream code
/// (`submit_response`, params decoding, presigned uploads) always sees
/// absolute URLs, exactly as before relative minting existed.
pub fn resolve_envelope_urls(envelope: &mut Envelope, base: &reqwest::Url) {
    let origin = base.origin().ascii_serialization();
    let resolve = |target: &mut String| {
        if target.starts_with('/') {
            *target = format!("{origin}{target}");
        }
    };

    resolve(&mut envelope.response_handling.submit_response_url);
    if let alien_core::presigned::PresignedRequestBackend::Http { url, .. } =
        &mut envelope.response_handling.storage_upload_request.backend
    {
        resolve(url);
    }
    if let alien_core::commands_types::BodySpec::Storage {
        storage_get_request: Some(request),
        ..
    } = &mut envelope.params
    {
        if let alien_core::presigned::PresignedRequestBackend::Http { url, .. } =
            &mut request.backend
        {
            resolve(url);
        }
    }
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
    use alien_core::presigned::{PresignedOperation, PresignedRequest};
    use chrono::Utc;

    fn create_test_envelope() -> Envelope {
        Envelope {
            protocol: PROTOCOL_VERSION.to_string(),
            target: crate::types::CommandTarget::new(
                "test-worker",
                crate::types::CommandTargetType::Worker,
            ),
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
                    PresignedOperation::Put,
                    "test-path".to_string(),
                    Utc::now() + chrono::Duration::hours(1),
                ),
            },
            deployment_id: "dep_123".to_string(),
        }
    }

    /// Root-relative envelope URLs resolve against the configured commands
    /// endpoint's origin; absolute URLs (cloud presigned, older managers)
    /// pass through untouched. Twin of the TS `resolveEnvelopeUrls` test.
    #[test]
    fn resolve_envelope_urls_resolves_relative_and_keeps_absolute() {
        let base = reqwest::Url::parse("http://host.docker.internal:9090/v1").unwrap();

        let mut envelope = create_test_envelope();
        envelope.response_handling.submit_response_url =
            "/v1/commands/cmd_123/response?response_token=t&expires=1".to_string();
        resolve_envelope_urls(&mut envelope, &base);
        assert_eq!(
            envelope.response_handling.submit_response_url,
            "http://host.docker.internal:9090/v1/commands/cmd_123/response?response_token=t&expires=1",
            "relative submit URL must resolve against the endpoint origin"
        );
        // The cloud-presigned upload URL is absolute and must never be touched.
        match &envelope.response_handling.storage_upload_request.backend {
            alien_core::presigned::PresignedRequestBackend::Http { url, .. } => {
                assert_eq!(url, "https://storage.example.com/upload");
            }
            other => panic!("unexpected backend: {other:?}"),
        }

        // A manager-served (relative) upload URL resolves too.
        let mut envelope = create_test_envelope();
        if let alien_core::presigned::PresignedRequestBackend::Http { url, .. } =
            &mut envelope.response_handling.storage_upload_request.backend
        {
            *url = "/v1/commands/cmd_123/response-blob?sig=x".to_string();
        }
        resolve_envelope_urls(&mut envelope, &base);
        match &envelope.response_handling.storage_upload_request.backend {
            alien_core::presigned::PresignedRequestBackend::Http { url, .. } => {
                assert_eq!(
                    url,
                    "http://host.docker.internal:9090/v1/commands/cmd_123/response-blob?sig=x"
                );
            }
            other => panic!("unexpected backend: {other:?}"),
        }

        // Absolute submit URLs pass through unchanged (older managers).
        let mut envelope = create_test_envelope();
        resolve_envelope_urls(&mut envelope, &base);
        assert_eq!(
            envelope.response_handling.submit_response_url,
            "https://commands.example.com/commands/cmd_123/response"
        );
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
            target: crate::types::CommandTarget::new(
                "test-worker",
                crate::types::CommandTargetType::Worker,
            ),
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
                    PresignedOperation::Put,
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
