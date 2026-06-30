//! Agent self-update actuator. When `/v1/sync` returns
//! `agent_target.helm`, this module creates a Kubernetes Job that runs
//! `helm upgrade --reuse-values` to flip the agent's image tag (and any
//! other values overrides the manager sent). The Job uses the
//! `alien-agent-upgrader` ServiceAccount the chart wires for this purpose;
//! the agent itself only needs Create permission on Jobs in its own
//! namespace, granted by the existing role.
//!
//! Talks to the Kubernetes API directly (POSTs to `/apis/batch/v1/...`)
//! using the in-pod ServiceAccount token mounted at
//! `/var/run/secrets/kubernetes.io/serviceaccount/`. Avoids pulling
//! `alien-k8s-clients` (and its k8s-openapi tree) into the agent crate
//! just for one Job-create call.
//!
//! `os-service` regime is not in this MVP — the wire format carries
//! `agent_target.binary` for it, but the actuator is unimplemented and
//! this module logs + skips that path.

use std::fs;
use std::time::Duration;

use alien_core::sync::{AgentHelmTarget, AgentTarget};
use alien_error::AlienError;
use tracing::{info, warn};

use crate::error::{ErrorData, Result};

const SA_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";
const SA_CA_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt";

/// Best-effort: emit the upgrader Job. Errors are logged but do not fail
/// the sync — the manager will keep emitting `agent_target` on each tick
/// until the agent reports the new version.
pub async fn apply_agent_target(target: &AgentTarget) {
    let Some(helm) = target.helm.as_ref() else {
        warn!(
            "Received agent_target with no helm payload; os-service upgrade path is not implemented in this MVP"
        );
        return;
    };
    if let Err(e) = spawn_helm_runner_job(target, helm).await {
        warn!(error = %e, target_version = %target.version, "Failed to spawn agent upgrader Job; will retry on next sync");
    }
}

async fn spawn_helm_runner_job(target: &AgentTarget, helm: &AgentHelmTarget) -> Result<()> {
    let inputs = resolve_job_inputs(target, helm)?;
    let (job_name, body) = build_job_body(&inputs);
    create_job(&inputs.namespace, &body).await?;
    info!(
        target_version = %inputs.target_version,
        job_name = %job_name,
        "Spawned agent upgrader Job"
    );
    Ok(())
}

/// Everything `build_job_body` needs that isn't a string literal. Separated
/// so the pure body builder is easy to unit-test without env-var or fs
/// mocks.
struct JobInputs {
    target_version: String,
    namespace: String,
    release: String,
    upgrader_sa: String,
    chart_ref: String,
    /// `None` means "let helm pick latest" — omits the `--version` flag.
    chart_version: Option<String>,
    runner_image: String,
    /// Extra flags spliced into the `helm upgrade` command verbatim.
    /// Empty in production; populated for local-dev escape hatches like
    /// `--plain-http` against HTTP-only OCI registries.
    extra_args: String,
    /// Pre-serialized JSON for `agent_target.helm.values`. Injected into
    /// the upgrader pod via the `VALUES_JSON` env var and re-materialised
    /// to a file inside the container.
    values_json: String,
}

/// Read env vars + apply the manager-vs-env fallback for chart ref/version,
/// and serialise the values overlay. This is the impure layer.
fn resolve_job_inputs(target: &AgentTarget, helm: &AgentHelmTarget) -> Result<JobInputs> {
    let namespace = std::env::var("KUBERNETES_NAMESPACE").map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "KUBERNETES_NAMESPACE env var missing — agent self-update Job needs it"
                .to_string(),
        })
    })?;
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

    // The manager may have sent chart_repo / chart_version empty, in which
    // case we fall back to the env vars the chart injects at install time.
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

/// Pure: build the Job name + the JSON body POSTed to the k8s API.
/// `--reuse-values` keeps install-time values (management.*, encryption key,
/// etc.); the manager-sent values overlay (written to /tmp/values.json
/// inside the runner pod) wins for whatever it sets — typically just
/// `runtime.image.tag`.
fn build_job_body(inputs: &JobInputs) -> (String, serde_json::Value) {
    let job_name = format!(
        "{}-upgrader-{}",
        inputs.release,
        sanitize_for_dns(&inputs.target_version, 16)
    );

    let version_flag = inputs
        .chart_version
        .as_deref()
        .map(|v| format!(" --version {v}"))
        .unwrap_or_default();
    // Extra args spliced verbatim before `--namespace`. Empty in
    // production; set via the chart's `runtime.upgrade.extraArgs` for
    // local-dev escape hatches (e.g. `--plain-http` for HTTP-only OCI
    // registries). Leading space is intentional — joins cleanly with
    // version_flag whether it's empty or set.
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
                "alien.dev/target-version": sanitize_for_dns(&inputs.target_version, 63),
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

/// POST a Job to the in-pod Kubernetes API. Returns Ok on 2xx, AlienError
/// otherwise. 409 (already exists) is treated as success — another sync
/// already kicked the Job and idempotency handles the retry.
async fn create_job(namespace: &str, body: &serde_json::Value) -> Result<()> {
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
    let port =
        std::env::var("KUBERNETES_SERVICE_PORT_HTTPS").unwrap_or_else(|_| "443".to_string());

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

    let url = format!("https://{host}:{port}/apis/batch/v1/namespaces/{namespace}/jobs");
    let resp = client
        .post(&url)
        .bearer_auth(token.trim())
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
        // Another sync (or a previous successful upgrade) already created
        // this Job. Idempotency — treat as success.
        info!("Upgrader Job already exists (409); leaving in place");
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
/// Used for Job names + label values derived from semver.
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

    #[test]
    fn job_name_combines_release_and_sanitized_version() {
        let (name, _) = build_job_body(&inputs());
        assert_eq!(name, "alien-upgrader-1-4-0");
    }

    #[test]
    fn job_name_caps_long_version_and_trims_trailing_hyphens() {
        let mut i = inputs();
        // Crafted so that the cap (16) lands on a non-alphanumeric, which
        // must be stripped — otherwise we'd produce an invalid DNS-1123 name.
        i.target_version = "1.4.0-rc.1+build".to_string();
        let (name, _) = build_job_body(&i);
        assert!(
            name.starts_with("alien-upgrader-"),
            "got {name}"
        );
        assert!(
            !name.ends_with('-'),
            "DNS-1123 names must not end in '-': {name}"
        );
        let suffix = name.trim_start_matches("alien-upgrader-");
        assert!(suffix.len() <= 16, "suffix too long: {suffix}");
    }

    #[test]
    fn helm_command_omits_version_flag_when_unset() {
        let mut i = inputs();
        i.chart_version = None;
        let (_, body) = build_job_body(&i);
        let cmd = body["spec"]["template"]["spec"]["containers"][0]["command"][2]
            .as_str()
            .unwrap();
        assert!(
            !cmd.contains("--version"),
            "command unexpectedly contains --version: {cmd}"
        );
        assert!(cmd.contains("helm upgrade \"$RELEASE\""), "got: {cmd}");
        assert!(cmd.contains("--reuse-values"), "got: {cmd}");
        assert!(cmd.contains("--atomic --wait"), "got: {cmd}");
    }

    #[test]
    fn helm_command_includes_version_flag_when_set() {
        let (_, body) = build_job_body(&inputs());
        let cmd = body["spec"]["template"]["spec"]["containers"][0]["command"][2]
            .as_str()
            .unwrap();
        assert!(cmd.contains(" --version 1.4.0"), "got: {cmd}");
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
        assert_eq!(by_name["NAMESPACE"], "alien-agent");
        assert_eq!(by_name["CHART_REF"], "oci://ghcr.io/alien-dev/alien");
        assert_eq!(
            by_name["VALUES_JSON"],
            r#"{"runtime":{"image":{"tag":"1.4.0"}}}"#
        );
    }

    #[test]
    fn job_metadata_uses_upgrader_service_account_and_safe_labels() {
        let (_, body) = build_job_body(&inputs());
        assert_eq!(
            body["spec"]["template"]["spec"]["serviceAccountName"],
            json!("alien-agent-upgrader")
        );
        assert_eq!(
            body["metadata"]["labels"]["alien.dev/target-version"],
            json!("1-4-0")
        );
        assert_eq!(body["spec"]["backoffLimit"], json!(1));
        assert_eq!(body["spec"]["ttlSecondsAfterFinished"], json!(600));
        assert_eq!(
            body["spec"]["template"]["spec"]["restartPolicy"],
            json!("Never")
        );
    }

    #[test]
    fn non_empty_treats_empty_string_as_none() {
        assert_eq!(non_empty(""), None);
        assert_eq!(non_empty("oci://x"), Some("oci://x"));
    }

    #[test]
    fn helm_command_omits_extra_args_when_unset() {
        let (_, body) = build_job_body(&inputs());
        let cmd = body["spec"]["template"]["spec"]["containers"][0]["command"][2]
            .as_str()
            .unwrap();
        // Default fixture's extra_args is empty — should not contain
        // double-spaces or stray flags.
        assert!(!cmd.contains("  --namespace"), "stray double-space: {cmd}");
        assert!(!cmd.contains("--plain-http"), "got: {cmd}");
    }

    #[test]
    fn helm_command_splices_extra_args_before_namespace() {
        let mut i = inputs();
        i.extra_args = "--plain-http".to_string();
        let (_, body) = build_job_body(&i);
        let cmd = body["spec"]["template"]["spec"]["containers"][0]["command"][2]
            .as_str()
            .unwrap();
        // Order matters: extra_args should appear after the chart ref
        // (and any version flag) but before --namespace, so flags that
        // affect chart pull (e.g. --plain-http) are in scope.
        let plain_pos = cmd.find("--plain-http").expect("--plain-http should be present");
        let ns_pos = cmd.find("--namespace").expect("--namespace should be present");
        assert!(plain_pos < ns_pos, "--plain-http must come before --namespace: {cmd}");
    }

    #[test]
    fn helm_command_trims_whitespace_in_extra_args() {
        let mut i = inputs();
        i.extra_args = "   --plain-http   ".to_string();
        let (_, body) = build_job_body(&i);
        let cmd = body["spec"]["template"]["spec"]["containers"][0]["command"][2]
            .as_str()
            .unwrap();
        // The trim should keep exactly one space delimiter on each side.
        assert!(cmd.contains(" --plain-http "), "got: {cmd}");
    }
}
