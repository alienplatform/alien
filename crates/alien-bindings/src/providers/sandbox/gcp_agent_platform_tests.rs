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
        create_time: None,
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

/// What the agent answers to a marker script that ran and succeeded.
fn marker_script_reply() -> Vec<u8> {
    ndjson(&[exit_frame(0)])
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
                // Every new sandbox carries the pause contract in its name.
                && request.display_name.as_deref() == Some("alienSandbox-pm1")
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
    // The marker the sandbox is left with at create.
    client
        .expect_execute()
        .withf(|_, sandbox, input| sandbox == "s1" && op_of(input) == "exec")
        .times(1)
        .returning(|_, _, _| Ok(marker_script_reply()));

    let sandbox = provider(client)
        .create(CreateSandboxRequest::default())
        .await
        .expect("create succeeds");

    assert_eq!(sandbox.sandbox_id, "s1");
    assert_eq!(sandbox.state, SandboxState::Running);
}

/// `create` resolves only once the agent answers: a probe the proxy refuses while the container is
/// still coming up is retried, and one that never answers within the budget deletes the sandbox.
#[tokio::test(start_paused = true)]
async fn create_waits_for_the_agent_and_deletes_the_sandbox_when_it_never_answers() {
    fn bad_gateway() -> AlienError<AgentPlatformErrorData> {
        AlienError::new(AgentPlatformErrorData::ExecuteFailed {
            sandbox: "s1".to_string(),
            message: "Bad Gateway: Unable to reach the sandbox environment.".to_string(),
        })
    }
    let client_with = |answer_after: Option<usize>, deletes: usize| {
        let mut client = MockAgentPlatformApi::new();
        client
            .expect_create_sandbox()
            .returning(|_, _| Ok(done_op(serde_json::json!({ "name": sandbox_name("s1") }))));
        client
            .expect_get_sandbox()
            .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
        let probes = AtomicUsize::new(0);
        client.expect_execute().returning(move |_, _, input| {
            if op_of(input) != "health" {
                return Ok(marker_script_reply());
            }
            let seen = probes.fetch_add(1, Ordering::SeqCst);
            match answer_after {
                Some(after) if seen >= after => Ok(health_reply()),
                _ => Err(bad_gateway()),
            }
        });
        client
            .expect_delete_sandbox()
            .times(deletes)
            .returning(|_, _| Ok(()));
        client
    };

    let ready = provider(client_with(Some(2), 0))
        .create(CreateSandboxRequest::default())
        .await
        .expect("the agent answers on the third probe");
    assert_eq!(ready.sandbox_id, "s1");

    let error = provider(client_with(None, 1))
        .create(CreateSandboxRequest::default())
        .await
        .expect_err("an agent that never answers fails create");
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Bad Gateway") && rendered.contains("did not become servable within"),
        "the timeout sits over the last probe's cause: {rendered}"
    );
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
        client.expect_execute().returning(|_, _, input| {
            Ok(if op_of(input) == "health" {
                health_reply()
            } else {
                marker_script_reply()
            })
        });

        provider(client)
            .create(CreateSandboxRequest {
                timeout_ms: Some(timeout_ms),
                ..Default::default()
            })
            .await
            .expect("create succeeds");
    }
}

/// The first marker is best effort: when every write fails, create still returns the sandbox and
/// does not delete it (another caller may already hold it by its key).
#[tokio::test(start_paused = true)]
async fn create_returns_the_sandbox_when_the_first_marker_cannot_be_written() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_create_sandbox()
        .returning(|_, _| Ok(done_op(serde_json::json!({ "name": sandbox_name("s1") }))));
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client.expect_execute().returning(|_, _, input| {
        if op_of(input) == "health" {
            Ok(health_reply())
        } else {
            Err(execute_refused())
        }
    });
    client.expect_delete_sandbox().never();

    let instance = provider(client)
        .create(CreateSandboxRequest::default())
        .await
        .expect("a failed first marker does not fail the create");
    assert_eq!(instance.sandbox_id, "s1");
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
        .get("1001")
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
        .get("1001")
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
        .withf(|_, sandbox| sandbox == "1002")
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .withf(|_, sandbox, _| sandbox == "1002")
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
    client
        .expect_execute()
        .withf(|_, sandbox, input| sandbox == "fresh" && op_of(input) == "exec")
        .returning(|_, _, _| Ok(marker_script_reply()));

    client.expect_delete_sandbox().never();

    let sandbox = provider(client)
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("1002".to_string()),
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

/// A reconnect to a sandbox still coming up waits for it. Replacing it would leave the first one
/// starting, reaching RUNNING and costing its owner, with nobody holding its id. Mutation check:
/// delete the `Starting` arm and `create_sandbox().never()` fires.
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

    let sandbox = provider(client)
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("1003".to_string()),
            ..Default::default()
        })
        .await
        .expect("a booting sandbox is waited for and handed back");

    assert_eq!(sandbox.sandbox.sandbox_id, "1003");
    assert_eq!(sandbox.sandbox.state, SandboxState::Running);
    assert!(
        !sandbox.created,
        "whoever started it created it, not this call"
    );
}

/// A paused record cannot be woken with its state kept, so a reconnect to one is served by a fresh
/// sandbox, and the paused one, which may be another revision's, is left untouched.
#[tokio::test]
async fn get_or_create_replaces_a_paused_sandbox_without_touching_it() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_get_sandbox().returning(|_, id| {
        if id == "1004" {
            Ok(sandbox_in_state(id, "STATE_PAUSED"))
        } else {
            Ok(sandbox_in_state(id, "STATE_RUNNING"))
        }
    });
    client.expect_create_sandbox().times(1).returning(|_, _| {
        Ok(done_op(
            serde_json::json!({ "name": sandbox_name("fresh") }),
        ))
    });
    client
        .expect_execute()
        .withf(|_, sandbox, input| sandbox == "fresh" && op_of(input) == "health")
        .returning(|_, _, _| Ok(health_reply()));
    client
        .expect_execute()
        .withf(|_, sandbox, input| sandbox == "fresh" && op_of(input) == "exec")
        .returning(|_, _, _| Ok(marker_script_reply()));
    client.expect_delete_sandbox().never();

    let resolved = provider(client)
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("1004".to_string()),
            ..Default::default()
        })
        .await
        .expect("a fresh sandbox serves the reconnect");
    assert_eq!(resolved.sandbox.sandbox_id, "fresh");
    assert!(resolved.created);
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

    let sandbox = provider(client)
        .get_or_create(CreateSandboxRequest {
            sandbox_id: Some("1005".to_string()),
            ..Default::default()
        })
        .await
        .expect("a wake already in flight is waited out");

    assert!(!sandbox.created, "the sandbox existed before this call");
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
        .get("1001")
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
        .get("1001")
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
        .get("1001")
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
    let error = provider_from(Arc::new(WedgedAgent::default()))
        .get("1001")
        .await
        .expect_err("a wedged agent is unreachable, not a hang");
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
}

/// The first readiness probe after create wedges, so the wait's own deadline cuts it off with no
/// finished probe to report, and the sandbox the caller never received is still deleted.
#[tokio::test(start_paused = true)]
async fn create_gives_up_on_a_wedged_first_probe_and_deletes_the_sandbox() {
    let client = Arc::new(WedgedAgent::default());
    let started = tokio::time::Instant::now();
    let error = provider_from(client.clone())
        .create(CreateSandboxRequest::default())
        .await
        .expect_err("a wedged agent fails create, not hangs it");
    assert!(
        started.elapsed() < AGENT_PROBE_BUDGET,
        "the wait's deadline, not the probe's own budget, ended the wait"
    );
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
    assert!(
        error.to_string().contains("did not become servable within"),
        "{error}"
    );
    assert_eq!(client.deletes.load(Ordering::SeqCst), 1);
}

/// A client whose sandbox reads RUNNING but whose `execute` never answers, standing in for an agent
/// that accepts the health probe and then wedges. Only the methods `get` and `create` reach are
/// real; the rest are unreachable in these tests.
#[derive(Debug, Default)]
struct WedgedAgent {
    deletes: AtomicUsize,
}

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

    async fn create_engine(
        &self,
        _display_name: &str,
        _description: Option<String>,
    ) -> ClientResult<Operation> {
        unimplemented!()
    }
    async fn delete_engine(&self, _engine: &str) -> ClientResult<()> {
        unimplemented!()
    }
    async fn list_engines(&self) -> ClientResult<Vec<ReasoningEngine>> {
        unimplemented!()
    }
    async fn get_engine_iam_policy(
        &self,
        _engine: &str,
    ) -> ClientResult<alien_gcp_clients::iam::IamPolicy> {
        unimplemented!()
    }
    async fn set_engine_iam_policy(
        &self,
        _engine: &str,
        _policy: alien_gcp_clients::iam::IamPolicy,
    ) -> ClientResult<alien_gcp_clients::iam::IamPolicy> {
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
        Ok(done_op(serde_json::json!({ "name": sandbox_name("s1") })))
    }
    async fn list_sandboxes(&self, _engine: &str) -> ClientResult<Vec<SandboxEnvironment>> {
        unimplemented!()
    }
    async fn list_sandboxes_by_display_names(
        &self,
        _engine: &str,
        _display_names: &[String],
    ) -> ClientResult<Vec<SandboxEnvironment>> {
        unimplemented!()
    }
    async fn delete_sandbox(&self, _engine: &str, _sandbox: &str) -> ClientResult<()> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn pause_sandbox(&self, _engine: &str, _sandbox: &str) -> ClientResult<Operation> {
        unimplemented!()
    }
    async fn resume_sandbox(&self, _engine: &str, _sandbox: &str) -> ClientResult<Operation> {
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
            .get("1001")
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

/// A job whose every poll answers `poll`.
fn cancel_watching_client(
    poll: serde_json::Value,
) -> (
    MockAgentPlatformApi,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    cancel_watching_client_answering(move || Ok(serde_json::to_vec(&poll).unwrap()))
}

/// A job whose first poll answers `poll` and whose every later poll fails, so the stream ends on a
/// failure while the job is still running.
fn cancel_watching_client_failing_after_one_poll(
    poll: serde_json::Value,
) -> (
    MockAgentPlatformApi,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    let polls = AtomicUsize::new(0);
    cancel_watching_client_answering(move || {
        if polls.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(serde_json::to_vec(&poll).unwrap())
        } else {
            Err(execute_refused())
        }
    })
}

/// A job whose every poll answers `poll` and whose every cancel fails, handing each `jobCancel`
/// body it received back through the channel before refusing it.
fn cancel_refusing_client(
    poll: serde_json::Value,
) -> (
    MockAgentPlatformApi,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    let (cancels, seen) = tokio::sync::mpsc::unbounded_channel();
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(move |_, _, input| match op_of(input).as_str() {
            "jobStart" => Ok(serde_json::to_vec(&serde_json::json!({ "jobId": "j1" })).unwrap()),
            "jobPoll" => Ok(serde_json::to_vec(&poll).unwrap()),
            "jobCancel" => {
                cancels
                    .send(serde_json::from_slice(input).expect("a cancel body is json"))
                    .expect("the test still listens for cancels");
                Err(execute_refused())
            }
            other => panic!("unexpected op {other}"),
        });
    (client, seen)
}

/// A job whose `jobPoll` calls answer from `poll`, handing each `jobCancel` body it receives back
/// through the channel so a test can wait for one or wait out its absence.
fn cancel_watching_client_answering(
    mut poll: impl FnMut() -> ClientResult<Vec<u8>> + Send + 'static,
) -> (
    MockAgentPlatformApi,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    let (cancels, seen) = tokio::sync::mpsc::unbounded_channel();
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(move |_, _, input| match op_of(input).as_str() {
            "jobStart" => Ok(serde_json::to_vec(&serde_json::json!({ "jobId": "j1" })).unwrap()),
            "jobPoll" => poll(),
            "jobCancel" => {
                cancels
                    .send(serde_json::from_slice(input).expect("a cancel body is json"))
                    .expect("the test still listens for cancels");
                Ok(b"{}".to_vec())
            }
            other => panic!("unexpected op {other}"),
        });
    (client, seen)
}

/// Dropping a command's stream kills the command. A detached job has no transport whose close
/// says so, so the drop has to send the cancel itself or the job runs to its full timeout with
/// nobody reading it. Mutation check: empty `JobPollState::drop` and no cancel arrives.
#[tokio::test(start_paused = true)]
async fn dropping_a_running_jobs_stream_cancels_the_job() {
    let (client, mut cancels) = cancel_watching_client(serde_json::json!({
        "running": true,
        "frames": [stdout_frame(0, b"work")],
    }));

    // Held for the whole test: it owns the client double, and with it the end of the channel the
    // cancel would arrive on.
    let sandbox = provider(client);
    let mut frames = sandbox
        .run_command("s1", long_command())
        .await
        .expect("the job starts");
    assert!(
        matches!(frames.next().await, Some(Ok(CommandOutput::Stdout { .. }))),
        "the job is running and has produced output"
    );
    drop(frames);

    let cancel = tokio::time::timeout(Duration::from_secs(5), cancels.recv())
        .await
        .expect("the drop cancels the job")
        .expect("the cancel carries a body");
    assert_eq!(cancel["op"], "jobCancel", "{cancel}");
    assert_eq!(cancel["jobId"], "j1", "{cancel}");
}

/// The cancel is for a job still running: one that reported its exit is spent, and a call naming a
/// finished job id is a call about nothing. Mutation check: delete `state.stopped = true` from the
/// `!poll.running` branch and a cancel arrives here.
#[tokio::test(start_paused = true)]
async fn a_job_that_reported_its_exit_is_not_cancelled_afterwards() {
    let (client, mut cancels) = cancel_watching_client(serde_json::json!({
        "running": false,
        "frames": [stdout_frame(0, b"work")],
        "exitCode": 0,
        "truncated": false,
    }));

    // Held for the whole test: it owns the client double, and with it the end of the channel the
    // cancel would arrive on, so waiting for one that never comes is a timeout and not a close.
    let sandbox = provider(client);
    let frames: Vec<_> = sandbox
        .run_command("s1", long_command())
        .await
        .expect("the job starts")
        .collect()
        .await;
    assert!(
        matches!(frames.last(), Some(Ok(CommandOutput::Exit { code: 0, .. }))),
        "the job ended on its own"
    );

    assert!(
        tokio::time::timeout(Duration::from_secs(5), cancels.recv())
            .await
            .is_err(),
        "a job that ended on its own is not cancelled afterwards"
    );
}

/// A poll that fails ends the stream while the job is still running, so the drop cancels it.
/// Mutation check: guard `JobPollState::drop` on `finished` and no cancel arrives.
#[tokio::test(start_paused = true)]
async fn a_stream_that_ended_on_a_failed_poll_still_cancels_the_job() {
    let (client, mut cancels) = cancel_watching_client_failing_after_one_poll(serde_json::json!({
        "running": true,
        "frames": [stdout_frame(0, b"work")],
    }));

    // Held for the whole test: it owns the client double, and with it the end of the channel the
    // cancel would arrive on.
    let sandbox = provider(client);
    let frames: Vec<_> = sandbox
        .run_command("s1", long_command())
        .await
        .expect("the job starts")
        .collect()
        .await;
    let Some(Err(error)) = frames.last() else {
        panic!("the failed poll reaches the caller: {frames:?}");
    };
    assert!(
        error.to_string().contains("the job is no longer watched"),
        "{error}"
    );

    let cancel = tokio::time::timeout(Duration::from_secs(5), cancels.recv())
        .await
        .expect("the drop cancels the job the failed poll left running")
        .expect("the cancel carries a body");
    assert_eq!(cancel["op"], "jobCancel", "{cancel}");
    assert_eq!(cancel["jobId"], "j1", "{cancel}");
}

/// A deadline only stops the job once its cancel has landed: the refused half leaves the drop
/// armed, and the landed half is what says the flag is read rather than stuck at false.
/// Mutation check: set `state.stopped = true` in the deadline branch and the refused half's
/// second cancel never arrives.
#[tokio::test(start_paused = true)]
async fn a_deadline_arms_the_drop_unless_its_cancel_landed() {
    // No frames, so the loop sleeps between polls and the paused clock reaches the deadline.
    let (client, mut cancels) = cancel_refusing_client(serde_json::json!({
        "running": true,
        "frames": [],
    }));

    // Held for the whole test: it owns the client double, and with it the end of the channel the
    // cancel would arrive on.
    let refused = provider(client);
    let frames: Vec<_> = refused
        .run_command("s1", long_command())
        .await
        .expect("the job starts")
        .collect()
        .await;
    let Some(Err(error)) = frames.last() else {
        panic!("the elapsed deadline reaches the caller: {frames:?}");
    };
    assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "{error}");
    assert!(
        error.to_string().contains("could not be cancelled"),
        "{error}"
    );

    for attempt in ["the deadline", "the drop"] {
        let cancel = tokio::time::timeout(Duration::from_secs(5), cancels.recv())
            .await
            .unwrap_or_else(|_| panic!("{attempt} cancels the job it could not stop"))
            .expect("the cancel carries a body");
        assert_eq!(cancel["op"], "jobCancel", "{cancel}");
        assert_eq!(cancel["jobId"], "j1", "{cancel}");
    }

    let (client, mut cancels) = cancel_watching_client(serde_json::json!({
        "running": true,
        "frames": [],
    }));

    let landed = provider(client);
    let frames: Vec<_> = landed
        .run_command("s1", long_command())
        .await
        .expect("the job starts")
        .collect()
        .await;
    let Some(Err(error)) = frames.last() else {
        panic!("the elapsed deadline reaches the caller: {frames:?}");
    };
    assert!(error.to_string().contains("timeoutExceeded"), "{error}");

    tokio::time::timeout(Duration::from_secs(5), cancels.recv())
        .await
        .expect("the deadline cancels the job")
        .expect("the cancel carries a body");
    assert!(
        tokio::time::timeout(Duration::from_secs(5), cancels.recv())
            .await
            .is_err(),
        "a job whose cancel landed is not cancelled again on drop"
    );
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
                "error": { "code": "timeoutExceeded", "message": "exceeded its 60000ms timeout" },
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

/// The `:execute` proxy relays an agent's 404 as its own, so a file the agent cannot find must not
/// read as a gone sandbox, while a 404 without an agent code still does. A relayed 5xx carries the
/// same shape but proves nothing about a command, so it stays outcome-unknown.
#[tokio::test]
async fn a_relayed_agent_404_is_a_refusal_not_a_gone_sandbox() {
    fn relayed(status: u16, details: &'static str) -> AlienError<AgentPlatformErrorData> {
        let body = serde_json::json!({
            "error": {
                "code": status,
                "message": format!("Execution Failed. URL not found `https://x.sandbox.vertexai.goog`. Error Details: {details}"),
            }
        })
        .to_string();
        let http = AlienError::new(alien_client_core::ErrorData::HttpResponseError {
            message: format!("Request failed with HTTP {status}"),
            url: "https://example.invalid/:execute".to_string(),
            http_status: status,
            http_request_text: None,
            http_response_text: Some(body),
        });
        let http = if status == 404 {
            http.context(alien_client_core::ErrorData::RemoteResourceNotFound {
                resource_type: "Vertex AI Agent Platform".to_string(),
                resource_name: "s1".to_string(),
            })
        } else {
            http
        };
        http.context(AgentPlatformErrorData::ExecuteFailed {
            sandbox: "s1".to_string(),
            message: "the API rejected or cut short the request".to_string(),
        })
    }
    fn relayed_404(details: &'static str) -> AlienError<AgentPlatformErrorData> {
        relayed(404, details)
    }

    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().returning(|_, _, _| {
        Err(relayed_404(
            "PATH_NOT_FOUND: No such file in the sandbox: /before",
        ))
    });
    let error = provider(client)
        .read_file("s1", "/before")
        .await
        .expect_err("a missing file is an error");
    let rendered = format!("{error:?}");
    assert!(rendered.contains("agentRefused"), "{rendered}");
    assert!(rendered.contains("PATH_NOT_FOUND"), "{rendered}");

    assert!(agent_answer(&not_found()).is_none());
    assert!(agent_answer(&relayed_404(
        "Bad Gateway: Unable to reach the sandbox environment."
    ))
    .is_none());

    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(move |_, _, _| Err(relayed(500, "INTERNAL_SERVER_ERROR: stream reset")));
    let Err(command) = provider(client)
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
        panic!("a relayed 5xx fails the command");
    };
    assert_eq!(command.code, "SANDBOX_OUTCOME_UNKNOWN", "{command}");
}

// ---- pause / resume / snapshot ----------------------------------------------------------------

/// A refused call as the real client reports it: the transport's `HttpResponseError` under the
/// client's `RequestFailed`.
fn http_error(status: u16, message: &str) -> AlienError<AgentPlatformErrorData> {
    AlienError::new(alien_client_core::ErrorData::HttpResponseError {
        message: format!("Request failed with HTTP {status}"),
        url: "https://example.invalid/sandbox".to_string(),
        http_status: status,
        http_request_text: None,
        http_response_text: Some(
            serde_json::json!({ "error": { "code": status, "message": message } }).to_string(),
        ),
    })
    .context(AgentPlatformErrorData::RequestFailed {
        operation: "resume sandbox".to_string(),
        message: "s1".to_string(),
    })
}

/// What a fake Agent Platform holds for one sandbox: the state the API reports and the boot id its
/// agent answers with.
struct PausableSandbox {
    state: std::sync::Mutex<&'static str>,
    boot_id: std::sync::Mutex<&'static str>,
    /// The pause marker's contents; a replaced container starts without it.
    file: std::sync::Mutex<Option<Vec<u8>>>,
    /// Makes reading the marker fail the way an agent that is not ready does.
    fail_marker_reads: std::sync::atomic::AtomicBool,
    /// The marker file is there but `cat` cannot read it.
    marker_unreadable: std::sync::atomic::AtomicBool,
    /// The sandbox was created with the pause contract: its display name carries the tag.
    tagged: std::sync::atomic::AtomicBool,
    /// The image has no shell, so a marker script exits 127.
    no_shell: std::sync::atomic::AtomicBool,
    /// Every marker write fails (neither `/dev/shm` nor `/tmp` is writable, or the agent drops it).
    fail_marker_writes: std::sync::atomic::AtomicBool,
}

impl PausableSandbox {
    fn running(boot_id: &'static str) -> Arc<Self> {
        Arc::new(Self {
            state: std::sync::Mutex::new("STATE_RUNNING"),
            boot_id: std::sync::Mutex::new(boot_id),
            file: std::sync::Mutex::new(None),
            fail_marker_reads: std::sync::atomic::AtomicBool::new(false),
            marker_unreadable: std::sync::atomic::AtomicBool::new(false),
            tagged: std::sync::atomic::AtomicBool::new(false),
            no_shell: std::sync::atomic::AtomicBool::new(false),
            fail_marker_writes: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// What the sandbox's agent answers to the envelope `input`.
    fn agent_reply(&self, input: &[u8]) -> ClientResult<Vec<u8>> {
        let value: serde_json::Value = serde_json::from_slice(input).unwrap();
        match op_of(input).as_str() {
            "health" => Ok(health_reply_with_boot(*self.boot_id.lock().unwrap())),
            // The marker travels as a shell command, so the fake reads the script the way a
            // shell would: the paths it names must be outside the session root, and what it
            // prints is what a real `cat` would.
            "exec" => {
                let script = value["command"][2].as_str().expect("a /bin/sh -c script");
                if self.no_shell.load(Ordering::SeqCst) {
                    return Ok(ndjson(&[exit_frame(127)]));
                }
                if self.fail_marker_reads.load(Ordering::SeqCst) && script.contains("cat ") {
                    return Err(http_error(503, "the agent is not ready"));
                }
                if self.fail_marker_writes.load(Ordering::SeqCst) && script.contains("printf %s '")
                {
                    return Ok(ndjson(&[exit_frame(1)]));
                }
                let paths: Vec<&str> = script
                    .split_whitespace()
                    .filter(|word| word.contains(".alien-pause-generation"))
                    .collect();
                assert!(!paths.is_empty(), "{script}");
                assert!(
                    paths
                        .iter()
                        .all(|p| p.starts_with("/dev/shm/") || p.starts_with("/tmp/")),
                    "the marker must stay out of the workload's folder: {script}"
                );
                let output: String = if let Some(rest) = script.split("printf %s '").nth(1) {
                    let generation = rest.split('\'').next().unwrap();
                    *self.file.lock().unwrap() = Some(generation.as_bytes().to_vec());
                    String::new()
                } else if script.contains("cat ") {
                    match self.file.lock().unwrap().as_ref() {
                        Some(_) if self.marker_unreadable.load(Ordering::SeqCst) => {
                            "unreadable".to_string()
                        }
                        Some(contents) => {
                            format!("gen:{}", String::from_utf8(contents.clone()).unwrap())
                        }
                        None => "none".to_string(),
                    }
                } else {
                    *self.file.lock().unwrap() = None;
                    "ok".to_string()
                };
                Ok(ndjson(&[stdout_frame(0, output.as_bytes()), exit_frame(0)]))
            }
            other => panic!("fake agent: unexpected op {other}"),
        }
    }

    /// The state a sandbox paused by `pause` is in: the marker holds `boot_id`'s generation.
    fn paused_with_marker(&self) {
        let boot_id = *self.boot_id.lock().unwrap();
        *self.file.lock().unwrap() =
            Some(generation_from_boot_id(boot_id).to_string().into_bytes());
        self.set_state("STATE_PAUSED");
    }

    /// A container that replaced this one: another boot id, and nothing the old one wrote.
    fn replace_container(&self, boot_id: &'static str) {
        *self.boot_id.lock().unwrap() = boot_id;
        *self.file.lock().unwrap() = None;
    }

    fn set_state(&self, state: &'static str) {
        *self.state.lock().unwrap() = state;
    }
}

/// A client whose `:pause` and `:resume` move the fake between states, with `boot_after_resume` the
/// boot id the agent answers with once it has been resumed.
fn pausing_client(
    fake: &Arc<PausableSandbox>,
    boot_after_resume: &'static str,
) -> MockAgentPlatformApi {
    let mut client = MockAgentPlatformApi::new();
    let state = fake.clone();
    client
        .expect_get_sandbox()
        .returning(move |_, id| Ok(pausable_sandbox_record(&state, id)));
    let agent = fake.clone();
    client
        .expect_execute()
        .returning(move |_, _, input| agent.agent_reply(input));
    let paused = fake.clone();
    client.expect_pause_sandbox().returning(move |_, _| {
        paused.set_state("STATE_PAUSED");
        Ok(done_op(serde_json::json!({})))
    });
    let resumed = fake.clone();
    client.expect_resume_sandbox().returning(move |_, _| {
        resumed.set_state("STATE_RUNNING");
        if *resumed.boot_id.lock().unwrap() != boot_after_resume {
            resumed.replace_container(boot_after_resume);
        }
        Ok(done_op(serde_json::json!({})))
    });
    client
}

/// The record Google would return for the fake: tagged when it was created with the contract.
fn pausable_sandbox_record(fake: &PausableSandbox, id: &str) -> SandboxEnvironment {
    let mut record = sandbox_in_state(id, *fake.state.lock().unwrap());
    if fake.tagged.load(Ordering::SeqCst) {
        record.display_name = Some("alienSandbox-pm1".to_string());
    }
    record
}

#[test]
fn the_capability_row_offers_pause_and_resume() {
    assert!(SandboxCapabilities::gcp_agent_platform().pause_resume);
}

/// A pause is `:pause` to `STATE_PAUSED`, and a resume that finds the container it paused returns.
#[tokio::test]
async fn pause_then_resume_keeps_the_generation() {
    let fake = PausableSandbox::running("boot-a");
    let provider = provider(pausing_client(&fake, "boot-a"));
    let before = provider
        .get("1234567890123456789")
        .await
        .unwrap()
        .unwrap()
        .generation;

    provider.pause("1234567890123456789").await.expect("pause");
    assert_eq!(*fake.state.lock().unwrap(), "STATE_PAUSED");
    provider
        .resume("1234567890123456789")
        .await
        .expect("resume");

    let after = provider.get("1234567890123456789").await.unwrap().unwrap();
    assert_eq!(after.state, SandboxState::Running);
    assert_eq!(after.generation, before, "the same container is back");
}

/// The regression guard: a container the service replaced under a stable sandbox name must never
/// reach a caller as a kept one.
#[tokio::test]
async fn a_resume_that_finds_a_new_container_is_an_error_not_a_kept_sandbox() {
    let fake = PausableSandbox::running("boot-a");
    let provider = provider(pausing_client(&fake, "boot-b"));

    provider.pause("s1").await.expect("pause");
    let lost = provider
        .resume("s1")
        .await
        .expect_err("a fresh container is not a resume");

    assert_eq!(lost.code, "SANDBOX_MEMORY_LOST", "{lost}");
    assert!(!lost.retryable, "a retry cannot bring the memory back");
    assert!(
        lost.message.contains("memory") && lost.message.contains("restarted"),
        "{}",
        lost.message
    );
}

/// The marker lives in the sandbox, so a resume in another process compares the same thing: a
/// sandbox paused by one binding is resumed by a second that never saw the pause, and a container
/// replaced in between is still caught.
#[tokio::test]
async fn a_resume_in_another_process_still_compares_with_the_pause() {
    for (boot_after, kept) in [("boot-a", true), ("boot-b", false)] {
        let fake = PausableSandbox::running("boot-a");
        let client: Arc<dyn AgentPlatformApi> = Arc::new(pausing_client(&fake, boot_after));

        provider_from(client.clone())
            .pause("s1")
            .await
            .expect("pause in the first process");
        let outcome = provider_from(client).resume("s1").await;

        assert_eq!(outcome.is_ok(), kept, "{outcome:?}");
        if let Err(lost) = outcome {
            assert_eq!(lost.code, "SANDBOX_MEMORY_LOST", "{lost}");
            assert!(!lost.retryable);
        }
    }
}

/// A sandbox with no pause marker was not paused by this binding, or lost its filesystem: its
/// memory cannot be confirmed kept, so a resume does not claim it.
#[tokio::test]
async fn a_resume_without_a_pause_marker_is_not_reported_as_kept() {
    let fake = PausableSandbox::running("boot-a");
    fake.set_state("STATE_PAUSED");
    let lost = provider(pausing_client(&fake, "boot-a"))
        .resume("s1")
        .await
        .expect_err("nothing says the memory was kept");
    assert_eq!(lost.code, "SANDBOX_MEMORY_LOST", "{lost}");
    assert!(!lost.retryable);
}

/// A caller holding lifecycle access only is refused by `:execute`; the error names the permission
/// sets it is missing, and nothing is paused without the memory check.
#[tokio::test]
async fn pause_and_resume_without_execute_access_name_the_missing_permission() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client.expect_execute().returning(|_, _, _| {
        Err(http_error(
            403,
            "Permission denied on aiplatform.sandboxEnvironments.execute",
        ))
    });
    client.expect_pause_sandbox().never();
    let provider = provider(client);

    let paused = provider.pause("s1").await.expect_err("no execute access");
    assert_eq!(paused.code, "SANDBOX_COMMAND_FAILED", "{paused}");
    assert!(!paused.retryable, "{paused}");
    assert!(
        paused.message.contains("sandbox/execute"),
        "{}",
        paused.message
    );
    assert!(
        paused.message.contains("sandbox/management"),
        "{}",
        paused.message
    );

    let resumed = provider.resume("s1").await.expect_err("no execute access");
    assert!(
        resumed.message.contains("sandbox/execute"),
        "{}",
        resumed.message
    );
}

/// Two callers resuming one paused sandbox both get it back: the first does not take the marker
/// away from the second.
#[tokio::test]
async fn two_resumes_of_one_paused_sandbox_both_succeed() {
    let fake = PausableSandbox::running("boot-a");
    let client: Arc<dyn AgentPlatformApi> = Arc::new(pausing_client(&fake, "boot-a"));
    provider_from(client.clone())
        .pause("s1")
        .await
        .expect("pause");
    // The second caller read the state while the sandbox was still paused, so it is not the
    // "running on entry" case that tolerates a missing marker.
    provider_from(client.clone())
        .resume("s1")
        .await
        .expect("first resume");
    fake.set_state("STATE_PAUSED");
    provider_from(client)
        .resume("s1")
        .await
        .expect("second resume");
}

/// A marker that cannot be read is not a missing marker: the sandbox may well have kept its memory,
/// and saying it lost it would be a false report. The failure passes through as it is.
#[tokio::test]
async fn a_resume_whose_marker_read_fails_does_not_claim_lost_memory() {
    let fake = PausableSandbox::running("boot-a");
    fake.paused_with_marker();
    fake.fail_marker_reads.store(true, Ordering::SeqCst);
    let error = provider(pausing_client(&fake, "boot-a"))
        .resume("s1")
        .await
        .expect_err("the marker could not be read");
    assert_ne!(error.code, "SANDBOX_MEMORY_LOST", "{error}");
}

/// A sandbox made with the pause contract that Google replaces with a new container, then woken
/// by a request, is running on entry to `resume` with nothing in `/dev/shm`. The tag, which lives
/// outside the container, is what says a marker should be there: the loss is reported.
#[tokio::test]
async fn a_tagged_sandbox_woken_into_a_replaced_container_is_lost_on_resume() {
    let fake = PausableSandbox::running("boot-a");
    fake.tagged.store(true, Ordering::SeqCst);
    fake.paused_with_marker();
    // A request woke it into a new container before `resume` was called.
    fake.replace_container("boot-b");
    fake.set_state("STATE_RUNNING");
    let lost = provider(pausing_client(&fake, "boot-b"))
        .resume("1234567890123456789")
        .await
        .expect_err("the container was replaced");
    assert_eq!(lost.code, "SANDBOX_MEMORY_LOST", "{lost}");
}

/// The tag says a marker belongs there; a running sandbox with no marker and no pause at all is
/// reported lost by a routine `resume`.
#[tokio::test]
async fn a_tagged_running_sandbox_with_no_marker_is_lost_without_any_pause() {
    let fake = PausableSandbox::running("boot-a");
    fake.tagged.store(true, Ordering::SeqCst);
    let lost = provider(pausing_client(&fake, "boot-a"))
        .resume("1234567890123456789")
        .await
        .expect_err("the marker is absent");
    assert_eq!(lost.code, "SANDBOX_MEMORY_LOST", "{lost}");
    assert!(!lost.retryable);
}

/// Documents the trade-off of a best-effort first marker: when every write at create fails, create
/// still returns the sandbox, and a later routine `resume` on the healthy running sandbox reports
/// SANDBOX_MEMORY_LOST because the tag promises a marker that was never written.
#[tokio::test]
async fn a_create_whose_marker_write_failed_reports_lost_on_the_next_resume() {
    let fake = PausableSandbox::running("boot-a");
    fake.tagged.store(true, Ordering::SeqCst);
    fake.fail_marker_writes.store(true, Ordering::SeqCst);
    let mut client = pausing_client(&fake, "boot-a");
    client.expect_create_sandbox().returning(|_, _| {
        Ok(done_op(
            serde_json::json!({ "name": sandbox_name("1234567890123456789") }),
        ))
    });
    let provider = provider(client);
    let created = provider
        .create(CreateSandboxRequest::default())
        .await
        .expect("create succeeds without the marker");
    assert_eq!(created.sandbox_id, "1234567890123456789");
    assert!(fake.file.lock().unwrap().is_none(), "no marker was written");
    let lost = provider
        .resume(&created.sandbox_id)
        .await
        .expect_err("the marker never existed");
    assert_eq!(lost.code, "SANDBOX_MEMORY_LOST", "{lost}");
}

/// A tagged sandbox that is running and untouched is kept: its marker matches.
#[tokio::test]
async fn a_tagged_sandbox_that_kept_its_container_resumes() {
    let fake = PausableSandbox::running("boot-a");
    fake.tagged.store(true, Ordering::SeqCst);
    // The marker `create` leaves.
    *fake.file.lock().unwrap() = Some(generation_from_boot_id("boot-a").to_string().into_bytes());
    let provider = provider(pausing_client(&fake, "boot-a"));
    provider.resume("1234567890123456789").await.expect("kept");
    provider.pause("1234567890123456789").await.expect("pause");
    provider
        .resume("1234567890123456789")
        .await
        .expect("kept across a pause");
}

/// A sandbox made before the contract, running, with no marker: nothing was promised, a no-op.
/// (`a_resume_of_a_running_sandbox_is_a_no_op` is the same sandbox without a tag.)
#[tokio::test]
async fn an_untagged_sandbox_paused_by_an_older_path_keeps_the_old_answer() {
    let fake = PausableSandbox::running("boot-a");
    fake.set_state("STATE_PAUSED");
    // No marker and no tag: paused outside this binding, so memory cannot be confirmed.
    let lost = provider(pausing_client(&fake, "boot-a"))
        .resume("1234567890123456789")
        .await
        .expect_err("not confirmable");
    assert_eq!(lost.code, "SANDBOX_MEMORY_LOST", "{lost}");
}

/// An image whose marker script cannot run (no shell) is never read as a replaced container: the
/// script would not run in the replacement either, so nothing was found out. Pause says so by name.
#[tokio::test]
async fn a_marker_script_that_cannot_run_is_not_memory_lost() {
    let fake = PausableSandbox::running("boot-a");
    fake.tagged.store(true, Ordering::SeqCst);
    fake.no_shell.store(true, Ordering::SeqCst);
    let provider = provider(pausing_client(&fake, "boot-a"));
    let resume = provider
        .resume("1234567890123456789")
        .await
        .expect_err("the marker cannot be read");
    assert_eq!(resume.code, "SANDBOX_COMMAND_FAILED", "{resume}");
    assert!(resume.to_string().contains("/bin/sh"), "{resume}");
    let pause = provider
        .pause("1234567890123456789")
        .await
        .expect_err("the marker cannot be left");
    assert_eq!(pause.code, "SANDBOX_COMMAND_FAILED", "{pause}");
}

/// A marker that exists but cannot be read is not an absent marker: it must not be taken for a
/// replaced container, and it must not be taken for a kept one.
#[tokio::test]
async fn a_resume_whose_marker_is_unreadable_is_neither_kept_nor_lost() {
    let fake = PausableSandbox::running("boot-a");
    fake.paused_with_marker();
    fake.marker_unreadable.store(true, Ordering::SeqCst);
    let error = provider(pausing_client(&fake, "boot-a"))
        .resume("s1")
        .await
        .expect_err("the marker could not be read");
    assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "{error}");
    assert_ne!(error.code, "SANDBOX_MEMORY_LOST", "{error}");
}

/// A 403 that is not a missing permission (an API that is not enabled) is not described as one.
#[tokio::test]
async fn a_403_that_is_not_a_missing_permission_is_not_labelled_as_one() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client.expect_execute().returning(|_, _, _| {
        Err(http_error(
            403,
            "Vertex AI API has not been used in project 1 before or it is disabled",
        ))
    });
    let error = provider(client)
        .pause("s1")
        .await
        .expect_err("the agent call is refused");
    assert!(!error.to_string().contains("sandbox/execute"), "{error}");
}

/// A resume of a sandbox that is simply running, and was never paused, changes nothing.
#[tokio::test]
async fn a_resume_of_a_running_sandbox_is_a_no_op() {
    let fake = PausableSandbox::running("boot-a");
    let mut client = pausing_client(&fake, "boot-a");
    client.checkpoint();
    {
        let state = fake.clone();
        client
            .expect_get_sandbox()
            .returning(move |_, id| Ok(sandbox_in_state(id, *state.state.lock().unwrap())));
        let agent = fake.clone();
        client
            .expect_execute()
            .returning(move |_, _, input| agent.agent_reply(input));
    }
    client.expect_resume_sandbox().never();
    provider(client)
        .resume("s1")
        .await
        .expect("nothing was paused, so nothing is lost");
}

/// Two pauses of one sandbox: the one whose `:pause` is refused because the other's was accepted
/// waits for the pause instead of reporting that it failed.
#[tokio::test]
async fn a_pause_that_loses_the_race_waits_for_the_winner() {
    let fake = PausableSandbox::running("boot-a");
    let mut client = MockAgentPlatformApi::new();
    {
        let state = fake.clone();
        let reads = Arc::new(AtomicUsize::new(0));
        client.expect_get_sandbox().returning(move |_, id| {
            // Running for the first read; after the refused `:pause` the other pause shows.
            match reads.fetch_add(1, Ordering::SeqCst) {
                0 => {}
                1 => state.set_state("STATE_PAUSING"),
                _ => state.set_state("STATE_PAUSED"),
            }
            Ok(sandbox_in_state(id, *state.state.lock().unwrap()))
        });
        let agent = fake.clone();
        client
            .expect_execute()
            .returning(move |_, _, input| agent.agent_reply(input));
    }
    client
        .expect_pause_sandbox()
        .returning(|_, _| Err(http_error(400, "Sandbox environment is already pausing")));
    provider(client)
        .pause("s1")
        .await
        .expect("the pause already under way is waited for");
}

/// A resume whose agent never answers reports the resume, not a create.
#[tokio::test(start_paused = true)]
async fn a_resume_whose_agent_never_answers_is_reported_under_resume() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    client
        .expect_execute()
        .returning(|_, _, _| Err(http_error(503, "not ready")));
    let error = provider(client)
        .resume("s1")
        .await
        .expect_err("the agent never serves");
    assert!(error.to_string().contains("sandbox.resume"), "{error}");
    assert!(!error.to_string().contains("sandbox.create"), "{error}");
}

/// `:resume` is refused with a 400 while the pause is under way; that is waited out.
#[tokio::test]
async fn a_resume_refused_during_pausing_is_retried() {
    let fake = PausableSandbox::running("boot-a");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    {
        let fake = fake.clone();
        client
            .expect_get_sandbox()
            .returning(move |_, id| Ok(sandbox_in_state(id, *fake.state.lock().unwrap())));
    }
    {
        let fake = fake.clone();
        client
            .expect_execute()
            .returning(move |_, _, input| fake.agent_reply(input));
    }
    {
        let fake = fake.clone();
        let calls = calls.clone();
        client.expect_resume_sandbox().returning(move |_, _| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(http_error(
                    400,
                    "Sandbox environment is pausing and cannot be resumed",
                ));
            }
            fake.set_state("STATE_RUNNING");
            Ok(done_op(serde_json::json!({})))
        });
    }
    fake.paused_with_marker();

    provider(client)
        .resume("s1")
        .await
        .expect("the refusal during pausing is retried");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

/// A request that reached the paused sandbox first started the wake; the 503 says so, and the
/// resume waits for it instead of failing.
#[tokio::test]
async fn a_503_while_paused_is_a_wake_in_progress_and_is_waited_out() {
    let fake = PausableSandbox::running("boot-a");
    fake.paused_with_marker();
    let probes = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    {
        let fake = fake.clone();
        let reads = Arc::new(AtomicUsize::new(0));
        client.expect_get_sandbox().returning(move |_, id| {
            // The wake completes a few reads after the refusal.
            if reads.fetch_add(1, Ordering::SeqCst) >= 3 {
                fake.set_state("STATE_RUNNING");
            }
            Ok(sandbox_in_state(id, *fake.state.lock().unwrap()))
        });
    }
    client.expect_resume_sandbox().times(1).returning(|_, _| {
        Err(http_error(
            503,
            "Sandbox environment is paused; resume has been initiated",
        ))
    });
    {
        let probes = probes.clone();
        let fake = fake.clone();
        client.expect_execute().returning(move |_, _, input| {
            // The proxy answers the same 503 until the agent is back.
            if probes.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(http_error(
                    503,
                    "Sandbox environment is paused; resume has been initiated",
                ));
            }
            fake.agent_reply(input)
        });
    }

    provider(client)
        .resume("s1")
        .await
        .expect("a wake in progress is waited out");
}

/// A 400 that is not the pausing refusal, and any other failure, is the caller's to see.
#[tokio::test]
async fn a_resume_the_service_refuses_for_another_reason_is_reported() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_PAUSED")));
    client
        .expect_resume_sandbox()
        .returning(|_, _| Err(http_error(403, "permission denied")));

    let refused = provider(client).resume("s1").await.expect_err("refused");
    assert_eq!(refused.code, "SANDBOX_COMMAND_FAILED", "{refused}");
}

/// Pausing a sandbox that is not there, or already ended, says so without calling `:pause`.
#[tokio::test]
async fn pause_of_a_gone_or_ended_sandbox_is_refused_without_a_call() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_get_sandbox().returning(|_, id| {
        if id == "gone" {
            Err(not_found())
        } else {
            Ok(sandbox_in_state(id, "STATE_STOPPED"))
        }
    });
    client.expect_pause_sandbox().never();
    let provider = provider(client);
    for id in ["gone", "ended"] {
        let refused = provider.pause(id).await.expect_err("nothing to pause");
        assert_eq!(refused.code, "SANDBOX_COMMAND_FAILED", "{refused}");
    }
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

// ---- preview ----------------------------------------------------------------------------------

/// Signs a fixed credential that lives `lifetime` seconds from the call, and counts the calls so a
/// test can prove nothing was signed for a caller that was refused first.
#[derive(Debug)]
struct FixedSigner {
    lifetime: i64,
    signed: Arc<AtomicUsize>,
}

impl FixedSigner {
    fn new() -> Self {
        Self::living(300)
    }

    fn living(lifetime: i64) -> Self {
        Self {
            lifetime,
            signed: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl EdgeTokenSigner for FixedSigner {
    async fn sign(&self) -> Result<SignedEdgeToken> {
        self.signed.fetch_add(1, Ordering::SeqCst);
        Ok(SignedEdgeToken {
            jwt: "edge.jwt".to_string(),
            expires_at: chrono::Utc::now().timestamp() + self.lifetime,
        })
    }
}

fn preview_provider(client: MockAgentPlatformApi) -> GcpAgentPlatformSandbox {
    provider(client)
        .with_preview_signer(Arc::new(FixedSigner::new()))
        .with_preview_ports(vec![8080, 8081, 9, 8090])
}

fn running_with_route(id: &str, expire_time: &str) -> SandboxEnvironment {
    let mut sandbox = sandbox_in_state(id, "STATE_RUNNING");
    sandbox.expire_time = Some(expire_time.to_string());
    sandbox.connection_info = Some(alien_gcp_clients::gcp::agent_platform::ConnectionInfo {
        load_balancer_hostname: Some("edge.example.test".to_string()),
        routing_token: Some("route-1".to_string()),
    });
    sandbox
}

fn far_future() -> String {
    (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339()
}

#[tokio::test]
async fn preview_without_a_signer_is_refused_as_unsupported() {
    let error = provider(MockAgentPlatformApi::new())
        .preview("s1", 8080)
        .await
        .expect_err("no signing identity is configured");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
}

/// A manager-resolved (remote) binding carries neither the declared ports nor the signing
/// identity: its lease cannot sign the edge credential. The refusal must say that, instead of
/// blaming a port the caller rightly named.
#[tokio::test]
async fn preview_on_a_binding_without_ports_or_signer_names_the_binding_not_the_port() {
    let error = provider(MockAgentPlatformApi::new())
        .preview("s1", 8080)
        .await
        .expect_err("a binding with no preview setup refuses");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
    let rendered = error.to_string();
    assert!(
        rendered.contains("remote") && !rendered.contains("not one of"),
        "{rendered}"
    );
}

/// The grant is requested over `:execute`, bounded by the sandbox's own deadline, and the answer
/// is returned as the one header contract: the edge credentials gate the agent port, the
/// capability and port ride in `X-Alien-*`, and the caller's bearer has its own header because
/// the edge consumes `Authorization`.
#[tokio::test]
async fn preview_grants_over_execute_and_returns_the_header_contract() {
    let expire = far_future();
    let deadline = chrono::DateTime::parse_from_rfc3339(&expire)
        .unwrap()
        .timestamp();
    let mut client = MockAgentPlatformApi::new();
    let sandbox_expire = expire.clone();
    client
        .expect_get_sandbox()
        .returning(move |_, id| Ok(running_with_route(id, &sandbox_expire)));
    client
        .expect_execute()
        .withf(move |_, sandbox, input| {
            let body: serde_json::Value = serde_json::from_slice(input).unwrap();
            sandbox == "s1"
                && body["op"] == "previewGrant"
                && body["sandbox"] == "s1"
                && body["port"] == 8081
                && body["declaredPorts"] == json!([8080, 8081, 9, 8090])
                && body["deadline"] == deadline
        })
        .times(1)
        .returning(|_, _, _| {
            Ok(serde_json::to_vec(&json!({
                "capability": "cap-1",
                "expiresAt": chrono::Utc::now().timestamp() + 900,
            }))
            .unwrap())
        });
    let preview = preview_provider(client).preview("s1", 8081).await.unwrap();

    assert_eq!(
        preview.endpoint,
        "https://edge.example.test/v1/preview/8081"
    );
    assert_eq!(preview.headers["Authorization"], "Bearer edge.jwt");
    assert_eq!(preview.headers["X-Sandbox-Routing-Token"], "route-1");
    assert_eq!(
        preview.headers["X-Sandbox-Port"],
        alien_core::sandbox_image::AGENT_PORT.to_string()
    );
    assert_eq!(preview.headers["x-alien-preview-capability"], "cap-1");
    assert_eq!(preview.headers["x-alien-preview-port"], "8081");
    assert_eq!(preview.allowed_ports, vec![8081]);
    assert_eq!(
        preview.caller_authorization_header,
        "x-alien-caller-authorization"
    );
    assert_eq!(
        preview.subprotocols,
        vec![
            "v1.stream",
            "edge.jwt",
            "route-1",
            &alien_core::sandbox_image::AGENT_PORT.to_string()
        ]
    );
    assert!(preview.expires_in_seconds <= 900);
}

/// A second call for the same sandbox and port reuses the granted capability; another port asks
/// again. The edge credentials are fresh each time, so only the grant is cached.
#[tokio::test]
async fn preview_caches_the_grant_per_port() {
    let expire = far_future();
    let grants = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(move |_, id| Ok(running_with_route(id, &expire)));
    let counted = grants.clone();
    client.expect_execute().returning(move |_, _, _| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::to_vec(&json!({
            "capability": "cap",
            "expiresAt": chrono::Utc::now().timestamp() + 900,
        }))
        .unwrap())
    });
    let provider = preview_provider(client);
    provider.preview("s1", 8080).await.unwrap();
    provider.preview("s1", 8080).await.unwrap();
    assert_eq!(grants.load(Ordering::SeqCst), 1);
    provider.preview("s1", 8081).await.unwrap();
    assert_eq!(grants.load(Ordering::SeqCst), 2);
}

/// A grant close to expiry is asked for again rather than handed out about to die.
#[tokio::test]
async fn preview_refreshes_a_grant_close_to_expiry() {
    let expire = far_future();
    let grants = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(move |_, id| Ok(running_with_route(id, &expire)));
    let counted = grants.clone();
    client.expect_execute().returning(move |_, _, _| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::to_vec(&json!({
            "capability": "cap",
            "expiresAt": chrono::Utc::now().timestamp() + 5,
        }))
        .unwrap())
    });
    let provider = preview_provider(client);
    provider.preview("s1", 8080).await.unwrap();
    provider.preview("s1", 8080).await.unwrap();
    assert_eq!(grants.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn preview_of_a_sandbox_without_an_edge_host_is_refused() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_get_sandbox().returning(|_, id| {
        let mut sandbox = running_with_route(id, &far_future());
        sandbox
            .connection_info
            .as_mut()
            .expect("connection info")
            .load_balancer_hostname = None;
        Ok(sandbox)
    });
    let error = preview_provider(client)
        .preview("s1", 8080)
        .await
        .expect_err("no edge host");
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
}

#[tokio::test]
async fn preview_of_a_sandbox_without_a_route_is_refused() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(|_, id| Ok(sandbox_in_state(id, "STATE_RUNNING")));
    let error = preview_provider(client)
        .preview("s1", 8080)
        .await
        .expect_err("no routing token");
    assert_eq!(error.code, "SANDBOX_UNREACHABLE", "{error}");
}

#[tokio::test]
async fn an_agent_refusal_of_the_grant_is_surfaced_and_nothing_is_cached() {
    let expire = far_future();
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(move |_, id| Ok(running_with_route(id, &expire)));
    client
        .expect_execute()
        .times(2)
        .returning(|_, _, _| Err(execute_refused()));
    let provider = preview_provider(client);
    assert!(provider.preview("s1", 9).await.is_err());
    assert!(provider.preview("s1", 9).await.is_err());
}

#[tokio::test]
async fn iam_edge_signer_signs_claims_for_the_edge_audience_with_the_service_account() {
    let mut iam = alien_gcp_clients::gcp::iam::MockIamApi::new();
    iam.expect_sign_jwt()
        .withf(|account, request| {
            let claims: serde_json::Value = serde_json::from_str(&request.payload).unwrap();
            account == "sa@p.iam.gserviceaccount.com"
                && claims["iss"] == "sa@p.iam.gserviceaccount.com"
                && claims["sub"] == "sa@p.iam.gserviceaccount.com"
                && claims["aud"] == "https://aiplatform.googleapis.com/"
                && claims["exp"].as_i64().unwrap() > claims["iat"].as_i64().unwrap()
                && claims["nonce"].as_u64().is_some()
        })
        .times(1)
        .returning(|_, _| {
            Ok(alien_gcp_clients::gcp::iam::SignJwtResponse {
                signed_jwt: "signed.jwt".to_string(),
            })
        });
    let signer = IamEdgeTokenSigner::new(Arc::new(iam), "sa@p.iam.gserviceaccount.com");
    let signed = signer.sign().await.unwrap();
    assert_eq!(signed.jwt, "signed.jwt");
    // The edge credential ends when its `exp` claim says, which is what the caller is told.
    let remaining = signed.expires_at - chrono::Utc::now().timestamp();
    assert!((295..=300).contains(&remaining), "{remaining}");
}

/// A port the declaration did not name is refused before any call to the cloud or the agent.
#[tokio::test]
async fn preview_of_an_undeclared_port_is_refused_without_a_call() {
    let error = preview_provider(MockAgentPlatformApi::new())
        .preview("s1", 9999)
        .await
        .expect_err("9999 is not declared");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
    assert!(error.to_string().contains("9999"), "{error}");
}

/// The agent's clock may run ahead of this host's, so a grant that reads as longer than the
/// longest one the agent issues is still reported as that longest one.
#[tokio::test]
async fn preview_never_reports_more_than_the_longest_grant() {
    let expire = far_future();
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(move |_, id| Ok(running_with_route(id, &expire)));
    client.expect_execute().returning(|_, _, _| {
        Ok(serde_json::to_vec(&json!({
            "capability": "cap",
            "expiresAt": chrono::Utc::now().timestamp() + 940,
        }))
        .unwrap())
    });
    // A signer whose credential outlives the cap, so the cap is what ends the report.
    let preview = provider(client)
        .with_preview_signer(Arc::new(FixedSigner::living(5000)))
        .with_preview_ports(vec![8080])
        .preview("s1", 8080)
        .await
        .unwrap();
    assert_eq!(
        preview.expires_in_seconds,
        alien_core::sandbox_image::PREVIEW_CAPABILITY_MAX_SECONDS as u64
    );
}

/// A binding must mint a grant from what each resolution path really hands the workload. The
/// Terraform output and the controller's binding are different JSON, so each is parsed here the
/// way the loader parses it, not built from the struct.
async fn mint_from_binding_json(binding: serde_json::Value) -> crate::traits::PreviewCapability {
    let alien_core::bindings::SandboxBinding::GcpAgentPlatform(binding) =
        serde_json::from_value(binding).expect("the binding parses as the Agent Platform type")
    else {
        panic!("expected a GCP Agent Platform binding");
    };
    let expire = far_future();
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(move |_, id| Ok(running_with_route(id, &expire)));
    client.expect_execute().returning(|_, _, _| {
        Ok(serde_json::to_vec(&json!({
            "capability": "cap-1",
            "expiresAt": chrono::Utc::now().timestamp() + 900,
        }))
        .unwrap())
    });
    let mut iam = alien_gcp_clients::gcp::iam::MockIamApi::new();
    iam.expect_sign_jwt()
        .withf(|account, _| account == "prev@p.iam.gserviceaccount.com")
        .returning(|_, _| {
            Ok(alien_gcp_clients::gcp::iam::SignJwtResponse {
                signed_jwt: "signed.jwt".to_string(),
            })
        });
    provider(client)
        .with_preview_from_binding(
            binding.preview_ports.clone(),
            binding.preview_service_account.clone(),
            "sandbox",
            Arc::new(iam),
        )
        .expect("the binding resolves")
        .preview("s1", 8081)
        .await
        .expect("a binding built from setup outputs mints a grant")
}

#[tokio::test]
async fn a_terraform_emitted_binding_mints_a_grant() {
    // The shape setup's emitter writes: plain strings and a numeric list, plus a service tag.
    let preview = mint_from_binding_json(json!({
        "service": "sandbox-gcp-agent-platform",
        "engine": "projects/p/locations/us-central1/reasoningEngines/e",
        "template": "projects/p/locations/us-central1/reasoningEngines/e/sandboxEnvironmentTemplates/t",
        "region": "us-central1",
        "allowEgress": false,
        "previewPorts": [8080, 8081],
        "previewServiceAccount": "prev@p.iam.gserviceaccount.com",
    }))
    .await;
    assert_eq!(preview.headers["Authorization"], "Bearer signed.jwt");
}

#[tokio::test]
async fn a_controller_binding_mints_a_grant() {
    use alien_core::bindings::{BindingValue, SandboxBinding};
    let mut binding = SandboxBinding::gcp_agent_platform(
        BindingValue::value("projects/p/locations/us-central1/reasoningEngines/e".to_string()),
        BindingValue::value(
            "projects/p/locations/us-central1/reasoningEngines/e/sandboxEnvironmentTemplates/t"
                .to_string(),
        ),
        BindingValue::value("us-central1".to_string()),
        None,
        false,
    );
    if let SandboxBinding::GcpAgentPlatform(value) = &mut binding {
        value.preview_ports = vec![8080, 8081];
        value.preview_service_account = Some(BindingValue::value(
            "prev@p.iam.gserviceaccount.com".to_string(),
        ));
    }
    let preview = mint_from_binding_json(serde_json::to_value(binding).unwrap()).await;
    assert_eq!(preview.headers["Authorization"], "Bearer signed.jwt");
    assert_eq!(preview.allowed_ports, vec![8081]);
}

// ---- owner access ------------------------------------------------------------------------------

/// Counts every sandbox grant the mock agent is asked for and answers with a capability of
/// `grant_seconds`.
fn grant_agent(grant_seconds: i64, grants: Arc<AtomicUsize>) -> MockAgentPlatformApi {
    let expire = far_future();
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(move |_, id| Ok(running_with_route(id, &expire)));
    client.expect_execute().returning(move |_, _, _| {
        grants.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::to_vec(&json!({
            "capability": "cap",
            "expiresAt": chrono::Utc::now().timestamp() + grant_seconds,
        }))
        .unwrap())
    });
    client
}

/// The reported time is the sooner of the grant and the edge credential, which lives 300 seconds
/// whatever the grant says. A caller that trusted the grant's 900 would be refused by the edge
/// five minutes in.
#[tokio::test]
async fn preview_reports_the_sooner_of_the_grant_and_the_edge_credential() {
    let preview = preview_provider(grant_agent(850, Default::default()))
        .preview("s1", 8080)
        .await
        .unwrap();
    assert!(
        (295..=300).contains(&preview.expires_in_seconds),
        "the 300 second edge credential ends first: {}",
        preview.expires_in_seconds
    );

    let short = provider(grant_agent(120, Default::default()))
        .with_preview_signer(Arc::new(FixedSigner::new()))
        .with_preview_ports(vec![8080]);
    let preview = short.preview("s1", 8080).await.unwrap();
    assert!(
        (115..=120).contains(&preview.expires_in_seconds),
        "the grant ends first: {}",
        preview.expires_in_seconds
    );
}

/// Preview is gated by the same `:execute` that runs commands. A caller that may not execute is
/// refused, names the rule, and is never signed an edge credential or handed a header.
#[tokio::test]
async fn a_caller_without_execute_is_refused_and_is_never_signed_for() {
    let expire = far_future();
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_get_sandbox()
        .returning(move |_, id| Ok(running_with_route(id, &expire)));
    client
        .expect_execute()
        .returning(|_, _, _| Err(execute_refused()));
    let signer = FixedSigner::new();
    let signed = signer.signed.clone();
    let provider = provider(client)
        .with_preview_signer(Arc::new(signer))
        .with_preview_ports(vec![8080]);
    let error = provider
        .preview("s1", 8080)
        .await
        .expect_err("a caller refused by :execute gets no preview");
    assert_eq!(signed.load(Ordering::SeqCst), 0, "nothing was signed");
    assert!(
        error
            .to_string()
            .contains(alien_core::GCP_PREVIEW_IS_COMMAND_ACCESS),
        "{error}"
    );
    assert!(provider.grants.lock().unwrap().is_empty());
}

/// An expired grant is never read again, so it leaves the cache whenever a grant is added, and a
/// terminated sandbox takes every grant it had.
#[tokio::test]
async fn grants_are_pruned_when_expired_and_dropped_when_the_sandbox_is_terminated() {
    let provider = preview_provider(grant_agent(900, Default::default()));
    provider.preview("s1", 8080).await.unwrap();
    provider.preview("s2", 8080).await.unwrap();
    assert_eq!(provider.grants.lock().unwrap().len(), 2);

    // An entry past its end goes the next time the cache is touched.
    provider.grants.lock().unwrap().insert(
        ("old".to_string(), 8080),
        CachedGrant {
            capability: "dead".to_string(),
            expires_at: chrono::Utc::now().timestamp() - 1,
        },
    );
    provider.preview("s1", 8081).await.unwrap();
    {
        let grants = provider.grants.lock().unwrap();
        assert!(
            !grants.contains_key(&("old".to_string(), 8080)),
            "{grants:?}"
        );
        assert_eq!(grants.len(), 3);
    }

    provider.forget_grants("s1");
    let grants = provider.grants.lock().unwrap();
    assert_eq!(grants.len(), 1, "{grants:?}");
    assert!(grants.contains_key(&("s2".to_string(), 8080)));
}

/// A terminate that is confirmed drops the sandbox's grants; one that was refused keeps them,
/// because the sandbox may still be there to preview.
#[tokio::test]
async fn terminate_drops_the_grants_of_a_sandbox_that_is_gone() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_delete_sandbox().returning(|_, _| Ok(()));
    client
        .expect_get_sandbox()
        .returning(|_, _| Err(not_found()));
    let provider = preview_provider(client);
    for sandbox in ["s1", "s2"] {
        provider.grants.lock().unwrap().insert(
            (sandbox.to_string(), 8080),
            CachedGrant {
                capability: "cap".to_string(),
                expires_at: chrono::Utc::now().timestamp() + 600,
            },
        );
    }
    provider.terminate("s1").await.unwrap();
    let grants = provider.grants.lock().unwrap();
    assert_eq!(grants.len(), 1, "{grants:?}");
    assert!(grants.contains_key(&("s2".to_string(), 8080)));
}

// ---- get_or_create and get by a caller's own key ----------------------------------------------

/// An in-memory engine that behaves the way Google does for these calls: it mints numeric ids,
/// keeps the display name it was given, orders sandboxes by creation time, answers 404 for an id
/// that is gone, and answers 400 for an id that is not numeric.
#[derive(Debug, Default)]
struct FakeEngine {
    state: std::sync::Mutex<FakeEngineState>,
    /// The first this-many `list` calls answer as if nothing existed yet: two callers that both
    /// looked before either created.
    blind_lists: AtomicUsize,
    /// The next this-many `list` calls fail.
    failing_lists: AtomicUsize,
    /// The first this-many `list` calls answer before any failure starts.
    lists_before_failures: AtomicUsize,
    /// The next this-many probes of one sandbox get no answer: its agent is still booting.
    silent_probes: std::sync::Mutex<std::collections::HashMap<String, usize>>,
    /// How far Google's clock is behind this host's: sandboxes are stamped that much earlier than
    /// the host's `now`, as when the host clock runs ahead.
    google_behind: std::sync::Mutex<chrono::Duration>,
    /// Sandboxes the list still shows as running after they stopped: a stop between a list and a read.
    stale_listed: std::sync::Mutex<std::collections::HashSet<String>>,
}

#[derive(Debug, Default)]
struct FakeEngineState {
    next: u64,
    sandboxes: Vec<FakeSandbox>,
    created_with: Vec<Option<String>>,
    /// Listing lag of the next creates, in order: a sandbox is listed only once this long after its
    /// create call (found live: a create shows in the list two to nine seconds after its request).
    create_lags: std::collections::VecDeque<Duration>,
}

#[derive(Debug)]
struct FakeSandbox {
    id: String,
    display: Option<String>,
    state: String,
    created: chrono::DateTime<chrono::Utc>,
    listed_from: tokio::time::Instant,
}

impl FakeEngine {
    fn names(&self) -> Vec<Option<String>> {
        self.state.lock().unwrap().created_with.clone()
    }
    fn live_ids(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .sandboxes
            .iter()
            .filter(|sandbox| sandbox.state == "STATE_RUNNING")
            .map(|sandbox| sandbox.id.clone())
            .collect()
    }
    fn set_state(&self, id: &str, new_state: &str) {
        for sandbox in self.state.lock().unwrap().sandboxes.iter_mut() {
            if sandbox.id == id {
                sandbox.state = new_state.to_string();
            }
        }
    }
    fn hide_state_from_list(&self, id: &str) {
        self.stale_listed.lock().unwrap().insert(id.to_string());
    }
    fn lag_creates(&self, lags: &[u64]) {
        self.state
            .lock()
            .unwrap()
            .create_lags
            .extend(lags.iter().map(|ms| Duration::from_millis(*ms)));
    }
    fn record(sandbox: &FakeSandbox) -> SandboxEnvironment {
        let mut record = sandbox_in_state(&sandbox.id, &sandbox.state);
        record.display_name = sandbox.display.clone();
        record.create_time = Some(
            sandbox
                .created
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        );
        record
    }
}

#[async_trait]
impl AgentPlatformApi for FakeEngine {
    async fn create_sandbox(
        &self,
        _engine: &str,
        request: SandboxCreateRequest,
    ) -> ClientResult<Operation> {
        let mut state = self.state.lock().unwrap();
        state.next += 1;
        let id = (7_000_000_000_000_000_000u64 + state.next).to_string();
        state.created_with.push(request.display_name.clone());
        let lag = state.create_lags.pop_front().unwrap_or_default();
        state.sandboxes.push(FakeSandbox {
            id: id.clone(),
            display: request.display_name,
            state: "STATE_RUNNING".to_string(),
            created: super::wall_now() - *self.google_behind.lock().unwrap(),
            listed_from: tokio::time::Instant::now() + lag,
        });
        Ok(done_op(serde_json::json!({ "name": sandbox_name(&id) })))
    }
    async fn get_sandbox(&self, _engine: &str, sandbox: &str) -> ClientResult<SandboxEnvironment> {
        assert!(
            sandbox.chars().all(|c| c.is_ascii_digit()),
            "Google answers 400 for a non-numeric id, and a caller's key must never reach it: {sandbox}"
        );
        let state = self.state.lock().unwrap();
        state
            .sandboxes
            .iter()
            .find(|candidate| candidate.id == sandbox)
            .map(Self::record)
            .ok_or_else(not_found)
    }
    async fn list_sandboxes(&self, _engine: &str) -> ClientResult<Vec<SandboxEnvironment>> {
        if self
            .blind_lists
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Ok(Vec::new());
        }
        let spared = self
            .lists_before_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if !spared
            && self
                .failing_lists
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
        {
            return Err(execute_refused());
        }
        let now = tokio::time::Instant::now();
        let state = self.state.lock().unwrap();
        // Newest first, so a caller that trusts the listing order picks the wrong one.
        Ok(state
            .sandboxes
            .iter()
            .rev()
            .filter(|sandbox| sandbox.listed_from <= now)
            .map(|sandbox| {
                let mut record = Self::record(sandbox);
                if self.stale_listed.lock().unwrap().contains(&sandbox.id) {
                    record.state = Some("STATE_RUNNING".to_string());
                }
                record
            })
            .collect())
    }
    async fn list_sandboxes_by_display_names(
        &self,
        engine: &str,
        display_names: &[String],
    ) -> ClientResult<Vec<SandboxEnvironment>> {
        Ok(self
            .list_sandboxes(engine)
            .await?
            .into_iter()
            .filter(|sandbox| {
                sandbox
                    .display_name
                    .as_deref()
                    .is_some_and(|name| display_names.iter().any(|n| n == name))
            })
            .collect())
    }
    async fn delete_sandbox(&self, _engine: &str, sandbox: &str) -> ClientResult<()> {
        self.state
            .lock()
            .unwrap()
            .sandboxes
            .retain(|candidate| candidate.id != sandbox);
        Ok(())
    }
    async fn execute(&self, _engine: &str, sandbox: &str, input: &[u8]) -> ClientResult<Vec<u8>> {
        if let Some(left) = self.silent_probes.lock().unwrap().get_mut(sandbox) {
            if *left > 0 {
                *left -= 1;
                return Err(execute_refused());
            }
        }
        // A command (the marker script at create) succeeds; anything else is the health probe.
        if op_of(input) != "health" {
            return Ok(marker_script_reply());
        }
        Ok(health_reply())
    }
    async fn get_engine_iam_policy(
        &self,
        _engine: &str,
    ) -> ClientResult<alien_gcp_clients::iam::IamPolicy> {
        unimplemented!()
    }
    async fn set_engine_iam_policy(
        &self,
        _engine: &str,
        _policy: alien_gcp_clients::iam::IamPolicy,
    ) -> ClientResult<alien_gcp_clients::iam::IamPolicy> {
        unimplemented!()
    }
    async fn create_engine(
        &self,
        _display_name: &str,
        _description: Option<String>,
    ) -> ClientResult<Operation> {
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
    async fn pause_sandbox(&self, _engine: &str, _sandbox: &str) -> ClientResult<Operation> {
        unimplemented!()
    }
    async fn resume_sandbox(&self, _engine: &str, _sandbox: &str) -> ClientResult<Operation> {
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

fn keyed(key: &str) -> CreateSandboxRequest {
    CreateSandboxRequest {
        sandbox_id: Some(key.to_string()),
        ..Default::default()
    }
}

#[tokio::test(start_paused = true)]
async fn a_second_get_or_create_with_the_same_key_finds_the_first() {
    let engine = Arc::new(FakeEngine::default());
    let first = provider_from(engine.clone())
        .get_or_create(keyed("engine-1111"))
        .await
        .unwrap();
    let second = provider_from(engine.clone())
        .get_or_create(keyed("engine-1111"))
        .await
        .unwrap();
    assert!(first.created);
    assert!(!second.created, "the key already names a sandbox");
    assert_eq!(second.sandbox.sandbox_id, first.sandbox.sandbox_id);
    assert_eq!(engine.live_ids().len(), 1, "one sandbox, not two");
}

#[tokio::test(start_paused = true)]
async fn different_keys_get_different_sandboxes() {
    let engine = Arc::new(FakeEngine::default());
    let a = provider_from(engine.clone())
        .get_or_create(keyed("a"))
        .await
        .unwrap();
    let b = provider_from(engine.clone())
        .get_or_create(keyed("b"))
        .await
        .unwrap();
    assert!(b.created);
    assert_ne!(a.sandbox.sandbox_id, b.sandbox.sandbox_id);
}

/// The key goes to Google only as its hash, never raw: it is caller-supplied, and the display name
/// is what a later list matches on.
#[tokio::test(start_paused = true)]
async fn the_raw_key_never_reaches_google() {
    let engine = Arc::new(FakeEngine::default());
    let key = "engine-5f3a";
    provider_from(engine.clone())
        .get_or_create(keyed(key))
        .await
        .unwrap();
    for name in engine.names() {
        let name = name.expect("a keyed create is stamped");
        assert!(!name.contains("engine") && !name.contains('"'), "{name}");
        assert!(
            name.ends_with(&format!("{}-pm1", request_key(key))),
            "{name}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn get_by_key_finds_it() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    let created = sut.get_or_create(keyed("lookup")).await.unwrap();
    let found = sut.get("lookup").await.unwrap().expect("found by key");
    assert_eq!(found.sandbox_id, created.sandbox.sandbox_id);
    let by_id = sut
        .get(&created.sandbox.sandbox_id)
        .await
        .unwrap()
        .expect("found by id");
    assert_eq!(by_id.sandbox_id, found.sandbox_id);
}

#[tokio::test(start_paused = true)]
async fn an_unknown_key_or_numeric_id_is_none_not_an_error() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine);
    assert!(sut.get("never-created").await.unwrap().is_none());
    assert!(sut.get("123456789").await.unwrap().is_none());
}

#[tokio::test(start_paused = true)]
async fn terminated_sandboxes_are_ignored() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    let first = sut.get_or_create(keyed("gone")).await.unwrap();
    engine.set_state(&first.sandbox.sandbox_id, "STATE_DELETING");
    assert!(
        sut.get("gone").await.unwrap().is_none(),
        "a terminating sandbox is not live"
    );
    let second = sut.get_or_create(keyed("gone")).await.unwrap();
    assert!(
        second.created,
        "a terminated sandbox does not answer the key"
    );
    assert_ne!(second.sandbox.sandbox_id, first.sandbox.sandbox_id);
}

#[tokio::test(start_paused = true)]
async fn after_terminate_the_key_and_the_id_are_none() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    let created = sut.get_or_create(keyed("done")).await.unwrap();
    sut.terminate(&created.sandbox.sandbox_id).await.unwrap();
    assert!(sut.get("done").await.unwrap().is_none());
    assert!(sut
        .get(&created.sandbox.sandbox_id)
        .await
        .unwrap()
        .is_none());
}

/// A numeric id naming an existing sandbox keeps working as it did.
#[tokio::test(start_paused = true)]
async fn a_numeric_id_that_names_a_sandbox_is_found_as_before() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    let created = sut.create(CreateSandboxRequest::default()).await.unwrap();
    let resolved = sut.get_or_create(keyed(&created.sandbox_id)).await.unwrap();
    assert!(!resolved.created);
    assert_eq!(resolved.sandbox.sandbox_id, created.sandbox_id);
}

/// Found live: a second caller looks while the first one's sandbox is listed and running but its
/// agent has not started answering. That sandbox is waited for, not passed over for a second one,
/// which is the duplicate the pair would then have to converge away from.
#[tokio::test(start_paused = true)]
async fn a_sandbox_whose_agent_is_still_booting_is_waited_for_not_replaced() {
    let engine = Arc::new(FakeEngine::default());
    let first = provider_from(engine.clone())
        .get_or_create(keyed("booting-key"))
        .await
        .unwrap();
    engine
        .silent_probes
        .lock()
        .unwrap()
        .insert(first.sandbox.sandbox_id.clone(), 3);
    let second = provider_from(engine.clone())
        .get_or_create(keyed("booting-key"))
        .await
        .unwrap();
    assert!(
        !second.created,
        "the first caller's sandbox answers the key"
    );
    assert_eq!(second.sandbox.sandbox_id, first.sandbox.sandbox_id);
    assert_eq!(engine.live_ids().len(), 1);
}

/// Runs one `get_or_create` per entry of `starts` (milliseconds after the first), each through its
/// own provider, all at once.
async fn race(engine: &Arc<FakeEngine>, key: &str, starts: &[u64]) -> Vec<Result<ResolvedSandbox>> {
    let callers = starts.iter().map(|start| {
        let engine = engine.clone();
        let (key, start) = (key.to_string(), *start);
        async move {
            tokio::time::sleep(Duration::from_millis(start)).await;
            provider_from(engine).get_or_create(keyed(&key)).await
        }
    });
    futures::future::join_all(callers).await
}

/// What every racing caller must end with: one live sandbox, one `created`, and every caller holding
/// that sandbox.
fn assert_converged(engine: &FakeEngine, results: &[Result<ResolvedSandbox>]) {
    let live = engine.live_ids();
    assert_eq!(
        live.len(),
        1,
        "exactly one live sandbox for the key: {live:?}"
    );
    let created = results
        .iter()
        .filter(|r| r.as_ref().unwrap().created)
        .count();
    assert_eq!(created, 1, "exactly one caller created it");
    for result in results {
        assert_eq!(
            result.as_ref().unwrap().sandbox.sandbox_id,
            live[0],
            "every caller holds the sandbox that is still live"
        );
    }
}

/// Two callers both looked before either created, so each made a sandbox. The newer one is
/// terminated and both callers end with the oldest.
#[tokio::test(start_paused = true)]
async fn a_race_of_two_converges_on_the_oldest_sandbox() {
    let engine = Arc::new(FakeEngine::default());
    engine.lag_creates(&[40, 40]);
    let results = race(&engine, "race", &[0, 5]).await;
    assert_eq!(
        engine.names().len(),
        2,
        "both callers created: this is a real duplicate race"
    );
    assert_converged(&engine, &results);
    assert!(
        results[0].as_ref().unwrap().created,
        "the older one kept its own"
    );
}

/// Two sandboxes for one key, one made before the tag existed and one tagged: the key resolves to
/// the oldest whichever kind it is, and the newer one yields.
#[tokio::test(start_paused = true)]
async fn the_oldest_wins_when_one_duplicate_is_tagged_and_one_is_not() {
    for plain_is_older in [true, false] {
        let engine = Arc::new(FakeEngine::default());
        let tagged = provider_from(engine.clone())
            .get_or_create(keyed("mixed"))
            .await
            .unwrap()
            .sandbox
            .sandbox_id;
        let tagged_name = engine.names()[0].clone().expect("a display name");
        let plain_name = tagged_name
            .strip_suffix("-pm1")
            .expect("tagged at create")
            .to_string();
        let long_ago = super::wall_now() - chrono::Duration::hours(1);
        let plain_id = {
            let mut state = engine.state.lock().unwrap();
            let plain_id = "7000000000000000999".to_string();
            if !plain_is_older {
                state.sandboxes[0].created = long_ago;
            }
            state.sandboxes.push(FakeSandbox {
                id: plain_id.clone(),
                display: Some(plain_name),
                state: "STATE_RUNNING".to_string(),
                created: if plain_is_older {
                    long_ago
                } else {
                    super::wall_now()
                },
                listed_from: tokio::time::Instant::now(),
            });
            plain_id
        };
        tokio::time::sleep(Duration::from_secs(120)).await;
        let found = provider_from(engine.clone())
            .get_or_create(keyed("mixed"))
            .await
            .unwrap();
        let oldest = if plain_is_older { plain_id } else { tagged };
        assert_eq!(found.sandbox.sandbox_id, oldest, "plain_is_older={plain_is_older}");
        assert!(!found.created);
    }
}

/// The review's three-caller case. A is the oldest and is listed late; B and C are listed fast. C's
/// first look at the key sees B, which is older than C and is itself about to yield to A. A caller
/// that yields to B at once holds a sandbox that is then deleted.
#[tokio::test(start_paused = true)]
async fn three_racers_with_an_older_sandbox_listed_late_all_end_with_one_live_sandbox() {
    let engine = Arc::new(FakeEngine::default());
    engine.lag_creates(&[120, 5, 5]);
    let results = race(&engine, "three", &[0, 10, 20]).await;
    assert_converged(&engine, &results);
    assert!(
        results[0].as_ref().unwrap().created,
        "A, the oldest, kept its own"
    );
}

#[tokio::test(start_paused = true)]
async fn five_racers_with_uneven_listing_lag_converge() {
    let engine = Arc::new(FakeEngine::default());
    engine.lag_creates(&[90, 10, 70, 5, 30]);
    let results = race(&engine, "five", &[0, 3, 6, 9, 12]).await;
    assert_converged(&engine, &results);
}

/// A caller that arrives while the oldest sandbox for the key is still inside its contention window
/// does not hand it out at once: an older one may still appear and make it yield.
#[tokio::test(start_paused = true)]
async fn a_caller_waits_out_the_window_of_a_fresh_sandbox_before_returning_it() {
    let engine = Arc::new(FakeEngine::default());
    engine.lag_creates(&[100, 0]);
    let first = provider_from(engine.clone());
    let older = tokio::spawn(async move { first.get_or_create(keyed("fresh")).await });
    tokio::time::sleep(Duration::from_millis(5)).await;
    // The first caller's own sandbox is not listed yet (lag 100 ms) so this caller creates one
    // too; the older one then shows and wins for both.
    let newer = provider_from(engine.clone())
        .get_or_create(keyed("fresh"))
        .await
        .unwrap();
    let older = older.await.unwrap().unwrap();
    assert_eq!(older.sandbox.sandbox_id, newer.sandbox.sandbox_id);
    assert_eq!(engine.live_ids(), vec![older.sandbox.sandbox_id]);
}

#[tokio::test(start_paused = true)]
async fn a_race_converges_while_the_older_agent_is_still_booting() {
    let engine = Arc::new(FakeEngine::default());
    // Both sandboxes are listed late, so neither caller sees the other on its first look and each
    // creates: a real duplicate. The older one's agent is still booting when the newer caller
    // re-checks and has to wait for it.
    engine.lag_creates(&[40, 40]);
    let older_id = (7_000_000_000_000_000_000u64 + 1).to_string();
    engine
        .silent_probes
        .lock()
        .unwrap()
        .insert(older_id.clone(), 3);
    let results = race(&engine, "race-boot", &[0, 5]).await;
    assert_eq!(engine.names().len(), 2, "both callers created");
    assert_converged(&engine, &results);
    assert_eq!(engine.live_ids(), vec![older_id]);
}

/// The re-check after a create is retried; a failure that clears does not change the answer.
#[tokio::test(start_paused = true)]
async fn a_failed_recheck_is_retried() {
    let engine = Arc::new(FakeEngine::default());
    engine.lists_before_failures.store(1, Ordering::SeqCst);
    engine.failing_lists.store(2, Ordering::SeqCst);
    let resolved = provider_from(engine.clone())
        .get_or_create(keyed("retry"))
        .await
        .unwrap();
    assert!(resolved.created);
    assert_eq!(engine.live_ids(), vec![resolved.sandbox.sandbox_id]);
}

/// When the re-check cannot be answered, nothing says an older sandbox is absent: the caller gets
/// an error, never `created = true` for a sandbox that may be a duplicate. The sandbox is kept,
/// because another caller may already have yielded to it, and a retry finds it by the key.
#[tokio::test(start_paused = true)]
async fn an_unanswered_recheck_is_an_error_not_a_created_sandbox() {
    let engine = Arc::new(FakeEngine::default());
    engine.lists_before_failures.store(1, Ordering::SeqCst);
    engine.failing_lists.store(1_000, Ordering::SeqCst);
    let sut = provider_from(engine.clone());
    let error = sut.get_or_create(keyed("no-recheck")).await.unwrap_err();
    assert_eq!(error.code, "SANDBOX_UNREACHABLE");
    assert!(error.retryable, "the caller may retry");
    let kept = engine.live_ids();
    assert_eq!(kept.len(), 1, "the sandbox is not deleted: {kept:?}");

    engine.failing_lists.store(0, Ordering::SeqCst);
    let retry = sut.get_or_create(keyed("no-recheck")).await.unwrap();
    assert!(
        !retry.created,
        "the retry finds the kept sandbox by the key"
    );
    assert_eq!(retry.sandbox.sandbox_id, kept[0]);
}

/// A sandbox whose agent stopped answering still holds the key. The key resolves to its id, so a
/// caller that has only the key can terminate it; get_or_create names it and makes no second one.
#[tokio::test(start_paused = true)]
async fn a_wedged_agent_does_not_lock_the_key() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    let created = sut.get_or_create(keyed("wedged")).await.unwrap();
    let id = created.sandbox.sandbox_id.clone();
    engine
        .silent_probes
        .lock()
        .unwrap()
        .insert(id.clone(), usize::MAX / 2);

    let found = sut
        .get("wedged")
        .await
        .unwrap()
        .expect("the key still resolves");
    assert_eq!(
        found.sandbox_id, id,
        "get(key) names the sandbox even though its agent is silent"
    );

    let error = sut.get_or_create(keyed("wedged")).await.unwrap_err();
    assert!(
        error.to_string().contains(&id) || format!("{error:?}").contains(&id),
        "the error names the sandbox to terminate: {error:?}"
    );
    assert_eq!(
        engine.live_ids(),
        vec![id.clone()],
        "no second sandbox was made"
    );

    sut.terminate(&id).await.unwrap();
    assert!(sut.get("wedged").await.unwrap().is_none());
    assert!(sut.get_or_create(keyed("wedged")).await.unwrap().created);
}

/// A host clock a few seconds ahead of Google's makes a sandbox look older than it is. The caller
/// that arrives mid-race would then see the window over early and hand the sandbox out while an
/// older one is still unlisted. The allowance keeps its wait past the window.
#[tokio::test(start_paused = true)]
async fn a_host_clock_ahead_of_googles_does_not_shorten_the_wait() {
    let engine = Arc::new(FakeEngine::default());
    *engine.google_behind.lock().unwrap() = chrono::Duration::seconds(4);
    let first = provider_from(engine.clone())
        .create(keyed("skewed"))
        .await
        .unwrap();
    let started = tokio::time::Instant::now();
    let found = provider_from(engine.clone())
        .get_or_create(keyed("skewed"))
        .await
        .unwrap();
    assert_eq!(found.sandbox.sandbox_id, first.sandbox_id);
    assert!(
        started.elapsed() >= CONTENTION_WINDOW,
        "waited {:?}, less than the window itself",
        started.elapsed()
    );
}

/// The creating caller's decision is counted from the moment its create returned, not from the
/// request: a slow create must not shorten the time older sandboxes have to be listed.
#[tokio::test(start_paused = true)]
async fn the_window_of_a_creator_starts_when_its_create_returns() {
    let engine = Arc::new(FakeEngine::default());
    let started = tokio::time::Instant::now();
    provider_from(engine.clone())
        .get_or_create(keyed("anchor"))
        .await
        .unwrap();
    // The first look takes no time, so the decision comes one window after the create.
    assert!(started.elapsed() >= CONTENTION_WINDOW);
}

/// A paused sandbox is live and owns its key in both calls: `get` and `get_or_create` agree, and
/// no second sandbox is made beside it.
#[tokio::test(start_paused = true)]
async fn a_paused_sandbox_owns_its_key_in_get_and_get_or_create() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    let first = sut.get_or_create(keyed("paused")).await.unwrap();
    engine.set_state(&first.sandbox.sandbox_id, "STATE_PAUSED");

    let got = sut.get("paused").await.unwrap().expect("found by key");
    assert_eq!(got.sandbox_id, first.sandbox.sandbox_id);
    assert_eq!(got.state, SandboxState::Paused);

    let resolved = sut.get_or_create(keyed("paused")).await.unwrap();
    assert!(!resolved.created);
    assert_eq!(resolved.sandbox.sandbox_id, first.sandbox.sandbox_id);
    assert_eq!(resolved.sandbox.state, SandboxState::Paused);
    assert_eq!(engine.state.lock().unwrap().sandboxes.len(), 1);
}

/// A failed re-check names the sandbox in a structured field, not only in the text.
#[tokio::test(start_paused = true)]
async fn a_failed_recheck_names_the_sandbox_in_the_error_context() {
    let engine = Arc::new(FakeEngine::default());
    engine.lists_before_failures.store(1, Ordering::SeqCst);
    engine.failing_lists.store(1_000, Ordering::SeqCst);
    let error = provider_from(engine.clone())
        .get_or_create(keyed("ctx"))
        .await
        .unwrap_err();
    let kept = engine.live_ids();
    assert_eq!(
        error.context.as_ref().and_then(|c| c.get("sandboxId")),
        Some(&serde_json::Value::String(kept[0].clone()))
    );
}

/// More than one sandbox older than the window is a duplicate that nothing deletes, and is
/// reported; sandboxes still inside the window are a race in progress and are not.
#[tokio::test(start_paused = true)]
async fn duplicates_older_than_the_window_are_reported() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    sut.create(keyed("dup")).await.unwrap();
    sut.create(keyed("dup")).await.unwrap();
    let both = sut
        .request_key_candidates("dup", GET_OR_CREATE)
        .await
        .unwrap();
    assert!(duplicates_past_the_window(&both).is_empty(), "both fresh");
    tokio::time::sleep(CONTENTION_WINDOW + Duration::from_secs(1)).await;
    let both = sut
        .request_key_candidates("dup", GET_OR_CREATE)
        .await
        .unwrap();
    assert_eq!(duplicates_past_the_window(&both).len(), 2);
}

/// A sandbox that ends during the wait is not handed out as a running one.
#[tokio::test(start_paused = true)]
async fn a_created_sandbox_that_expired_during_the_wait_is_not_returned() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    let mut request = keyed("short-lived");
    request.timeout_ms = Some(10_000);
    let call = tokio::spawn(async move { sut.get_or_create(request).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    engine.set_state(
        &(7_000_000_000_000_000_000u64 + 1).to_string(),
        "STATE_TERMINATED",
    );
    let result = call.await.unwrap();
    assert!(
        result.is_err(),
        "an expired sandbox is not reported as created and running: {result:?}"
    );
}

/// A lifetime shorter than the contention window is not waited out: the sandbox would end first.
#[tokio::test(start_paused = true)]
async fn the_wait_stays_inside_a_short_lifetime() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    let mut request = keyed("short-wait");
    request.timeout_ms = Some(10_000);
    let started = tokio::time::Instant::now();
    sut.get_or_create(request).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "waited {:?}",
        started.elapsed()
    );
}

/// `get(key)` skips a sandbox that stops between the list and the read.
#[tokio::test(start_paused = true)]
async fn get_by_key_skips_a_sandbox_that_stopped_after_the_list() {
    let engine = Arc::new(FakeEngine::default());
    let sut = provider_from(engine.clone());
    let older = sut.get_or_create(keyed("stops")).await.unwrap();
    tokio::time::sleep(Duration::from_secs(30)).await;
    let newer_id = {
        // a second live sandbox for the same key (a missed duplicate)
        let created = provider_from(engine.clone())
            .create(keyed("stops"))
            .await
            .unwrap();
        created.sandbox_id
    };
    engine.hide_state_from_list(&older.sandbox.sandbox_id);
    engine.set_state(&older.sandbox.sandbox_id, "STATE_TERMINATED");
    tokio::time::sleep(Duration::from_secs(30)).await;
    let found = sut
        .get("stops")
        .await
        .unwrap()
        .expect("the live one answers");
    assert_eq!(found.sandbox_id, newer_id);
}

/// Refreshing the marker replaces it in one step: a resume that reads it during the refresh sees
/// the old or the new generation, never "none" (which would read as a replaced container).
#[tokio::test]
async fn refreshing_the_pause_marker_never_leaves_it_absent() {
    let scripts = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let mut client = MockAgentPlatformApi::new();
    {
        let scripts = scripts.clone();
        client.expect_execute().returning(move |_, _, input| {
            let value: serde_json::Value = serde_json::from_slice(input).unwrap();
            scripts
                .lock()
                .unwrap()
                .push(value["command"][2].as_str().unwrap().to_string());
            Ok(ndjson(&[exit_frame(0)]))
        });
    }
    let provider = provider(client);
    provider
        .write_pause_marker(PAUSE, "s1", 7)
        .await
        .expect("the marker is written");
    let script = scripts.lock().unwrap().pop().expect("a script ran");

    let shm = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let script = script
        .replace("/dev/shm", "@SHM@")
        .replace("/tmp", "@TMP@")
        .replace("@SHM@", shm.path().to_str().unwrap())
        .replace("@TMP@", tmp.path().to_str().unwrap());
    let marker = shm.path().join(PAUSE_MARKER_NAME);
    let run = |script: &str| {
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", script])
            .status()
            .unwrap();
        assert!(status.success());
    };
    run(&script);
    assert!(marker.exists());

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let absent = Arc::new(AtomicUsize::new(0));
    let reader = {
        let (stop, absent, marker) = (stop.clone(), absent.clone(), marker.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match std::fs::read_to_string(&marker) {
                    Ok(text) if text.is_empty() => {
                        absent.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(_) => {}
                    Err(_) => {
                        absent.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        })
    };
    for _ in 0..150 {
        run(&script);
    }
    stop.store(true, Ordering::SeqCst);
    reader.join().unwrap();
    assert_eq!(absent.load(Ordering::SeqCst), 0, "the marker was absent or empty");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "7");
}

/// Another pause finished between this one's state read and its health probe: the probe woke the
/// sandbox (503) and the call must not fail; the sandbox is paused again.
#[tokio::test]
async fn a_pause_whose_probe_wakes_the_sandbox_pauses_it_again() {
    let fake = PausableSandbox::running("boot-a");
    let probes = Arc::new(AtomicUsize::new(0));
    let pauses = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    {
        let fake = fake.clone();
        client
            .expect_get_sandbox()
            .returning(move |_, id| Ok(sandbox_in_state(id, *fake.state.lock().unwrap())));
    }
    {
        let fake = fake.clone();
        let probes = probes.clone();
        client.expect_execute().returning(move |_, _, input| {
            if op_of(input) == "health" && probes.fetch_add(1, Ordering::SeqCst) == 0 {
                // The other pause landed; this request wakes the sandbox.
                fake.set_state("STATE_RUNNING");
                return Err(http_error(503, "paused; resume has been initiated"));
            }
            fake.agent_reply(input)
        });
    }
    {
        let fake = fake.clone();
        let pauses = pauses.clone();
        client.expect_pause_sandbox().returning(move |_, _| {
            pauses.fetch_add(1, Ordering::SeqCst);
            fake.set_state("STATE_PAUSED");
            Ok(done_op(serde_json::json!({})))
        });
    }
    provider(client).pause("s1").await.expect("paused again");
    assert_eq!(*fake.state.lock().unwrap(), "STATE_PAUSED");
    assert_eq!(pauses.load(Ordering::SeqCst), 1);
}

/// The probe succeeds, then another pause lands: the marker write wakes the sandbox and gets the
/// 503. The call must not leave with an error and the sandbox awake; it pauses again.
#[tokio::test]
async fn a_pause_whose_marker_write_wakes_the_sandbox_pauses_it_again() {
    let fake = PausableSandbox::running("boot-a");
    let failed_once = Arc::new(AtomicUsize::new(0));
    let pauses = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    {
        let fake = fake.clone();
        client
            .expect_get_sandbox()
            .returning(move |_, id| Ok(sandbox_in_state(id, *fake.state.lock().unwrap())));
    }
    {
        let fake = fake.clone();
        let failed_once = failed_once.clone();
        client.expect_execute().returning(move |_, _, input| {
            if op_of(input) != "health" && failed_once.fetch_add(1, Ordering::SeqCst) == 0 {
                fake.set_state("STATE_RUNNING");
                return Err(http_error(503, "paused; resume has been initiated"));
            }
            fake.agent_reply(input)
        });
    }
    {
        let fake = fake.clone();
        let pauses = pauses.clone();
        client.expect_pause_sandbox().returning(move |_, _| {
            pauses.fetch_add(1, Ordering::SeqCst);
            fake.set_state("STATE_PAUSED");
            Ok(done_op(serde_json::json!({})))
        });
    }
    provider(client).pause("s1").await.expect("paused again");
    assert_eq!(*fake.state.lock().unwrap(), "STATE_PAUSED");
    assert!(failed_once.load(Ordering::SeqCst) >= 2, "the marker write was retried");
    assert_eq!(pauses.load(Ordering::SeqCst), 1);
}

/// The other pause is already done when the probe fails: the goal is reached, so it is a no-op.
#[tokio::test]
async fn a_pause_whose_probe_fails_on_an_already_paused_sandbox_is_done() {
    let fake = PausableSandbox::running("boot-a");
    let reads = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    {
        let fake = fake.clone();
        client.expect_get_sandbox().returning(move |_, id| {
            // Running for the first read, paused from the second on.
            if reads.fetch_add(1, Ordering::SeqCst) >= 1 {
                fake.set_state("STATE_PAUSED");
            }
            Ok(sandbox_in_state(id, *fake.state.lock().unwrap()))
        });
    }
    client
        .expect_execute()
        .returning(|_, _, _| Err(http_error(503, "paused; resume has been initiated")));
    client.expect_pause_sandbox().never();
    provider(client).pause("s1").await.expect("already paused");
}

/// The probe's wake is followed by a stale `PAUSED` read, then the truth: running. The pause is
/// not reported done on the stale read.
#[tokio::test]
async fn a_stale_paused_read_after_a_waking_probe_does_not_end_the_pause() {
    let fake = PausableSandbox::running("boot-a");
    let probes = Arc::new(AtomicUsize::new(0));
    let pauses = Arc::new(AtomicUsize::new(0));
    // Reads after the first probe: PAUSED (stale), then RUNNING (the wake started).
    let stale = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
    let mut client = MockAgentPlatformApi::new();
    {
        let fake = fake.clone();
        let stale = stale.clone();
        client.expect_get_sandbox().returning(move |_, id| {
            let next = stale.lock().unwrap().pop();
            if let Some(state) = next {
                fake.set_state(state);
            }
            Ok(sandbox_in_state(id, *fake.state.lock().unwrap()))
        });
    }
    {
        let fake = fake.clone();
        let probes = probes.clone();
        let stale = stale.clone();
        client.expect_execute().returning(move |_, _, input| {
            if op_of(input) == "health" && probes.fetch_add(1, Ordering::SeqCst) == 0 {
                *stale.lock().unwrap() = vec!["STATE_RUNNING", "STATE_PAUSED"];
                return Err(http_error(503, "paused; resume has been initiated"));
            }
            fake.agent_reply(input)
        });
    }
    {
        let fake = fake.clone();
        let pauses = pauses.clone();
        client.expect_pause_sandbox().returning(move |_, _| {
            pauses.fetch_add(1, Ordering::SeqCst);
            fake.set_state("STATE_PAUSED");
            Ok(done_op(serde_json::json!({})))
        });
    }
    provider(client).pause("s1").await.expect("paused");
    assert_eq!(*fake.state.lock().unwrap(), "STATE_PAUSED");
    assert_eq!(pauses.load(Ordering::SeqCst), 1);
}

/// A sandbox that wakes after every `:pause` is an error after three attempts, not an Ok.
#[tokio::test]
async fn a_sandbox_that_keeps_waking_fails_the_pause_after_three_attempts() {
    let fake = PausableSandbox::running("boot-a");
    let pauses = Arc::new(AtomicUsize::new(0));
    let mut client = MockAgentPlatformApi::new();
    {
        let fake = fake.clone();
        client
            .expect_get_sandbox()
            .returning(move |_, id| Ok(sandbox_in_state(id, *fake.state.lock().unwrap())));
    }
    {
        let fake = fake.clone();
        client
            .expect_execute()
            .returning(move |_, _, input| fake.agent_reply(input));
    }
    {
        let pauses = pauses.clone();
        client.expect_pause_sandbox().returning(move |_, _| {
            // Accepted, but something wakes the sandbox at once: it stays running.
            pauses.fetch_add(1, Ordering::SeqCst);
            Ok(done_op(serde_json::json!({})))
        });
    }
    let error = provider(client).pause("s1").await.expect_err("did not hold");
    assert!(
        error.to_string().contains("pause did not hold"),
        "unexpected error: {error}"
    );
    assert_eq!(pauses.load(Ordering::SeqCst), 3);
}

// ---- output cap and oversized replies ---------------------------------------------------------

/// The refusal Agent Platform returns for a reply above its size limit, as the client reports it:
/// HTTP 400 `FAILED_PRECONDITION` becomes a conflict carrying Google's message.
fn reply_too_large_error() -> AlienError<AgentPlatformErrorData> {
    AlienError::new(alien_client_core::ErrorData::RemoteResourceConflict {
        resource_type: "sandbox".to_string(),
        resource_name: "s1".to_string(),
        message: "Response size too large. Received at least 2000341 bytes.".to_string(),
    })
    .context(AgentPlatformErrorData::ExecuteFailed {
        sandbox: "s1".to_string(),
        message: "the API rejected or cut short the request".to_string(),
    })
}

fn short_command() -> RunCommandRequest {
    RunCommandRequest {
        command: "/bin/echo".to_string(),
        args: vec![],
        cwd: None,
        env: BTreeMap::new(),
        timeout: Duration::from_secs(5),
    }
}

/// The synchronous envelope carries the per-stream reply budget, so the agent cuts the output to
/// fit; the job envelope does not (a job keeps all of it), and every poll carries it so each reply
/// fits. Mutation check: send the budget on `jobStart` and the job half fails.
#[tokio::test]
async fn the_reply_budget_cuts_exec_and_pages_job_polls() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().times(3).returning(|_, _, input| {
        let body: serde_json::Value = serde_json::from_slice(input).expect("json");
        let budget = &body["replyBudget"];
        match body["op"].as_str().unwrap() {
            "exec" => {
                assert_eq!(budget, &serde_json::json!(GCP_STREAM_REPLY_BUDGET));
                Ok(ndjson(&[exit_frame(0)]))
            }
            "jobStart" => {
                assert!(budget.is_null(), "a job is not cut: {body}");
                Ok(serde_json::to_vec(&serde_json::json!({ "jobId": "j1" })).unwrap())
            }
            "jobPoll" => {
                assert_eq!(budget, &serde_json::json!(GCP_STREAM_REPLY_BUDGET));
                Ok(
                    serde_json::to_vec(&serde_json::json!({"running": false, "exitCode": 0}))
                        .unwrap(),
                )
            }
            other => panic!("unexpected op {other}"),
        }
    });
    let sandbox = provider(client);
    let _: Vec<_> = sandbox
        .run_command("s1", short_command())
        .await
        .expect("runs")
        .collect()
        .await;
    let job = sandbox
        .start_job("s1", short_command())
        .await
        .expect("job starts");
    sandbox
        .poll_job("s1", &job.job_id, None)
        .await
        .expect("polls");
}

/// The worst reply the agent can build under the budget: both streams spent to the byte on
/// frames of the given size, each charged as the agent charges it (its JSON and a separator,
/// base64-encoded once more). Returns the sync reply and a job poll reply holding every frame.
fn worst_replies(data_bytes_per_frame: usize) -> (usize, usize) {
    let mut frames = Vec::new();
    let mut seq = 0u64;
    for name in ["stdout", "stderr"] {
        let mut remaining = GCP_STREAM_REPLY_BUDGET;
        loop {
            let frame = serde_json::json!({
                "t": name,
                "seq": seq,
                "data": BASE64.encode(vec![0xffu8; data_bytes_per_frame]),
            });
            // The agent's own charge, so this cannot drift from what it enforces.
            let agent_frame = alien_sandbox_agent::exec::Frame::Stdout {
                seq,
                data: BASE64.encode(vec![0xffu8; data_bytes_per_frame]),
            };
            let agent_frame = if name == "stderr" {
                alien_sandbox_agent::exec::Frame::Stderr {
                    seq,
                    data: BASE64.encode(vec![0xffu8; data_bytes_per_frame]),
                }
            } else {
                agent_frame
            };
            let cost = alien_sandbox_agent::exec::frame_wire_cost(&agent_frame);
            if cost > remaining {
                break;
            }
            remaining -= cost;
            seq += 1;
            frames.push(frame);
        }
    }
    frames.push(exit_frame(0));

    let sync = serde_json::to_vec(&serde_json::json!({
        "outputs": [{ "mimeType": "application/json", "data": BASE64.encode(ndjson(&frames)) }]
    }))
    .unwrap()
    .len();
    let poll = serde_json::to_vec(&serde_json::json!({
        "outputs": [{ "mimeType": "application/json", "data": BASE64.encode(serde_json::to_vec(
            &serde_json::json!({ "running": false, "frames": frames, "exitCode": 0, "truncated": true })
        ).unwrap()) }]
    }))
    .unwrap()
    .len();
    (sync, poll)
}

/// The budget exists so the worst reply stays under the proxy's limit, on `exec` and on a single
/// `jobPoll` that returns every frame, for frames of any size (one byte per line up to the
/// agent's 64 KiB reads). Mutation check: raise `GCP_STREAM_REPLY_BUDGET` to 1.1 MB and this fails.
#[test]
fn the_worst_reply_under_the_budget_fits_the_proxy_limit_with_margin() {
    for data in [1, 16, 256, 4096, 65_536] {
        let (sync, poll) = worst_replies(data);
        for (path, size) in [("exec", sync), ("jobPoll", poll)] {
            assert!(
                size * 10 < GCP_REPLY_LIMIT_BYTES * 9,
                "{path} with {data}-byte frames: reply is {size} bytes, limit {GCP_REPLY_LIMIT_BYTES}"
            );
        }
    }
}

/// A reply the proxy could not return is a clear, non-retryable failure, never an unknown outcome.
#[tokio::test]
async fn an_oversized_exec_reply_is_not_an_unknown_outcome() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(|_, _, _| Err(reply_too_large_error()));
    let error = match provider(client).run_command("s1", short_command()).await {
        Err(error) => error,
        Ok(_) => panic!("the oversized reply must fail"),
    };
    assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "{error}");
    assert!(!error.retryable, "{error}");
    assert!(error.to_string().contains("too large"), "{error}");
}

/// The same on the job path, where it used to read "the job is no longer watched".
#[tokio::test(start_paused = true)]
async fn an_oversized_job_poll_is_not_an_unknown_outcome() {
    let polls = AtomicUsize::new(0);
    let (client, _cancels) = cancel_watching_client_answering(move || {
        if polls.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(serde_json::to_vec(&serde_json::json!({"running": true, "frames": []})).unwrap())
        } else {
            Err(reply_too_large_error())
        }
    });
    let sandbox = provider(client);
    let frames: Vec<_> = sandbox
        .run_command("s1", long_command())
        .await
        .expect("the job starts")
        .collect()
        .await;
    let Some(Err(error)) = frames.last() else {
        panic!("the failed poll reaches the caller: {frames:?}");
    };
    assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "{error}");
    assert!(!error.retryable, "{error}");
}

/// A job whose output the agent returns in several budgeted replies, drained by a reader that
/// takes four seconds per frame, ran for its whole timeout and exited 0. The drain outlasts the
/// poll deadline's grace, and the command must still report its exit, not a timeout. Mutation
/// check: drop the deadline extension in `job_poll_step` and this reports `timeoutExceeded`.
#[tokio::test(start_paused = true)]
async fn a_job_still_draining_buffered_output_is_not_timed_out() {
    let polls = AtomicUsize::new(0);
    let (client, _cancels) = cancel_watching_client_answering(move || {
        let n = polls.fetch_add(1, Ordering::SeqCst);
        let body = if n < 20 {
            serde_json::json!({"running": true, "frames": [stdout_frame(n as u64, b"x")]})
        } else {
            serde_json::json!({"running": false, "frames": [], "exitCode": 0})
        };
        Ok(serde_json::to_vec(&body).unwrap())
    });
    let sandbox = provider(client);
    let mut request = long_command();
    request.timeout = Duration::from_secs(31);
    let mut frames = sandbox
        .run_command("s1", request)
        .await
        .expect("the job starts");
    let mut seen = Vec::new();
    while let Some(item) = frames.next().await {
        tokio::time::sleep(Duration::from_secs(4)).await;
        seen.push(item);
    }
    assert!(
        matches!(seen.last(), Some(Ok(CommandOutput::Exit { code: 0, .. }))),
        "{seen:?}"
    );
    assert_eq!(seen.len(), 21);
}

/// An agent that has reported the command finished (with its exit code) while pages of output
/// remain is only waiting to be read. A reader slower than the deadline must still get every page
/// and the real exit code, and the job is never cancelled. Mutation check: drop the `draining`
/// guard in `job_poll_step` and this reports `timeoutExceeded`.
#[tokio::test(start_paused = true)]
async fn a_finished_job_with_pages_left_is_drained_past_the_deadline() {
    let polls = AtomicUsize::new(0);
    let (client, mut cancels) = cancel_watching_client_answering(move || {
        let n = polls.fetch_add(1, Ordering::SeqCst);
        let body = if n < 5 {
            serde_json::json!({
                "running": true, "finished": true, "exitCode": 7,
                "frames": [stdout_frame(n as u64, b"x")]
            })
        } else {
            serde_json::json!({"running": false, "finished": true, "frames": [], "exitCode": 7})
        };
        Ok(serde_json::to_vec(&body).unwrap())
    });
    let sandbox = provider(client);
    let mut request = long_command();
    request.timeout = Duration::from_secs(31);
    let mut frames = sandbox
        .run_command("s1", request)
        .await
        .expect("the job starts");
    let mut seen = Vec::new();
    while let Some(item) = frames.next().await {
        // Far past the deadline plus grace, per page.
        tokio::time::sleep(Duration::from_secs(120)).await;
        seen.push(item);
    }
    assert!(
        matches!(seen.last(), Some(Ok(CommandOutput::Exit { code: 7, .. }))),
        "{seen:?}"
    );
    assert_eq!(seen.len(), 6, "{seen:?}");
    drop(frames);
    assert!(
        cancels.try_recv().is_err(),
        "a finished job is never cancelled"
    );
}

/// The agent reports a finished job's outcome while pages remain, but the public poll hands it out
/// only with the last page, so a caller that stops at the first `exit` or `error` misses nothing.
/// Mutation check: drop the `settled` filter in `poll_job` and the first poll carries the exit.
#[tokio::test]
async fn a_public_poll_reports_the_outcome_only_when_no_pages_remain() {
    let polls = AtomicUsize::new(0);
    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().times(3).returning(move |_, _, _| {
        let body = match polls.fetch_add(1, Ordering::SeqCst) {
            n @ 0..=1 => serde_json::json!({
                "running": true, "finished": true, "exitCode": 7, "truncated": true,
                "frames": [stdout_frame(n as u64, b"x")]
            }),
            _ => serde_json::json!({
                "running": false, "finished": true, "exitCode": 7, "truncated": true,
                "frames": [stdout_frame(2, b"x")]
            }),
        };
        Ok(serde_json::to_vec(&body).unwrap())
    });
    let sandbox = provider(client);
    let mut seq = None;
    for _ in 0..2 {
        let poll = sandbox.poll_job("s1", "j1", seq).await.expect("polls");
        assert!(
            poll.running && poll.exit.is_none() && poll.error.is_none(),
            "{poll:?}"
        );
        assert_eq!(poll.frames.len(), 1);
        seq = Some(seq.map_or(0, |s| s + 1));
    }
    let last = sandbox.poll_job("s1", "j1", seq).await.expect("polls");
    assert!(!last.running);
    assert_eq!(last.frames.len(), 1);
    assert!(
        matches!(
            last.exit,
            Some(JobExit {
                code: 7,
                truncated: true
            })
        ),
        "{last:?}"
    );
}

/// A file larger than one reply is read in pages and joined, and an agent without paging, which
/// answers with the whole file and no size, still reads.
#[tokio::test]
async fn read_file_pages_a_large_file() {
    let file: Vec<u8> = (0..(GCP_READ_PAGE_BYTES * 2 + 17))
        .map(|i| (i % 251) as u8)
        .collect();
    let served = file.clone();
    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().returning(move |_, _, input| {
        let body: serde_json::Value = serde_json::from_slice(input).unwrap();
        assert_eq!(body["op"], "readFile");
        let offset = body["offset"].as_u64().unwrap() as usize;
        let length = body["length"].as_u64().unwrap() as usize;
        let end = (offset + length).min(served.len());
        Ok(serde_json::to_vec(&serde_json::json!({
            "contentsBase64": BASE64.encode(&served[offset..end]),
            "totalBytes": served.len(),
            "version": "1-1.000000000-stable",
        }))
        .unwrap())
    });
    let read = provider(client)
        .read_file("s1", "big")
        .await
        .expect("reads");
    assert_eq!(read, file);
}

/// What a mock agent serves for one `readFile` call: the file's bytes and its identity.
type FileVersion = (Vec<u8>, &'static str);

/// A client whose agent serves `versions[n]` to the n-th call (the last one repeats), answering
/// each page from that version's bytes at the asked offset. Returns the client and the offsets
/// asked for.
fn paging_client(
    versions: Vec<FileVersion>,
) -> (
    MockAgentPlatformApi,
    std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
) {
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = asked.clone();
    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().returning(move |_, _, input| {
        let body: serde_json::Value = serde_json::from_slice(input).unwrap();
        let offset = body["offset"].as_u64().unwrap() as usize;
        let length = body["length"].as_u64().unwrap() as usize;
        let mut log = log.lock().unwrap();
        let (file, version) = &versions[log.len().min(versions.len() - 1)];
        log.push(offset);
        let start = offset.min(file.len());
        let end = (offset + length).min(file.len());
        Ok(serde_json::to_vec(&serde_json::json!({
            "contentsBase64": BASE64.encode(&file[start..end]),
            "totalBytes": file.len(),
            "version": version,
        }))
        .unwrap())
    });
    (client, asked)
}

fn big_file(fill: u8) -> Vec<u8> {
    vec![fill; GCP_READ_PAGE_BYTES * 2 + 5]
}

/// A file replaced by rename between pages is read again from the start, so the result is one
/// version and never page one of the old file joined to the rest of the new one.
#[tokio::test]
async fn read_file_restarts_once_when_the_file_is_replaced_between_pages() {
    let (client, asked) = paging_client(vec![(big_file(1), "old"), (big_file(2), "new")]);
    let read = provider(client).read_file("s1", "f").await.expect("reads");
    assert_eq!(read, big_file(2));
    assert_eq!(
        *asked.lock().unwrap(),
        vec![
            0,
            GCP_READ_PAGE_BYTES,
            0,
            GCP_READ_PAGE_BYTES,
            GCP_READ_PAGE_BYTES * 2
        ]
    );
}

/// A file that shrinks below the offset between pages is not returned as an old prefix: the
/// read restarts and returns the smaller file whole.
#[tokio::test]
async fn read_file_restarts_when_the_file_shrinks_below_the_offset() {
    let small = vec![9u8; 10];
    let (client, _) = paging_client(vec![(big_file(1), "old"), (small.clone(), "new")]);
    let read = provider(client).read_file("s1", "f").await.expect("reads");
    assert_eq!(read, small);
}

/// A file that keeps changing fails with a retryable error after the one restart.
#[tokio::test]
async fn read_file_fails_retryably_when_the_file_keeps_changing() {
    let (client, _) = paging_client(vec![
        (big_file(1), "a"),
        (big_file(2), "b"),
        (big_file(3), "c"),
        (big_file(4), "d"),
    ]);
    let error = provider(client)
        .read_file("s1", "f")
        .await
        .expect_err("changing file");
    assert!(error.retryable, "{error:?}");
    assert_eq!(error.code, "SANDBOX_FILE_CHANGED", "{error}");
    assert!(error.to_string().contains("'f'"), "{error}");
}

/// A file that does not change is read in one pass, one call per page, with no restart.
#[tokio::test]
async fn read_file_of_a_stable_file_does_not_restart() {
    let (client, asked) = paging_client(vec![(big_file(7), "same")]);
    let read = provider(client).read_file("s1", "f").await.expect("reads");
    assert_eq!(read, big_file(7));
    assert_eq!(asked.lock().unwrap().len(), 3);
}

/// The page size is what keeps a page's reply under the proxy's limit: raw bytes are base64-encoded
/// in the agent's JSON and again in the reply, 16/9 of the raw size, and the page must leave at
/// least a 20% margin under `GCP_REPLY_LIMIT_BYTES`. Changing the constant must change this test.
#[test]
fn read_page_size_leaves_a_margin_under_the_reply_limit() {
    assert_eq!(GCP_READ_PAGE_BYTES, 900_000);
    let on_the_wire = GCP_READ_PAGE_BYTES.div_ceil(3) * 4;
    let on_the_wire = on_the_wire.div_ceil(3) * 4;
    assert!(
        on_the_wire * 12 <= GCP_REPLY_LIMIT_BYTES * 10,
        "{on_the_wire} bytes on the wire leaves under a 20% margin below {GCP_REPLY_LIMIT_BYTES}"
    );
}

#[tokio::test]
async fn read_file_from_an_agent_without_paging_takes_the_whole_reply() {
    let mut client = MockAgentPlatformApi::new();
    client.expect_execute().times(1).returning(|_, _, _| {
        Ok(
            serde_json::to_vec(&serde_json::json!({ "contentsBase64": BASE64.encode(b"all") }))
                .unwrap(),
        )
    });
    assert_eq!(provider(client).read_file("s1", "f").await.unwrap(), b"all");
}

/// A file read that the proxy refuses for size names the file and the paged-read cause, not the
/// output budget, which has nothing to do with it.
#[tokio::test]
async fn an_oversized_file_reply_names_paged_reads_not_the_output_budget() {
    let mut client = MockAgentPlatformApi::new();
    client
        .expect_execute()
        .returning(|_, _, _| Err(reply_too_large_error()));
    let error = provider(client)
        .read_file("s1", "big")
        .await
        .expect_err("refused");
    let text = error.to_string();
    assert!(text.contains("paged reads"), "{text}");
    assert!(!text.contains("output budget"), "{text}");
}
