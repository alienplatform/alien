//! Telemetry loop - pushes buffered telemetry to manager

use crate::{db::OperatorDb, OperatorState};
use alien_error::{Context, IntoAlienError};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Run the telemetry loop
///
/// This loop:
/// 1. Gets buffered telemetry from local database
/// 2. Pushes to manager via raw HTTP (protobuf data)
/// 3. Deletes pushed telemetry on success, and telemetry the manager rejects
///    with a permanent 4xx (counted and logged with the manager's reason)
///
/// Rows that fail with a 5xx, another retryable status, or a network error
/// stay in the database and are retried on the next pass.
pub async fn run_telemetry_loop(state: Arc<OperatorState>) {
    let interval = Duration::from_secs(state.config.telemetry_interval_seconds);

    let sync_config = match &state.config.sync {
        Some(config) => config,
        None => {
            info!("Sync configuration not provided, telemetry loop disabled");
            return;
        }
    };

    // Create HTTP client with authentication
    let client = match create_authenticated_client(&sync_config.token) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to create authenticated HTTP client");
            return;
        }
    };

    info!(
        interval_seconds = state.config.telemetry_interval_seconds,
        "Starting telemetry loop"
    );

    let mut dropped_total: usize = 0;

    loop {
        match push_telemetry(&state, &client, &sync_config.url).await {
            Ok(outcome) => {
                if outcome.pushed > 0 {
                    debug!(pushed = outcome.pushed, "Pushed telemetry to manager");
                }
                if outcome.dropped > 0 {
                    dropped_total += outcome.dropped;
                    warn!(
                        dropped = outcome.dropped,
                        dropped_since_start = dropped_total,
                        "Dropped telemetry the manager rejected"
                    );
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to push telemetry");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {},
            _ = state.cancel.cancelled() => {
                info!("Telemetry loop shutting down");
                return;
            }
        }
    }
}

/// Create an authenticated HTTP client
fn create_authenticated_client(token: &str) -> crate::error::Result<reqwest::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    let auth_value = format!("Bearer {}", token);
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(crate::error::ErrorData::SyncFailed {
                message: "Invalid auth token".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("operator"));

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::SyncFailed {
            message: "Failed to build HTTP client".to_string(),
        })
}

/// What one pass over the buffered telemetry did.
#[derive(Debug, Default, PartialEq, Eq)]
struct PushOutcome {
    /// Rows the manager accepted.
    pushed: usize,
    /// Rows the manager permanently rejected and the operator deleted.
    dropped: usize,
}

/// Whether a response status means the backend will never accept this row.
///
/// Matches the statuses the air-gapped report path treats as permanent
/// rejections. Other 4xx statuses (401, 403, 408, 429, ...) describe the
/// caller or the moment, not the row, so those rows are kept and retried.
fn is_permanent_rejection(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 400 | 413 | 422)
}

/// The backend's reason for a rejection: the `message` of a JSON error body,
/// or the raw body text, cut to a length that is safe to log.
fn rejection_reason(body: &str) -> String {
    const MAX_REASON_CHARS: usize = 500;
    let reason = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("message")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| body.trim().to_owned());
    reason.chars().take(MAX_REASON_CHARS).collect()
}

/// Read the start of a rejection body for the log line. Bounded in time and
/// size so a stalled or oversized body can't hold up the rows behind it; a
/// failed read just leaves the reason empty.
async fn read_rejection_body(mut response: reqwest::Response) -> String {
    const MAX_BODY_BYTES: usize = 4096;
    const READ_TIMEOUT: Duration = Duration::from_secs(5);

    let mut body = Vec::new();
    let read = async {
        while body.len() < MAX_BODY_BYTES {
            match response.chunk().await {
                Ok(Some(chunk)) => body.extend_from_slice(&chunk),
                Ok(None) | Err(_) => break,
            }
        }
    };
    // On timeout, keep what arrived so far.
    let _ = tokio::time::timeout(READ_TIMEOUT, read).await;
    body.truncate(MAX_BODY_BYTES);
    String::from_utf8_lossy(&body).into_owned()
}

async fn push_telemetry(
    state: &OperatorState,
    client: &reqwest::Client,
    base_url: &url::Url,
) -> crate::error::Result<PushOutcome> {
    push_pending_telemetry(&state.db, client, base_url).await
}

async fn push_pending_telemetry(
    db: &OperatorDb,
    client: &reqwest::Client,
    base_url: &url::Url,
) -> crate::error::Result<PushOutcome> {
    // Get pending telemetry
    let pending = db.get_pending_telemetry(100).await?;

    if pending.is_empty() {
        return Ok(PushOutcome::default());
    }

    let mut pushed_ids = Vec::new();
    let mut dropped_ids = Vec::new();

    for (id, telemetry_type, data) in &pending {
        let endpoint_path = match telemetry_type.as_str() {
            "logs" => "v1/logs",
            "metrics" => "v1/metrics",
            "traces" => "v1/traces",
            _ => {
                debug!(id = id, telemetry_type = %telemetry_type, "Unknown telemetry type");
                continue;
            }
        };

        let endpoint = match base_url.join(endpoint_path) {
            Ok(url) => url,
            Err(e) => {
                debug!(id = id, error = %e, "Failed to construct endpoint URL");
                continue;
            }
        };

        let result = client
            .post(endpoint)
            .header("Content-Type", "application/x-protobuf")
            .body(data.clone())
            .send()
            .await;

        match result {
            Ok(response) if response.status().is_success() => {
                match response.json::<TelemetryResponse>().await {
                    Ok(resp) if resp.accepted => {
                        pushed_ids.push(*id);
                    }
                    Ok(_) => {
                        debug!(id = id, "Telemetry not accepted, will retry");
                    }
                    Err(e) => {
                        debug!(id = id, error = %e, "Failed to parse telemetry response");
                    }
                }
            }
            Ok(response) if is_permanent_rejection(response.status()) => {
                let status = response.status().as_u16();
                let body = read_rejection_body(response).await;
                // Retrying can't help, and the row would keep its place among
                // the oldest rows read each pass. Drop it so the rows behind
                // it can be pushed.
                warn!(
                    id = id,
                    telemetry_type = %telemetry_type,
                    bytes = data.len(),
                    status = status,
                    reason = %rejection_reason(&body),
                    "Telemetry rejected by the manager, dropping it"
                );
                dropped_ids.push(*id);
            }
            Ok(response) => {
                debug!(
                    id = id,
                    status = response.status().as_u16(),
                    "Telemetry request failed, will retry"
                );
            }
            Err(e) => {
                debug!(id = id, error = %e, "Failed to push telemetry entry, will retry");
            }
        }
    }

    // Delete pushed telemetry and rows the manager will never accept
    let finished: Vec<i64> = pushed_ids.iter().chain(&dropped_ids).copied().collect();
    if !finished.is_empty() {
        db.delete_telemetry(&finished).await?;
    }

    Ok(PushOutcome {
        pushed: pushed_ids.len(),
        dropped: dropped_ids.len(),
    })
}

/// Response from telemetry endpoints
#[derive(serde::Deserialize)]
struct TelemetryResponse {
    accepted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Bytes,
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::post,
        Json, Router,
    };
    use std::sync::{
        atomic::{AtomicU16, Ordering},
        Mutex,
    };

    const BAD: &[u8] = b"bad-timestamp";
    const TEST_ENCRYPTION_KEY: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// A manager stand-in. A body containing `BAD` is rejected with the
    /// status in `bad_status` and a JSON error body. Every other body gets
    /// `ok_status`, with the success body when that is 200.
    struct Stub {
        bad_status: AtomicU16,
        ok_status: AtomicU16,
        requests: Mutex<Vec<Vec<u8>>>,
    }

    impl Stub {
        fn requests_containing(&self, needle: &[u8]) -> usize {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|body| body.windows(needle.len()).any(|w| w == needle))
                .count()
        }
    }

    async fn handle(State(stub): State<Arc<Stub>>, body: Bytes) -> Response {
        stub.requests.lock().unwrap().push(body.to_vec());
        let is_bad = body.windows(BAD.len()).any(|w| w == BAD);
        let status = if is_bad {
            stub.bad_status.load(Ordering::SeqCst)
        } else {
            stub.ok_status.load(Ordering::SeqCst)
        };
        match status {
            200 => Json(serde_json::json!({"accepted": true})).into_response(),
            status => (
                StatusCode::from_u16(status).unwrap(),
                Json(serde_json::json!({
                    "code": "TEST_REJECTED",
                    "message": "timestamp is past the supported range",
                    "retryable": false,
                })),
            )
                .into_response(),
        }
    }

    struct Harness {
        db: OperatorDb,
        stub: Arc<Stub>,
        base_url: url::Url,
        client: reqwest::Client,
        _dir: tempfile::TempDir,
    }

    async fn harness() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let db = OperatorDb::new(dir.path().to_str().unwrap(), TEST_ENCRYPTION_KEY)
            .await
            .unwrap();
        let stub = Arc::new(Stub {
            bad_status: AtomicU16::new(400),
            ok_status: AtomicU16::new(200),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/v1/logs", post(handle))
            .with_state(stub.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = url::Url::parse(&format!("http://{}/", listener.local_addr().unwrap()))
            .unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Harness {
            db,
            stub,
            base_url,
            client: reqwest::Client::new(),
            _dir: dir,
        }
    }

    async fn pending_count(db: &OperatorDb) -> usize {
        db.get_pending_telemetry(1000).await.unwrap().len()
    }

    #[tokio::test]
    async fn rejected_row_is_dropped_once_and_later_rows_are_delivered() {
        let h = harness().await;
        // The status the manager answers with for records it rejects.
        h.stub.bad_status.store(422, Ordering::SeqCst);
        h.db.store_telemetry("logs", b"first").await.unwrap();
        h.db.store_telemetry("logs", BAD).await.unwrap();
        h.db.store_telemetry("logs", b"last").await.unwrap();

        let outcome = push_pending_telemetry(&h.db, &h.client, &h.base_url)
            .await
            .unwrap();
        assert_eq!(outcome, PushOutcome { pushed: 2, dropped: 1 });
        assert_eq!(pending_count(&h.db).await, 0);

        // A later pass must not send the dropped row again.
        let outcome = push_pending_telemetry(&h.db, &h.client, &h.base_url)
            .await
            .unwrap();
        assert_eq!(outcome, PushOutcome::default());
        assert_eq!(h.stub.requests_containing(BAD), 1);
        assert_eq!(h.stub.requests_containing(b"first"), 1);
        assert_eq!(h.stub.requests_containing(b"last"), 1);
    }

    #[tokio::test]
    async fn a_full_page_of_rejected_rows_does_not_block_the_rows_behind_it() {
        let h = harness().await;
        for _ in 0..100 {
            h.db.store_telemetry("logs", BAD).await.unwrap();
        }
        h.db.store_telemetry("logs", b"behind-the-queue")
            .await
            .unwrap();

        let mut dropped = 0;
        let mut pushed = 0;
        for _ in 0..2 {
            let outcome = push_pending_telemetry(&h.db, &h.client, &h.base_url)
                .await
                .unwrap();
            dropped += outcome.dropped;
            pushed += outcome.pushed;
        }
        assert_eq!((pushed, dropped), (1, 100));
        assert_eq!(pending_count(&h.db).await, 0);
        assert_eq!(h.stub.requests_containing(BAD), 100);
        assert_eq!(h.stub.requests_containing(b"behind-the-queue"), 1);
    }

    #[tokio::test]
    async fn server_errors_keep_rows_for_retry() {
        let h = harness().await;
        h.stub.bad_status.store(500, Ordering::SeqCst);
        h.db.store_telemetry("logs", BAD).await.unwrap();

        for _ in 0..2 {
            let outcome = push_pending_telemetry(&h.db, &h.client, &h.base_url)
                .await
                .unwrap();
            assert_eq!(outcome, PushOutcome::default());
        }
        assert_eq!(pending_count(&h.db).await, 1);
        assert_eq!(h.stub.requests_containing(BAD), 2);

        // Once the backend recovers, the same row goes through.
        h.stub.bad_status.store(200, Ordering::SeqCst);
        let outcome = push_pending_telemetry(&h.db, &h.client, &h.base_url)
            .await
            .unwrap();
        assert_eq!(outcome, PushOutcome { pushed: 1, dropped: 0 });
        assert_eq!(pending_count(&h.db).await, 0);
    }

    #[tokio::test]
    async fn statuses_that_describe_the_caller_not_the_row_keep_rows() {
        let h = harness().await;
        for status in [401, 403, 408, 429] {
            h.stub.bad_status.store(status, Ordering::SeqCst);
            h.db.store_telemetry("logs", BAD).await.unwrap();
            let outcome = push_pending_telemetry(&h.db, &h.client, &h.base_url)
                .await
                .unwrap();
            assert_eq!(outcome, PushOutcome::default(), "status {status}");
            assert_eq!(pending_count(&h.db).await, 1, "status {status}");
            // Clear the row for the next status.
            let id = h.db.get_pending_telemetry(1).await.unwrap()[0].0;
            h.db.delete_telemetry(&[id]).await.unwrap();
        }
    }

    #[test]
    fn rejection_reason_prefers_the_json_message_and_is_bounded() {
        assert_eq!(
            rejection_reason(r#"{"code":"X","message":"timestamp too large"}"#),
            "timestamp too large"
        );
        assert_eq!(rejection_reason("  plain text  "), "plain text");
        assert_eq!(rejection_reason(&"a".repeat(2000)).chars().count(), 500);
    }
}
