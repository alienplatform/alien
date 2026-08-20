//! Telemetry ingestion endpoints.
//!
//! Accepts OTLP protobuf data and forwards to the TelemetryBackend.
//! Scope and source are extracted exclusively from the authenticated subject.

use axum::{
    body::Bytes,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::auth::{Role, Scope, TelemetryCapability};
use crate::error::ErrorData;
use crate::traits::{TelemetryCaller, TelemetrySignal};

use super::{auth, AppState};

#[derive(Debug, Serialize)]
pub struct TelemetryResponse {
    pub accepted: bool,
}

/// POST /v1/logs
///
/// Inbound: a deployment bearer or a logs-only telemetry capability.
pub async fn ingest_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    ingest(state, headers, body, TelemetrySignal::Logs).await
}

/// POST /v1/traces
///
/// Inbound: deployment bearer. See [`ingest_logs`] doc for the auth model.
pub async fn ingest_traces(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    ingest(state, headers, body, TelemetrySignal::Traces).await
}

/// POST /v1/metrics
///
/// Inbound: deployment bearer. See [`ingest_logs`] doc for the auth model.
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
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_ingest_telemetry(&subject, signal) {
        return ErrorData::forbidden("Cannot ingest this telemetry signal").into_response();
    }

    let caller = match &subject.scope {
        Scope::Deployment {
            deployment_id,
            project_id,
        } => TelemetryCaller {
            deployment_id: Some(deployment_id.clone()),
            project_id: Some(project_id.clone()),
            workspace_id: Some(subject.workspace_id.clone()),
            gateway_log_source: None,
        },
        Scope::Telemetry {
            project_id,
            capability: TelemetryCapability::GatewayLogs { source },
        } if subject.role == Role::TelemetryCapability => TelemetryCaller {
            deployment_id: None,
            project_id: Some(project_id.clone()),
            workspace_id: Some(subject.workspace_id.clone()),
            gateway_log_source: Some(*source),
        },
        _ => {
            return ErrorData::forbidden("Telemetry subject has an unsupported scope")
                .into_response();
        }
    };

    match state.telemetry_backend.ingest(signal, &caller, body).await {
        Ok(()) => Json(TelemetryResponse { accepted: true }).into_response(),
        Err(e) => e.into_response(),
    }
}
