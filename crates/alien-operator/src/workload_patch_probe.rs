//! `verify-workload-patch-policy`: prove that the cluster refuses a workload
//! change the chart's admission policy forbids, before the Operator gets the
//! `patch` permission on workloads.
//!
//! A new admission policy is not enforced by the API server for a short time
//! after it is created. The chart therefore runs this command as a Helm hook
//! Job under a throwaway ServiceAccount that the policy treats like the
//! Operator's. That account may patch one named Deployment with zero replicas.
//! The command patches a field a restart never touches until the cluster refuses
//! it because of the policy, then checks that a restart patch is admitted. Only
//! then does the hook that gives the Operator its permission run. A cluster that
//! never enforces the policy fails the Job, and the release with it.

use std::{path::PathBuf, time::Duration};

use clap::Parser;
use reqwest::StatusCode;
use serde_json::{json, Value};

const SERVICE_ACCOUNT_DIR: &str = "/var/run/secrets/kubernetes.io/serviceaccount";
const RESTART_ANNOTATION: &str = "kubectl.kubernetes.io/restartedAt";

#[derive(Parser, Debug)]
#[command(
    name = "operator verify-workload-patch-policy",
    about = "Check that the workload admission policy of this installation is enforced"
)]
struct Args {
    /// Namespace of the probe Deployment.
    #[arg(long)]
    namespace: String,
    /// Name of the probe Deployment (zero replicas).
    #[arg(long)]
    deployment: String,
    /// Name of the admission policy that must refuse the change.
    #[arg(long)]
    policy: String,
    /// How long to wait for the cluster to start enforcing the policy.
    #[arg(long, default_value_t = 60)]
    deadline_seconds: u64,
}

/// What one probe request showed.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The change was admitted: the policy is not enforced (yet).
    Admitted,
    /// The change was refused by the named policy.
    RefusedByPolicy,
    /// Anything else (permission not visible yet, other refusal, server error).
    Other(String),
}

fn classify(status: StatusCode, body: &str, policy: &str) -> Outcome {
    if status.is_success() {
        return Outcome::Admitted;
    }
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value["message"].as_str().map(str::to_owned))
        .unwrap_or_default();
    // A refusal by a policy is an `Invalid` status that names the policy.
    if status == StatusCode::UNPROCESSABLE_ENTITY
        && message.contains("ValidatingAdmissionPolicy")
        && message.contains(&format!("'{policy}'"))
    {
        return Outcome::RefusedByPolicy;
    }
    Outcome::Other(format!("{status}: {message}"))
}

struct Probe {
    client: reqwest::Client,
    url: String,
    token: String,
    policy: String,
}

impl Probe {
    async fn patch(&self, body: Value) -> Result<(StatusCode, String), String> {
        let response = self
            .client
            .patch(&self.url)
            .bearer_auth(&self.token)
            .header("content-type", "application/merge-patch+json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|error| format!("request failed: {error}"))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        Ok((status, text))
    }

    /// Patch a field no restart changes until the policy refuses it.
    async fn wait_until_refused(&self, deadline: Duration, pause: Duration) -> Result<(), String> {
        let started = std::time::Instant::now();
        let mut attempt: u32 = 0;
        let mut last = String::from("no response");
        while started.elapsed() < deadline {
            attempt += 1;
            // A different value every time, so an admitted change is a change.
            let body = json!({"spec": {"minReadySeconds": attempt}});
            match self.patch(body).await {
                Ok((status, text)) => match classify(status, &text, &self.policy) {
                    Outcome::RefusedByPolicy => return Ok(()),
                    Outcome::Admitted => last = "the change was admitted".to_owned(),
                    Outcome::Other(detail) => last = detail,
                },
                Err(error) => last = error,
            }
            tokio::time::sleep(pause).await;
        }
        Err(format!(
            "the policy refused no change within {} s (last result: {last})",
            deadline.as_secs()
        ))
    }

    /// A restart must still go through.
    async fn check_restart_admitted(&self) -> Result<(), String> {
        let body = json!({"spec": {"template": {"metadata": {"annotations": {
            RESTART_ANNOTATION: chrono::Utc::now().to_rfc3339()
        }}}}});
        let (status, text) = self.patch(body).await?;
        match classify(status, &text, &self.policy) {
            Outcome::Admitted => Ok(()),
            Outcome::RefusedByPolicy => {
                Err("the policy refused a restart of the probe Deployment".to_owned())
            }
            Outcome::Other(detail) => Err(format!("a restart was not admitted ({detail})")),
        }
    }
}

async fn run(args: Args) -> Result<(), String> {
    let dir = PathBuf::from(SERVICE_ACCOUNT_DIR);
    let token = std::fs::read_to_string(dir.join("token"))
        .map_err(|error| format!("cannot read the service account token: {error}"))?;
    let ca = std::fs::read(dir.join("ca.crt"))
        .map_err(|error| format!("cannot read the cluster certificate: {error}"))?;
    let host = std::env::var("KUBERNETES_SERVICE_HOST")
        .map_err(|_| "KUBERNETES_SERVICE_HOST is not set".to_owned())?;
    let port = std::env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".to_owned());
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host
    };
    let client = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(&ca)
                .map_err(|error| format!("invalid cluster certificate: {error}"))?,
        )
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("cannot build the client: {error}"))?;
    let probe = Probe {
        client,
        url: format!(
            "https://{host}:{port}/apis/apps/v1/namespaces/{}/deployments/{}",
            args.namespace, args.deployment
        ),
        token: token.trim().to_owned(),
        policy: args.policy,
    };
    probe
        .wait_until_refused(
            Duration::from_secs(args.deadline_seconds),
            Duration::from_millis(250),
        )
        .await?;
    probe.check_restart_admitted().await
}

/// Entry point of the subcommand; returns the process exit code.
pub fn main(args: impl IntoIterator<Item = std::ffi::OsString>) -> i32 {
    let args = Args::parse_from(
        std::iter::once(std::ffi::OsString::from(
            "operator verify-workload-patch-policy",
        ))
        .chain(args),
    );
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("verify-workload-patch-policy: {error}");
            return 1;
        }
    };
    match runtime.block_on(run(args)) {
        Ok(()) => {
            println!("verify-workload-patch-policy: the policy is enforced");
            0
        }
        Err(error) => {
            eprintln!("verify-workload-patch-policy: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };

    const POLICY: &str = "op-restart-abcd1234";

    fn refusal(policy: &str) -> String {
        json!({"kind": "Status", "code": 422, "message": format!(
            "Deployment.apps \"probe\" is invalid: : ValidatingAdmissionPolicy '{policy}' with binding '{policy}' denied request: only a restart"
        )})
        .to_string()
    }

    #[test]
    fn only_a_refusal_by_the_named_policy_counts() {
        assert_eq!(
            classify(StatusCode::UNPROCESSABLE_ENTITY, &refusal(POLICY), POLICY),
            Outcome::RefusedByPolicy
        );
        // Another policy, a permission error and a plain validation error do not prove it.
        assert!(matches!(
            classify(StatusCode::UNPROCESSABLE_ENTITY, &refusal("other"), POLICY),
            Outcome::Other(_)
        ));
        assert!(matches!(
            classify(
                StatusCode::FORBIDDEN,
                r#"{"message":"deployments.apps is forbidden"}"#,
                POLICY
            ),
            Outcome::Other(_)
        ));
        assert!(matches!(
            classify(
                StatusCode::UNPROCESSABLE_ENTITY,
                r#"{"message":"spec.replicas: Invalid value"}"#,
                POLICY
            ),
            Outcome::Other(_)
        ));
        assert_eq!(classify(StatusCode::OK, "{}", POLICY), Outcome::Admitted);
    }

    /// A fake API server: admits the first `admit` patches that change
    /// `minReadySeconds`, then refuses them like the policy does; restarts are
    /// always admitted.
    async fn serve(admit: u32, enforce: bool) -> (String, Arc<AtomicU32>) {
        use axum::{extract::State, http::StatusCode as S, routing::patch, Router};
        let seen = Arc::new(AtomicU32::new(0));
        let state = (seen.clone(), admit, enforce);
        async fn handle(
            State((seen, admit, enforce)): State<(Arc<AtomicU32>, u32, bool)>,
            body: String,
        ) -> (S, String) {
            let body: Value = serde_json::from_str(&body).unwrap();
            if body["spec"]["minReadySeconds"].is_null() {
                return (S::OK, "{}".into());
            }
            let n = seen.fetch_add(1, Ordering::SeqCst);
            if enforce && n >= admit {
                (S::UNPROCESSABLE_ENTITY, refusal(POLICY))
            } else {
                (S::OK, "{}".into())
            }
        }
        let app = Router::new()
            .route("/probe", patch(handle))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/probe", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    fn probe(url: String) -> Probe {
        Probe {
            client: reqwest::Client::new(),
            url,
            token: "t".into(),
            policy: POLICY.into(),
        }
    }

    #[tokio::test]
    async fn waits_through_the_window_then_passes() {
        let (url, seen) = serve(3, true).await;
        let probe = probe(url);
        probe
            .wait_until_refused(Duration::from_secs(5), Duration::from_millis(5))
            .await
            .unwrap();
        // Three admitted changes, then the refusal.
        assert_eq!(seen.load(Ordering::SeqCst), 4);
        probe.check_restart_admitted().await.unwrap();
    }

    #[tokio::test]
    async fn a_cluster_that_never_enforces_fails() {
        let (url, _) = serve(0, false).await;
        let error = probe(url)
            .wait_until_refused(Duration::from_millis(200), Duration::from_millis(5))
            .await
            .unwrap_err();
        assert!(error.contains("refused no change"), "{error}");
    }
}
