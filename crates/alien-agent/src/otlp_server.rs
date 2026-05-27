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
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::db::AgentDb;

/// OTLP response
#[derive(Serialize)]
struct OtlpResponse {
    accepted: bool,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// Start the OTLP server with graceful shutdown support.
pub async fn start_otlp_server(
    host: IpAddr,
    port: u16,
    db: Arc<AgentDb>,
    cancel: CancellationToken,
) -> crate::error::Result<()> {
    let addr = SocketAddr::new(host, port);

    info!(address = %addr, "Starting OTLP server");

    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/v1/logs", post(handle_logs))
        .route("/v1/metrics", post(handle_metrics))
        .route("/v1/traces", post(handle_traces))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .with_state(db);

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

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let data_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            AgentDb::new(data_dir.path().to_str().unwrap(), TEST_ENCRYPTION_KEY)
                .await
                .unwrap(),
        );
        let port = free_port();
        let cancel = CancellationToken::new();
        let server_cancel = cancel.clone();

        let server = tokio::spawn(async move {
            start_otlp_server(IpAddr::V4(Ipv4Addr::LOCALHOST), port, db, server_cancel).await
        });

        let url = format!("http://127.0.0.1:{port}/health");
        let client = reqwest::Client::new();
        let mut response = None;
        for _ in 0..50 {
            if let Ok(success) = client.get(&url).send().await {
                response = Some(success);
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }

        let response = response.expect("health endpoint did not become reachable");
        assert!(response.status().is_success());
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap(),
            serde_json::json!({ "status": "ok" })
        );

        cancel.cancel();
        server.await.unwrap().unwrap();
    }
}
