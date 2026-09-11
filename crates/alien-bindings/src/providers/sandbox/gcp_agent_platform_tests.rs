use super::*;
use alien_gcp_clients::gcp::agent_platform::{
    MockAgentPlatformApi, ReasoningEngine, SandboxEnvironmentTemplate,
};
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
    GcpAgentPlatformSandbox::new(
        client,
        ENGINE_FULL.to_string(),
        TEMPLATE.to_string(),
        Some(3600),
    )
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
        .and_then(|value| {
            value
                .get("op")
                .and_then(|op| op.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn ndjson(lines: &[serde_json::Value]) -> Vec<u8> {
    let mut body = Vec::new();
    for line in lines {
        body.extend_from_slice(
            serde_json::to_string(line)
                .expect("frame serializes")
                .as_bytes(),
        );
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

    let sandbox = provider(client)
        .create(CreateSandboxRequest::default())
        .await
        .expect("create succeeds");

    assert_eq!(sandbox.sandbox_id, "s1");
    assert_eq!(sandbox.state, SandboxState::Running);
}

/// Pins the unit as well as the value: `timeoutMs` is milliseconds and Agent Platform's `ttl` is
/// a duration string in seconds, so a missed conversion is 1000x either way. The declared ceiling
/// (3600s, from `provider`) must not be raised by a request asking for more.
#[tokio::test]
async fn a_requested_lifetime_reaches_the_create_body_and_cannot_raise_the_declared_ceiling() {
    for (timeout_ms, expected) in [(90_000_u64, "90s"), (7_200_000, "3600s")] {
        let mut client = MockAgentPlatformApi::new();
        client
            .expect_create_sandbox()
            .withf(move |_, request| request.ttl.as_deref() == Some(expected))
            .times(1)
            .returning(|_, _| Ok(done_op(serde_json::json!({ "name": sandbox_name("s1") }))));
        client
            .expect_get_sandbox()
            .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
        client
            .expect_execute()
            .returning(|_, _, _| Ok(health_reply()));

        provider(client)
            .create(CreateSandboxRequest {
                timeout_ms: Some(timeout_ms),
                ..Default::default()
            })
            .await
            .expect("create succeeds");
    }
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
        .create(CreateSandboxRequest::default())
        .await
        .expect_err("a sandbox whose agent is silent is not a usable sandbox");
}

/// A per-sandbox environment has no representation, so it is refused rather than dropped — and the
/// create is never sent, so the refusal is before any side effect.
#[tokio::test]
async fn create_refuses_a_per_sandbox_environment() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_create_sandbox().never();

    let error = provider(client)
        .create(CreateSandboxRequest {
            env: BTreeMap::from([("TOKEN".to_string(), "secret".to_string())]),
            ..Default::default()
        })
        .await
        .expect_err("a sandbox environment must be refused");
    // Same code AWS answers the identical condition with (see the reasoning above create's check).
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
    assert!(error.to_string().contains("env"), "{error}");
    assert!(error.to_string().contains("per command"), "{error}");
}

// ---- get / get_or_create ----------------------------------------------------------------------

#[tokio::test]
async fn get_returns_none_when_the_sandbox_is_gone() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, _| Err(not_found()));

    let found = provider(client)
        .get("s1")
        .await
        .expect("a gone sandbox is a valid answer");
    assert!(found.is_none(), "a not-found sandbox is None, not an error");
}

/// A sandbox reports RUNNING while its agent does not answer, and `get` must not report that as a
/// usable sandbox. Mutation check: drop the `probe_agent` call in `get` and this returns
/// `Some(Running)` instead of the unreachable error.
#[tokio::test]
async fn get_does_not_report_a_running_sandbox_whose_agent_is_silent() {
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
        .expect_err("a running record with a silent agent is not a healthy sandbox");
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
}

/// Refuse-don't-destroy: `get_or_create` handed a stale id provisions a fresh sandbox and never
/// deletes the stale one, which may be another revision's. Mutation check: add a `delete_sandbox`
/// on the reconnect-failure path and `expect_delete_sandbox().never()` fails.
#[tokio::test]
async fn get_or_create_replaces_a_stale_sandbox_without_deleting_it() {
    let mut client = MockAgentPlatformApi::new();
    // The stale sandbox reads RUNNING but its agent is silent; the fresh one is healthy.
    client
        .expect_get_sandbox()
        .withf(|_, sandbox| sandbox == "stale")
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .withf(|_, sandbox, _| sandbox == "stale")
        .returning(|_, _, _| Err(execute_refused()));

    client.expect_create_sandbox().times(1).returning(|_, _| {
        Ok(done_op(
            serde_json::json!({ "name": sandbox_name("fresh") }),
        ))
    });
    client
        .expect_get_sandbox()
        .withf(|_, sandbox| sandbox == "fresh")
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .withf(|_, sandbox, input| sandbox == "fresh" && op_of(input) == "health")
        .returning(|_, _, _| Ok(health_reply()));

    client.expect_delete_sandbox().never();

    let sandbox = provider(client)
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("stale".to_string()),
            ..Default::default()
        })
        .await
        .expect("a stale sandbox is replaced");
    assert_eq!(
        sandbox.sandbox.sandbox_id, "fresh",
        "the fresh sandbox is returned, not the stale id"
    );
    assert!(sandbox.created, "a replacement is a sandbox this call made");
}

/// A reconnect to a suspended sandbox wakes it and hands it back, rather than creating a second
/// A reconnect to a sandbox still coming up waits for it. Replacing it would leave the first one
/// starting, reaching RUNNING and costing its owner, with nobody holding its id — the same leak the
/// suspended arm avoids. Mutation check: delete the `Starting` arm and `create_sandbox().never()`
/// fires.
#[tokio::test]
async fn get_or_create_waits_for_a_booting_sandbox_rather_than_creating_a_second() {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    client.expect_get_sandbox().returning(move |_, id| {
        // Coming up on the first read, running once it has settled.
        if reads.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(sandbox_in_state(id, "STATE_CREATING"))
        } else {
            Ok(sandbox_in_state(id, "STATE_RUNNING"))
        }
    });
    client
        .expect_execute()
        .withf(|_, _, input| op_of(input) == "health")
        .returning(|_, _, _| Ok(health_reply()));
    client.expect_create_sandbox().never();
    client.expect_delete_sandbox().never();
    client.expect_resume().never();

    let sandbox = provider(client)
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("booting".to_string()),
            ..Default::default()
        })
        .await
        .expect("a booting sandbox is waited for and handed back");

    assert_eq!(sandbox.sandbox.sandbox_id, "booting");
    assert_eq!(sandbox.sandbox.state, SandboxState::Running);
    assert!(
        !sandbox.created,
        "whoever started it created it, not this call"
    );
}

/// `STATE_RESUMING` reads as `Starting` too, and it is the reading two callers sharing one id
/// actually produce: one wakes the sandbox, the other must not answer the wake in progress with a
/// second sandbox.
#[tokio::test]
async fn get_or_create_waits_out_a_wake_someone_else_started() {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    client.expect_get_sandbox().returning(move |_, id| {
        if reads.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(sandbox_in_state(id, "STATE_RESUMING"))
        } else {
            Ok(sandbox_in_state(id, "STATE_RUNNING"))
        }
    });
    client
        .expect_execute()
        .withf(|_, _, input| op_of(input) == "health")
        .returning(|_, _, _| Ok(health_reply()));
    client.expect_create_sandbox().never();
    client.expect_delete_sandbox().never();
    // Not ours to wake: someone else's resume is already in flight.
    client.expect_resume().never();

    let sandbox = provider(client)
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("waking".to_string()),
            ..Default::default()
        })
        .await
        .expect("a wake already in flight is waited out");

    assert!(!sandbox.created, "the sandbox existed before this call");
}

/// sandbox and orphaning the paused one. Mutation check: fold the `Suspended` arm into `Ok(_) =>
/// {}` and `create_sandbox().never()` fails while a second sandbox is minted.
#[tokio::test]
async fn get_or_create_resumes_a_suspended_sandbox_rather_than_creating_a_second() {
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
    client
        .expect_resume()
        .times(1)
        .returning(|_, _| Ok(done_op(serde_json::json!({}))));
    client
        .expect_execute()
        .withf(|_, _, input| op_of(input) == "health")
        .returning(|_, _, _| Ok(health_reply()));
    client.expect_create_sandbox().never();
    client.expect_delete_sandbox().never();

    let sandbox = provider(client)
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("paused".to_string()),
            ..Default::default()
        })
        .await
        .expect("a suspended sandbox is resumed and returned");
    assert_eq!(sandbox.sandbox.sandbox_id, "paused");
    assert_eq!(sandbox.sandbox.state, SandboxState::Running);
    assert!(
        !sandbox.created,
        "waking a sleeping sandbox is not creating one"
    );
    // The reconnect path the capability flip promises: a woken sandbox carries a real generation
    // read from the container it came back on, not the unprobed sentinel.
    assert_ne!(
        sandbox.sandbox.generation, NO_GENERATION,
        "a woken sandbox carries its container generation"
    );
}

#[tokio::test]
async fn get_or_create_fails_rather_than_leaking_a_resume_it_cannot_roll_back() {
    let mut client = MockAgentPlatformApi::new();
    // Paused before the wake and paused after it: the wake never brought the sandbox up.
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_PAUSED")));
    client
        .expect_resume()
        .times(1)
        .returning(|_, _| Ok(done_op(serde_json::json!({}))));
    // The compensating suspend fails, so the woken sandbox cannot be put back to sleep.
    client
        .expect_pause()
        .times(1)
        .returning(|_, _| Err(not_found()));
    client
        .expect_execute()
        .returning(|_, _, _| Ok(health_reply()));
    // A second live sandbox must never be provisioned beside the one this call woke.
    client.expect_create_sandbox().never();
    client.expect_delete_sandbox().never();

    let error = provider(client)
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("paused".to_string()),
            ..Default::default()
        })
        .await
        .expect_err("a resume that cannot be rolled back must fail, not leak a live sandbox");
    assert!(
        error.to_string().contains("paused"),
        "the failure names the woken sandbox so it stays identifiable: {error}"
    );
}

// ---- generation and health -------------------------------------------------------------------

/// The generation a `get` reports for a running sandbox answering with `boot_id`.
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
        .expect("a running sandbox")
        .expect("a present sandbox")
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

    assert_ne!(
        first, replaced,
        "a replaced container changes the generation"
    );
    assert_eq!(
        first, same,
        "the same container keeps its generation across separate reads"
    );
    assert_ne!(
        first, NO_GENERATION,
        "a probed running sandbox carries a real generation"
    );
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

/// A health reply that omits the boot id entirely is unreadable, so the sandbox is not reported as
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
    async fn list_engines(&self) -> ClientResult<Vec<ReasoningEngine>> {
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
    async fn list_templates(&self, _engine: &str) -> ClientResult<Vec<SandboxEnvironmentTemplate>> {
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
async fn list_reports_each_sandbox_with_its_state() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_list_sandboxes().returning(|_| {
        Ok(vec![
            sandbox_in_state("a", "STATE_RUNNING"),
            sandbox_in_state("b", "STATE_PAUSED"),
        ])
    });

    let sandboxes = provider(client)
        .list()
        .await
        .expect("list is supported here");
    assert_eq!(sandboxes.len(), 2);
    assert_eq!(sandboxes[0].sandbox_id, "a");
    assert_eq!(sandboxes[0].state, SandboxState::Running);
    assert_eq!(sandboxes[1].sandbox_id, "b");
    assert_eq!(sandboxes[1].state, SandboxState::Paused);
}

/// Every state word the API sends, mapped onto the binding's four.
///
/// The pause family is three words, and only one of them shares a stem with the binding's own
/// `Paused`: a backend still reporting `STATE_SUSPENDED` has to keep reading as paused, or an
/// idle sandbox is dropped from `list` as unreadable and an orphan sweep never sees it.
#[tokio::test]
async fn every_state_word_the_api_sends_maps_onto_one_of_ours() {
    for (reported, expected) in [
        ("STATE_RUNNING", SandboxState::Running),
        ("STATE_CREATING", SandboxState::Starting),
        ("STATE_PENDING", SandboxState::Starting),
        ("STATE_RESUMING", SandboxState::Starting),
        ("STATE_PAUSED", SandboxState::Paused),
        ("STATE_PAUSING", SandboxState::Paused),
        ("STATE_SUSPENDED", SandboxState::Paused),
        ("STATE_STOPPED", SandboxState::Terminated),
        ("STATE_FAILED", SandboxState::Terminated),
        ("STATE_DELETING", SandboxState::Terminated),
        ("STATE_DELETED", SandboxState::Terminated),
    ] {
        let mut client = MockAgentPlatformApi::new();
        client
            .expect_list_sandboxes()
            .returning(move |_| Ok(vec![sandbox_in_state("s1", reported)]));

        let sandboxes = provider(client)
            .list()
            .await
            .unwrap_or_else(|error| panic!("{reported}: {error}"));

        assert_eq!(
            sandboxes.len(),
            1,
            "{reported} was dropped as a state this provider cannot read"
        );
        assert_eq!(sandboxes[0].state, expected, "state {reported}");
    }
}

/// A word outside that set is a preview API that moved. Guessing which of the four it is is how
/// a caller ends up sending work to a sandbox that is going away, so a read refuses instead.
#[tokio::test]
async fn a_state_word_outside_that_set_is_an_error_rather_than_a_guess() {
    for reported in [Some("STATE_HIBERNATED"), None] {
        let mut client = MockAgentPlatformApi::new();
        client.expect_get_sandbox().returning(move |_, id| {
            let mut sandbox = sandbox_in_state(id, "STATE_RUNNING");
            sandbox.state = reported.map(str::to_string);
            Ok(sandbox)
        });

        let error = provider(client)
            .get("s1")
            .await
            .expect_err("an unreadable state must not become a sandbox");

        assert_eq!(error.code, "UNEXPECTED_RESPONSE_FORMAT", "{error}");
    }
}

// ---- run_command: argv ------------------------------------------------------------------------

/// The agent takes one argv array, so the program and its arguments are rejoined on the way out.
/// A rejoin that dropped, reordered or duplicated an element would run something other than what
/// was asked for. Two arguments rather than one: with a single argument an inverted or duplicated
/// rejoin builds the same array as the correct one.
#[tokio::test]
async fn the_program_leads_its_arguments_in_the_envelope() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .times(1)
        .withf(|_, _, input| {
            let body: serde_json::Value =
                serde_json::from_slice(input).expect("the envelope is json");
            body["command"] == serde_json::json!(["python", "-u", "main.py"])
                && body["cwd"] == serde_json::json!("/work")
        })
        .returning(|_, _, _| Ok(ndjson(&[exit_frame(0)])));

    let frames: Vec<_> = provider(client)
        .run_command(
            "s1",
            RunCommandRequest {
                command: "python".to_string(),
                args: vec!["-u".to_string(), "main.py".to_string()],
                cwd: Some("/work".to_string()),
                env: BTreeMap::new(),
                timeout: Duration::from_secs(5),
            },
        )
        .await
        .expect("the command runs")
        .collect()
        .await;

    assert!(
        matches!(frames.last(), Some(Ok(CommandOutput::Exit { code, .. })) if *code == 0),
        "the command has to reach its exit: {frames:?}"
    );
}

// ---- run_command: cap threshold ---------------------------------------------------------------

/// A command inside the synchronous window runs through `exec` and starts no job. Mutation check:
/// invert the `timeout <= MAX_SYNCHRONOUS_TIMEOUT` test and the `jobStart` panic below fires.
#[tokio::test]
async fn a_short_command_runs_synchronously_without_a_job() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(|_, _, input| match op_of(input).as_str() {
            "exec" => Ok(ndjson(&[stdout_frame(0, b"hi"), exit_frame(0)])),
            "jobStart" => panic!("a short command must not start a job"),
            other => panic!("unexpected op {other}"),
        });

    let frames: Vec<_> = provider(client)
        .run_command(
            "s1",
            RunCommandRequest {
                command: "/bin/echo".to_string(),
                args: vec!["hi".to_string()],
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(5),
            },
        )
        .await
        .expect("the command runs")
        .collect()
        .await;

    assert!(
        matches!(frames.first(), Some(Ok(CommandOutput::Stdout { data, .. })) if data == b"hi")
    );
    assert!(matches!(
        frames.last(),
        Some(Ok(CommandOutput::Exit { code: 0, .. }))
    ));
}

/// A command longer than the synchronous window is detached as a job and polled to its exit; no
/// `exec` is sent. The poll cursor advances so a second poll asks for frames after the first.
#[tokio::test(start_paused = true)]
async fn a_long_command_uses_the_job_path() {
    let polls = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(move |_, _, input| match op_of(input).as_str() {
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
                command: "/bin/sleep".to_string(),
                args: vec!["40".to_string()],
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(60),
            },
        )
        .await
        .expect("the job starts")
        .collect()
        .await;

    assert!(
        matches!(frames.first(), Some(Ok(CommandOutput::Stdout { data, .. })) if data == b"work")
    );
    assert!(matches!(
        frames.last(),
        Some(Ok(CommandOutput::Exit { code: 0, .. }))
    ));
}

/// A client whose job answers depend only on the cursor it is asked for, so the streaming path and
/// the trait methods read the same job the same way.
fn job_client() -> MockAgentPlatformApi {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(|_, _, input| match op_of(input).as_str() {
            "jobStart" => Ok(serde_json::to_vec(&serde_json::json!({ "jobId": "j1" })).unwrap()),
            "jobPoll" => Ok(serde_json::to_vec(&match since_of(input) {
                None => serde_json::json!({
                    "running": true,
                    "frames": [stdout_frame(0, b"work")],
                }),
                Some(_) => serde_json::json!({
                    "running": false,
                    "frames": [],
                    "exitCode": 0,
                    "truncated": false,
                }),
            })
            .unwrap()),
            other => panic!("unexpected op {other}"),
        });
    client
}

fn since_of(input: &[u8]) -> Option<u64> {
    serde_json::from_slice::<serde_json::Value>(input)
        .ok()?
        .get("sinceSeq")?
        .as_u64()
}

/// `run_command`'s long path is `start_job` plus a poll loop, so what a caller polls for itself
/// has to be exactly what the stream would have carried. A job surfaced through the trait that
/// dropped or reordered a frame would be a second, quieter implementation of the same thing.
#[tokio::test(start_paused = true)]
async fn a_polled_job_carries_what_run_command_would_have_streamed() {
    let streamed: Vec<CommandOutput> = provider(job_client())
        .run_command("s1", long_command())
        .await
        .expect("the job starts")
        .map(|frame| frame.expect("every frame is output"))
        .collect()
        .await;

    let sandbox = provider(job_client());
    let started = sandbox
        .start_job("s1", long_command())
        .await
        .expect("the job starts");
    assert_eq!(started.job_id, "j1");

    let first = sandbox
        .poll_job("s1", &started.job_id, None)
        .await
        .expect("the first poll answers");
    assert!(first.running, "the job has not ended yet");
    let last = sandbox
        .poll_job("s1", &started.job_id, Some(0))
        .await
        .expect("the second poll answers");
    assert!(!last.running);

    let exit = last.exit.expect("a job that ended carries its exit");
    let polled: Vec<CommandOutput> = first
        .frames
        .into_iter()
        .chain(last.frames)
        .chain([CommandOutput::Exit {
            code: exit.code,
            truncated: exit.truncated,
        }])
        .collect();

    assert_eq!(polled, streamed);
    assert_eq!(
        polled.len(),
        2,
        "the fixture produces one output frame and one exit, so an empty match would prove nothing"
    );
}

fn long_command() -> RunCommandRequest {
    RunCommandRequest {
        command: "/bin/sleep".to_string(),
        args: vec!["40".to_string()],
        cwd: None,
        env: BTreeMap::new(),
        timeout: Duration::from_secs(60),
    }
}

/// A job the agent reports as failing (a deadline, a spawn failure) carries an error object with no
/// exit code, and the provider surfaces it rather than fabricating a clean exit.
#[tokio::test(start_paused = true)]
async fn a_job_error_object_becomes_a_stream_error() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(|_, _, input| match op_of(input).as_str() {
            "jobStart" => Ok(serde_json::to_vec(&serde_json::json!({ "jobId": "j1" })).unwrap()),
            "jobPoll" => Ok(serde_json::to_vec(&serde_json::json!({
                "running": false,
                "frames": [],
                "error": { "code": "timeoutExceeded", "message": "exceeded its 60000ms deadline" },
            }))
            .unwrap()),
            other => panic!("unexpected op {other}"),
        });

    let frames: Vec<_> = provider(client)
        .run_command(
            "s1",
            RunCommandRequest {
                command: "/bin/sleep".to_string(),
                args: vec!["99".to_string()],
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(60),
            },
        )
        .await
        .expect("the job starts")
        .collect()
        .await;

    let error = frames
        .last()
        .expect("a terminal item")
        .as_ref()
        .expect_err("an error object is a failure");
    assert!(error.to_string().contains("timeoutExceeded"), "{error}");
}

/// The write-once / read-retries split, pinned in one test so neither half can pass on the
/// absence of the other. A mutating `:execute` that fails is delivered exactly once — it may have
/// already run, so its outcome is unestablished and re-sending it could double a side effect —
/// while a read is polled until the sandbox settles. Mutation check: give `execute_op` a retry loop and `execute`'s `.times(1)`
/// fails; remove `terminate`'s poll and the read count collapses to one.
#[tokio::test(start_paused = true)]
async fn a_failed_command_is_delivered_once_where_a_read_still_retries() {
    let reads = Arc::new(AtomicUsize::new(0));
    let reads_seen = reads.clone();
    let mut client = MockAgentPlatformApi::new();

    client
        .expect_execute()
        .times(1)
        .returning(|_, _, _| Err(execute_refused()));
    client
        .expect_delete_sandbox()
        .times(1)
        .returning(|_, _| Ok(()));
    client.expect_get_sandbox().returning(move |_, id| {
        // Present on the first two reads, gone on the third: the poll, not one read, decides.
        if reads.fetch_add(1, Ordering::SeqCst) < 2 {
            Ok(sandbox_in_state(id, "STATE_RUNNING"))
        } else {
            Err(not_found())
        }
    });

    let sut = provider(client);

    let Err(command) = sut
        .run_command(
            "s1",
            RunCommandRequest {
                command: "/bin/true".to_string(),
                args: Vec::new(),
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(5),
            },
        )
        .await
    else {
        panic!("a mutating command whose execute fails is refused, not retried into success");
    };
    assert_eq!(command.code, "SANDBOX_OUTCOME_UNKNOWN", "{command}");
    assert!(
        !command.retryable,
        "the call may have started the command, so it is delivered once: {command}"
    );

    sut.terminate("s1")
        .await
        .expect("the poll confirms the sandbox is gone");

    assert!(
        reads_seen.load(Ordering::SeqCst) > 1,
        "confirming the sandbox gone took more than one read, so the read path retries"
    );
}

/// Refuse-don't-destroy: a command against a gone sandbox is refused and nothing is deleted.
/// Mutation check: add a `delete_sandbox` to `run_command`'s failure path and `.never()` fails.
#[tokio::test]
async fn a_command_on_a_gone_sandbox_is_refused_and_deletes_nothing() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(|_, _, _| Err(not_found()));
    client.expect_delete_sandbox().never();

    // The synchronous exec fails before a stream exists, so the refusal is the call's own error.
    let Err(error) = provider(client)
        .run_command(
            "s1",
            RunCommandRequest {
                command: "/bin/true".to_string(),
                args: Vec::new(),
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(5),
            },
        )
        .await
    else {
        panic!("a command against a gone sandbox is refused");
    };
    assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "{error}");
    assert!(error.to_string().contains("sandboxGone"), "{error}");
}

#[tokio::test]
async fn a_command_without_a_timeout_or_program_is_refused() {
    // `run_command`'s Ok is a stream, which is not `Debug`, so the error is matched out by hand.
    let Err(empty) = provider(MockAgentPlatformApi::new())
        .run_command(
            "s1",
            RunCommandRequest {
                command: String::new(),
                args: Vec::new(),
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(5),
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
                command: "/bin/true".to_string(),
                args: Vec::new(),
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::ZERO,
            },
        )
        .await
    else {
        panic!("a zero timeout is refused");
    };
    assert!(zero.to_string().contains("timeout"), "{zero}");
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
                && value.get("contentsBase64").and_then(|v| v.as_str())
                    == Some(&BASE64.encode(b"data"))
                && value.get("contents").is_none()
        })
        .times(1)
        .returning(|_, _, _| Ok(Vec::new()));

    provider(client)
        .write_files(
            "s1",
            BTreeMap::from([("a.txt".to_string(), b"data".to_vec())]),
        )
        .await
        .expect("an empty body is a successful write");
}

#[tokio::test]
async fn read_file_decodes_the_agent_reply() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .withf(|_, _, input| op_of(input) == "readFile")
        .returning(|_, _, _| {
            Ok(serde_json::to_vec(
                &serde_json::json!({ "contentsBase64": BASE64.encode(b"file body") }),
            )
            .unwrap())
        });

    let contents = provider(client)
        .read_file("s1", "a.txt")
        .await
        .expect("read succeeds");
    assert_eq!(contents, b"file body");
}

// ---- pause / resume / snapshot ----------------------------------------------------------------

#[tokio::test]
async fn pause_and_resume_await_their_operations() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_pause()
        .times(1)
        .returning(|_, _| Ok(done_op(serde_json::json!({}))));
    client
        .expect_resume()
        .times(1)
        .returning(|_, _| Ok(done_op(serde_json::json!({}))));

    let provider = provider(client);
    provider.pause("s1").await.expect("pause completes");
    provider.resume("s1").await.expect("resume completes");
}

#[tokio::test]
async fn snapshot_returns_the_snapshot_name() {
    let mut client = MockAgentPlatformApi::new();
    let name =
        "projects/p/locations/us-central1/reasoningEngines/eng1/sandboxEnvironmentSnapshots/snap1";
    client
        .expect_snapshot()
        .withf(|engine, sandbox, display| {
            engine == "eng1" && sandbox == "s1" && !display.is_empty()
        })
        .returning(move |_, _, _| Ok(done_op(serde_json::json!({ "name": name }))));

    let returned = provider(client)
        .snapshot("s1")
        .await
        .expect("snapshot completes");
    assert_eq!(returned, name);
}

// ---- terminate --------------------------------------------------------------------------------

/// terminate polls the accepted delete to not-found before it reports containment. Mutation check:
/// return `Ok(())` right after `delete_sandbox` and the "still present" test below passes wrongly.
#[tokio::test(start_paused = true)]
async fn terminate_confirms_by_polling_to_not_found() {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_delete_sandbox()
        .times(1)
        .returning(|_, _| Ok(()));
    client.expect_get_sandbox().returning(move |_, id| {
        // Present on the first read, gone on the second: an accepted delete is not a completed one.
        if reads.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(sandbox_in_state(id, "STATE_RUNNING"))
        } else {
            Err(not_found())
        }
    });

    provider(client)
        .terminate("s1")
        .await
        .expect("a sandbox that goes absent is confirmed gone");
}

#[tokio::test(start_paused = true)]
async fn terminate_reports_unconfirmed_when_the_sandbox_stays_present() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_delete_sandbox().returning(|_, _| Ok(()));
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));

    let error = provider(client)
        .terminate("s1")
        .await
        .expect_err("a sandbox still present after the poll is not contained");
    assert!(
        error.to_string().contains("may still be running"),
        "{error}"
    );
}

// ---- unit guards ------------------------------------------------------------------------------

/// AllowDomains is refused naming the sandbox and both accepted modes; the two expressible modes
/// map to the boolean. Mutation check: return `Ok` for AllowDomains and this fails.
#[test]
fn egress_refuses_domain_scoping_and_names_the_modes() {
    let error = egress_control_config(
        "sbx-7",
        &SandboxEgress::AllowDomains {
            domains: vec!["x.io".into()],
        },
    )
    .expect_err("domain-scoped egress has no representation");
    assert_eq!(error.code, "INVALID_INPUT", "{error}");
    let rendered = error.to_string();
    assert!(rendered.contains("sbx-7"), "names the sandbox: {rendered}");
    assert!(
        rendered.contains("allow") && rendered.contains("deny"),
        "names both modes: {rendered}"
    );

    assert_eq!(
        egress_control_config("s", &SandboxEgress::Deny)
            .expect("deny maps")
            .internet_access,
        Some(false)
    );
    assert_eq!(
        egress_control_config("s", &SandboxEgress::Allow)
            .expect("allow maps")
            .internet_access,
        Some(true)
    );
}

/// A sandbox id that could address another sandbox never reaches a URL. Mutation check: weaken
/// `is_addressable_id` to accept '/' and the traversal ids below stop being refused.
#[tokio::test]
async fn a_sandbox_id_that_could_escape_its_sandbox_is_refused() {
    for id in [
        "../other",
        "a/b",
        "has space",
        "",
        "with?query",
        "with#frag",
    ] {
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
    let error = frames[1]
        .as_ref()
        .expect_err("a truncated stream is not success");
    assert!(
        error.to_string().contains("without a terminal frame"),
        "{error}"
    );
    assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "{error}");
    assert!(
        !error.retryable,
        "the command started and its end was lost, so a repeat would run it twice: {error}"
    );
}

/// A frame that arrived is proof the command ran, so a payload that will not decode leaves the
/// outcome unestablished. Reported as a response-format problem it would read as safe to repeat.
#[test]
fn a_frame_that_does_not_decode_leaves_the_outcome_unknown() {
    let frames = parse_exec_frames(&ndjson(&[serde_json::json!({
        "t": "stdout",
        "seq": 0,
        "data": "!!not base64!!",
    })]))
    .expect("frames parse");

    let error = frames[0]
        .as_ref()
        .expect_err("a payload that does not decode is not output");
    assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "{error}");
    assert!(
        error.to_string().contains("base64"),
        "the decode failure must stay in the chain: {error}"
    );
}

/// The same rule for a line that does not parse once frames have already arrived.
#[test]
fn a_frame_that_does_not_parse_after_output_leaves_the_outcome_unknown() {
    let mut body = ndjson(&[stdout_frame(0, b"partial")]);
    body.extend_from_slice(b"{not json at all}\n");

    let frames = parse_exec_frames(&body).expect("frames parse");
    let error = frames
        .last()
        .expect("a trailing item")
        .as_ref()
        .expect_err("a malformed frame is not output");
    assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "{error}");
}

/// A frame that will not convert ends the body. Letting the exit frame after it through would
/// answer the question the failure just reported as unanswerable, and a caller reading to the end
/// would believe the wrong one of the two.
#[test]
fn a_frame_that_does_not_convert_ends_the_body() {
    let mut body = ndjson(&[serde_json::json!({
        "t": "stdout",
        "seq": 0,
        "data": "!!not base64!!",
    })]);
    body.extend_from_slice(&ndjson(&[exit_frame(0)]));

    let frames = parse_exec_frames(&body).expect("frames parse");
    assert_eq!(
        frames.len(),
        1,
        "the exit frame must not follow the failure"
    );
    assert_eq!(
        frames[0]
            .as_ref()
            .expect_err("a bad payload is not output")
            .code,
        "SANDBOX_OUTCOME_UNKNOWN"
    );
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
    assert!(
        is_not_found(&not_found()),
        "a wrapped 404 is a gone sandbox"
    );
    assert!(
        !is_not_found(&execute_refused()),
        "an ordinary execute failure is not a gone sandbox"
    );
}

#[test]
fn the_engine_is_reduced_to_a_bare_segment() {
    let provider = provider(MockAgentPlatformApi::new());
    assert_eq!(
        provider.engine(),
        "eng1",
        "the full resource name is reduced to the engine id"
    );
}

// ---- capabilities, sandbox fields and terminate idempotency ------------------------------------

/// The access denial in the client's own form: what a delete under an engine this deployment was
/// not granted returns. The API answers a cross-engine call with `PERMISSION_DENIED` naming the
/// sandbox environment, so the denial arrives as a refusal rather than as a not-found.
fn access_denied() -> AlienError<AgentPlatformErrorData> {
    AlienError::new(alien_client_core::ErrorData::RemoteAccessDenied {
        resource_type: "SandboxEnvironment".to_string(),
        resource_name: "s1".to_string(),
    })
    .context(AgentPlatformErrorData::RequestFailed {
        operation: "delete sandbox".to_string(),
        message: "s1".to_string(),
    })
}

/// Pins `capabilities()`'s doc: `sandboxLifetime` must stay true even for a sandbox with no
/// declared ttl, the case a narrowing would get wrong (Agent Platform always sets `expireTime`).
#[test]
fn capabilities_describe_the_backend_not_this_declaration() {
    let platform = SandboxCapabilities::gcp_agent_platform();

    assert_eq!(
        provider(MockAgentPlatformApi::new()).capabilities(),
        platform
    );

    let untimed = GcpAgentPlatformSandbox::new(
        Arc::new(MockAgentPlatformApi::new()),
        ENGINE_FULL.to_string(),
        TEMPLATE.to_string(),
        None,
    );
    assert_eq!(
        untimed.capabilities(),
        platform,
        "a sandbox with no declared ttl still expires, so the row does not change"
    );
    assert!(
        platform.sandbox_lifetime,
        "Agent Platform always sets expireTime"
    );
}

/// Pins the refusal, not the message: `create_sandbox` must never be called — the failure
/// this guards is a sandbox that starts anyway and serves every tenant from one box.
#[tokio::test]
async fn a_tenant_key_is_refused_rather_than_dropped() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_create_sandbox().never();

    let error = provider(client)
        .create(CreateSandboxRequest {
            tenant_key: Some("tenant-1".to_string()),
            ..Default::default()
        })
        .await
        .expect_err("a tenant key Agent Platform cannot honour is refused");

    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
    assert!(
        error.to_string().contains("tenantKey"),
        "the refusal has to name the field a caller must remove: {error}"
    );
}

/// Terminating an already-gone sandbox succeeds — gone is the state it asks for. The poll is
/// expected never: reaching it would mean not-found had been treated as a failure.
#[tokio::test]
async fn terminate_of_an_absent_sandbox_succeeds() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_delete_sandbox()
        .times(1)
        .returning(|_, _| Err(not_found()));
    client.expect_get_sandbox().never();

    provider(client)
        .terminate("s1")
        .await
        .expect("terminating an absent sandbox succeeds");
}

/// A sandbox this deployment cannot reach stays refused: mapping every delete failure to `Ok`
/// would report containment for a sandbox under another deployment's engine that was never
/// deleted. Only not-found may pass.
#[tokio::test]
async fn terminate_of_a_sandbox_this_deployment_cannot_reach_is_still_refused() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_delete_sandbox()
        .times(1)
        .returning(|_, _| Err(access_denied()));
    client.expect_get_sandbox().never();

    let error = provider(client)
        .terminate("s1")
        .await
        .expect_err("a refused delete is not containment");

    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
    assert!(
        format!("{error}").to_lowercase().contains("denied"),
        "the refusal must carry why the delete was refused, got: {error}"
    );
}
