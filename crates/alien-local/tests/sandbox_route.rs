//! The local sandbox lifecycle over the real loopback transport.
//!
//! This is the point of building Local first: the same shape the cloud backends carry runs
//! here against a real daemon, over real HTTP, with real auth — no in-process shortcut.
//!
//! `cargo test -p alien-local --test sandbox_route -- --ignored --test-threads=1`

use std::sync::Arc;

use alien_local::{LocalSandboxManager, SandboxEgressMode, SandboxRoute, SandboxSessionConfig};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;
use tempfile::TempDir;

const IMAGE: &str = "alpine:3.20";
const SANDBOX: &str = "route-a";

struct Harness {
    route: SandboxRoute,
    token: String,
    client: reqwest::Client,
    _dir: TempDir,
}

impl Harness {
    async fn start() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let manager = Arc::new(
            LocalSandboxManager::new(dir.path().to_path_buf()).expect("Docker must be reachable"),
        );
        manager.reap(SANDBOX).await.expect("clean slate");
        // The route registry is process-global, so a prior test in this binary leaves an entry
        // whose token file lived in a TempDir that is now gone. Evict it the way teardown would,
        // or `ensure` hands back that stale route and the token read below fails.
        SandboxRoute::remove(SANDBOX).await;

        let template = SandboxSessionConfig {
            image: IMAGE.to_string(),
            cpu_cores: 0.5,
            memory_bytes: 268_435_456,
            pids_limit: Some(64),
            scratch_bytes: 16_777_216,
            egress: SandboxEgressMode::Allow,
            preview_ports: vec![8080],
            env: Default::default(),
        };

        let route = SandboxRoute::ensure(manager, SANDBOX, template)
            .await
            .expect("route binds on loopback");
        let token = std::fs::read_to_string(&route.token_path).expect("token file is readable");

        Self {
            route,
            token,
            client: reqwest::Client::new(),
            _dir: dir,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.route.base_url)
    }

    fn authed(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.bearer_auth(&self.token)
    }
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn the_full_session_lifecycle_works_over_the_loopback_route() {
    let harness = Harness::start().await;

    let created = harness
        .authed(harness.client.post(harness.url("/v1/sessions")))
        .json(&json!({ "sessionId": "s1" }))
        .send()
        .await
        .expect("create request sends");
    assert_eq!(created.status(), 200, "{}", created.text().await.unwrap_or_default());

    let listed: Vec<serde_json::Value> = harness
        .authed(harness.client.get(harness.url("/v1/sessions")))
        .send()
        .await
        .expect("list sends")
        .json()
        .await
        .expect("list parses");
    assert_eq!(listed.len(), 1, "the created session must be listed");

    let exec: serde_json::Value = harness
        .authed(harness.client.post(harness.url("/v1/sessions/s1/exec")))
        .json(&json!({ "command": ["/bin/sh", "-c", "echo hello-over-http; id -u"] }))
        .send()
        .await
        .expect("exec sends")
        .json()
        .await
        .expect("exec parses");

    assert_eq!(exec["exitCode"], 0);
    let decoded: String = exec["output"]
        .as_array()
        .expect("frames")
        .iter()
        .filter(|frame| frame["stream"] == "stdout")
        .map(|frame| {
            String::from_utf8(
                BASE64
                    .decode(frame["dataBase64"].as_str().expect("base64 payload"))
                    .expect("decodes"),
            )
            .expect("utf8")
        })
        .collect();
    assert!(decoded.contains("hello-over-http"), "got: {decoded}");
    assert!(
        decoded.contains("65534"),
        "the workload must stay unprivileged across the transport: {decoded}"
    );

    let write = harness
        .authed(harness.client.put(harness.url("/v1/sessions/s1/files")))
        .json(&json!({
            "path": "in.txt",
            "contentsBase64": BASE64.encode(b"payload-over-http")
        }))
        .send()
        .await
        .expect("write sends");
    assert_eq!(write.status(), 204);

    let read: serde_json::Value = harness
        .authed(
            harness
                .client
                .get(harness.url("/v1/sessions/s1/files?path=in.txt")),
        )
        .send()
        .await
        .expect("read sends")
        .json()
        .await
        .expect("read parses");
    let contents = BASE64
        .decode(read["contentsBase64"].as_str().expect("base64 payload"))
        .expect("decodes");
    assert_eq!(String::from_utf8_lossy(&contents), "payload-over-http");

    // Preview is a published loopback port, resolved through the authenticated route rather
    // than guessed. An undeclared port must not resolve at all.
    let raw = harness
        .authed(harness.client.get(harness.url("/v1/sessions/s1/preview?port=8080")))
        .send()
        .await
        .expect("preview sends");
    let status = raw.status();
    let body = raw.text().await.expect("body reads");
    let preview: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("preview {status}: {body}"));
    let endpoint = preview["endpoint"].as_str().expect("an endpoint");
    assert!(
        endpoint.starts_with("http://127.0.0.1:"),
        "a preview must be bound to loopback, not the local network: {endpoint}"
    );

    let undeclared = harness
        .authed(harness.client.get(harness.url("/v1/sessions/s1/preview?port=9999")))
        .send()
        .await
        .expect("preview sends");
    assert_ne!(
        undeclared.status(),
        200,
        "a port not declared at create time must not resolve"
    );

    let terminated = harness
        .authed(harness.client.delete(harness.url("/v1/sessions/s1")))
        .send()
        .await
        .expect("terminate sends");
    assert_eq!(terminated.status(), 204);

    let after: Vec<serde_json::Value> = harness
        .authed(harness.client.get(harness.url("/v1/sessions")))
        .send()
        .await
        .expect("list sends")
        .json()
        .await
        .expect("list parses");
    assert!(after.is_empty(), "a terminated session must not be listed");
}

/// The route is on loopback, which is not authorization — anything else on the machine can
/// reach it, and reaching it means running code in someone's sandbox.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn every_route_refuses_a_request_without_a_valid_token() {
    let harness = Harness::start().await;

    let requests = vec![
        harness.client.get(harness.url("/v1/sessions")),
        harness
            .client
            .post(harness.url("/v1/sessions"))
            .json(&json!({ "sessionId": "nope" })),
        harness
            .client
            .post(harness.url("/v1/sessions/s1/exec"))
            .json(&json!({ "command": ["/bin/sh", "-c", "id"] })),
        harness
            .client
            .get(harness.url("/v1/sessions/s1/files?path=in.txt")),
        harness.client.delete(harness.url("/v1/sessions/s1")),
    ];

    for request in requests {
        let response = request.send().await.expect("request sends");
        assert_eq!(
            response.status(),
            401,
            "an unauthenticated request must be refused, got {}",
            response.status()
        );
    }

    // A wrong token of the right length must fail too, or the check is only testing length.
    let wrong = "0".repeat(harness.token.len());
    let response = harness
        .client
        .get(harness.url("/v1/sessions"))
        .bearer_auth(wrong)
        .send()
        .await
        .expect("request sends");
    assert_eq!(response.status(), 401);
}

/// The template lives server-side, so extra fields in a create request are simply ignored.
/// If they were honoured, an application could raise its own ceilings by asking.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn an_application_cannot_raise_its_own_limits() {
    let harness = Harness::start().await;

    let created = harness
        .authed(harness.client.post(harness.url("/v1/sessions")))
        .json(&json!({
            "sessionId": "greedy",
            "memoryBytes": 68_719_476_736i64,
            "pidsLimit": 1_000_000,
            "image": "ubuntu:24.04"
        }))
        .send()
        .await
        .expect("create sends");
    assert_eq!(created.status(), 200);

    // The template said alpine with 64 pids; the request asked for ubuntu with a million.
    let exec: serde_json::Value = harness
        .authed(harness.client.post(harness.url("/v1/sessions/greedy/exec")))
        .json(&json!({ "command": ["/bin/sh", "-c", "cat /etc/os-release | head -1"] }))
        .send()
        .await
        .expect("exec sends")
        .json()
        .await
        .expect("exec parses");

    let os: String = exec["output"]
        .as_array()
        .expect("frames")
        .iter()
        .filter(|frame| frame["stream"] == "stdout")
        .map(|frame| {
            String::from_utf8(
                BASE64
                    .decode(frame["dataBase64"].as_str().expect("base64"))
                    .expect("decodes"),
            )
            .expect("utf8")
        })
        .collect();
    assert!(
        os.to_lowercase().contains("alpine"),
        "the template's image must win over the request's: {os}"
    );

    harness
        .authed(harness.client.delete(harness.url("/v1/sessions/greedy")))
        .send()
        .await
        .expect("cleanup");
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn the_token_file_is_not_world_readable() {
    let harness = Harness::start().await;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&harness.route.token_path)
            .expect("token file exists")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "the token must not be readable by other users, mode was {mode:o}"
        );
    }
}

/// Removing the route is what makes a sweep final, so it has to mean "no create is in flight" and
/// not merely "no new create is accepted". A create already inside the route when removal starts
/// must have committed by the time removal returns — otherwise the reap that follows can run
/// between the two, and the session it missed lives on with no route left to reach it by.
///
/// The create is made genuinely slow by pointing it at an image the daemon has to pull, so it is
/// parked inside the route for seconds when removal fires; a warm image commits in milliseconds
/// and would let removal win every race without proving anything.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn a_create_in_flight_at_removal_is_visible_to_the_reap_that_follows() {
    const SLOW_IMAGE: &str = "busybox:1.36";
    const SANDBOX_SLOW: &str = "route-slow";
    // Absent before every run, so the pull — and the in-flight window — is real each time.
    let _ = tokio::process::Command::new("docker")
        .args(["image", "rm", "-f", SLOW_IMAGE])
        .output()
        .await;

    let dir = TempDir::new().expect("temp dir");
    let manager = Arc::new(
        LocalSandboxManager::new(dir.path().to_path_buf()).expect("Docker must be reachable"),
    );
    manager.reap(SANDBOX_SLOW).await.expect("clean slate");
    let template = SandboxSessionConfig {
        image: SLOW_IMAGE.to_string(),
        cpu_cores: 0.5,
        memory_bytes: 268_435_456,
        pids_limit: Some(64),
        scratch_bytes: 16_777_216,
        egress: SandboxEgressMode::Allow,
        preview_ports: Vec::new(),
        env: Default::default(),
    };
    let route = SandboxRoute::ensure(Arc::clone(&manager), SANDBOX_SLOW, template)
        .await
        .expect("route binds on loopback");
    let token = std::fs::read_to_string(&route.token_path).expect("token file is readable");
    let client = reqwest::Client::new();

    // Fire the create; it is now pulling inside the route. Give it a moment to be certainly
    // past the front door, then remove the route underneath it.
    let create = tokio::spawn({
        let client = client.clone();
        let url = format!("{}/v1/sessions", route.base_url);
        let token = token.clone();
        async move {
            client
                .post(url)
                .bearer_auth(token)
                .json(&json!({ "sessionId": "racing" }))
                .send()
                .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    SandboxRoute::remove(SANDBOX_SLOW).await;

    // Removal has returned. If it waited for the listener, the create has already committed or
    // been refused — so the reap sees the whole truth and nothing can appear afterwards.
    let reaped = manager.reap(SANDBOX_SLOW).await.expect("reap");
    let created = create.await.expect("create task");
    let committed = created.map(|r| r.status().is_success()).unwrap_or(false);
    assert_eq!(
        reaped,
        usize::from(committed),
        "a create that answered success must already be there for the sweep after removal \
         (committed={committed}, reaped={reaped})"
    );
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let after = manager.list_sessions(SANDBOX_SLOW).await.expect("lists");
    assert!(after.is_empty(), "a session appeared after the route was removed: {after:?}");
    manager.reap(SANDBOX_SLOW).await.expect("cleanup");
}
