//! Telemetry ingestion endpoints.
//!
//! Accepts OTLP protobuf data and forwards to the TelemetryBackend.
//! Deployment ID is extracted from the auth subject (deployment token).

use axum::{
    body::Bytes,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::error::ErrorData;
use crate::traits::TelemetrySignal;

use super::{auth, AppState};

#[derive(Debug, Serialize)]
pub struct TelemetryResponse {
    pub accepted: bool,
}

/// POST /v1/logs
pub async fn ingest_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    ingest(state, headers, body, TelemetrySignal::Logs).await
}

/// POST /v1/traces
pub async fn ingest_traces(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    ingest(state, headers, body, TelemetrySignal::Traces).await
}

/// POST /v1/metrics
pub async fn ingest_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    ingest(state, headers, body, TelemetrySignal::Metrics).await
}

async fn ingest(
    state: AppState,
    headers: HeaderMap,
    body: Bytes,
    signal: TelemetrySignal,
) -> Response {
    // Extract deployment_id from auth subject
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let deployment_id = match &subject.scope.deployment_id {
        Some(id) => id.clone(),
        None => {
            // Allow admin tokens to ingest with a generic scope
            if subject.is_admin() {
                "admin".to_string()
            } else {
                return ErrorData::forbidden("Telemetry ingestion requires a deployment token")
                    .into_response();
            }
        }
    };

    match state
        .telemetry_backend
        .ingest(signal, &deployment_id, body)
        .await
    {
        Ok(()) => Json(TelemetryResponse { accepted: true }).into_response(),
        Err(e) => e.into_response(),
    }
}
