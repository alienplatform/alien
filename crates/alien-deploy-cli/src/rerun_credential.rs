//! Credentials for rerunning a Kubernetes install.
//!
//! Once the Operator enrolls, the key the install used is retired, so a saved
//! key stops working. A rerun asks the running Operator for a short-lived key
//! instead, through the cluster access the person running the command already
//! has: `kubectl exec` into the Operator's pod. The key is used for this run
//! only. It is never saved, never put in Helm values, and never printed: it
//! travels on the command's stdout and nowhere else.

use std::{fmt, sync::Mutex};

use alien_error::AlienError;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::process::Command;

use crate::{
    deployment_tracking::TrackedDeployment,
    error::{ErrorData, Result},
};

/// Where a rerun finds the Operator, and how to reach the cluster.
#[derive(Debug, Clone, Default)]
pub struct KubeAccess {
    pub release: Option<String>,
    pub namespace: Option<String>,
    pub kubeconfig: Option<String>,
    pub kube_context: Option<String>,
}

/// A key to authenticate with, and where it came from.
pub struct RerunToken {
    pub token: String,
    /// Made by the Operator for this run: a 401 later means a newer rerun took over.
    pub in_cluster: bool,
}

impl fmt::Debug for RerunToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RerunToken")
            .field("token", &"<redacted>")
            .field("in_cluster", &self.in_cluster)
            .finish()
    }
}

/// What `setup-credential` prints. Not `Debug`: it holds a key.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Printed {
    credential: String,
    expires_at: DateTime<Utc>,
}

/// The release and namespace a rerun uses: flags first, then what the tracker
/// saved, then the install defaults.
pub fn helm_target(tracked: &TrackedDeployment, access: &KubeAccess) -> (String, String) {
    let release = access
        .release
        .clone()
        .or_else(|| tracked.helm.as_ref().map(|helm| helm.release.clone()))
        .unwrap_or_else(|| "alien-operator".to_string());
    let namespace = access
        .namespace
        .clone()
        .or_else(|| tracked.helm.as_ref().map(|helm| helm.namespace.clone()))
        .unwrap_or_else(|| format!("alien-{}", dns_label(&tracked.name)));
    (release, namespace)
}

/// The key for a rerun of a tracked deployment: `--token` first, then a key
/// from the in-cluster Operator (Kubernetes installs), then a saved token.
pub async fn resolve(
    explicit: Option<String>,
    tracked: &TrackedDeployment,
    access: &KubeAccess,
    cli_name: &str,
    verb: &str,
) -> Result<RerunToken> {
    if let Some(token) = explicit {
        return Ok(RerunToken {
            token,
            in_cluster: false,
        });
    }
    let mut refusal = None;
    if tracked.platform == "kubernetes" {
        let (release, namespace) = helm_target(tracked, access);
        match fetch(&release, &namespace, access).await {
            Ok(token) => {
                return Ok(RerunToken {
                    token,
                    in_cluster: true,
                })
            }
            Err(reason) => refusal = Some(reason),
        }
    }
    if let Some(token) = tracked.token.clone() {
        return Ok(RerunToken {
            token,
            in_cluster: false,
        });
    }
    Err(setup_token_required(
        &tracked.name,
        cli_name,
        verb,
        refusal.as_deref().unwrap_or("no key is saved for it"),
    ))
}

/// Ask the Operator in `release`/`namespace` for a key. The reason of a failure
/// is one line for the person running the command. It never contains the key:
/// the Operator writes only a fixed code to stderr.
async fn fetch(
    release: &str,
    namespace: &str,
    access: &KubeAccess,
) -> std::result::Result<String, String> {
    let mut command = Command::new("kubectl");
    if let Some(kubeconfig) = &access.kubeconfig {
        // The same variable kubectl reads, so a list of files works too.
        command.env("KUBECONFIG", kubeconfig);
    }
    if let Some(context) = &access.kube_context {
        command.arg("--context").arg(context);
    }
    // `/proc/1/exe` is the Operator's own binary, whatever the image names it.
    command
        .args(["exec", &format!("deploy/{release}"), "-n", namespace])
        .args(["--", "/proc/1/exe", "setup-credential"])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = command.output().await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "kubectl is not installed".to_string()
        } else {
            "kubectl could not run".to_string()
        }
    })?;
    if !output.status.success() {
        return Err(failure_reason(
            &String::from_utf8_lossy(&output.stderr),
            release,
            namespace,
        ));
    }
    let printed: Printed = String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| serde_json::from_str(line).ok())
        .ok_or_else(|| "the Operator returned an unreadable answer".to_string())?;
    if printed.credential.is_empty() || printed.expires_at <= Utc::now() {
        return Err("the Operator returned an expired answer".to_string());
    }
    Ok(printed.credential)
}

/// One line saying why no key came back, from `kubectl exec`'s stderr.
fn failure_reason(stderr: &str, release: &str, namespace: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    let has = |needle: &str| lower.contains(needle);
    if has("setup-credential: not-enrolled") {
        "the Operator has not finished enrolling yet".into()
    } else if has("setup-credential: refused") {
        "the Operator's key was not accepted".into()
    } else if has("setup-credential: unsupported") {
        "the manager cannot issue a key from the Operator".into()
    } else if has("setup-credential: unavailable") {
        "the Operator could not reach the manager".into()
    } else if has("unexpected argument")
        || has("unrecognized subcommand")
        || has("unknown command")
        || has("no such file")
    {
        "the installed Operator is too old to issue one".into()
    } else if has("forbidden") {
        "your Kubernetes access does not allow running a command in the Operator pod".into()
    } else if has("notfound") || has("not found") {
        format!("no Operator named '{release}' is running in namespace '{namespace}'")
    } else if has("unauthorized")
        || has("must be logged in")
        || has("unable to connect")
        || has("connection refused")
        || has("no configuration has been provided")
    {
        "the cluster could not be reached with your Kubernetes access".into()
    } else {
        "the Operator could not be asked".into()
    }
}

/// Nothing to authenticate with. The deployment is untouched.
pub fn setup_token_required(
    name: &str,
    cli_name: &str,
    verb: &str,
    reason: &str,
) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SetupTokenRequired {
        message: format!(
            "Cannot update \"{name}\" without a setup token: {reason}.\n\
             Run this again with the setup token you were given: {cli_name} {verb} --name {} --token-file <path>\n\
             Your deployment keeps running.",
            shell_single_quote(name)
        ),
    })
}

/// A key the Operator made was refused mid-run: minting a newer one revokes
/// this one, so a newer rerun started.
pub fn took_over(name: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::UpdateTookOver {
        message: format!(
            "A newer update of \"{name}\" started and took over; this run stopped. Your deployment keeps running."
        ),
    })
}

/// Whether `error` or a cause is an HTTP 401.
pub fn is_unauthorized(error: &AlienError<ErrorData>) -> bool {
    let mut refused = error.http_status_code == Some(401);
    let mut source = error.source.as_deref();
    while let Some(cause) = source {
        refused |= cause.http_status_code == Some(401);
        source = cause.source.as_deref();
    }
    refused
}

/// Records that a run uses an in-cluster key, and for which deployment, so a
/// 401 later in the run can be explained.
#[derive(Default)]
pub struct InClusterUse(Mutex<Option<String>>);

impl InClusterUse {
    pub fn mark(&self, name: &str) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_string());
    }

    /// Replace a 401 from a run that used an in-cluster key with the takeover message.
    pub fn explain(&self, error: AlienError<ErrorData>) -> AlienError<ErrorData> {
        let name = self.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        match name {
            Some(name) if is_unauthorized(&error) => took_over(&name),
            _ => error,
        }
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// The Kubernetes DNS label the installer derives from a deployment name.
fn dns_label(value: &str) -> String {
    crate::commands::up::sanitize_kubernetes_dns_label(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked(token: Option<&str>) -> TrackedDeployment {
        TrackedDeployment {
            name: "prod".into(),
            deployment_id: "dep_1".into(),
            token: token.map(str::to_string),
            manager_url: "https://m".into(),
            platform: "aws".into(),
            helm: None,
            local: None,
            tracked_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[tokio::test]
    async fn an_explicit_token_wins() {
        let resolved = resolve(
            Some("explicit".into()),
            &tracked(Some("saved")),
            &KubeAccess::default(),
            "cli",
            "deploy",
        )
        .await
        .unwrap();
        assert_eq!(resolved.token, "explicit");
        assert!(!resolved.in_cluster);
    }

    #[tokio::test]
    async fn a_saved_token_is_used_when_nothing_else_is_given() {
        let resolved = resolve(None, &tracked(Some("saved")), &KubeAccess::default(), "cli", "deploy")
            .await
            .unwrap();
        assert_eq!(resolved.token, "saved");
    }

    #[tokio::test]
    async fn no_token_at_all_names_the_setup_token_and_the_rerun_command() {
        let error = resolve(None, &tracked(None), &KubeAccess::default(), "acme-deploy", "deploy")
            .await
            .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("Cannot update \"prod\" without a setup token"), "{text}");
        assert!(text.contains("acme-deploy deploy --name 'prod' --token-file <path>"), "{text}");
        assert!(text.contains("Your deployment keeps running."), "{text}");
        assert!(!text.to_lowercase().contains("alien"), "{text}");
    }

    #[test]
    fn kubectl_failures_become_one_line_reasons() {
        let reason = |stderr: &str| failure_reason(stderr, "rel", "ns");
        assert!(reason("error: unexpected argument 'setup-credential' found").contains("too old"));
        assert!(reason("Error from server (Forbidden): pods is forbidden").contains("Kubernetes access"));
        assert!(reason("Error from server (NotFound): deployments.apps \"rel\" not found")
            .contains("no Operator named 'rel' is running in namespace 'ns'"));
        assert!(reason("setup-credential: not-enrolled").contains("not finished enrolling"));
        assert!(!reason("anything else").contains('\n'));
    }

    #[test]
    fn a_401_after_an_in_cluster_key_says_a_newer_update_took_over() {
        let usage = InClusterUse::default();
        let mut unauthorized = AlienError::new(ErrorData::GenericError {
            message: "refused".into(),
        });
        unauthorized.http_status_code = Some(401);
        let untouched = usage.explain(unauthorized.clone());
        assert!(untouched.to_string().contains("refused"));
        usage.mark("prod");
        let explained = usage.explain(unauthorized);
        assert_eq!(
            explained.message,
            "A newer update of \"prod\" started and took over; this run stopped. Your deployment keeps running."
        );
    }

    #[test]
    fn debug_output_never_holds_the_key() {
        let token = RerunToken {
            token: "ax_setup_secret_value".into(),
            in_cluster: true,
        };
        assert!(!format!("{token:?}").contains("ax_setup_secret_value"));
    }
}
