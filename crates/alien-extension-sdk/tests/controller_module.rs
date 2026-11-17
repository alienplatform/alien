//! The SDK's controller module as Alien runs it: the docs' controller library
//! (`tests/guests/acme-controller`) built for wasm32-wasip1, loaded in alien-hooks-runtime and
//! stepped through `WasmTransport`. Every cloud is a server on this machine that records what
//! reached it.

use alien_client_core::testing::capture::{self, Captured, Reply, Server};
use alien_core::{Platform, ResourceStatus};
use alien_hooks_runtime::{
    build_controller_module, Capabilities, ControllerModule, Runtime, StepLimits,
};
use alien_infra::step_protocol::protocol::{
    codes, Credentials, LoadResult, ResponseBody, StepError, StepRequest, StepResponse,
    TransitionPreview,
};
use alien_infra::step_protocol::wasm::{WasmHost, WasmTransport};
use alien_infra::step_protocol::Transport;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
// `#[controller]` expands to `crate::core`, `crate::error` and `crate::Result`.
use alien_extension_sdk::controller_prelude::{core, error, Result};

// ---------------------------------------------------------------- fixtures

const ROUTE53: &str = "https://route53.amazonaws.com";
const CLOUD_DNS: &str = "https://dns.googleapis.com";
const RESOURCE_MANAGER: &str = "https://management.azure.com";

const AWS_SECRET: &str = "AWSSECRETACCESSKEY";
const GCP_SECRET: &str = "GCPACCESSTOKEN";
const AZURE_SECRET: &str = "AZUREACCESSTOKEN";

/// The docs' controller library as `alien extensions build` builds it. Cargo builds it once,
/// and each test process takes the build's lock in turn.
fn guest() -> &'static [u8] {
    static GUEST: OnceLock<Vec<u8>> = OnceLock::new();
    GUEST.get_or_init(|| built_in("extension-sdk-guests"))
}

/// The docs' controller library built into `target_dir`, a directory under the tests' own.
fn built_in(target_dir: &str) -> Vec<u8> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(target_dir);
    build_controller_module(
        &workspace,
        "acme-controller",
        "acme_controller",
        &target_dir,
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

fn module() -> ControllerModule {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| Runtime::new().expect("the runtime starts"));
    runtime
        .load_controller(guest(), &hex::encode(Sha256::digest(guest())))
        .expect("the module loads")
}

fn aws() -> Credentials {
    Credentials::Aws {
        account_id: "123456789012".into(),
        region: "eu-west-1".into(),
        access_key_id: "ASIAEXTENSION".into(),
        secret_access_key: AWS_SECRET.into(),
        session_token: None,
        expires_at: None,
    }
}

fn gcp() -> Credentials {
    Credentials::Gcp {
        project_id: "the-project".into(),
        region: "us-central1".into(),
        access_token: GCP_SECRET.into(),
        expires_at: "2099-01-01T00:00:00Z".into(),
    }
}

fn azure() -> Credentials {
    Credentials::Azure {
        subscription_id: "the-subscription".into(),
        tenant_id: "the-tenant".into(),
        access_token: AZURE_SECRET.into(),
        expires_at: "2099-01-01T00:00:00Z".into(),
    }
}

/// A transport whose calls to `origin` go to `stub`.
fn transport_to(origin: &str, stub: &str) -> WasmTransport {
    Arc::new(WasmHost::new().unwrap().routing(origin, stub))
        .transport("proj_test", "acme", module(), &[])
        .unwrap()
}

/// A step of the zone `zone` on `platform`, from `state`.
fn zone_step(platform: &str, credentials: Credentials, state: Value) -> StepRequest {
    let mut request: StepRequest = serde_json::from_value(json!({
        "protocol": 1, "stepId": format!("step-{platform}"), "deploymentId": "dep_acme",
        "resourceId": "zone", "resourceType": "acme/dns-zone", "platform": platform,
        "resourcePrefix": "q6aa0b26", "initialSetupAuthority": "importedHandoff",
        "lifecycle": "live",
        "resource": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
        "previousResource": null, "dependencies": {}, "deploymentConfig": {},
        "setupRun": false, "setupRunId": null, "state": state, "transitions": [],
        "credentials": null, "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap();
    request.credentials = Some(credentials);
    request
}

async fn step(transport: &WasmTransport, request: &StepRequest) -> StepResponse {
    transport
        .step(request, Duration::from_secs(30))
        .await
        .unwrap_or_else(|failure| panic!("the module did not answer: {failure:?}"))
}

/// The controller's state after a step that ran.
fn stepped(response: StepResponse) -> (Value, ResourceStatus, Option<Value>) {
    match response.body {
        ResponseBody::Stepped { controller, .. } => (
            controller.state,
            controller.status,
            controller.outputs.map(|outputs| json!(outputs)),
        ),
        other => panic!("expected a stepped reply, got {other:?}"),
    }
}

/// The error of a step that ran and failed, and the status its controller is left with.
fn failed(response: StepResponse) -> (StepError, ResourceStatus) {
    match response.body {
        ResponseBody::Failed { controller, error } => (error, controller.on_failure_status),
        other => panic!("expected a failed reply, got {other:?}"),
    }
}

fn json_reply(status: u16, body: Value) -> Reply {
    Reply::new(status, "application/json", body.to_string())
}

/// The requests `stub` got, as `METHOD target`.
fn targets(stub: &Server) -> Vec<String> {
    stub.requests()
        .iter()
        .map(|request| format!("{} {}", request.method, request.target))
        .collect()
}

// ---------------------------------------------------------------- the build

/// The module's bytes come from its sources alone: a second compile of every crate, in another
/// target directory, gives the same module, so a bundle's digest names one build of the
/// extension wherever and whenever it is built.
#[test]
fn a_second_build_gives_the_same_module() {
    let again = built_in("extension-sdk-guests-again");
    assert_eq!(
        hex::encode(Sha256::digest(&again)),
        hex::encode(Sha256::digest(guest()))
    );
}

// ---------------------------------------------------------------- the ops

/// The module lists the type and platforms of each controller its `register` adds, sorted, so
/// a build writes the same manifest every time.
#[test]
fn the_module_lists_its_controllers() {
    struct NoCalls;
    impl Capabilities for NoCalls {
        fn cloud(&mut self, _: &[u8]) -> std::result::Result<Vec<u8>, String> {
            Err("refused: listing controllers makes no call".to_string())
        }
        fn http(&mut self, _: &[u8]) -> std::result::Result<Vec<u8>, String> {
            Err("refused: listing controllers makes no call".to_string())
        }
    }
    let limits = StepLimits::within(Duration::from_secs(30));
    let reply = module()
        .step(
            br#"{"protocol":1,"op":"controllers"}"#,
            &limits,
            Box::new(NoCalls),
        )
        .expect("the module answers");
    assert_eq!(
        serde_json::from_slice::<Value>(&reply).unwrap(),
        json!({
            "protocol": 1,
            "controllers": {
                "acme/dns-zone": ["aws", "azure", "gcp"],
                "acme/probe": ["test"],
            }
        })
    );

    let unknown = module().step(
        br#"{"protocol":1,"op":"restart"}"#,
        &limits,
        Box::new(NoCalls),
    );
    assert_eq!(
        unknown,
        Err(alien_hooks_runtime::StepFailure::NoReply(
            "this controller module has no `restart` op".to_string()
        ))
    );
}

/// `load` reads an importer's fields without stepping: the view of the state as the controller
/// read it, or why it can't.
#[tokio::test(flavor = "multi_thread")]
async fn the_module_loads_an_importer_s_fields() {
    let transport = Arc::new(WasmHost::new().unwrap())
        .transport("proj_test", "acme", module(), &[])
        .unwrap();
    let load = |platform, state| {
        let transport = transport.clone();
        async move {
            transport
                .load("acme/dns-zone", platform, state, Duration::from_secs(30))
                .await
                .expect("the module answers")
        }
    };

    let LoadResult::View(view) =
        load(Platform::Aws, json!({ "state": "ready", "zoneId": "Z123" })).await
    else {
        panic!("the AWS controller reads the importer's fields")
    };
    assert_eq!(view.status, ResourceStatus::Running);
    assert_eq!(
        json!(view.outputs),
        json!({ "type": "acme/dns-zone", "zoneId": "Z123" })
    );
    assert_eq!(
        view.state,
        json!({
            "type": "AwsDnsZoneController", "_controllerStateVersion": 1,
            "state": "ready", "zoneId": "Z123", "internalStayCount": null
        })
    );
    assert_eq!(view.on_failure_status, ResourceStatus::RefreshFailed);
    assert_eq!(
        view.update_start,
        TransitionPreview::Allowed {
            status: ResourceStatus::Updating
        }
    );
    assert_eq!(
        view.delete_start,
        TransitionPreview::Allowed {
            status: ResourceStatus::Deleting
        }
    );

    for (platform, state, reason) in [
        (
            Platform::Aws,
            json!({ "state": "running", "zoneId": "Z123" }),
            "unknown variant `running`",
        ),
        (
            Platform::Gcp,
            json!({ "state": "ready", "zoneName": "z", "type": "GcpDnsZoneController" }),
            "the imported state sets 'type', which the controller owns",
        ),
        (
            Platform::Kubernetes,
            json!({ "state": "ready" }),
            "no controller for acme/dns-zone on kubernetes",
        ),
    ] {
        let LoadResult::Refused(refusal) = load(platform, state).await else {
            panic!("{reason}: the load passed")
        };
        assert!(refusal.contains(reason), "{refusal}");
    }
}

// ---------------------------------------------------------------- Google Cloud

/// The zone's whole life on Google Cloud, each call written with `CloudRequest::gcp`: Alien
/// sends it to Cloud DNS with the step's token, which the module never holds. A create and a
/// delete that run a second time, as after a lost reply, end the same.
#[tokio::test(flavor = "multi_thread")]
async fn a_zone_is_created_updated_and_deleted_on_google_cloud() {
    let stub = capture::start(|request: &Captured| match request.method.as_str() {
        "POST" => json_reply(200, json!({ "name": "q6aa0b26-zone" })),
        _ => Reply::new(204, "application/json", ""),
    })
    .await;
    let transport = transport_to(CLOUD_DNS, &stub.base_url);

    let (state, status, outputs) =
        stepped(step(&transport, &zone_step("gcp", gcp(), Value::Null)).await);
    assert_eq!(status, ResourceStatus::Running);
    assert_eq!(
        outputs,
        Some(json!({ "type": "acme/dns-zone", "zoneId": "q6aa0b26-zone" }))
    );
    assert_eq!(state["state"], "ready");
    let created = &stub.requests()[0];
    assert_eq!(
        (created.method.as_str(), created.target.as_str()),
        ("POST", "/dns/v1/projects/the-project/managedZones")
    );
    assert_eq!(
        created.header("authorization"),
        Some(format!("Bearer {GCP_SECRET}").as_str())
    );
    assert_eq!(created.header("content-type"), Some("application/json"));
    assert_eq!(
        serde_json::from_slice::<Value>(&created.body).unwrap(),
        json!({
            "name": "q6aa0b26-zone", "dnsName": "internal.acme.dev.",
            "description": "The zone zone"
        })
    );
    assert!(!state.to_string().contains(GCP_SECRET), "{state}");

    // An update that keeps the domain goes back to Ready, and one that changes it is refused
    // for good: the handler reads the config before the change.
    let mut update = zone_step("gcp", gcp(), state.clone());
    update.previous_resource = Some(update.resource.clone());
    update.transitions = serde_json::from_value(json!(["update-start"])).unwrap();
    let (updated, status, _) = stepped(step(&transport, &update).await);
    assert_eq!(status, ResourceStatus::Running);
    update.previous_resource =
        Some(json!({ "type": "acme/dns-zone", "id": "zone", "domain": "old.acme.dev" }));
    let (error, on_failure) = failed(step(&transport, &update).await);
    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("RESOURCE_CONFIG_INVALID", false)
    );
    assert!(
        error
            .message
            .contains("can't change its domain from old.acme.dev to internal.acme.dev"),
        "{}",
        error.message
    );
    assert_eq!(on_failure, ResourceStatus::UpdateFailed);

    let mut delete = zone_step("gcp", gcp(), updated);
    delete.transitions = serde_json::from_value(json!(["delete-start"])).unwrap();
    let (_, status, _) = stepped(step(&transport, &delete).await);
    assert_eq!(status, ResourceStatus::Deleted);
    assert_eq!(
        targets(&stub),
        [
            "POST /dns/v1/projects/the-project/managedZones",
            "DELETE /dns/v1/projects/the-project/managedZones/q6aa0b26-zone",
        ]
    );

    // The same create and delete against a cloud where the first run already happened.
    let again = capture::start(|request: &Captured| match request.method.as_str() {
        "POST" => json_reply(409, json!({ "error": { "status": "ALREADY_EXISTS" } })),
        _ => json_reply(404, json!({ "error": { "status": "NOT_FOUND" } })),
    })
    .await;
    let transport = transport_to(CLOUD_DNS, &again.base_url);
    let (state, status, _) = stepped(step(&transport, &zone_step("gcp", gcp(), Value::Null)).await);
    assert_eq!(status, ResourceStatus::Running);
    assert_eq!(state["zoneName"], "q6aa0b26-zone");
    let mut delete = zone_step("gcp", gcp(), state);
    delete.transitions = serde_json::from_value(json!(["delete-start"])).unwrap();
    let (_, status, _) = stepped(step(&transport, &delete).await);
    assert_eq!(status, ResourceStatus::Deleted);
}

/// A step whose call the cloud denies fails with the code and the retry flag Alien's own
/// clients give a 403, and leaves the controller where the step began.
#[tokio::test(flavor = "multi_thread")]
async fn a_denied_call_fails_the_step_with_alien_s_code() {
    let stub =
        capture::start(|_| json_reply(403, json!({ "error": { "status": "PERMISSION_DENIED" } })))
            .await;
    let transport = transport_to(CLOUD_DNS, &stub.base_url);

    let response = step(&transport, &zone_step("gcp", gcp(), Value::Null)).await;
    let ResponseBody::Failed { controller, error } = response.body else {
        panic!("the step failed")
    };
    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("REMOTE_ACCESS_DENIED", true)
    );
    assert!(
        error
            .message
            .starts_with("Creating the Cloud DNS zone answered 403: "),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("PERMISSION_DENIED"),
        "{}",
        error.message
    );
    assert_eq!(controller.state["state"], "createStart");
    assert_eq!(controller.status, ResourceStatus::Provisioning);
    assert_eq!(
        controller.on_failure_status,
        ResourceStatus::ProvisionFailed
    );
    assert_eq!(controller.outputs, None);
}

// ---------------------------------------------------------------- Azure

/// On Azure the zone goes in the deployment's resource group, which the handler reads from the
/// outputs of a dependency. Each call is written with `CloudRequest::azure`.
#[tokio::test(flavor = "multi_thread")]
async fn a_zone_is_created_and_deleted_on_azure_in_its_dependency_s_resource_group() {
    const ZONE: &str = "/subscriptions/the-subscription/resourceGroups/rg-q6aa0b26/providers/Microsoft.Network/dnsZones/internal.acme.dev";
    let stub = capture::start(|request: &Captured| match request.method.as_str() {
        "PUT" => json_reply(201, json!({ "id": ZONE, "location": "global" })),
        _ => Reply::new(200, "application/json", ""),
    })
    .await;
    let transport = transport_to(RESOURCE_MANAGER, &stub.base_url);
    let mut create = zone_step("azure", azure(), Value::Null);

    // The resource group is not among the step's dependencies, then it has no outputs yet.
    let (error, _) = failed(step(&transport, &create).await);
    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("DEPENDENCY_NOT_FOUND", false)
    );
    let group = |outputs: Value| {
        serde_json::from_value(json!({
            "default-resource-group": {
                "resourceType": "azure_resource_group", "status": "running",
                "outputs": outputs, "bindingParams": null
            }
        }))
        .unwrap()
    };
    create.dependencies = group(Value::Null);
    let (error, _) = failed(step(&transport, &create).await);
    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("DEPENDENCY_NOT_READY", true)
    );
    assert!(stub.requests().is_empty());

    create.dependencies = group(json!({
        "type": "azure_resource_group", "name": "rg-q6aa0b26",
        "resourceId": "/subscriptions/the-subscription/resourceGroups/rg-q6aa0b26",
        "location": "westeurope"
    }));
    let (state, status, outputs) = stepped(step(&transport, &create).await);
    assert_eq!(status, ResourceStatus::Running);
    assert_eq!(
        outputs,
        Some(json!({ "type": "acme/dns-zone", "zoneId": ZONE }))
    );
    let created = &stub.requests()[0];
    assert_eq!(created.method, "PUT");
    assert_eq!(created.target, format!("{ZONE}?api-version=2018-05-01"));
    assert_eq!(
        created.header("authorization"),
        Some(format!("Bearer {AZURE_SECRET}").as_str())
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&created.body).unwrap(),
        json!({ "location": "global" })
    );

    let mut delete = zone_step("azure", azure(), state);
    delete.transitions = serde_json::from_value(json!(["delete-start"])).unwrap();
    let (_, status, _) = stepped(step(&transport, &delete).await);
    assert_eq!(status, ResourceStatus::Deleted);
    assert_eq!(
        targets(&stub)[1],
        format!("DELETE {ZONE}?api-version=2018-05-01")
    );
}

/// A body that does not read as the type the handler asks `CloudResponse::json` for fails the
/// step for good: the same call gets the same body.
#[tokio::test(flavor = "multi_thread")]
async fn a_response_body_of_another_shape_fails_the_step_for_good() {
    let stub = capture::start(|_| json_reply(201, json!({ "location": "global" }))).await;
    let transport = transport_to(RESOURCE_MANAGER, &stub.base_url);
    let mut create = zone_step("azure", azure(), Value::Null);
    create.dependencies = serde_json::from_value(json!({
        "default-resource-group": {
            "resourceType": "azure_resource_group", "status": "running",
            "outputs": {
                "type": "azure_resource_group", "name": "rg-q6aa0b26",
                "resourceId": "/subscriptions/the-subscription/resourceGroups/rg-q6aa0b26",
                "location": "westeurope"
            },
            "bindingParams": null
        }
    }))
    .unwrap();

    let (error, _) = failed(step(&transport, &create).await);
    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("UNEXPECTED_RESPONSE", false)
    );
    assert!(
        error
            .message
            .contains("the response with status 201 did not read as the type asked for"),
        "{}",
        error.message
    );
    assert_eq!(stub.requests().len(), 1);
}

// ---------------------------------------------------------------- AWS, through the AWS SDK

const CREATED_ZONE: &str = r#"<?xml version="1.0"?>
<CreateHostedZoneResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <HostedZone><Id>/hostedzone/Z0123</Id><Name>internal.acme.dev.</Name><CallerReference>q6aa0b26-zone</CallerReference></HostedZone>
  <ChangeInfo><Id>/change/C1</Id><Status>PENDING</Status><SubmittedAt>2026-01-01T00:00:00Z</SubmittedAt></ChangeInfo>
  <DelegationSet><NameServers><NameServer>ns-1.example.</NameServer></NameServers></DelegationSet>
</CreateHostedZoneResponse>"#;

const THROTTLED: &str = r#"<?xml version="1.0"?>
<ErrorResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <Error><Type>Sender</Type><Code>Throttling</Code><Message>Rate exceeded</Message></Error>
  <RequestId>r1</RequestId>
</ErrorResponse>"#;

/// The docs' AWS controller calls Route53 through the official SDK. The SDK's request reaches
/// Route53's own endpoint signed with the step's credentials, not with the placeholder the SDK
/// signed with inside the module.
#[tokio::test(flavor = "multi_thread")]
async fn an_aws_sdk_client_in_the_module_calls_through_alien() {
    let stub = capture::start(|_| {
        Reply::new(201, "text/xml", CREATED_ZONE).header(
            "location",
            "https://route53.amazonaws.com/2013-04-01/hostedzone/Z0123",
        )
    })
    .await;
    let transport = transport_to(ROUTE53, &stub.base_url);

    let (state, status, outputs) =
        stepped(step(&transport, &zone_step("aws", aws(), Value::Null)).await);
    assert_eq!(status, ResourceStatus::Running);
    assert_eq!(
        outputs,
        Some(json!({ "type": "acme/dns-zone", "zoneId": "Z0123" }))
    );
    assert!(!state.to_string().contains(AWS_SECRET), "{state}");

    let requests = stub.requests();
    assert_eq!(targets(&stub), ["POST /2013-04-01/hostedzone"]);
    let authorization = requests[0].header("authorization").unwrap();
    // Route53 is a global service: its requests are signed for us-east-1 whatever the step's
    // region is.
    assert!(
        authorization.starts_with("AWS4-HMAC-SHA256 Credential=ASIAEXTENSION/")
            && authorization.contains("/us-east-1/route53/aws4_request"),
        "{authorization}"
    );
    assert!(!authorization.contains("PLACEHOLDER"), "{authorization}");
    let body = requests[0].body_text();
    assert!(body.contains("<Name>internal.acme.dev</Name>"), "{body}");
    assert!(
        body.contains("<CallerReference>q6aa0b26-zone</CallerReference>"),
        "{body}"
    );
}

/// Nothing sends a throttled call again: the SDK's retries are off and Alien's host sends each
/// call once. `aws_sdk_error` gives the failure the retry flag Alien's own clients give it, so
/// the executor runs the step again.
#[tokio::test(flavor = "multi_thread")]
async fn a_throttled_aws_call_is_sent_once_and_fails_the_step_as_retryable() {
    let stub = capture::start(|_| Reply::new(400, "text/xml", THROTTLED)).await;
    let transport = transport_to(ROUTE53, &stub.base_url);

    let (error, on_failure) = failed(step(&transport, &zone_step("aws", aws(), Value::Null)).await);
    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("CLOUD_PLATFORM_ERROR", true)
    );
    assert!(
        error.message.contains(
            "caused by RATE_LIMIT_EXCEEDED: Rate limit exceeded: AWS answered 400 (Throttling): Rate exceeded"
        ),
        "{}",
        error.message
    );
    assert_eq!(on_failure, ResourceStatus::ProvisionFailed);
    assert_eq!(stub.requests().len(), 1);
}

/// A call that never reached Route53 fails the step as retryable too.
#[tokio::test(flavor = "multi_thread")]
async fn an_aws_call_that_got_no_answer_fails_the_step_as_retryable() {
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let transport = transport_to(ROUTE53, &closed);

    let (error, _) = failed(step(&transport, &zone_step("aws", aws(), Value::Null)).await);
    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("CLOUD_PLATFORM_ERROR", true)
    );
    assert!(
        error
            .message
            .contains("caused by HTTP_REQUEST_FAILED: The call failed: "),
        "{}",
        error.message
    );
}

/// A call the cloud never answers ends at the step's deadline, and the step fails with the
/// deadline's own code, which Alien retries, whatever the handler made of the cut call.
#[tokio::test(flavor = "multi_thread")]
async fn a_step_whose_call_outlasts_the_deadline_fails_as_a_retryable_deadline() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent = format!("http://{}", listener.local_addr().unwrap());
    let held = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            open.push(socket);
        }
    });
    let transport = transport_to(ROUTE53, &silent);

    // Four seconds: two for the module, and the host's two of grace to answer in. The step
    // fails for its deadline in the module's own reply, or the host fails it so without
    // reading the reply.
    let stepped = transport
        .step(
            &zone_step("aws", aws(), Value::Null),
            Duration::from_secs(4),
        )
        .await;
    let (code, retryable) = match stepped {
        Ok(response) => {
            let (error, _) = failed(response);
            (error.code, error.retryable)
        }
        Err(failure) => (failure.code.to_string(), failure.retryable),
    };
    assert_eq!((code.as_str(), retryable), (codes::STEP_DEADLINE, true));
    held.abort();
}

/// A handler reports the resource's health with `ctx.emit_heartbeat`, and the step's reply
/// carries the report to Alien.
#[tokio::test(flavor = "multi_thread")]
async fn a_heartbeat_a_handler_emits_reaches_alien() {
    let transport = Arc::new(WasmHost::new().unwrap())
        .transport("proj_test", "acme", module(), &[])
        .unwrap();
    // An imported zone's first step is a refresh from Ready.
    let mut refresh = zone_step("aws", aws(), Value::Null);
    refresh.pending_import = Some(json!({ "state": "ready", "zoneId": "Z123" }));

    let ResponseBody::Stepped {
        heartbeats,
        suggested_delay_ms,
        ..
    } = step(&transport, &refresh).await.body
    else {
        panic!("the refresh stepped")
    };
    assert_eq!(suggested_delay_ms, Some(60_000));
    assert_eq!(heartbeats.len(), 1);
    let heartbeat = json!(heartbeats[0]);
    assert_eq!(heartbeat["resourceId"], "zone");
    assert_eq!(heartbeat["resourceType"], "acme/dns-zone");
    assert_eq!(heartbeat["controllerPlatform"], "aws");
    assert_eq!(heartbeat["data"]["data"]["status"]["health"], "healthy");
}

// ---------------------------------------------------------------- declared origins

/// A request to an origin the extension declares goes out with the module's own header and
/// body, bytes that are not text included, and with nothing of the deployment's. A request to
/// any other origin is refused, and the same request would be again, so the step fails for
/// good.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_to_a_declared_origin_carries_only_what_the_module_sent() {
    let stub =
        capture::start(|_| Reply::new(200, "application/octet-stream", vec![0xfe, 0x01])).await;
    let host = Arc::new(WasmHost::new().unwrap().allowing_local_egress());
    let probe = |url: String| {
        let mut request: StepRequest = serde_json::from_value(json!({
            "protocol": 1, "stepId": "step-probe", "deploymentId": "dep_acme",
            "resourceId": "probe", "resourceType": "acme/probe", "platform": "test",
            "resourcePrefix": "q6aa0b26", "initialSetupAuthority": "importedHandoff",
            "lifecycle": "live",
            "resource": { "type": "acme/probe", "id": "probe", "url": url },
            "previousResource": null, "dependencies": {}, "deploymentConfig": {},
            "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
            "credentials": null, "deadline": "2099-01-01T00:00:00Z"
        }))
        .unwrap();
        request.credentials = Some(aws());
        request
    };

    let declared = host
        .transport("proj_test", "acme", module(), &[stub.base_url.clone()])
        .unwrap();
    let (_, status, outputs) =
        stepped(step(&declared, &probe(format!("{}/v1/zones", stub.base_url))).await);
    assert_eq!(status, ResourceStatus::Running);
    assert_eq!(
        outputs,
        Some(json!({ "type": "acme/probe", "answer": "200 [254, 1]" }))
    );
    let sent = &stub.requests()[0];
    assert_eq!(
        (sent.method.as_str(), sent.target.as_str()),
        ("POST", "/v1/zones")
    );
    assert_eq!(sent.header("authorization"), Some("Bearer the-vendor-key"));
    assert_eq!(sent.body, [0xff, 0x00, 0x7f]);
    assert!(sent.header("x-amz-date").is_none());

    let undeclared = host.transport("proj_test", "acme", module(), &[]).unwrap();
    let (error, _) = failed(step(&undeclared, &probe(format!("{}/v1/zones", stub.base_url))).await);
    assert_eq!(
        (error.code.as_str(), error.retryable),
        ("EXTENSION_CALL_REFUSED", false)
    );
    assert_eq!(stub.requests().len(), 1);
}

// ---------------------------------------------------------------- a handler's errors, natively

/// A zone controller whose create makes an AWS SDK call that goes out unsigned, and whose
/// delete wraps the cloud's answer in its own context.
mod handler_errors {
    use alien_extension_sdk::controller_prelude::*;

    #[controller]
    pub struct ZoneController {
        zone_id: Option<String>,
    }

    #[controller]
    impl ZoneController {
        #[flow_entry(Create)]
        #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
        async fn create_start(
            &mut self,
            ctx: &ResourceControllerContext<'_>,
        ) -> Result<HandlerAction> {
            // STS takes this call without a signature, so the SDK sends none.
            aws_sdk_sts::Client::new(&ctx.aws_sdk_config().await?)
                .assume_role_with_web_identity()
                .role_arn("arn:aws:iam::123456789012:role/zone")
                .role_session_name("zone")
                .web_identity_token("token")
                .send()
                .await
                .map_err(aws_sdk_error)?;
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        }

        #[flow_entry(Update, from = [Ready])]
        #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
        async fn update_start(
            &mut self,
            ctx: &ResourceControllerContext<'_>,
        ) -> Result<HandlerAction> {
            let request = CloudRequest::aws("route53", "GET", "/2013-04-01/hostedzone/Z0456");
            ctx.cloud()
                .send(&request)?
                .ok("Reading the parent zone")
                .context(ErrorData::ResourceConfigInvalid {
                    message: "the parent zone Z0456 the resource names does not exist".to_string(),
                    resource_id: Some("zone".to_string()),
                })?;
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        }

        #[flow_entry(Delete)]
        #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
        async fn delete_start(
            &mut self,
            ctx: &ResourceControllerContext<'_>,
        ) -> Result<HandlerAction> {
            let request = CloudRequest::aws("route53", "DELETE", "/2013-04-01/hostedzone/Z0123");
            ctx.cloud()
                .send(&request)?
                .ok("Deleting the Route53 zone")
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to delete the zone".to_string(),
                    resource_id: Some("zone".to_string()),
                })?;
            Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            })
        }

        terminal_state!(state = Ready, status = ResourceStatus::Running);
        terminal_state!(
            state = CreateFailed,
            status = ResourceStatus::ProvisionFailed
        );
        terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
        terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
        terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

        fn build_outputs(&self) -> Option<ResourceOutputs> {
            None
        }
    }

    pub fn register(controllers: &mut Controllers) {
        controllers.register(
            acme_types::DnsZone::RESOURCE_TYPE,
            Platform::Aws,
            ZoneController::default(),
        );
    }
}

/// A step of the zone on AWS, answered by `testing::answer` instead of a module.
fn native_zone_step(transitions: Value) -> Value {
    json!({
        "protocol": 1, "stepId": "step-1", "resourceType": "acme/dns-zone",
        "platform": "aws", "resourcePrefix": "q6aa0b26",
        "resource": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
        "previousResource": null, "dependencies": {}, "state": null, "transitions": transitions,
        "cloud": { "platform": "aws", "accountId": "123456789012", "region": "eu-west-1" },
        "deadline": "2099-01-01T00:00:00Z"
    })
}

/// A 404 under the handler's own `.context(...)` still replies with the 404's code, which Alien
/// takes as already deleted while the resource deletes.
#[test]
fn a_not_found_under_a_handler_s_context_replies_with_the_not_found_code() {
    let reply = alien_extension_sdk::controller_module::testing::answer(
        handler_errors::register,
        &native_zone_step(json!(["delete-start"])),
        |_| Ok(json!({ "status": 404, "headers": [], "body": "NoSuchHostedZone" })),
    )
    .unwrap();

    assert_eq!(reply["result"], "failed", "{reply}");
    assert_eq!(
        reply["error"]["code"], "REMOTE_RESOURCE_NOT_FOUND",
        "{reply}"
    );
    assert_eq!(reply["error"]["retryable"], true, "{reply}");
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("caused by REMOTE_RESOURCE_NOT_FOUND"),
        "{reply}"
    );
}

/// Outside a delete, a 404 under a handler's context that another try can't pass keeps that
/// context's retry flag: the step fails for good rather than wait on a resource that is gone.
#[test]
fn a_not_found_under_a_final_context_outside_a_delete_fails_the_step_for_good() {
    // A zone that reached Ready: the state a failed create leaves, moved on to Ready.
    let created = alien_extension_sdk::controller_module::testing::answer(
        handler_errors::register,
        &native_zone_step(json!([])),
        |_| Err("refused: this step only reads the controller's state".to_string()),
    )
    .unwrap();
    let mut ready = created["controller"]["state"].clone();
    ready["state"] = json!("ready");
    let mut update = native_zone_step(json!(["update-start"]));
    update["state"] = ready;

    let reply = alien_extension_sdk::controller_module::testing::answer(
        handler_errors::register,
        &update,
        |_| Ok(json!({ "status": 404, "headers": [], "body": "NoSuchHostedZone" })),
    )
    .unwrap();

    assert_eq!(reply["result"], "failed", "{reply}");
    assert_eq!(
        reply["error"]["code"], "REMOTE_RESOURCE_NOT_FOUND",
        "{reply}"
    );
    assert_eq!(reply["error"]["retryable"], false, "{reply}");
}

/// The host signs every AWS call, so an SDK request that comes unsigned can't be made, and
/// another try makes the same request: the step fails for good, and nothing reaches the host.
#[test]
fn an_aws_sdk_request_the_connector_cannot_make_fails_the_step_for_good() {
    let calls = Arc::new(std::sync::Mutex::new(0));
    let seen = calls.clone();
    let reply = alien_extension_sdk::controller_module::testing::answer(
        handler_errors::register,
        &native_zone_step(json!([])),
        move |_| {
            *seen.lock().unwrap() += 1;
            Err("refused: an unsigned request never reaches the host".to_string())
        },
    )
    .unwrap();

    assert_eq!(reply["result"], "failed", "{reply}");
    assert_eq!(
        reply["error"]["code"], "EXTENSION_REQUEST_INVALID",
        "{reply}"
    );
    assert_eq!(reply["error"]["retryable"], false, "{reply}");
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("the SDK sent an unsigned request"),
        "{reply}"
    );
    assert_eq!(*calls.lock().unwrap(), 0);
}
