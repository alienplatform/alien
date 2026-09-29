//! `GET /v1/deployments/{id}/logs` — recent logs the manager has received
//! from a deployment. Long-term storage and search belong to your
//! OpenTelemetry backend; this answers "what is it doing right now".

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{auth, AppState};
use crate::error::ErrorData;

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[serde(rename_all = "camelCase")]
pub struct RecentLogsQuery {
    /// Most recent entries to return (default 200, max 5000).
    pub limit: Option<usize>,
    /// Only entries at or after this time (RFC 3339). Inclusive, so a caller
    /// following logs doesn't miss entries that share its last timestamp.
    pub since: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RecentLogEntry {
    pub timestamp: DateTime<Utc>,
    pub severity: String,
    /// Workload that produced the entry (OTLP `service.name`).
    pub resource: Option<String>,
    pub message: String,
    pub attributes: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RecentLogsResponse {
    /// Oldest first.
    pub items: Vec<RecentLogEntry>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/deployments/{id}/logs", get(recent_logs))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployments/{id}/logs",
    operation_id = "get_deployment_logs",
    tag = "deployments",
    params(("id" = String, Path, description = "Deployment ID"), RecentLogsQuery),
    responses((status = 200, description = "Recent logs", body = RecentLogsResponse)),
    security(("bearer" = []))
))]
pub(crate) async fn recent_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<RecentLogsQuery>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(e) => return e.into_response(),
    };
    let deployment = match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(deployment)) => deployment,
        Ok(None) => {
            return alien_error::AlienError::new(ErrorData::DeploymentNotFound {
                deployment_id: id,
            })
            .into_response()
        }
        Err(e) => return e.into_response(),
    };
    if !state.authz.can_read_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Cannot read this deployment's logs").into_response();
    }

    let limit = query.limit.unwrap_or(200).min(5000);
    let mut entries = state
        .log_buffer
        .get_entries(Some(&deployment.id), usize::MAX)
        .await;
    if let Some(since) = query.since {
        entries.retain(|entry| entry.timestamp >= since);
    }
    entries.sort_by_key(|entry| entry.timestamp);
    let skip = entries.len().saturating_sub(limit);
    Json(RecentLogsResponse {
        items: entries
            .into_iter()
            .skip(skip)
            .map(|entry| RecentLogEntry {
                timestamp: entry.timestamp,
                severity: entry.severity,
                resource: entry.resource_name,
                message: entry.body,
                attributes: entry.attributes.into_iter().collect(),
            })
            .collect(),
    })
    .into_response()
}
