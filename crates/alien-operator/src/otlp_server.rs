//! OTLP server for local functions
//!
//! Functions running locally send telemetry to this server instead of
//! directly to the manager. The Operator buffers and forwards telemetry.

use alien_error::{Context, IntoAlienError};
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
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
    /// Set when air-gapped: authorizes `GET /airgap/telemetry`.
    airgap_export_token: Option<String>,
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
    airgap_export_token: Option<String>,
    sandbox_broker: Option<axum::Router>,
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
        .route("/airgap/telemetry", get(handle_airgap_export))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .with_state(OtlpServerState {
            db,
            namespace,
            collector_token,
            airgap_export_token,
        });

    // The sandbox broker shares this server because the chart already exposes this port through
    // the operator's Service. A second listener would need a second port and a chart change to
    // reach the same pods.
    let app = match sandbox_broker {
        Some(broker) => app.merge(broker),
        None => app,
    };

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

#[derive(Deserialize)]
struct AirgapExportQuery {
    /// Return batches with an ID above this one.
    #[serde(default)]
    after: i64,
    #[serde(default = "default_export_limit")]
    limit: u32,
}

fn default_export_limit() -> u32 {
    200
}

/// One buffered OTLP batch, as the vendor's manager ingests it.
#[derive(Serialize)]
struct AirgapTelemetryBatch {
    id: i64,
    /// `logs`, `metrics` or `traces`.
    signal: String,
    /// OTLP protobuf, base64.
    data: String,
}

#[derive(Serialize)]
struct AirgapTelemetryPage {
    items: Vec<AirgapTelemetryBatch>,
    /// Batches dropped so far to keep the buffer under its limit.
    dropped: u64,
}

/// Buffered telemetry for `alien-deploy sync` to carry out of an air-gapped
/// site. Batches stay buffered until a signed bundle acknowledges them.
async fn handle_airgap_export(
    State(state): State<OtlpServerState>,
    headers: HeaderMap,
    Query(query): Query<AirgapExportQuery>,
) -> Response {
    let Some(expected) = &state.airgap_export_token else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if presented != Some(expected.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let batches = match state
        .db
        .telemetry_after(query.after, query.limit.clamp(1, 1000))
        .await
    {
        Ok(batches) => batches,
        Err(e) => {
            error!(error = %e, "Failed to read buffered telemetry");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let dropped = match state.db.airgap_telemetry_dropped().await {
        Ok(dropped) => dropped,
        Err(e) => {
            error!(error = %e, "Failed to read dropped telemetry");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    Json(AirgapTelemetryPage {
        items: batches
            .into_iter()
            .map(|(id, signal, data)| AirgapTelemetryBatch {
                id,
                signal,
                data: base64::engine::general_purpose::STANDARD.encode(data),
            })
            .collect(),
        dropped,
    })
    .into_response()
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

    #[tokio::test]
    async fn airgap_export_pages_through_the_buffer_until_acknowledged() {
        let data_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            OperatorDb::new(data_dir.path().to_str().unwrap(), TEST_ENCRYPTION_KEY)
                .await
                .unwrap(),
        );
        for i in 0..5u8 {
            db.store_telemetry(if i % 2 == 0 { "logs" } else { "traces" }, &[i; 100])
                .await
                .unwrap();
        }
        let port = free_port();
        let cancel = CancellationToken::new();
        let server_cancel = cancel.clone();
        let server_db = db.clone();
        let server = tokio::spawn(async move {
            start_otlp_server(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                port,
                server_db,
                None,
                None,
                Some("export-secret".to_string()),
                None,
                server_cancel,
            )
            .await
        });
        let client = reqwest::Client::new();
        let url =
            |after: i64| format!("http://127.0.0.1:{port}/airgap/telemetry?after={after}&limit=3");
        for _ in 0..50 {
            if client.get(url(0)).send().await.is_ok() {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }

        let unauthorized = client.get(url(0)).send().await.unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

        let page = |after: i64| {
            let client = client.clone();
            let url = url(after);
            async move {
                client
                    .get(url)
                    .bearer_auth("export-secret")
                    .send()
                    .await
                    .unwrap()
                    .json::<serde_json::Value>()
                    .await
                    .unwrap()
            }
        };
        let first = page(0).await;
        let ids: Vec<i64> = first["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_i64().unwrap())
            .collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(first["items"][0]["signal"], "logs");
        assert_eq!(first["items"][1]["signal"], "traces");
        let data = base64::engine::general_purpose::STANDARD
            .decode(first["items"][1]["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(data, vec![1u8; 100], "batches keep their bytes");
        let second = page(ids[2]).await;
        assert_eq!(second["items"].as_array().unwrap().len(), 2);

        // Exporting doesn't free anything; the acknowledgement does.
        assert_eq!(page(0).await["items"].as_array().unwrap().len(), 3);
        assert_eq!(db.delete_telemetry_through(ids[2]).await.unwrap(), 3);
        let rest = page(0).await;
        assert_eq!(rest["items"].as_array().unwrap().len(), 2);
        assert_eq!(rest["dropped"], 0);

        // Over the limit, the oldest go and are counted.
        let dropped = db.prune_telemetry(100).await.unwrap();
        assert_eq!(dropped, 1);
        let after_prune = page(0).await;
        assert_eq!(after_prune["items"].as_array().unwrap().len(), 1);
        assert_eq!(after_prune["dropped"], 1);

        cancel.cancel();
        server.await.unwrap().unwrap();
    }
}
