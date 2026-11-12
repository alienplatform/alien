//! The real `StackExecutor` drives `ExtensionProxyController`, whose every step crosses to a
//! controller that speaks the step protocol: a child process over stdin and stdout, or a server
//! on 127.0.0.1 over HTTP.

use alien_aws_clients::AwsClientConfigExt;
use alien_core::{
    AwsClientConfig, ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, Platform, ResourceStatus, ResourceType, Stack, StackResourceState,
    StackSettings, StackState,
};
use alien_infra::step_protocol::protocol::{
    codes, Credentials, ResponseBody, StepRequest, PROTOCOL_VERSION,
};
use alien_infra::step_protocol::test_controller::{
    ControllerHost, MockCloud, MockControl, DNS_ZONE,
};
use alien_infra::step_protocol::token::{self, Claims, Expected};
use alien_infra::step_protocol::{
    CredentialSource, HttpTransport, ProxyFactory, StdioTransport, TokenSigner, Transport,
    PROXY_CONTROLLER_TYPE,
};
use alien_infra::{
    deserialize_controller, serialize_controller, validate_controller_state_value,
    MockPlatformServiceProvider, ResourceControllerContext, ResourceRegistry, StackExecutor,
    StepResult,
};
use async_trait::async_trait;
use ed25519_compact::{KeyPair, PublicKey, Seed};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_step-protocol-test-controller");

// ---------------------------------------------------------------- fixtures

/// One deployment's controller: its cloud and step log in `dir`, reached over one transport.
struct Fixture {
    dir: TempDir,
    deployment_id: String,
    transport: Arc<dyn Transport>,
    stdio: Option<Arc<StdioTransport>>,
    /// Over HTTP: the server's base URL and the keys it trusts.
    url: Option<String>,
    trusted: Option<Arc<Mutex<Vec<PublicKey>>>>,
    credentials: Option<Arc<dyn CredentialSource>>,
    deadline: Duration,
}

impl Fixture {
    fn cloud(&self) -> MockCloud {
        MockCloud::at(self.dir.path())
    }

    fn log_path(&self) -> std::path::PathBuf {
        self.dir.path().join("log.jsonl")
    }

    /// Each handled step as `{request, response}`.
    fn log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.log_path())
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn requests(&self) -> Vec<Value> {
        self.log()
            .into_iter()
            .map(|line| line["request"].clone())
            .collect()
    }

    fn factory(&self) -> ProxyFactory {
        self.factory_with(self.transport.clone())
    }

    /// A factory for this deployment that reaches the controller over `transport`, as the
    /// next run of the deployment loop builds it.
    fn factory_with(&self, transport: Arc<dyn Transport>) -> ProxyFactory {
        ProxyFactory::new(
            self.deployment_id.clone(),
            transport,
            self.credentials.clone(),
        )
        .with_step_deadline(self.deadline)
    }
}

/// Launches the controller binary once per test binary, before any test times a step. macOS
/// stalls the first launch of a newly written binary while it scans it, for seconds on a loaded
/// machine, which would otherwise land inside a step's deadline.
fn launch_once() {
    static LAUNCHED: std::sync::Once = std::sync::Once::new();
    LAUNCHED.call_once(|| {
        // With stdin closed, the controller reads no message and exits.
        let cloud = TempDir::new().unwrap();
        let status = std::process::Command::new(BIN)
            .env("MOCK_CLOUD_DIR", cloud.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()
            .expect("the controller binary starts");
        assert!(
            status.success(),
            "the controller binary exited with {status}"
        );
    });
}

fn stdio_fixture(name: &str, deadline: Duration) -> Fixture {
    launch_once();
    let dir = TempDir::new().unwrap();
    let env = vec![
        (
            "MOCK_CLOUD_DIR".to_string(),
            dir.path().display().to_string(),
        ),
        (
            "STEP_LOG".to_string(),
            dir.path().join("log.jsonl").display().to_string(),
        ),
    ];
    let stdio = Arc::new(StdioTransport::new(BIN, Vec::new(), env));
    Fixture {
        dir,
        deployment_id: format!("dep_stdio_{name}"),
        transport: stdio.clone(),
        stdio: Some(stdio),
        url: None,
        trusted: None,
        credentials: None,
        deadline,
    }
}

/// Starts the controller process before a test times a step, so the first launch of a freshly
/// built binary (which the OS may scan) isn't counted.
async fn warm_up(fixture: &Fixture) {
    let stdio = fixture.stdio.as_ref().expect("a stdio fixture");
    stdio
        .load(
            "acme/none",
            Platform::Aws,
            json!({}),
            Duration::from_secs(30),
        )
        .await
        .expect("the controller process starts");
}

fn key_pair(seed: u8) -> KeyPair {
    KeyPair::from_seed(Seed::new([seed; 32]))
}

fn push_credentials() -> Credentials {
    Credentials::Aws {
        account_id: "123456789012".into(),
        region: "us-east-1".into(),
        access_key_id: "ASIAEXTENSION".into(),
        secret_access_key: "secret".into(),
        session_token: Some("session".into()),
        expires_at: Some("2099-01-01T00:00:00Z".into()),
    }
}

/// The credentials a push deployment sends: minted per step in a real manager, fixed here.
#[derive(Debug)]
struct FixedCredentials(Credentials);

#[async_trait]
impl CredentialSource for FixedCredentials {
    async fn credentials(
        &self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> alien_infra::Result<Credentials> {
        Ok(self.0.clone())
    }
}

/// The project the test manager's deployments belong to, and the test server serves.
const PROJECT: &str = "prj_test";

fn signer(key: &KeyPair) -> TokenSigner {
    TokenSigner {
        issuer: "alien-test-manager".into(),
        project: PROJECT.into(),
        secret_key: key.sk.clone(),
    }
}

fn http_transport(url: &str, key: &KeyPair) -> Arc<HttpTransport> {
    Arc::new(HttpTransport::new(url, signer(key)).allowing_local(Vec::new()))
}

#[derive(Clone)]
struct Server {
    host: Arc<ControllerHost>,
    audience: String,
    trusted: Arc<Mutex<Vec<PublicKey>>>,
}

/// `POST /v1/step` as an extension's server answers it: 400 with a `refused` reply for a
/// protocol it doesn't speak or a body it can't read, 401 for a token that fails a check,
/// 200 otherwise.
async fn step_handler(
    axum::extract::State(server): axum::extract::State<Server>,
    headers: axum::http::HeaderMap,
    raw: axum::body::Bytes,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::Json;

    let body: Value = serde_json::from_slice(&raw).unwrap_or_default();
    let step_id = body["stepId"].as_str().unwrap_or_default().to_string();
    let refused = |message: String| {
        let reply = json!({
            "protocol": PROTOCOL_VERSION, "stepId": step_id, "result": "refused",
            "error": { "code": codes::PROTOCOL_UNSUPPORTED, "message": message, "retryable": false },
            "supportedProtocols": [PROTOCOL_VERSION]
        });
        (StatusCode::BAD_REQUEST, Json(reply)).into_response()
    };
    if body["protocol"].as_u64() != Some(u64::from(PROTOCOL_VERSION)) {
        return refused(format!(
            "this controller speaks step protocol {PROTOCOL_VERSION}"
        ));
    }
    let request: StepRequest = match serde_json::from_value(body) {
        Ok(request) => request,
        Err(e) => return refused(format!("cannot read the request: {e}")),
    };
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let trusted = server.trusted.lock().unwrap().clone();
    let expected = Expected {
        issuer: "alien-test-manager",
        audience: &server.audience,
        projects: &[PROJECT],
        deployment_id: &request.deployment_id,
        extension: token::extension_of(&request.resource_type),
        resource_id: &request.resource_id,
        step_id: &request.step_id,
        body: &raw,
    };
    if let Err(reason) = token::verify(token, &trusted, expected, chrono::Utc::now().timestamp())
        .map_err(|refusal| refusal.to_string())
    {
        let body = json!({ "error": { "code": codes::AUTH_REJECTED, "message": reason } });
        return (StatusCode::UNAUTHORIZED, Json(body)).into_response();
    }
    (StatusCode::OK, Json(server.host.handle(request).await)).into_response()
}

/// Serves a controller on 127.0.0.1 that trusts `server_trusts`, behind a transport that signs
/// with `host_signs_with`.
async fn http_fixture(
    name: &str,
    server_trusts: &[&KeyPair],
    host_signs_with: &KeyPair,
    deadline: Duration,
) -> Fixture {
    use std::future::IntoFuture;
    let dir = TempDir::new().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let trusted = Arc::new(Mutex::new(server_trusts.iter().map(|key| key.pk).collect()));
    let server = Server {
        host: Arc::new(ControllerHost::new(
            dir.path(),
            Some(dir.path().join("log.jsonl")),
        )),
        audience: url.clone(),
        trusted: trusted.clone(),
    };
    let app = axum::Router::new()
        .route("/v1/step", axum::routing::post(step_handler))
        .with_state(server);
    tokio::spawn(axum::serve(listener, app).into_future());
    Fixture {
        dir,
        deployment_id: format!("dep_http_{name}"),
        transport: http_transport(&url, host_signs_with),
        stdio: None,
        url: Some(url),
        trusted: Some(trusted),
        credentials: Some(Arc::new(FixedCredentials(push_credentials()))),
        deadline,
    }
}

fn zone_entry(id: &str, domain: &str) -> Value {
    json!({
        "config": { "type": DNS_ZONE, "id": id, "domain": domain },
        "lifecycle": "live",
        "dependencies": []
    })
}

/// A stack as alien.ts writes it, so the zone loads as Alien's `CustomResource`.
fn stack(domain: &str) -> Stack {
    serde_json::from_value(json!({
        "id": "acme",
        "resources": { "zone": zone_entry("zone", domain) }
    }))
    .unwrap()
}

fn empty_stack() -> Stack {
    serde_json::from_value(json!({ "id": "acme", "resources": {} })).unwrap()
}

fn deployment_config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build()
}

fn registry_with(factory: ProxyFactory, platform: Platform) -> Arc<ResourceRegistry> {
    let mut registry = ResourceRegistry::with_built_ins();
    registry.register_controller_factory(ResourceType::from(DNS_ZONE), platform, Box::new(factory));
    Arc::new(registry)
}

fn executor(stack: &Stack, config: &DeploymentConfig, fixture: &Fixture) -> StackExecutor {
    executor_with(stack, config, fixture.factory())
}

fn executor_with(stack: &Stack, config: &DeploymentConfig, factory: ProxyFactory) -> StackExecutor {
    StackExecutor::builder(stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
        .deployment_config(config)
        .resource_registry(registry_with(factory, Platform::Aws))
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .build()
        .unwrap()
}

async fn step(executor: &StackExecutor, state: StackState) -> StepResult {
    executor
        .step(state)
        .await
        .unwrap_or_else(|e| panic!("executor.step failed: {} ({})", e.message, e.code))
}

fn zone(state: &StackState) -> &StackResourceState {
    &state.resources["zone"]
}

fn saved(state: &StackState) -> Value {
    zone(state).internal_state.clone().expect("internal state")
}

/// The proxy's saved state passes alien-infra's envelope check and survives a load and save.
fn assert_round_trip(state: &StackState) {
    let value = saved(state);
    validate_controller_state_value(&value, Some("zone")).unwrap();
    assert_eq!(value["type"], PROXY_CONTROLLER_TYPE);
    let controller = deserialize_controller(value.clone()).unwrap();
    assert_eq!(serialize_controller(controller.as_ref()).unwrap(), value);
}

/// Every request carries exactly the state the previous reply returned.
fn assert_state_chain(fixture: &Fixture) {
    let log = fixture.log();
    assert!(log.len() >= 2, "expected at least two logged steps");
    assert!(
        log[0]["request"]["state"].is_null(),
        "the first request starts from no state"
    );
    for pair in log.windows(2) {
        assert_eq!(
            pair[1]["request"]["state"], pair[0]["response"]["controller"]["state"],
            "a request's state differs from the previous reply's"
        );
    }
}

async fn step_until(
    executor: &StackExecutor,
    mut state: StackState,
    status: ResourceStatus,
    max: usize,
) -> StackState {
    for _ in 0..max {
        state = step(executor, state).await.next_state;
        assert_round_trip(&state);
        if zone(&state).status == status {
            return state;
        }
    }
    panic!(
        "never reached {status:?}; last: {:?} {:?}",
        zone(&state).status,
        zone(&state).error.as_ref().map(|e| &e.message)
    );
}

async fn wait_for(path: &std::path::Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{} never appeared", path.display());
}

/// Waits until the step log holds `count` whole lines: the file appears before its first line
/// is written.
async fn wait_for_log_lines(fixture: &Fixture, count: usize) {
    for _ in 0..200 {
        let whole = std::fs::read_to_string(fixture.log_path())
            .unwrap_or_default()
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).is_ok())
            .count();
        if whole >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the step log never held {count} lines");
}

// ---------------------------------------------------------------- lifecycle

async fn lifecycle(fixture: &Fixture) {
    let config = deployment_config();
    let cloud = fixture.cloud();

    // Create: CreateStart continues at once, CreateWait stays 2 s once, then Ready.
    let create = stack("a.example");
    let exec = executor(&create, &config, fixture);
    let s1 = step(&exec, StackState::new(Platform::Aws)).await;
    assert_eq!(zone(&s1.next_state).status, ResourceStatus::Provisioning);
    assert_eq!(
        s1.suggested_delay_ms,
        Some(0),
        "Continue with no delay steps again at once"
    );
    assert_eq!(
        saved(&s1.next_state)["remote"]["state"]["state"],
        "createWait"
    );
    assert_eq!(
        zone(&s1.next_state).progress.as_deref(),
        Some("Waiting for the zone to become active")
    );
    assert_round_trip(&s1.next_state);

    let s2 = step(&exec, s1.next_state).await;
    assert_eq!(
        s2.suggested_delay_ms,
        Some(2000),
        "Stay's delay reaches the executor"
    );
    assert_eq!(
        saved(&s2.next_state)["remote"]["state"]["internalStayCount"],
        1
    );
    assert_round_trip(&s2.next_state);

    let s3 = step(&exec, s2.next_state).await;
    assert_eq!(zone(&s3.next_state).status, ResourceStatus::Running);
    let outputs = serde_json::to_value(zone(&s3.next_state).outputs.as_ref().unwrap()).unwrap();
    assert_eq!(outputs["type"], DNS_ZONE);
    assert_eq!(outputs["domain"], "a.example");
    let zone_id = outputs["zoneId"].as_str().unwrap().to_string();
    assert!(
        zone(&s3.next_state).progress.is_none(),
        "progress clears at Running"
    );
    assert_eq!(
        s3.suggested_delay_ms,
        Some(30_000),
        "a running resource that suggests no delay waits as Alien's do, not at once"
    );
    assert_round_trip(&s3.next_state);

    let s4 = step(&exec, s3.next_state).await;
    assert_eq!(
        s4.suggested_delay_ms,
        Some(60_000),
        "the Ready handler's delay reaches the executor"
    );
    assert_eq!(cloud.state().zones.len(), 1);

    // Update: the executor starts it without a round trip; the next request carries the
    // transition and the previous config.
    let update = stack("b.example");
    let exec = executor(&update, &config, fixture);
    let state = step_until(&exec, s4.next_state, ResourceStatus::Running, 3).await;
    let outputs = serde_json::to_value(zone(&state).outputs.as_ref().unwrap()).unwrap();
    assert_eq!(outputs["domain"], "b.example");
    assert_eq!(
        cloud.calls("update"),
        vec![format!("update {zone_id} a.example -> b.example")]
    );
    let update_request = fixture
        .requests()
        .into_iter()
        .find(|r| r["transitions"] == json!(["update-start"]))
        .expect("a request carrying the update");
    assert_eq!(update_request["previousResource"]["domain"], "a.example");
    assert_eq!(update_request["resource"]["domain"], "b.example");

    // Delete: the resource left the stack.
    let gone = empty_stack();
    let exec = executor(&gone, &config, fixture);
    let state = step_until(&exec, state, ResourceStatus::Deleted, 3).await;
    assert!(cloud.state().zones.values().all(|z| z.deleted));
    assert!(fixture
        .requests()
        .iter()
        .any(|r| r["transitions"] == json!(["delete-start"])));
    assert_eq!(zone(&state).status, ResourceStatus::Deleted);

    assert_state_chain(fixture);
    let first = &fixture.requests()[0];
    assert_eq!(first["protocol"], PROTOCOL_VERSION);
    assert_eq!(first["resourceType"], DNS_ZONE);
    assert_eq!(first["deploymentId"], fixture.deployment_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn lifecycle_over_stdio() {
    let fixture = stdio_fixture("lifecycle", Duration::from_secs(10));
    lifecycle(&fixture).await;
    assert!(
        fixture
            .requests()
            .iter()
            .all(|r| r["credentials"].is_null()),
        "a local controller uses its own environment"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lifecycle_over_http() {
    let key = key_pair(1);
    let fixture = http_fixture("lifecycle", &[&key], &key, Duration::from_secs(10)).await;
    lifecycle(&fixture).await;
    let first = &fixture.requests()[0];
    assert_eq!(first["credentials"]["platform"], "aws");
    assert_eq!(first["credentials"]["accessKeyId"], "ASIAEXTENSION");
}

// ---------------------------------------------------------------- failures

#[tokio::test(flavor = "multi_thread")]
async fn a_retryable_error_is_retried_and_a_fatal_one_is_not() {
    let config = deployment_config();
    let create = stack("a.example");

    let fixture = stdio_fixture("retryable", Duration::from_secs(10));
    fixture.cloud().set_control(&MockControl {
        fail_once: Some("retryable".into()),
        ..Default::default()
    });
    let exec = executor(&create, &config, &fixture);
    let s1 = step(&exec, StackState::new(Platform::Aws)).await;
    let z = zone(&s1.next_state);
    assert_eq!(z.status, ResourceStatus::Provisioning);
    assert_eq!(z.retry_attempt, 1);
    let error = z.error.as_ref().unwrap();
    assert!(error.retryable, "{error:?}");
    assert!(
        error.message.contains("refused the create (retryable)"),
        "{}",
        error.message
    );
    assert_eq!(
        s1.suggested_delay_ms,
        Some(1000),
        "exponential backoff starts at 1 s"
    );
    assert_round_trip(&s1.next_state);
    let s2 = step(&exec, s1.next_state).await;
    let z = zone(&s2.next_state);
    assert_eq!(z.retry_attempt, 0);
    assert!(z.error.is_none());
    assert_eq!(
        saved(&s2.next_state)["remote"]["state"]["state"],
        "createWait"
    );

    let fixture = stdio_fixture("fatal", Duration::from_secs(10));
    fixture.cloud().set_control(&MockControl {
        fail_once: Some("fatal".into()),
        ..Default::default()
    });
    let exec = executor(&create, &config, &fixture);
    let s1 = step(&exec, StackState::new(Platform::Aws)).await;
    let z = zone(&s1.next_state);
    assert_eq!(z.status, ResourceStatus::ProvisionFailed);
    let error = z.error.as_ref().unwrap();
    assert!(!error.retryable);
    assert_eq!(
        error.code, "EXTENSION_STEP_FAILED",
        "Alien acts on its own codes, so a controller's can't pass for one"
    );
    assert!(
        error.message.contains("(CLOUD_PLATFORM_ERROR)"),
        "the controller's own code is in the message: {}",
        error.message
    );
    assert!(
        z.last_failed_state.is_some(),
        "the checkpoint before the failure is kept for retry"
    );
    assert_eq!(saved(&s1.next_state)["pending"], json!(["failure"]));
    assert_eq!(saved(&s1.next_state)["status"], "provision-failed");
    assert_round_trip(&s1.next_state);
}

fn kill(pid: u32) {
    let status = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

#[tokio::test(flavor = "multi_thread")]
async fn killing_the_child_mid_step_recovers_on_the_next_step() {
    let config = deployment_config();
    let create = stack("a.example");
    let fixture = stdio_fixture("kill", Duration::from_secs(30));
    let cloud = fixture.cloud();
    cloud.set_control(&MockControl {
        hang_once: true,
        ..Default::default()
    });
    let transport = fixture.stdio.clone().unwrap();

    let exec = executor(&create, &config, &fixture);
    let stepping = tokio::spawn(async move { exec.step(StackState::new(Platform::Aws)).await });
    wait_for(&cloud.hanging_marker()).await;
    let pid = transport.child_pid().expect("a child is running");
    kill(pid);
    let s1 = stepping.await.unwrap().unwrap();
    let z = zone(&s1.next_state);
    assert_eq!(z.status, ResourceStatus::Provisioning);
    assert_eq!(z.retry_attempt, 1);
    let error = z.error.as_ref().unwrap();
    assert_eq!(error.code, codes::UNREACHABLE);
    assert!(error.retryable);
    let in_flight = saved(&s1.next_state)["inFlightStepId"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        saved(&s1.next_state)["remote"].is_null(),
        "nothing came back, so no remote state is saved"
    );
    assert_eq!(
        cloud.state().zones.len(),
        1,
        "the cloud call ran before the kill"
    );
    assert_round_trip(&s1.next_state);

    let exec = executor(&create, &config, &fixture);
    let s2 = step(&exec, s1.next_state).await;
    let new_pid = transport.child_pid().expect("a new child");
    assert_ne!(new_pid, pid);
    let z = zone(&s2.next_state);
    assert!(z.error.is_none());
    assert_eq!(
        saved(&s2.next_state)["remote"]["state"]["state"],
        "createWait"
    );
    assert!(saved(&s2.next_state)["inFlightStepId"].is_null());
    assert_eq!(
        cloud.state().zones.len(),
        1,
        "the create ran twice but made one zone"
    );
    assert_eq!(cloud.calls("create ").len(), 2);
    let requests = fixture.requests();
    assert_eq!(
        requests.len(),
        1,
        "the killed process logged nothing; the resend did"
    );
    assert_eq!(
        requests[0]["stepId"], in_flight,
        "the resend carries the same step id"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_step_timeout_kills_and_respawns_the_child() {
    let config = deployment_config();
    let create = stack("a.example");
    let fixture = stdio_fixture("timeout", Duration::from_secs(1));
    fixture.cloud().set_control(&MockControl {
        hang_once: true,
        ..Default::default()
    });
    let exec = executor(&create, &config, &fixture);
    warm_up(&fixture).await;
    let started = std::time::Instant::now();
    let s1 = step(&exec, StackState::new(Platform::Aws)).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    let error = zone(&s1.next_state).error.as_ref().unwrap();
    assert_eq!(error.code, codes::UNREACHABLE);
    assert!(
        error.message.contains("did not answer within"),
        "{}",
        error.message
    );
    let s2 = step(&exec, s1.next_state).await;
    assert!(zone(&s2.next_state).error.is_none());
    assert_eq!(
        saved(&s2.next_state)["remote"]["state"]["state"],
        "createWait"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_reply_is_replayed_not_rerun() {
    let config = deployment_config();
    let create = stack("a.example");
    let key = key_pair(2);
    let fixture = http_fixture("replay", &[&key], &key, Duration::from_millis(500)).await;
    let cloud = fixture.cloud();
    // Blocks past the host's wait (the 500 ms deadline plus the grace), then answers late.
    cloud.set_control(&MockControl {
        block_once_ms: Some(4000),
        ..Default::default()
    });
    let exec = executor(&create, &config, &fixture);
    let s1 = step(&exec, StackState::new(Platform::Aws)).await;
    assert_eq!(
        zone(&s1.next_state).error.as_ref().unwrap().code,
        codes::UNREACHABLE
    );
    let in_flight = saved(&s1.next_state)["inFlightStepId"].clone();
    wait_for_log_lines(&fixture, 1).await;
    let s2 = step(&exec, s1.next_state).await;
    assert!(zone(&s2.next_state).error.is_none());
    assert_eq!(
        saved(&s2.next_state)["remote"]["state"]["state"],
        "createWait"
    );
    assert_eq!(
        cloud.calls("create ").len(),
        1,
        "the resend was answered from the replay cache"
    );
    let log = fixture.log();
    assert_eq!(log.len(), 2, "the first request and the resend");
    assert_eq!(log[0]["request"]["stepId"], in_flight);
    assert_eq!(
        log[1]["request"]["stepId"], in_flight,
        "the resend carries the same step id"
    );
    assert_eq!(
        log[1]["response"], log[0]["response"],
        "the resend got the first reply"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_controller_cuts_a_slow_step_at_the_deadline_and_keeps_its_state() {
    let config = deployment_config();
    let create = stack("a.example");
    let fixture = stdio_fixture("deadline", Duration::from_secs(1));
    let cloud = fixture.cloud();
    cloud.set_control(&MockControl {
        slow_once_ms: Some(3000),
        ..Default::default()
    });
    let exec = executor(&create, &config, &fixture);
    warm_up(&fixture).await;
    let started = std::time::Instant::now();
    let s1 = step(&exec, StackState::new(Platform::Aws)).await;
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(2500),
        "the controller answered at its deadline, not the host's: {elapsed:?}"
    );
    let z = zone(&s1.next_state);
    let error = z.error.as_ref().unwrap();
    assert_eq!(error.code, codes::STEP_DEADLINE);
    assert!(error.retryable);
    assert_eq!(z.status, ResourceStatus::Provisioning);
    // The handler recorded the zone id before it stalled, and the failed step's state is kept.
    assert!(saved(&s1.next_state)["remote"]["state"]["zoneId"].is_string());
    assert_eq!(
        saved(&s1.next_state)["remote"]["state"]["state"],
        "createStart"
    );
    assert!(
        saved(&s1.next_state)["inFlightStepId"].is_null(),
        "the step ran; the retry is a new step"
    );
    let s2 = step(&exec, s1.next_state).await;
    assert!(zone(&s2.next_state).error.is_none());
    assert_eq!(cloud.state().zones.len(), 1);
}

/// A request body with every field, for posting straight at a server.
fn request_body(config: &DeploymentConfig, protocol: u32, step_id: &str) -> Value {
    deployment_request_body(config, protocol, step_id, "dep", "p")
}

fn deployment_request_body(
    config: &DeploymentConfig,
    protocol: u32,
    step_id: &str,
    deployment_id: &str,
    resource_prefix: &str,
) -> Value {
    json!({
        "protocol": protocol, "stepId": step_id, "deploymentId": deployment_id,
        "resourceId": "zone", "resourceType": DNS_ZONE, "platform": "aws",
        "resourcePrefix": resource_prefix,
        "initialSetupAuthority": "importedHandoff", "lifecycle": null,
        "resource": { "type": DNS_ZONE, "id": "zone", "domain": "a.example" },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": serde_json::to_value(config).unwrap(),
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": null, "deadline": "2099-01-01T00:00:00Z"
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_or_expired_token_is_refused() {
    let config = deployment_config();
    let create = stack("a.example");
    let server_key = key_pair(3);
    let other_key = key_pair(4);
    let fixture = http_fixture(
        "badtoken",
        &[&server_key],
        &other_key,
        Duration::from_secs(10),
    )
    .await;
    let exec = executor(&create, &config, &fixture);
    let s1 = step(&exec, StackState::new(Platform::Aws)).await;
    let z = zone(&s1.next_state);
    assert_eq!(z.status, ResourceStatus::ProvisionFailed);
    let error = z.error.as_ref().unwrap();
    assert_eq!(error.code, codes::AUTH_REJECTED);
    assert!(!error.retryable);
    assert!(
        error.message.contains("token signature is not valid"),
        "{}",
        error.message
    );
    assert!(fixture.cloud().state().calls.is_empty(), "nothing ran");

    let url = fixture.url.clone().unwrap();
    let body = request_body(&config, 1, "s1");
    let now = chrono::Utc::now().timestamp();
    let sent = serde_json::to_vec(&body).unwrap();
    let claims = |aud: &str, project: &str, step_id: &str, exp: i64, body: &[u8]| Claims {
        iss: "alien-test-manager".into(),
        aud: aud.into(),
        project: project.into(),
        deployment_id: "dep".into(),
        extension: "acme".into(),
        resource_id: "zone".into(),
        step_id: step_id.into(),
        body_sha256: token::body_sha256(body),
        iat: now - 10,
        exp,
        jti: "j".into(),
    };
    let mint = |claims: Claims| token::mint(&claims, &server_key.sk).unwrap();
    let client = reqwest::Client::new();
    let step_url = format!("{url}/v1/step");
    let post = |token: String| {
        client
            .post(&step_url)
            .bearer_auth(token)
            .header("content-type", "application/json")
            .body(sent.clone())
            .send()
    };
    for (name, token, expect) in [
        (
            "expired",
            mint(claims(&url, PROJECT, "s1", now - 1, &sent)),
            "token has expired",
        ),
        (
            "audience",
            mint(claims(
                "https://elsewhere.dev",
                PROJECT,
                "s1",
                now + 60,
                &sent,
            )),
            "another audience",
        ),
        (
            "project",
            mint(claims(&url, "prj_other", "s1", now + 60, &sent)),
            "another project (prj_other)",
        ),
        (
            "step",
            mint(claims(&url, PROJECT, "other", now + 60, &sent)),
            "does not match the request",
        ),
        (
            "body",
            mint(claims(&url, PROJECT, "s1", now + 60, b"{}")),
            "does not match the request",
        ),
        ("garbage", "not.a.token".to_string(), "malformed"),
    ] {
        let response = post(token).await.unwrap();
        assert_eq!(response.status().as_u16(), 401, "{name}");
        let text = response.text().await.unwrap();
        assert!(text.contains(expect), "{name}: {text}");
    }
    let good = mint(claims(&url, PROJECT, "s1", now + 60, &sent));
    assert_eq!(post(good).await.unwrap().status().as_u16(), 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_newer_protocol_version_is_refused_clearly() {
    use std::future::IntoFuture;
    // A server that answers with protocol 2 and a field version 1 doesn't have.
    async fn v2(axum::Json(body): axum::Json<Value>) -> axum::Json<Value> {
        axum::Json(json!({
            "protocol": 2, "stepId": body["stepId"], "result": "stepped",
            "controller": { "state": {}, "status": "running", "progress": null, "outputs": null,
                "bindingParams": null, "onFailureStatus": "refresh-failed",
                "updateStart": {"outcome": "allowed", "status": "updating"},
                "deleteStart": {"outcome": "allowed", "status": "deleting"},
                "teardownStart": {"outcome": "allowed", "status": "deleting"},
                "somethingNew": true },
            "suggestedDelayMs": null, "heartbeats": []
        }))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route("/v1/step", axum::routing::post(v2));
    tokio::spawn(axum::serve(listener, app).into_future());
    let fixture = Fixture {
        dir: TempDir::new().unwrap(),
        deployment_id: "dep_v2".into(),
        transport: http_transport(&url, &key_pair(5)),
        stdio: None,
        url: Some(url),
        trusted: None,
        credentials: Some(Arc::new(FixedCredentials(push_credentials()))),
        deadline: Duration::from_secs(5),
    };
    let config = deployment_config();
    let create = stack("a.example");
    let exec = executor(&create, &config, &fixture);
    let s1 = step(&exec, StackState::new(Platform::Aws)).await;
    let z = zone(&s1.next_state);
    assert_eq!(z.status, ResourceStatus::ProvisionFailed);
    let error = z.error.as_ref().unwrap();
    assert_eq!(error.code, codes::PROTOCOL_UNSUPPORTED);
    assert!(!error.retryable);
    assert!(
        error.message.contains("protocol 2") && error.message.contains("up to 1"),
        "{}",
        error.message
    );

    // The controller side refuses a request from a newer host the same way.
    let dir = TempDir::new().unwrap();
    let host = ControllerHost::new(dir.path(), None);
    let request: StepRequest = serde_json::from_value(request_body(&config, 2, "s1")).unwrap();
    match host.handle(request).await.body {
        ResponseBody::Refused {
            error,
            supported_protocols,
        } => {
            assert_eq!(error.code, codes::PROTOCOL_UNSUPPORTED);
            assert_eq!(supported_protocols, Some(vec![1]));
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// ---------------------------------------------------------------- what the POC did not cover

async fn create_to_running(fixture: &Fixture, stack: &Stack) -> StackState {
    let exec = executor(stack, &deployment_config(), fixture);
    step_until(
        &exec,
        StackState::new(Platform::Aws),
        ResourceStatus::Running,
        5,
    )
    .await
}

/// A Ready handler that fails leaves the resource RefreshFailed. The executor keeps stepping it
/// (the controller applies `failure` and stays in its RefreshFailed state), and a retry resumes
/// the Ready handler with a fresh polling count.
#[tokio::test(flavor = "multi_thread")]
async fn a_refresh_failure_keeps_stepping_and_a_retry_resumes_the_ready_handler() {
    use alien_infra::StackStateExt;
    let fixture = stdio_fixture("refresh", Duration::from_secs(10));
    let create = stack("a.example");
    let running = create_to_running(&fixture, &create).await;
    fixture.cloud().set_control(&MockControl {
        fail_refresh_once: true,
        ..Default::default()
    });

    let exec = executor(&create, &deployment_config(), &fixture);
    let failed = step(&exec, running).await;
    let z = zone(&failed.next_state);
    assert_eq!(z.status, ResourceStatus::RefreshFailed);
    assert!(!z.error.as_ref().unwrap().retryable);
    assert_eq!(
        failed.suggested_delay_ms,
        Some(30_000),
        "a refresh failure waits before its next step"
    );
    assert_eq!(saved(&failed.next_state)["pending"], json!(["failure"]));

    let stepped = step(&exec, failed.next_state).await;
    let last = fixture.requests().pop().unwrap();
    assert_eq!(
        last["transitions"],
        json!(["failure"]),
        "RefreshFailed is still stepped"
    );
    assert_eq!(
        saved(&stepped.next_state)["remote"]["state"]["state"],
        "refreshFailed"
    );
    assert_eq!(
        zone(&stepped.next_state).status,
        ResourceStatus::RefreshFailed
    );

    let mut retried = stepped.next_state;
    assert_eq!(retried.retry_failed().unwrap(), vec!["zone".to_string()]);
    assert_eq!(zone(&retried).status, ResourceStatus::Running);
    let gets_before = fixture.cloud().calls("get ").len();
    let recovered = step(&exec, retried).await;
    let last = fixture.requests().pop().unwrap();
    assert_eq!(last["transitions"], json!(["reset-stay-count"]));
    assert_eq!(
        last["state"]["state"], "ready",
        "the retry resumes from the step before the failure"
    );
    assert_eq!(zone(&recovered.next_state).status, ResourceStatus::Running);
    assert!(zone(&recovered.next_state).error.is_none());
    assert_eq!(
        fixture.cloud().calls("get ").len(),
        gets_before + 1,
        "the Ready handler ran"
    );
}

/// Setup teardown starts the controller's teardown, which the next request carries.
#[tokio::test(flavor = "multi_thread")]
async fn a_teardown_start_reaches_the_controller() {
    use alien_core::ResourceLifecycle;
    use alien_infra::StackStateExt;
    let fixture = stdio_fixture("teardown", Duration::from_secs(10));
    let mut state = create_to_running(&fixture, &stack("a.example")).await;
    // Setup teardown runs on resources a runtime delete left for it.
    state.resources.get_mut("zone").unwrap().status = ResourceStatus::TeardownRequired;
    let prepared = state
        .prepare_for_teardown_with_lifecycle_filter(&[ResourceLifecycle::Live])
        .unwrap();
    assert_eq!(prepared, vec!["zone".to_string()]);
    assert_eq!(
        zone(&state).status,
        ResourceStatus::Deleting,
        "the view's teardown preview"
    );

    let exec = executor(&empty_stack(), &deployment_config(), &fixture);
    let state = step_until(&exec, state, ResourceStatus::Deleted, 3).await;
    assert_eq!(zone(&state).status, ResourceStatus::Deleted);
    let teardown = fixture
        .requests()
        .into_iter()
        .find(|r| r["transitions"] == json!(["teardown-start"]))
        .expect("a request carrying the teardown");
    assert_eq!(teardown["state"]["state"], "ready");
    assert!(fixture.cloud().state().zones.values().all(|z| z.deleted));
}

/// A custom resource that depends on Alien's own storage gets the bucket's status and outputs
/// in the request, never its saved state, and its controller reads them.
#[tokio::test(flavor = "multi_thread")]
async fn a_controller_reads_the_outputs_of_an_alien_dependency() {
    use alien_core::{Resource, Storage};
    let fixture = stdio_fixture("dependency", Duration::from_secs(10));
    let bucket = Resource::new(Storage::new("bucket".to_string()).build());
    let stack: Stack = serde_json::from_value(json!({
        "id": "acme",
        "resources": {
            "bucket": { "config": bucket, "lifecycle": "live", "dependencies": [] },
            "zone": {
                "config": { "type": DNS_ZONE, "id": "zone", "domain": "a.example" },
                "lifecycle": "live",
                "dependencies": [{ "type": "storage", "id": "bucket" }]
            }
        }
    }))
    .unwrap();
    let exec = StackExecutor::builder(&stack, ClientConfig::Test)
        .deployment_config(&deployment_config())
        .resource_registry(registry_with(fixture.factory(), Platform::Test))
        .build()
        .unwrap();
    let mut state = StackState::new(Platform::Test);
    for _ in 0..20 {
        state = step(&exec, state).await.next_state;
        if state
            .resources
            .get("zone")
            .is_some_and(|z| z.status == ResourceStatus::Running)
        {
            break;
        }
    }
    assert_eq!(zone(&state).status, ResourceStatus::Running);

    let first = &fixture.requests()[0];
    let dependency = &first["dependencies"]["bucket"];
    assert_eq!(dependency["resourceType"], "storage");
    assert_eq!(
        dependency["status"], "running",
        "the zone waited for its dependency"
    );
    assert_eq!(dependency["outputs"]["type"], "storage");
    let bucket_name = dependency["outputs"]["bucketName"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(dependency.get("internalState").is_none());
    assert_eq!(first["platform"], "test");
    let reads = fixture.cloud().calls("read bucket");
    assert_eq!(reads.len(), 1);
    assert!(reads[0].contains(&bucket_name), "{reads:?}");
}

/// The executor steps up to four resources at once. One controller process answers them one
/// at a time, and each gets its own reply.
#[tokio::test(flavor = "multi_thread")]
async fn four_resources_step_at_once_over_one_process() {
    let fixture = stdio_fixture("four", Duration::from_secs(10));
    let ids = ["z1", "z2", "z3", "z4"];
    let resources: serde_json::Map<String, Value> = ids
        .iter()
        .map(|id| (id.to_string(), zone_entry(id, &format!("{id}.example"))))
        .collect();
    let stack: Stack =
        serde_json::from_value(json!({ "id": "acme", "resources": resources })).unwrap();
    let exec = executor(&stack, &deployment_config(), &fixture);
    let transport = fixture.stdio.clone().unwrap();

    let s1 = step(&exec, StackState::new(Platform::Aws)).await;
    let pid = transport.child_pid().expect("a child is running");
    let mut stepped: Vec<String> = fixture
        .requests()
        .iter()
        .map(|r| r["resourceId"].as_str().unwrap().to_string())
        .collect();
    stepped.sort();
    assert_eq!(
        stepped, ids,
        "each resource sent one step in the same executor step"
    );
    for id in ids {
        let z = &s1.next_state.resources[id];
        assert!(z.error.is_none(), "{id}: {:?}", z.error);
        assert_eq!(
            z.internal_state.as_ref().unwrap()["remote"]["state"]["state"],
            "createWait"
        );
    }

    let mut state = s1.next_state;
    for _ in 0..3 {
        state = step(&exec, state).await.next_state;
    }
    for id in ids {
        assert_eq!(state.resources[id].status, ResourceStatus::Running, "{id}");
        let outputs = serde_json::to_value(state.resources[id].outputs.as_ref().unwrap()).unwrap();
        assert_eq!(
            outputs["domain"],
            format!("{id}.example"),
            "each reply went to its own resource"
        );
    }
    assert_eq!(
        transport.child_pid(),
        Some(pid),
        "one process served every step"
    );
    assert_eq!(fixture.cloud().state().zones.len(), 4);
}

/// A manager rotates its signing key by trusting both keys on the server, moving the host to
/// the new key, then retiring the old one.
#[tokio::test(flavor = "multi_thread")]
async fn a_rotated_signing_key_keeps_the_resource_stepping() {
    let old_key = key_pair(6);
    let new_key = key_pair(7);
    let fixture = http_fixture("rotation", &[&old_key], &old_key, Duration::from_secs(10)).await;
    let url = fixture.url.clone().unwrap();
    let trusted = fixture.trusted.clone().unwrap();
    let create = stack("a.example");
    let config = deployment_config();

    let s1 = step(
        &executor(&create, &config, &fixture),
        StackState::new(Platform::Aws),
    )
    .await;
    assert!(zone(&s1.next_state).error.is_none());

    *trusted.lock().unwrap() = vec![old_key.pk, new_key.pk];
    let rotated = fixture.factory_with(http_transport(&url, &new_key));
    let s2 = step(&executor_with(&create, &config, rotated), s1.next_state).await;
    assert!(
        zone(&s2.next_state).error.is_none(),
        "{:?}",
        zone(&s2.next_state).error
    );

    *trusted.lock().unwrap() = vec![new_key.pk];
    let rotated = fixture.factory_with(http_transport(&url, &new_key));
    let s3 = step(&executor_with(&create, &config, rotated), s2.next_state).await;
    assert_eq!(zone(&s3.next_state).status, ResourceStatus::Running);

    // A host still on the retired key is refused.
    let stale = step(&executor(&create, &config, &fixture), s3.next_state).await;
    let error = zone(&stale.next_state).error.as_ref().unwrap();
    assert_eq!(error.code, codes::AUTH_REJECTED);
}

/// An update whose step never reaches the controller fails as an update. The proxy's view still
/// describes the Ready state, whose failure is RefreshFailed, so the status comes from the
/// started update instead. A retry with the right key then finishes the update.
#[tokio::test(flavor = "multi_thread")]
async fn an_update_refused_before_the_controller_fails_as_an_update() {
    use alien_infra::StackStateExt;
    let key = key_pair(8);
    let unknown = key_pair(9);
    let fixture = http_fixture("update-refused", &[&key], &key, Duration::from_secs(10)).await;
    let url = fixture.url.clone().unwrap();
    let running = create_to_running(&fixture, &stack("a.example")).await;
    assert_eq!(
        saved(&running)["remote"]["onFailureStatus"],
        "refresh-failed"
    );

    let update = stack("b.example");
    let config = deployment_config();
    let wrong = fixture.factory_with(http_transport(&url, &unknown));
    let failed = step(&executor_with(&update, &config, wrong), running).await;
    let z = zone(&failed.next_state);
    assert_eq!(z.status, ResourceStatus::UpdateFailed);
    assert_eq!(z.error.as_ref().unwrap().code, codes::AUTH_REJECTED);
    assert_eq!(
        saved(&failed.next_state)["pending"],
        json!(["update-start", "failure"])
    );

    let mut retried = failed.next_state;
    retried.retry_failed().unwrap();
    assert_eq!(zone(&retried).status, ResourceStatus::Updating);
    let exec = executor(&update, &config, &fixture);
    let state = step_until(&exec, retried, ResourceStatus::Running, 3).await;
    let last_update = fixture
        .requests()
        .into_iter()
        .find(|r| r["transitions"] == json!(["update-start", "reset-stay-count"]))
        .expect("the retry carries the update it resumes");
    assert_eq!(last_update["previousResource"]["domain"], "a.example");
    let outputs = serde_json::to_value(zone(&state).outputs.as_ref().unwrap()).unwrap();
    assert_eq!(outputs["domain"], "b.example");
}

/// Two deployments with the same extension reach their own controllers in one process.
#[tokio::test(flavor = "multi_thread")]
async fn two_deployments_with_one_extension_use_their_own_transports() {
    let key = key_pair(10);
    let local = stdio_fixture("two", Duration::from_secs(10));
    let hosted = http_fixture("two", &[&key], &key, Duration::from_secs(10)).await;
    let create = stack("a.example");
    let config = deployment_config();
    let local_exec = executor(&create, &config, &local);
    let hosted_exec = executor(&create, &config, &hosted);
    let mut local_state = StackState::new(Platform::Aws);
    let mut hosted_state = StackState::new(Platform::Aws);
    for _ in 0..3 {
        local_state = step(&local_exec, local_state).await.next_state;
        hosted_state = step(&hosted_exec, hosted_state).await.next_state;
    }
    assert_eq!(zone(&local_state).status, ResourceStatus::Running);
    assert_eq!(zone(&hosted_state).status, ResourceStatus::Running);
    for (fixture, credentials) in [(&local, false), (&hosted, true)] {
        let requests = fixture.requests();
        assert_eq!(requests.len(), 3, "{}", fixture.deployment_id);
        for request in requests {
            assert_eq!(request["deploymentId"], fixture.deployment_id);
            assert_eq!(!request["credentials"].is_null(), credentials);
        }
        assert_eq!(fixture.cloud().state().zones.len(), 1);
    }
}

/// The replay cache is keyed by deployment: another deployment's step with the same id runs.
#[tokio::test(flavor = "multi_thread")]
async fn the_replay_cache_never_answers_another_deployments_step() {
    let dir = TempDir::new().unwrap();
    let host = ControllerHost::new(dir.path(), None);
    let config = deployment_config();
    let request = |deployment_id: &str, prefix: &str| -> StepRequest {
        serde_json::from_value(deployment_request_body(
            &config,
            1,
            "same-step",
            deployment_id,
            prefix,
        ))
        .unwrap()
    };
    let first = host.handle(request("dep_a", "a")).await;
    let other = host.handle(request("dep_b", "b")).await;
    let resent = host.handle(request("dep_a", "a")).await;
    assert_ne!(other, first, "another deployment's step ran");
    assert_eq!(resent, first, "the same deployment's resend is replayed");
    let cloud = MockCloud::at(dir.path());
    assert_eq!(cloud.calls("create ").len(), 2);
    assert_eq!(cloud.state().zones.len(), 2);
}

/// A resend that arrives while the first send still runs waits for it, and gets its reply.
#[tokio::test(flavor = "multi_thread")]
async fn a_resend_during_the_step_waits_for_it_instead_of_running_again() {
    let dir = TempDir::new().unwrap();
    let cloud = MockCloud::at(dir.path());
    cloud.set_control(&MockControl {
        slow_once_ms: Some(500),
        ..Default::default()
    });
    let host = ControllerHost::new(dir.path(), None);
    let config = deployment_config();
    let request = || -> StepRequest {
        serde_json::from_value(deployment_request_body(
            &config,
            1,
            "same-step",
            "dep_a",
            "a",
        ))
        .unwrap()
    };
    let (first, resent) = tokio::join!(host.handle(request()), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        host.handle(request()).await
    });
    assert_eq!(resent, first);
    assert_eq!(cloud.calls("create ").len(), 1, "the step ran once");
}

/// A request that goes away mid-step, as one whose client disconnects does, doesn't stop the
/// step: its resend waits for the same run and gets its reply.
#[tokio::test(flavor = "multi_thread")]
async fn a_step_whose_request_goes_away_still_runs_once_for_its_resend() {
    let dir = TempDir::new().unwrap();
    let cloud = MockCloud::at(dir.path());
    cloud.set_control(&MockControl {
        slow_once_ms: Some(500),
        ..Default::default()
    });
    let host = ControllerHost::new(dir.path(), None);
    let config = deployment_config();
    let request = || -> StepRequest {
        serde_json::from_value(deployment_request_body(
            &config,
            1,
            "gone-step",
            "dep_a",
            "a",
        ))
        .unwrap()
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(100), host.handle(request()))
            .await
            .is_err(),
        "the first request goes away while the step runs"
    );
    let resent = host.handle(request()).await;
    assert!(
        matches!(resent.body, ResponseBody::Stepped { .. }),
        "{resent:?}"
    );
    assert_eq!(cloud.calls("create ").len(), 1, "the step ran once");
}

/// The replay cache keeps a step that still runs, however many others finish meanwhile, so its
/// resend never runs it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_running_step_stays_in_the_replay_cache_past_its_capacity() {
    let dir = TempDir::new().unwrap();
    let cloud = MockCloud::at(dir.path());
    cloud.set_control(&MockControl {
        slow_once_ms: Some(1500),
        ..Default::default()
    });
    let host = Arc::new(ControllerHost::new(dir.path(), None));
    let config = deployment_config();
    let request = |step: &str, prefix: &str| -> StepRequest {
        serde_json::from_value(deployment_request_body(&config, 1, step, "dep_a", prefix)).unwrap()
    };
    let slow = tokio::spawn({
        let host = host.clone();
        let request = request("slow-step", "slow");
        async move { host.handle(request).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    for n in 0..80 {
        host.handle(request(&format!("quick-{n}"), &format!("q{n}")))
            .await;
    }
    let resent = host.handle(request("slow-step", "slow")).await;
    assert_eq!(resent, slow.await.unwrap());
    assert_eq!(
        cloud.calls("create ").len(),
        81,
        "the slow step ran once, and each quick one once"
    );
}

/// An importer's fields wait on the proxy until the first step, which carries them as
/// `pendingImport` with no state. The controller loads them and runs its Ready handler; it
/// never creates the zone.
#[tokio::test(flavor = "multi_thread")]
async fn an_imported_resource_loads_on_its_first_step_without_a_create() {
    use alien_core::ResourceLifecycle;
    use alien_infra::step_protocol::ExtensionProxyController;
    let fixture = stdio_fixture("import", Duration::from_secs(10));
    fixture
        .cloud()
        .seed_zone("setup-template", "ZSETUP", "a.example");
    let stack = stack("a.example");
    let fields = json!({ "state": "ready", "zoneId": "ZSETUP", "domain": "a.example" });
    let proxy = ExtensionProxyController::imported(fields.clone(), ResourceStatus::Running, None);
    let mut imported = StackResourceState::builder()
        .resource_type(DNS_ZONE.to_string())
        .status(ResourceStatus::Running)
        .config(stack.resources["zone"].config.clone())
        .lifecycle(ResourceLifecycle::Frozen)
        .build();
    imported.internal_state = Some(serialize_controller(&proxy).unwrap());
    imported.controller_platform = Some(Platform::Aws);
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert("zone".to_string(), imported);
    assert_round_trip(&state);

    let stepped = step(&executor(&stack, &deployment_config(), &fixture), state).await;
    let first = &fixture.requests()[0];
    assert!(first["state"].is_null());
    assert_eq!(first["pendingImport"], fields);
    let z = zone(&stepped.next_state);
    assert_eq!(z.status, ResourceStatus::Running);
    assert!(z.error.is_none(), "{:?}", z.error);
    assert!(
        saved(&stepped.next_state)["pendingImport"].is_null(),
        "loaded once"
    );
    assert_eq!(
        saved(&stepped.next_state)["remote"]["state"]["zoneId"],
        "ZSETUP"
    );
    let outputs = serde_json::to_value(z.outputs.as_ref().unwrap()).unwrap();
    assert_eq!(outputs["zoneId"], "ZSETUP");
    let cloud = fixture.cloud();
    assert!(
        cloud.calls("create ").is_empty(),
        "{:?}",
        cloud.state().calls
    );
    assert_eq!(cloud.calls("get ZSETUP").len(), 1);

    let next = step(
        &executor(&stack, &deployment_config(), &fixture),
        stepped.next_state,
    )
    .await;
    assert!(fixture.requests()[1].get("pendingImport").is_none());
    assert_eq!(zone(&next.next_state).status, ResourceStatus::Running);
}

/// `load` asks the controller for the view of an importer's fields, with no step and no cloud
/// call, on the same process that steps.
#[tokio::test(flavor = "multi_thread")]
async fn load_returns_the_view_of_an_importers_fields() {
    use alien_infra::step_protocol::protocol::LoadResult;
    let fixture = stdio_fixture("load", Duration::from_secs(10));
    let transport = fixture.stdio.clone().unwrap();
    let fields = json!({ "state": "ready", "zoneId": "Z123", "domain": "a.example" });
    let LoadResult::View(view) = transport
        .load(DNS_ZONE, Platform::Aws, fields, Duration::from_secs(5))
        .await
        .unwrap()
    else {
        panic!("the controller refused the fields");
    };
    assert_eq!(
        view.state["type"], "DnsZoneController",
        "the controller adds its envelope"
    );
    assert_eq!(view.state["_controllerStateVersion"], 1);
    assert_eq!(view.state["zoneId"], "Z123");
    assert_eq!(view.status, ResourceStatus::Running);
    let outputs = serde_json::to_value(view.outputs.unwrap()).unwrap();
    assert_eq!(
        outputs,
        json!({ "type": DNS_ZONE, "zoneId": "Z123", "domain": "a.example" })
    );
    assert!(fixture.cloud().state().calls.is_empty(), "no cloud call");

    let reserved = json!({ "type": "DnsZoneController", "state": "ready", "zoneId": "Z123" });
    let result = transport
        .load(DNS_ZONE, Platform::Aws, reserved, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        matches!(&result, LoadResult::Refused(message) if message.contains("'type'")),
        "{result:?}"
    );
    let unknown = transport
        .load(
            "acme/other",
            Platform::Aws,
            json!({}),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert!(matches!(unknown, LoadResult::Refused(_)), "{unknown:?}");
}

/// What the manager does between steps, here as JSON: the custom resource's config, the
/// proxy's saved state and the outputs survive the store, a worker that depends on the custom
/// resource is created after it, and its Ready step reports a custom heartbeat.
#[tokio::test(flavor = "multi_thread")]
async fn executor_steps_a_custom_resource_through_saved_state() {
    use alien_core::{
        CustomResource, CustomResourceOutputs, DeploymentState, DeploymentStatus, Resource,
        ResourceHeartbeatData, Worker, WorkerCode,
    };
    let fixture = stdio_fixture("saved-state", Duration::from_secs(10));
    let api = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "api:1".to_string(),
        })
        .permissions("api".to_string())
        .build();
    let stack: Stack = serde_json::from_value(json!({
        "id": "acme",
        "resources": {
            "zone": zone_entry("zone", "internal.acme.dev"),
            "api": {
                "config": Resource::new(api),
                "lifecycle": "live",
                "dependencies": [{ "type": DNS_ZONE, "id": "zone" }]
            }
        }
    }))
    .unwrap();
    let config = deployment_config();
    let executor = StackExecutor::builder(&stack, ClientConfig::Test)
        .deployment_config(&config)
        .resource_registry(registry_with(fixture.factory(), Platform::Test))
        .build()
        .expect("the registry knows the custom type");
    let synced = |state: &StackState| {
        stack.resources.iter().all(|(id, entry)| {
            state
                .resources
                .get(id)
                .is_some_and(|r| r.status == ResourceStatus::Running && r.config == entry.config)
        })
    };

    let mut state = StackState::new(Platform::Test);
    let mut custom_heartbeats = 0;
    for _ in 0..40 {
        if synced(&state) {
            break;
        }
        let step = step(&executor, state).await;
        custom_heartbeats += step
            .heartbeats
            .iter()
            .filter(|h| matches!(h.data, ResourceHeartbeatData::Custom(_)))
            .count();
        let deployment = DeploymentState::builder()
            .status(DeploymentStatus::Provisioning)
            .platform(Platform::Test)
            .stack_state(step.next_state)
            .retry_requested(false)
            .protocol_version(1)
            .build();
        let stored = serde_json::to_value(&deployment).unwrap();
        let loaded: DeploymentState = serde_json::from_value(stored.clone()).unwrap();
        assert_eq!(serde_json::to_value(&loaded).unwrap(), stored);
        state = loaded.stack_state.unwrap();
    }

    assert!(synced(&state), "not synced: {state:#?}");
    let zone = &state.resources["zone"];
    assert_eq!(zone.resource_type, DNS_ZONE);
    assert!(zone.config.downcast_ref::<CustomResource>().is_some());
    let outputs = zone
        .outputs
        .as_ref()
        .and_then(|o| o.downcast_ref::<CustomResourceOutputs>())
        .expect("custom outputs");
    assert!(outputs.fields()["zoneId"].is_string());
    assert_eq!(
        zone.internal_state.as_ref().unwrap()["type"],
        PROXY_CONTROLLER_TYPE
    );
    assert_eq!(state.resources["api"].status, ResourceStatus::Running);
    assert!(
        custom_heartbeats >= 1,
        "the zone's Ready step reported a heartbeat"
    );
}

/// The credentials of the cloud the step's context reaches: what a source that mints from the
/// deployment's own cloud identity starts from.
#[derive(Debug)]
struct TheContextsCloud;

#[async_trait]
impl CredentialSource for TheContextsCloud {
    async fn credentials(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> alien_infra::Result<Credentials> {
        let ClientConfig::Aws(aws) = &ctx.client_config else {
            panic!(
                "the step's context reaches {:?}",
                ctx.client_config.platform()
            );
        };
        let alien_core::AwsCredentials::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        } = &aws.credentials
        else {
            panic!("the mock AWS config holds access keys");
        };
        Ok(Credentials::Aws {
            account_id: aws.account_id.clone(),
            region: aws.region.clone(),
            access_key_id: access_key_id.clone(),
            secret_access_key: secret_access_key.clone(),
            session_token: session_token.clone(),
            expires_at: None,
        })
    }
}

/// On an EKS cluster a Live custom resource steps on its extension's AWS controller, with AWS
/// credentials: the controller calls AWS, and the extension registers none for Kubernetes.
#[tokio::test(flavor = "multi_thread")]
async fn a_live_custom_resource_on_eks_steps_on_the_aws_controller_with_aws_credentials() {
    let mut fixture = stdio_fixture("eks", Duration::from_secs(10));
    fixture.credentials = Some(Arc::new(TheContextsCloud));
    let mut config = deployment_config();
    config.base_platform = Some(Platform::Aws);
    let eks = ClientConfig::KubernetesCloud {
        kubernetes: Box::new(alien_core::KubernetesClientConfig::Manual {
            server_url: "https://kubernetes.test".to_string(),
            certificate_authority_data: None,
            insecure_skip_tls_verify: Some(true),
            client_certificate_data: None,
            client_key_data: None,
            token: None,
            username: None,
            password: None,
            namespace: Some("app".to_string()),
            additional_headers: Default::default(),
        }),
        cloud: Box::new(ClientConfig::Aws(Box::new(AwsClientConfig::mock()))),
    };
    let stack = stack("a.example");
    let executor = StackExecutor::builder(&stack, eks)
        .deployment_config(&config)
        .resource_registry(registry_with(fixture.factory(), Platform::Aws))
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .build()
        .unwrap_or_else(|e| panic!("the executor refused the zone: {} ({})", e.message, e.code));

    let state = step_until(
        &executor,
        StackState::new(Platform::Kubernetes),
        ResourceStatus::Running,
        4,
    )
    .await;
    assert_eq!(zone(&state).controller_platform, Some(Platform::Aws));
    let requests = fixture.requests();
    assert!(!requests.is_empty());
    for request in &requests {
        assert_eq!(request["platform"], "aws", "{request}");
        assert_eq!(request["credentials"]["platform"], "aws", "{request}");
        assert_eq!(request["credentials"]["accountId"], "123456789012");
        assert_eq!(
            request["credentials"]["accessKeyId"],
            "AKIAIOSFODNN7EXAMPLE"
        );
    }
}
