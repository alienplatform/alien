//! Air-gapped deployments: the sync exchange, carried by hand.
//!
//! `alien-deploy sync` calls these with the site's deployment token from a
//! machine that can reach the manager:
//!
//! - `GET /v1/deployments/{id}/target` returns the target the deployment
//!   should converge to (its pin, else its channel's release).
//! - `GET /v1/deployments/{id}/bundle-sources` says where the release's chart
//!   and Operator image come from.
//! - `POST /v1/deployments/{id}/bundle-signature` signs a bundle's manifest
//!   with the manager's bundle signing key, which the site verifies.
//! - `POST /v1/deployments/{id}/status-report` accepts the state and the
//!   buffered telemetry the site carried back, reconciled the same way a sync
//!   is, with telemetry passed to the telemetry backend.

use std::{future::ready, sync::Arc, time::Duration};

use alien_bindings::traits::Kv;
use alien_core::{sync::TargetDeployment, DeploymentState, Platform};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine as _;
use futures::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use super::{auth, AppState};
use crate::{
    auth::{Scope, Subject},
    error::ErrorData,
    traits::{
        DeploymentRecord, ReconcileData, ReleaseRecord, TelemetryBackend, TelemetryCaller,
        TelemetrySignal,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/deployments/{id}/target", get(target))
        .route("/v1/deployments/{id}/bundle-sources", get(bundle_sources))
        .route(
            "/v1/deployments/{id}/bundle-signature",
            post(bundle_signature),
        )
        .route("/v1/deployments/{id}/status-report", post(status_report))
}

/// The deployment, if the caller may run the air-gapped exchange for it.
async fn airgapped_deployment(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
) -> Result<(Subject, DeploymentRecord), Response> {
    let subject = auth::require_auth(state, headers)
        .await
        .map_err(IntoResponse::into_response)?;
    let deployment = match state.deployment_store.get_deployment(&subject, id).await {
        Ok(Some(deployment)) => deployment,
        Ok(None) => {
            return Err(alien_error::AlienError::new(ErrorData::DeploymentNotFound {
                deployment_id: id.to_string(),
            })
            .into_response())
        }
        Err(e) => return Err(e.into_response()),
    };
    // Only the site's own deployment token runs the exchange: it is what
    // `alien onboard --airgapped` hands the site, and anything broader
    // (a deployment group's token, say) could report state and logs for a
    // connected deployment.
    let own_token = matches!(
        &subject.scope,
        Scope::Deployment { deployment_id, .. } if deployment_id == &deployment.id
    );
    if !own_token || !state.authz.can_update_deployment(&subject, &deployment) {
        return Err(ErrorData::forbidden(
            "Air-gapped sync takes the deployment's own token (the one `alien onboard --airgapped` printed)",
        )
        .into_response());
    }
    if deployment.platform != Platform::Kubernetes {
        return Err(
            ErrorData::bad_request("Air-gapped sync is for Kubernetes deployments").into_response(),
        );
    }
    Ok((subject, deployment))
}

async fn release(
    state: &AppState,
    subject: &Subject,
    deployment: &DeploymentRecord,
    release_id: Option<&str>,
) -> Result<ReleaseRecord, Response> {
    let release = match release_id {
        Some(release_id) => state.release_store.get_release(subject, release_id).await,
        None => super::channels::release_for_deployment(state, subject, Some(deployment)).await,
    };
    match release {
        Ok(Some(release)) => Ok(release),
        Ok(None) => Err(ErrorData::bad_request("There is no release to sync yet").into_response()),
        Err(e) => Err(e.into_response()),
    }
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[serde(rename_all = "camelCase")]
pub struct TargetQuery {
    /// Release to target; defaults to the deployment's pin or channel.
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
    let (subject, deployment) = match airgapped_deployment(&state, &headers, &id).await {
        Ok(found) => found,
        Err(response) => return response,
    };
    let release = match release(&state, &subject, &deployment, query.release_id.as_deref()).await {
        Ok(release) => release,
        Err(response) => return response,
    };
    // No token: the environment pulls from its own registry and never calls
    // the manager. Bundles are applied by Operators that support air-gapped
    // mode, which all understand tunnels.
    let mut target: TargetDeployment =
        match super::sync::build_pull_target(&state, &deployment, release, None, true).await {
            Ok(target) => target,
            Err(response) => return response,
        };
    // Workloads can't reach this manager to export telemetry; the Operator
    // buffers it for the next sync instead.
    target.config.monitoring = None;
    Json(target).into_response()
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[serde(rename_all = "camelCase")]
pub struct BundleSourcesQuery {
    /// Release whose chart to use.
    pub release_id: String,
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployments/{id}/bundle-sources",
    operation_id = "get_deployment_bundle_sources",
    tag = "deployments",
    params(("id" = String, Path, description = "Deployment ID"), BundleSourcesQuery),
    responses((status = 200, description = "Where the chart and Operator image come from", body = crate::traits::BundleSources)),
    security(("bearer" = []))
))]
async fn bundle_sources(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<BundleSourcesQuery>,
) -> Response {
    let (subject, deployment) = match airgapped_deployment(&state, &headers, &id).await {
        Ok(found) => found,
        Err(response) => return response,
    };
    let release = match release(&state, &subject, &deployment, Some(&query.release_id)).await {
        Ok(release) => release,
        Err(response) => return response,
    };
    if let Some(resolver) = &state.bundle_sources {
        return match resolver.resolve(&subject, &deployment, &release).await {
            Ok(sources) => Json(sources).into_response(),
            Err(e) => e.into_response(),
        };
    }
    match super::charts::served_bundle_sources(&state, &subject, &release).await {
        Ok(Some(sources)) => Json(sources).into_response(),
        Ok(None) => {
            ErrorData::bad_request("This manager doesn't serve air-gapped bundles").into_response()
        }
        Err(response) => response,
    }
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
    let (_, deployment) = match airgapped_deployment(&state, &headers, &id).await {
        Ok(found) => found,
        Err(response) => return response,
    };
    let Some(key) = state.bundle_signing_key.clone() else {
        return ErrorData::bad_request("This manager doesn't sign air-gapped bundles")
            .into_response();
    };
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
    #[cfg_attr(feature = "openapi", schema(value_type = std::collections::HashMap<String, serde_json::Value>))]
    pub state: DeploymentState,
    /// Telemetry the Operator buffered, as the OTLP batches it received.
    #[serde(default)]
    pub telemetry: Vec<ReportedTelemetry>,
}

/// One OTLP batch from the environment.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReportedTelemetry {
    /// The site's sequence number for the batch. The manager keeps the
    /// highest one it has passed on and skips any it already has, so a
    /// report sent again doesn't duplicate telemetry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    /// `logs`, `metrics` or `traces`.
    pub signal: String,
    /// OTLP protobuf, base64.
    pub data: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatusReportResponse {
    pub status: String,
    /// Telemetry batches passed to the telemetry backend.
    pub telemetry_accepted: usize,
    /// Highest batch the manager has passed on for this deployment, across
    /// reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry_through: Option<i64>,
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
    let (subject, deployment) = match airgapped_deployment(&state, &headers, &id).await {
        Ok(found) => found,
        Err(response) => return response,
    };

    // Decode everything before recording anything, so a bad batch rejects
    // the whole report and the site can resend it unchanged.
    let mut batches = Vec::with_capacity(report.telemetry.len());
    for batch in &report.telemetry {
        let signal = match batch.signal.as_str() {
            "logs" => TelemetrySignal::Logs,
            "metrics" => TelemetrySignal::Metrics,
            "traces" => TelemetrySignal::Traces,
            other => {
                return ErrorData::bad_request(format!("Unknown telemetry signal '{other}'"))
                    .into_response()
            }
        };
        // The same rule live telemetry from this deployment follows.
        if !state.authz.can_ingest_telemetry(&subject, signal) {
            return ErrorData::forbidden(format!(
                "This token can't send {} for the deployment",
                batch.signal
            ))
            .into_response();
        }
        let Ok(data) = base64::engine::general_purpose::STANDARD.decode(&batch.data) else {
            return ErrorData::bad_request("Telemetry data must be base64").into_response();
        };
        batches.push((batch.id, signal, data));
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

    // The same attribution live telemetry from this deployment gets.
    let caller = TelemetryCaller {
        deployment_id: Some(deployment.id.clone()),
        project_id: Some(deployment.project_id.clone()),
        workspace_id: Some(deployment.workspace_id.clone()),
        gateway_log_source: None,
    };
    // In its own task, so a client that disconnects (a load balancer timing
    // the request out, say) can't cancel it after the backend took a batch
    // but before the mark covers it, which would duplicate that batch on
    // the next report. The budget bounds how long the task outlives the
    // request.
    let forwarding = tokio::spawn(forward_telemetry(
        state.telemetry_backend.clone(),
        state.kv.clone(),
        caller,
        telemetry_mark_key(&deployment.id),
        batches,
        TELEMETRY_BUDGET,
    ));
    let forwarded = match forwarding.await {
        Ok(Ok(forwarded)) => forwarded,
        Ok(Err(response)) => return response,
        Err(e) => return ErrorData::internal(format!("forwarding telemetry: {e}")).into_response(),
    };

    (
        StatusCode::OK,
        Json(StatusReportResponse {
            status,
            telemetry_accepted: forwarded.accepted,
            telemetry_through: forwarded.through,
        }),
    )
        .into_response()
}

/// How long a status report spends forwarding telemetry before it answers.
/// It stays under the 60 second idle timeout load balancers commonly put in
/// front of the manager: a site with a backlog gets partial progress in
/// `telemetryThrough` and sends the rest in its next request, instead of a
/// gateway timeout that makes no progress.
const TELEMETRY_BUDGET: Duration = Duration::from_secs(45);

/// Batches written to the telemetry backend at once. Backends that
/// acknowledge a write only once it is flushed take seconds per batch.
const TELEMETRY_CONCURRENCY: usize = 8;

/// One decoded batch from a report: its id, signal and OTLP bytes.
type DecodedBatch = (Option<i64>, TelemetrySignal, Vec<u8>);

/// What forwarding a report's telemetry got through.
#[derive(Debug)]
struct Forwarded {
    /// Batches the telemetry backend accepted.
    accepted: usize,
    /// The mark after forwarding: every batch up to it has been received.
    through: Option<i64>,
}

/// Write a report's telemetry to the backend, skipping batches below the
/// deployment's mark and advancing the mark as batches go through.
///
/// A single mark is enough because a site exports everything its Operator
/// holds after the last acknowledgement, and the Operator frees only what
/// an acknowledgement covers (at most this mark). Any later report therefore
/// contains every batch an earlier one had that the manager hasn't
/// confirmed, so a batch below the mark has been received.
///
/// Batches are written [`TELEMETRY_CONCURRENCY`] at a time, but the mark
/// only moves over the batches that completed in order, so it never covers
/// a batch that didn't go through. No new batch starts once `budget` has
/// passed; the ones in flight finish and the rest wait for the next report.
/// A failure keeps what went through before it; batches after it that
/// completed are sent again with the next report.
async fn forward_telemetry(
    backend: Arc<dyn TelemetryBackend>,
    kv: Arc<dyn Kv>,
    caller: TelemetryCaller,
    mark_key: String,
    mut batches: Vec<DecodedBatch>,
    budget: Duration,
) -> Result<Forwarded, Response> {
    let deadline = Instant::now() + budget;
    let mut through = read_telemetry_mark(kv.as_ref(), &mark_key)
        .await
        .map_err(IntoResponse::into_response)?;
    let mark = through;
    batches.retain(|(id, _, _)| !id.is_some_and(|id| mark.is_some_and(|mark| id <= mark)));
    batches.sort_by_key(|(id, _, _)| *id);

    let backend = backend.as_ref();
    let caller = &caller;
    let mut written = stream::iter(batches)
        // A batch without an id (from an older site) can't be picked up by
        // the next report, so the budget never holds one back. They sort
        // first.
        .take_while(|(id, _, _)| ready(id.is_none() || Instant::now() < deadline))
        .map(|(id, signal, data)| async move {
            backend
                .ingest(signal, caller, data.into())
                .await
                .map(|()| id)
        })
        .buffered(TELEMETRY_CONCURRENCY);

    let mut accepted = 0;
    while let Some(written_batch) = written.next().await {
        let id = written_batch.map_err(IntoResponse::into_response)?;
        accepted += 1;
        if let Some(id) = id {
            through = Some(id);
            kv.put(&mark_key, id.to_string().into_bytes(), None)
                .await
                .map_err(|e| {
                    ErrorData::internal(format!("recording received telemetry: {e}"))
                        .into_response()
                })?;
        }
    }
    Ok(Forwarded { accepted, through })
}

fn telemetry_mark_key(deployment_id: &str) -> String {
    format!("airgap-telemetry-through:{deployment_id}")
}

/// Highest telemetry batch already passed on for a deployment.
async fn read_telemetry_mark(
    kv: &dyn Kv,
    key: &str,
) -> Result<Option<i64>, alien_error::AlienError<ErrorData>> {
    let Some(entry) = kv
        .get(key)
        .await
        .map_err(|e| ErrorData::internal(format!("reading received telemetry: {e}")))?
    else {
        return Ok(None);
    };
    String::from_utf8_lossy(&entry.value)
        .trim()
        .parse()
        .map(Some)
        .map_err(|_| {
            ErrorData::internal(format!(
                "the received-telemetry mark at {key} is not a number"
            ))
        })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use alien_bindings::providers::kv::local::LocalKv;
    use alien_error::AlienError;
    use async_trait::async_trait;

    use super::*;

    /// A backend that, like one acknowledging a write only once it is
    /// flushed, takes a while per batch. Records the id each batch carries
    /// in its payload.
    struct SlowBackend {
        delay: Duration,
        received: Mutex<Vec<i64>>,
    }

    #[async_trait]
    impl TelemetryBackend for SlowBackend {
        async fn ingest(
            &self,
            _signal: TelemetrySignal,
            _caller: &TelemetryCaller,
            data: bytes::Bytes,
        ) -> Result<(), AlienError> {
            tokio::time::sleep(self.delay).await;
            let id = std::str::from_utf8(&data).unwrap().parse().unwrap();
            self.received.lock().unwrap().push(id);
            Ok(())
        }
    }

    fn batches(ids: impl IntoIterator<Item = i64>) -> Vec<DecodedBatch> {
        ids.into_iter()
            .map(|id| (Some(id), TelemetrySignal::Logs, id.to_string().into_bytes()))
            .collect()
    }

    fn caller() -> TelemetryCaller {
        TelemetryCaller {
            deployment_id: Some("dep".to_string()),
            project_id: None,
            workspace_id: None,
            gateway_log_source: None,
        }
    }

    #[tokio::test]
    async fn a_backlog_is_forwarded_across_reports_within_the_budget_without_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
        let backend = Arc::new(SlowBackend {
            delay: Duration::from_millis(200),
            received: Mutex::new(Vec::new()),
        });
        let budget = Duration::from_millis(500);
        let backlog = 1..=100;

        // Serially, 200ms per batch would fit 2-3 batches in the budget.
        let started = Instant::now();
        let first = forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".to_string(),
            batches(backlog.clone()),
            budget,
        )
        .await
        .unwrap();
        let took = started.elapsed();

        assert!(
            took < budget + Duration::from_millis(400),
            "answers within the budget plus one batch in flight, took {took:?}"
        );
        assert!(
            first.accepted >= 2 * TELEMETRY_CONCURRENCY && first.accepted < 100,
            "batches are written concurrently but the backlog doesn't fit: {first:?}"
        );
        let through = first.through.expect("progress was made");
        assert_eq!(
            through, first.accepted as i64,
            "the mark covers exactly the batches that went through"
        );
        assert_eq!(
            read_telemetry_mark(kv.as_ref(), "mark").await.unwrap(),
            Some(through)
        );

        // The site sends its whole backlog again, as it does until the
        // acknowledgement reaches it; each report picks up after the mark.
        let mut last = first;
        while last.through != Some(100) {
            let next = forward_telemetry(
                backend.clone(),
                kv.clone(),
                caller(),
                "mark".to_string(),
                batches(backlog.clone()),
                budget,
            )
            .await
            .unwrap();
            assert!(next.through > last.through, "every report makes progress");
            last = next;
        }

        let mut received = backend.received.lock().unwrap().clone();
        received.sort();
        assert_eq!(
            received,
            backlog.collect::<Vec<_>>(),
            "every batch reached the backend exactly once"
        );
    }
}
