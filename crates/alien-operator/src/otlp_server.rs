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
    db: Arc<OperatorDb>,
    namespace: Option<String>,
    collector_token: Option<String>,
    cancel: CancellationToken,
) -> crate::error::Result<()> {
    let addr = SocketAddr::new(host, port);

    info!(address = %addr, "Starting OTLP server");

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
        });

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

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let data_dir = tempfile::tempdir().unwrap();
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
                None,
                None,
                server_cancel,
            )
            .await
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
