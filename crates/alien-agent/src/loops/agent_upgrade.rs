//! Agent self-update actuator (Kubernetes regime). When `/v1/sync` returns
//! `agent_target.helm`, this module reconciles a Helm-runner Job that runs
//! `helm upgrade --reuse-values` to flip the agent's image tag (and any other
//! values overrides the manager sent).
//!
//! Unlike the original fire-and-forget version, this is **status-aware**: it
//! reads the live state of the upgrader Job(s) so it can (a) report progress and
//! failures back to the manager via `SyncRequest.agent_update`, and (b) retry
//! with exponential backoff without the old 409-on-a-dead-Job stall. Each retry
//! is a *distinct* Job (`<release>-upgrader-<version>-<attempt>`), so a failed
//! attempt never blocks the next one and its pod survives its TTL for debugging.
//!
//! Talks to the Kubernetes API directly (GET/POST to `/apis/batch/v1/...` and
//! `/api/v1/...`) using the in-pod ServiceAccount token, avoiding a dependency
//! on `alien-k8s-clients`.
//!
//! `os-service` regime is not in this MVP — the wire carries
//! `agent_target.binary` for it, but the actuator is unimplemented and this
//! module logs + skips that path (its failure reporting lands with that work).

use std::fs;
use std::time::Duration;

use alien_core::sync::{AgentHelmTarget, AgentTarget, AgentUpdatePhase, AgentUpdateReport};
use alien_error::AlienError;
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::{info, warn};

use crate::error::{ErrorData, Result};

const SA_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";
const SA_CA_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt";

const COMPONENT_SELECTOR: &str = "alien.dev/component=agent-upgrader";
const ANNOTATION_TARGET_VERSION: &str = "alien.dev/target-version";
const LABEL_ATTEMPT: &str = "alien.dev/attempt";

/// Exponential backoff between failed attempts: `30s * 2^(attempt-1)`, capped.
const BACKOFF_BASE_SECS: u64 = 30;
const BACKOFF_MAX_SECS: u64 = 300;

/// Container `waiting.reason`s that mean "the image could not be pulled".
const IMAGE_PULL_REASONS: &[&str] = &[
    "ImagePullBackOff",
    "ErrImagePull",
    "InvalidImageName",
    "ImageInspectError",
    "RegistryUnavailable",
    "ErrImageNeverPull",
];

// ============================================================================
// Public API — called from the sync loop
// ============================================================================

/// Reconcile the upgrader Job for `target`: spawn the first attempt, wait while
/// one is running, or (after exponential backoff) spawn the next attempt when
/// the last one failed. Best-effort — never fails the sync; the report the
/// manager sees comes from [`current_update_report`] on the next request.
pub async fn apply_agent_target(target: &AgentTarget) {
    let Some(helm) = target.helm.as_ref() else {
        warn!(
            "Received agent_target with no helm payload; os-service upgrade path is not implemented in this MVP"
        );
        return;
    };
    let namespace = match k8s_namespace() {
        Ok(ns) => ns,
        Err(e) => {
            warn!(error = %e, "Cannot reconcile agent update — namespace unavailable");
            return;
        }
    };

    let jobs = match list_upgrader_jobs(&namespace).await {
        Ok(jobs) => jobs,
        Err(e) => {
            warn!(error = %e, "Failed to list upgrader Jobs; skipping this tick");
            return;
        }
    };
    let for_version: Vec<UpgraderJob> = jobs
        .into_iter()
        .filter(|j| j.target_version == target.version)
        .collect();

    match decide_action(&for_version, Utc::now()) {
        Action::Wait => {}
        Action::Create(attempt) => {
            if let Some(prev) = for_version.iter().find(|j| j.status == JobStatus::Failed) {
                warn!(
                    target_version = %target.version,
                    failed_attempt = prev.attempt,
                    "Previous upgrader Job failed; spawning retry attempt {attempt}"
                );
            }
            match resolve_job_inputs(target, helm, attempt) {
                Ok(inputs) => {
                    let (name, body) = build_job_body(&inputs);
                    match create_job(&namespace, &body).await {
                        Ok(()) => info!(
                            target_version = %target.version,
                            job_name = %name,
                            attempt,
                            "Spawned agent upgrader Job"
                        ),
                        Err(e) => warn!(
                            error = %e,
                            attempt,
                            "Failed to spawn agent upgrader Job; will retry on next sync"
                        ),
                    }
                }
                Err(e) => warn!(error = %e, "Failed to resolve upgrader Job inputs"),
            }
        }
    }
}

/// Derive the `agent_update` field for the next `SyncRequest` from the live
/// state of the newest upgrader Job. Returns `None` when no update is in flight
/// (or the agent is not in a Kubernetes pod).
pub async fn current_update_report() -> Option<AgentUpdateReport> {
    let namespace = k8s_namespace().ok()?;
    let jobs = list_upgrader_jobs(&namespace).await.ok()?;
    let newest = jobs.iter().max_by_key(|j| j.created_at)?;

    match newest.status {
        JobStatus::Active | JobStatus::Succeeded => Some(AgentUpdateReport::InProgress {
            target_version: newest.target_version.clone(),
            attempt: newest.attempt,
        }),
        JobStatus::Failed => {
            let release = std::env::var("KUBERNETES_HELM_RELEASE").unwrap_or_default();
            let (phase, message) = classify_failure(&namespace, &release, newest).await;
            Some(AgentUpdateReport::Failed {
                target_version: newest.target_version.clone(),
                phase,
                message,
                attempt: newest.attempt,
            })
        }
    }
}

// ============================================================================
// Pure decision core (unit-tested)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobStatus {
    Active,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
struct UpgraderJob {
    #[allow(dead_code)]
    name: String,
    target_version: String,
    attempt: u32,
    status: JobStatus,
    created_at: DateTime<Utc>,
    /// When the Job reached a terminal condition (for backoff). None if unknown.
    finished_at: Option<DateTime<Utc>>,
    /// Message from the `Failed` condition, if any.
    failed_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Spawn a new attempt with this 1-based number.
    Create(u32),
    /// A Job is running, succeeded, or is within its backoff window.
    Wait,
}

/// Parse a k8s Job object into an [`UpgraderJob`]. Returns `None` if it is not a
/// recognizable upgrader Job (missing name).
fn parse_upgrader_job(job: &Value) -> Option<UpgraderJob> {
    let meta = job.get("metadata")?;
    let name = meta.get("name")?.as_str()?.to_string();

    let target_version = meta
        .get("annotations")
        .and_then(|a| a.get(ANNOTATION_TARGET_VERSION))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let attempt = meta
        .get("labels")
        .and_then(|l| l.get(LABEL_ATTEMPT))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1);
    let created_at = meta
        .get("creationTimestamp")
        .and_then(Value::as_str)
        .and_then(parse_ts)
        .unwrap_or_else(Utc::now);

    let mut status = JobStatus::Active;
    let mut finished_at = None;
    let mut failed_message = None;
    if let Some(conds) = job
        .get("status")
        .and_then(|s| s.get("conditions"))
        .and_then(Value::as_array)
    {
        for cond in conds {
            let ctype = cond.get("type").and_then(Value::as_str).unwrap_or_default();
            let ctrue = cond.get("status").and_then(Value::as_str) == Some("True");
            if !ctrue {
                continue;
            }
            let ts = cond
                .get("lastTransitionTime")
                .and_then(Value::as_str)
                .and_then(parse_ts);
            match ctype {
                "Failed" => {
                    status = JobStatus::Failed;
                    finished_at = ts;
                    failed_message = cond
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                "Complete" => {
                    if status != JobStatus::Failed {
                        status = JobStatus::Succeeded;
                        finished_at = ts;
                    }
                }
                _ => {}
            }
        }
    }

    Some(UpgraderJob {
        name,
        target_version,
        attempt,
        status,
        created_at,
        finished_at,
        failed_message,
    })
}

/// Backoff before retrying attempt `attempt` (1-based): `30s * 2^(attempt-1)`,
/// capped at [`BACKOFF_MAX_SECS`].
fn backoff_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(20);
    let secs = BACKOFF_BASE_SECS
        .saturating_mul(1u64 << shift)
        .min(BACKOFF_MAX_SECS);
    Duration::from_secs(secs)
}

/// Whether a failed Job's backoff window has elapsed and a retry is due.
fn should_retry(job: &UpgraderJob, now: DateTime<Utc>) -> bool {
    match job.finished_at {
        Some(finished) => {
            let elapsed = now.signed_duration_since(finished);
            elapsed.to_std().map(|d| d >= backoff_delay(job.attempt)).unwrap_or(true)
        }
        // Failed but no timestamp — be permissive and allow the retry.
        None => true,
    }
}

/// Decide what to do given the upgrader Jobs for a single target version.
fn decide_action(jobs_for_version: &[UpgraderJob], now: DateTime<Utc>) -> Action {
    match jobs_for_version.iter().max_by_key(|j| j.attempt) {
        None => Action::Create(1),
        Some(newest) => match newest.status {
            JobStatus::Active | JobStatus::Succeeded => Action::Wait,
            JobStatus::Failed => {
                if should_retry(newest, now) {
                    Action::Create(newest.attempt + 1)
                } else {
                    Action::Wait
                }
            }
        },
    }
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn is_image_pull_reason(reason: &str) -> bool {
    IMAGE_PULL_REASONS.iter().any(|r| *r == reason)
}

/// Scan a pod's container statuses for an image-pull `waiting` state. Returns
/// `(image, reason, message)` if found. Pure — takes the pod JSON.
fn image_pull_waiting(pod: &Value) -> Option<(String, String, String)> {
    let statuses = ["containerStatuses", "initContainerStatuses"];
    let podspec_status = pod.get("status")?;
    for key in statuses {
        let Some(list) = podspec_status.get(key).and_then(Value::as_array) else {
            continue;
        };
        for cs in list {
            let Some(waiting) = cs.get("state").and_then(|s| s.get("waiting")) else {
                continue;
            };
            let reason = waiting.get("reason").and_then(Value::as_str).unwrap_or_default();
            if is_image_pull_reason(reason) {
                let image = cs.get("image").and_then(Value::as_str).unwrap_or_default();
                let message = waiting.get("message").and_then(Value::as_str).unwrap_or_default();
                return Some((image.to_string(), reason.to_string(), message.to_string()));
            }
        }
    }
    None
}

// ============================================================================
// Failure classification (I/O)
// ============================================================================

/// Determine whether a failed upgrade was a `Pull` failure (the new agent image
/// couldn't be pulled — the ImagePullBackOff case) or an `Apply` failure (helm
/// upgrade / rollback). Inspects the agent workload pods for an image-pull
/// waiting state; falls back to `Apply` with the Job's failure message.
async fn classify_failure(
    namespace: &str,
    release: &str,
    job: &UpgraderJob,
) -> (AgentUpdatePhase, String) {
    if !release.is_empty() {
        let selector = format!("app.kubernetes.io/instance={release}");
        if let Ok(pods) = list_pods(namespace, &selector).await {
            for pod in &pods {
                // Skip the upgrader runner pods; we want the agent workload pod.
                let is_upgrader = pod
                    .get("metadata")
                    .and_then(|m| m.get("labels"))
                    .and_then(|l| l.get("alien.dev/component"))
                    .and_then(Value::as_str)
                    == Some("agent-upgrader");
                if is_upgrader {
                    continue;
                }
                if let Some((image, reason, message)) = image_pull_waiting(pod) {
                    return (
                        AgentUpdatePhase::Pull,
                        format!("{image}: {reason} {message}").trim().to_string(),
                    );
                }
            }
        }
    }
    let message = job
        .failed_message
        .clone()
        .unwrap_or_else(|| "helm upgrade failed".to_string());
    (AgentUpdatePhase::Apply, message)
}

// ============================================================================
// Job body construction (pure)
// ============================================================================

/// Everything `build_job_body` needs that isn't a string literal.
struct JobInputs {
    target_version: String,
    attempt: u32,
    namespace: String,
    release: String,
    upgrader_sa: String,
    chart_ref: String,
    /// `None` means "let helm pick latest" — omits the `--version` flag.
    chart_version: Option<String>,
    runner_image: String,
    /// Extra flags spliced into the `helm upgrade` command verbatim.
    extra_args: String,
    /// Pre-serialized JSON for `agent_target.helm.values`.
    values_json: String,
}

/// Read env vars + apply the manager-vs-env fallback for chart ref/version, and
/// serialise the values overlay. `attempt` is decided by the reconcile.
fn resolve_job_inputs(target: &AgentTarget, helm: &AgentHelmTarget, attempt: u32) -> Result<JobInputs> {
    let namespace = k8s_namespace()?;
    let release = std::env::var("KUBERNETES_HELM_RELEASE").map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "KUBERNETES_HELM_RELEASE env var missing — set by the chart at install time"
                .to_string(),
        })
    })?;
    let upgrader_sa = std::env::var("ALIEN_AGENT_UPGRADER_SA").map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "ALIEN_AGENT_UPGRADER_SA env var missing — set by the chart at install time"
                .to_string(),
        })
    })?;

    let chart_ref = non_empty(&helm.chart_repo)
        .map(String::from)
        .or_else(|| std::env::var("ALIEN_AGENT_CHART_REF").ok())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message:
                    "agent_target.helm.chart_repo empty AND ALIEN_AGENT_CHART_REF unset — \
                     can't run helm upgrade without a chart reference"
                        .to_string(),
            })
        })?;
    let chart_version = non_empty(&helm.chart_version)
        .map(String::from)
        .or_else(|| std::env::var("ALIEN_AGENT_CHART_VERSION").ok().filter(|s| !s.is_empty()));
    let runner_image = std::env::var("ALIEN_AGENT_HELM_RUNNER_IMAGE")
        .unwrap_or_else(|_| "alpine/helm:3.18.4".to_string());
    let extra_args = std::env::var("ALIEN_AGENT_HELM_EXTRA_ARGS").unwrap_or_default();

    let values_json = serde_json::to_string(&helm.values).map_err(|e| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to serialize agent_target.helm.values: {e}"),
        })
    })?;

    Ok(JobInputs {
        target_version: target.version.clone(),
        attempt,
        namespace,
        release,
        upgrader_sa,
        chart_ref,
        chart_version,
        runner_image,
        extra_args,
        values_json,
    })
}

/// Pure: build the Job name + the JSON body POSTed to the k8s API. The name is
/// unique per attempt so a failed attempt never blocks a retry; the raw target
/// version rides in an annotation (label values can't hold `+build` metadata).
fn build_job_body(inputs: &JobInputs) -> (String, serde_json::Value) {
    let job_name = format!(
        "{}-upgrader-{}-{}",
        inputs.release,
        sanitize_for_dns(&inputs.target_version, 16),
        inputs.attempt
    );

    let version_flag = inputs
        .chart_version
        .as_deref()
        .map(|v| format!(" --version {v}"))
        .unwrap_or_default();
    let extra_args_flag = if inputs.extra_args.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", inputs.extra_args.trim())
    };
    let helm_cmd = format!(
        "set -e\n\
         printf '%s' \"$VALUES_JSON\" > /tmp/values.json\n\
         exec helm upgrade \"$RELEASE\" \"$CHART_REF\"{version_flag}{extra_args_flag} \
            --namespace \"$NAMESPACE\" \
            --reuse-values \
            --atomic --wait \
            --values /tmp/values.json"
    );

    let body = serde_json::json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": {
            "name": job_name,
            "namespace": inputs.namespace,
            "labels": {
                "app.kubernetes.io/managed-by": "alien-agent",
                "alien.dev/component": "agent-upgrader",
                LABEL_ATTEMPT: inputs.attempt.to_string(),
            },
            "annotations": {
                ANNOTATION_TARGET_VERSION: inputs.target_version,
            },
        },
        "spec": {
            "backoffLimit": 1,
            "ttlSecondsAfterFinished": 600,
            "template": {
                "metadata": {
                    "labels": {
                        "app.kubernetes.io/managed-by": "alien-agent",
                        "alien.dev/component": "agent-upgrader",
                    },
                },
                "spec": {
                    "serviceAccountName": inputs.upgrader_sa,
                    "restartPolicy": "Never",
                    "containers": [{
                        "name": "helm-upgrade",
                        "image": inputs.runner_image,
                        "command": ["sh", "-c", helm_cmd],
                        "env": [
                            { "name": "RELEASE", "value": inputs.release },
                            { "name": "NAMESPACE", "value": inputs.namespace },
                            { "name": "CHART_REF", "value": inputs.chart_ref },
                            { "name": "VALUES_JSON", "value": inputs.values_json },
                        ],
                    }],
                },
            },
        },
    });

    (job_name, body)
}

// ============================================================================
// Kubernetes API I/O
// ============================================================================

fn k8s_namespace() -> Result<String> {
    std::env::var("KUBERNETES_NAMESPACE").map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "KUBERNETES_NAMESPACE env var missing — agent self-update needs it".to_string(),
        })
    })
}

/// Build an HTTP client + base URL + bearer token for the in-pod k8s API.
fn k8s_conn() -> Result<(reqwest::Client, String, String)> {
    let token = fs::read_to_string(SA_TOKEN_PATH).map_err(|e| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to read in-pod SA token at {SA_TOKEN_PATH}: {e}"),
        })
    })?;
    let host = std::env::var("KUBERNETES_SERVICE_HOST").map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "KUBERNETES_SERVICE_HOST env var missing — agent not running in a pod?"
                .to_string(),
        })
    })?;
    let port = std::env::var("KUBERNETES_SERVICE_PORT_HTTPS").unwrap_or_else(|_| "443".to_string());

    let mut builder = reqwest::ClientBuilder::new().timeout(Duration::from_secs(15));
    if let Ok(ca_pem) = fs::read(SA_CA_PATH) {
        if let Ok(cert) = reqwest::Certificate::from_pem(&ca_pem) {
            builder = builder.add_root_certificate(cert);
        }
    }
    let client = builder.build().map_err(|e| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to build k8s API HTTP client: {e}"),
        })
    })?;
    Ok((client, format!("https://{host}:{port}"), token.trim().to_string()))
}

async fn k8s_get(path: &str, query: &[(&str, &str)]) -> Result<Value> {
    let (client, base, token) = k8s_conn()?;
    let url = format!("{base}{path}");
    let resp = client
        .get(&url)
        .query(query)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("Failed to GET k8s API {url}: {e}"),
            })
        })?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("k8s API GET {url} returned {status}: {text}"),
        }));
    }
    resp.json().await.map_err(|e| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to parse k8s API response from {url}: {e}"),
        })
    })
}

async fn list_upgrader_jobs(namespace: &str) -> Result<Vec<UpgraderJob>> {
    let path = format!("/apis/batch/v1/namespaces/{namespace}/jobs");
    let v = k8s_get(&path, &[("labelSelector", COMPONENT_SELECTOR)]).await?;
    Ok(v.get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_upgrader_job).collect())
        .unwrap_or_default())
}

async fn list_pods(namespace: &str, selector: &str) -> Result<Vec<Value>> {
    let path = format!("/api/v1/namespaces/{namespace}/pods");
    let v = k8s_get(&path, &[("labelSelector", selector)]).await?;
    Ok(v.get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// POST a Job to the in-pod Kubernetes API. Unique per-attempt names make a 409
/// a rare same-sync race rather than the old dead-Job stall — treat it as benign.
async fn create_job(namespace: &str, body: &serde_json::Value) -> Result<()> {
    let (client, base, token) = k8s_conn()?;
    let url = format!("{base}/apis/batch/v1/namespaces/{namespace}/jobs");
    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(body)
        .send()
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("Failed to call k8s API at {url}: {e}"),
            })
        })?;

    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    if status.as_u16() == 409 {
        warn!("Upgrader Job create returned 409 (race with a concurrent sync); leaving in place");
        return Ok(());
    }
    let text = resp.text().await.unwrap_or_default();
    Err(AlienError::new(ErrorData::ConfigurationError {
        message: format!("k8s API rejected Job creation ({status}): {text}"),
    }))
}

fn non_empty(s: &str) -> Option<&str> {
    if s.is_empty() { None } else { Some(s) }
}

/// Lowercase, DNS-1123-safe, max `cap` chars, trimmed of trailing hyphens.
fn sanitize_for_dns(value: &str, cap: usize) -> String {
    let mut out: String = value
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    out.truncate(cap);
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inputs() -> JobInputs {
        JobInputs {
            target_version: "1.4.0".to_string(),
            attempt: 1,
            namespace: "alien-agent".to_string(),
            release: "alien".to_string(),
            upgrader_sa: "alien-agent-upgrader".to_string(),
            chart_ref: "oci://ghcr.io/alien-dev/alien".to_string(),
            chart_version: Some("1.4.0".to_string()),
            runner_image: "alpine/helm:3.18.4".to_string(),
            extra_args: String::new(),
            values_json: r#"{"runtime":{"image":{"tag":"1.4.0"}}}"#.to_string(),
        }
    }

    fn ts(s: &str) -> DateTime<Utc> {
        parse_ts(s).unwrap()
    }

    #[test]
    fn job_name_includes_release_version_and_attempt() {
        let (name, _) = build_job_body(&inputs());
        assert_eq!(name, "alien-upgrader-1-4-0-1");
        let mut i = inputs();
        i.attempt = 3;
        let (name, _) = build_job_body(&i);
        assert_eq!(name, "alien-upgrader-1-4-0-3");
    }

    #[test]
    fn job_name_caps_long_version_and_trims_trailing_hyphens() {
        let mut i = inputs();
        i.target_version = "1.4.0-rc.1+build".to_string();
        let (name, _) = build_job_body(&i);
        assert!(name.starts_with("alien-upgrader-"), "got {name}");
        assert!(name.ends_with("-1"), "attempt suffix should survive: {name}");
        // No double-hyphen from a trimmed sanitized core meeting the attempt.
        assert!(!name.contains("--"), "no empty segment: {name}");
    }

    #[test]
    fn job_metadata_carries_attempt_label_and_raw_version_annotation() {
        let mut i = inputs();
        i.attempt = 2;
        i.target_version = "1.4.0+build.7".to_string();
        let (_, body) = build_job_body(&i);
        assert_eq!(body["metadata"]["labels"]["alien.dev/attempt"], json!("2"));
        // Raw version (with '+') preserved in the annotation, not the label.
        assert_eq!(
            body["metadata"]["annotations"]["alien.dev/target-version"],
            json!("1.4.0+build.7")
        );
    }

    #[test]
    fn helm_command_omits_version_flag_when_unset() {
        let mut i = inputs();
        i.chart_version = None;
        let (_, body) = build_job_body(&i);
        let cmd = body["spec"]["template"]["spec"]["containers"][0]["command"][2]
            .as_str()
            .unwrap();
        assert!(!cmd.contains("--version"), "got: {cmd}");
        assert!(cmd.contains("--reuse-values"), "got: {cmd}");
        assert!(cmd.contains("--atomic --wait"), "got: {cmd}");
    }

    #[test]
    fn env_vars_carry_manager_sent_values_and_release_metadata() {
        let (_, body) = build_job_body(&inputs());
        let envs = body["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array()
            .unwrap();
        let by_name: std::collections::HashMap<_, _> = envs
            .iter()
            .map(|e| (e["name"].as_str().unwrap(), e["value"].as_str().unwrap()))
            .collect();
        assert_eq!(by_name["RELEASE"], "alien");
        assert_eq!(by_name["CHART_REF"], "oci://ghcr.io/alien-dev/alien");
        assert_eq!(by_name["VALUES_JSON"], r#"{"runtime":{"image":{"tag":"1.4.0"}}}"#);
    }

    #[test]
    fn helm_command_splices_extra_args_before_namespace() {
        let mut i = inputs();
        i.extra_args = "--plain-http".to_string();
        let (_, body) = build_job_body(&i);
        let cmd = body["spec"]["template"]["spec"]["containers"][0]["command"][2]
            .as_str()
            .unwrap();
        let plain_pos = cmd.find("--plain-http").expect("--plain-http present");
        let ns_pos = cmd.find("--namespace").expect("--namespace present");
        assert!(plain_pos < ns_pos, "--plain-http before --namespace: {cmd}");
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        assert_eq!(backoff_delay(1), Duration::from_secs(30));
        assert_eq!(backoff_delay(2), Duration::from_secs(60));
        assert_eq!(backoff_delay(3), Duration::from_secs(120));
        assert_eq!(backoff_delay(4), Duration::from_secs(240));
        assert_eq!(backoff_delay(5), Duration::from_secs(300)); // capped
        assert_eq!(backoff_delay(50), Duration::from_secs(300)); // no overflow
    }

    fn failed_job(attempt: u32, finished: &str) -> UpgraderJob {
        UpgraderJob {
            name: format!("alien-upgrader-1-4-0-{attempt}"),
            target_version: "1.4.0".to_string(),
            attempt,
            status: JobStatus::Failed,
            created_at: ts(finished),
            finished_at: Some(ts(finished)),
            failed_message: Some("BackoffLimitExceeded".to_string()),
        }
    }

    #[test]
    fn decide_creates_first_attempt_when_no_jobs() {
        assert_eq!(decide_action(&[], Utc::now()), Action::Create(1));
    }

    #[test]
    fn decide_waits_while_active() {
        let mut j = failed_job(1, "2026-07-05T12:00:00Z");
        j.status = JobStatus::Active;
        assert_eq!(decide_action(&[j], Utc::now()), Action::Wait);
    }

    #[test]
    fn decide_retries_failed_after_backoff_elapses() {
        let j = failed_job(1, "2026-07-05T12:00:00Z");
        // attempt 1 backoff = 30s; 40s later -> retry as attempt 2.
        let now = ts("2026-07-05T12:00:40Z");
        assert_eq!(decide_action(&[j], now), Action::Create(2));
    }

    #[test]
    fn decide_waits_within_backoff_window() {
        let j = failed_job(2, "2026-07-05T12:00:00Z");
        // attempt 2 backoff = 60s; only 20s later -> wait.
        let now = ts("2026-07-05T12:00:20Z");
        assert_eq!(decide_action(&[j], now), Action::Wait);
    }

    #[test]
    fn image_pull_waiting_detects_backoff_reason() {
        let pod = json!({
            "status": {
                "containerStatuses": [{
                    "image": "public.ecr.aws/x/agent:1.4.0",
                    "state": { "waiting": { "reason": "ImagePullBackOff", "message": "not found" } }
                }]
            }
        });
        let (image, reason, message) = image_pull_waiting(&pod).expect("should detect");
        assert_eq!(image, "public.ecr.aws/x/agent:1.4.0");
        assert_eq!(reason, "ImagePullBackOff");
        assert_eq!(message, "not found");
    }

    #[test]
    fn image_pull_waiting_ignores_running_pod() {
        let pod = json!({
            "status": { "containerStatuses": [{ "state": { "running": {} } }] }
        });
        assert!(image_pull_waiting(&pod).is_none());
    }

    #[test]
    fn parse_upgrader_job_reads_labels_annotations_and_failed_status() {
        let job = json!({
            "metadata": {
                "name": "alien-upgrader-1-4-0-2",
                "creationTimestamp": "2026-07-05T12:00:00Z",
                "labels": { "alien.dev/attempt": "2" },
                "annotations": { "alien.dev/target-version": "1.4.0+build" }
            },
            "status": {
                "conditions": [
                    { "type": "Failed", "status": "True", "lastTransitionTime": "2026-07-05T12:05:00Z", "message": "BackoffLimitExceeded" }
                ]
            }
        });
        let parsed = parse_upgrader_job(&job).unwrap();
        assert_eq!(parsed.attempt, 2);
        assert_eq!(parsed.target_version, "1.4.0+build");
        assert_eq!(parsed.status, JobStatus::Failed);
        assert_eq!(parsed.failed_message.as_deref(), Some("BackoffLimitExceeded"));
        assert_eq!(parsed.finished_at, Some(ts("2026-07-05T12:05:00Z")));
    }
}
