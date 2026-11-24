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
//!
//! A cluster can have several API servers, and one that already enforces the
//! policy says nothing about another that has not loaded it yet. The command
//! counts the serving API servers from the addresses of the `kubernetes`
//! Service's EndpointSlice in `default` (each API server keeps its address
//! there), sends every request on a fresh connection, and needs at least three
//! times that many refusals in a row, spread over a stabilization window,
//! before it passes. When the count cannot be read (forbidden, or not there)
//! it does not assume one server: it uses a fixed conservative sample instead
//! (at least 10 refusals over at least 10 seconds) and says so in its log. Requests go
//! through the cluster's service address, which spreads new connections over
//! the API servers, so this makes it very likely, not certain, that every one
//! of them enforces the policy; the remaining limit is stated in the chart's
//! documentation.

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
    /// The refusals in a row must span at least this long.
    #[arg(long, default_value_t = 5)]
    stabilization_seconds: u64,
}

/// Refusals needed per API server.
const REFUSALS_PER_API_SERVER: usize = 3;
/// Sample used when the API servers cannot be counted.
const FALLBACK_REFUSALS: usize = 10;
const FALLBACK_STABILIZATION: Duration = Duration::from_secs(10);

/// Count the API servers serving now from the `kubernetes` EndpointSlice: one
/// per address of an endpoint that is not marked not ready. `None` when the
/// body is not an EndpointSlice or lists no address.
fn count_api_servers(body: &str) -> Option<usize> {
    let slice: Value = serde_json::from_str(body).ok()?;
    let endpoints = slice["endpoints"].as_array()?;
    let serving: usize = endpoints
        .iter()
        .filter(|endpoint| endpoint["conditions"]["ready"] != json!(false))
        .map(|endpoint| endpoint["addresses"].as_array().map_or(0, Vec::len))
        .sum();
    (serving > 0).then_some(serving)
}

/// How many refusals in a row, over how long, and what to log about it.
fn sampling(count: Result<usize, String>, stabilization: Duration) -> (usize, Duration, String) {
    match count {
        Ok(count) => (
            REFUSALS_PER_API_SERVER * count,
            stabilization,
            format!("{count} API server(s) serving"),
        ),
        Err(reason) => (
            FALLBACK_REFUSALS,
            stabilization.max(FALLBACK_STABILIZATION),
            format!(
                "cannot count the API servers ({reason}); using the fixed sample of {FALLBACK_REFUSALS} refusals over {} s",
                stabilization.max(FALLBACK_STABILIZATION).as_secs()
            ),
        ),
    }
}

/// An error with its causes, so a failed request says why.
fn describe(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
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
    /// Trust anchor for the cluster certificate (`None` for a test server).
    ca: Option<reqwest::Certificate>,
    url: String,
    token: String,
    policy: String,
    /// Refusals in a row required (3 per API server).
    required_refusals: usize,
    /// The refusals in a row must span at least this long.
    stabilization: Duration,
}

impl Probe {
    /// A client that opens a new connection for every request, so that the
    /// cluster's service address can pick another API server each time.
    fn fresh_client(&self) -> Result<reqwest::Client, String> {
        let mut builder = reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .timeout(Duration::from_secs(10));
        if let Some(ca) = &self.ca {
            builder = builder.add_root_certificate(ca.clone());
        }
        builder
            .build()
            .map_err(|error| format!("cannot build the client: {error}"))
    }

    async fn patch(&self, body: Value) -> Result<(StatusCode, String), String> {
        let response = self
            .fresh_client()?
            .patch(&self.url)
            .header("connection", "close")
            .bearer_auth(&self.token)
            .header("content-type", "application/merge-patch+json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|error| format!("request failed: {}", describe(&error)))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        Ok((status, text))
    }

    /// The serving API servers, from the `kubernetes` Service's EndpointSlice.
    async fn read_api_server_count(&self, base: &str) -> Result<usize, String> {
        let response = self
            .fresh_client()?
            .get(format!(
                "{base}/apis/discovery.k8s.io/v1/namespaces/default/endpointslices/kubernetes"
            ))
            .bearer_auth(&self.token)
            .header("connection", "close")
            .send()
            .await
            .map_err(|error| format!("request failed: {}", describe(&error)))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("the read was refused with {status}"));
        }
        count_api_servers(&text).ok_or_else(|| "the answer lists no API server address".to_owned())
    }

    /// Patch a field no restart changes until the policy has refused it
    /// `required_refusals` times in a row on fresh connections, over at least
    /// the stabilization window. Any other answer starts the count again.
    async fn wait_until_refused(&self, deadline: Duration, pause: Duration) -> Result<(), String> {
        let started = std::time::Instant::now();
        let mut attempt: u32 = 0;
        let mut streak: usize = 0;
        let mut streak_started = started;
        let mut last = String::from("no response");
        while started.elapsed() < deadline {
            attempt += 1;
            // A different value every time, so an admitted change is a change.
            let body = json!({"spec": {"minReadySeconds": attempt}});
            let refused = match self.patch(body).await {
                Ok((status, text)) => match classify(status, &text, &self.policy) {
                    Outcome::RefusedByPolicy => true,
                    Outcome::Admitted => {
                        last = "the change was admitted".to_owned();
                        false
                    }
                    Outcome::Other(detail) => {
                        last = detail;
                        false
                    }
                },
                Err(error) => {
                    last = error;
                    false
                }
            };
            if refused {
                if streak == 0 {
                    streak_started = std::time::Instant::now();
                }
                streak += 1;
                if streak >= self.required_refusals
                    && streak_started.elapsed() >= self.stabilization
                {
                    return Ok(());
                }
            } else {
                streak = 0;
            }
            tokio::time::sleep(pause).await;
        }
        Err(format!(
            "the policy did not refuse {} changes in a row over {} s within {} s (last result: {last})",
            self.required_refusals,
            self.stabilization.as_secs(),
            deadline.as_secs()
        ))
    }

    /// A restart must still go through, on every one of as many fresh
    /// connections as the refusals used.
    async fn check_restart_admitted(&self) -> Result<(), String> {
        for _ in 0..self.required_refusals {
            let body = json!({"spec": {"template": {"metadata": {"annotations": {
                RESTART_ANNOTATION: chrono::Utc::now().to_rfc3339()
            }}}}});
            let (status, text) = self.patch(body).await?;
            match classify(status, &text, &self.policy) {
                Outcome::Admitted => {}
                Outcome::RefusedByPolicy => {
                    return Err("the policy refused a restart of the probe Deployment".to_owned())
                }
                Outcome::Other(detail) => {
                    return Err(format!("a restart was not admitted ({detail})"))
                }
            }
        }
        Ok(())
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
    let ca = reqwest::Certificate::from_pem(&ca)
        .map_err(|error| format!("invalid cluster certificate: {error}"))?;
    let base = format!("https://{host}:{port}");
    let mut probe = Probe {
        ca: Some(ca),
        url: format!(
            "{base}/apis/apps/v1/namespaces/{}/deployments/{}",
            args.namespace, args.deployment
        ),
        token: token.trim().to_owned(),
        policy: args.policy,
        required_refusals: REFUSALS_PER_API_SERVER,
        stabilization: Duration::from_secs(args.stabilization_seconds),
    };
    let (required, stabilization, note) = sampling(
        probe.read_api_server_count(&base).await,
        probe.stabilization,
    );
    println!("verify-workload-patch-policy: {note}");
    probe.required_refusals = required;
    probe.stabilization = stabilization;
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
        Arc, Mutex,
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

    /// How a fake API server answers a change of `minReadySeconds`.
    #[derive(Clone, Copy)]
    enum Behavior {
        /// Admits the first `n` changes, then refuses every one.
        EnforceAfter(u32),
        /// Never refuses.
        Never,
        /// Two API servers behind one address: connections alternate between
        /// one that enforces and one that has not loaded the policy yet. After
        /// `lag` connections the second one enforces too.
        Alternating { lag: usize },
    }

    #[derive(Clone)]
    struct Fake {
        changes: Arc<AtomicU32>,
        /// Remote port of every connection, in the order first seen.
        connections: Arc<Mutex<Vec<u16>>>,
        /// Number of requests each connection carried.
        requests: Arc<Mutex<Vec<u32>>>,
        behavior: Behavior,
        restart_refused: bool,
    }

    /// A fake API server that answers per connection.
    async fn serve(behavior: Behavior) -> (String, Fake) {
        serve_with(behavior, false).await
    }

    async fn serve_with(behavior: Behavior, restart_refused: bool) -> (String, Fake) {
        use axum::{
            extract::{ConnectInfo, State},
            http::StatusCode as S,
            routing::patch,
            Router,
        };
        use std::net::SocketAddr;
        let fake = Fake {
            changes: Arc::new(AtomicU32::new(0)),
            connections: Arc::new(Mutex::new(Vec::new())),
            requests: Arc::new(Mutex::new(Vec::new())),
            behavior,
            restart_refused,
        };
        async fn handle(
            State(fake): State<Fake>,
            ConnectInfo(peer): ConnectInfo<SocketAddr>,
            body: String,
        ) -> (S, String) {
            let index = {
                let mut seen = fake.connections.lock().unwrap();
                let mut requests = fake.requests.lock().unwrap();
                match seen.iter().position(|port| *port == peer.port()) {
                    Some(index) => {
                        requests[index] += 1;
                        index
                    }
                    None => {
                        seen.push(peer.port());
                        requests.push(1);
                        seen.len() - 1
                    }
                }
            };
            let body: Value = serde_json::from_str(&body).unwrap();
            if body["spec"]["minReadySeconds"].is_null() {
                // A restart: the lagging server admits it, the other too.
                return if fake.restart_refused && index % 2 == 1 {
                    (S::UNPROCESSABLE_ENTITY, refusal(POLICY))
                } else {
                    (S::OK, "{}".into())
                };
            }
            let n = fake.changes.fetch_add(1, Ordering::SeqCst);
            let enforced = match fake.behavior {
                Behavior::EnforceAfter(admit) => n >= admit,
                Behavior::Never => false,
                Behavior::Alternating { lag } => index % 2 == 0 || index >= lag,
            };
            if enforced {
                (S::UNPROCESSABLE_ENTITY, refusal(POLICY))
            } else {
                (S::OK, "{}".into())
            }
        }
        let app = Router::new()
            .route("/probe", patch(handle))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/probe", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap()
        });
        (url, fake)
    }

    fn probe(url: String, required_refusals: usize, stabilization: Duration) -> Probe {
        Probe {
            ca: None,
            url,
            token: "t".into(),
            policy: POLICY.into(),
            required_refusals,
            stabilization,
        }
    }

    #[tokio::test]
    async fn waits_through_the_window_then_passes() {
        let (url, fake) = serve(Behavior::EnforceAfter(3)).await;
        let probe = probe(url, 3, Duration::from_millis(30));
        probe
            .wait_until_refused(Duration::from_secs(5), Duration::from_millis(5))
            .await
            .unwrap();
        // Three admitted changes, then at least the three refusals in a row.
        assert!(fake.changes.load(Ordering::SeqCst) >= 6);
        probe.check_restart_admitted().await.unwrap();
    }

    #[tokio::test]
    async fn every_request_uses_a_fresh_connection() {
        let (url, fake) = serve(Behavior::EnforceAfter(0)).await;
        let probe = probe(url, 3, Duration::from_millis(30));
        probe
            .wait_until_refused(Duration::from_secs(5), Duration::from_millis(5))
            .await
            .unwrap();
        probe.check_restart_admitted().await.unwrap();
        let requests = fake.requests.lock().unwrap();
        assert!(requests.len() >= 6, "{requests:?}");
        assert!(
            requests.iter().all(|count| *count == 1),
            "a connection was reused: {requests:?}"
        );
    }

    #[tokio::test]
    async fn one_refusing_server_is_not_enough_when_another_has_not_loaded_the_policy() {
        // Two servers behind one address; the second never loads the policy.
        let (url, fake) = serve(Behavior::Alternating { lag: usize::MAX }).await;
        let error = probe(url, 6, Duration::from_millis(30))
            .wait_until_refused(Duration::from_millis(600), Duration::from_millis(5))
            .await
            .unwrap_err();
        assert!(
            error.contains("did not refuse 6 changes in a row"),
            "{error}"
        );
        assert!(fake.connections.lock().unwrap().len() >= 6);
    }

    #[tokio::test]
    async fn passes_once_the_lagging_server_enforces_too() {
        let (url, _) = serve(Behavior::Alternating { lag: 10 }).await;
        probe(url, 6, Duration::from_millis(30))
            .wait_until_refused(Duration::from_secs(5), Duration::from_millis(5))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_refusals_must_span_the_stabilization_window() {
        let (url, _) = serve(Behavior::EnforceAfter(0)).await;
        let started = std::time::Instant::now();
        probe(url, 3, Duration::from_millis(400))
            .wait_until_refused(Duration::from_secs(5), Duration::from_millis(5))
            .await
            .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(400));
    }

    #[tokio::test]
    async fn a_restart_refused_on_one_server_fails() {
        // Every second connection reaches a server that refuses restarts.
        let (url, _) = serve_with(Behavior::EnforceAfter(0), true).await;
        let error = probe(url, 3, Duration::from_millis(30))
            .check_restart_admitted()
            .await
            .unwrap_err();
        assert!(error.contains("refused a restart"), "{error}");
    }

    #[tokio::test]
    async fn a_cluster_that_never_enforces_fails() {
        let (url, _) = serve(Behavior::Never).await;
        let error = probe(url, 3, Duration::from_millis(30))
            .wait_until_refused(Duration::from_millis(200), Duration::from_millis(5))
            .await
            .unwrap_err();
        assert!(error.contains("did not refuse"), "{error}");
    }

    fn slice(endpoints: Value) -> String {
        json!({"kind": "EndpointSlice", "endpoints": endpoints}).to_string()
    }

    #[test]
    fn counts_the_serving_api_servers_from_the_endpoint_slice() {
        let body = slice(json!([
            {"addresses": ["10.0.0.1"], "conditions": {"ready": true}},
            {"addresses": ["10.0.0.2"]},
            {"addresses": ["10.0.0.3"], "conditions": {"ready": false}},
        ]));
        assert_eq!(count_api_servers(&body), Some(2));
        assert_eq!(count_api_servers(&slice(json!([]))), None);
        assert_eq!(count_api_servers("not json"), None);
        assert_eq!(count_api_servers(r#"{"kind":"Status"}"#), None);
    }

    #[test]
    fn a_known_count_scales_the_sample_and_an_unknown_one_uses_the_fixed_sample() {
        let five = Duration::from_secs(5);
        let (required, window, note) = sampling(Ok(2), five);
        assert_eq!((required, window), (6, five));
        assert!(note.contains("2 API server"), "{note}");
        let (required, window, note) = sampling(Err("forbidden".into()), five);
        assert_eq!((required, window), (10, Duration::from_secs(10)));
        assert!(note.contains("fixed sample"), "{note}");
        // A longer configured window is kept.
        let (_, window, _) = sampling(Err("x".into()), Duration::from_secs(20));
        assert_eq!(window, Duration::from_secs(20));
    }

    /// A fake API server that answers the EndpointSlice read with `status`.
    async fn serve_slice(status: u16, body: String) -> String {
        use axum::{http::StatusCode as S, routing::get, Router};
        let app = Router::new().route(
            "/apis/discovery.k8s.io/v1/namespaces/default/endpointslices/kubernetes",
            get(move || {
                let body = body.clone();
                async move { (S::from_u16(status).unwrap(), body) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    #[tokio::test]
    async fn reads_the_count_and_falls_back_when_the_read_is_forbidden() {
        let base = serve_slice(
            200,
            slice(json!([
                {"addresses": ["10.0.0.1"]},
                {"addresses": ["10.0.0.2"]},
            ])),
        )
        .await;
        let probe = probe(base.clone(), 3, Duration::from_millis(30));
        assert_eq!(probe.read_api_server_count(&base).await, Ok(2));

        let forbidden = serve_slice(403, r#"{"kind":"Status","code":403}"#.into()).await;
        let error = probe.read_api_server_count(&forbidden).await.unwrap_err();
        assert!(error.contains("403"), "{error}");
        let (required, window, note) = sampling(Err(error), Duration::from_secs(5));
        assert_eq!((required, window), (10, Duration::from_secs(10)));
        assert!(note.contains("fixed sample"), "{note}");

        let missing = serve_slice(404, "{}".into()).await;
        assert!(probe.read_api_server_count(&missing).await.is_err());
    }
}
