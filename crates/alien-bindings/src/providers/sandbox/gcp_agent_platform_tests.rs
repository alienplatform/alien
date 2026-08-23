use super::*;
use alien_gcp_clients::gcp::agent_platform::{MockAgentPlatformApi, SandboxEnvironmentTemplate};
use futures::StreamExt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The client's own `Result`, distinct from the binding's `Result` that `super::*` brings in.
type ClientResult<T> = alien_error::Result<T, AgentPlatformErrorData>;

// ---- Fixtures ---------------------------------------------------------------------------------

const ENGINE_FULL: &str = "projects/p/locations/us-central1/reasoningEngines/eng1";
const TEMPLATE: &str = "projects/p/locations/us-central1/sandboxTemplates/agent";

fn provider(client: MockAgentPlatformApi) -> GcpAgentPlatformSandbox {
    provider_from(Arc::new(client))
}

fn provider_from(client: Arc<dyn AgentPlatformApi>) -> GcpAgentPlatformSandbox {
    GcpAgentPlatformSandbox::new(client, ENGINE_FULL.to_string(), TEMPLATE.to_string(), Some(3600))
}

fn sandbox_name(id: &str) -> String {
    format!("{ENGINE_FULL}/sandboxEnvironments/{id}")
}

fn sandbox_in_state(id: &str, state: &str) -> SandboxEnvironment {
    SandboxEnvironment {
        name: Some(sandbox_name(id)),
        display_name: None,
        state: Some(state.to_string()),
        sandbox_environment_template: None,
        expire_time: None,
        connection_info: None,
        extra: Default::default(),
    }
}

/// A completed operation whose response is `value`.
fn done_op(value: serde_json::Value) -> Operation {
    Operation {
        name: Some("projects/p/locations/us-central1/operations/op1".to_string()),
        metadata: None,
        done: Some(true),
        result: Some(OperationResult::Response { response: value }),
    }
}

fn op_of(input: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(input)
        .ok()
        .and_then(|value| value.get("op").and_then(|op| op.as_str()).map(str::to_string))
        .unwrap_or_default()
}

fn ndjson(lines: &[serde_json::Value]) -> Vec<u8> {
    let mut body = Vec::new();
    for line in lines {
        body.extend_from_slice(serde_json::to_string(line).expect("frame serializes").as_bytes());
        body.push(b'\n');
    }
    body
}

fn health_reply() -> Vec<u8> {
    health_reply_with_boot("11111111-1111-1111-1111-111111111111")
}

fn health_reply_with_boot(boot_id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "protocolVersion": 1, "bootId": boot_id }))
        .expect("health serializes")
}

fn stdout_frame(seq: u64, data: &[u8]) -> serde_json::Value {
    serde_json::json!({ "t": "stdout", "seq": seq, "data": BASE64.encode(data) })
}

fn exit_frame(code: i32) -> serde_json::Value {
    serde_json::json!({ "t": "exit", "code": code, "truncated": false })
}

/// The client-shaped not-found: a `RemoteResourceNotFound` wrapped as `RequestFailed`, matching how
/// the real client reports an absent sandbox.
fn not_found() -> AlienError<AgentPlatformErrorData> {
    AlienError::new(alien_client_core::ErrorData::RemoteResourceNotFound {
        resource_type: "SandboxEnvironment".to_string(),
        resource_name: "s1".to_string(),
    })
    .context(AgentPlatformErrorData::RequestFailed {
        operation: "get sandbox".to_string(),
        message: "s1".to_string(),
    })
}

fn execute_refused() -> AlienError<AgentPlatformErrorData> {
    AlienError::new(AgentPlatformErrorData::ExecuteFailed {
        sandbox: "s1".to_string(),
        message: "the API rejected the request".to_string(),
    })
}

// ---- create -----------------------------------------------------------------------------------

/// create awaits RUNNING, probes the agent, and pins the three arguments that reach the client:
/// the engine reduced to a bare segment, the template unchanged, and the ttl as a duration.
#[tokio::test]
async fn create_awaits_running_probes_the_agent_and_pins_its_arguments() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_create_sandbox()
        .withf(|engine, request| {
            engine == "eng1"
                && request.sandbox_environment_template.as_deref() == Some(TEMPLATE)
                && request.ttl.as_deref() == Some("3600s")
        })
        .times(1)
        .returning(|_, _| Ok(done_op(serde_json::json!({ "name": sandbox_name("s1") }))));
    client
        .expect_get_sandbox()
        .withf(|engine, sandbox| engine == "eng1" && sandbox == "s1")
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .withf(|_, sandbox, input| sandbox == "s1" && op_of(input) == "health")
        .returning(|_, _, _| Ok(health_reply()));

    let session = provider(client)
        .create(CreateSessionRequest::default())
        .await
        .expect("create succeeds");

    assert_eq!(session.session_id, "s1");
    assert_eq!(session.state, SandboxSessionState::Running);
}

/// Delete-on-create-failure: a probe the agent never answers deletes the sandbox the caller never
/// received, through the one discard path. Mutation check: drop the `discard` call in `create` and
/// this test's `expect_delete_sandbox().times(1)` goes unmet.
#[tokio::test]
async fn create_deletes_the_sandbox_when_its_agent_never_answers() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_create_sandbox()
        .returning(|_, _| Ok(done_op(serde_json::json!({ "name": sandbox_name("s1") }))));
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .returning(|_, _, _| Err(execute_refused()));
    client
        .expect_delete_sandbox()
        .withf(|engine, sandbox| engine == "eng1" && sandbox == "s1")
        .times(1)
        .returning(|_, _| Ok(()));

    provider(client)
        .create(CreateSessionRequest::default())
        .await
        .expect_err("a sandbox whose agent is silent is not a usable session");
}

/// A per-session environment has no representation, so it is refused rather than dropped — and the
/// create is never sent, so the refusal is before any side effect.
#[tokio::test]
async fn create_refuses_a_per_session_environment() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_create_sandbox().never();

    let error = provider(client)
        .create(CreateSessionRequest {
            env: BTreeMap::from([("TOKEN".to_string(), "secret".to_string())]),
            ..Default::default()
        })
        .await
        .expect_err("a session environment must be refused");
    assert_eq!(error.code, "INVALID_INPUT", "{error}");
    assert!(error.to_string().contains("each command"), "{error}");
}

// ---- get / get_or_create ----------------------------------------------------------------------

#[tokio::test]
async fn get_returns_none_when_the_sandbox_is_gone() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, _| Err(not_found()));

    let found = provider(client).get("s1").await.expect("a gone sandbox is a valid answer");
    assert!(found.is_none(), "a not-found sandbox is None, not an error");
}

/// A sandbox reports RUNNING while its agent does not answer, and `get` must not report that as a
/// usable session. Mutation check: drop the `probe_agent` call in `get` and this returns
/// `Some(Running)` instead of the unreachable error.
#[tokio::test]
async fn get_does_not_report_a_running_session_whose_agent_is_silent() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .returning(|_, _, _| Err(execute_refused()));

    let error = provider(client)
        .get("s1")
        .await
        .expect_err("a running record with a silent agent is not a healthy session");
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
}

/// Refuse-don't-destroy: `get_or_create` handed a stale id provisions a fresh session and never
/// deletes the stale one, which may be another revision's. Mutation check: add a `delete_sandbox`
/// on the reconnect-failure path and `expect_delete_sandbox().never()` fails.
#[tokio::test]
async fn get_or_create_replaces_a_stale_session_without_deleting_it() {
    let mut client = MockAgentPlatformApi::new();
    // The stale session reads RUNNING but its agent is silent; the fresh one is healthy.
    client
        .expect_get_sandbox()
        .withf(|_, sandbox| sandbox == "stale")
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .withf(|_, sandbox, _| sandbox == "stale")
        .returning(|_, _, _| Err(execute_refused()));

    client
        .expect_create_sandbox()
        .times(1)
        .returning(|_, _| Ok(done_op(serde_json::json!({ "name": sandbox_name("fresh") }))));
    client
        .expect_get_sandbox()
        .withf(|_, sandbox| sandbox == "fresh")
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .withf(|_, sandbox, input| sandbox == "fresh" && op_of(input) == "health")
        .returning(|_, _, _| Ok(health_reply()));

    client.expect_delete_sandbox().never();

    let session = provider(client)
        .get_or_create(CreateSessionRequest {
            session_id: Some("stale".to_string()),
            ..Default::default()
        })
        .await
        .expect("a stale session is replaced");
    assert_eq!(session.session_id, "fresh", "the fresh session is returned, not the stale id");
}

/// A reconnect to a suspended session wakes it and hands it back, rather than creating a second
/// sandbox and orphaning the paused one. Mutation check: fold the `Suspended` arm into `Ok(_) =>
/// {}` and `create_sandbox().never()` fails while a second sandbox is minted.
#[tokio::test]
async fn get_or_create_resumes_a_suspended_session_rather_than_creating_a_second() {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    client.expect_get_sandbox().returning(move |_, id| {
        // Paused on the first read, running once resumed.
        if reads.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(sandbox_in_state(id, "STATE_PAUSED"))
        } else {
            Ok(sandbox_in_state(id, "STATE_RUNNING"))
        }
    });
    client.expect_resume().times(1).returning(|_, _| Ok(done_op(serde_json::json!({}))));
    client
        .expect_execute()
        .withf(|_, _, input| op_of(input) == "health")
        .returning(|_, _, _| Ok(health_reply()));
    client.expect_create_sandbox().never();
    client.expect_delete_sandbox().never();

    let session = provider(client)
        .get_or_create(CreateSessionRequest {
            session_id: Some("paused".to_string()),
            ..Default::default()
        })
        .await
        .expect("a suspended session is resumed and returned");
    assert_eq!(session.session_id, "paused");
    assert_eq!(session.state, SandboxSessionState::Running);
    // The reconnect path the capability flip promises: a woken session carries a real generation
    // read from the container it came back on, not the unprobed sentinel.
    assert_ne!(session.generation, NO_GENERATION, "a woken session carries its container generation");
}

// ---- generation and health -------------------------------------------------------------------

/// The generation a `get` reports for a running session answering with `boot_id`.
async fn generation_for_boot(boot_id: &'static str) -> u64 {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .returning(move |_, _, _| Ok(health_reply_with_boot(boot_id)));

    provider(client)
        .get("s1")
        .await
        .expect("a running session")
        .expect("a present session")
        .generation
}

/// The generation follows the container boot id: it changes when the container is replaced and is
/// stable without a replacement, across separate reads. Mutation check: make
/// `generation_from_boot_id` return a constant and the `assert_ne` below goes red — a reconnect
/// test that could not see a replaced container is the exact failure this backend has.
#[tokio::test]
async fn generation_tracks_the_container_boot_id() {
    let first = generation_for_boot("boot-id-aaaa").await;
    let replaced = generation_for_boot("boot-id-bbbb").await;
    let same = generation_for_boot("boot-id-aaaa").await;

    assert_ne!(first, replaced, "a replaced container changes the generation");
    assert_eq!(first, same, "the same container keeps its generation across separate reads");
    assert_ne!(first, NO_GENERATION, "a probed running session carries a real generation");
}

/// A running record whose agent reports an empty boot id has no identity to reconnect to, so `get`
/// refuses it. Mutation check: drop the emptiness guard in `probe_agent` and this returns
/// `Some(Running)` instead of the unreachable error.
#[tokio::test]
async fn get_refuses_an_agent_that_reports_an_empty_boot_id() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .returning(|_, _, _| Ok(health_reply_with_boot("")));

    let error = provider(client)
        .get("s1")
        .await
        .expect_err("an empty boot id is no container identity");
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
}

/// A health reply that omits the boot id entirely is unreadable, so the session is not reported as
/// usable. Mutation check: make `Health.boot_id` an `Option` without a guard and this returns
/// `Some(Running)`.
#[tokio::test]
async fn get_refuses_an_agent_whose_health_omits_the_boot_id() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client.expect_execute().returning(|_, _, _| {
        Ok(serde_json::to_vec(&serde_json::json!({ "protocolVersion": 1 })).expect("serializes"))
    });

    let error = provider(client)
        .get("s1")
        .await
        .expect_err("a health reply without a boot id is not usable");
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
}

/// A wedged agent that accepts the probe and never answers must not hang `get`; the probe budget
/// cuts it off and `get` returns unreachable. `start_paused` advances the clock to the budget
/// rather than sleeping in real time. Mutation check: drop the `tokio::time::timeout` in
/// `probe_agent` and the clock instead advances to the stub's long sleep, whose `unreachable!`
/// then panics the test — red either way.
#[tokio::test(start_paused = true)]
async fn get_does_not_hang_on_a_wedged_agent() {
    let error = provider_from(Arc::new(WedgedAgent))
        .get("s1")
        .await
        .expect_err("a wedged agent is unreachable, not a hang");
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
}

/// A client whose sandbox reads RUNNING but whose `execute` never answers, standing in for an agent
/// that accepts the health probe and then wedges. Only the two methods `get` reaches are real; the
/// rest are unreachable in this test.
#[derive(Debug)]
struct WedgedAgent;

#[async_trait]
impl AgentPlatformApi for WedgedAgent {
    async fn get_sandbox(&self, _engine: &str, sandbox: &str) -> ClientResult<SandboxEnvironment> {
        Ok(sandbox_in_state(sandbox, "STATE_RUNNING"))
    }

    async fn execute(&self, _engine: &str, _sandbox: &str, _input: &[u8]) -> ClientResult<Vec<u8>> {
        // Far past any probe budget; the budget must return before this does.
        tokio::time::sleep(Duration::from_secs(86_400)).await;
        unreachable!("the probe budget should fire before a wedged execute returns")
    }

    async fn create_engine(&self, _display_name: &str) -> ClientResult<Operation> {
        unimplemented!()
    }
    async fn delete_engine(&self, _engine: &str) -> ClientResult<()> {
        unimplemented!()
    }
    async fn create_template(
        &self,
        _engine: &str,
        _template: SandboxEnvironmentTemplate,
    ) -> ClientResult<Operation> {
        unimplemented!()
    }
    async fn get_template(
        &self,
        _engine: &str,
        _template: &str,
    ) -> ClientResult<SandboxEnvironmentTemplate> {
        unimplemented!()
    }
    async fn delete_template(&self, _engine: &str, _template: &str) -> ClientResult<()> {
        unimplemented!()
    }
    async fn list_templates(
        &self,
        _engine: &str,
    ) -> ClientResult<Vec<SandboxEnvironmentTemplate>> {
        unimplemented!()
    }
    async fn create_sandbox(
        &self,
        _engine: &str,
        _request: SandboxCreateRequest,
    ) -> ClientResult<Operation> {
        unimplemented!()
    }
    async fn list_sandboxes(&self, _engine: &str) -> ClientResult<Vec<SandboxEnvironment>> {
        unimplemented!()
    }
    async fn delete_sandbox(&self, _engine: &str, _sandbox: &str) -> ClientResult<()> {
        unimplemented!()
    }
    async fn pause(&self, _engine: &str, _sandbox: &str) -> ClientResult<Operation> {
        unimplemented!()
    }
    async fn resume(&self, _engine: &str, _sandbox: &str) -> ClientResult<Operation> {
        unimplemented!()
    }
    async fn snapshot(
        &self,
        _engine: &str,
        _sandbox: &str,
        _display_name: &str,
    ) -> ClientResult<Operation> {
        unimplemented!()
    }
    async fn get_operation(&self, _name: &str) -> ClientResult<Operation> {
        unimplemented!()
    }
}

// ---- list -------------------------------------------------------------------------------------

#[tokio::test]
async fn list_maps_sandboxes_to_sessions() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_list_sandboxes().returning(|_| {
        Ok(vec![
            sandbox_in_state("a", "STATE_RUNNING"),
            sandbox_in_state("b", "STATE_PAUSED"),
        ])
    });

    let sessions = provider(client).list().await.expect("list is supported here");
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].session_id, "a");
    assert_eq!(sessions[0].state, SandboxSessionState::Running);
    assert_eq!(sessions[1].session_id, "b");
    assert_eq!(sessions[1].state, SandboxSessionState::Suspended);
}

// ---- run_command: cap threshold ---------------------------------------------------------------

/// A command inside the synchronous window runs through `exec` and starts no job. Mutation check:
/// invert the `deadline <= MAX_SYNCHRONOUS_DEADLINE` test and the `jobStart` panic below fires.
#[tokio::test]
async fn a_short_command_runs_synchronously_without_a_job() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().returning(|_, _, input| match op_of(input).as_str() {
        "exec" => Ok(ndjson(&[stdout_frame(0, b"hi"), exit_frame(0)])),
        "jobStart" => panic!("a short command must not start a job"),
        other => panic!("unexpected op {other}"),
    });

    let frames: Vec<_> = provider(client)
        .run_command(
            "s1",
            RunCommandRequest {
                command: vec!["/bin/echo".to_string(), "hi".to_string()],
                working_directory: None,
                env: BTreeMap::new(),
                deadline: Duration::from_secs(5),
            },
        )
        .await
        .expect("the command runs")
        .collect()
        .await;

    assert!(matches!(frames.first(), Some(Ok(CommandOutput::Stdout { data, .. })) if data == b"hi"));
    assert!(matches!(frames.last(), Some(Ok(CommandOutput::Exit { code: 0, .. }))));
}

/// A command longer than the synchronous window is detached as a job and polled to its exit; no
/// `exec` is sent. The poll cursor advances so a second poll asks for frames after the first.
#[tokio::test(start_paused = true)]
async fn a_long_command_uses_the_job_path() {
    let polls = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().returning(move |_, _, input| match op_of(input).as_str() {
        "jobStart" => Ok(serde_json::to_vec(&serde_json::json!({ "jobId": "j1" })).unwrap()),
        "jobPoll" => {
            let poll = polls.fetch_add(1, Ordering::SeqCst);
            if poll == 0 {
                Ok(serde_json::to_vec(&serde_json::json!({
                    "running": true,
                    "frames": [stdout_frame(0, b"work")],
                }))
                .unwrap())
            } else {
                Ok(serde_json::to_vec(&serde_json::json!({
                    "running": false,
                    "frames": [],
                    "exitCode": 0,
                    "truncated": false,
                }))
                .unwrap())
            }
        }
        "exec" => panic!("a long command must not run synchronously"),
        other => panic!("unexpected op {other}"),
    });

    let frames: Vec<_> = provider(client)
        .run_command(
            "s1",
            RunCommandRequest {
                command: vec!["/bin/sleep".to_string(), "40".to_string()],
                working_directory: None,
                env: BTreeMap::new(),
                deadline: Duration::from_secs(60),
            },
        )
        .await
        .expect("the job starts")
        .collect()
        .await;

    assert!(matches!(frames.first(), Some(Ok(CommandOutput::Stdout { data, .. })) if data == b"work"));
    assert!(matches!(frames.last(), Some(Ok(CommandOutput::Exit { code: 0, .. }))));
}

/// A job the agent reports as failing (a deadline, a spawn failure) carries an error object with no
/// exit code, and the provider surfaces it rather than fabricating a clean exit.
#[tokio::test(start_paused = true)]
async fn a_job_error_object_becomes_a_stream_error() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().returning(|_, _, input| match op_of(input).as_str() {
        "jobStart" => Ok(serde_json::to_vec(&serde_json::json!({ "jobId": "j1" })).unwrap()),
        "jobPoll" => Ok(serde_json::to_vec(&serde_json::json!({
            "running": false,
            "frames": [],
            "error": { "code": "deadlineExceeded", "message": "exceeded its 60000ms deadline" },
        }))
        .unwrap()),
        other => panic!("unexpected op {other}"),
    });

    let frames: Vec<_> = provider(client)
        .run_command(
            "s1",
            RunCommandRequest {
                command: vec!["/bin/sleep".to_string(), "99".to_string()],
                working_directory: None,
                env: BTreeMap::new(),
                deadline: Duration::from_secs(60),
            },
        )
        .await
        .expect("the job starts")
        .collect()
        .await;

    let error = frames.last().expect("a terminal item").as_ref().expect_err("an error object is a failure");
    assert!(error.to_string().contains("deadlineExceeded"), "{error}");
}

/// Refuse-don't-destroy: a command against a gone session is refused and nothing is deleted.
/// Mutation check: add a `delete_sandbox` to `run_command`'s failure path and `.never()` fails.
#[tokio::test]
async fn a_command_on_a_gone_session_is_refused_and_deletes_nothing() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().returning(|_, _, _| Err(not_found()));
    client.expect_delete_sandbox().never();

    // The synchronous exec fails before a stream exists, so the refusal is the call's own error.
    let Err(error) = provider(client)
        .run_command(
            "s1",
            RunCommandRequest {
                command: vec!["/bin/true".to_string()],
                working_directory: None,
                env: BTreeMap::new(),
                deadline: Duration::from_secs(5),
            },
        )
        .await
    else {
        panic!("a command against a gone session is refused");
    };
    assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "{error}");
    assert!(error.to_string().contains("sessionGone"), "{error}");
}

#[tokio::test]
async fn a_command_without_a_deadline_or_program_is_refused() {
    // `run_command`'s Ok is a stream, which is not `Debug`, so the error is matched out by hand.
    let Err(empty) = provider(MockAgentPlatformApi::new())
        .run_command(
            "s1",
            RunCommandRequest {
                command: vec![],
                working_directory: None,
                env: BTreeMap::new(),
                deadline: Duration::from_secs(5),
            },
        )
        .await
    else {
        panic!("an empty command is refused");
    };
    assert_eq!(empty.code, "INVALID_INPUT", "{empty}");

    let Err(zero) = provider(MockAgentPlatformApi::new())
        .run_command(
            "s1",
            RunCommandRequest {
                command: vec!["/bin/true".to_string()],
                working_directory: None,
                env: BTreeMap::new(),
                deadline: Duration::ZERO,
            },
        )
        .await
    else {
        panic!("a zero deadline is refused");
    };
    assert!(zero.to_string().contains("deadline"), "{zero}");
}

// ---- files ------------------------------------------------------------------------------------

/// writeFile sends the agent's `contentsBase64` field (never `contents`) and treats an empty body
/// as success. Mutation check: rename the field to `contents` and the `withf` assertion fails.
#[tokio::test]
async fn write_files_sends_contents_base64_and_accepts_an_empty_body() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .withf(|_, _, input| {
            let value: serde_json::Value = serde_json::from_slice(input).unwrap();
            op_of(input) == "writeFile"
                && value.get("contentsBase64").and_then(|v| v.as_str()) == Some(&BASE64.encode(b"data"))
                && value.get("contents").is_none()
        })
        .times(1)
        .returning(|_, _, _| Ok(Vec::new()));

    provider(client)
        .write_files("s1", BTreeMap::from([("a.txt".to_string(), b"data".to_vec())]))
        .await
        .expect("an empty body is a successful write");
}

#[tokio::test]
async fn mkdir_accepts_an_empty_body() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .withf(|_, _, input| op_of(input) == "mkdir")
        .returning(|_, _, _| Ok(Vec::new()));

    provider(client).mkdir("s1", "out").await.expect("mkdir succeeds on an empty body");
}

#[tokio::test]
async fn read_file_decodes_the_agent_reply() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .withf(|_, _, input| op_of(input) == "readFile")
        .returning(|_, _, _| {
            Ok(serde_json::to_vec(&serde_json::json!({ "contentsBase64": BASE64.encode(b"file body") }))
                .unwrap())
        });

    let contents = provider(client).read_file("s1", "a.txt").await.expect("read succeeds");
    assert_eq!(contents, b"file body");
}

// ---- suspend / resume / snapshot --------------------------------------------------------------

#[tokio::test]
async fn suspend_and_resume_await_their_operations() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_pause().times(1).returning(|_, _| Ok(done_op(serde_json::json!({}))));
    client.expect_resume().times(1).returning(|_, _| Ok(done_op(serde_json::json!({}))));

    let provider = provider(client);
    provider.suspend("s1").await.expect("suspend completes");
    provider.resume("s1").await.expect("resume completes");
}

#[tokio::test]
async fn snapshot_returns_the_snapshot_name() {
    let mut client = MockAgentPlatformApi::new();
    let name = "projects/p/locations/us-central1/reasoningEngines/eng1/sandboxEnvironmentSnapshots/snap1";
    client
        .expect_snapshot()
        .withf(|engine, sandbox, display| engine == "eng1" && sandbox == "s1" && !display.is_empty())
        .returning(move |_, _, _| Ok(done_op(serde_json::json!({ "name": name }))));

    let returned = provider(client).snapshot("s1").await.expect("snapshot completes");
    assert_eq!(returned, name);
}

// ---- terminate --------------------------------------------------------------------------------

/// terminate polls the accepted delete to not-found before it reports containment. Mutation check:
/// return `Ok(())` right after `delete_sandbox` and the "still present" test below passes wrongly.
#[tokio::test(start_paused = true)]
async fn terminate_confirms_by_polling_to_not_found() {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    client.expect_delete_sandbox().times(1).returning(|_, _| Ok(()));
    client.expect_get_sandbox().returning(move |_, id| {
        // Present on the first read, gone on the second: an accepted delete is not a completed one.
        if reads.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(sandbox_in_state(id, "STATE_RUNNING"))
        } else {
            Err(not_found())
        }
    });

    provider(client).terminate("s1").await.expect("a session that goes absent is confirmed gone");
}

#[tokio::test(start_paused = true)]
async fn terminate_reports_unconfirmed_when_the_session_stays_present() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_delete_sandbox().returning(|_, _| Ok(()));
    client.expect_get_sandbox().returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));

    let error = provider(client)
        .terminate("s1")
        .await
        .expect_err("a session still present after the poll is not contained");
    assert!(error.to_string().contains("may still be running"), "{error}");
}

// ---- unit guards ------------------------------------------------------------------------------

/// AllowDomains is refused naming the sandbox and both accepted modes; the two expressible modes
/// map to the boolean. Mutation check: return `Ok` for AllowDomains and this fails.
#[test]
fn egress_refuses_domain_scoping_and_names_the_modes() {
    let error = egress_control_config("sbx-7", &SandboxEgress::AllowDomains { domains: vec!["x.io".into()] })
        .expect_err("domain-scoped egress has no representation");
    assert_eq!(error.code, "INVALID_INPUT", "{error}");
    let rendered = error.to_string();
    assert!(rendered.contains("sbx-7"), "names the sandbox: {rendered}");
    assert!(rendered.contains("allow") && rendered.contains("deny"), "names both modes: {rendered}");

    assert_eq!(
        egress_control_config("s", &SandboxEgress::Deny).expect("deny maps").internet_access,
        Some(false)
    );
    assert_eq!(
        egress_control_config("s", &SandboxEgress::Allow).expect("allow maps").internet_access,
        Some(true)
    );
}

/// A session id that could address another sandbox never reaches a URL. Mutation check: weaken
/// `is_addressable_id` to accept '/' and the traversal ids below stop being refused.
#[tokio::test]
async fn a_session_id_that_could_escape_its_sandbox_is_refused() {
    for id in ["../other", "a/b", "has space", "", "with?query", "with#frag"] {
        let error = provider(MockAgentPlatformApi::new())
            .get(id)
            .await
            .expect_err(&format!("'{id}' must be refused before it reaches a URL"));
        assert_eq!(error.code, "INVALID_INPUT", "'{id}': {error}");
    }
}

/// An output stream that ends without a terminal frame is a transport failure, not a command that
/// finished. Mutation check: drop the `saw_terminal` trailing item and this reads as success.
#[test]
fn an_output_without_a_terminal_frame_is_an_unknown_outcome() {
    let frames = parse_exec_frames(&ndjson(&[stdout_frame(0, b"partial")])).expect("frames parse");
    assert_eq!(frames.len(), 2);
    frames[0].as_ref().expect("the stdout frame still arrives");
    let error = frames[1].as_ref().expect_err("a truncated stream is not success");
    assert!(error.to_string().contains("without a terminal frame"), "{error}");
}

/// A body that is not frames at all is the agent's refusal, not a command's output.
#[test]
fn a_non_frame_body_is_reported_as_a_refusal() {
    let error = parse_exec_frames(b"forbidden: a capability is required")
        .expect_err("an error body is not a stream");
    assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "{error}");
}

/// The not-found classification is read off the source chain, where the client leaves it, not off
/// the outer `RequestFailed` variant.
#[test]
fn not_found_is_read_from_the_source_chain() {
    assert!(is_not_found(&not_found()), "a wrapped 404 is a gone session");
    assert!(
        !is_not_found(&execute_refused()),
        "an ordinary execute failure is not a gone session"
    );
}

#[test]
fn the_engine_is_reduced_to_a_bare_segment() {
    let provider = provider(MockAgentPlatformApi::new());
    assert_eq!(provider.engine(), "eng1", "the full resource name is reduced to the engine id");
}
