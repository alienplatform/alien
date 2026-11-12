//! Edge cases of the host side of the step protocol: replies a controller must not be able to
//! slip past the proxy, and what never leaves the host.

use alien_aws_clients::AwsClientConfigExt;
use alien_core::{
    AwsClientConfig, ClientConfig, DeploymentConfig, EnvironmentVariable, EnvironmentVariableType,
    EnvironmentVariablesSnapshot, ExternalBindings, OtlpConfig, Platform, ResourceStatus,
    ResourceType, Stack, StackSettings, StackState,
};
use alien_infra::step_protocol::protocol::{
    codes, Credentials, StepRequest, StepResponse, Transition,
};
use alien_infra::step_protocol::test_controller::DNS_ZONE;
use alien_infra::step_protocol::{
    deployment_config_for_extension, ProxyFactory, StdioTransport, Transport, TransportFailure,
    DELETE_TIMEOUT,
};
use alien_infra::{MockPlatformServiceProvider, ResourceRegistry, StackExecutor};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

type Reply = Box<dyn Fn(&StepRequest) -> Value + Send + Sync>;

const DEPLOYMENT_ID: &str = "dep_edges";

/// Answers every step with the JSON the closure builds from the request.
struct Canned(Reply);

impl Canned {
    fn new(reply: impl Fn(&StepRequest) -> Value + Send + Sync + 'static) -> Self {
        Self(Box::new(reply))
    }
}

impl std::fmt::Debug for Canned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Canned")
    }
}

#[async_trait]
impl Transport for Canned {
    async fn step(
        &self,
        request: &StepRequest,
        _timeout: Duration,
    ) -> Result<StepResponse, TransportFailure> {
        serde_json::from_value((self.0)(request)).map_err(|e| TransportFailure {
            code: codes::PROTOCOL_ERROR,
            message: e.to_string(),
            retryable: false,
        })
    }
}

fn view(outputs: Value) -> Value {
    json!({
        "state": { "state": "ready" }, "status": "running", "progress": null,
        "outputs": outputs, "bindingParams": null, "onFailureStatus": "refresh-failed",
        "updateStart": { "outcome": "allowed", "status": "updating" },
        "deleteStart": { "outcome": "allowed", "status": "deleting" },
        "teardownStart": { "outcome": "allowed", "status": "deleting" }
    })
}

fn stack() -> Stack {
    serde_json::from_value(json!({
        "id": "acme",
        "resources": { "zone": {
            "config": { "type": DNS_ZONE, "id": "zone", "domain": "a.example" },
            "lifecycle": "live", "dependencies": []
        } }
    }))
    .unwrap()
}

fn config() -> DeploymentConfig {
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

async fn first_step(transport: Arc<dyn Transport>) -> alien_core::StackResourceState {
    first_step_result(transport).await.next_state.resources["zone"].clone()
}

async fn first_step_result(transport: Arc<dyn Transport>) -> alien_infra::StepResult {
    let mut registry = ResourceRegistry::with_built_ins();
    registry.register_controller_factory(
        ResourceType::from(DNS_ZONE),
        Platform::Aws,
        Box::new(ProxyFactory::new(DEPLOYMENT_ID, transport, None)),
    );
    let stack = stack();
    let config = config();
    let executor =
        StackExecutor::builder(&stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
            .deployment_config(&config)
            .resource_registry(Arc::new(registry))
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .unwrap();
    executor.step(StackState::new(Platform::Aws)).await.unwrap()
}

/// Alien acts on its own codes, so a controller's failed step keeps only those it may answer
/// with; one that names a failure of the host, such as an unreachable controller, is the
/// controller's own failure.
#[tokio::test]
async fn a_failed_step_keeps_only_the_codes_a_controller_may_answer_with() {
    let failed = |code: &'static str, retryable: bool| {
        Arc::new(Canned::new(move |request| {
            json!({ "protocol": 1, "stepId": request.step_id, "result": "failed",
                "controller": view(Value::Null),
                "error": { "code": code, "message": "m", "retryable": retryable } })
        }))
    };
    for (code, retryable, expected, expected_retryable) in [
        (codes::UNREACHABLE, true, "EXTENSION_STEP_FAILED", true),
        (codes::AUTH_REJECTED, false, "EXTENSION_STEP_FAILED", false),
        (codes::PROTOCOL_ERROR, false, "EXTENSION_STEP_FAILED", false),
        // A resource already gone ends only a delete.
        (
            "REMOTE_RESOURCE_NOT_FOUND",
            false,
            "EXTENSION_STEP_FAILED",
            false,
        ),
        (codes::STEP_DEADLINE, true, codes::STEP_DEADLINE, true),
        (codes::STEP_REFUSED, true, codes::STEP_REFUSED, false),
    ] {
        let zone = first_step(failed(code, retryable)).await;
        let error = zone.error.expect("an error");
        assert_eq!(error.code, expected, "{code}: {}", error.message);
        assert_eq!(error.retryable, expected_retryable, "{code}");
        if expected != code {
            assert!(error.message.contains(code), "{code}: {}", error.message);
        }
    }

    // A dependency that is not ready makes the step wait, as Alien's own controllers do.
    let zone = first_step(failed("DEPENDENCY_NOT_READY", true)).await;
    assert!(zone.error.is_none(), "{:?}", zone.error);
}

/// A delete is the customer's to start, even when an extension's controller refuses it: the
/// controller's refusal fails its own resource, DeleteFailed with the controller's message, and
/// the rest of the stack still deletes. A resource the controller finds already gone is deleted.
#[tokio::test]
async fn a_controller_that_refuses_a_delete_fails_only_its_own_resource() {
    use alien_core::{ResourceLifecycle, StackResourceState};
    use alien_infra::serialize_controller;

    let transport = Arc::new(Canned::new(|request| {
        let deleting = request.transitions.contains(&Transition::DeleteStart);
        match request.resource_id.as_str() {
            "held" if deleting => json!({ "protocol": 1, "stepId": request.step_id,
                "result": "refused",
                "error": { "code": "ZONE_IN_USE", "message": "the vendor says no", "retryable": false } }),
            // Already gone from the cloud: the delete ends there.
            "vanished" if deleting => json!({ "protocol": 1, "stepId": request.step_id,
                "result": "failed", "controller": view(Value::Null),
                "error": { "code": "REMOTE_RESOURCE_NOT_FOUND", "message": "no such zone", "retryable": false } }),
            _ => {
                let mut deleted = view(Value::Null);
                deleted["status"] = json!("deleted");
                json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
                    "controller": deleted, "suggestedDelayMs": null, "heartbeats": [] })
            }
        }
    }));
    let mut registry = ResourceRegistry::with_built_ins();
    registry.register_controller_factory(
        ResourceType::from(DNS_ZONE),
        Platform::Aws,
        Box::new(ProxyFactory::new(DEPLOYMENT_ID, transport, None)),
    );
    let empty: Stack = serde_json::from_value(json!({ "id": "acme", "resources": {} })).unwrap();
    let config = config();
    let executor =
        StackExecutor::builder(&empty, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
            .deployment_config(&config)
            .resource_registry(Arc::new(registry))
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .unwrap();

    // Two running zones the stack no longer lists, each with a controller that has replied.
    let mut ready_view = view(Value::Null);
    ready_view["deleteStart"] = json!({ "outcome": "refused", "message": "the vendor says no" });
    let proxy: alien_infra::step_protocol::ExtensionProxyController =
        serde_json::from_value(json!({
            "status": "running", "remote": ready_view, "pending": [],
            "inFlightStepId": null, "pendingImport": null
        }))
        .unwrap();
    let mut state = StackState::new(Platform::Aws);
    for id in ["held", "gone", "vanished"] {
        let config: alien_core::Resource = serde_json::from_value(
            json!({ "type": DNS_ZONE, "id": id, "domain": format!("{id}.example") }),
        )
        .unwrap();
        let mut zone = StackResourceState::builder()
            .resource_type(DNS_ZONE.to_string())
            .status(ResourceStatus::Running)
            .config(config)
            .lifecycle(ResourceLifecycle::Live)
            .build();
        zone.internal_state = Some(serialize_controller(&proxy).unwrap());
        zone.controller_platform = Some(Platform::Aws);
        state.resources.insert(id.to_string(), zone);
    }

    for _ in 0..5 {
        state = executor.step(state).await.unwrap().next_state;
    }

    assert_eq!(state.resources["gone"].status, ResourceStatus::Deleted);
    assert_eq!(
        state.resources["vanished"].status,
        ResourceStatus::Deleted,
        "{:?}",
        state.resources["vanished"].error
    );
    let held = &state.resources["held"];
    assert_eq!(held.status, ResourceStatus::DeleteFailed);
    let error = held.error.as_ref().expect("the refusal");
    assert_eq!(error.code, codes::STEP_REFUSED);
    assert!(
        error.message.contains("the vendor says no"),
        "{}",
        error.message
    );
}

/// The executor for `stack` with the extension's controller reached through `transport`.
fn executor_with(
    stack: &Stack,
    config: &DeploymentConfig,
    platform: Platform,
    transport: Arc<dyn Transport>,
) -> StackExecutor {
    let mut registry = ResourceRegistry::with_built_ins();
    registry.register_controller_factory(
        ResourceType::from(DNS_ZONE),
        platform,
        Box::new(ProxyFactory::new(DEPLOYMENT_ID, transport, None)),
    );
    let client_config = match platform {
        Platform::Test => ClientConfig::Test,
        _ => ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
    };
    StackExecutor::builder(stack, client_config)
        .deployment_config(config)
        .resource_registry(Arc::new(registry))
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .build()
        .unwrap()
}

/// A running zone on `platform` whose controller has replied, with a delete it will refuse.
fn running_zone(
    id: &str,
    platform: Platform,
    dependencies: Vec<alien_core::ResourceRef>,
) -> alien_core::StackResourceState {
    let mut ready_view = view(Value::Null);
    ready_view["deleteStart"] = json!({ "outcome": "refused", "message": "the vendor says no" });
    let proxy: alien_infra::step_protocol::ExtensionProxyController =
        serde_json::from_value(json!({
            "status": "running", "remote": ready_view, "pending": [],
            "inFlightStepId": null, "pendingImport": null
        }))
        .unwrap();
    let config: alien_core::Resource = serde_json::from_value(
        json!({ "type": DNS_ZONE, "id": id, "domain": format!("{id}.example") }),
    )
    .unwrap();
    let mut zone = alien_core::StackResourceState::builder()
        .resource_type(DNS_ZONE.to_string())
        .status(ResourceStatus::Running)
        .config(config)
        .lifecycle(alien_core::ResourceLifecycle::Live)
        .dependencies(dependencies)
        .build();
    zone.internal_state = Some(alien_infra::serialize_controller(&proxy).unwrap());
    zone.controller_platform = Some(platform);
    zone
}

/// A delete has no dependency to wait for, so a controller that answers every delete step that
/// a dependency is not ready spends the retry budget, and its resource ends DeleteFailed instead
/// of holding the delete forever.
#[tokio::test]
async fn a_delete_waiting_on_a_dependency_ends_failed() {
    let transport = Arc::new(Canned::new(|request| {
        let mut deleting = view(Value::Null);
        deleting["status"] = json!("deleting");
        deleting["onFailureStatus"] = json!("delete-failed");
        json!({ "protocol": 1, "stepId": request.step_id, "result": "failed",
            "controller": deleting,
            "error": { "code": "DEPENDENCY_NOT_READY", "message": "waiting", "retryable": true } })
    }));
    let empty: Stack = serde_json::from_value(json!({ "id": "acme", "resources": {} })).unwrap();
    let config = config();
    let executor = executor_with(&empty, &config, Platform::Aws, transport);
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert(
        "zone".to_string(),
        running_zone("zone", Platform::Aws, Vec::new()),
    );

    for _ in 0..15 {
        state = executor.step(state).await.unwrap().next_state;
    }

    let zone = &state.resources["zone"];
    assert_eq!(
        zone.status,
        ResourceStatus::DeleteFailed,
        "{:?}",
        zone.error
    );
    let error = zone.error.as_ref().expect("the controller's failure");
    assert!(
        error.message.contains("DEPENDENCY_NOT_READY"),
        "{}",
        error.message
    );
}

/// A resource that failed to delete still depends on what it depends on, and Alien deletes a
/// resource only after everything that depends on it is gone. So a controller's refused delete
/// keeps the resources its resource depends on, Alien's own among them, until the refused
/// resource is deleted.
#[cfg(feature = "test")]
#[tokio::test]
async fn a_refused_delete_keeps_what_its_resource_depends_on() {
    use alien_core::{ResourceLifecycle, ResourceRef, Storage};

    let config = config();
    let deployed = Stack::new("acme".to_string())
        .add(
            Storage::new("bucket".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .build();
    let deploy = StackExecutor::builder(&deployed, ClientConfig::Test)
        .deployment_config(&config)
        .build()
        .unwrap();
    let mut state = StackState::new(Platform::Test);
    for _ in 0..10 {
        state = deploy.step(state).await.unwrap().next_state;
    }
    assert_eq!(state.resources["bucket"].status, ResourceStatus::Running);
    state.resources.insert(
        "held".to_string(),
        running_zone(
            "held",
            Platform::Test,
            vec![ResourceRef::new(Storage::RESOURCE_TYPE, "bucket")],
        ),
    );

    let transport = Arc::new(Canned::new(|request| {
        json!({ "protocol": 1, "stepId": request.step_id, "result": "refused",
            "error": { "code": "ZONE_IN_USE", "message": "the vendor says no", "retryable": false } })
    }));
    let empty: Stack = serde_json::from_value(json!({ "id": "acme", "resources": {} })).unwrap();
    let executor = executor_with(&empty, &config, Platform::Test, transport);
    for _ in 0..10 {
        state = executor.step(state).await.unwrap().next_state;
    }

    assert_eq!(state.resources["held"].status, ResourceStatus::DeleteFailed);
    let bucket = &state.resources["bucket"];
    assert_eq!(bucket.status, ResourceStatus::Running, "{:?}", bucket.error);
}

/// Deletes two zones the stack no longer lists: `stuck`, whose controller answers every delete
/// step with a `result` reply whose view has `status`, and `normal`, whose controller deletes it
/// at once. Both ask to wait a day between steps. Steps until the delete's deadline has passed,
/// and returns the executor, the final state and the longest wait any step asked for.
async fn delete_past_the_deadline(
    result: &'static str,
    status: &'static str,
) -> (StackExecutor, StackState, u64) {
    let transport = Arc::new(Canned::new(move |request| {
        let mut controller = view(Value::Null);
        if request.resource_id == "normal" {
            controller["status"] = json!("deleted");
            return json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
                "controller": controller, "suggestedDelayMs": 86_400_000, "heartbeats": [] });
        }
        controller["status"] = json!(status);
        json!({ "protocol": 1, "stepId": request.step_id, "result": result,
            "controller": controller, "suggestedDelayMs": 86_400_000, "heartbeats": [],
            "error": { "code": "THROTTLED", "message": "try again", "retryable": true } })
    }));
    let empty: Stack = serde_json::from_value(json!({ "id": "acme", "resources": {} })).unwrap();
    let config = config();
    let executor = executor_with(&empty, &config, Platform::Aws, transport);
    let mut state = StackState::new(Platform::Aws);
    for id in ["stuck", "normal"] {
        let mut zone = running_zone(id, Platform::Aws, Vec::new());
        zone.internal_state.as_mut().unwrap()["remote"]["deleteStart"] =
            json!({ "outcome": "allowed", "status": "deleting" });
        state.resources.insert(id.to_string(), zone);
    }
    let mut longest_wait = 0;
    let mut step = |result: alien_infra::StepResult| {
        longest_wait = longest_wait.max(result.suggested_delay_ms.unwrap_or_default());
        result.next_state
    };

    for _ in 0..5 {
        state = step(executor.step(state).await.unwrap());
    }
    assert_eq!(state.resources["normal"].status, ResourceStatus::Deleted);
    let stuck = &state.resources["stuck"];
    assert_eq!(
        stuck.status,
        ResourceStatus::Deleting,
        "inside the deadline the delete goes on: {:?}",
        stuck.error
    );

    let saved = state
        .resources
        .get_mut("stuck")
        .unwrap()
        .internal_state
        .as_mut()
        .unwrap();
    saved["deleteStartedAt"] = json!((chrono::Utc::now() - DELETE_TIMEOUT).to_rfc3339());
    for _ in 0..3 {
        state = step(executor.step(state).await.unwrap());
    }
    (executor, state, longest_wait)
}

fn assert_delete_failed_at_the_deadline(state: &StackState, longest_wait: u64) {
    let stuck = &state.resources["stuck"];
    assert_eq!(
        stuck.status,
        ResourceStatus::DeleteFailed,
        "{:?}",
        stuck.error
    );
    let error = stuck.error.as_ref().expect("the deadline's failure");
    assert!(
        error
            .message
            .contains("did not finish deleting the resource within 30 minutes"),
        "{}",
        error.message
    );
    assert!(
        longest_wait <= DELETE_TIMEOUT.num_milliseconds() as u64,
        "a step waited {longest_wait} ms, past the delete's deadline"
    );
    assert_eq!(state.resources["normal"].status, ResourceStatus::Deleted);
}

/// A controller that answers every delete step that it is still deleting can't hold the
/// customer's delete forever: past the deadline its resource ends DeleteFailed, even though the
/// view names another failure status. A retry gives the delete a fresh deadline.
#[tokio::test]
async fn a_delete_the_controller_never_finishes_ends_failed_at_the_deadline() {
    use alien_infra::StackResourceStateExt;

    let (executor, mut state, longest_wait) = delete_past_the_deadline("stepped", "deleting").await;
    assert_delete_failed_at_the_deadline(&state, longest_wait);

    state
        .resources
        .get_mut("stuck")
        .unwrap()
        .retry_failed()
        .unwrap();
    state = executor.step(state).await.unwrap().next_state;
    let stuck = &state.resources["stuck"];
    assert_eq!(stuck.status, ResourceStatus::Deleting, "{:?}", stuck.error);
    assert!(stuck.error.is_none(), "{:?}", stuck.error);
}

/// A reply naming another status during a delete, whether the executor would start the delete
/// again (`running`) or wait on it (`refresh-failed`), doesn't end the delete or its deadline.
#[tokio::test]
async fn a_controller_answering_a_delete_with_another_status_ends_failed_at_the_deadline() {
    for (result, status) in [
        ("stepped", "running"),
        ("stepped", "refresh-failed"),
        ("failed", "refresh-failed"),
    ] {
        let (_, state, longest_wait) = delete_past_the_deadline(result, status).await;
        assert_delete_failed_at_the_deadline(&state, longest_wait);
    }
}

/// A running resource that asks to be stepped again at once would keep the whole deployment
/// polling, so it waits at least as long as Alien's own running resources.
#[tokio::test]
async fn a_running_resource_waits_at_least_the_ready_delay() {
    for delay in [Value::Null, json!(0), json!(5)] {
        let suggested = delay.clone();
        let result = first_step_result(Arc::new(Canned::new(move |request| {
            json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
                "controller": view(json!({ "type": DNS_ZONE, "zoneId": "Z1", "domain": "a.example" })),
                "suggestedDelayMs": suggested, "heartbeats": [] })
        })))
        .await;
        assert_eq!(
            result.next_state.resources["zone"].status,
            ResourceStatus::Running
        );
        assert!(
            result.suggested_delay_ms.is_some_and(|ms| ms >= 30_000),
            "{delay}: {:?}",
            result.suggested_delay_ms
        );
    }
}

/// A reply to another step can't be taken for this one's.
#[tokio::test]
async fn a_reply_naming_another_step_fails_the_resource() {
    let reply = json!({ "protocol": 1, "stepId": "not-this-step", "result": "stepped",
        "controller": view(Value::Null), "suggestedDelayMs": null, "heartbeats": [] });
    let script = format!("while read line; do echo '{reply}'; done");
    let transport = Arc::new(StdioTransport::new(
        "/bin/sh",
        vec!["-c".into(), script],
        Vec::new(),
    ));
    let zone = first_step(transport).await;
    let error = zone.error.expect("an error");
    assert_eq!(error.code, codes::PROTOCOL_ERROR, "{}", error.message);
    assert!(!error.retryable);
}

/// A controller can't publish outputs of another type, which a linked workload would read as
/// Alien's own.
#[tokio::test]
async fn outputs_of_another_type_fail_the_resource() {
    let zone = first_step(Arc::new(Canned::new(|request| {
        json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
            "controller": view(json!({ "type": "storage", "bucketName": "someone-elses" })),
            "suggestedDelayMs": null, "heartbeats": [] })
    })))
    .await;
    let error = zone.error.expect("an error");
    assert_eq!(error.code, codes::PROTOCOL_ERROR, "{}", error.message);
    assert!(!error.retryable);
    assert!(zone.outputs.is_none(), "{:?}", zone.outputs);
}

/// The step protocol reference marks only `protocol`, `stepId` and `result` required in a reply,
/// so a controller in another language may leave out `heartbeats` when it has none.
#[tokio::test]
async fn a_stepped_reply_without_heartbeats_is_a_step() {
    let zone = first_step(Arc::new(Canned::new(|request| {
        json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
            "controller": view(json!({ "type": DNS_ZONE, "zoneId": "Z1", "domain": "a.example" })),
            "suggestedDelayMs": null })
    })))
    .await;
    assert!(
        zone.error.is_none(),
        "{:?}",
        zone.error.as_ref().map(|e| (&e.code, &e.message))
    );
    assert_eq!(zone.status, ResourceStatus::Running);
}

#[test]
fn credentials_debug_shows_no_secret() {
    let aws = Credentials::Aws {
        account_id: "123456789012".into(),
        region: "us-east-1".into(),
        access_key_id: "ASIAKEYID".into(),
        secret_access_key: "SECRETACCESSKEY".into(),
        session_token: Some("SESSIONTOKEN".into()),
        expires_at: None,
    };
    let gcp = Credentials::Gcp {
        project_id: "p".into(),
        region: "r".into(),
        access_token: "GCPTOKEN".into(),
        expires_at: "x".into(),
    };
    let azure = Credentials::Azure {
        subscription_id: "s".into(),
        tenant_id: "t".into(),
        access_token: "AZURETOKEN".into(),
        expires_at: "x".into(),
    };
    let printed = format!("{aws:?} {gcp:?} {azure:?}");
    for secret in [
        "ASIAKEYID",
        "SECRETACCESSKEY",
        "SESSIONTOKEN",
        "GCPTOKEN",
        "AZURETOKEN",
    ] {
        assert!(!printed.contains(secret), "{secret} in {printed}");
    }
}

#[test]
fn the_log_export_auth_header_never_reaches_a_controller() {
    let mut config = config();
    config.environment_variables.variables = vec![EnvironmentVariable {
        name: "S".into(),
        value: "SECRETVALUE".into(),
        var_type: EnvironmentVariableType::Secret,
        target_resources: None,
    }];
    config.monitoring = Some(OtlpConfig {
        logs_endpoint: "https://manager.example/v1/logs".into(),
        logs_auth_header: "authorization=Bearer WRITETOKEN".into(),
        metrics_endpoint: None,
        metrics_auth_header: None,
        resource_attributes: Default::default(),
    });
    config.deployment_token = Some("REGISTRYTOKEN".into());
    let sent = serde_json::to_string(&deployment_config_for_extension(&config)).unwrap();
    for secret in ["SECRETVALUE", "WRITETOKEN", "REGISTRYTOKEN"] {
        assert!(!sent.contains(secret), "{secret} in {sent}");
    }
}

/// A controller process that never reads its stdin must still fail the step at the deadline,
/// even when the request is larger than the pipe's buffer.
#[tokio::test(flavor = "multi_thread")]
async fn a_process_that_never_reads_stdin_still_misses_its_deadline() {
    let transport: Arc<StdioTransport> = Arc::new(StdioTransport::new(
        "/bin/sleep",
        vec!["60".into()],
        Vec::new(),
    ));
    let mut request: StepRequest = serde_json::from_value(json!({
        "protocol": 1, "stepId": "s1", "deploymentId": "dep", "resourceId": "zone",
        "resourceType": DNS_ZONE, "platform": "aws", "resourcePrefix": "p",
        "initialSetupAuthority": "importedHandoff", "lifecycle": null,
        "resource": { "type": DNS_ZONE, "id": "zone", "domain": "a.example" },
        "previousResource": null, "dependencies": {}, "deploymentConfig": {},
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": null, "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap();
    request.deployment_config = json!({ "big": "x".repeat(1 << 20) });
    let started = std::time::Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        transport.step(&request, Duration::from_secs(3)),
    )
    .await;
    match outcome {
        Err(_) => panic!(
            "the transport was still blocked after {:?}, past its 3 s timeout",
            started.elapsed()
        ),
        Ok(result) => {
            let failure = result.expect_err("sleep never answers");
            assert_eq!(failure.code, codes::UNREACHABLE);
        }
    }
}

/// What a controller started outlives it unless its process group is killed, even after the
/// controller itself exited.
#[cfg(unix)]
#[tokio::test]
async fn what_an_exited_controller_started_is_killed_with_it() {
    let dir = tempfile::TempDir::new().unwrap();
    let pid_file = dir.path().join("child.pid");
    let script = format!("sleep 60 & echo $! > {}; exit 0", pid_file.display());
    let transport = StdioTransport::new("/bin/sh", vec!["-c".into(), script], Vec::new());
    let request: StepRequest = serde_json::from_value(json!({
        "protocol": 1, "stepId": "s1", "deploymentId": "dep", "resourceId": "zone",
        "resourceType": DNS_ZONE, "platform": "aws", "resourcePrefix": "p",
        "initialSetupAuthority": "importedHandoff", "lifecycle": null,
        "resource": { "type": DNS_ZONE, "id": "zone", "domain": "a.example" },
        "previousResource": null, "dependencies": {}, "deploymentConfig": {},
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": null, "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap();
    let failure = transport
        .step(&request, Duration::from_secs(5))
        .await
        .expect_err("the controller exits without answering");
    assert_eq!(failure.code, codes::UNREACHABLE, "{}", failure.message);
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let mut alive = true;
    for _ in 0..50 {
        // SAFETY: signal 0 only asks whether the process exists.
        alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!alive, "the controller's child {pid} still runs");
}

/// The controller's deadline is the step's timeout less the host's grace, so a timeout that
/// leaves nothing is refused rather than sent with a deadline already past.
#[tokio::test]
async fn a_timeout_within_the_host_grace_is_refused() {
    let transport = StdioTransport::new("/bin/cat", Vec::new(), Vec::new());
    let request: StepRequest = serde_json::from_value(json!({
        "protocol": 1, "stepId": "s1", "deploymentId": "dep", "resourceId": "zone",
        "resourceType": DNS_ZONE, "platform": "aws", "resourcePrefix": "p",
        "initialSetupAuthority": "importedHandoff", "lifecycle": null,
        "resource": { "type": DNS_ZONE, "id": "zone", "domain": "a.example" },
        "previousResource": null, "dependencies": {}, "deploymentConfig": {},
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": null, "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap();
    let failure = transport
        .step(&request, alien_infra::step_protocol::HOST_GRACE)
        .await
        .expect_err("no time is left for the controller");
    assert_eq!(failure.code, codes::PROTOCOL_ERROR, "{}", failure.message);
}

fn refused(supported_protocols: Option<Vec<u32>>) -> Canned {
    Canned::new(move |request| {
        let mut reply = json!({ "protocol": 1, "stepId": request.step_id, "result": "refused",
            "error": { "code": "ACME_BAD_STATE", "message": "the zone's state is unreadable",
                "retryable": false } });
        if let Some(supported) = &supported_protocols {
            reply["supportedProtocols"] = json!(supported);
        }
        reply
    })
}

/// Only a controller that lists the versions it speaks, without this one, is refused for the
/// protocol. Any other refusal is the controller's own, with its error.
#[tokio::test]
async fn a_refusal_is_for_the_protocol_only_when_the_controller_lacks_this_version() {
    let zone = first_step(Arc::new(refused(Some(vec![2, 3])))).await;
    let error = zone.error.expect("an error");
    assert_eq!(error.code, codes::PROTOCOL_UNSUPPORTED, "{}", error.message);
    assert!(error.message.contains("[2, 3]"), "{}", error.message);
    assert!(!error.retryable);

    for supported in [None, Some(vec![1])] {
        let zone = first_step(Arc::new(refused(supported.clone()))).await;
        let error = zone.error.expect("an error");
        assert_eq!(
            error.code,
            codes::STEP_REFUSED,
            "{supported:?}: {}",
            error.message
        );
        assert!(
            error.message.contains("ACME_BAD_STATE"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("the zone's state is unreadable"),
            "{}",
            error.message
        );
        assert!(!error.retryable);
    }
}

fn heartbeat(resource_id: &str, resource_type: &str, deployment_id: Option<&str>) -> Value {
    json!({
        "deploymentId": deployment_id,
        "resourceId": resource_id,
        "resourceType": resource_type,
        "controllerPlatform": "aws",
        "backend": "aws",
        "observedAt": "2026-11-07T00:00:00Z",
        "data": {
            "resourceType": resource_type,
            "data": { "status": {
                "health": "healthy", "lifecycle": "running", "message": null,
                "stale": false, "partial": false, "collectionIssues": []
            } }
        },
        "raw": []
    })
}

fn stepped_with(heartbeat: Value) -> Canned {
    Canned::new(move |request| {
        json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
            "controller": view(json!({ "type": DNS_ZONE, "zoneId": "Z1", "domain": "a.example" })),
            "suggestedDelayMs": null, "heartbeats": [heartbeat.clone()] })
    })
}

/// A controller reports health only for the resource it steps, so it can't make the dashboard
/// show another resource, type or deployment as healthy.
#[tokio::test]
async fn a_heartbeat_for_anything_but_the_stepped_resource_fails_the_step() {
    for (what, foreign) in [
        ("resource", heartbeat("api", DNS_ZONE, None)),
        ("type", heartbeat("zone", "acme/other", None)),
        (
            "deployment",
            heartbeat("zone", DNS_ZONE, Some("dep_someone_else")),
        ),
    ] {
        let zone = first_step(Arc::new(stepped_with(foreign))).await;
        let error = zone.error.expect("an error");
        assert_eq!(
            error.code,
            codes::PROTOCOL_ERROR,
            "{what}: {}",
            error.message
        );
        assert!(!error.retryable, "{what}");
        assert!(zone.outputs.is_none(), "{what}: the reply was not adopted");
    }

    for own in [
        heartbeat("zone", DNS_ZONE, None),
        heartbeat("zone", DNS_ZONE, Some(DEPLOYMENT_ID)),
    ] {
        let zone = first_step(Arc::new(stepped_with(own))).await;
        assert!(zone.error.is_none(), "{:?}", zone.error.map(|e| e.message));
        assert_eq!(zone.status, ResourceStatus::Running);
    }
}

/// A failed step that loaded nothing keeps the importer's fields, which the next step carries
/// again, and the outputs the resource had.
#[tokio::test]
async fn a_failed_first_step_keeps_the_import_for_the_next() {
    use alien_core::{ResourceLifecycle, StackResourceState};
    use alien_infra::serialize_controller;
    use alien_infra::step_protocol::ExtensionProxyController;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let fields = json!({ "state": "ready", "zoneId": "ZSETUP", "domain": "a.example" });
    let steps = Arc::new(AtomicUsize::new(0));
    let seen = steps.clone();
    let expected = fields.clone();
    let transport = Arc::new(Canned::new(move |request| {
        let carried = serde_json::to_value(request).unwrap()["pendingImport"].clone();
        assert_eq!(
            carried, expected,
            "every step until a load carries the import"
        );
        if seen.fetch_add(1, Ordering::SeqCst) == 0 {
            let mut failed = view(Value::Null);
            failed["state"] = Value::Null;
            return json!({ "protocol": 1, "stepId": request.step_id, "result": "failed",
                "controller": failed,
                "error": { "code": "UNREACHABLE_API", "message": "try again", "retryable": true } });
        }
        json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
            "controller": view(json!({ "type": DNS_ZONE, "zoneId": "ZSETUP", "domain": "a.example" })),
            "suggestedDelayMs": 1000, "heartbeats": [] })
    }));

    let mut registry = ResourceRegistry::with_built_ins();
    registry.register_controller_factory(
        ResourceType::from(DNS_ZONE),
        Platform::Aws,
        Box::new(ProxyFactory::new(DEPLOYMENT_ID, transport, None)),
    );
    let stack = stack();
    let config = config();
    let executor =
        StackExecutor::builder(&stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
            .deployment_config(&config)
            .resource_registry(Arc::new(registry))
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .unwrap();
    let mut imported = StackResourceState::builder()
        .resource_type(DNS_ZONE.to_string())
        .status(ResourceStatus::Running)
        .config(stack.resources["zone"].config.clone())
        .lifecycle(ResourceLifecycle::Live)
        .build();
    let outputs: alien_core::ResourceOutputs = serde_json::from_value(
        json!({ "type": DNS_ZONE, "zoneId": "ZSETUP", "domain": "a.example" }),
    )
    .unwrap();
    imported.outputs = Some(outputs.clone());
    imported.internal_state = Some(
        serialize_controller(&ExtensionProxyController::imported(
            fields,
            ResourceStatus::Running,
            Some(outputs),
        ))
        .unwrap(),
    );
    imported.controller_platform = Some(Platform::Aws);
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert("zone".to_string(), imported);

    let failed = executor.step(state).await.unwrap().next_state;
    assert!(failed.resources["zone"].error.is_some());
    assert_eq!(
        serde_json::to_value(failed.resources["zone"].outputs.as_ref().expect("kept")).unwrap()
            ["zoneId"],
        "ZSETUP",
        "a failed step keeps the importer's outputs"
    );
    let loaded = executor.step(failed).await.unwrap().next_state;
    assert_eq!(steps.load(Ordering::SeqCst), 2);
    let zone = &loaded.resources["zone"];
    assert!(zone.error.is_none(), "{:?}", zone.error);
    assert_eq!(
        serde_json::to_value(zone.outputs.as_ref().unwrap()).unwrap()["zoneId"],
        "ZSETUP"
    );
}
