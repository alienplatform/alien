//! The agent protocol over a real socket.
//!
//! The unit tests prove each rule in isolation; these prove the rules survive being wired to
//! HTTP — that authorization actually runs before a handler touches the session, that a stream
//! carries frames a caller can parse, and that a refusal reaches the caller as a status code
//! rather than a body it might mistake for a result.

use std::net::SocketAddr;
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::sync::Arc;

use alien_core::sandbox_capability::{
    SandboxCapabilityClaims, SandboxOperationClass, SandboxSessionIdentity,
};
use alien_core::sandbox_capability_token;
use alien_sandbox_agent::exec::ExecIdentity;
use alien_sandbox_agent::jobs::JobRegistry;
use alien_sandbox_agent::server::{router, AgentAuthorization, AgentState, PROTOCOL_VERSION};
use alien_sandbox_agent::validate;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_compact::{KeyPair, Seed};
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;

const SESSION: &str = "session-1";

/// The test process's own identity. Setting a uid to its own is permitted unprivileged, so the
/// real drop path runs without needing root.
/// What the test agent keeps of one `/v1/exec` stream.
const OUTPUT_CAP: usize = 1 << 20;

fn test_identity() -> ExecIdentity {
    unsafe {
        ExecIdentity {
            uid: libc::getuid(),
            gid: libc::getgid(),
        }
    }
}
const GENERATION: u64 = 3;

/// The real agent binary, so the validate hook's identity probe runs what an image runs.
fn agent_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alien-sandbox-agent"))
}

struct Agent {
    base_url: String,
    keys: KeyPair,
    root: PathBuf,
    state: Arc<AgentState>,
    _dir: TempDir,
}

impl Agent {
    async fn start() -> Self {
        Self::start_with_authorization(None).await
    }

    async fn start_with_authorization(authorization: Option<AgentAuthorization>) -> Self {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().canonicalize().expect("canonical root");
        let keys = KeyPair::from_seed(Seed::new([3u8; 32]));

        let state = Arc::new(AgentState {
            image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
            #[cfg(target_os = "linux")]
            gateway_egress: None,
            session_root: root.clone(),
            authorization: authorization.unwrap_or(AgentAuthorization::Capability {
                public_key: keys.pk,
                identity: SandboxSessionIdentity {
                    session_id: SESSION.to_string(),
                    generation: GENERATION,
                },
            }),
            exec_identity: test_identity(),
            output_cap: OUTPUT_CAP,
            jobs: JobRegistry::new(),
            interactive: Default::default(),
            preview: Default::default(),
            agent_binary: agent_binary(),
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
        port: None,
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
        .json(&json!({"command": ["/bin/echo", "hi"], "timeoutMs": 5000}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 401);
}

/// The write path carries the same check as exec: it is the one route that changes the sandbox's
/// filesystem, and the transport-mode tests below rely on this check being what they relax.
#[tokio::test]
async fn a_write_without_a_capability_is_refused() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .put(format!("{}/v1/files", agent.base_url))
        .json(&json!({"path": "/work/landed.txt", "contentsBase64": BASE64.encode("x")}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 401);
    assert!(
        !agent.root.join("work/landed.txt").exists(),
        "a refused write must not have landed anyway"
    );
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
        .json(&json!({"command": ["/bin/echo", "hi"], "timeoutMs": 5000}))
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
        .json(&json!({"command": ["/bin/sh", "-c", "echo out; echo err 1>&2; exit 7"], "timeoutMs": 10_000}))
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

/// Over the wire: the stream ends with an error frame naming the timeout, not with a silent
/// close the caller could read as success.
#[tokio::test]
async fn a_command_that_overruns_ends_the_stream_with_a_timeout_error() {
    let agent = Agent::start().await;

    let response = reqwest::Client::new()
        .post(format!("{}/v1/exec", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"command": ["/bin/sleep", "30"], "timeoutMs": 300}))
        .send()
        .await
        .expect("responds");

    let frames = frames(&response.text().await.expect("body"));
    let terminal = frames.last().expect("terminal");
    assert_eq!(terminal["t"], "error");
    assert_eq!(terminal["code"], "timeoutExceeded");
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
            "timeoutMs": 5000,
            "cwd": "/../.."
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

/// AWS: the proxy validates a JWE scoped to one MicroVM, an explicit port set and an expiry
/// before a request ever arrives, and one MicroVM is one session. This asserts the mode is
/// real — and, by contrast with the 401 above, that choosing it is what changes the outcome
/// rather than the capability check being absent everywhere.
#[tokio::test]
async fn transport_authorization_needs_no_capability() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");

    let state = Arc::new(AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        #[cfg(target_os = "linux")]
        gateway_egress: None,
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
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: agent_binary(),
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
        .put(format!("http://{address}/v1/files"))
        .json(&json!({"path": "/work/landed.txt", "contentsBase64": BASE64.encode("x")}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 204);
    assert!(root.join("work/landed.txt").is_file());
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
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        #[cfg(target_os = "linux")]
        gateway_egress: None,
        session_root: root.clone(),
        authorization: AgentAuthorization::Transport,
        exec_identity: test_identity(),
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: agent_binary(),
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
        .put(format!("http://{address}/v1/files"))
        .json(&json!({"path": "/work/landed.txt", "contentsBase64": BASE64.encode("x")}))
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

    for hook in ["ready", "run", "resume", "suspend", "terminate"] {
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

// --- The validate hook ---
//
// AWS calls it on a MicroVM run from the finished snapshot; 200 passes the build and any other
// status fails it with the body as the reason. These prove a working agent passes without a trace
// left in the session, and that each check fails by name.

/// Serves `state` on loopback and returns the base URL.
async fn serve(state: Arc<AgentState>) -> String {
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
    format!("http://{address}")
}

/// A working agent passes, and the probe file it wrote is gone again: the hook runs on a VM the
/// build throws away, but it is also reachable on a live session, where a leftover would be a
/// file the caller never wrote.
#[tokio::test]
async fn the_validate_hook_passes_and_leaves_the_session_root_empty() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");

    validate::run(&AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        #[cfg(target_os = "linux")]
        gateway_egress: None,
        session_root: root.clone(),
        authorization: AgentAuthorization::Transport,
        exec_identity: test_identity(),
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: agent_binary(),
    })
    .await
    .expect("a working agent must validate");

    let left: Vec<_> = std::fs::read_dir(&root)
        .expect("the root is readable")
        .collect();
    assert!(left.is_empty(), "the probe file must be removed: {left:?}");
}

/// The checks spawn a process and write into the session, so under transport authorization the
/// hook serves only a caller the transport vouches for. The command shares the guest's network
/// stack, and a request from a socket it owns is refused as every other session route refuses it.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_validate_hook_refuses_a_caller_running_as_the_exec_identity() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");

    let base_url = serve(Arc::new(AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        gateway_egress: None,
        session_root: root.clone(),
        authorization: AgentAuthorization::Transport,
        exec_identity: test_identity(),
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: agent_binary(),
    }))
    .await;

    let response = reqwest::get(format!(
        "{base_url}{}",
        alien_sandbox_agent::server::hook_path("validate")
    ))
    .await
    .expect("responds");

    assert_eq!(
        response.status(),
        403,
        "the command must not drive the hook"
    );
    let left: Vec<_> = std::fs::read_dir(&root).expect("readable").collect();
    assert!(
        left.is_empty(),
        "a refused caller must not cause a write: {left:?}"
    );
}

/// Under a capability, reaching the port proves nothing: the checks spawn a process and write into
/// the session, so a caller without one is refused rather than served, and nothing runs.
#[cfg(unix)]
#[tokio::test]
async fn the_validate_hook_refuses_a_caller_without_a_capability() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");
    let keys = KeyPair::from_seed(Seed::new([3u8; 32]));

    let base_url = serve(Arc::new(AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        #[cfg(target_os = "linux")]
        gateway_egress: None,
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
        interactive: Default::default(),
        // A check that ran would answer 500 on this, so a 401 is the refusal and not a failed check.
        preview: Default::default(),
        agent_binary: root.join("no-such-agent"),
    }))
    .await;

    let response = reqwest::get(format!(
        "{base_url}{}",
        alien_sandbox_agent::server::hook_path("validate")
    ))
    .await
    .expect("responds");
    let status = response.status();
    let body = response.text().await.expect("body");

    assert_eq!(
        status, 401,
        "a caller without a capability is refused: {body}"
    );
    let left: Vec<_> = std::fs::read_dir(&root).expect("readable").collect();
    assert!(
        left.is_empty(),
        "nothing may be written into the session: {left:?}"
    );
}

/// The file check fails when the confined write cannot land, and the body says which check.
#[cfg(unix)]
#[tokio::test]
async fn the_validate_hook_names_the_file_check_when_the_session_root_is_not_writable() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");
    // Enterable but not writable, so the identity probe's chdir into it still succeeds and the
    // failure is the file check's alone.
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o500)).expect("chmod");

    let outcome = validate::run(&AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        #[cfg(target_os = "linux")]
        gateway_egress: None,
        session_root: root.clone(),
        authorization: AgentAuthorization::Transport,
        exec_identity: test_identity(),
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: agent_binary(),
    })
    .await;

    // Root writes through a 0500 directory, so the refusal cannot be provoked there.
    if unsafe { libc::geteuid() } == 0 {
        outcome.expect("root writes through a 0500 root");
        return;
    }

    let error = outcome.expect_err("an unwritable root must fail the build");
    assert!(
        error.to_string().contains("file check"),
        "the failure must name the check: {error}"
    );
}

/// The identity check fails when the probe cannot run as the configured identity, and the body
/// says so. Unprivileged, the drop to 65534 is refused by the kernel; as root the drop succeeds
/// and the probe then cannot enter a root-owned `0700` session root. Either way the probe never
/// reports an identity, and that is the identity check's failure to name.
#[cfg(unix)]
#[tokio::test]
async fn the_validate_hook_names_the_identity_check_when_the_probe_cannot_run_as_the_exec_identity()
{
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");

    let base_url = serve(Arc::new(AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        #[cfg(target_os = "linux")]
        gateway_egress: None,
        session_root: root.clone(),
        authorization: AgentAuthorization::Transport,
        exec_identity: ExecIdentity {
            uid: 65534,
            gid: 65534,
        },
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: agent_binary(),
    }))
    .await;

    let response = reqwest::get(format!(
        "{base_url}{}",
        alien_sandbox_agent::server::hook_path("validate")
    ))
    .await
    .expect("responds");
    let status = response.status();
    let body = response.text().await.expect("body");

    assert_eq!(
        status, 500,
        "a probe that cannot run must fail the build: {body}"
    );
    assert!(
        body.contains("identity check"),
        "the body must name the check that failed: {body}"
    );
    let left: Vec<_> = std::fs::read_dir(&root).expect("readable").collect();
    assert!(
        left.is_empty(),
        "a failed identity check must not have gone on to write: {left:?}"
    );
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
        .json(&json!({"command": ["/bin/echo", "hello"], "timeoutMs": 10_000}))
        .send()
        .await
        .expect("responds");
    let enveloped = client
        .post(format!("{}/", agent.base_url))
        .bearer_auth(agent.capability())
        .json(
            &json!({"v": 1, "op": "exec", "command": ["/bin/echo", "hello"], "timeoutMs": 10_000}),
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
/// refused there exactly as it is on `/v1/files`. Running the agent with this process as its exec
/// identity is what that in-guest caller looks like from the inside.
///
/// Linux-only because the socket's owner is read from `/proc/net/tcp`.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn the_envelope_refuses_the_code_the_agent_runs_under_transport() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");

    let state = Arc::new(AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        #[cfg(target_os = "linux")]
        gateway_egress: None,
        session_root: root.clone(),
        authorization: AgentAuthorization::Transport,
        exec_identity: test_identity(),
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: agent_binary(),
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
        .json(&json!({
            "v": 1,
            "op": "writeFile",
            "path": "/work/landed.txt",
            "contentsBase64": BASE64.encode("x"),
        }))
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
            "timeoutMs": 10_000
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
        .json(&json!({"command": ["/bin/echo", "hi"], "timeoutMs": 10_000}))
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
        .json(&json!({"command": ["/bin/echo", "hi"], "timeoutMs": 10_000}))
        .send()
        .await
        .expect("responds");

    assert_eq!(response.status(), 401);
    assert!(
        agent.state.jobs.is_empty(),
        "a refused start creates no job"
    );
}

/// The native file API delivers only a boot credential; exec still uses the normal confined path.
#[cfg(unix)]
#[tokio::test]
async fn native_file_credential_authorizes_only_its_own_boot() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("token");
    let old = alien_sandbox_agent::native_token::NativeToken::create(&path).unwrap();
    let old_value = std::fs::read_to_string(&path).unwrap();
    drop(old);
    let token = alien_sandbox_agent::native_token::NativeToken::create(&path).unwrap();
    let current = std::fs::read_to_string(&path).unwrap();
    let agent = Agent::start_with_authorization(Some(AgentAuthorization::NativeFile(token))).await;
    let client = reqwest::Client::new();
    let request = json!({"command": ["/bin/sh", "-c", "touch accepted; id -u"], "timeoutMs": 5000});
    for rejected in [None, Some(old_value.as_str()), Some("incorrect")] {
        let mut send = client
            .post(format!("{}/v1/exec", agent.base_url))
            .json(&request);
        if let Some(value) = rejected {
            send = send.bearer_auth(value);
        }
        let response = send.send().await.unwrap();
        assert_eq!(response.status(), 401);
        assert!(!agent.root.join("accepted").exists());
        let mut activation = client.post(format!("{}/v1/activate", agent.base_url));
        if let Some(value) = rejected {
            activation = activation.bearer_auth(value);
        }
        assert_eq!(activation.send().await.unwrap().status(), 401);
    }
    assert_eq!(
        client
            .post(format!("{}/v1/activate", agent.base_url))
            .bearer_auth(&current)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    let response = client
        .post(format!("{}/v1/exec", agent.base_url))
        .bearer_auth(current)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let output = frames(&response.text().await.unwrap());
    assert_eq!(output.last().unwrap()["code"], 0);
    let stdout: Vec<u8> = output
        .iter()
        .filter(|frame| frame["t"] == "stdout")
        .flat_map(|frame| BASE64.decode(frame["data"].as_str().unwrap()).unwrap())
        .collect();
    assert_eq!(
        String::from_utf8(stdout).unwrap().trim(),
        test_identity().uid.to_string()
    );
    assert!(agent.root.join("accepted").exists());
}

/// Exercise the same root spawn restriction as an OCI supervisor, over real TCP.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires root with Docker default capabilities and Python 3"]
async fn root_image_cannot_call_control_routes() {
    assert_eq!(unsafe { libc::geteuid() }, 0);
    let dir = TempDir::new().unwrap();
    let state = Arc::new(AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::awaiting_activation(),
        gateway_egress: None,
        session_root: dir.path().to_owned(),
        authorization: AgentAuthorization::Transport,
        exec_identity: ExecIdentity {
            uid: 60000,
            gid: 60000,
        },
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: agent_binary(),
    });
    let base = serve(state).await;
    let mut command = std::process::Command::new("python3");
    command.args(["-c", r#"
import json, sys, urllib.request, urllib.error
routes = [
 ('POST', '/aws/lambda-microvms/runtime/v1/resume', {}),
 ('POST', '/aws/lambda-microvms/runtime/v1/run', {}),
 ('GET', '/aws/lambda-microvms/runtime/v1/validate', None),
 ('PUT', '/v1/egress', {'policy':'forged','caCert':'forged'}),
 ('POST', '/v1/exec', {'command':['/bin/true'],'timeoutMs':1000}),
 ('PUT', '/v1/files', {'path':'forged','contentsBase64':'eA=='}),
 ('GET', '/v1/files?path=forged', None),
 ('POST', '/v1/jobs/start', {'command':['/bin/true'],'timeoutMs':1000}),
 ('POST', '/', {'v':1,'op':'exec','command':['/bin/true'],'timeoutMs':1000}),
]
failures=[]
for method,path,body in routes:
 request=urllib.request.Request(sys.argv[1]+path, method=method,
   data=None if body is None else json.dumps(body).encode(), headers={'Content-Type':'application/json'})
 try:
  with urllib.request.urlopen(request, timeout=5) as response: code=response.status
 except urllib.error.HTTPError as error: code=error.code
 print(method, path, code, flush=True)
 if code != 403: failures.append((path,code))
assert not failures, failures
"#, &base]);
    unsafe {
        command.pre_exec(|| {
            alien_sandbox_agent::privilege::drop_image_to(ExecIdentity { uid: 0, gid: 0 })
        });
    }
    let output = tokio::process::Command::from(command)
        .output()
        .await
        .unwrap();
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.path().join("forged").exists());
}

// --- Interactive commands: one output stream, input and signals as separate requests ---

/// One interactive command as a caller drives it: the output stream it reads, and the id its
/// writes, closes and signals name.
struct Interactive {
    agent_url: String,
    token: String,
    id: String,
    response: reqwest::Response,
    pending: Vec<u8>,
    stdout: Vec<u8>,
    frames: Vec<serde_json::Value>,
}

impl Interactive {
    async fn start(agent: &Agent, id: &str, command: &[&str]) -> Self {
        let token = agent.capability();
        let response = reqwest::Client::new()
            .post(format!("{}/v1/interactive", agent.base_url))
            .bearer_auth(&token)
            .json(&json!({"id": id, "command": command, "timeoutMs": 30_000}))
            .send()
            .await
            .expect("responds");
        assert_eq!(response.status(), 200, "an interactive command starts");
        Self {
            agent_url: agent.base_url.clone(),
            token,
            id: id.to_string(),
            response,
            pending: Vec::new(),
            stdout: Vec::new(),
            frames: Vec::new(),
        }
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("{}{path}", self.agent_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .expect("responds")
    }

    async fn write(&self, offset: u64, data: &[u8]) -> reqwest::Response {
        self.write_in(0, offset, data).await
    }

    async fn write_in(&self, generation: u64, offset: u64, data: &[u8]) -> reqwest::Response {
        self.post(
            "/v1/interactive/stdin",
            json!({
                "id": self.id,
                "generation": generation,
                "offset": offset,
                "data": BASE64.encode(data),
            }),
        )
        .await
    }

    async fn accepted(&self, offset: u64, data: &[u8]) -> u64 {
        let response = self.write(offset, data).await;
        assert_eq!(response.status(), 200, "the write is taken");
        let body: serde_json::Value = response.json().await.expect("json");
        body["accepted"].as_u64().expect("a count")
    }

    /// Reads frames until stdout ends with `expected`, failing after a bound rather than hanging.
    async fn read_until(&mut self, expected: &str) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while !String::from_utf8_lossy(&self.stdout).ends_with(expected) {
            let chunk = tokio::time::timeout_at(deadline, self.response.chunk())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "stdout never ended with {expected:?}: {:?}",
                        String::from_utf8_lossy(&self.stdout)
                    )
                })
                .expect("the stream reads")
                .unwrap_or_else(|| panic!("the stream ended before {expected:?}"));
            self.take(&chunk);
        }
    }

    /// Reads to the end of the stream and returns the terminal frame.
    async fn finish(mut self) -> (serde_json::Value, Vec<serde_json::Value>, String) {
        while let Some(chunk) =
            tokio::time::timeout(std::time::Duration::from_secs(10), self.response.chunk())
                .await
                .expect("the stream ends")
                .expect("the stream reads")
        {
            self.take(&chunk);
        }
        let terminals = self
            .frames
            .iter()
            .filter(|frame| matches!(frame["t"].as_str(), Some("exit") | Some("error")))
            .count();
        assert_eq!(
            terminals, 1,
            "exactly one terminal frame: {:?}",
            self.frames
        );
        let terminal = self.frames.last().cloned().expect("a terminal frame");
        let stdout = String::from_utf8_lossy(&self.stdout).into_owned();
        (terminal, self.frames, stdout)
    }

    fn take(&mut self, chunk: &[u8]) {
        self.pending.extend_from_slice(chunk);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let frame: serde_json::Value =
                serde_json::from_slice(&line).expect("every frame is a complete JSON line");
            if frame["t"] == "stdout" {
                self.stdout.extend(
                    BASE64
                        .decode(frame["data"].as_str().expect("data"))
                        .expect("base64"),
                );
            }
            self.frames.push(frame);
        }
    }
}

/// The whole point of the verb: a program that prompts, reads a line, answers and prompts again,
/// driven turn by turn while it runs. The prompt carries no newline, so this also proves output
/// is framed per read rather than per line.
#[tokio::test]
async fn an_interactive_command_answers_each_input_while_it_runs() {
    let agent = Agent::start().await;
    let mut command = Interactive::start(
        &agent,
        "repl-1",
        &[
            "/bin/sh",
            "-c",
            "printf '> '; while read line; do echo \"got $line\"; printf '> '; done",
        ],
    )
    .await;

    command.read_until("> ").await;
    assert_eq!(command.accepted(0, b"one\n").await, 4);
    command.read_until("got one\n> ").await;
    assert_eq!(command.accepted(4, b"two\n").await, 8);
    command.read_until("got two\n> ").await;

    let closed = command
        .post("/v1/interactive/close-stdin", json!({"id": "repl-1"}))
        .await;
    assert_eq!(closed.status(), 204);

    let (terminal, _, stdout) = command.finish().await;
    assert_eq!(terminal["t"], "exit");
    assert_eq!(terminal["code"], 0, "EOF on stdin ends the loop cleanly");
    assert_eq!(stdout, "> got one\n> got two\n> ");
    assert!(
        agent.state.interactive.is_empty(),
        "a finished command releases its id"
    );
}

/// A caller that holds the stream open without reading cannot keep a slot for a command its
/// deadline has killed: sixteen such callers would otherwise refuse every later start.
#[tokio::test]
async fn a_command_killed_at_its_deadline_frees_its_id_while_its_stream_is_unread() {
    let agent = Agent::start().await;
    let unread = reqwest::Client::new()
        .post(format!("{}/v1/interactive", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"id": "flood-1", "command": ["yes"], "timeoutMs": 500}))
        .send()
        .await
        .expect("responds");
    assert_eq!(unread.status(), 200, "an interactive command starts");

    // `yes` fills every buffer between it and the caller well before its 500ms deadline.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while !agent.state.interactive.is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the id outlived the command its deadline killed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let again = Interactive::start(&agent, "flood-1", &["true"]).await;
    let (terminal, _, _) = again.finish().await;
    assert_eq!(terminal["t"], "exit", "the freed id starts a new command");
    assert_eq!(terminal["code"], 0);
    drop(unread);
}

/// An interactive command's output is not capped as `/v1/exec`'s is: a REPL that went silent at a
/// byte count would look hung while it ran on. Every byte past the agent's cap arrives, in order.
#[tokio::test]
async fn an_interactive_command_streams_past_the_output_cap() {
    let agent = Agent::start().await;
    // Past both this agent's cap and the 4 MiB a deployed agent keeps of one stream.
    let total = (5 << 20) + 12_344;
    let script = format!(
        "i=0; while [ $i -lt {} ]; do printf '%07d\\n' $i; i=$((i+1)); done",
        total / 8
    );
    let command = Interactive::start(&agent, "big-1", &["/bin/sh", "-c", &script]).await;
    let (terminal, _, stdout) = command.finish().await;
    assert_eq!(terminal["t"], "exit", "{terminal}");
    assert_eq!(terminal["code"], 0);
    assert_eq!(terminal["truncated"], false, "nothing was dropped");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), total / 8, "every line arrived");
    assert!(
        lines
            .iter()
            .enumerate()
            .all(|(i, line)| *line == format!("{i:07}")),
        "in order and intact"
    );
}

/// A write that is retried — because its answer was lost — must be applied once, and a write that
/// would skip bytes must be refused rather than leave a silent gap in the program's input.
#[tokio::test]
async fn a_retried_write_is_applied_once_and_a_gap_is_refused() {
    let agent = Agent::start().await;
    let command = Interactive::start(&agent, "cat-1", &["/bin/cat"]).await;

    assert_eq!(command.accepted(0, b"abc").await, 3);
    assert_eq!(
        command.accepted(0, b"abc").await,
        3,
        "a repeat of bytes already taken is acknowledged, not written again"
    );
    assert_eq!(
        command.accepted(2, b"cde").await,
        5,
        "an overlapping write applies only its new tail"
    );

    let gap = command.write(9, b"x").await;
    assert_eq!(gap.status(), 409, "a write past what was taken is refused");
    assert!(
        gap.text().await.unwrap().contains("would leave a gap"),
        "the refusal says why"
    );

    command
        .post("/v1/interactive/close-stdin", json!({"id": "cat-1"}))
        .await;
    let (terminal, _, stdout) = command.finish().await;
    assert_eq!(terminal["code"], 0);
    assert_eq!(stdout, "abcde", "every byte exactly once, in order");
}

/// A barrier retires every write sent before it. A caller that gave up on a write cannot know
/// whether the request is still on its way; after the barrier that request is refused if it
/// arrives, rather than landing ahead of the bytes the caller sends next.
#[tokio::test]
async fn a_write_from_before_a_barrier_is_refused_after_it() {
    let agent = Agent::start().await;
    let command = Interactive::start(&agent, "cat-2", &["/bin/cat"]).await;

    assert_eq!(command.accepted(0, b"one ").await, 4);

    let barrier: serde_json::Value = command
        .post(
            "/v1/interactive/stdin-barrier",
            json!({"id": "cat-2", "generation": 1}),
        )
        .await
        .json()
        .await
        .expect("json");
    assert_eq!(barrier["accepted"], 4, "the barrier reports what was taken");
    assert_eq!(barrier["generation"], 1, "and moves the input on");

    // The same barrier again, as a retry whose first answer was lost: it moves nothing.
    let repeat: serde_json::Value = command
        .post(
            "/v1/interactive/stdin-barrier",
            json!({"id": "cat-2", "generation": 1}),
        )
        .await
        .json()
        .await
        .expect("json");
    assert_eq!(repeat["generation"], 1, "a repeated barrier is a no-op");

    // The old write, delayed past the barrier, at the offset where the caller now writes.
    let stale = command.write_in(0, 4, b"STALE ").await;
    assert_eq!(stale.status(), 409, "a retired write is refused");
    assert!(
        stale.text().await.unwrap().starts_with("STDIN_STALE_WRITE"),
        "the refusal names itself"
    );

    let taken: serde_json::Value = command
        .write_in(1, 4, b"two")
        .await
        .json()
        .await
        .expect("json");
    assert_eq!(taken["accepted"], 7);

    command
        .post("/v1/interactive/close-stdin", json!({"id": "cat-2"}))
        .await;
    let (terminal, _, stdout) = command.finish().await;
    assert_eq!(terminal["code"], 0);
    assert_eq!(stdout, "one two", "nothing from the retired write");
}

/// A signal reaches the command and the program decides what it means: here it traps `SIGINT`,
/// reports it, and exits on its own terms, as a REPL abandoning its input would keep running.
#[tokio::test]
async fn a_signal_reaches_the_command_while_it_runs() {
    let agent = Agent::start().await;
    let mut command = Interactive::start(
        &agent,
        "trap-1",
        &[
            "/bin/sh",
            "-c",
            "trap 'echo interrupted; exit 7' INT; echo ready; while :; do sleep 0.05; done",
        ],
    )
    .await;

    command.read_until("ready\n").await;
    let signalled = command
        .post(
            "/v1/interactive/signal",
            json!({"id": "trap-1", "signal": "interrupt"}),
        )
        .await;
    assert_eq!(signalled.status(), 204);

    let (terminal, _, stdout) = command.finish().await;
    assert_eq!(terminal["t"], "exit");
    assert_eq!(terminal["code"], 7, "the trap ran and chose the exit code");
    assert!(stdout.ends_with("interrupted\n"), "{stdout:?}");

    // The command has been reaped, so its id names nothing a later signal could reach.
    let late = reqwest::Client::new()
        .post(format!("{}/v1/interactive/signal", agent.base_url))
        .bearer_auth(agent.capability())
        .json(&json!({"id": "trap-1", "signal": "kill"}))
        .send()
        .await
        .expect("responds");
    assert_eq!(late.status(), 404);
}

/// Input larger than any pipe buffer: the command's pipe pushes back, the agent waits for it to
/// read, and every byte arrives in order. A command slow to start reading is the case a buffered
/// write would hide.
#[tokio::test]
async fn input_larger_than_a_pipe_buffer_arrives_whole() {
    let agent = Agent::start().await;
    let command = Interactive::start(&agent, "wc-1", &["/bin/sh", "-c", "sleep 1; wc -c"]).await;

    let data = vec![b'x'; 4 << 20];
    let mut offset = 0u64;
    while offset < data.len() as u64 {
        let end = (offset as usize + (256 << 10)).min(data.len());
        offset = command.accepted(offset, &data[offset as usize..end]).await;
    }
    command
        .post("/v1/interactive/close-stdin", json!({"id": "wc-1"}))
        .await;

    let (terminal, _, stdout) = command.finish().await;
    assert_eq!(terminal["code"], 0);
    assert_eq!(stdout.trim(), (4 << 20).to_string());
}

/// The rule `/v1/exec` already keeps, held for interactive commands too: a caller that stops
/// reading ends the command, rather than leaving it running for nobody.
#[tokio::test]
async fn a_departed_reader_ends_the_interactive_command() {
    let agent = Agent::start().await;
    let command = Interactive::start(&agent, "sleep-1", &["/bin/sleep", "30"]).await;
    assert_eq!(agent.state.interactive.len(), 1);

    drop(command);

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while !agent.state.interactive.is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the command outlived its reader"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// Every control names a command; one that is not running is a 404, never a silent success.
#[tokio::test]
async fn controls_for_an_unknown_command_are_refused() {
    let agent = Agent::start().await;
    let client = reqwest::Client::new();
    for (path, body) in [
        (
            "/v1/interactive/stdin",
            json!({"id": "nope", "generation": 0, "offset": 0, "data": ""}),
        ),
        ("/v1/interactive/close-stdin", json!({"id": "nope"})),
        (
            "/v1/interactive/signal",
            json!({"id": "nope", "signal": "interrupt"}),
        ),
    ] {
        let response = client
            .post(format!("{}{path}", agent.base_url))
            .bearer_auth(agent.capability())
            .json(&body)
            .send()
            .await
            .expect("responds");
        assert_eq!(response.status(), 404, "{path}");
    }
}

/// Interactive commands are reached only with the same capability every other operation needs.
#[tokio::test]
async fn starting_an_interactive_command_without_a_capability_is_refused() {
    let agent = Agent::start().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/interactive", agent.base_url))
        .json(&json!({"id": "x", "command": ["/bin/true"], "timeoutMs": 1000}))
        .send()
        .await
        .expect("responds");
    assert_eq!(response.status(), 401);
    assert!(agent.state.interactive.is_empty());
}

/// The declared preview ports are the process environment, so the tests that set them run one at a
/// time.
static PREVIEW_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A server standing in for the code a sandbox runs. Answers every request with `hello` and keeps
/// the head of each request it saw.
async fn upstream_server() -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
        .await
        .expect("bind upstream");
    let port = listener.local_addr().expect("address").port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let kept = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let kept = Arc::clone(&kept);
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 8192];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                kept.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[..read]).to_string());
                let _ = stream
                    .write_all(
                        b"HTTP/1.0 200 OK\r\nContent-Length: 5\r\nX-From: upstream\r\n\r\nhello",
                    )
                    .await;
            });
        }
    });
    (port, seen)
}

/// Preview reaches a declared port with a preview capability for exactly that port, and every
/// other combination is refused before anything is forwarded.
#[tokio::test]
async fn preview_forwards_only_a_declared_port_to_a_capability_for_that_port() {
    let _env = PREVIEW_ENV.lock().await;
    let agent = Agent::start().await;
    let (port, seen) = upstream_server().await;
    let (undeclared, undeclared_seen) = upstream_server().await;
    std::env::set_var("ALIEN_SANDBOX_PREVIEW_PORTS", format!("{port},8971"));
    let preview = |port: u16| {
        let mut preview = claims();
        preview.operation = SandboxOperationClass::Preview;
        preview.port = Some(port);
        agent.mint(preview)
    };
    let client = reqwest::Client::new();
    let get = |path: String, token: String| {
        let client = client.clone();
        let base = agent.base_url.clone();
        async move {
            client
                .get(format!("{base}{path}"))
                .bearer_auth(token)
                .send()
                .await
                .expect("request")
        }
    };

    let ok = get(format!("/v1/preview/{port}/a/b?x=1"), preview(port)).await;
    assert_eq!(ok.status(), 200);
    assert_eq!(ok.headers()["x-from"], "upstream");
    assert_eq!(ok.text().await.unwrap(), "hello");
    {
        let seen = seen.lock().unwrap();
        assert!(seen[0].starts_with("GET /a/b?x=1 HTTP/1.0"), "{}", seen[0]);
        assert!(
            !seen[0].to_ascii_lowercase().contains("authorization"),
            "the capability must not reach the code inside: {}",
            seen[0]
        );
    }

    // A capability for another port, for another class, for another pod, or already expired.
    let mut wrong_pod = claims();
    wrong_pod.operation = SandboxOperationClass::Preview;
    wrong_pod.port = Some(port);
    wrong_pod.session_id = "another-pod".to_string();
    let mut expired = claims();
    expired.operation = SandboxOperationClass::Preview;
    expired.port = Some(port);
    expired.expires_at = chrono::Utc::now().timestamp() - 1;
    for (label, token) in [
        ("a capability for another port", preview(port + 1)),
        ("an execute capability", agent.capability()),
        ("another pod's capability", agent.mint(wrong_pod)),
        ("an expired capability", agent.mint(expired)),
    ] {
        let refused = get(format!("/v1/preview/{port}/"), token).await;
        assert!(
            [401, 403].contains(&refused.status().as_u16()),
            "{label} must be refused, got {}",
            refused.status()
        );
    }
    // A port the sandbox did not declare, even with a capability minted for it.
    let refused = get(format!("/v1/preview/{undeclared}/"), preview(undeclared)).await;
    assert_eq!(refused.status(), 403);
    assert!(undeclared_seen.lock().unwrap().is_empty());
    // The agent's own port, even when declared and minted for.
    let refused = get("/v1/preview/8971/".to_string(), preview(8971)).await;
    assert_eq!(refused.status(), 403);
    // No credential at all.
    let anonymous = client
        .get(format!("{}/v1/preview/{port}/", agent.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), 401);
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "only the one valid request was forwarded"
    );
}

/// A WebSocket upgrade is allowed on a previewed port and is then a plain byte stream until the
/// capability expires: what the client writes reaches the sandbox's server and its answer comes
/// back.
#[tokio::test]
async fn preview_passes_an_upgrade_through_as_a_byte_stream() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _env = PREVIEW_ENV.lock().await;
    let agent = Agent::start().await;
    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
        .await
        .expect("bind upstream");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = vec![0u8; 4096];
        let _ = stream.read(&mut buffer).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n")
            .await
            .unwrap();
        let read = stream.read(&mut buffer).await.unwrap();
        let _ = stream.write_all(&buffer[..read]).await;
    });
    std::env::set_var("ALIEN_SANDBOX_PREVIEW_PORTS", port.to_string());
    let mut preview = claims();
    preview.operation = SandboxOperationClass::Preview;
    preview.port = Some(port);
    let token = agent.mint(preview);
    let address = agent.base_url.trim_start_matches("http://").to_string();
    let mut client = tokio::net::TcpStream::connect(&address).await.unwrap();
    client
        .write_all(
            format!(
                "GET /v1/preview/{port}/socket HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut head = vec![0u8; 512];
    let read = tokio::time::timeout(std::time::Duration::from_secs(5), client.read(&mut head))
        .await
        .expect("a response")
        .unwrap();
    assert!(String::from_utf8_lossy(&head[..read]).starts_with("HTTP/1.1 101"));
    client.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.read_exact(&mut echoed),
    )
    .await
    .expect("an echo")
    .unwrap();
    assert_eq!(&echoed, b"ping");
}

/// An agent in transport mode, the way a GCP sandbox runs: the cloud authorizes the control path
/// and the agent issues and verifies its own preview capabilities.
#[cfg(target_os = "linux")]
async fn transport_agent() -> (String, TempDir) {
    transport_agent_listening_on(&[]).await
}

/// A transport agent that records `own_ports` as the ports it itself listens on.
#[cfg(target_os = "linux")]
async fn transport_agent_listening_on(own_ports: &[u16]) -> (String, TempDir) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");
    let state = Arc::new(AgentState {
        image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
        gateway_egress: None,
        session_root: root,
        authorization: AgentAuthorization::Transport,
        exec_identity: ExecIdentity {
            uid: 60000,
            gid: 60000,
        },
        output_cap: 1 << 20,
        jobs: JobRegistry::new(),
        interactive: Default::default(),
        preview: Default::default(),
        agent_binary: agent_binary(),
    });
    state.preview.set_own_ports(own_ports);
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
    (format!("http://{address}"), dir)
}

#[cfg(target_os = "linux")]
async fn grant(base: &str, sandbox: &str, port: u16) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}/"))
        .json(&json!({"v": 1, "op": "previewGrant", "sandbox": sandbox, "port": port}))
        .send()
        .await
        .expect("responds")
}

/// The daemon behind a preview checks its own bearer token, so the caller's `Authorization` must
/// arrive unchanged, and the capability the agent verified must not.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn transport_preview_forwards_the_callers_authorization_and_hides_the_capability() {
    let _env = PREVIEW_ENV.lock().await;
    let (base, _dir) = transport_agent().await;
    let (port, seen) = upstream_server().await;
    let (other, _) = upstream_server().await;
    std::env::set_var(
        "ALIEN_SANDBOX_PREVIEW_PORTS",
        format!("{port},{other},8971"),
    );

    let granted: serde_json::Value = grant(&base, "sandbox-a", port).await.json().await.unwrap();
    let capability = granted["capability"].as_str().expect("a capability");
    assert!(granted["expiresAt"].as_i64().unwrap() > chrono::Utc::now().timestamp());

    let call = |port: u16, capability: Option<&str>, named: u16| {
        let mut request = reqwest::Client::new()
            .get(format!("{base}/v1/preview/{port}/a/b?x=1"))
            .header("authorization", "Bearer daemon-secret")
            .header("x-alien-preview-port", named.to_string())
            .header("x-sandbox-routing-token", "edge-token");
        if let Some(capability) = capability {
            request = request.header("x-alien-preview-capability", capability);
        }
        async move { request.send().await.expect("responds") }
    };

    let ok = call(port, Some(capability), port).await;
    assert_eq!(ok.status(), 200);
    assert_eq!(ok.text().await.unwrap(), "hello");
    let head = seen.lock().unwrap()[0].to_ascii_lowercase();
    assert!(head.contains("authorization: bearer daemon-secret"));
    assert!(head.starts_with("get /a/b?x=1 "));
    assert!(!head.contains("x-alien-preview"));
    assert!(!head.contains(&capability.to_ascii_lowercase()));
    assert!(!head.contains("x-sandbox-routing-token"));

    let forged = format!("v1.{}.{}", i64::MAX / 2, "00".repeat(32));
    for (what, response) in [
        ("no capability", call(port, None, port).await),
        ("a forged capability", call(port, Some(&forged), port).await),
        (
            "a capability for another port",
            call(other, Some(capability), other).await,
        ),
        (
            "a header naming another port",
            call(port, Some(capability), other).await,
        ),
        (
            "the agent's own port",
            call(8971, Some(capability), 8971).await,
        ),
    ] {
        assert!(
            matches!(response.status().as_u16(), 401 | 403),
            "{what}: {}",
            response.status()
        );
    }

    // One boot serves one sandbox, and the agent's own port is never granted.
    assert_eq!(grant(&base, "sandbox-b", port).await.status(), 403);
    assert_eq!(grant(&base, "sandbox-a", 8971).await.status(), 403);
    assert_eq!(seen.lock().unwrap().len(), 1, "refused calls reach nothing");
}

/// A backend whose template cannot carry the environment names the declared ports in the grant.
/// Only the ports named there are granted, and the capability then reaches the daemon.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn transport_preview_takes_declared_ports_from_the_grant_when_the_environment_has_none() {
    let _env = PREVIEW_ENV.lock().await;
    std::env::remove_var("ALIEN_SANDBOX_PREVIEW_PORTS");
    let (base, _dir) = transport_agent().await;
    let (port, seen) = upstream_server().await;
    let (other, _) = upstream_server().await;
    let client = reqwest::Client::new();
    let grant_with = |sandbox: &'static str, port: u16, declared: Vec<u16>| {
        let client = client.clone();
        let base = base.clone();
        async move {
            client
                .post(format!("{base}/"))
                .json(&json!({"v": 1, "op": "previewGrant", "sandbox": sandbox, "port": port, "declaredPorts": declared}))
                .send()
                .await
                .expect("responds")
        }
    };

    // Named in neither the environment nor the request: refused.
    assert_eq!(grant_with("sandbox-a", port, vec![]).await.status(), 403);
    // A port the request does not name is refused even when another one is.
    assert_eq!(
        grant_with("sandbox-a", other, vec![port]).await.status(),
        403
    );
    // The agent's own port is never grantable, whatever the request names.
    assert_eq!(
        grant_with("sandbox-a", 8971, vec![8971]).await.status(),
        403
    );

    let granted: serde_json::Value = grant_with("sandbox-a", port, vec![port])
        .await
        .json()
        .await
        .unwrap();
    let capability = granted["capability"].as_str().expect("a capability");
    let ok = client
        .get(format!("{base}/v1/preview/{port}/x"))
        .header("authorization", "Bearer daemon-secret")
        .header("x-alien-preview-port", port.to_string())
        .header("x-alien-preview-capability", capability)
        .send()
        .await
        .expect("responds");
    assert_eq!(ok.status(), 200);
    assert!(seen.lock().unwrap()[0]
        .to_ascii_lowercase()
        .contains("authorization: bearer daemon-secret"));
    // The capability is for that port only.
    let other_port = client
        .get(format!("{base}/v1/preview/{other}/x"))
        .header("x-alien-preview-port", other.to_string())
        .header("x-alien-preview-capability", capability)
        .send()
        .await
        .expect("responds");
    assert!(matches!(other_port.status().as_u16(), 401 | 403));
}

/// The caller puts its bearer under the header its backend names (`caller_authorization_header`).
/// Whichever it is, the daemon sees it as `Authorization` and never sees an `x-alien-*` header.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn transport_preview_delivers_the_callers_bearer_under_either_backend_header() {
    let _env = PREVIEW_ENV.lock().await;
    let (base, _dir) = transport_agent().await;
    let (port, seen) = upstream_server().await;
    std::env::set_var("ALIEN_SANDBOX_PREVIEW_PORTS", format!("{port},8971"));
    let granted: serde_json::Value = grant(&base, "sandbox-a", port).await.json().await.unwrap();
    let capability = granted["capability"].as_str().expect("a capability");
    let call = |headers: &[(&str, &str)]| {
        let mut request = reqwest::Client::new()
            .get(format!("{base}/v1/preview/{port}/"))
            .header("x-alien-preview-port", port.to_string())
            .header("x-alien-preview-capability", capability);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        async move { request.send().await.expect("responds") }
    };
    let last = || seen.lock().unwrap().last().unwrap().to_ascii_lowercase();

    // A backend whose header is `Authorization` (the default).
    std::env::remove_var("ALIEN_SANDBOX_CALLER_AUTHORIZATION_HEADER");
    assert_eq!(
        call(&[("authorization", "Bearer plain")]).await.status(),
        200
    );
    assert!(last().contains("authorization: bearer plain"));
    // Both headers at once is ambiguous: refused, and nothing reaches the daemon.
    let before = seen.lock().unwrap().len();
    let both = call(&[
        ("authorization", "Bearer one"),
        ("x-alien-caller-authorization", "Bearer two"),
    ])
    .await;
    assert_eq!(both.status(), 400);
    assert_eq!(seen.lock().unwrap().len(), before);

    // GCP: the edge consumed `Authorization`, so the caller's bearer is in the X-Alien header and
    // anything in `Authorization` is ignored.
    std::env::set_var(
        "ALIEN_SANDBOX_CALLER_AUTHORIZATION_HEADER",
        "x-alien-caller-authorization",
    );
    let ok = call(&[
        ("authorization", "Bearer edge-jwt"),
        ("x-alien-caller-authorization", "Bearer daemon-secret"),
    ])
    .await;
    assert_eq!(ok.status(), 200);
    let head = last();
    assert!(
        head.contains("authorization: bearer daemon-secret"),
        "{head}"
    );
    assert!(!head.contains("edge-jwt"), "{head}");
    assert!(!head.contains("x-alien-"), "{head}");
    // With no caller bearer, the daemon gets no Authorization at all (not the edge's).
    assert_eq!(
        call(&[("authorization", "Bearer edge-jwt")]).await.status(),
        200
    );
    assert!(!last().contains("authorization"), "{}", last());
    std::env::remove_var("ALIEN_SANDBOX_CALLER_AUTHORIZATION_HEADER");
}

/// An upgrade carries the caller's `Authorization` to the daemon too.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn transport_preview_upgrade_reaches_the_daemon_with_the_callers_authorization() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _env = PREVIEW_ENV.lock().await;
    let (base, _dir) = transport_agent().await;
    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
        .await
        .expect("bind upstream");
    let port = listener.local_addr().unwrap().port();
    let (head_tx, head_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = vec![0u8; 4096];
        let read = stream.read(&mut buffer).await.unwrap();
        let _ = head_tx.send(String::from_utf8_lossy(&buffer[..read]).to_string());
        stream
            .write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n")
            .await
            .unwrap();
        let read = stream.read(&mut buffer).await.unwrap();
        let _ = stream.write_all(&buffer[..read]).await;
    });
    std::env::set_var("ALIEN_SANDBOX_PREVIEW_PORTS", port.to_string());
    let granted: serde_json::Value = grant(&base, "sandbox-a", port).await.json().await.unwrap();
    let capability = granted["capability"].as_str().unwrap();
    let address = base.trim_start_matches("http://").to_string();
    let mut client = tokio::net::TcpStream::connect(&address).await.unwrap();
    client
        .write_all(
            format!(
                "GET /v1/preview/{port}/socket HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer daemon-secret\r\nX-Alien-Preview-Capability: {capability}\r\nX-Alien-Preview-Port: {port}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut head = vec![0u8; 512];
    let read = tokio::time::timeout(std::time::Duration::from_secs(5), client.read(&mut head))
        .await
        .expect("a response")
        .unwrap();
    assert!(String::from_utf8_lossy(&head[..read]).starts_with("HTTP/1.1 101"));
    client.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.read_exact(&mut echoed),
    )
    .await
    .expect("an echo")
    .unwrap();
    assert_eq!(&echoed, b"ping");
    let seen = head_rx.await.unwrap().to_ascii_lowercase();
    assert!(seen.contains("authorization: bearer daemon-secret"));
    assert!(!seen.contains("x-alien-preview"));
}

/// A refused preview call, plain or upgrade, must not open a connection to the daemon: not one
/// byte reaches it, and an upgrade is answered with a refusal, never a 101.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn transport_preview_refusals_never_connect_to_the_daemon() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _env = PREVIEW_ENV.lock().await;
    let (base, _dir) = transport_agent().await;
    // A second boot serving the same sandbox name has its own key.
    let (other_boot, _other_dir) = transport_agent().await;

    let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut ports = Vec::new();
    for _ in 0..2 {
        let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
            .await
            .expect("bind daemon stub");
        ports.push(listener.local_addr().unwrap().port());
        let accepted = accepted.clone();
        tokio::spawn(async move {
            loop {
                if listener.accept().await.is_ok() {
                    accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        });
    }
    let (port, other_port) = (ports[0], ports[1]);
    std::env::set_var(
        "ALIEN_SANDBOX_PREVIEW_PORTS",
        format!("{port},{other_port}"),
    );

    let capability_for = |base: String, port: u16, deadline: Option<i64>| async move {
        let mut body = json!({"v": 1, "op": "previewGrant", "sandbox": "sandbox-a", "port": port});
        if let Some(deadline) = deadline {
            body["deadline"] = json!(deadline);
        }
        let granted: serde_json::Value = reqwest::Client::new()
            .post(format!("{base}/"))
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        granted["capability"]
            .as_str()
            .expect("a capability")
            .to_string()
    };
    let good = capability_for(base.clone(), port, None).await;
    // Another boot, same sandbox name: valid MAC, wrong key.
    let from_other_boot = capability_for(other_boot, port, None).await;
    let short_lived =
        capability_for(base.clone(), port, Some(chrono::Utc::now().timestamp() + 1)).await;
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    let forged = format!("v1.{}.{}", i64::MAX / 2, "00".repeat(32));

    let cases: Vec<(&str, Option<String>, u16, u16)> = vec![
        ("missing", None, port, port),
        ("forged", Some(forged), port, port),
        ("expired", Some(short_lived), port, port),
        ("another boot", Some(from_other_boot), port, port),
        ("wrong port", Some(good.clone()), other_port, other_port),
        ("header names another port", Some(good), port, other_port),
    ];
    let address = base.trim_start_matches("http://").to_string();
    for (what, capability, path_port, named) in cases {
        for upgrade in [false, true] {
            let mut request = format!(
                "GET /v1/preview/{path_port}/socket HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer daemon-secret\r\nX-Alien-Preview-Port: {named}\r\nConnection: close\r\n"
            );
            if let Some(capability) = &capability {
                request.push_str(&format!("X-Alien-Preview-Capability: {capability}\r\n"));
            }
            if upgrade {
                request.push_str("Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n");
            }
            request.push_str("\r\n");
            let mut client = tokio::net::TcpStream::connect(&address).await.unwrap();
            client.write_all(request.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client.read_to_end(&mut response),
            )
            .await;
            let head = String::from_utf8_lossy(&response);
            let status = head.split_whitespace().nth(1).unwrap_or("");
            assert!(
                matches!(status, "401" | "403"),
                "{what} (upgrade={upgrade}): {head}"
            );
        }
    }
    assert_eq!(
        accepted.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "no refused call may open a connection to the daemon"
    );
}

/// The agent's own listener is not previewable even when the sandbox declares it: a forward
/// connects as the agent's uid, so it would reach the agent's API. A stub standing in for the
/// agent's port must see no connection from the grant or the route.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn transport_preview_never_reaches_the_agents_own_listener() {
    let _env = PREVIEW_ENV.lock().await;
    let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().expect("literal"))
        .await
        .expect("bind own-port stub");
    let own = listener.local_addr().unwrap().port();
    {
        let accepted = accepted.clone();
        tokio::spawn(async move {
            loop {
                if listener.accept().await.is_ok() {
                    accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        });
    }
    let (base, _dir) = transport_agent_listening_on(&[own]).await;
    std::env::set_var("ALIEN_SANDBOX_PREVIEW_PORTS", format!("{own},8971"));

    for port in [own, 8971] {
        assert_eq!(
            grant(&base, "sandbox-a", port).await.status(),
            403,
            "{port}"
        );
        // A capability minted under another configuration is refused by the route too.
        let forged = format!("v1.{}.{}", i64::MAX / 2, "00".repeat(32));
        let response = reqwest::Client::new()
            .get(format!("{base}/v1/preview/{port}/"))
            .header("x-alien-preview-port", port.to_string())
            .header("x-alien-preview-capability", forged)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403, "route {port}");
    }
    assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 0);
}
