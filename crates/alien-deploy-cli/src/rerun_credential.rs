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
    /// When an in-cluster key stops working.
    pub expires_at: Option<DateTime<Utc>>,
    /// The Helm release and namespace looked for, when the Operator was not there.
    pub missing_install: Option<MissingInstall>,
}

/// Where the Operator was looked for and not found.
#[derive(Debug, Clone)]
pub struct MissingInstall {
    pub release: String,
    pub namespace: String,
    pub cli_name: String,
    /// The command that was run (`deploy`, `status` or `down`), so the advice names it.
    pub verb: String,
}

impl fmt::Debug for RerunToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RerunToken")
            .field("token", &"<redacted>")
            .field("in_cluster", &self.in_cluster)
            .field("expires_at", &self.expires_at)
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
    resolve_with(explicit, tracked, access, cli_name, verb, |release, namespace, reuse| async move {
        fetch(&release, &namespace, access, reuse).await
    })
    .await
}

/// `status` only reads, so it asks the Operator for the key it last issued
/// instead of a new one: a new key would revoke the one a running deploy, down
/// or Terraform apply is using. `deploy` and `down` always get a new key, so a
/// newer update takes over.
fn reuses_key(verb: &str) -> bool {
    verb == "status"
}

async fn resolve_with<F, Fut>(
    explicit: Option<String>,
    tracked: &TrackedDeployment,
    access: &KubeAccess,
    cli_name: &str,
    verb: &str,
    fetch: F,
) -> Result<RerunToken>
where
    F: FnOnce(String, String, bool) -> Fut,
    Fut: std::future::Future<Output = std::result::Result<(String, DateTime<Utc>), String>>,
{
    if let Some(token) = explicit {
        return Ok(RerunToken {
            token,
            in_cluster: false,
            expires_at: None,
            missing_install: None,
        });
    }
    let mut refusal = None;
    let mut missing_install = None;
    if tracked.platform == "kubernetes" {
        let (release, namespace) = helm_target(tracked, access);
        match fetch(release.clone(), namespace.clone(), reuses_key(verb)).await {
            Ok((token, expires_at)) => {
                return Ok(RerunToken {
                    token,
                    in_cluster: true,
                    expires_at: Some(expires_at),
                    missing_install: None,
                })
            }
            Err(reason) => {
                if reason.starts_with(NO_OPERATOR) {
                    missing_install = Some(MissingInstall {
                        release,
                        namespace,
                        cli_name: cli_name.to_string(),
                        verb: verb.to_string(),
                    });
                }
                refusal = Some(reason)
            }
        }
    }
    if let Some(token) = tracked.token.clone() {
        return Ok(RerunToken {
            token,
            in_cluster: false,
            expires_at: None,
            missing_install,
        });
    }
    if refusal.as_deref() == Some(REMOVING) {
        return Err(AlienError::new(ErrorData::SetupTokenRequired {
            message: format!(
                "Cannot update \"{}\": it is being removed. Nothing was changed.",
                tracked.name
            ),
        }));
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
    reuse: bool,
) -> std::result::Result<(String, DateTime<Utc>), String> {
    let first = exec_once(release, namespace, access, reuse).await;
    // An Operator from before `--reuse` answers with a usage error: ask it
    // again without the flag, which mints as before.
    match first {
        Err(ExecFailure::UsageError) if reuse => exec_once(release, namespace, access, false)
            .await
            .map_err(|failure| failure.reason(release, namespace)),
        other => other.map_err(|failure| failure.reason(release, namespace)),
    }
}

enum ExecFailure {
    /// The command did not accept its arguments.
    UsageError,
    Reason(String),
    Stderr(String),
}

impl ExecFailure {
    fn reason(self, release: &str, namespace: &str) -> String {
        match self {
            Self::UsageError => "the installed Operator is too old to issue one".into(),
            Self::Reason(reason) => reason,
            Self::Stderr(stderr) => failure_reason(&stderr, release, namespace),
        }
    }
}

/// The arguments of `kubectl exec` for the Operator's `setup-credential`.
fn exec_args(release: &str, namespace: &str, reuse: bool) -> Vec<String> {
    let mut args = vec![
        "exec".to_string(),
        format!("deploy/{release}"),
        "-n".into(),
        namespace.into(),
        "--".into(),
        // `/proc/1/exe` is the Operator's own binary, whatever the image names it.
        "/proc/1/exe".into(),
        "setup-credential".into(),
    ];
    if reuse {
        args.push("--reuse".into());
    }
    args
}

async fn exec_once(
    release: &str,
    namespace: &str,
    access: &KubeAccess,
    reuse: bool,
) -> std::result::Result<(String, DateTime<Utc>), ExecFailure> {
    let mut command = Command::new("kubectl");
    if let Some(kubeconfig) = &access.kubeconfig {
        // The same variable kubectl reads, so a list of files works too.
        command.env("KUBECONFIG", kubeconfig);
    }
    if let Some(context) = &access.kube_context {
        command.arg("--context").arg(context);
    }
    command
        .args(exec_args(release, namespace, reuse))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = command.output().await.map_err(|error| {
        ExecFailure::Reason(if error.kind() == std::io::ErrorKind::NotFound {
            "kubectl is not installed".to_string()
        } else {
            "kubectl could not run".to_string()
        })
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if reuse && stderr.to_ascii_lowercase().contains("unexpected argument '--reuse'") {
            return Err(ExecFailure::UsageError);
        }
        return Err(ExecFailure::Stderr(stderr));
    }
    let printed: Printed = String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| serde_json::from_str(line).ok())
        .ok_or_else(|| ExecFailure::Reason("the Operator returned an unreadable answer".into()))?;
    if printed.credential.is_empty() || printed.expires_at <= Utc::now() {
        return Err(ExecFailure::Reason(
            "the Operator returned an expired answer".into(),
        ));
    }
    Ok((printed.credential, printed.expires_at))
}

const REMOVING: &str = "the deployment is being removed";
const NO_OPERATOR: &str = "no Operator named";

/// One line saying why no key came back, from `kubectl exec`'s stderr.
fn failure_reason(stderr: &str, release: &str, namespace: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    let has = |needle: &str| lower.contains(needle);
    if has("setup-credential: not-enrolled") {
        "the Operator has not finished enrolling yet".into()
    } else if has("setup-credential: refused") {
        "the Operator's key was not accepted".into()
    } else if has("setup-credential: removing") {
        REMOVING.into()
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
        format!("{NO_OPERATOR} '{release}' is running in namespace '{namespace}'")
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

/// The 15-minute key ran out before the run finished.
pub fn key_expired(name: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::UpdateTookOver {
        message: format!(
            "The update key for \"{name}\" expired after 15 minutes; this run stopped. Your deployment keeps running. Run it again."
        ),
    })
}

/// Whether `error` or a cause is an HTTP 401.
pub fn is_unauthorized(error: &AlienError<ErrorData>) -> bool {
    has_status(error, 401)
}

/// What a run learned about its credential, so an error later in the run can
/// be explained: an in-cluster key (a 401 means a newer run took over or the
/// key expired), or no installation at the place it looked (a refused saved
/// key means the wrong release or namespace).
#[derive(Default)]
pub struct InClusterUse(Mutex<UseState>);

#[derive(Default)]
struct UseState {
    in_cluster: Option<(String, Option<DateTime<Utc>>)>,
    missing: Option<(String, MissingInstall)>,
}

impl InClusterUse {
    pub fn mark(&self, name: &str, expires_at: Option<DateTime<Utc>>) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).in_cluster =
            Some((name.to_string(), expires_at));
    }

    /// Record what `resolve` found for `name`.
    pub fn note(&self, name: &str, rerun: &RerunToken) {
        if rerun.in_cluster {
            self.mark(name, rerun.expires_at);
        }
        if let Some(missing) = &rerun.missing_install {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).missing =
                Some((name.to_string(), missing.clone()));
        }
    }

    /// Replace a refusal with what it means for this run.
    pub fn explain(&self, error: AlienError<ErrorData>) -> AlienError<ErrorData> {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((name, missing)) = &state.missing {
            if has_status(&error, 401) || has_status(&error, 409) {
                return no_installation(name, missing);
            }
        }
        match &state.in_cluster {
            Some((name, Some(expires_at)))
                if is_unauthorized(&error) && Utc::now() >= *expires_at =>
            {
                key_expired(name)
            }
            Some((name, _)) if is_unauthorized(&error) => took_over(name),
            _ => error,
        }
    }
}

/// The Operator was not where the CLI looked, and the saved key was refused.
fn no_installation(name: &str, missing: &MissingInstall) -> AlienError<ErrorData> {
    let MissingInstall {
        release,
        namespace,
        cli_name,
        verb,
    } = missing;
    AlienError::new(ErrorData::SetupTokenRequired {
        message: format!(
            "Cannot update \"{name}\": no installation found as Helm release \"{release}\" in namespace \"{namespace}\". \
             If it was installed under another release or namespace, run this again with --helm-release <release> --namespace <namespace>. \
             Or use the setup token you were given: {cli_name} {verb} --name {} --token-file <path>. \
             Your deployment keeps running.",
            shell_single_quote(name)
        ),
    })
}

/// Whether `error` or a cause has this HTTP status.
fn has_status(error: &AlienError<ErrorData>, status: u16) -> bool {
    let mut found = error.http_status_code == Some(status);
    let mut source = error.source.as_deref();
    while let Some(cause) = source {
        found |= cause.http_status_code == Some(status);
        source = cause.source.as_deref();
    }
    found
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

    fn fetched(
        calls: &std::sync::Mutex<Vec<bool>>,
        result: std::result::Result<(String, DateTime<Utc>), String>,
    ) -> impl FnOnce(String, String, bool) -> std::future::Ready<
        std::result::Result<(String, DateTime<Utc>), String>,
    > + '_ {
        move |_, _, reuse| {
            calls.lock().unwrap().push(reuse);
            std::future::ready(result)
        }
    }

    fn kubernetes(token: Option<&str>) -> TrackedDeployment {
        TrackedDeployment {
            platform: "kubernetes".into(),
            ..tracked(token)
        }
    }

    #[tokio::test]
    async fn status_asks_for_the_last_key_and_deploy_and_down_ask_for_a_new_one() {
        for (verb, reuse) in [("status", true), ("deploy", false), ("down", false)] {
            let calls = std::sync::Mutex::new(Vec::new());
            let key = Ok(("k".to_string(), Utc::now() + chrono::Duration::minutes(10)));
            let resolved = resolve_with(
                None,
                &kubernetes(None),
                &KubeAccess::default(),
                "cli",
                verb,
                fetched(&calls, key),
            )
            .await
            .unwrap();
            assert!(resolved.in_cluster);
            assert_eq!(*calls.lock().unwrap(), vec![reuse], "{verb}");
        }
    }

    #[test]
    fn the_exec_command_carries_reuse_only_when_asked() {
        assert!(exec_args("r", "n", true).ends_with(&["setup-credential".into(), "--reuse".into()]));
        assert_eq!(exec_args("r", "n", false).last().unwrap(), "setup-credential");
    }

    /// A first install that failed before the Operator enrolled is tracked with no key. A plain
    /// rerun gets the setup-token message, not a raw error.
    #[tokio::test]
    async fn a_plain_rerun_after_a_failed_first_install_asks_for_the_setup_token() {
        for stderr in [
            "Error from server (NotFound): deployments.apps \"alien-operator\" not found",
            "error: unable to upgrade connection: container not found (\"operator\")",
            "setup-credential: not-enrolled",
        ] {
            let calls = std::sync::Mutex::new(Vec::new());
            let reason = failure_reason(stderr, "alien-operator", "alien-prod");
            let error = resolve_with(
                None,
                &kubernetes(None),
                &KubeAccess::default(),
                "acme-deploy",
                "deploy",
                fetched(&calls, Err(reason)),
            )
            .await
            .unwrap_err();
            let text = error.to_string();
            assert!(text.contains("Cannot update \"prod\" without a setup token: "), "{text}");
            assert!(
                text.contains("Run this again with the setup token you were given: acme-deploy deploy --name 'prod' --token-file <path>"),
                "{text}"
            );
            assert!(text.contains("Your deployment keeps running."), "{text}");
        }
    }

    #[tokio::test]
    async fn a_saved_token_is_used_when_nothing_else_is_given() {
        let resolved = resolve(
            None,
            &tracked(Some("saved")),
            &KubeAccess::default(),
            "cli",
            "deploy",
        )
        .await
        .unwrap();
        assert_eq!(resolved.token, "saved");
    }

    #[tokio::test]
    async fn no_token_at_all_names_the_setup_token_and_the_rerun_command() {
        let error = resolve(
            None,
            &tracked(None),
            &KubeAccess::default(),
            "acme-deploy",
            "deploy",
        )
        .await
        .unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("Cannot update \"prod\" without a setup token"),
            "{text}"
        );
        assert!(
            text.contains("acme-deploy deploy --name 'prod' --token-file <path>"),
            "{text}"
        );
        assert!(text.contains("Your deployment keeps running."), "{text}");
        assert!(!text.to_lowercase().contains("alien"), "{text}");
    }

    #[test]
    fn kubectl_failures_become_one_line_reasons() {
        let reason = |stderr: &str| failure_reason(stderr, "rel", "ns");
        assert!(reason("error: unexpected argument 'setup-credential' found").contains("too old"));
        assert!(reason("Error from server (Forbidden): pods is forbidden")
            .contains("Kubernetes access"));
        assert!(
            reason("Error from server (NotFound): deployments.apps \"rel\" not found")
                .contains("no Operator named 'rel' is running in namespace 'ns'")
        );
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
        usage.mark("prod", None);
        let explained = usage.explain(unauthorized);
        assert_eq!(
            explained.message,
            "A newer update of \"prod\" started and took over; this run stopped. Your deployment keeps running."
        );
    }

    #[test]
    fn a_retired_saved_key_keeps_the_servers_message() {
        // An old tracker file holds a key the platform retired when the Operator enrolled. The
        // server answers 409 with its own explanation; it is not a takeover and is shown as is.
        let mut retired = AlienError::new(ErrorData::GenericError {
            message: "This deployment key was retired. Use the setup token.".into(),
        });
        retired.http_status_code = Some(409);
        retired.code = "DEPLOYMENT_KEY_RETIRED".into();
        for usage in [InClusterUse::default(), {
            let usage = InClusterUse::default();
            usage.mark("prod", None);
            usage
        }] {
            let shown = usage.explain(retired.clone());
            assert_eq!(shown.code, "DEPLOYMENT_KEY_RETIRED");
            assert!(shown.to_string().contains("was retired"), "{shown}");
            assert!(!shown.to_string().contains("took over"));
        }
    }

    #[test]
    fn a_401_after_the_key_expired_says_so_not_a_takeover() {
        let usage = InClusterUse::default();
        usage.mark("prod", Some(Utc::now() - chrono::Duration::seconds(1)));
        let mut unauthorized = AlienError::new(ErrorData::GenericError {
            message: "refused".into(),
        });
        unauthorized.http_status_code = Some(401);
        let shown = usage.explain(unauthorized).message;
        assert!(shown.contains("expired after 15 minutes"), "{shown}");
        assert!(!shown.contains("took over"));
    }

    #[test]
    fn a_refused_saved_key_after_no_installation_names_what_was_looked_for() {
        let missing_for = |verb: &str| RerunToken {
            token: "saved".into(),
            in_cluster: false,
            expires_at: None,
            missing_install: Some(MissingInstall {
                release: "alien-operator".into(),
                namespace: "alien-prod".into(),
                cli_name: "acme-deploy".into(),
                verb: verb.into(),
            }),
        };
        let missing = missing_for("deploy");
        // The advice names the command that was run, not always `deploy`.
        for verb in ["deploy", "status", "down"] {
            let usage = InClusterUse::default();
            usage.note("prod", &missing_for(verb));
            let mut refused = AlienError::new(ErrorData::GenericError {
                message: "Invalid token".into(),
            });
            refused.http_status_code = Some(401);
            let shown = usage.explain(refused).to_string();
            assert!(
                shown.contains(&format!("acme-deploy {verb} --name 'prod' --token-file <path>")),
                "{shown}"
            );
        }
        for status in [401, 409] {
            let usage = InClusterUse::default();
            usage.note("prod", &missing);
            let mut refused = AlienError::new(ErrorData::GenericError {
                message: "Invalid token".into(),
            });
            refused.http_status_code = Some(status);
            let shown = usage.explain(refused).to_string();
            assert!(
                shown.contains("Cannot update \"prod\": no installation found as Helm release \"alien-operator\" in namespace \"alien-prod\"."),
                "{shown}"
            );
            assert!(
                shown.contains("--helm-release <release> --namespace <namespace>"),
                "{shown}"
            );
            assert!(shown.contains("Your deployment keeps running."), "{shown}");
            assert!(!shown.to_lowercase().contains("alien\""), "{shown}");
        }
        // Other failures, and a run that found the installation, are not rewritten.
        let usage = InClusterUse::default();
        usage.note("prod", &missing);
        let mut other = AlienError::new(ErrorData::GenericError {
            message: "boom".into(),
        });
        other.http_status_code = Some(500);
        assert!(usage.explain(other).to_string().contains("boom"));
    }

    /// `status` and `down` call the manager through the generated client. A 401 from it must
    /// reach `explain` with its status, or a takeover or an expired key shows a generic error.
    #[tokio::test]
    async fn a_manager_401_keeps_its_status_through_the_calls_status_and_down_make() {
        use alien_error::Context as _;
        use alien_manager_api::SdkResultExt as _;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = vec![0; 8192];
                let _ = stream.read(&mut buffer).await;
                let body = r#"{"code":"UNAUTHORIZED","message":"Invalid token","retryable":false,"internal":false}"#;
                let response = format!(
                    "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let client = crate::commands::up::create_manager_client(
            "ax_key",
            &format!("http://{address}"),
        )
        .unwrap();
        let usage = InClusterUse::default();
        usage.mark("prod", None);

        // status: fetch the deployment.
        let status_error = client
            .get_deployment()
            .id("dep_1")
            .send()
            .await
            .into_sdk_error()
            .await
            .context(ErrorData::ManagementUnavailable)
            .unwrap_err();
        assert!(usage
            .explain(status_error)
            .message
            .contains("took over"));

        // down: request the deletion.
        let down_error = client
            .delete_deployment()
            .id("dep_1")
            .body(alien_manager_api::types::DeleteDeploymentRequest {
                action: alien_manager_api::types::DeleteDeploymentAction::Forget,
            })
            .send()
            .await
            .into_sdk_error()
            .await
            .context(ErrorData::DeploymentFailed {
                operation: "request deletion".to_string(),
            })
            .unwrap_err();
        assert!(usage.explain(down_error).message.contains("took over"));
    }

    #[test]
    fn no_operator_in_the_namespace_is_remembered_as_a_missing_installation() {
        let reason = failure_reason(
            "Error from server (NotFound): deployments.apps \"alien-operator\" not found",
            "alien-operator",
            "alien-prod",
        );
        assert!(reason.starts_with(NO_OPERATOR), "{reason}");
    }

    #[test]
    fn a_removing_deployment_gets_its_own_reason() {
        assert_eq!(
            failure_reason("setup-credential: removing", "r", "n"),
            REMOVING
        );
    }

    #[test]
    fn debug_output_never_holds_the_key() {
        let token = RerunToken {
            token: "ax_setup_secret_value".into(),
            in_cluster: true,
            expires_at: None,
            missing_install: None,
        };
        assert!(!format!("{token:?}").contains("ax_setup_secret_value"));
    }
}
