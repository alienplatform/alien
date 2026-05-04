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
///
/// Inbound: deployment bearer — the validator must yield a
/// `Scope::Deployment`. The handler builds [`TelemetryCaller`] from the
/// subject's scope and never reads a deployment record; the deployment id
/// the bearer was issued for is sufficient.
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
    // The validator already bound `Subject` to the deployment whose token
    // we hold (`Scope::Deployment` carries `project_id` and `deployment_id`;
    // `subject.workspace_id` comes from the issuer). Everything the
    // telemetry forward needs is on the subject — round-tripping through
    // `deployment_store.get_deployment` only added a redundant lookup.
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let (deployment_id, project_id) = match &subject.scope {
        crate::auth::Scope::Deployment {
            deployment_id,
            project_id,
        } => (deployment_id.clone(), project_id.clone()),
        _ => {
            return ErrorData::forbidden("Telemetry ingestion requires a deployment token")
                .into_response()
        }
    };

    if !state.authz.can_ingest_telemetry_for(&subject, &deployment_id) {
        return ErrorData::forbidden("Cannot ingest telemetry for this deployment")
            .into_response();
    }

    let caller = TelemetryCaller {
        deployment_id,
        project_id: Some(project_id),
        workspace_id: Some(subject.workspace_id.clone()),
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
