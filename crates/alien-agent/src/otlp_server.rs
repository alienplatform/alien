//! OTLP server for local functions
//!
//! Functions running locally send telemetry to this server instead of
//! directly to the manager. The Agent buffers and forwards telemetry.

use alien_error::{Context, IntoAlienError};
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::db::AgentDb;

/// Shared state for axum handlers — the agent DB and the readiness signal.
#[derive(Clone)]
pub struct ProbeState {
    pub db: Arc<AgentDb>,
    /// Flipped to `true` by the sync loop once at least one /v1/sync round-trip
    /// with the manager has succeeded. Consumed by `/readyz`.
    pub first_sync_completed: Arc<AtomicBool>,
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
    db: Arc<AgentDb>,
    first_sync_completed: Arc<AtomicBool>,
    cancel: CancellationToken,
) -> crate::error::Result<()> {
    let addr = SocketAddr::new(host, port);

    info!(address = %addr, "Starting OTLP server");

    let probe_state = ProbeState {
        db: db.clone(),
        first_sync_completed,
    };

    // Probes ride on a separate Router so they don't need the OTLP body-limit
    // layer and have a different (slim) state type; they are merged into the
    // OTLP app at the end.
    let probes = Router::new()
        .route("/livez", get(handle_livez))
        .route("/readyz", get(handle_readyz))
        .with_state(probe_state);

    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/v1/logs", post(handle_logs))
        .route("/v1/metrics", post(handle_metrics))
        .route("/v1/traces", post(handle_traces))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .with_state(db)
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

/// Readiness probe. Returns 200 only after the agent has completed at
/// least one successful `/v1/sync` round-trip with the manager — the gate
/// the chart's `readinessProbe` and Helm's `--atomic --wait` rely on so a
/// freshly-rolled agent isn't considered ready until it has actually
/// proven it can reach the manager.
///
/// Other readiness preconditions (process alive, DB opened, InstanceLock
/// held) are implicit — the agent only starts this server after acquiring
/// the InstanceLock and opening the DB. The deployment-loop-progressing
/// check is a follow-up.
async fn handle_readyz(State(state): State<ProbeState>) -> Result<Json<HealthResponse>, StatusCode> {
    if !state.first_sync_completed.load(Ordering::Acquire) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(Json(HealthResponse { status: "ok" }))
}

async fn handle_logs(
    State(db): State<Arc<AgentDb>>,
    body: Bytes,
) -> Result<Json<OtlpResponse>, StatusCode> {
    debug!(bytes = body.len(), "Received OTLP logs");

    if let Err(e) = db.store_telemetry("logs", &body).await {
        error!(error = %e, "Failed to store logs");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(OtlpResponse { accepted: true }))
}

async fn handle_metrics(
    State(db): State<Arc<AgentDb>>,
    body: Bytes,
) -> Result<Json<OtlpResponse>, StatusCode> {
    debug!(bytes = body.len(), "Received OTLP metrics");

    if let Err(e) = db.store_telemetry("metrics", &body).await {
        error!(error = %e, "Failed to store metrics");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(OtlpResponse { accepted: true }))
}

async fn handle_traces(
    State(db): State<Arc<AgentDb>>,
    body: Bytes,
) -> Result<Json<OtlpResponse>, StatusCode> {
    debug!(bytes = body.len(), "Received OTLP traces");

    if let Err(e) = db.store_telemetry("traces", &body).await {
        error!(error = %e, "Failed to store traces");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(OtlpResponse { accepted: true }))
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

    async fn start_test_server(
        first_sync_completed: Arc<AtomicBool>,
    ) -> (u16, CancellationToken, tokio::task::JoinHandle<crate::error::Result<()>>) {
        let data_dir = tempfile::tempdir().unwrap();
        // Leak the tempdir guard so it outlives the spawned server.
        let data_dir = Box::leak(Box::new(data_dir));
        let db = Arc::new(
            AgentDb::new(data_dir.path().to_str().unwrap(), TEST_ENCRYPTION_KEY)
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
                first_sync_completed,
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
        let readiness = Arc::new(AtomicBool::new(false));
        let (port, cancel, server) = start_test_server(readiness.clone()).await;
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
        let readiness = Arc::new(AtomicBool::new(false));
        let (port, cancel, server) = start_test_server(readiness.clone()).await;
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
        let readiness = Arc::new(AtomicBool::new(false));
        let (port, cancel, server) = start_test_server(readiness.clone()).await;
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/readyz");

        // Before the sync loop flips the flag, readyz must be 503 so that
        // a freshly-rolled agent isn't marked ready until it has actually
        // talked to the manager once.
        let before = client.get(&url).send().await.unwrap();
        assert_eq!(before.status().as_u16(), 503, "expected 503 before first sync");

        // Simulate the sync loop completing its first round-trip.
        readiness.store(true, Ordering::Release);

        let after = client.get(&url).send().await.unwrap();
        assert!(after.status().is_success(), "expected 200 after first sync");

        cancel.cancel();
        server.await.unwrap().unwrap();
    }
}
