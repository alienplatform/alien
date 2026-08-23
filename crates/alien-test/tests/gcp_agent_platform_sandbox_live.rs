//! The GCP Agent Platform sandbox backend, driven against a real project.
//!
//! Every test here is `#[ignore]`d because it provisions real reasoning engines, templates, and
//! sandboxes and needs GCP credentials. **CI does not run `--ignored`, so none of these run in
//! CI** — they are the manual live gate that a mocked test cannot stand in for: a mock can only
//! confirm the request we chose to send, never that the real API accepts it or that a reconnect
//! actually reaches the same container.
//!
//! The Agent Platform emitter and controller are not yet wired into the provider factory, so a
//! live test cannot go through a deployed stack. It drives the client and provider directly, as
//! the proof-of-concept scripts did: create an engine, create a template from a prebuilt agent
//! image, then exercise the `Sandbox` trait against sandboxes cut from it. That means these tests
//! prove the runtime path, not the controller's template-body mapping — the inline template body
//! below mirrors the controller's `build_template_body` so it at least proves the real API accepts
//! that shape.
//!
//! Run the full suite (single-threaded, because sandbox quota is pooled per project + location):
//!
//! ```text
//! GOOGLE_TARGET_PROJECT_ID=... \
//! GOOGLE_TARGET_REGION=us-central1 \
//! GOOGLE_TARGET_SERVICE_ACCOUNT_KEY="$(cat key.json)" \
//! ALIEN_TEST_GCP_AGENT_IMAGE=<region>-docker.pkg.dev/<project>/<repo>/agent:<tag> \
//! ALIEN_TEST_GIT_TOKEN=<pat> \
//! ALIEN_TEST_PRIVATE_REPO=<owner>/<repo> \
//!   cargo test -p alien-test --test gcp_agent_platform_sandbox_live -- --ignored --test-threads=1
//! ```
//!
//! The agent image must be a prebuilt `alien-sandbox-agent` image in a registry the project can
//! pull, with `git` on its PATH for the clone tests. Teardown deletes the engine on every exit
//! path including a panic, which cascades its templates and sandboxes; `sweep_orphaned_engines`
//! reaps engines that a hard-killed run recorded but could not delete. An engine killed in the
//! window between create resolving and being recorded cannot be swept without an engine-list verb,
//! which this backend does not expose.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;

use alien_bindings::providers::sandbox::gcp_agent_platform::GcpAgentPlatformSandbox;
use alien_bindings::traits::{
    CommandOutput, CreateSessionRequest, RunCommandRequest, Sandbox, SandboxSessionState,
};
use alien_core::{GcpClientConfig, GcpCredentials};
use alien_gcp_clients::gcp::agent_platform::{
    AgentPlatformApi, AgentPlatformClient, ContainerResources, CustomContainerEnvironment,
    CustomContainerSpec, EgressControlConfig, PollBudget, ReasoningEngine, SandboxCreateRequest,
    SandboxEnvironment, SandboxEnvironmentTemplate,
};

// ---- Configuration and clients ----------------------------------------------------------------

const HANDOFF_ENV: &str = "ALIEN_SANDBOX_LIVE_RECONNECT";
/// Display-name prefix on every engine and template this suite creates, so the sweep can find an
/// orphan the scratch log never recorded.
const LIVE_PREFIX: &str = "alien-sbx-live-";
const TTL_SECONDS: u32 = 3600;

/// The credentials a client needs, from the same `GOOGLE_TARGET_*` variables the rest of the E2E
/// harness uses. The agent image is read separately by [`agent_image`] so a process that only
/// reconnects — the reconnect child — does not have to supply an image it never provisions from.
struct LiveConfig {
    project_id: String,
    region: String,
    credentials_json: String,
}

fn require_env(key: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("{key} must be set to run this live test; see the module docs"))
}

/// The prebuilt agent image a template is cut from. Separate from [`LiveConfig`] because only the
/// provisioning tests need it.
fn agent_image() -> String {
    require_env("ALIEN_TEST_GCP_AGENT_IMAGE")
}

impl LiveConfig {
    /// Fails loudly on a missing variable rather than skipping: a live test that quietly passes
    /// with nothing set is the false PASS this suite exists to rule out.
    fn from_env() -> Self {
        LiveConfig {
            project_id: require_env("GOOGLE_TARGET_PROJECT_ID"),
            region: require_env("GOOGLE_TARGET_REGION"),
            credentials_json: require_env("GOOGLE_TARGET_SERVICE_ACCOUNT_KEY"),
        }
    }

    fn client(&self) -> Arc<AgentPlatformClient> {
        let config = GcpClientConfig {
            project_id: self.project_id.clone(),
            region: self.region.clone(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: self.credentials_json.clone(),
            },
            service_overrides: None,
            project_number: None,
        };
        // A per-request timeout, comfortably above the ~30s :execute proxy window: without one a
        // single stalled request hangs the whole test forever, since the poll budget bounds the
        // loop but not one call.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()
            .expect("a client with a request timeout builds");
        Arc::new(AgentPlatformClient::new(http, config))
    }
}

/// Generous against real provisioning: the POC measured ~113s to a template reaching `ACTIVE`.
fn budget() -> PollBudget {
    PollBudget {
        interval: Duration::from_secs(2),
        max_attempts: 150,
    }
}

fn last_segment(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

// ---- Provisioning and teardown ----------------------------------------------------------------

/// Deletes the engine on every exit path, panic included, so an assertion failure does not leak a
/// running engine. The delete runs on a throwaway thread with its own runtime because `Drop` is
/// synchronous; deleting the engine cascades its templates and sandboxes, and a not-found is
/// already success in the client.
struct EngineGuard {
    client: Arc<AgentPlatformClient>,
    engine: String,
}

impl Drop for EngineGuard {
    fn drop(&mut self) {
        let client = self.client.clone();
        let engine = last_segment(&self.engine).to_string();
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("teardown runtime builds");
            runtime.block_on(async move {
                // An engine will not delete while it still has child sandboxes, and a panicked test
                // leaves its session behind. Reap the sandboxes and retry: delete_sandbox only
                // starts the removal, so the engine delete has to wait for them to be gone.
                for _ in 0..6 {
                    if let Ok(sandboxes) = client.list_sandboxes(&engine).await {
                        for sandbox in &sandboxes {
                            if let Some(name) = sandbox.name.as_deref() {
                                let _ = client.delete_sandbox(&engine, last_segment(name)).await;
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    if client.delete_engine(&engine).await.is_ok() {
                        return;
                    }
                }
                eprintln!("teardown: could not delete engine {engine} after reaping its sandboxes");
            });
        })
        .join();
    }
}

async fn provision_engine(client: &Arc<AgentPlatformClient>) -> String {
    let display = format!("{LIVE_PREFIX}{}", uuid::Uuid::new_v4().simple());
    let operation = client
        .create_engine(&display)
        .await
        .expect("engine create accepted");
    let engine: ReasoningEngine = client
        .await_operation(&operation, budget())
        .await
        .expect("engine create operation resolves");
    engine
        .name
        .expect("a created engine carries a resource name")
}

/// Builds the immutable template body, mirroring the controller's `build_template_body` shape so a
/// live run proves the real API accepts the same body the controller would send.
fn template_body(image: &str, internet_access: bool) -> SandboxEnvironmentTemplate {
    SandboxEnvironmentTemplate {
        name: None,
        display_name: Some(format!("{LIVE_PREFIX}{}", uuid::Uuid::new_v4().simple())),
        custom_container_environment: Some(CustomContainerEnvironment {
            custom_container_spec: Some(CustomContainerSpec {
                image_uri: image.to_string(),
                extra: Default::default(),
            }),
            resources: Some(ContainerResources {
                requests: None,
                limits: Some(HashMap::from([
                    ("cpu".to_string(), "2".to_string()),
                    ("memory".to_string(), "4Gi".to_string()),
                ])),
            }),
            ports: vec![],
            extra: Default::default(),
        }),
        egress_control_config: Some(EgressControlConfig {
            internet_access: Some(internet_access),
            extra: Default::default(),
        }),
        state: None,
        extra: Default::default(),
    }
}

/// Creates a template under `engine` and waits for it to reach `ACTIVE`, returning its full name.
async fn provision_template(
    client: &Arc<AgentPlatformClient>,
    engine: &str,
    image: &str,
    internet_access: bool,
) -> String {
    let engine_seg = last_segment(engine);
    let operation = client
        .create_template(engine_seg, template_body(image, internet_access))
        .await
        .expect("template create accepted");
    let created: SandboxEnvironmentTemplate = client
        .await_operation(&operation, budget())
        .await
        .expect("template create operation resolves");
    let name = created
        .name
        .expect("a created template carries a resource name");
    client
        .await_template_active(engine_seg, last_segment(&name), budget())
        .await
        .expect("the template reaches ACTIVE");
    name
}

fn provider(
    client: &Arc<AgentPlatformClient>,
    engine: &str,
    template: &str,
) -> GcpAgentPlatformSandbox {
    GcpAgentPlatformSandbox::new(
        client.clone(),
        engine.to_string(),
        template.to_string(),
        Some(TTL_SECONDS),
    )
}

// ---- Command helpers --------------------------------------------------------------------------

struct CommandResult {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: i32,
}

/// Drives a command to its exit, collecting the decoded streams. Asserting on this — never on the
/// transport envelope — is what keeps an empty probe from reading as a pass.
async fn run(
    provider: &GcpAgentPlatformSandbox,
    session: &str,
    argv: &[&str],
    env: BTreeMap<String, String>,
    deadline: Duration,
) -> CommandResult {
    let mut stream = provider
        .run_command(
            session,
            RunCommandRequest {
                command: argv.iter().map(|arg| arg.to_string()).collect(),
                working_directory: None,
                env,
                deadline,
            },
        )
        .await
        .expect("the command starts");

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_code = None;
    while let Some(frame) = stream.next().await {
        match frame.expect("a command frame decodes") {
            CommandOutput::Stdout { data, .. } => stdout.extend_from_slice(&data),
            CommandOutput::Stderr { data, .. } => stderr.extend_from_slice(&data),
            CommandOutput::Exit { code, .. } => exit_code = Some(code),
        }
    }

    CommandResult {
        stdout,
        stderr,
        exit_code: exit_code.expect("exactly one terminal exit frame arrives"),
    }
}

async fn shell(provider: &GcpAgentPlatformSandbox, session: &str, script: &str) -> CommandResult {
    run(
        provider,
        session,
        &["/bin/sh", "-lc", script],
        BTreeMap::new(),
        Duration::from_secs(20),
    )
    .await
}

async fn wait_until_running(provider: &GcpAgentPlatformSandbox, session: &str) -> u64 {
    for _ in 0..60 {
        if let Some(found) = provider.get(session).await.expect("get answers") {
            if found.state == SandboxSessionState::Running {
                return found.generation;
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("session {session} never reached Running");
}

// ---- The mandatory flow -----------------------------------------------------------------------

/// create → exec → reconnect from a second process → private clone → terminate.
///
/// The one flow the POC left half-open. Every step asserts on decoded content; the reconnect step
/// is a genuinely separate process, because an in-process reconnect only proves the client agrees
/// with itself.
#[tokio::test]
#[ignore = "requires a real GCP project; see module docs"]
async fn create_exec_reconnect_private_clone_terminate() {
    let config = LiveConfig::from_env();
    // Required before any provisioning, so the run fails on setup rather than after a live engine
    // exists: the private clone is part of this flow, not an optional extra.
    let git_token = require_env("ALIEN_TEST_GIT_TOKEN");
    let private_repo = require_env("ALIEN_TEST_PRIVATE_REPO");

    let client = config.client();
    let engine = provision_engine(&client).await;
    record_engine(&engine);
    let _guard = EngineGuard {
        client: client.clone(),
        engine: engine.clone(),
    };
    let template = provision_template(&client, &engine, &agent_image(), true).await;
    let provider = provider(&client, &engine, &template);

    let session = provider
        .create(CreateSessionRequest::default())
        .await
        .expect("create reaches a running, agent-answering session");
    assert_eq!(session.state, SandboxSessionState::Running);
    let sid = session.session_id.clone();

    let marker = format!("alien-sbx-live-{}", uuid::Uuid::new_v4().simple());
    let wrote = shell(
        &provider,
        &sid,
        &format!("mkdir -p /sandbox/session && printf %s '{marker}' > /sandbox/session/marker"),
    )
    .await;
    assert_eq!(
        wrote.exit_code, 0,
        "writing the marker succeeds: {:?}",
        wrote.stderr
    );
    let read_back = provider
        .read_file(&sid, "/session/marker")
        .await
        .expect("the marker file reads back");
    assert_eq!(
        read_back,
        marker.as_bytes(),
        "the write is visible to a read"
    );

    reconnect_from_a_second_process(&engine, &template, &sid, &marker, session.generation);

    // The private clone proves the token path specifically; a public clone would only prove the
    // network path, so the token/repo are required rather than substituted.
    let clone = run(
        &provider,
        &sid,
        &[
            "/bin/sh",
            "-lc",
            "git clone --depth 1 \"https://x-access-token:${GIT_TOKEN}@github.com/${PRIVATE_REPO}.git\" /sandbox/priv >/sandbox/clone.log 2>&1; echo rc=$?",
        ],
        BTreeMap::from([
            ("GIT_TOKEN".to_string(), git_token),
            ("PRIVATE_REPO".to_string(), private_repo),
        ]),
        Duration::from_secs(25),
    )
    .await;
    assert_eq!(clone.exit_code, 0, "the clone command runs");
    assert!(
        String::from_utf8_lossy(&clone.stdout).contains("rc=0"),
        "the private clone succeeds: {}",
        String::from_utf8_lossy(&clone.stdout)
    );
    let head = provider
        .read_file(&sid, "/priv/.git/HEAD")
        .await
        .expect("the cloned repo has a git dir");
    assert!(
        String::from_utf8_lossy(&head).contains("ref:"),
        "the clone produced a real working tree"
    );

    provider
        .terminate(&sid)
        .await
        .expect("terminate polls the session to gone");
    assert!(
        provider.get(&sid).await.expect("get answers").is_none(),
        "a terminated session is gone, not merely requested gone"
    );
}

// ---- The two-process reconnect ----------------------------------------------------------------

/// Re-execs this test binary at [`reconnect_reader_child`], handing it only a file of resource
/// names — no shared memory. The child, a fresh process with a fresh client, must read the marker
/// and match the container generation, then write a proof file naming a nonce only this process
/// knows. The capability verdict lives here, in the proof check, which is why the child no-ops
/// harmlessly when run on its own.
fn reconnect_from_a_second_process(
    engine: &str,
    template: &str,
    sid: &str,
    marker: &str,
    generation: u64,
) {
    let dir = std::env::temp_dir();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let proof_path = dir.join(format!("alien-sbx-live-proof-{nonce}"));
    let handoff_path = dir.join(format!("alien-sbx-live-handoff-{nonce}"));

    let handoff = serde_json::json!({
        "engine": engine,
        "template": template,
        "sandbox": sid,
        "marker": marker,
        "generation": generation,
        "nonce": nonce,
        "proofPath": proof_path.to_string_lossy(),
    });
    std::fs::write(&handoff_path, handoff.to_string()).expect("the handoff file writes");

    let exe = std::env::current_exe().expect("the test binary path");
    let status = std::process::Command::new(exe)
        .args([
            "--exact",
            "reconnect_reader_child",
            "--ignored",
            "--nocapture",
        ])
        .env(HANDOFF_ENV, &handoff_path)
        .status()
        .expect("the reader process starts");
    assert!(
        status.success(),
        "the reconnect reader process failed its own assertions"
    );

    let proof = std::fs::read_to_string(&proof_path).expect("the reader wrote a proof file");
    assert!(
        proof.contains(&nonce) && proof.contains(marker),
        "the reader proved it read the marker in this run, not a stale one: {proof}"
    );

    let _ = std::fs::remove_file(&handoff_path);
    let _ = std::fs::remove_file(&proof_path);
}

/// Process two. When [`HANDOFF_ENV`] is unset it is not the child — it no-ops, because the
/// reconnect verdict is owned by the parent's proof-file check, not by this test running alone.
#[tokio::test]
#[ignore = "the second process of the reconnect test; the parent launches it"]
async fn reconnect_reader_child() {
    let Some(handoff_path) = std::env::var_os(HANDOFF_ENV) else {
        eprintln!("{HANDOFF_ENV} unset; not the reconnect child, nothing to do");
        return;
    };

    let raw = std::fs::read_to_string(&handoff_path).expect("the handoff file reads");
    let handoff: serde_json::Value = serde_json::from_str(&raw).expect("the handoff is JSON");
    let engine = handoff["engine"].as_str().expect("engine name");
    let template = handoff["template"].as_str().expect("template name");
    let sid = handoff["sandbox"].as_str().expect("sandbox id");
    let marker = handoff["marker"].as_str().expect("marker");
    let generation = handoff["generation"].as_u64().expect("generation");
    let nonce = handoff["nonce"].as_str().expect("nonce");
    let proof_path = handoff["proofPath"].as_str().expect("proof path");

    // A fresh client built from the environment, not handed across from process one.
    let client = LiveConfig::from_env().client();
    let provider = provider(&client, engine, template);

    let session = provider
        .get(sid)
        .await
        .expect("get answers")
        .expect("the sandbox is still present for the second process");
    assert_eq!(
        session.state,
        SandboxSessionState::Running,
        "the reconnected session is running"
    );
    assert_eq!(
        session.generation, generation,
        "the same container answers process two — its generation matches process one's"
    );

    let seen = provider
        .read_file(sid, "/session/marker")
        .await
        .expect("process two reads process one's file");
    assert_eq!(
        seen,
        marker.as_bytes(),
        "process two sees the exact bytes process one wrote"
    );

    // Not just a read: a second process can still mutate the same filesystem.
    let appended = shell(
        &provider,
        sid,
        "printf ' second' >> /sandbox/session/marker && cat /sandbox/session/marker",
    )
    .await;
    assert_eq!(
        appended.exit_code, 0,
        "process two mutates the shared filesystem"
    );
    assert!(
        String::from_utf8_lossy(&appended.stdout).contains("second"),
        "the mutation is visible"
    );

    // The proof the parent verifies: only a process that actually read the marker in this run can
    // write both the nonce and the marker it read.
    std::fs::write(
        proof_path,
        format!("{nonce}:{}", String::from_utf8_lossy(&seen)),
    )
    .expect("the proof file writes");
}

// ---- The proxy cap ----------------------------------------------------------------------------

/// A command longer than the ~30s `:execute` proxy cap completes via the detached job path.
///
/// This is the single biggest difference from AWS: one synchronous execute cannot carry the work,
/// so the provider must detach and poll. A mocked test cannot see the real cap.
#[tokio::test]
#[ignore = "requires a real GCP project; spends ~40s of wall-clock against the proxy cap"]
async fn a_command_past_the_proxy_cap_completes_detached() {
    let config = LiveConfig::from_env();
    let client = config.client();
    let engine = provision_engine(&client).await;
    record_engine(&engine);
    let _guard = EngineGuard {
        client: client.clone(),
        engine: engine.clone(),
    };
    let template = provision_template(&client, &engine, &agent_image(), true).await;
    let provider = provider(&client, &engine, &template);

    let session = provider
        .create(CreateSessionRequest::default())
        .await
        .expect("create succeeds");
    let sid = session.session_id.clone();

    // A deadline past the synchronous window forces the provider onto the detached path; the
    // command sleeps well past the ~30s cap and must still report its output and exit.
    let result = run(
        &provider,
        &sid,
        &["/bin/sh", "-lc", "echo start; sleep 40; echo end"],
        BTreeMap::new(),
        Duration::from_secs(90),
    )
    .await;
    assert_eq!(
        result.exit_code, 0,
        "a 40s command exits cleanly, not at a cap"
    );
    let out = String::from_utf8_lossy(&result.stdout);
    assert!(
        out.contains("start") && out.contains("end"),
        "both the pre- and post-sleep output survive the detached poll: {out}"
    );

    provider
        .terminate(&sid)
        .await
        .expect("terminate confirms gone");
}

// ---- Capability rows measured live ------------------------------------------------------------

/// `suspendResume`: a suspended session resumes onto the same container with its filesystem intact.
#[tokio::test]
#[ignore = "requires a real GCP project; see module docs"]
async fn suspend_resume_preserves_the_container_and_filesystem() {
    let config = LiveConfig::from_env();
    let client = config.client();
    let engine = provision_engine(&client).await;
    record_engine(&engine);
    let _guard = EngineGuard {
        client: client.clone(),
        engine: engine.clone(),
    };
    let template = provision_template(&client, &engine, &agent_image(), true).await;
    let provider = provider(&client, &engine, &template);

    let session = provider
        .create(CreateSessionRequest::default())
        .await
        .expect("create succeeds");
    let sid = session.session_id.clone();
    let before = session.generation;

    let marker = format!("mark-{}", uuid::Uuid::new_v4().simple());
    let wrote = shell(
        &provider,
        &sid,
        &format!("printf %s '{marker}' > /sandbox/keep"),
    )
    .await;
    assert_eq!(wrote.exit_code, 0, "the pre-suspend marker writes");

    provider.suspend(&sid).await.expect("the session suspends");
    provider.resume(&sid).await.expect("the session resumes");

    let after = wait_until_running(&provider, &sid).await;
    // The load-bearing guarantee is that the filesystem survives. Resume may return onto a
    // reissued container with a fresh boot id — the generation is derived from it precisely so a
    // caller detects that — so the generation is observed, not asserted to be unchanged.
    eprintln!("suspend/resume generation: before={before} after={after}");
    let kept = provider
        .read_file(&sid, "/keep")
        .await
        .expect("the marker survives the suspend/resume");
    assert_eq!(
        kept,
        marker.as_bytes(),
        "the filesystem is intact across suspend/resume"
    );

    provider
        .terminate(&sid)
        .await
        .expect("terminate confirms gone");
}

/// `egressDeny`: a `deny` template closes the network — the connection fails and DNS with it.
#[tokio::test]
#[ignore = "requires a real GCP project; see module docs"]
async fn egress_deny_blocks_the_network_including_dns() {
    let config = LiveConfig::from_env();
    let client = config.client();
    let engine = provision_engine(&client).await;
    record_engine(&engine);
    let _guard = EngineGuard {
        client: client.clone(),
        engine: engine.clone(),
    };
    // The template is immutable and carries the egress switch, so a closed network needs its own
    // template rather than a flag on a command.
    let template = provision_template(&client, &engine, &agent_image(), false).await;
    let provider = provider(&client, &engine, &template);

    let session = provider
        .create(CreateSessionRequest::default())
        .await
        .expect("create succeeds");
    let sid = session.session_id.clone();

    // DNS alone, and the resolver's own exit code is captured so a missing binary (127) cannot be
    // mistaken for a blocked network — that mistake is exactly the false PASS this row must avoid.
    let resolve = shell(&provider, &sid, "getent hosts github.com >/dev/null 2>&1; echo rc=$?").await;
    assert_eq!(resolve.exit_code, 0, "the probe wrapper itself runs");
    let stdout = String::from_utf8_lossy(&resolve.stdout);
    let rc: i32 = stdout
        .trim()
        .strip_prefix("rc=")
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("the probe reported no resolver exit code: {stdout}"));
    assert_ne!(rc, 127, "the resolver must exist, so a nonzero code is a blocked network, not a missing binary");
    assert_ne!(rc, 0, "a closed sandbox cannot resolve github.com");

    provider
        .terminate(&sid)
        .await
        .expect("terminate confirms gone");
}

/// Snapshot **restore**: a sandbox restored from a snapshot carries the pre-snapshot filesystem and
/// not a mutation made after the snapshot. Both halves are asserted — one alone proves nothing.
///
/// Restore has no trait verb (`create` hardcodes no snapshot), so it goes through the client
/// directly, which is the only path that can restore today.
#[tokio::test]
#[ignore = "requires a real GCP project; see module docs"]
async fn snapshot_restore_carries_pre_snapshot_state_only() {
    let config = LiveConfig::from_env();
    let client = config.client();
    let engine = provision_engine(&client).await;
    record_engine(&engine);
    let _guard = EngineGuard {
        client: client.clone(),
        engine: engine.clone(),
    };
    let template = provision_template(&client, &engine, &agent_image(), true).await;
    let provider = provider(&client, &engine, &template);

    let session = provider
        .create(CreateSessionRequest::default())
        .await
        .expect("create succeeds");
    let sid = session.session_id.clone();

    let before = format!("before-{}", uuid::Uuid::new_v4().simple());
    assert_eq!(
        shell(
            &provider,
            &sid,
            &format!("printf %s '{before}' > /sandbox/before")
        )
        .await
        .exit_code,
        0,
        "the pre-snapshot marker writes"
    );

    let snapshot = provider
        .snapshot(&sid)
        .await
        .expect("a snapshot is captured");

    // A mutation the restore must not carry.
    assert_eq!(
        shell(&provider, &sid, "printf %s after > /sandbox/after")
            .await
            .exit_code,
        0,
        "the post-snapshot marker writes"
    );

    let engine_seg = last_segment(&engine);
    let operation = client
        .create_sandbox(
            engine_seg,
            SandboxCreateRequest {
                display_name: Some(format!("restore-{}", uuid::Uuid::new_v4().simple())),
                sandbox_environment_template: None,
                sandbox_environment_snapshot: Some(snapshot),
                ttl: Some(format!("{TTL_SECONDS}s")),
            },
        )
        .await
        .expect("restore create accepted");
    let restored: SandboxEnvironment = client
        .await_operation(&operation, budget())
        .await
        .expect("restore create resolves");
    let restored_id = last_segment(&restored.name.expect("the restore carries a name")).to_string();
    wait_until_running(&provider, &restored_id).await;

    let carried = provider
        .read_file(&restored_id, "/before")
        .await
        .expect("the restore carries the pre-snapshot marker");
    assert_eq!(
        carried,
        before.as_bytes(),
        "the pre-snapshot state is present"
    );
    assert!(
        provider
            .read_file(&restored_id, "/after")
            .await
            .is_err(),
        "the post-snapshot mutation is absent from the restore"
    );

    provider
        .terminate(&restored_id)
        .await
        .expect("the restore tears down");
    provider
        .terminate(&sid)
        .await
        .expect("the source tears down");
}

// ---- Orphan sweep -----------------------------------------------------------------------------

fn sweep_log() -> PathBuf {
    std::env::temp_dir().join("alien-sbx-live-engines.log")
}

/// Records an engine name the instant it exists, so a run killed before its guard runs still leaves
/// a trail the sweep can reap.
fn record_engine(engine: &str) {
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(sweep_log())
    {
        let _ = writeln!(file, "{engine}");
    }
}

/// Deletes every engine a prior live run recorded, tolerating not-found. This is the sweep for
/// orphans a failed run left behind; a completed run's engine is already gone and its line is a
/// harmless not-found here.
#[tokio::test]
#[ignore = "requires a real GCP project; reaps engines recorded by failed live runs"]
async fn sweep_orphaned_engines() {
    let client = LiveConfig::from_env().client();
    let recorded = std::fs::read_to_string(sweep_log()).unwrap_or_default();

    // Two sources, deduped: engines a failed run recorded, and engines the API still lists under
    // this suite's display-name prefix. The second catches one killed before it was ever recorded —
    // the gap a log-only sweep leaves. Only this suite's prefix is reaped, never a stray engine.
    let mut targets: BTreeSet<String> = recorded
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| last_segment(line).to_string())
        .collect();
    for engine in client.list_engines().await.expect("listing engines to sweep") {
        let matches_suite = engine
            .display_name
            .as_deref()
            .is_some_and(|name| name.starts_with(LIVE_PREFIX));
        if let (true, Some(name)) = (matches_suite, engine.name.as_deref()) {
            targets.insert(last_segment(name).to_string());
        }
    }

    let mut failures = Vec::new();
    for engine in &targets {
        if let Err(error) = client.delete_engine(engine).await {
            failures.push(format!("{engine}: {error}"));
        }
    }
    let _ = std::fs::remove_file(sweep_log());

    assert!(
        failures.is_empty(),
        "every orphan engine must be gone after a sweep; still present: {failures:?}"
    );
}
