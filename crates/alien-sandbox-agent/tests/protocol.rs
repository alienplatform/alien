//! The agent protocol over a real socket.
//!
//! The unit tests prove each rule in isolation; these prove the rules survive being wired to
//! HTTP — that authorization actually runs before a handler touches the session, that a stream
//! carries frames a caller can parse, and that a refusal reaches the caller as a status code
//! rather than a body it might mistake for a result.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use alien_core::sandbox_capability::{
    SandboxCapabilityClaims, SandboxOperationClass, SandboxSessionIdentity,
};
use alien_core::sandbox_capability_token;
use alien_sandbox_agent::exec::ExecIdentity;
use alien_sandbox_agent::jobs::JobRegistry;
use alien_sandbox_agent::server::{router, AgentAuthorization, AgentState, PROTOCOL_VERSION};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_compact::{KeyPair, Seed};
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;

const SESSION: &str = "session-1";

/// The test process's own identity. Setting a uid to its own is permitted unprivileged, so the
/// real drop path runs without needing root.
fn test_identity() -> ExecIdentity {
    unsafe {
        ExecIdentity {
            uid: libc::getuid(),
            gid: libc::getgid(),
        }
    }
}
const GENERATION: u64 = 3;

struct Agent {
    base_url: String,
    keys: KeyPair,
    root: PathBuf,
    state: Arc<AgentState>,
    _dir: TempDir,
}

impl Agent {
    async fn start() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().canonicalize().expect("canonical root");
        let keys = KeyPair::from_seed(Seed::new([3u8; 32]));

        let state = Arc::new(AgentState {
            session_root: root.clone(),
            authorization: AgentAuthorization::Capability {
                public_key: keys.pk,
                identity: SandboxSessionIdentity {
                    session_id: SESSION.to_string(),
                    generation: GENERATION,
                },
            },
            exec_identity: test_identity(),
            output_cap: 1 << 20,
            jobs: JobRegistry::new(),
        });

        let served = Arc::clone(&state);

        let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
            .await
            .expect("bind loopback");
        let address = listener.local_addr().expect("address");

        tokio::spawn(async move {
            axum::serve(
                listener,
                router(served).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .expect("serve");
        });

        Self {
            base_url: format!("http://{address}"),
            keys,
            root,
            state,
            _dir: dir,
        }
    }

    /// A capability this agent should accept.
    fn capability(&self) -> String {
        self.mint(claims())
    }

    fn mint(&self, claims: SandboxCapabilityClaims) -> String {
        sandbox_capability_token::mint(&claims, &self.keys.sk).expect("mints")
    }
}

fn claims() -> SandboxCapabilityClaims {
    SandboxCapabilityClaims {
        session_id: SESSION.to_string(),
        operation: SandboxOperationClass::Execute,
        generation: GENERATION,
        expires_at: chrono::Utc::now().timestamp() + 300,
        key_id: "k1".to_string(),
    }
}

/// Parses an NDJSON body into frames, asserting the stream is well-formed as the protocol
/// defines it: parseable lines, and exactly one terminal frame, last.
fn frames(body: &str) -> Vec<serde_json::Value> {
    let frames: Vec<serde_json::Value> = body
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("every frame is a complete JSON line"))
        .collect();

    let terminals = frames
        .iter()
        .filter(|frame| matches!(frame["t"].as_str(), Some("exit") | Some("error")))
        .count();
    assert_eq!(terminals, 1, "exactly one terminal frame: {frames:?}");
    assert!(
        matches!(
            frames.last().expect("at least one frame")["t"].as_str(),
            Some("exit") | Some("error")
        ),
        "the terminal frame must be last: {frames:?}"
    );

    frames
}

#[tokio::test]
async fn health_reports_the_protocol_version_without_a_capability() {
    let agent = Agent::start().await;

    let response = reqwest::get(format!("{}/v1/health", agent.base_url))
        .await
        .expect("health responds");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.expect("json");
    assert_eq!(body["protocolVersion"], PROTOCOL_VERSION);
    // The container boot id a caller derives its generation from: present and non-empty, so a
    // reconnecting caller can tell its container from a blank one wearing the same name.
    assert!(
        body["bootId"].as_str().is_some_and(|id| !id.is_empty()),
        "health reports a non-empty container boot id: {body}"
    );
}

/// The agent outlives the deployment that built its image, so a mismatch has
/// to be a named error — not a request the agent half-understands.
#[tokio::test]
async fn a_version_mismatch_is_a_typed_error_naming_both_versions() {
    let agent = Agent::start().await;

    let response = reqwest::get(format!(
        "{}/v1/health?version={}",
        agent.base_url,
        PROTOCOL_VERSION + 1
    ))
    .await
    .expect("responds");

    assert_eq!(response.status(), 400);
    let body = response.text().await.expect("body");
    assert!(
        body.contains(&format!("v{}", PROTOCOL_VERSION + 1))
            && body.contains(&format!("v{PROTOCOL_VERSION}")),
        "the error must name both versions: {body}"
    );
}

#[tokio::test]
async fn a_request_without_a_capability_is_refused() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/exec", agent.base_url))
        .json(&json!({"command": ["/bin/echo", "hi"], "deadlineMs": 5000}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 401);
}

/// Over the wire: session ids and hostnames are guessable, so this is the refusal that matters
/// most.
#[tokio::test]
async fn a_capability_for_another_session_is_refused() {
    let agent = Agent::start().await;
    let mut other = claims();
    other.session_id = "someone-elses-session".to_string();

    let response = reqwest::Client::new()
        .post(format!("{}/v1/exec", agent.base_url))
        .bearer_auth(agent.mint(other))
        .json(&json!({"command": ["/bin/echo", "hi"], "deadlineMs": 5000}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 403);
}

/// Terminate bumps the generation, and anything minted before it is void.
#[tokio::test]
async fn a_capability_from_a_previous_generation_is_refused() {
    let agent = Agent::start().await;
    let mut stale = claims();
    stale.generation = GENERATION - 1;

    let response = reqwest::Client::new()
        .get(format!("{}/v1/files?path=/anything", agent.base_url))
        .bearer_auth(agent.mint(stale))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 403);
}

/// A malformed token is refused with the same status as a well-formed one for the wrong session.
///
/// The status is all an unauthenticated caller sees, so a different one for a garbled token would
/// tell them which of the two they sent. This pins that: the agent derives the status from the
/// error, and every failure inside `verify` is the same coarse refusal.
#[tokio::test]
async fn a_malformed_token_is_refused_like_a_wrong_one() {
    let agent = Agent::start().await;

    for token in ["not-a-token", "a.b.c", "!!!!", "eyJhbGciOiJub25lIn0."] {
        let response = reqwest::Client::new()
            .get(format!("{}/v1/files?path=/anything", agent.base_url))
            .bearer_auth(token)
            .send()
            .await
            .expect("responds");

        assert_eq!(response.status(), 403, "malformed token {token:?}");
    }
}

#[tokio::test]
async fn an_expired_capability_is_refused() {
    let agent = Agent::start().await;
    let mut expired = claims();
    expired.expires_at = chrono::Utc::now().timestamp() - 1;

    let response = reqwest::Client::new()
        .get(format!("{}/v1/files?path=/anything", agent.base_url))
        .bearer_auth(agent.mint(expired))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn a_command_streams_its_output_and_a_real_exit_code() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/exec", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"command": ["/bin/sh", "-c", "echo out; echo err 1>&2; exit 7"], "deadlineMs": 10_000}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/x-ndjson")
    );

    let frames = frames(&response.text().await.expect("body"));
    let terminal = frames.last().expect("terminal");
    assert_eq!(terminal["t"], "exit");
    assert_eq!(
        terminal["code"], 7,
        "the real exit code, not a normalised one"
    );

    let decoded: Vec<String> = frames
        .iter()
        .filter_map(|frame| frame["data"].as_str())
        .map(|data| String::from_utf8(BASE64.decode(data).expect("base64")).expect("utf8"))
        .collect();
    assert!(
        decoded.iter().any(|line| line.trim() == "out")
            && decoded.iter().any(|line| line.trim() == "err"),
        "both streams must reach the caller: {decoded:?}"
    );
}

/// Over the wire: the stream ends with an error frame naming the deadline, not with a silent
/// close the caller could read as success.
#[tokio::test]
async fn a_command_that_overruns_ends_the_stream_with_a_deadline_error() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/exec", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"command": ["/bin/sleep", "30"], "deadlineMs": 300}))
        .send()
        .await
        .expect("responds");

    let frames = frames(&response.text().await.expect("body"));
    let terminal = frames.last().expect("terminal");
    assert_eq!(terminal["t"], "error");
    assert_eq!(terminal["code"], "deadlineExceeded");
}

#[tokio::test]
async fn a_file_round_trips_through_the_protocol() {
    let agent = Agent::start().await;
    let client = reqwest::Client::new();

    let write = client
        .put(format!("{}/v1/files", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"path": "/work/main.py", "contentsBase64": BASE64.encode("print(1)")}))
        .send()
        .await
        .expect("responds");
    assert_eq!(write.status(), 204);

    let read: serde_json::Value = client
        .get(format!("{}/v1/files?path=/work/main.py", agent.base_url))
        .bearer_auth(agent.capability())
        .send()
        .await
        .expect("responds")
        .json()
        .await
        .expect("json");

    let contents = BASE64
        .decode(read["contentsBase64"].as_str().expect("contents"))
        .expect("base64");
    assert_eq!(String::from_utf8(contents).expect("utf8"), "print(1)");

    // On disk at the asked-for path: a trailing separator lands it elsewhere and the OS refuses the write.
    assert!(agent.root.join("work/main.py").is_file());
}

/// Over the wire: the resolver is unit-tested; this proves the HTTP layer cannot reach the
/// filesystem around it.
#[tokio::test]
async fn path_traversal_is_refused_over_the_protocol() {
    let agent = Agent::start().await;
    let client = reqwest::Client::new();

    let read = client
        .get(format!(
            "{}/v1/files?path=/../../etc/passwd",
            agent.base_url
        ))
        .bearer_auth(agent.capability())
        .send()
        .await
        .expect("responds");
    assert_eq!(read.status(), 400);

    let write = client
        .put(format!("{}/v1/files", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"path": "../escaped.txt", "contentsBase64": BASE64.encode("x")}))
        .send()
        .await
        .expect("responds");
    assert_eq!(write.status(), 400);

    let working_directory = client
        .post(format!("{}/v1/exec", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({
            "command": ["/bin/pwd"],
            "deadlineMs": 5000,
            "workingDirectory": "/../.."
        }))
        .send()
        .await
        .expect("responds");
    assert_eq!(
        working_directory.status(),
        400,
        "a working directory outside the session must be refused before anything is spawned"
    );
}

#[tokio::test]
async fn mkdir_creates_a_directory_inside_the_session() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/mkdir", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"path": "/work/build"}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 204);
    assert!(agent.root.join("work/build").is_dir());
}

/// AWS: the proxy validates a JWE scoped to one MicroVM, an explicit port set and an expiry
/// before a request ever arrives, and one MicroVM is one session. This asserts the mode is
/// real — and, by contrast with the 401 above, that choosing it is what changes the outcome
/// rather than the capability check being absent everywhere.
#[tokio::test]
async fn transport_authorization_needs_no_capability() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");

    let state = Arc::new(AgentState {
        session_root: root.clone(),
        authorization: AgentAuthorization::Transport,
        // Not the test's own uid: the agent and the code it runs are different users in a real
        // image, and the caller here stands in for one arriving through the transport.
        exec_identity: ExecIdentity {
            uid: 60000,
            gid: 60000,
        },
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
    });

    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
        .await
        .expect("bind loopback");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(
            listener,
            router(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve");
    });

    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/mkdir"))
        .json(&json!({"path": "/work"}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 204);
    assert!(root.join("work").is_dir());
}

/// Transport mode accepts a caller without a capability, which is safe for anything arriving
/// through the transport and is not safe for the command the agent itself started — that command
/// shares the guest's network stack and can reach the same port. Running the agent with this
/// process as its exec identity is what a command connecting back looks like from the inside.
///
/// Linux-only because the socket's owner is read from `/proc/net/tcp`.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn transport_authorization_refuses_the_code_the_agent_runs() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");

    let state = Arc::new(AgentState {
        session_root: root.clone(),
        authorization: AgentAuthorization::Transport,
        exec_identity: test_identity(),
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
    });

    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
        .await
        .expect("bind loopback");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(
            listener,
            router(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve");
    });

    let response = reqwest::Client::new()
        .post(format!("http://{address}/v1/mkdir"))
        .json(&json!({"path": "/work"}))
        .send()
        .await
        .expect("responds");

    assert_eq!(
        response.status(),
        403,
        "the agent must not serve its own supervised code"
    );
    assert!(
        !root.join("work").exists(),
        "a refused request must not have done its work anyway"
    );
}

/// The paths carry the `/aws/lambda-microvms/runtime/v1` prefix. Serving the short `/ready` the
/// service's error message names 404s every probe, and the image build then fails after minutes
/// with no logs to explain it.
#[tokio::test]
async fn the_lifecycle_hooks_answer_without_a_capability() {
    let agent = Agent::start().await;

    for hook in ["ready", "validate", "run", "resume", "suspend", "terminate"] {
        let path = alien_sandbox_agent::server::hook_path(hook);
        let response = reqwest::get(format!("{}{path}", agent.base_url))
            .await
            .expect("responds");
        assert_eq!(
            response.status(),
            200,
            "{path} is called by the MicroVM service, not a session caller, so it cannot require a capability"
        );
    }
}

// --- The GCP Agent Platform envelope on `POST /` ---
//
// One route carries every operation, selected by `op`, because the platform's `:execute` proxies
// `POST /` with the body verbatim and can set neither path nor method. These prove the envelope's
// output is the same bytes the versioned route produces, and that a bad envelope is refused with a
// typed error rather than defaulted onto some operation.

/// Captures what a response is on the wire: the three things the envelope must reproduce exactly.
async fn wire(response: reqwest::Response) -> (u16, Option<String>, String) {
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.text().await.expect("body");
    (status, content_type, body)
}

/// A single write to one stream forces a deterministic frame sequence — one `stdout` at `seq: 0`
/// then `exit` — so the two NDJSON bodies are comparable byte for byte. A command writing to both
/// streams would interleave nondeterministically and its `seq` would differ per run.
#[tokio::test]
async fn exec_through_the_envelope_is_byte_identical_to_v1() {
    let agent = Agent::start().await;
    let client = reqwest::Client::new();

    let versioned = client
        .post(format!("{}/v1/exec", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"command": ["/bin/echo", "hello"], "deadlineMs": 10_000}))
        .send()
        .await
        .expect("responds");
    let enveloped = client
        .post(format!("{}/", agent.base_url))
        .bearer_auth(agent.capability())
        .json(
            &json!({"v": 1, "op": "exec", "command": ["/bin/echo", "hello"], "deadlineMs": 10_000}),
        )
        .send()
        .await
        .expect("responds");

    let versioned = wire(versioned).await;
    let enveloped = wire(enveloped).await;
    assert_eq!(versioned.1.as_deref(), Some("application/x-ndjson"));
    assert_eq!(
        versioned, enveloped,
        "the envelope must reproduce the versioned route's stream exactly"
    );
}

#[tokio::test]
async fn read_file_through_the_envelope_is_byte_identical_to_v1() {
    let agent = Agent::start().await;
    let client = reqwest::Client::new();

    client
        .put(format!("{}/v1/files", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"path": "/work/main.py", "contentsBase64": BASE64.encode("print(1)")}))
        .send()
        .await
        .expect("responds");

    let versioned = client
        .get(format!("{}/v1/files?path=/work/main.py", agent.base_url))
        .bearer_auth(agent.capability())
        .send()
        .await
        .expect("responds");
    let enveloped = client
        .post(format!("{}/", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"v": 1, "op": "readFile", "path": "/work/main.py"}))
        .send()
        .await
        .expect("responds");

    assert_eq!(wire(versioned).await, wire(enveloped).await);
}

#[tokio::test]
async fn write_file_through_the_envelope_is_byte_identical_to_v1_and_lands() {
    let agent = Agent::start().await;
    let client = reqwest::Client::new();

    let versioned = client
        .put(format!("{}/v1/files", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"path": "/work/v1.txt", "contentsBase64": BASE64.encode("x")}))
        .send()
        .await
        .expect("responds");
    let enveloped = client
        .post(format!("{}/", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"v": 1, "op": "writeFile", "path": "/work/env.txt", "contentsBase64": BASE64.encode("x")}))
        .send()
        .await
        .expect("responds");

    assert_eq!(wire(versioned).await, wire(enveloped).await);
    assert_eq!(
        std::fs::read(agent.root.join("work/env.txt")).expect("the envelope write landed"),
        b"x"
    );
}

#[tokio::test]
async fn mkdir_through_the_envelope_is_byte_identical_to_v1_and_lands() {
    let agent = Agent::start().await;
    let client = reqwest::Client::new();

    let versioned = client
        .post(format!("{}/v1/mkdir", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"path": "/work/v1"}))
        .send()
        .await
        .expect("responds");
    let enveloped = client
        .post(format!("{}/", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"v": 1, "op": "mkdir", "path": "/work/env"}))
        .send()
        .await
        .expect("responds");

    assert_eq!(wire(versioned).await, wire(enveloped).await);
    assert!(
        agent.root.join("work/env").is_dir(),
        "the envelope mkdir landed"
    );
}

/// `health` carries no capability on either route, so the envelope arm must reach it without one —
/// the version it would assert is already the envelope's own `v`.
#[tokio::test]
async fn health_through_the_envelope_is_byte_identical_to_v1() {
    let agent = Agent::start().await;

    let versioned = reqwest::get(format!("{}/v1/health", agent.base_url))
        .await
        .expect("responds");
    let enveloped = reqwest::Client::new()
        .post(format!("{}/", agent.base_url))
        .json(&json!({"v": 1, "op": "health"}))
        .send()
        .await
        .expect("responds");

    assert_eq!(wire(versioned).await, wire(enveloped).await);
}

/// An absent `op` is refused, not defaulted onto an operation. Refused before authorization, so no
/// capability is needed to reach the check.
#[tokio::test]
async fn an_absent_op_is_refused() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/", agent.base_url))
        .json(&json!({"v": 1}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 400);
    assert!(
        response
            .text()
            .await
            .expect("body")
            .contains("must name an 'op'"),
        "the refusal must say an op is required"
    );
}

/// An unknown `op` is refused rather than silently mapped to some operation.
#[tokio::test]
async fn an_unknown_op_is_refused() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/", agent.base_url))
        .json(&json!({"v": 1, "op": "frobnicate"}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 400);
    assert!(
        response.text().await.expect("body").contains("frobnicate"),
        "the refusal must name the unknown op"
    );
}

/// The agent outlives the image that built it, so a version it does not implement is a named
/// refusal — not a request it half-understands.
#[tokio::test]
async fn an_unsupported_version_is_refused() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/", agent.base_url))
        .json(&json!({"v": PROTOCOL_VERSION + 1, "op": "health"}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 400);
    let body = response.text().await.expect("body");
    assert!(
        body.contains(&format!("v{}", PROTOCOL_VERSION + 1))
            && body.contains(&format!("v{PROTOCOL_VERSION}")),
        "the error must name both versions: {body}"
    );
}

/// The envelope must route through `peer.rs`, not around it: under transport authorization the code
/// the agent itself runs shares the guest's network stack and reaches this port, and it must be
/// refused there exactly as it is on `/v1/mkdir`. Running the agent with this process as its exec
/// identity is what that in-guest caller looks like from the inside.
///
/// Linux-only because the socket's owner is read from `/proc/net/tcp`.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn the_envelope_refuses_the_code_the_agent_runs_under_transport() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");

    let state = Arc::new(AgentState {
        session_root: root.clone(),
        authorization: AgentAuthorization::Transport,
        exec_identity: test_identity(),
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
    });

    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
        .await
        .expect("bind loopback");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(
            listener,
            router(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve");
    });

    let response = reqwest::Client::new()
        .post(format!("http://{address}/"))
        .json(&json!({"v": 1, "op": "mkdir", "path": "/work"}))
        .send()
        .await
        .expect("responds");

    assert_eq!(
        response.status(),
        403,
        "the envelope must not serve the agent's own supervised code"
    );
    assert!(
        !root.join("work").exists(),
        "a refused envelope must not have done its work anyway"
    );
}

// --- Detached jobs: start, poll for output across calls, cancel ---
//
// A command longer than one proxied call can stay open runs as a job. These prove the endpoints
// are wired to the same authorization and framing the streaming path uses, and that the
// synchronous path is left untouched.

/// A job runs to completion and its output is collected across polls, exactly as a provider that
/// cannot hold one long call open would have to read it.
#[tokio::test]
async fn a_job_completes_and_its_output_is_polled_across_calls() {
    let agent = Agent::start().await;
    let client = reqwest::Client::new();

    let started = client
        .post(format!("{}/", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({
            "v": 1,
            "op": "jobStart",
            "command": ["/bin/sh", "-c", "echo one; echo two"],
            "deadlineMs": 10_000
        }))
        .send()
        .await
        .expect("responds");
    assert_eq!(started.status(), 200);
    let job_id = started.json::<serde_json::Value>().await.expect("json")["jobId"]
        .as_str()
        .expect("a job id")
        .to_string();

    let mut collected = Vec::new();
    let mut since: Option<u64> = None;
    let mut running = true;
    for _ in 0..200 {
        let body = client
            .post(format!("{}/", agent.base_url))
            .bearer_auth(agent.capability())
            .json(&json!({"v": 1, "op": "jobPoll", "jobId": job_id, "sinceSeq": since}))
            .send()
            .await
            .expect("responds")
            .json::<serde_json::Value>()
            .await
            .expect("json");

        for frame in body["frames"].as_array().expect("frames array") {
            if let Some(seq) = frame["seq"].as_u64() {
                since = Some(since.map_or(seq, |s| s.max(seq)));
            }
            if let Some(data) = frame["data"].as_str() {
                collected
                    .push(String::from_utf8(BASE64.decode(data).expect("base64")).expect("utf8"));
            }
        }

        if !body["running"].as_bool().expect("running is a bool") {
            running = false;
            assert_eq!(body["exitCode"], 0, "the job exited cleanly");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    assert!(
        !running,
        "the job must reach a terminal state within the poll budget"
    );
    let text: Vec<&str> = collected
        .iter()
        .flat_map(|line| line.split_whitespace())
        .collect();
    assert_eq!(text, vec!["one", "two"], "every line survives being polled");
}

/// The synchronous path is left as it was: an ordinary command over `/v1/exec` runs and returns
/// without ever creating a job. The provider decides when to reach for a job; `exec` never does.
#[tokio::test]
async fn a_command_over_exec_creates_no_job() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/exec", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"command": ["/bin/echo", "hi"], "deadlineMs": 10_000}))
        .send()
        .await
        .expect("responds");
    assert_eq!(response.status(), 200);
    let _ = response.text().await.expect("body");

    assert!(
        agent.state.jobs.is_empty(),
        "the synchronous exec path must not create a job"
    );
}

/// A poll for a job the session never held is a typed 404, not an empty running job the caller
/// would wait on forever.
#[tokio::test]
async fn a_poll_for_an_unknown_job_is_refused() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"v": 1, "op": "jobPoll", "jobId": "nonexistent", "sinceSeq": null}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 404);
}

/// The job endpoints refuse a request that carries no capability, like every other operation that
/// can reach session contents.
#[tokio::test]
async fn starting_a_job_without_a_capability_is_refused() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/jobs/start", agent.base_url))
        .json(&json!({"command": ["/bin/echo", "hi"], "deadlineMs": 10_000}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 401);
    assert!(
        agent.state.jobs.is_empty(),
        "a refused start creates no job"
    );
}
