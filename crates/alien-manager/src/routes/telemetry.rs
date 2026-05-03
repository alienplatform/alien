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
use crate::traits::{TelemetryCaller, TelemetrySignal};

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
    // Extract deployment_id from auth subject and run the subject telemetry-
    // ingest authz check. The configured `Authz::can_ingest_telemetry_for`
    // impl decides whether this caller may ingest for this deployment.
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let deployment_id = match &subject.scope {
        crate::auth::Scope::Deployment { deployment_id, .. } => deployment_id.clone(),
        _ => {
            return ErrorData::forbidden("Telemetry ingestion requires a deployment token")
                .into_response()
        }
    };

    let deployment = match state.deployment_store.get_deployment(&deployment_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_ingest_telemetry_for(&subject, &deployment) {
        return ErrorData::forbidden("Cannot ingest telemetry for this deployment")
            .into_response();
    }

    let caller = TelemetryCaller {
        deployment_id: deployment.id.clone(),
        project_id: Some(deployment.project_id.clone()),
        workspace_id: Some(deployment.workspace_id.clone()),
    };

    match state
        .telemetry_backend
        .ingest(signal, &caller, body)
        .await
    {
        Ok(()) => Json(TelemetryResponse { accepted: true }).into_response(),
        Err(e) => e.into_response(),
    }
}
