use std::time::{Duration, Instant};

use alien_core::{presigned::redact_url_for_error, MessagePayload, QueueMessage};
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use chrono::{DateTime, Utc};
use tracing::debug;

use crate::{
    error::{Error, ErrorData, Result},
    resolve_envelope_urls,
    types::{BodySpec, CommandResponse, Envelope, LeaseInfo, LeaseRequest, LeaseResponse},
    PROTOCOL_VERSION,
};

/// Safety margin subtracted from a lease's expiry when computing a command's
/// execution budget. Stopping this far before the lease actually expires
/// guarantees the runtime finishes (or abandons) the command while the lease
/// is still held, so an expired lease is never redelivered by the manager
/// while a duplicate is still in flight. Used by the app-owned pull
/// `Receiver`; twin of the TypeScript receiver's `LEASE_SAFETY_MARGIN_MS`.
pub const LEASE_SAFETY_MARGIN: Duration = Duration::from_secs(5);

/// Response upload plus final status submission must finish inside the
/// operator's additional 60-second lease headroom.
const COMMAND_RESPONSE_SUBMISSION_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-command execution budget: `min(envelope.deadline, lease_expiry −
/// [`LEASE_SAFETY_MARGIN`])`. The LEASE bound is clamped to now; an
/// already-past deadline is not — it yields a zero budget and an immediate
/// `HANDLER_TIMEOUT`, which is the correct outcome for a command delivered
/// after its deadline. There is
/// no lease-renew call in the protocol, so the safety-margined lease expiry
/// always bounds the budget. Twin of the TypeScript receiver's
/// `commandBudget`.
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
    submit_response_with_timeout(envelope, response, COMMAND_RESPONSE_SUBMISSION_TIMEOUT).await
}

/// Submit a command response without running beyond an absolute lease expiry.
///
/// The normal 30-second submission cap still applies, but a receiver holding a
/// shorter remaining lease must use that smaller budget. Computing the
/// remaining duration here, immediately before the upload/submission flow,
/// prevents either stage from extending the lease deadline.
#[cfg(feature = "receiver")]
pub(crate) async fn submit_response_before(
    envelope: &Envelope,
    response: CommandResponse,
    lease_expires_at: DateTime<Utc>,
) -> Result<()> {
    let remaining_lease = (lease_expires_at - Utc::now())
        .to_std()
        .unwrap_or(Duration::ZERO);
    submit_response_with_timeout(
        envelope,
        response,
        COMMAND_RESPONSE_SUBMISSION_TIMEOUT.min(remaining_lease),
    )
    .await
}

#[cfg(any(feature = "runtime", feature = "receiver"))]
async fn submit_response_with_timeout(
    envelope: &Envelope,
    response: CommandResponse,
    timeout: Duration,
) -> Result<()> {
    let operation = async {
        let start_time = Instant::now();
        let client = reqwest::Client::builder()
            .timeout(COMMAND_RESPONSE_SUBMISSION_TIMEOUT)
            .pool_max_idle_per_host(2)
            .pool_idle_timeout(Some(Duration::from_secs(60)))
            .build()
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to create HTTP client".to_string(),
            })?;
        let (final_response, mut pending_upload) = prepare_response_submission(envelope, response)?;
        let mut retry_delay = Duration::from_millis(100);
        loop {
            match submit_response_attempt(&client, envelope, &final_response, &mut pending_upload)
                .await
            {
                Ok(()) => {
                    debug!(
                        command_id = %envelope.command_id,
                        processing_ms = start_time.elapsed().as_millis(),
                        response_type = if final_response.is_success() { "success" } else { "error" },
                        "Command response submitted successfully"
                    );
                    return Ok(());
                }
                Err(SubmissionAttemptError::Permanent(error)) => return Err(error),
                Err(SubmissionAttemptError::Retryable(error)) => {
                    debug!(
                        command_id = %envelope.command_id,
                        error_code = %error.code,
                        retry_delay_ms = retry_delay.as_millis(),
                        "Transient command response submission failure; retrying"
                    );
                    tokio::time::sleep(retry_delay).await;
                    retry_delay = (retry_delay * 2).min(Duration::from_secs(1));
                }
            }
        }
    };

    match tokio::time::timeout(timeout, operation).await {
        Ok(result) => result,
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::HttpOperationFailed {
                message: format!(
                    "Command response upload/submission exceeded its {} ms headroom budget",
                    timeout.as_millis()
                ),
                method: None,
                url: Some(redact_url_for_error(
                    &envelope.response_handling.submit_response_url,
                )),
            })),
    }
}

#[cfg(any(feature = "runtime", feature = "receiver"))]
enum SubmissionAttemptError {
    Retryable(Error),
    Permanent(Error),
}

#[cfg(any(feature = "runtime", feature = "receiver"))]
fn prepare_response_submission(
    envelope: &Envelope,
    response: CommandResponse,
) -> Result<(CommandResponse, Option<bytes::Bytes>)> {
    match &response {
        CommandResponse::Success { response: body } => {
            let body_size = body.size().unwrap_or(0);

            if body_size > envelope.response_handling.max_inline_bytes {
                let body_bytes = body.decode_inline().ok_or_else(|| {
                    AlienError::new(ErrorData::Other {
                        message: "Cannot upload storage body - expected inline body".to_string(),
                    })
                })?;
                Ok((
                    CommandResponse::Success {
                        response: BodySpec::Storage {
                            size: Some(body_bytes.len() as u64),
                            storage_get_request: None,
                            storage_put_used: Some(true),
                        },
                    },
                    Some(bytes::Bytes::from(body_bytes)),
                ))
            } else {
                Ok((response, None))
            }
        }
        CommandResponse::Error { .. } => Ok((response, None)),
    }
}

#[cfg(any(feature = "runtime", feature = "receiver"))]
async fn submit_response_attempt(
    client: &reqwest::Client,
    envelope: &Envelope,
    final_response: &CommandResponse,
    pending_upload: &mut Option<bytes::Bytes>,
) -> std::result::Result<(), SubmissionAttemptError> {
    if let Some(body_bytes) = pending_upload.as_ref() {
        debug!(
            command_id = %envelope.command_id,
            body_size = body_bytes.len(),
            max_inline = envelope.response_handling.max_inline_bytes,
            "Uploading large response body to storage"
        );

        let upload_result = envelope
            .response_handling
            .storage_upload_request
            .execute_with_client(client, Some(body_bytes.clone()))
            .await;
        let upload_response = match upload_result {
            Ok(response) => response,
            Err(error) => {
                let retryable = error.retryable;
                let error = error.context(ErrorData::StorageOperationFailed {
                    message: "Failed to upload response to storage".to_string(),
                    operation: Some("put".to_string()),
                    path: Some(
                        envelope
                            .response_handling
                            .storage_upload_request
                            .path
                            .clone(),
                    ),
                });
                return Err(if retryable {
                    SubmissionAttemptError::Retryable(error)
                } else {
                    SubmissionAttemptError::Permanent(error)
                });
            }
        };

        if upload_response.status_code < 200 || upload_response.status_code >= 300 {
            let status = upload_response.status_code;
            let error = AlienError::new(ErrorData::StorageOperationFailed {
                message: format!("Storage upload failed with status {}", status),
                operation: Some("put".to_string()),
                path: Some(
                    envelope
                        .response_handling
                        .storage_upload_request
                        .path
                        .clone(),
                ),
            });
            return Err(if status == 408 || status == 429 || status >= 500 {
                SubmissionAttemptError::Retryable(error)
            } else {
                SubmissionAttemptError::Permanent(error)
            });
        }

        debug!(
            command_id = %envelope.command_id,
            upload_status = upload_response.status_code,
            "Response body uploaded to storage successfully"
        );
        *pending_upload = None;
    }

    // Submit response to command server using the URL from the envelope
    let submit_url = &envelope.response_handling.submit_response_url;
    let safe_submit_url = redact_url_for_error(submit_url);

    debug!(
        command_id = %envelope.command_id,
        url = %safe_submit_url,
        "Submitting command response"
    );

    let http_response = client
        .put(submit_url)
        .json(&crate::types::SubmitResponseRequest {
            response: final_response.clone(),
        })
        .send()
        .await;
    let http_response =
        match http_response {
            Ok(response) => response,
            Err(error) => {
                let retryable = !error.is_builder();
                let error = error.without_url().into_alien_error().context(
                    ErrorData::HttpOperationFailed {
                        message: "Failed to submit response".to_string(),
                        method: Some("PUT".to_string()),
                        url: Some(safe_submit_url.clone()),
                    },
                );
                return Err(if retryable {
                    SubmissionAttemptError::Retryable(error)
                } else {
                    SubmissionAttemptError::Permanent(error)
                });
            }
        };

    if !http_response.status().is_success()
        && http_response.status() != reqwest::StatusCode::CONFLICT
        && http_response.status() != reqwest::StatusCode::GONE
    {
        let status = http_response.status();
        let error = AlienError::new(ErrorData::HttpOperationFailed {
            // A failing endpoint may echo the bearer-equivalent response
            // token or another signed URL, so its body is not diagnostic-safe.
            message: format!("Response submission failed with status {status}"),
            method: Some("PUT".to_string()),
            url: Some(safe_submit_url),
        });
        return Err(
            if status == reqwest::StatusCode::REQUEST_TIMEOUT
                || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error()
            {
                SubmissionAttemptError::Retryable(error)
            } else {
                SubmissionAttemptError::Permanent(error)
            },
        );
    }

    Ok(())
}

/// Shared lease-acquisition client for app-owned pull receivers.
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
        self.acquire_with_token(request, &self.token).await
    }

    /// Acquire leases with a caller-supplied token. Receivers use this for
    /// file-backed token rotation; [`Self::acquire`] uses the configured token.
    pub async fn acquire_with_token(
        &self,
        request: &LeaseRequest,
        token: &str,
    ) -> Result<Vec<LeaseInfo>> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .header("Authorization", format!("Bearer {token}"))
            .json(request)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpOperationFailed {
                message: "Failed to acquire leases".to_string(),
                method: Some("POST".to_string()),
                url: Some(self.endpoint.to_string()),
            })?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AlienError::new(ErrorData::CommandReceiverUnauthorized {
                operation: "lease acquisition".to_string(),
                url: self.endpoint.to_string(),
            }));
        }

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

    /// Release a lease during graceful shutdown or duplicate suppression.
    /// Conflict/gone responses mean the lease is already no longer owned and
    /// are therefore successful idempotent outcomes.
    pub async fn release_with_token(&self, lease_id: &str, token: &str) -> Result<()> {
        let mut endpoint = self.endpoint.clone();
        let Ok(mut segments) = endpoint.path_segments_mut() else {
            return Err(AlienError::new(ErrorData::HttpOperationFailed {
                message: "Commands lease endpoint cannot be extended for release".to_string(),
                method: Some("POST".to_string()),
                url: Some(endpoint.to_string()),
            }));
        };
        segments.push(lease_id).push("release");
        drop(segments);
        let response = self
            .client
            .post(endpoint.clone())
            .header("Authorization", format!("Bearer {token}"))
            .json(&crate::types::ReleaseRequest {
                lease_id: lease_id.to_string(),
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpOperationFailed {
                message: format!("Failed to release lease '{lease_id}'"),
                method: Some("POST".to_string()),
                url: Some(endpoint.to_string()),
            })?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AlienError::new(ErrorData::CommandReceiverUnauthorized {
                operation: "lease release".to_string(),
                url: endpoint.to_string(),
            }));
        }
        if response.status().is_success()
            || response.status() == reqwest::StatusCode::CONFLICT
            || response.status() == reqwest::StatusCode::GONE
        {
            return Ok(());
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(AlienError::new(ErrorData::HttpOperationFailed {
            message: format!("Lease release failed with status {status}: {body}"),
            method: Some("POST".to_string()),
            url: Some(endpoint.to_string()),
        }))
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
    use axum::{extract::State, http::StatusCode, routing::put, Router};
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    async fn fail_once_then_accept(State(attempts): State<Arc<AtomicUsize>>) -> StatusCode {
        if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::OK
        }
    }

    async fn reject_permanently(State(attempts): State<Arc<AtomicUsize>>) -> StatusCode {
        attempts.fetch_add(1, Ordering::SeqCst);
        StatusCode::BAD_REQUEST
    }

    #[derive(Clone)]
    struct UploadThenRetryState {
        uploads: Arc<AtomicUsize>,
        submissions: Arc<AtomicUsize>,
    }

    async fn accept_upload(State(state): State<UploadThenRetryState>) -> StatusCode {
        state.uploads.fetch_add(1, Ordering::SeqCst);
        StatusCode::OK
    }

    async fn fail_first_submission(State(state): State<UploadThenRetryState>) -> StatusCode {
        if state.submissions.fetch_add(1, Ordering::SeqCst) == 0 {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::OK
        }
    }

    fn create_test_envelope() -> Envelope {
        Envelope {
            protocol: PROTOCOL_VERSION.to_string(),
            target: crate::types::CommandTarget::new(
                "test-worker",
                crate::types::CommandTargetType::Worker,
            ),
            command_id: "cmd_123".to_string(),
            attempt: 1,
            trace_context: None,
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

    #[tokio::test]
    async fn submit_response_error_does_not_expose_response_token() {
        let secret = "do-not-log-response-token";
        let mut envelope = create_test_envelope();
        envelope.response_handling.submit_response_url =
            format!("http://127.0.0.1:0/response?response_token={secret}&expires=1");

        let error = submit_response_with_timeout(
            &envelope,
            create_test_response(b"ok"),
            Duration::from_millis(50),
        )
        .await
        .expect_err("port zero must reject the response submission");
        let serialized = serde_json::to_string(&error).unwrap();
        let debug = format!("{error:?}");

        assert!(
            !serialized.contains(secret),
            "serialized error leaked token"
        );
        assert!(!debug.contains(secret), "debug error leaked token");
        assert!(serialized.contains("http://127.0.0.1:0/response"));
    }

    #[tokio::test]
    async fn response_upload_and_submit_share_one_bounded_redacted_timeout() {
        let upload_secret = "do-not-log-upload-signature";
        let response_secret = "do-not-log-response-token";
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });

        let mut envelope = create_test_envelope();
        envelope.response_handling.max_inline_bytes = 1;
        envelope.response_handling.submit_response_url =
            format!("http://{address}/response?response_token={response_secret}");
        envelope.response_handling.storage_upload_request = PresignedRequest::new_http(
            format!("http://{address}/upload?signature={upload_secret}"),
            "PUT".to_string(),
            std::collections::HashMap::new(),
            PresignedOperation::Put,
            "test-path".to_string(),
            Utc::now() + chrono::Duration::hours(1),
        );

        let error = submit_response_with_timeout(
            &envelope,
            create_test_response(b"large-response"),
            Duration::from_millis(50),
        )
        .await
        .expect_err("blackholed response upload must time out");
        let serialized = serde_json::to_string(&error).unwrap();
        let debug = format!("{error:?}");

        for secret in [upload_secret, response_secret] {
            assert!(
                !serialized.contains(secret),
                "serialized error leaked token"
            );
            assert!(!debug.contains(secret), "debug error leaked token");
        }
        assert!(serialized.contains(&format!("http://{address}/response")));
        server.abort();
    }

    #[tokio::test]
    async fn transient_final_put_is_retried_within_submission_budget() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_attempts = attempts.clone();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/response", put(fail_once_then_accept))
                    .with_state(server_attempts),
            )
            .await
            .unwrap();
        });

        let mut envelope = create_test_envelope();
        envelope.response_handling.submit_response_url = format!("http://{address}/response");
        submit_response_with_timeout(
            &envelope,
            create_test_response(b"ok"),
            Duration::from_secs(2),
        )
        .await
        .expect("second PUT should terminalize the command");

        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn retrying_final_put_does_not_repeat_successful_blob_upload() {
        let state = UploadThenRetryState {
            uploads: Arc::new(AtomicUsize::new(0)),
            submissions: Arc::new(AtomicUsize::new(0)),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_state = state.clone();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/upload", put(accept_upload))
                    .route("/response", put(fail_first_submission))
                    .with_state(server_state),
            )
            .await
            .unwrap();
        });

        let mut envelope = create_test_envelope();
        envelope.response_handling.max_inline_bytes = 1;
        envelope.response_handling.submit_response_url = format!("http://{address}/response");
        envelope.response_handling.storage_upload_request = PresignedRequest::new_http(
            format!("http://{address}/upload"),
            "PUT".to_string(),
            std::collections::HashMap::new(),
            PresignedOperation::Put,
            "test-path".to_string(),
            Utc::now() + chrono::Duration::hours(1),
        );

        submit_response_with_timeout(
            &envelope,
            create_test_response(b"large-response"),
            Duration::from_secs(2),
        )
        .await
        .expect("retrying the terminal PUT should not re-upload its blob");

        assert_eq!(state.uploads.load(Ordering::SeqCst), 1);
        assert_eq!(state.submissions.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn permanent_final_put_rejection_is_not_retried() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_attempts = attempts.clone();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/response", put(reject_permanently))
                    .with_state(server_attempts),
            )
            .await
            .unwrap();
        });

        let mut envelope = create_test_envelope();
        envelope.response_handling.submit_response_url = format!("http://{address}/response");
        let error = submit_response_with_timeout(
            &envelope,
            create_test_response(b"ok"),
            Duration::from_secs(2),
        )
        .await
        .expect_err("400 is a permanent submission rejection");

        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!(error.message.contains("400"));
        server.abort();
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
            trace_context: None,
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
