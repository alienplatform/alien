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

/// What a controller writes reaches the stored state cut short and on one line: its refusal, its
/// failure and its progress lose their control characters and all but the first kilobyte.
#[tokio::test]
async fn a_controller_s_text_is_stored_short_and_on_one_line() {
    let long = format!("first\nsecond\r\u{1b}[2Jthird {}", "x".repeat(8000));
    let code = "BAD\nCODE\u{1b}[31m";
    let clean = |text: &str| {
        assert!(
            !text.chars().any(|c| c.is_control() && c != '\t'),
            "{text:?}"
        );
        assert!(text.len() < 3 * 1024, "{} bytes", text.len());
        assert!(text.contains("more bytes"), "{text}");
    };

    let (long_failure, long_refusal) = (long.clone(), long.clone());
    let failed = first_step(Arc::new(Canned::new(move |request| {
        json!({ "protocol": 1, "stepId": request.step_id, "result": "failed",
            "controller": view(Value::Null),
            "error": { "code": code, "message": long_failure, "retryable": false } })
    })))
    .await;
    let refused = first_step(Arc::new(Canned::new(move |request| {
        json!({ "protocol": 1, "stepId": request.step_id, "result": "refused",
            "error": { "code": code, "message": long_refusal, "retryable": false } })
    })))
    .await;
    for zone in [failed, refused] {
        let error = zone.error.expect("an error");
        clean(&error.message);
        assert!(error.message.contains("BAD CODE"), "{}", error.message);
    }

    // A reply the host cannot read fails with a parse error that quotes it.
    let long_result = long.clone();
    let unreadable = first_step(Arc::new(Canned::new(
        move |request| json!({ "protocol": 1, "stepId": request.step_id, "result": long_result }),
    )))
    .await;
    clean(&unreadable.error.expect("an error").message);

    let stepped = first_step(Arc::new(Canned::new(move |request| {
        let mut controller = view(Value::Null);
        controller["status"] = json!("provisioning");
        controller["progress"] = json!(long);
        json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
            "controller": controller, "suggestedDelayMs": null, "heartbeats": [] })
    })))
    .await;
    clean(stepped.progress.as_deref().expect("a progress line"));
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

/// A resource that failed to refresh is stepped no sooner than a running one.
#[tokio::test]
async fn a_resource_that_failed_to_refresh_waits_at_least_the_ready_delay() {
    let result = first_step_result(Arc::new(Canned::new(|request| {
        let mut failed = view(json!({ "type": DNS_ZONE, "zoneId": "Z1", "domain": "a.example" }));
        failed["status"] = json!("refresh-failed");
        json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
            "controller": failed, "suggestedDelayMs": 0, "heartbeats": [] })
    })))
    .await;
    assert_eq!(
        result.next_state.resources["zone"].status,
        ResourceStatus::RefreshFailed
    );
    assert!(
        result.suggested_delay_ms.is_some_and(|ms| ms >= 30_000),
        "{:?}",
        result.suggested_delay_ms
    );
}

/// Only a delete leads to a deleting or deleted status: a controller that names one for a
/// resource it creates fails its step for good.
#[tokio::test]
async fn a_reply_that_the_resource_is_deleted_outside_a_delete_fails_the_step() {
    for (result, status) in ["stepped", "failed"]
        .into_iter()
        .flat_map(|result| ["deleting", "deleted", "delete-failed"].map(|status| (result, status)))
    {
        let zone = first_step(Arc::new(Canned::new(move |request| {
            let mut deleted = view(Value::Null);
            deleted["status"] = json!(status);
            json!({ "protocol": 1, "stepId": request.step_id, "result": result,
                "controller": deleted, "suggestedDelayMs": null, "heartbeats": [],
                "error": { "code": "PROBE_FAILED", "message": "m", "retryable": true } })
        })))
        .await;
        assert_ne!(zone.status, ResourceStatus::Deleted, "{result} {status}");
        let error = zone.error.expect("an error");
        assert_eq!(
            error.code,
            codes::PROTOCOL_ERROR,
            "{result} {status}: {}",
            error.message
        );
        assert!(!error.retryable, "{result} {status}");
    }
}

/// A running resource is in a delete only once the executor starts one. A controller that
/// answers a refresh with `deleting`, then `deleted`, fails the first step for good, so the
/// resource stays in the deployment's state with its controller.
#[tokio::test]
async fn a_running_resource_the_controller_says_is_deleting_fails_the_step() {
    use alien_core::{ResourceLifecycle, StackResourceState};
    use alien_infra::serialize_controller;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let steps = Arc::new(AtomicUsize::new(0));
    let seen = steps.clone();
    let transport = Arc::new(Canned::new(move |request| {
        let mut answer = view(Value::Null);
        answer["status"] = if seen.fetch_add(1, Ordering::SeqCst) == 0 {
            json!("deleting")
        } else {
            json!("deleted")
        };
        json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
            "controller": answer, "suggestedDelayMs": 1000, "heartbeats": [] })
    }));
    let stack = stack();
    let config = config();
    let executor = executor_with(&stack, &config, Platform::Aws, transport);
    let proxy: alien_infra::step_protocol::ExtensionProxyController =
        serde_json::from_value(json!({
            "status": "running", "remote": view(Value::Null), "pending": [],
            "inFlightStepId": null, "pendingImport": null
        }))
        .unwrap();
    let mut zone = StackResourceState::builder()
        .resource_type(DNS_ZONE.to_string())
        .status(ResourceStatus::Running)
        .config(stack.resources["zone"].config.clone())
        .lifecycle(ResourceLifecycle::Live)
        .build();
    zone.internal_state = Some(serialize_controller(&proxy).unwrap());
    zone.controller_platform = Some(Platform::Aws);
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert("zone".to_string(), zone);

    for _ in 0..2 {
        state = executor.step(state).await.unwrap().next_state;
    }

    let zone = state.resources.get("zone").expect("the zone stays");
    assert!(
        !matches!(
            zone.status,
            ResourceStatus::Deleting | ResourceStatus::Deleted | ResourceStatus::DeleteFailed
        ),
        "{:?}",
        zone.status
    );
    let error = zone.error.as_ref().expect("an error");
    assert_eq!(error.code, codes::PROTOCOL_ERROR, "{}", error.message);
    assert!(!error.retryable);
}

/// A failed step of a controller that has loaded reports the outputs its view names, none
/// included; only one whose controller never loaded keeps the outputs the resource had.
#[tokio::test]
async fn a_failed_step_after_a_load_reports_the_view_s_outputs() {
    use alien_core::{ResourceLifecycle, StackResourceState};
    use alien_infra::serialize_controller;

    let outputs = json!({ "type": DNS_ZONE, "zoneId": "ZOLD", "domain": "a.example" });
    let transport = Arc::new(Canned::new(|request| {
        json!({ "protocol": 1, "stepId": request.step_id, "result": "failed",
            "controller": view(Value::Null),
            "error": { "code": "UNREACHABLE_API", "message": "try again", "retryable": true } })
    }));
    let stack = stack();
    let config = config();
    let executor = executor_with(&stack, &config, Platform::Aws, transport);
    let proxy: alien_infra::step_protocol::ExtensionProxyController =
        serde_json::from_value(json!({
            "status": "running", "remote": view(outputs.clone()), "pending": [],
            "inFlightStepId": null, "pendingImport": null
        }))
        .unwrap();
    let mut zone = StackResourceState::builder()
        .resource_type(DNS_ZONE.to_string())
        .status(ResourceStatus::Running)
        .config(stack.resources["zone"].config.clone())
        .lifecycle(ResourceLifecycle::Live)
        .build();
    zone.outputs = Some(serde_json::from_value(outputs).unwrap());
    zone.internal_state = Some(serialize_controller(&proxy).unwrap());
    zone.controller_platform = Some(Platform::Aws);
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert("zone".to_string(), zone);

    let failed = executor.step(state).await.unwrap().next_state;
    let zone = &failed.resources["zone"];
    assert!(zone.error.is_some());
    assert!(zone.outputs.is_none(), "{:?}", zone.outputs);
}

/// The Operator mints a step's credentials only for the identity setup created for the
/// extension: a resource at the identity's id that is not that service account gets the
/// controller nothing, and no credential is minted for it.
#[tokio::test]
async fn credentials_are_minted_only_for_the_identity_setup_created_for_the_extension() {
    use alien_client_core::testing::capture::{self, Reply};
    use alien_core::{
        AwsCredentials, AwsServiceOverrides, ResourceLifecycle, ResourceOutputs, ServiceAccount,
        ServiceAccountOutputs, StackResourceState,
    };
    use alien_infra::step_protocol::ExtensionIdentityCredentials;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let sts = capture::start(|_| Reply::new(500, "text/plain", "no")).await;
    let caller = ClientConfig::Aws(Box::new(AwsClientConfig {
        account_id: "210987654321".to_string(),
        region: "us-east-1".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "AKIAOPERATOR".to_string(),
            secret_access_key: "operator-secret".to_string(),
            session_token: None,
        },
        service_overrides: Some(AwsServiceOverrides {
            endpoints: [("sts".to_string(), sts.base_url.clone())].into(),
        }),
    }));
    let manifest: alien_core::ExtensionManifest = serde_json::from_value(json!({
        "name": "acme", "version": "1.0.0", "digest": "sha256:0", "resourceTypes": {}
    }))
    .unwrap();
    for extension in [None, Some("other")] {
        let stepped = Arc::new(AtomicUsize::new(0));
        let seen = stepped.clone();
        let transport = Arc::new(Canned::new(move |request| {
            seen.fetch_add(1, Ordering::SeqCst);
            json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
                "controller": view(Value::Null), "suggestedDelayMs": null, "heartbeats": [] })
        }));
        let credentials = ExtensionIdentityCredentials::new(&manifest, &caller).unwrap();
        let mut registry = ResourceRegistry::with_built_ins();
        registry.register_controller_factory(
            ResourceType::from(DNS_ZONE),
            Platform::Aws,
            Box::new(ProxyFactory::new(
                DEPLOYMENT_ID,
                transport,
                Some(Arc::new(credentials)),
            )),
        );
        let stack = stack();
        let config = config();
        let executor = StackExecutor::builder(&stack, caller.clone())
            .deployment_config(&config)
            .resource_registry(Arc::new(registry))
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .unwrap();
        let account = ServiceAccount::new("acme-ext".to_string())
            .maybe_extension(extension.map(str::to_string))
            .build();
        let mut state = StackState::new(Platform::Aws);
        state.resources.insert(
            "acme-ext".to_string(),
            StackResourceState::builder()
                .resource_type(ServiceAccount::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(alien_core::Resource::new(account))
                .outputs(ResourceOutputs::new(ServiceAccountOutputs {
                    identity: "arn:aws:iam::210987654321:role/not-the-extension-s".to_string(),
                    resource_id: "not-the-extension-s".to_string(),
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .build(),
        );

        let next = executor.step(state).await.unwrap().next_state;
        let error = next.resources["zone"]
            .error
            .as_ref()
            .expect("the step fails");
        assert!(
            error.message.contains("acme-ext"),
            "{extension:?}: {}",
            error.message
        );
        assert!(
            error
                .message
                .contains("not the identity setup created for it"),
            "{extension:?}: {}",
            error.message
        );
        assert_eq!(stepped.load(Ordering::SeqCst), 0, "{extension:?}");
    }
    assert!(sts.requests().is_empty(), "{:?}", sts.requests().len());
}

/// An extension identity the stack declares but setup has not created yet gets the controller
/// nothing, and says it has no outputs rather than that it is some other resource.
#[tokio::test]
async fn an_identity_setup_has_not_created_yet_gets_the_controller_no_credentials() {
    use alien_client_core::testing::capture::{self, Reply};
    use alien_core::{
        AwsCredentials, AwsServiceOverrides, Resource, ResourceEntry, ResourceLifecycle,
        ServiceAccount,
    };
    use alien_infra::step_protocol::ExtensionIdentityCredentials;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let sts = capture::start(|_| Reply::new(500, "text/plain", "no")).await;
    let caller = ClientConfig::Aws(Box::new(AwsClientConfig {
        account_id: "210987654321".to_string(),
        region: "us-east-1".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "AKIAOPERATOR".to_string(),
            secret_access_key: "operator-secret".to_string(),
            session_token: None,
        },
        service_overrides: Some(AwsServiceOverrides {
            endpoints: [("sts".to_string(), sts.base_url.clone())].into(),
        }),
    }));
    let manifest: alien_core::ExtensionManifest = serde_json::from_value(json!({
        "name": "acme", "version": "1.0.0", "digest": "sha256:0", "resourceTypes": {}
    }))
    .unwrap();
    let stepped = Arc::new(AtomicUsize::new(0));
    let seen = stepped.clone();
    let transport = Arc::new(Canned::new(move |request| {
        seen.fetch_add(1, Ordering::SeqCst);
        json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
            "controller": view(Value::Null), "suggestedDelayMs": null, "heartbeats": [] })
    }));
    let credentials = ExtensionIdentityCredentials::new(&manifest, &caller).unwrap();
    let mut registry = ResourceRegistry::with_built_ins();
    registry.register_controller_factory(
        ResourceType::from(DNS_ZONE),
        Platform::Aws,
        Box::new(ProxyFactory::new(
            DEPLOYMENT_ID,
            transport,
            Some(Arc::new(credentials)),
        )),
    );
    let mut stack = stack();
    stack.resources.insert(
        "acme-ext".to_string(),
        ResourceEntry {
            enabled_when: None,
            config: Resource::new(
                ServiceAccount::new("acme-ext".to_string())
                    .extension("acme".to_string())
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            display_name: None,
            description: None,
            remote_access: false,
        },
    );
    let config = config();
    // As the Operator steps a deployment: the identity is setup's to create.
    let executor = StackExecutor::builder(&stack, caller.clone())
        .deployment_config(&config)
        .resource_registry(Arc::new(registry))
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .lifecycle_filter(vec![ResourceLifecycle::Live])
        .build()
        .unwrap();

    let next = executor
        .step(StackState::new(Platform::Aws))
        .await
        .unwrap()
        .next_state;
    let error = next.resources["zone"]
        .error
        .as_ref()
        .expect("the step fails");
    assert!(
        error
            .message
            .contains("'acme-ext' has no outputs in this deployment"),
        "{}",
        error.message
    );
    assert!(!error.retryable, "{}", error.message);
    assert_eq!(stepped.load(Ordering::SeqCst), 0);
    assert!(sts.requests().is_empty(), "{:?}", sts.requests().len());
}

/// A role the deployment's state names for the extension that is not the one setup creates for
/// it, in another account or named for another deployment, is never assumed: the step fails
/// before any STS call.
#[tokio::test]
async fn a_role_setup_did_not_create_for_the_extension_is_never_assumed() {
    use alien_client_core::testing::capture::{self, Reply};
    use alien_core::{
        AwsCredentials, AwsServiceOverrides, ResourceLifecycle, ResourceOutputs, ServiceAccount,
        ServiceAccountOutputs, StackResourceState,
    };
    use alien_infra::step_protocol::ExtensionIdentityCredentials;

    for (label, role, refusal) in [
        (
            "another account",
            "arn:aws:iam::999999999999:role/test-prefix-acme-ext",
            "not in the deployment's account",
        ),
        (
            "another deployment",
            "arn:aws:iam::210987654321:role/other-prefix-acme-ext",
            "not the role setup creates for it",
        ),
        (
            "another role",
            "arn:aws:iam::210987654321:role/OrganizationAccountAccessRole",
            "not the role setup creates for it",
        ),
    ] {
        let sts = capture::start(|_| Reply::new(500, "text/plain", "no")).await;
        let caller = ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "210987654321".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIAOPERATOR".to_string(),
                secret_access_key: "operator-secret".to_string(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: [("sts".to_string(), sts.base_url.clone())].into(),
            }),
        }));
        let manifest: alien_core::ExtensionManifest = serde_json::from_value(json!({
            "name": "acme", "version": "1.0.0", "digest": "sha256:0", "resourceTypes": {}
        }))
        .unwrap();
        let transport = Arc::new(Canned::new(move |request| {
            json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
                "controller": view(Value::Null), "suggestedDelayMs": null, "heartbeats": [] })
        }));
        let credentials = ExtensionIdentityCredentials::new(&manifest, &caller).unwrap();
        let mut registry = ResourceRegistry::with_built_ins();
        registry.register_controller_factory(
            ResourceType::from(DNS_ZONE),
            Platform::Aws,
            Box::new(ProxyFactory::new(
                DEPLOYMENT_ID,
                transport,
                Some(Arc::new(credentials)),
            )),
        );
        let stack = stack();
        let config = config();
        let executor = StackExecutor::builder(&stack, caller.clone())
            .deployment_config(&config)
            .resource_registry(Arc::new(registry))
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .unwrap();
        let mut state = StackState::with_resource_prefix(Platform::Aws, "test-prefix".to_string());
        state.resources.insert(
            "acme-ext".to_string(),
            StackResourceState::builder()
                .resource_type(ServiceAccount::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(alien_core::Resource::new(
                    ServiceAccount::new("acme-ext".to_string())
                        .extension("acme".to_string())
                        .build(),
                ))
                .outputs(ResourceOutputs::new(ServiceAccountOutputs {
                    identity: role.to_string(),
                    resource_id: "acme-ext".to_string(),
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .build(),
        );

        let next = executor.step(state).await.unwrap().next_state;

        let error = next.resources["zone"]
            .error
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: the step fails"));
        assert!(
            error.message.contains(refusal),
            "{label}: {}",
            error.message
        );
        assert!(sts.requests().is_empty(), "{label}: STS was called");
    }
}

/// On Azure the Operator mints for the client id the deployment's state records, so the record
/// is what it checks: one copied from another deployment's state, naming that deployment's or
/// another subscription's managed identity, or one naming the Operator's own client id, gets
/// the controller nothing and asks Entra ID for no token. The record setup wrote does, but only
/// from a workload identity token, which Entra ID exchanges only for an identity that trusts the
/// Operator. A client secret gets a token for the Operator itself, an access token can't get
/// another identity's, and a managed identity's host endpoint returns a token for any identity
/// attached to the host, so from those the Operator asks for no token.
#[tokio::test]
async fn an_azure_identity_setup_did_not_record_for_this_deployment_is_never_minted() {
    use alien_client_core::testing::capture::{self, Reply};
    use alien_core::{
        AzureClientConfig, AzureCredentials, ResourceLifecycle, ResourceOutputs, ServiceAccount,
        ServiceAccountOutputs, StackResourceState,
    };
    use alien_infra::step_protocol::ExtensionIdentityCredentials;

    const SUBSCRIPTION: &str = "customer-subscription";
    const OPERATOR_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-000000000000";
    const ACME_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-111111111111";
    let managed_identity = |subscription: &str, name: &str| {
        format!("/subscriptions/{subscription}/resourceGroups/rg-a/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{name}")
    };
    let token = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(token.path(), "operator-federated-token").unwrap();
    let workload_identity = |authority_host: &str| AzureCredentials::WorkloadIdentity {
        client_id: OPERATOR_CLIENT_ID.to_string(),
        tenant_id: "customer-directory".to_string(),
        federated_token_file: token.path().to_str().unwrap().to_string(),
        authority_host: authority_host.to_string(),
    };
    let client_secret = |_: &str| AzureCredentials::ServicePrincipal {
        client_id: OPERATOR_CLIENT_ID.to_string(),
        client_secret: "operator-secret".to_string(),
    };
    let access_token = |_: &str| AzureCredentials::AccessToken {
        token: "operator-token".to_string(),
    };
    let scoped_access_tokens = |_: &str| AzureCredentials::ScopedAccessTokens {
        tokens: [(
            "https://management.azure.com/.default".to_string(),
            "operator-token".to_string(),
        )]
        .into(),
    };
    let setup_wrote = managed_identity(SUBSCRIPTION, "test-prefix-acme-ext");
    let vm_managed_identity = |endpoint: &str| AzureCredentials::VmManagedIdentity {
        client_id: OPERATOR_CLIENT_ID.to_string(),
        identity_endpoint: Some(endpoint.to_string()),
    };
    let app_managed_identity = |endpoint: &str| AzureCredentials::ManagedIdentity {
        client_id: OPERATOR_CLIENT_ID.to_string(),
        identity_endpoint: endpoint.to_string(),
        identity_header: "operator-header".to_string(),
    };

    let rows: [(
        &str,
        &dyn Fn(&str) -> AzureCredentials,
        &str,
        String,
        Option<&str>,
    ); 9] = [
        (
            "another deployment's record",
            &workload_identity,
            ACME_CLIENT_ID,
            managed_identity(SUBSCRIPTION, "other-prefix-acme-ext"),
            Some("not the managed identity 'test-prefix-acme-ext'"),
        ),
        (
            "another subscription's record",
            &workload_identity,
            ACME_CLIENT_ID,
            managed_identity("other-subscription", "test-prefix-acme-ext"),
            Some("not the managed identity 'test-prefix-acme-ext'"),
        ),
        (
            "the Operator's own client id",
            &workload_identity,
            OPERATOR_CLIENT_ID,
            setup_wrote.clone(),
            Some("is the Operator's own"),
        ),
        (
            "an Operator on a client secret",
            &client_secret,
            ACME_CLIENT_ID,
            setup_wrote.clone(),
            Some("the Operator runs on a client secret, which can't mint credentials for the extension's identity"),
        ),
        (
            "an Operator on an access token",
            &access_token,
            ACME_CLIENT_ID,
            setup_wrote.clone(),
            Some("the Operator runs on an access token"),
        ),
        (
            "an Operator on scoped access tokens",
            &scoped_access_tokens,
            ACME_CLIENT_ID,
            setup_wrote.clone(),
            Some("the Operator runs on scoped access tokens"),
        ),
        (
            "an Operator on a VM managed identity",
            &vm_managed_identity,
            ACME_CLIENT_ID,
            setup_wrote.clone(),
            Some("the Operator runs on a VM managed identity"),
        ),
        (
            "an Operator on an App Service or Container Apps managed identity",
            &app_managed_identity,
            ACME_CLIENT_ID,
            setup_wrote.clone(),
            Some("run the Operator with Azure workload identity"),
        ),
        (
            "the record setup wrote",
            &workload_identity,
            ACME_CLIENT_ID,
            setup_wrote.clone(),
            None,
        ),
    ];
    for (label, credentials, client_id, resource_id, refusal) in rows {
        let entra = capture::start(|_| Reply::new(500, "text/plain", "no")).await;
        let caller = ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: SUBSCRIPTION.to_string(),
            tenant_id: "customer-directory".to_string(),
            region: Some("eastus".to_string()),
            credentials: credentials(&entra.base_url),
            service_overrides: None,
        }));
        let manifest: alien_core::ExtensionManifest = serde_json::from_value(json!({
            "name": "acme", "version": "1.0.0", "digest": "sha256:0", "resourceTypes": {}
        }))
        .unwrap();
        let transport = Arc::new(Canned::new(move |request| {
            json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
                "controller": view(Value::Null), "suggestedDelayMs": null, "heartbeats": [] })
        }));
        let credentials = ExtensionIdentityCredentials::new(&manifest, &caller).unwrap();
        let mut registry = ResourceRegistry::with_built_ins();
        registry.register_controller_factory(
            ResourceType::from(DNS_ZONE),
            Platform::Azure,
            Box::new(ProxyFactory::new(
                DEPLOYMENT_ID,
                transport,
                Some(Arc::new(credentials)),
            )),
        );
        let stack = stack();
        let config = config();
        let executor = StackExecutor::builder(&stack, caller.clone())
            .deployment_config(&config)
            .resource_registry(Arc::new(registry))
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .unwrap();
        let mut state =
            StackState::with_resource_prefix(Platform::Azure, "test-prefix".to_string());
        state.resources.insert(
            "acme-ext".to_string(),
            StackResourceState::builder()
                .resource_type(ServiceAccount::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(alien_core::Resource::new(
                    ServiceAccount::new("acme-ext".to_string())
                        .extension("acme".to_string())
                        .build(),
                ))
                .outputs(ResourceOutputs::new(ServiceAccountOutputs {
                    identity: client_id.to_string(),
                    resource_id,
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .build(),
        );

        let next = executor.step(state).await.unwrap().next_state;

        let Some(refusal) = refusal else {
            assert!(
                !entra.requests().is_empty(),
                "{label}: a token is asked for the identity setup recorded"
            );
            continue;
        };
        let error = next.resources["zone"]
            .error
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: the step fails"));
        assert!(
            error.message.contains(refusal),
            "{label}: {}",
            error.message
        );
        // The managed identity rows point their host endpoint here. A client secret would ask
        // login.microsoftonline.com and an access token asks nothing, so on those rows the
        // refusal, which returns before any mint, is what shows no token was asked for.
        assert!(
            entra.requests().is_empty(),
            "{label}: a token was asked for"
        );
    }
}

/// On EKS setup lets only the Operator pod's web identity token assume the extension's
/// identity, so its credentials come from exchanging that token, never from a signed
/// `AssumeRole` with the Operator's own role.
#[tokio::test]
async fn on_eks_the_identity_is_minted_from_the_operator_pods_token() {
    use alien_client_core::testing::capture::{self, Reply};
    use alien_core::{
        AwsCredentials, AwsServiceOverrides, AwsWebIdentityConfig, KubernetesClientConfig,
        ResourceLifecycle, ResourceOutputs, ServiceAccount, ServiceAccountOutputs,
        StackResourceState,
    };
    use alien_infra::step_protocol::ExtensionIdentityCredentials;
    use std::sync::Mutex;

    const IDENTITY_ROLE: &str = "arn:aws:iam::210987654321:role/test-prefix-acme-ext";
    let sts = capture::start(|_| {
        Reply::new(
            200,
            "text/xml",
            r#"<AssumeRoleWithWebIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleWithWebIdentityResult>
    <SubjectFromWebIdentityToken>system:serviceaccount:acme:acme-manager-sa</SubjectFromWebIdentityToken>
    <Audience>sts.amazonaws.com</Audience>
    <Provider>oidc.eks.us-east-1.amazonaws.com/id/EXAMPLE</Provider>
    <AssumedRoleUser>
      <Arn>arn:aws:sts::210987654321:assumed-role/acme-ext/alien-extension-acme</Arn>
      <AssumedRoleId>AROAEXTENSION:alien-extension-acme</AssumedRoleId>
    </AssumedRoleUser>
    <Credentials>
      <AccessKeyId>ASIAEXTENSION</AccessKeyId>
      <SecretAccessKey>extension-secret</SecretAccessKey>
      <SessionToken>extension-session</SessionToken>
      <Expiration>2099-01-01T00:15:00Z</Expiration>
    </Credentials>
  </AssumeRoleWithWebIdentityResult>
</AssumeRoleWithWebIdentityResponse>"#,
        )
    })
    .await;
    let token = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(token.path(), "pod-token\n").unwrap();
    let caller = ClientConfig::KubernetesCloud {
        kubernetes: Box::new(KubernetesClientConfig::InCluster {
            namespace: Some("acme".to_string()),
            additional_headers: None,
        }),
        cloud: Box::new(ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "210987654321".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::WebIdentity {
                config: AwsWebIdentityConfig {
                    role_arn: "arn:aws:iam::210987654321:role/acme-manager".to_string(),
                    session_name: None,
                    web_identity_token_file: token.path().to_str().unwrap().to_string(),
                    duration_seconds: None,
                },
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: [("sts".to_string(), sts.base_url.clone())].into(),
            }),
        }))),
    };
    let manifest: alien_core::ExtensionManifest = serde_json::from_value(json!({
        "name": "acme", "version": "1.0.0", "digest": "sha256:0", "resourceTypes": {}
    }))
    .unwrap();
    let sent = Arc::new(Mutex::new(None));
    let seen = sent.clone();
    let transport = Arc::new(Canned::new(move |request| {
        *seen.lock().unwrap() = request.credentials.clone();
        json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
            "controller": view(Value::Null), "suggestedDelayMs": null, "heartbeats": [] })
    }));
    let credentials = ExtensionIdentityCredentials::new(&manifest, &caller).unwrap();
    let mut registry = ResourceRegistry::with_built_ins();
    registry.register_controller_factory(
        ResourceType::from(DNS_ZONE),
        Platform::Aws,
        Box::new(ProxyFactory::new(
            DEPLOYMENT_ID,
            transport,
            Some(Arc::new(credentials)),
        )),
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
    let mut state = StackState::with_resource_prefix(Platform::Aws, "test-prefix".to_string());
    state.resources.insert(
        "acme-ext".to_string(),
        StackResourceState::builder()
            .resource_type(ServiceAccount::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(alien_core::Resource::new(
                ServiceAccount::new("acme-ext".to_string())
                    .extension("acme".to_string())
                    .build(),
            ))
            .outputs(ResourceOutputs::new(ServiceAccountOutputs {
                identity: IDENTITY_ROLE.to_string(),
                resource_id: "acme-ext".to_string(),
            }))
            .lifecycle(ResourceLifecycle::Frozen)
            .build(),
    );

    let next = executor.step(state).await.unwrap().next_state;

    assert!(
        next.resources["zone"].error.is_none(),
        "{:?}",
        next.resources["zone"].error
    );
    let requests: Vec<Vec<(String, String)>> = sts
        .requests()
        .iter()
        .map(|request| {
            reqwest::Url::parse(&format!("http://sts/?{}", request.body_text()))
                .unwrap()
                .query_pairs()
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect()
        })
        .collect();
    let param = |request: &[(String, String)], key: &str| {
        request
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(
        param(&requests[0], "Action").as_deref(),
        Some("AssumeRoleWithWebIdentity")
    );
    assert_eq!(
        param(&requests[0], "RoleArn").as_deref(),
        Some(IDENTITY_ROLE)
    );
    assert_eq!(
        param(&requests[0], "WebIdentityToken").as_deref(),
        Some("pod-token")
    );
    assert_eq!(
        param(&requests[0], "RoleSessionName").as_deref(),
        Some("alien-extension-acme")
    );
    let Some(Credentials::Aws {
        access_key_id,
        expires_at,
        ..
    }) = sent.lock().unwrap().clone()
    else {
        panic!("the step carries no AWS credentials");
    };
    assert_eq!(access_key_id, "ASIAEXTENSION");
    assert_eq!(expires_at.as_deref(), Some("2099-01-01T00:15:00Z"));
}

/// A reply to another step can't be taken for this one's.
#[cfg(unix)]
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
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(gone(pid).await, "the controller's child {pid} still runs");
}

/// Whether process `pid` stopped within five seconds. A zombie counts as stopped: a killed
/// orphan stays one until its new parent reaps it, which a container's PID 1 may never do.
#[cfg(unix)]
async fn gone(pid: u32) -> bool {
    for _ in 0..50 {
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        // `ps` prints nothing for a pid that no longer exists; an error goes to stderr.
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stat = String::from_utf8_lossy(&out.stdout);
        if stat.trim().is_empty() || stat.trim_start().starts_with('Z') {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status();
    false
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
