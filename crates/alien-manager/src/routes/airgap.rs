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
use alien_core::{
    sync::TargetDeployment, AirgappedReportMetadata, DeploymentState, Platform, ResourceHeartbeat,
};
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
use sha2::{Digest, Sha256};
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
            .into_response());
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
    if !deployment
        .stack_settings
        .as_ref()
        .is_some_and(|settings| settings.airgapped)
    {
        return Err(ErrorData::bad_request(
            "This deployment is not configured for air-gapped sync",
        )
        .into_response());
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
    if deployment.status == "retained" {
        return Json(serde_json::json!({"action": "retained"})).into_response();
    }
    let deleting = matches!(
        deployment.status.as_str(),
        "delete-pending" | "deleting" | "delete-failed" | "teardown-required" | "teardown-failed"
    );
    let release_id = if deleting {
        deployment.current_release_id.as_deref()
    } else {
        query.release_id.as_deref()
    };
    let release = match release(&state, &subject, &deployment, release_id).await {
        Ok(release) => release,
        Err(response) => return response,
    };
    if deleting {
        let Some(stack) = release.stacks.get(&deployment.platform) else {
            return ErrorData::bad_request("The installed release has no stack for this platform")
                .into_response();
        };
        return Json(serde_json::json!({
            "action": "delete",
            "deletionPolicy": deployment.runtime_metadata.as_ref().map(|m| m.deletion_policy).unwrap_or_default(),
            "releaseInfo": {"releaseId": release.id, "stack": {"id": stack.id()}},
        }))
        .into_response();
    }
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
    let (subject, deployment) = match airgapped_deployment(&state, &headers, &id).await {
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
    let value: serde_json::Value = match serde_json::from_slice(&manifest) {
        Ok(value) => value,
        Err(_) => return ErrorData::bad_request("manifest must be JSON").into_response(),
    };
    if value["releaseId"] == "delete" {
        let policy: alien_core::DeletionPolicy = match serde_json::from_value(
            value
                .get("deletionPolicy")
                .cloned()
                .unwrap_or_else(|| "retain".into()),
        ) {
            Ok(policy) => policy,
            Err(_) => return ErrorData::bad_request("invalid deletion policy").into_response(),
        };
        let accepted = deployment
            .runtime_metadata
            .as_ref()
            .map(|m| m.deletion_policy)
            .unwrap_or_default();
        if !matches!(
            deployment.status.as_str(),
            "delete-pending"
                | "deleting"
                | "delete-failed"
                | "teardown-required"
                | "teardown-failed"
        ) || policy != accepted
        {
            return ErrorData::bad_request(
                "The deletion manifest must match the accepted deletion request",
            )
            .into_response();
        }
    }
    sign_bundle_manifest(
        state.deployment_store.as_ref(),
        &subject,
        &deployment.id,
        &key,
        &manifest,
    )
    .await
}

async fn sign_bundle_manifest(
    store: &dyn crate::traits::DeploymentStore,
    subject: &Subject,
    deployment_id: &str,
    key: &alien_core::bundle_signature::BundleSigningKey,
    manifest: &[u8],
) -> Response {
    // Sign only manifests for this deployment, so a signature can't be
    // reused to install another deployment's bundle.
    let manifest_value = serde_json::from_slice::<serde_json::Value>(manifest).ok();
    let manifest_deployment = manifest_value
        .as_ref()
        .and_then(|value| value.get("deploymentId")?.as_str());
    if manifest_deployment != Some(deployment_id) {
        return ErrorData::bad_request("The manifest is not a bundle manifest for this deployment")
            .into_response();
    }
    let signature = key.sign(manifest);
    if let Some(release_id) = manifest_value
        .as_ref()
        .and_then(|value| value.get("releaseId")?.as_str())
        .filter(|id| !id.trim().is_empty() && *id != "delete")
    {
        if let Err(error) = store
            .record_airgapped_bundle_signed(subject, deployment_id, release_id)
            .await
        {
            return error.into_response();
        }
    }
    Json(BundleSignatureResponse {
        signature,
        public_key: key.public_key(),
    })
    .into_response()
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    /// Site clock time when the report was written. Older clients omit it.
    #[serde(default)]
    pub written_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Deployment state as the environment's Operator last recorded it.
    #[cfg_attr(feature = "openapi", schema(value_type = std::collections::HashMap<String, serde_json::Value>))]
    pub state: DeploymentState,
    /// Resource observations captured by the Operator alongside this state.
    #[serde(default)]
    pub heartbeats: Vec<ResourceHeartbeat>,
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
    /// Permanently rejected batches recorded separately for diagnosis, not ingested.
    #[serde(default)]
    pub telemetry_rejected: usize,
    /// Highest batch durably ingested or retained as permanently rejected,
    /// across reports.
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
    Json(mut report): Json<StatusReport>,
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
                    .into_response();
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

    report
        .state
        .runtime_metadata
        .get_or_insert_with(Default::default)
        .airgapped_report = Some(AirgappedReportMetadata {
        written_at: report.written_at,
        received_at: chrono::Utc::now(),
    });

    let status = serde_json::to_value(&report.state.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    let deleting = matches!(
        report.state.status,
        alien_core::DeploymentStatus::Deleted | alien_core::DeploymentStatus::Retained
    );
    if deleting && report.telemetry.iter().any(|batch| batch.id.is_none()) {
        return ErrorData::bad_request("Final deletion reports require telemetry batch IDs")
            .into_response();
    }
    let last_batch = report.telemetry.iter().filter_map(|batch| batch.id).max();
    let report_state = report.state.clone();
    if !deleting {
        if let Err(error) = reconcile_report(
            state.deployment_store.as_ref(),
            &subject,
            &deployment.id,
            report.state,
            report.heartbeats.clone(),
        )
        .await
        {
            return error.into_response();
        }
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

    // Retained data is a completed checkpoint. A repeated report can finish
    // acknowledging telemetry without replacing that checkpoint.
    let retained_replay = deployment.status == "retained"
        && report_state.status == alien_core::DeploymentStatus::Retained;
    if deleting
        && !retained_replay
        && last_batch.is_none_or(|last| forwarded.through.is_some_and(|mark| mark >= last))
    {
        if let Err(error) = reconcile_report(
            state.deployment_store.as_ref(),
            &subject,
            &deployment.id,
            report_state,
            report.heartbeats,
        )
        .await
        {
            return error.into_response();
        }
    }

    (
        StatusCode::OK,
        Json(StatusReportResponse {
            status,
            telemetry_accepted: forwarded.accepted,
            telemetry_rejected: forwarded.rejected,
            telemetry_through: forwarded.through,
        }),
    )
        .into_response()
}

async fn reconcile_report(
    store: &dyn crate::traits::DeploymentStore,
    subject: &Subject,
    deployment_id: &str,
    report: DeploymentState,
    heartbeats: Vec<ResourceHeartbeat>,
) -> Result<(), alien_error::AlienError> {
    store
        .reconcile(
            subject,
            ReconcileData {
                deployment_id: deployment_id.to_string(),
                session: "airgap-status-report".to_string(),
                state: report,
                update_heartbeat: true,
                suggested_delay_ms: None,
                heartbeats,
                observed_inventory_batches: Vec::new(),
                capabilities: Vec::new(),
                operator_version: None,
                execution_claim: None,
                operations_report: None,
            },
        )
        .await
        .map(|_| ())
}

/// How long a status report spends forwarding telemetry before it answers.
/// It stays under the 60 second idle timeout load balancers commonly put in
/// front of the manager: a site with a backlog gets partial progress in
/// `telemetryThrough` and sends the rest in its next request, instead of a
/// gateway timeout that makes no progress.
const TELEMETRY_BUDGET: Duration = Duration::from_secs(45);

/// Signal writes to the telemetry backend at once. Backends that
/// acknowledge a write only once it is flushed take seconds per batch.
const TELEMETRY_CONCURRENCY: usize = 8;
const TELEMETRY_GROUP_BATCHES: usize = 128;
const TELEMETRY_GROUP_BYTES: usize = 1024 * 1024;

/// One decoded batch from a report: its id, signal and OTLP bytes.
type DecodedBatch = (Option<i64>, TelemetrySignal, Vec<u8>);

/// An ordered window acknowledged only after all its signals are durable.
struct TelemetryGroup {
    through: Option<i64>,
    count: usize,
    bytes: usize,
    signals: Vec<(TelemetrySignal, Vec<u8>)>,
}

fn group_telemetry(batches: Vec<DecodedBatch>, max_batches: usize) -> Vec<TelemetryGroup> {
    let mut groups: Vec<TelemetryGroup> = Vec::new();
    for (id, signal, data) in batches {
        if groups.last().is_none_or(|group| {
            group.count == max_batches
                || id.is_none()
                || group.bytes + data.len() > TELEMETRY_GROUP_BYTES
                || group.through.is_some() != id.is_some()
        }) {
            groups.push(TelemetryGroup {
                through: id,
                count: 0,
                bytes: 0,
                signals: Vec::new(),
            });
        }
        let group = groups.last_mut().expect("a group was created");
        group.through = id;
        group.count += 1;
        group.bytes += data.len();
        // OTLP export requests contain repeated resource messages. Protobuf
        // concatenation appends those messages without changing their contents.
        // Keep signals separate, including when source batches interleave them.
        if let Some((_, combined)) = group.signals.iter_mut().find(|(kind, _)| *kind == signal) {
            combined.extend(data);
        } else {
            group.signals.push((signal, data));
        }
    }
    groups
}

/// What forwarding a report's telemetry got through.
#[derive(Debug)]
struct Forwarded {
    /// Batches the telemetry backend accepted.
    accepted: usize,
    /// Batches recorded as permanently rejected by the backend.
    rejected: usize,
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
/// Signal writes run [`TELEMETRY_CONCURRENCY`] at a time. The mark
/// only moves over groups completed in order: every covered batch was ingested
/// or has a durable record of permanent rejection. No new signal starts once `budget` has
/// passed; writes in flight finish and the rest wait for the next report.
/// Partially written groups and any groups after a failure are replayed.
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
    let mut rejected = 0;
    // A failed request or lost response may have hidden an earlier warning.
    // Repeat it until the client stops submitting that acknowledged batch ID.
    for (id, _, _) in &batches {
        if let Some(id) = id.filter(|id| mark.is_some_and(|mark| *id <= mark)) {
            if kv
                .exists(&format!("{mark_key}:rejected:{id}"))
                .await
                .map_err(|error| {
                    ErrorData::internal(format!("reading telemetry rejection: {error}"))
                        .into_response()
                })?
            {
                rejected += 1;
            }
        }
    }
    batches.retain(|(id, _, _)| !id.is_some_and(|id| mark.is_some_and(|mark| id <= mark)));
    batches.sort_by_key(|(id, _, _)| *id);

    // A rejected group is retried one batch at a time. Persist the upper bound
    // so the next request can resume within the normal time budget.
    let split_key = format!("{mark_key}:individual-through");
    let split_through = read_telemetry_mark(kv.as_ref(), &split_key)
        .await
        .map_err(IntoResponse::into_response)?;
    let individual = split_through.is_some_and(|end| through.is_none_or(|mark| mark < end));
    let max_batches = if individual {
        1
    } else {
        TELEMETRY_GROUP_BATCHES
    };
    let last_id = batches.iter().filter_map(|(id, _, _)| *id).max();
    let signals = group_telemetry(batches.clone(), max_batches)
        .into_iter()
        .flat_map(|group| {
            let signals = group.signals.len();
            group
                .signals
                .into_iter()
                .enumerate()
                .map(move |(index, (signal, data))| {
                    (
                        group.through,
                        signal,
                        data,
                        group.count,
                        (index + 1 == signals).then_some(group.count),
                    )
                })
        });
    let mut written = stream::iter(signals)
        .take_while(|(id, _, _, _, _)| ready(id.is_none() || Instant::now() < deadline))
        .map(|(id, signal, data, count, completed_count)| {
            let backend = backend.as_ref();
            let kv = kv.as_ref();
            let caller = &caller;
            let mark_key = &mark_key;
            async move {
                let result = match backend.ingest(signal, caller, data.clone().into()).await {
                    Ok(()) => Ok(0),
                    Err(error) if count == 1 && permanent_payload_rejection(&error) => {
                        retain_rejected_batch(kv, mark_key, id, signal, &data, &error)
                            .await
                            .map(|()| 1)
                    }
                    Err(error) => Err(error),
                };
                (id, completed_count, result)
            }
        })
        .buffered(TELEMETRY_CONCURRENCY);

    let mut accepted = 0;
    while let Some((id, completed_count, result)) = written.next().await {
        let rejected_count = match result {
            Ok(count) => count,
            Err(error)
                if !individual && permanent_payload_rejection(&error) && last_id.is_some() =>
            {
                drop(written);
                let Some(end) = last_id else {
                    return Err(error.into_response());
                };
                kv.put(&split_key, end.to_string().into_bytes(), None)
                    .await
                    .map_err(|error| {
                        ErrorData::internal(format!(
                            "recording individual telemetry retry: {error}"
                        ))
                        .into_response()
                    })?;
                let remaining = deadline.saturating_duration_since(Instant::now());
                // Completed batches and their warnings are already counted above.
                batches
                    .retain(|(id, _, _)| id.is_some_and(|id| through.is_none_or(|mark| id > mark)));
                let rest = Box::pin(forward_telemetry(
                    backend, kv, caller, mark_key, batches, remaining,
                ))
                .await?;
                return Ok(Forwarded {
                    accepted: accepted + rest.accepted,
                    rejected: rejected + rest.rejected,
                    through: rest.through,
                });
            }
            Err(error) => return Err(error.into_response()),
        };
        let Some(count) = completed_count else {
            continue;
        };
        accepted += count - rejected_count;
        rejected += rejected_count;
        if let Some(id) = id {
            kv.put(&mark_key, id.to_string().into_bytes(), None)
                .await
                .map_err(|error| {
                    ErrorData::internal(format!("recording received telemetry: {error}"))
                        .into_response()
                })?;
            through = Some(id);
        }
    }
    Ok(Forwarded {
        accepted,
        rejected,
        through,
    })
}

fn permanent_payload_rejection(error: &alien_error::AlienError) -> bool {
    matches!(error.http_status_code, Some(400 | 413 | 422))
}

/// Record the explicit rejection before acknowledging it. Keep only bounded
/// metadata: telemetry payloads can exceed the KV store's value limit.
async fn retain_rejected_batch(
    kv: &dyn Kv,
    mark_key: &str,
    id: Option<i64>,
    signal: TelemetrySignal,
    data: &[u8],
    error: &alien_error::AlienError,
) -> Result<(), alien_error::AlienError> {
    let signal = match signal {
        TelemetrySignal::Logs => "logs",
        TelemetrySignal::Metrics => "metrics",
        TelemetrySignal::Traces => "traces",
    };
    let identity = id
        .map(|id| id.to_string())
        .unwrap_or_else(|| format!("{signal}-{:x}", Sha256::digest(data)));
    let record = serde_json::json!({
        "id": id, "signal": signal, "httpStatus": error.http_status_code,
        "bytes": data.len(), "sha256": format!("{:x}", Sha256::digest(data)),
    });
    kv.put(
        &format!("{mark_key}:rejected:{identity}"),
        record.to_string().into_bytes(),
        None,
    )
    .await
    .map_err(|error| error.into_generic())?;
    tracing::warn!(batch_id = ?id, signal, status = ?error.http_status_code, "Telemetry batch permanently rejected; rejection details recorded");
    Ok(())
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

    use alien_bindings::{
        providers::kv::local::LocalKv,
        traits::{Binding, KvEntry, PutOptions, ScanResult},
    };
    use alien_error::AlienError;
    use async_trait::async_trait;
    use opentelemetry_proto::tonic::{
        collector::{
            logs::v1::ExportLogsServiceRequest, metrics::v1::ExportMetricsServiceRequest,
            trace::v1::ExportTraceServiceRequest,
        },
        common::v1::{any_value::Value, AnyValue, KeyValue},
        logs::v1::ResourceLogs,
        resource::v1::Resource,
    };
    use prost::Message;

    use super::*;

    #[derive(Debug)]
    struct RejectRecordWrites(Arc<dyn Kv>);
    impl Binding for RejectRecordWrites {}
    #[async_trait]
    impl Kv for RejectRecordWrites {
        async fn get(&self, key: &str) -> alien_bindings::error::Result<Option<KvEntry>> {
            self.0.get(key).await
        }
        async fn put(
            &self,
            key: &str,
            value: Vec<u8>,
            options: Option<PutOptions>,
        ) -> alien_bindings::error::Result<bool> {
            if key.contains(":rejected:") {
                return Err(AlienError::new(
                    alien_bindings::error::ErrorData::KvOperationFailed {
                        operation: "put".into(),
                        key: key.into(),
                        reason: "rejection storage unavailable".into(),
                    },
                ));
            }
            self.0.put(key, value, options).await
        }
        async fn delete(
            &self,
            key: &str,
            version: Option<&str>,
        ) -> alien_bindings::error::Result<bool> {
            self.0.delete(key, version).await
        }
        async fn exists(&self, key: &str) -> alien_bindings::error::Result<bool> {
            self.0.exists(key).await
        }
        async fn scan_prefix(
            &self,
            prefix: &str,
            limit: Option<usize>,
            cursor: Option<String>,
        ) -> alien_bindings::error::Result<ScanResult> {
            self.0.scan_prefix(prefix, limit, cursor).await
        }
    }

    #[tokio::test]
    async fn rejection_record_failure_keeps_the_batch_unacknowledged() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(RejectRecordWrites(Arc::new(
            LocalKv::new(tmp.path().join("kv")).await.unwrap(),
        )));
        let backend = Arc::new(RejectMalformed {
            status: 400,
            delay: Duration::ZERO,
            received: Mutex::new(Vec::new()),
        });
        let result = forward_telemetry(
            backend,
            kv.clone(),
            caller(),
            "mark".into(),
            vec![(Some(1), TelemetrySignal::Logs, b"{}".to_vec())],
            TELEMETRY_BUDGET,
        )
        .await;
        assert_eq!(result.unwrap_err().status(), StatusCode::BAD_GATEWAY);
        assert!(kv.get("mark").await.unwrap().is_none());
        assert!(kv.get("mark:rejected:1").await.unwrap().is_none());
    }

    struct RejectMalformed {
        status: u16,
        delay: Duration,
        received: Mutex<Vec<i64>>,
    }

    #[async_trait]
    impl TelemetryBackend for RejectMalformed {
        async fn ingest(
            &self,
            _: TelemetrySignal,
            _: &TelemetryCaller,
            data: bytes::Bytes,
        ) -> Result<(), AlienError> {
            if data.starts_with(b"{") {
                let mut error = ErrorData::bad_request("invalid telemetry").into_generic();
                error.http_status_code = Some(self.status);
                return Err(error);
            }
            tokio::time::sleep(self.delay).await;
            self.received.lock().unwrap().extend(batch_ids(&data));
            Ok(())
        }
    }

    #[tokio::test]
    async fn permanent_rejection_records_metadata_and_unblocks_later_batches() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
        let backend = Arc::new(RejectMalformed {
            status: 400,
            delay: Duration::ZERO,
            received: Mutex::new(Vec::new()),
        });
        let invalid = vec![b'{'; 100_000]; // Larger than the KV value limit.
        let mut input = vec![(Some(1), TelemetrySignal::Logs, invalid.clone())];
        input.extend(batches(2..=200));
        let result = forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".into(),
            input.clone(),
            TELEMETRY_BUDGET,
        )
        .await
        .unwrap();
        assert_eq!(
            (result.accepted, result.rejected, result.through),
            (199, 1, Some(200))
        );
        let record = kv.get("mark:rejected:1").await.unwrap().unwrap();
        let record: serde_json::Value = serde_json::from_slice(&record.value).unwrap();
        assert_eq!(record["bytes"], 100_000);
        assert_eq!(record["httpStatus"], 400);
        assert_eq!(record["sha256"], format!("{:x}", Sha256::digest(&invalid)));
        assert!(record.get("data").is_none());
        let received: std::collections::BTreeSet<_> =
            backend.received.lock().unwrap().iter().copied().collect();
        assert_eq!(received, (2..=200).collect());
        let replay = forward_telemetry(
            backend,
            kv,
            caller(),
            "mark".into(),
            input,
            TELEMETRY_BUDGET,
        )
        .await
        .unwrap();
        assert_eq!(
            (replay.accepted, replay.rejected, replay.through),
            (0, 1, Some(200))
        );
    }

    struct RejectThenUnavailable(std::sync::atomic::AtomicBool);

    #[async_trait]
    impl TelemetryBackend for RejectThenUnavailable {
        async fn ingest(
            &self,
            _: TelemetrySignal,
            _: &TelemetryCaller,
            data: bytes::Bytes,
        ) -> Result<(), AlienError> {
            if data.starts_with(b"{") {
                let mut error = ErrorData::bad_request("invalid telemetry").into_generic();
                error.http_status_code = Some(400);
                return Err(error);
            }
            if self.0.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let mut error = ErrorData::bad_request("temporarily unavailable").into_generic();
                error.http_status_code = Some(503);
                return Err(error);
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn rejection_warning_survives_a_later_failure_and_lost_response() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
        kv.put("mark:individual-through", b"2".to_vec(), None)
            .await
            .unwrap();
        let backend = Arc::new(RejectThenUnavailable(std::sync::atomic::AtomicBool::new(
            true,
        )));
        let input = vec![
            (Some(1), TelemetrySignal::Logs, b"{}".to_vec()),
            (Some(2), TelemetrySignal::Logs, batch_data(2)),
        ];
        let error = forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".into(),
            input.clone(),
            TELEMETRY_BUDGET,
        )
        .await
        .unwrap_err();
        assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            read_telemetry_mark(kv.as_ref(), "mark").await.unwrap(),
            Some(1)
        );
        for accepted in [1, 0] {
            let result = forward_telemetry(
                backend.clone(),
                kv.clone(),
                caller(),
                "mark".into(),
                input.clone(),
                TELEMETRY_BUDGET,
            )
            .await
            .unwrap();
            assert_eq!(
                (result.accepted, result.rejected, result.through),
                (accepted, 1, Some(2))
            );
        }
        let result = forward_telemetry(
            backend,
            kv,
            caller(),
            "mark".into(),
            batches(3..=3),
            TELEMETRY_BUDGET,
        )
        .await
        .unwrap();
        assert_eq!(
            (result.accepted, result.rejected, result.through),
            (1, 0, Some(3))
        );
    }

    #[tokio::test]
    async fn authentication_throttling_and_server_failures_do_not_acknowledge_data() {
        for status in [401, 403, 429, 500, 502, 503, 504] {
            let tmp = tempfile::tempdir().unwrap();
            let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
            let backend = Arc::new(RejectMalformed {
                status,
                delay: Duration::ZERO,
                received: Mutex::new(Vec::new()),
            });
            let input = vec![(Some(1), TelemetrySignal::Logs, b"{}".to_vec())];
            let error = forward_telemetry(
                backend,
                kv.clone(),
                caller(),
                "mark".into(),
                input,
                TELEMETRY_BUDGET,
            )
            .await
            .unwrap_err();
            assert_eq!(error.status().as_u16(), status);
            assert!(kv.get("mark").await.unwrap().is_none());
            assert!(kv.get("mark:rejected:1").await.unwrap().is_none());
            assert!(kv.get("mark:individual-through").await.unwrap().is_none());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn individual_fallback_resumes_across_budgets_then_restores_grouping() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
        let backend = Arc::new(RejectMalformed {
            status: 400,
            delay: Duration::from_secs(5),
            received: Mutex::new(Vec::new()),
        });
        let mut input = vec![(Some(1), TelemetrySignal::Logs, b"{}".to_vec())];
        input.extend(batches(2..=200));
        let mut through = 0;
        let mut rejected = 0;
        for _ in 0..5 {
            let start = Instant::now();
            let result = forward_telemetry(
                backend.clone(),
                kv.clone(),
                caller(),
                "mark".into(),
                input.clone(),
                TELEMETRY_BUDGET,
            )
            .await
            .unwrap();
            assert!(start.elapsed() < Duration::from_secs(60));
            assert!(result.through.unwrap() > through);
            through = result.through.unwrap();
            // The CLI no longer sends batch IDs after their response was acknowledged.
            input.retain(|(id, _, _)| id.is_none_or(|id| id > through));
            rejected += result.rejected;
            if through == 200 {
                break;
            }
        }
        assert_eq!((through, rejected), (200, 1));
        let start = Instant::now();
        let result = forward_telemetry(
            backend,
            kv,
            caller(),
            "mark".into(),
            batches(201..=1224),
            TELEMETRY_BUDGET,
        )
        .await
        .unwrap();
        assert_eq!((result.accepted, result.through), (1024, Some(1224)));
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn signed_release_records_the_authenticated_download() {
        let key = alien_core::bundle_signature::BundleSigningKey::from_seed([7; 32]);
        let manifest = br#"{"deploymentId":"dep_1","releaseId":"rel_1","chart":"chart.tgz"}"#;
        let mut subject = Subject::system();
        subject.workspace_id = "download-workspace".into();
        let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
        store
            .expect_record_airgapped_bundle_signed()
            .times(1)
            .withf(|caller, deployment, release| {
                caller.workspace_id == "download-workspace"
                    && deployment == "dep_1"
                    && release == "rel_1"
            })
            .returning(|_, _, _| Ok(()));
        let response = sign_bundle_manifest(&store, &subject, "dep_1", &key, manifest).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let signed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        alien_core::bundle_signature::verify(
            &key.public_key(),
            manifest,
            signed["signature"].as_str().unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn signing_failure_is_returned_when_download_recording_fails() {
        let key = alien_core::bundle_signature::BundleSigningKey::from_seed([7; 32]);
        let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
        store
            .expect_record_airgapped_bundle_signed()
            .times(1)
            .returning(|_, _, _| Err(ErrorData::internal("recording failed").into_generic()));
        let response = sign_bundle_manifest(
            &store,
            &Subject::system(),
            "dep_1",
            &key,
            br#"{"deploymentId":"dep_1","releaseId":"rel_1"}"#,
        )
        .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn signing_does_not_record_deletions_or_invalid_manifests() {
        let key = alien_core::bundle_signature::BundleSigningKey::from_seed([7; 32]);
        let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
        store.expect_record_airgapped_bundle_signed().times(0);
        for (manifest, expected) in [
            (
                br#"{"deploymentId":"dep_1","releaseId":"delete","chart":""}"#.as_slice(),
                StatusCode::OK,
            ),
            (
                br#"{"deploymentId":"dep_1","releaseId":""}"#.as_slice(),
                StatusCode::OK,
            ),
            (
                br#"{"deploymentId":"dep_other","releaseId":"rel_1"}"#.as_slice(),
                StatusCode::BAD_REQUEST,
            ),
            (b"invalid json".as_slice(), StatusCode::BAD_REQUEST),
        ] {
            let response =
                sign_bundle_manifest(&store, &Subject::system(), "dep_1", &key, manifest).await;
            assert_eq!(response.status(), expected);
        }
    }

    #[tokio::test]
    async fn report_reconcile_preserves_heartbeats_and_accepts_old_reports() {
        let heartbeat = serde_json::json!({
         "deploymentId": "dep_1", "resourceId": "web", "resourceType": "container",
         "controllerPlatform": "kubernetes", "backend": "kubernetes", "observedAt": "2026-01-01T00:00:00Z",
         "data": {"resourceType": "container", "data": {"backend": "kubernetes", "namespace": "site", "name": "web",
         "workloadKind": "deployment", "status": {"health": "healthy", "lifecycle": "running", "stale": false, "partial": false, "collectionIssues": []},
         "replicas": {"desired": 7, "current": 7, "ready": 7, "available": 7, "updated": 7}, "pods": [], "events": []}}, "raw": []
        });
        for heartbeats in [None, Some(serde_json::json!([heartbeat]))] {
            let mut body = serde_json::json!({"state": {"status": "running", "platform": "kubernetes", "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION}});
            if let Some(heartbeats) = heartbeats {
                body["heartbeats"] = heartbeats;
            }
            let report: StatusReport = serde_json::from_value(body.clone()).unwrap();
            let expected: Vec<ResourceHeartbeat> = serde_json::from_value(
                body.get("heartbeats")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .unwrap();
            let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
            store
                .expect_reconcile()
                .times(1)
                .withf(move |_, data| data.deployment_id == "dep_1" && data.heartbeats == expected)
                .returning(|_, _| Err(ErrorData::internal("store reached").into_generic()));
            let error = reconcile_report(
                &store,
                &Subject::system(),
                "dep_1",
                report.state,
                report.heartbeats,
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains("store reached"));
        }
    }

    /// A backend that, like one acknowledging a write only once it is
    /// flushed, takes a while per batch. Records the id each batch carries
    /// in its payload.
    #[derive(Default)]
    struct SlowBackend {
        delay: Duration,
        received: Mutex<Vec<i64>>,
        active: std::sync::atomic::AtomicUsize,
        max_active: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl TelemetryBackend for SlowBackend {
        async fn ingest(
            &self,
            _signal: TelemetrySignal,
            _caller: &TelemetryCaller,
            data: bytes::Bytes,
        ) -> Result<(), AlienError> {
            let active = self
                .active
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            self.max_active
                .fetch_max(active, std::sync::atomic::Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.active
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            let ids = batch_ids(&data);
            self.received.lock().unwrap().extend(ids);
            Ok(())
        }
    }

    fn batch_data(id: i64) -> Vec<u8> {
        ExportLogsServiceRequest {
            resource_logs: vec![ResourceLogs {
                resource: Some(Resource {
                    attributes: vec![KeyValue {
                        key: "batch.id".into(),
                        value: Some(AnyValue {
                            value: Some(Value::IntValue(id)),
                        }),
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }],
        }
        .encode_to_vec()
    }

    fn batch_ids(data: &[u8]) -> Vec<i64> {
        ExportLogsServiceRequest::decode(data)
            .unwrap()
            .resource_logs
            .into_iter()
            .map(|logs| {
                let value = logs
                    .resource
                    .unwrap()
                    .attributes
                    .remove(0)
                    .value
                    .unwrap()
                    .value
                    .unwrap();
                let Value::IntValue(id) = value else {
                    panic!("batch id must be an integer")
                };
                id
            })
            .collect()
    }

    fn batches(ids: impl IntoIterator<Item = i64>) -> Vec<DecodedBatch> {
        ids.into_iter()
            .map(|id| (Some(id), TelemetrySignal::Logs, batch_data(id)))
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

    #[test]
    fn grouped_otlp_preserves_interleaved_signals_and_every_resource() {
        let mut input = batches(1..=1024);
        for (id, signal, _) in &mut input {
            *signal = match id.unwrap() % 3 {
                0 => TelemetrySignal::Logs,
                1 => TelemetrySignal::Traces,
                _ => TelemetrySignal::Metrics,
            };
        }
        let groups = group_telemetry(input, TELEMETRY_GROUP_BATCHES);
        assert_eq!(groups.len(), 8);
        let mut received = Vec::new();
        for group in groups {
            assert_eq!(group.count, 128);
            assert!(group.bytes <= TELEMETRY_GROUP_BYTES);
            assert_eq!(group.signals.len(), 3);
            for (signal, data) in group.signals {
                let ids = batch_ids(&data);
                let resources = match signal {
                    TelemetrySignal::Logs => ExportLogsServiceRequest::decode(data.as_slice())
                        .unwrap()
                        .resource_logs
                        .len(),
                    TelemetrySignal::Traces => ExportTraceServiceRequest::decode(data.as_slice())
                        .unwrap()
                        .resource_spans
                        .len(),
                    TelemetrySignal::Metrics => {
                        ExportMetricsServiceRequest::decode(data.as_slice())
                            .unwrap()
                            .resource_metrics
                            .len()
                    }
                };
                assert_eq!(resources, ids.len());
                for id in &ids {
                    assert_eq!(
                        *id % 3,
                        match signal {
                            TelemetrySignal::Logs => 0,
                            TelemetrySignal::Traces => 1,
                            TelemetrySignal::Metrics => 2,
                        }
                    );
                }
                received.extend(ids);
            }
        }
        received.sort();
        assert_eq!(received, (1..=1024).collect::<Vec<_>>());
    }

    #[test]
    fn grouping_bounds_payloads_and_keeps_legacy_batches_out_of_the_budget() {
        let mut request = ExportLogsServiceRequest::decode(batch_data(2).as_slice()).unwrap();
        request.resource_logs[0]
            .resource
            .as_mut()
            .unwrap()
            .attributes
            .push(KeyValue {
                key: "payload".into(),
                value: Some(AnyValue {
                    value: Some(Value::StringValue("x".repeat(TELEMETRY_GROUP_BYTES))),
                }),
            });
        let large = request.encode_to_vec();
        let groups = group_telemetry(
            vec![
                (None, TelemetrySignal::Logs, batch_data(0)),
                (Some(1), TelemetrySignal::Logs, batch_data(1)),
                (Some(2), TelemetrySignal::Logs, large.clone()),
                (Some(3), TelemetrySignal::Logs, batch_data(3)),
            ],
            TELEMETRY_GROUP_BATCHES,
        );
        assert_eq!(
            groups.iter().map(|group| group.through).collect::<Vec<_>>(),
            vec![None, Some(1), Some(2), Some(3)]
        );
        assert_eq!(
            groups[2].signals[0].1, large,
            "an oversized original batch is forwarded alone without truncation"
        );
    }

    struct RejectOneGroup {
        reject_id: i64,
        reject: std::sync::atomic::AtomicBool,
        received: Mutex<Vec<i64>>,
    }

    #[tokio::test]
    async fn an_expired_budget_still_forwards_legacy_batches_without_acknowledging_new_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
        let backend = Arc::new(SlowBackend {
            delay: Duration::ZERO,
            ..Default::default()
        });
        let mut input = batches(0..=1);
        input[0].0 = None;
        let forwarded = forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".into(),
            input,
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(forwarded.accepted, 1);
        assert_eq!(forwarded.through, None);
        assert_eq!(*backend.received.lock().unwrap(), vec![0]);
        assert_eq!(
            read_telemetry_mark(kv.as_ref(), "mark").await.unwrap(),
            None
        );
    }

    #[async_trait]
    impl TelemetryBackend for RejectOneGroup {
        async fn ingest(
            &self,
            _signal: TelemetrySignal,
            _caller: &TelemetryCaller,
            data: bytes::Bytes,
        ) -> Result<(), AlienError> {
            let ids = batch_ids(&data);
            if ids.contains(&self.reject_id)
                && self.reject.swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(ErrorData::internal("rejected group").into_generic());
            }
            self.received.lock().unwrap().extend(ids);
            Ok(())
        }
    }

    #[tokio::test]
    async fn failed_group_never_advances_the_mark_and_replay_finishes_the_backlog() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
        let backend = Arc::new(RejectOneGroup {
            reject_id: 129,
            reject: std::sync::atomic::AtomicBool::new(true),
            received: Mutex::new(Vec::new()),
        });
        let first = forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".into(),
            batches(1..=1024),
            Duration::from_secs(5),
        )
        .await;
        assert!(first.is_err());
        assert_eq!(
            read_telemetry_mark(kv.as_ref(), "mark").await.unwrap(),
            Some(128)
        );
        let replay = forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".into(),
            batches(1..=1024),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(replay.through, Some(1024));
        assert_eq!(replay.accepted, 896);
        let received = backend.received.lock().unwrap();
        for id in 1..=1024 {
            let count = received.iter().filter(|&&found| found == id).count();
            if id <= 128 {
                assert_eq!(count, 1);
            } else {
                assert!(count >= 1);
            }
        }
    }

    #[tokio::test]
    async fn one_successful_signal_does_not_acknowledge_a_partially_failed_group() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
        let backend = Arc::new(RejectOneGroup {
            reject_id: 200,
            reject: std::sync::atomic::AtomicBool::new(true),
            received: Mutex::new(Vec::new()),
        });
        let mut input = batches(1..=256);
        input[199].1 = TelemetrySignal::Traces;
        assert!(forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".into(),
            input.clone(),
            Duration::from_secs(5)
        )
        .await
        .is_err());
        assert!(
            backend.received.lock().unwrap().contains(&256),
            "logs in the failed window reached the backend"
        );
        assert_eq!(
            read_telemetry_mark(kv.as_ref(), "mark").await.unwrap(),
            Some(128)
        );
        let replay = forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".into(),
            input,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(replay.through, Some(256));
        assert_eq!(replay.accepted, 128);
        assert!(backend.received.lock().unwrap().contains(&200));
    }

    #[tokio::test(start_paused = true)]
    async fn slow_mixed_signals_share_the_write_limit_and_return_partial_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
        let backend = Arc::new(SlowBackend {
            delay: Duration::from_secs(25),
            ..Default::default()
        });
        let mut input = batches(1..=1024);
        for (id, signal, _) in &mut input {
            *signal = match id.unwrap() % 3 {
                0 => TelemetrySignal::Logs,
                1 => TelemetrySignal::Traces,
                _ => TelemetrySignal::Metrics,
            };
        }
        let started = Instant::now();
        let first = forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".into(),
            input.clone(),
            TELEMETRY_BUDGET,
        )
        .await
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "mixed signals must not wait for three serial backend flushes: {:?}",
            started.elapsed()
        );
        assert!(first.accepted > 0 && first.accepted < 1024);
        assert_eq!(first.through, Some(first.accepted as i64));
        assert_eq!(
            read_telemetry_mark(kv.as_ref(), "mark").await.unwrap(),
            first.through
        );
        assert_eq!(
            backend.max_active.load(std::sync::atomic::Ordering::SeqCst),
            TELEMETRY_CONCURRENCY
        );
        let replay = forward_telemetry(
            backend.clone(),
            kv.clone(),
            caller(),
            "mark".into(),
            input,
            TELEMETRY_BUDGET,
        )
        .await
        .unwrap();
        assert_eq!(replay.through, Some(1024));
        let received = backend.received.lock().unwrap();
        for id in 1..=1024 {
            let count = received.iter().filter(|&&found| found == id).count();
            if id <= first.through.unwrap() {
                assert_eq!(count, 1);
            } else {
                assert!(count >= 1);
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_backlog_is_forwarded_across_reports_within_the_budget_without_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let kv: Arc<dyn Kv> = Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
        let backend = Arc::new(SlowBackend {
            delay: Duration::from_millis(200),
            ..Default::default()
        });
        let budget = Duration::from_millis(500);
        let last_id = (TELEMETRY_GROUP_BATCHES * TELEMETRY_CONCURRENCY * 4) as i64;
        let backlog = 1..=last_id;

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
            first.accepted >= 2 * TELEMETRY_CONCURRENCY * TELEMETRY_GROUP_BATCHES
                && first.accepted < last_id as usize,
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
        while last.through != Some(last_id) {
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
