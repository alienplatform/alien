//! OTLP server for local functions
//!
//! Functions running locally send telemetry to this server instead of
//! directly to the manager. The Operator buffers and forwards telemetry.

use alien_error::{Context, IntoAlienError};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::collector_logs::{collector_records_to_otlp, require_collector_auth};
use crate::db::OperatorDb;

#[derive(Clone)]
struct OtlpServerState {
    db: Arc<OperatorDb>,
    namespace: Option<String>,
    collector_token: Option<String>,
}

/// The operator's readiness, decomposed into the observable conditions of
/// the health contract ("an operator is healthy when…"). `/readyz` is 200
/// iff ALL of them hold; condition 1 — the process is alive — is implicit in
/// the server answering at all.
#[derive(Clone)]
pub struct ReadinessSignals {
    /// Condition 2: the encrypted state DB opened successfully. Set once at
    /// startup after `OperatorDb::new` succeeds (a DB that dies later
    /// surfaces through condition 5 — the loops start failing).
    pub db_open: Arc<AtomicBool>,
    /// Condition 3: the `InstanceLock` is held. The CLI acquires it before
    /// the operator runs and the guard lives for the process lifetime.
    pub lock_held: Arc<AtomicBool>,
    /// Condition 4: flipped to `true` by the sync loop once at least one
    /// /v1/sync round-trip with the manager has succeeded.
    pub first_sync_completed: Arc<AtomicBool>,
    /// Condition 5: the deployment loop is progressing — flipped to `false`
    /// after it errors several consecutive ticks, back to `true` on the next
    /// success (see `loops::deployment::LoopHealth`).
    pub deployment_loop_ok: Arc<AtomicBool>,
}

impl ReadinessSignals {
    /// Fresh signals: nothing proven yet (DB, lock, sync all pending) except
    /// the deployment loop, which is healthy-until-it-fails — it may not
    /// even have work to do.
    pub fn new() -> Self {
        Self {
            db_open: Arc::new(AtomicBool::new(false)),
            lock_held: Arc::new(AtomicBool::new(false)),
            first_sync_completed: Arc::new(AtomicBool::new(false)),
            deployment_loop_ok: Arc::new(AtomicBool::new(true)),
        }
    }

    /// All explicit readiness conditions hold.
    pub fn all_ready(&self) -> bool {
        self.db_open.load(Ordering::Acquire)
            && self.lock_held.load(Ordering::Acquire)
            && self.first_sync_completed.load(Ordering::Acquire)
            && self.deployment_loop_ok.load(Ordering::Acquire)
    }
}

impl Default for ReadinessSignals {
    fn default() -> Self {
        Self::new()
    }
}

/// OTLP response
#[derive(Serialize)]
struct OtlpResponse {
    accepted: bool,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// Start the OTLP server with graceful shutdown support. Also serves
/// `/livez` and `/readyz` on the same port — the Kubernetes probes the
/// chart's deployment template references.
pub async fn start_otlp_server(
    host: IpAddr,
    port: u16,
    db: Arc<OperatorDb>,
    readiness: ReadinessSignals,
    namespace: Option<String>,
    collector_token: Option<String>,
    cancel: CancellationToken,
) -> crate::error::Result<()> {
    let addr = SocketAddr::new(host, port);

    info!(address = %addr, "Starting OTLP server");

    // Probes ride on a separate Router so they don't need the OTLP body-limit
    // layer and have a different (slim) state type; they are merged into the
    // OTLP app at the end.
    let probes = Router::new()
        .route("/livez", get(handle_livez))
        .route("/readyz", get(handle_readyz))
        .with_state(readiness);

    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/v1/logs", post(handle_logs))
        .route("/v1/metrics", post(handle_metrics))
        .route("/v1/traces", post(handle_traces))
        .route("/internal/logs", post(handle_collector_logs))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .with_state(OtlpServerState {
            db,
            namespace,
            collector_token,
        })
        .merge(probes);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::ConfigurationError {
            message: format!("Failed to bind OTLP server on {addr}"),
        })?;

    axum::serve(listener, app)
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::ConfigurationError {
            message: "OTLP server error".to_string(),
        })?;

    info!("OTLP server shut down");
    Ok(())
}

async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Liveness probe. The agent is "live" as long as this server is
/// answering requests — i.e. the process is alive and tokio is not
/// deadlocked. Consumed by the Kubernetes `livenessProbe`.
async fn handle_livez() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Readiness probe: 200 iff ALL health conditions hold — DB opened,
/// InstanceLock held, at least one successful `/v1/sync` round-trip, and the
/// deployment loop progressing (the process being alive is implicit in the
/// response itself). Consumed by the chart's `readinessProbe` (and therefore
/// Helm's `--atomic --wait`) on Kubernetes and by the launcher's probation
/// gate on os-service — a freshly-swapped operator isn't considered healthy
/// until it has actually proven it can run and reach the manager.
async fn handle_readyz(
    State(readiness): State<ReadinessSignals>,
) -> Result<Json<HealthResponse>, StatusCode> {
    if !readiness.all_ready() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(Json(HealthResponse { status: "ok" }))
}

/// The health-endpoint bind override the launcher passes on spawn
/// (`ALIEN_HEALTH_ADDR=127.0.0.1:<port>`). `None` when unset (Kubernetes /
/// standalone — the config's OTLP host+port apply). An unparseable value is
/// a hard error: the launcher probes the exact address it set, so falling
/// back to the config port would make every probation fail by mismatch —
/// better to crash loudly and let the launcher's backoff surface it.
pub fn health_addr_override() -> crate::error::Result<Option<SocketAddr>> {
    parse_health_addr(std::env::var(alien_core::self_update::ENV_HEALTH_ADDR).ok())
}

fn parse_health_addr(env: Option<String>) -> crate::error::Result<Option<SocketAddr>> {
    let Some(raw) = env.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    raw.parse::<SocketAddr>()
        .map(Some)
        .into_alien_error()
        .context(crate::error::ErrorData::ConfigurationError {
            message: format!(
                "{} is set but is not a valid socket address: '{raw}'",
                alien_core::self_update::ENV_HEALTH_ADDR
            ),
        })
}

async fn handle_logs(
    State(state): State<OtlpServerState>,
    body: Bytes,
) -> Result<Json<OtlpResponse>, StatusCode> {
    debug!(bytes = body.len(), "Received OTLP logs");

    if let Err(e) = state.db.store_telemetry("logs", &body).await {
        error!(error = %e, "Failed to store logs");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(OtlpResponse { accepted: true }))
}

async fn handle_metrics(
    State(state): State<OtlpServerState>,
    body: Bytes,
) -> Result<Json<OtlpResponse>, StatusCode> {
    debug!(bytes = body.len(), "Received OTLP metrics");

    if let Err(e) = state.db.store_telemetry("metrics", &body).await {
        error!(error = %e, "Failed to store metrics");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(OtlpResponse { accepted: true }))
}

async fn handle_traces(
    State(state): State<OtlpServerState>,
    body: Bytes,
) -> Result<Json<OtlpResponse>, StatusCode> {
    debug!(bytes = body.len(), "Received OTLP traces");

    if let Err(e) = state.db.store_telemetry("traces", &body).await {
        error!(error = %e, "Failed to store traces");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(OtlpResponse { accepted: true }))
}

async fn handle_collector_logs(
    State(state): State<OtlpServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match ingest_collector_logs(state, headers, body).await {
        Ok(count) => (StatusCode::ACCEPTED, format!("accepted {count} records")).into_response(),
        Err(error) => {
            error!(error = %error, "Failed to ingest collector logs");
            let status = error
                .http_status_code
                .and_then(|code| StatusCode::from_u16(code).ok())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, error.to_string()).into_response()
        }
    }
}

async fn ingest_collector_logs(
    state: OtlpServerState,
    headers: HeaderMap,
    body: Bytes,
) -> crate::error::Result<usize> {
    require_collector_auth(&headers, state.collector_token.as_deref())?;

    let namespace = state.namespace.as_deref().ok_or_else(|| {
        alien_error::AlienError::new(crate::error::ErrorData::CollectorPayloadInvalid {
            message: "collector ingest requires a configured Kubernetes namespace".to_string(),
        })
    })?;
    let deployment_id = state.db.get_deployment_id().await?.ok_or_else(|| {
        alien_error::AlienError::new(crate::error::ErrorData::CollectorPayloadInvalid {
            message: "collector ingest requires a registered deployment id".to_string(),
        })
    })?;

    let (count, otlp) = collector_records_to_otlp(&body, namespace, &deployment_id)?;
    state.db.store_telemetry("logs", &otlp).await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, TcpListener};
    use tokio::time::{sleep, Duration};

    const TEST_ENCRYPTION_KEY: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn free_port() -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.local_addr().unwrap().port()
    }

    /// Signals with every condition already satisfied — tests flip individual
    /// flags off from here.
    fn all_ready_signals() -> ReadinessSignals {
        let signals = ReadinessSignals::new();
        signals.db_open.store(true, Ordering::Release);
        signals.lock_held.store(true, Ordering::Release);
        signals.first_sync_completed.store(true, Ordering::Release);
        signals
    }

    async fn start_test_server(
        readiness: ReadinessSignals,
    ) -> (u16, CancellationToken, tokio::task::JoinHandle<crate::error::Result<()>>) {
        let data_dir = tempfile::tempdir().unwrap();
        // Leak the tempdir guard so it outlives the spawned server.
        let data_dir = Box::leak(Box::new(data_dir));
        let db = Arc::new(
            OperatorDb::new(data_dir.path().to_str().unwrap(), TEST_ENCRYPTION_KEY)
                .await
                .unwrap(),
        );
        let port = free_port();
        let cancel = CancellationToken::new();
        let server_cancel = cancel.clone();
        let server = tokio::spawn(async move {
            start_otlp_server(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                port,
                db,
                readiness,
                None,
                None,
                server_cancel,
            )
            .await
        });
        // Wait for the server to bind so subsequent GETs don't race.
        let url = format!("http://127.0.0.1:{port}/health");
        let client = reqwest::Client::new();
        for _ in 0..50 {
            if client.get(&url).send().await.is_ok() {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
        (port, cancel, server)
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let (port, cancel, server) = start_test_server(ReadinessSignals::new()).await;
        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap(),
            serde_json::json!({ "status": "ok" })
        );
        cancel.cancel();
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn livez_returns_ok_even_before_first_sync() {
        // Nothing proven yet — livez must still be 200 (process liveness only).
        let (port, cancel, server) = start_test_server(ReadinessSignals::new()).await;
        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://127.0.0.1:{port}/livez"))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "livez should be 200 even before the agent has synced — it reflects \
             process liveness, not manager reachability"
        );
        cancel.cancel();
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn readyz_is_503_before_first_sync_and_200_after() {
        let signals = all_ready_signals();
        signals.first_sync_completed.store(false, Ordering::Release);
        let (port, cancel, server) = start_test_server(signals.clone()).await;
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/readyz");

        // Before the sync loop flips the flag, readyz must be 503 so that
        // a freshly-rolled agent isn't marked ready until it has actually
        // talked to the manager once.
        let before = client.get(&url).send().await.unwrap();
        assert_eq!(before.status().as_u16(), 503, "expected 503 before first sync");

        // Simulate the sync loop completing its first round-trip.
        signals.first_sync_completed.store(true, Ordering::Release);

        let after = client.get(&url).send().await.unwrap();
        assert!(after.status().is_success(), "expected 200 after first sync");

        cancel.cancel();
        server.await.unwrap().unwrap();
    }

    /// Every one of the four explicit conditions gates /readyz on its own:
    /// flipping any single flag off must 503, restoring it must 200.
    #[tokio::test]
    async fn readyz_requires_every_condition() {
        let signals = all_ready_signals();
        let (port, cancel, server) = start_test_server(signals.clone()).await;
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/readyz");

        let all_ok = client.get(&url).send().await.unwrap();
        assert!(all_ok.status().is_success(), "all conditions true → 200");

        let flags = [
            ("db_open", &signals.db_open),
            ("lock_held", &signals.lock_held),
            ("first_sync_completed", &signals.first_sync_completed),
            ("deployment_loop_ok", &signals.deployment_loop_ok),
        ];
        for (name, flag) in flags {
            flag.store(false, Ordering::Release);
            let degraded = client.get(&url).send().await.unwrap();
            assert_eq!(
                degraded.status().as_u16(),
                503,
                "{name}=false must gate readiness"
            );
            flag.store(true, Ordering::Release);
            let recovered = client.get(&url).send().await.unwrap();
            assert!(
                recovered.status().is_success(),
                "{name} restored must recover readiness"
            );
        }

        cancel.cancel();
        server.await.unwrap().unwrap();
    }

    /// The launcher-passed bind override parses strictly: valid → Some,
    /// absent/empty → None, garbage → loud error (a silent fallback would
    /// make every probation fail by port mismatch).
    #[test]
    fn health_addr_override_parses_strictly() {
        assert_eq!(
            parse_health_addr(Some("127.0.0.1:7799".to_string())).unwrap(),
            Some("127.0.0.1:7799".parse().unwrap())
        );
        assert_eq!(parse_health_addr(None).unwrap(), None);
        assert_eq!(parse_health_addr(Some(String::new())).unwrap(), None);
        let err = parse_health_addr(Some("not-an-addr".to_string()))
            .expect_err("garbage must be a hard error");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }
}
