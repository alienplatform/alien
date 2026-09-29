//! Air-gapped deployments: the sync exchange, carried by hand.
//!
//! - `GET /v1/deployments/{id}/target` returns the target sync would deliver
//!   for a release (and records it as the deployment's desired release), for
//!   `alien airgap bundle` to package.
//! - `POST /v1/deployments/{id}/bundle-signature` signs a bundle's manifest
//!   with the manager's bundle signing key, which the environment verifies.
//! - `POST /v1/deployments/{id}/status-report` accepts the state and logs an
//!   environment exported with `alien-deploy airgap status`, reconciled the
//!   same way a sync is, with logs passed to the telemetry backend.

use std::collections::HashMap;

use alien_core::{sync::TargetDeployment, DeploymentState, Platform};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use opentelemetry_proto::tonic::{
    collector::logs::v1::ExportLogsServiceRequest,
    common::v1::{any_value::Value, AnyValue, KeyValue},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
    resource::v1::Resource,
};
use prost::Message;
use serde::{Deserialize, Serialize};

use super::{auth, AppState};
use crate::{
    error::ErrorData,
    traits::{ReconcileData, TelemetryCaller, TelemetrySignal},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/deployments/{id}/target", get(target))
        .route(
            "/v1/deployments/{id}/bundle-signature",
            post(bundle_signature),
        )
        .route("/v1/deployments/{id}/status-report", post(status_report))
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[serde(rename_all = "camelCase")]
pub struct TargetQuery {
    /// Release to target; defaults to the latest release.
    pub release_id: Option<String>,
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployments/{id}/target",
    operation_id = "get_deployment_target",
    tag = "deployments",
    params(("id" = String, Path, description = "Deployment ID"), TargetQuery),
    responses((status = 200, description = "Target the deployment converges to", body = serde_json::Value)),
    security(("bearer" = []))
))]
async fn target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<TargetQuery>,
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
    if !state.authz.can_update_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Cannot target this deployment").into_response();
    }
    if deployment.platform != Platform::Kubernetes {
        return ErrorData::bad_request("Air-gapped bundles are for Kubernetes deployments")
            .into_response();
    }

    let release = match &query.release_id {
        Some(release_id) => state.release_store.get_release(&subject, release_id).await,
        None => state.release_store.get_latest_release(&subject).await,
    };
    let release = match release {
        Ok(Some(release)) => release,
        Ok(None) => return ErrorData::bad_request("No such release").into_response(),
        Err(e) => return e.into_response(),
    };
    if let Err(e) = state
        .deployment_store
        .set_deployment_desired_release(&subject, &deployment.id, &release.id)
        .await
    {
        return e.into_response();
    }

    // No token: the environment pulls from its own registry and never
    // calls the manager. Bundles are applied by Operators that support
    // air-gapped mode, which all understand tunnels.
    let mut target: TargetDeployment =
        match super::sync::build_pull_target(&state, &deployment, release, None, true).await {
            Ok(target) => target,
            Err(response) => return response,
        };
    // Workloads can't reach this manager to export telemetry.
    target.config.monitoring = None;
    Json(target).into_response()
}

/// Body of `POST /v1/deployments/{id}/bundle-signature`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BundleSignatureRequest {
    /// The bundle's `manifest.json`, base64-encoded, exactly as packaged.
    pub manifest: String,
}

/// A signature over a bundle manifest.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BundleSignatureResponse {
    /// `ed25519:<base64>` signature over the manifest bytes.
    pub signature: String,
    /// `ed25519:<base64>` public key that verifies it.
    pub public_key: String,
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/deployments/{id}/bundle-signature",
    operation_id = "sign_deployment_bundle",
    tag = "deployments",
    params(("id" = String, Path, description = "Deployment ID")),
    request_body = BundleSignatureRequest,
    responses((status = 200, description = "Signature over the manifest", body = BundleSignatureResponse)),
    security(("bearer" = []))
))]
async fn bundle_signature(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<BundleSignatureRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(e) => return e.into_response(),
    };
    let Some(key) = state
        .charts
        .as_ref()
        .and_then(|charts| charts.bundle_signing_key.clone())
    else {
        return ErrorData::bad_request("This manager doesn't sign air-gapped bundles")
            .into_response();
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
    if !state.authz.can_update_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Cannot sign bundles for this deployment").into_response();
    }
    let Ok(manifest) = base64::engine::general_purpose::STANDARD.decode(&body.manifest) else {
        return ErrorData::bad_request("manifest must be base64").into_response();
    };
    // Sign only manifests for this deployment, so a signature can't be
    // reused to install another deployment's bundle.
    let manifest_deployment = serde_json::from_slice::<serde_json::Value>(&manifest)
        .ok()
        .and_then(|value| value.get("deploymentId")?.as_str().map(str::to_string));
    if manifest_deployment.as_deref() != Some(deployment.id.as_str()) {
        return ErrorData::bad_request("The manifest is not a bundle manifest for this deployment")
            .into_response();
    }
    Json(BundleSignatureResponse {
        signature: key.sign(&manifest),
        public_key: key.public_key(),
    })
    .into_response()
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    /// Deployment state as the environment's Operator last recorded it.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub state: DeploymentState,
    /// Recent log lines collected from the environment.
    #[serde(default)]
    pub logs: Vec<ReportedLog>,
}

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReportedLog {
    pub timestamp: DateTime<Utc>,
    /// Workload that wrote the line.
    pub resource: Option<String>,
    pub message: String,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatusReportResponse {
    pub status: String,
    pub logs_accepted: usize,
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/deployments/{id}/status-report",
    operation_id = "import_deployment_status",
    tag = "deployments",
    params(("id" = String, Path, description = "Deployment ID")),
    request_body = StatusReport,
    responses((status = 200, description = "Report applied", body = StatusReportResponse)),
    security(("bearer" = []))
))]
async fn status_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(report): Json<StatusReport>,
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
    if !state.authz.can_update_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Cannot report for this deployment").into_response();
    }

    let status = serde_json::to_value(&report.state.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    if let Err(e) = state
        .deployment_store
        .reconcile(
            &subject,
            ReconcileData {
                deployment_id: deployment.id.clone(),
                session: "airgap-status-report".to_string(),
                state: report.state,
                update_heartbeat: true,
                suggested_delay_ms: None,
                heartbeats: Vec::new(),
                observed_inventory_batches: Vec::new(),
                capabilities: Vec::new(),
                operator_version: None,
                execution_claim: None,
                operations_report: None,
            },
        )
        .await
    {
        return e.into_response();
    }

    let logs_accepted = report.logs.len();
    if logs_accepted > 0 {
        let caller = TelemetryCaller {
            deployment_id: Some(deployment.id.clone()),
            project_id: Some(deployment.project_id.clone()),
            workspace_id: Some(deployment.workspace_id.clone()),
            gateway_log_source: None,
        };
        let batch = logs_to_otlp(&deployment.id, report.logs).encode_to_vec();
        if let Err(e) = state
            .telemetry_backend
            .ingest(TelemetrySignal::Logs, &caller, batch.into())
            .await
        {
            return e.into_response();
        }
    }

    (
        StatusCode::OK,
        Json(StatusReportResponse {
            status,
            logs_accepted,
        }),
    )
        .into_response()
}

/// Reported lines as an OTLP batch, one resource per workload, tagged like
/// live telemetry so backends can't tell carried logs from streamed ones
/// except by `alien.log.source`.
fn logs_to_otlp(deployment_id: &str, logs: Vec<ReportedLog>) -> ExportLogsServiceRequest {
    let string = |value: String| {
        Some(AnyValue {
            value: Some(Value::StringValue(value)),
        })
    };
    let mut by_resource: HashMap<Option<String>, Vec<LogRecord>> = HashMap::new();
    for log in logs {
        let nanos = log
            .timestamp
            .timestamp_nanos_opt()
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or_default();
        let mut attributes: Vec<KeyValue> = log
            .attributes
            .into_iter()
            .map(|(key, value)| KeyValue {
                key,
                value: string(value),
            })
            .collect();
        attributes.push(KeyValue {
            key: "alien.log.source".to_string(),
            value: string("airgap-status-report".to_string()),
        });
        by_resource
            .entry(log.resource)
            .or_default()
            .push(LogRecord {
                time_unix_nano: nanos,
                observed_time_unix_nano: nanos,
                body: string(log.message),
                attributes,
                ..Default::default()
            });
    }
    ExportLogsServiceRequest {
        resource_logs: by_resource
            .into_iter()
            .map(|(resource, records)| {
                let mut attributes = vec![KeyValue {
                    key: "alien.deployment_id".to_string(),
                    value: string(deployment_id.to_string()),
                }];
                if let Some(resource) = resource {
                    attributes.push(KeyValue {
                        key: "service.name".to_string(),
                        value: string(resource),
                    });
                }
                ResourceLogs {
                    resource: Some(Resource {
                        attributes,
                        ..Default::default()
                    }),
                    scope_logs: vec![ScopeLogs {
                        log_records: records,
                        ..Default::default()
                    }],
                    ..Default::default()
                }
            })
            .collect(),
    }
}
