//! A controller that runs as a WebAssembly module: `tests/controller-guest`, built for
//! wasm32-wasip1 and stepped through `WasmTransport`, alone and under the real proxy and
//! executor. Every cloud is a server on this machine that records what reached it.

use alien_aws_clients::AwsClientConfigExt;
use alien_client_core::interceptor::{CallInterceptor, Cloud, InterceptorChain};
use alien_client_core::testing::capture::{self, Captured, Reply, Server};
use alien_client_core::testing::TestInterceptor;
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{
    AwsClientConfig, ClientConfig, DeploymentConfig, EnvironmentVariable, EnvironmentVariableType,
    EnvironmentVariablesSnapshot, ExternalBindings, OtlpConfig, Platform, ResourceStatus,
    ResourceType, Stack, StackResourceState, StackSettings, StackState,
};
use alien_error::AlienError;
use alien_hooks_runtime::{ControllerModule, Runtime};
use alien_infra::step_protocol::protocol::{
    codes, Credentials, ResponseBody, StepRequest, StepResponse,
};
use alien_infra::step_protocol::wasm::{
    WasmHost, WasmTransport, CALLS_PER_STEP, MAX_REQUEST_BODY, MAX_RESPONSE_BYTES,
};
use alien_infra::step_protocol::{
    deployment_config_for_extension, CredentialSource, ProxyFactory, Transport, TransportFailure,
    DELETE_TIMEOUT,
};
use alien_infra::{
    MockPlatformServiceProvider, ResourceControllerContext, ResourceRegistry, StackExecutor,
};
use async_trait::async_trait;
use base64::Engine;
use futures::FutureExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- fixtures

fn guest() -> &'static [u8] {
    alien_infra::extension_bundle::testing::controller_guest()
}

/// The guest, compiled once: every test's transports share the one compiled module, as every
/// deployment of an extension does in one process.
fn module() -> ControllerModule {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| Runtime::new().expect("the runtime starts"));
    runtime
        .load_controller(guest(), &format!("{:x}", Sha256::digest(guest())))
        .expect("the guest loads")
}

const AWS_EC2: &str = "https://ec2.us-east-1.amazonaws.com";
/// The project every transport here steps a deployment of, unless a test names another.
const PROJECT: &str = "proj";
const GCP_DNS: &str = "https://dns.googleapis.com";
const GCP_SQL: &str = "https://sqladmin.googleapis.com";
const AZURE: &str = "https://management.azure.com";

const AWS_SECRETS: [&str; 2] = ["AWSSECRETACCESSKEY", "AWSSESSIONTOKEN"];
const GCP_SECRET: &str = "GCPACCESSTOKEN";
const AZURE_SECRET: &str = "AZUREACCESSTOKEN";

fn aws_named(access_key_id: &str) -> Credentials {
    Credentials::Aws {
        account_id: "123456789012".into(),
        region: "us-east-1".into(),
        access_key_id: access_key_id.into(),
        secret_access_key: AWS_SECRETS[0].into(),
        session_token: Some(AWS_SECRETS[1].into()),
        expires_at: Some("2099-01-01T00:00:00Z".into()),
    }
}

fn aws() -> Credentials {
    aws_named("ASIAEXTENSION")
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

fn platform_of(credentials: &Option<Credentials>) -> &'static str {
    match credentials {
        Some(Credentials::Aws { .. }) => "aws",
        Some(Credentials::Gcp { .. }) => "gcp",
        Some(Credentials::Azure { .. }) => "azure",
        None => "kubernetes",
    }
}

/// A host whose requests for the clouds' API origins go to `stub`.
fn host_routed_to(stub: &Server) -> WasmHost {
    [AWS_EC2, GCP_DNS, GCP_SQL, AZURE]
        .iter()
        .fold(WasmHost::new().unwrap(), |host, origin| {
            host.routing(origin, &stub.base_url)
        })
}

fn chain_of(interceptor: TestInterceptor) -> InterceptorChain {
    InterceptorChain::new(vec![Arc::new(interceptor) as Arc<dyn CallInterceptor>])
}

fn transport_of(host: WasmHost) -> WasmTransport {
    Arc::new(host)
        .transport(PROJECT, "acme", module(), &[])
        .unwrap()
}

/// A stub that answers every request with `status` and `body`.
async fn answering(status: u16, body: &'static str) -> Server {
    capture::start(move |_| Reply::new(status, "text/plain", body)).await
}

fn step_request(
    deployment_id: &str,
    probe: &str,
    arg: Value,
    credentials: &Option<Credentials>,
) -> StepRequest {
    let mut request: StepRequest = serde_json::from_value(json!({
        "protocol": 1, "stepId": format!("step-{probe}"), "deploymentId": deployment_id,
        "resourceId": "probe", "resourceType": "acme/probe",
        "platform": platform_of(credentials), "resourcePrefix": "p",
        "initialSetupAuthority": "importedHandoff", "lifecycle": "live",
        "resource": { "type": "acme/probe", "id": "probe", "probe": probe, "arg": arg },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": {}, "setupRun": false, "setupRunId": null, "state": null,
        "transitions": [], "credentials": null, "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap();
    request.credentials = credentials.clone();
    request
}

async fn step(
    transport: &WasmTransport,
    request: &StepRequest,
    timeout: Duration,
) -> Result<StepResponse, TransportFailure> {
    transport.step(request, timeout).await
}

/// Runs one step of `probe` and returns what the module reported as its progress.
async fn probe(
    transport: &WasmTransport,
    credentials: Option<Credentials>,
    probe: &str,
    arg: Value,
) -> String {
    let request = step_request("dep_probe", probe, arg, &credentials);
    let response = step(transport, &request, Duration::from_secs(30))
        .await
        .unwrap_or_else(|failure| panic!("the {probe} step failed: {failure:?}"));
    progress(response)
}

fn progress(response: StepResponse) -> String {
    match response.body {
        ResponseBody::Stepped { controller, .. } => controller.progress.expect("a progress line"),
        other => panic!("expected a stepped reply, got {other:?}"),
    }
}

/// The response the module got from a call, parsed.
fn answered(progress: &str) -> Value {
    let reply = progress
        .strip_prefix("OK ")
        .unwrap_or_else(|| panic!("the call was not answered: {progress}"));
    serde_json::from_str(reply).expect("the host answers a call with JSON")
}

/// Why the host refused or failed a call.
fn failed(progress: &str) -> &str {
    progress
        .strip_prefix("ERR ")
        .unwrap_or_else(|| panic!("the call was answered: {progress}"))
}

fn call(cloud: &str, service: &str, method: &str, path: &str) -> Value {
    json!({ "cloud": cloud, "service": service, "method": method, "path": path })
}

fn header<'a>(response: &'a Value, name: &str) -> Option<&'a str> {
    response["headers"]
        .as_array()?
        .iter()
        .find(|pair| pair[0] == name)
        .and_then(|pair| pair[1].as_str())
}

fn assert_no_secret(text: &str) {
    for secret in AWS_SECRETS.iter().chain([GCP_SECRET, AZURE_SECRET].iter()) {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}

// ---------------------------------------------------------------- what a module gets

/// The module's step has no credentials, only where its calls go, and the sandbox gives it no
/// environment, file or socket to find any in.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_gets_the_step_without_its_credentials() {
    let transport = transport_of(WasmHost::new().unwrap());
    for (credentials, cloud) in [
        (
            aws(),
            json!({ "platform": "aws", "accountId": "123456789012", "region": "us-east-1" }),
        ),
        (
            gcp(),
            json!({ "platform": "gcp", "projectId": "the-project", "region": "us-central1" }),
        ),
        (
            azure(),
            json!({
                "platform": "azure", "subscriptionId": "the-subscription", "tenantId": "the-tenant"
            }),
        ),
    ] {
        let seen = probe(&transport, Some(credentials), "echo", Value::Null).await;
        assert_no_secret(&seen);
        assert!(!seen.contains("ASIAEXTENSION"), "{seen}");
        let seen: Value = serde_json::from_str(&seen).expect("the step is JSON");
        assert_eq!(seen["credentials"], Value::Null);
        assert_eq!(seen["cloud"], cloud);
        assert_eq!(seen["stepId"], "step-echo");
        // The deadline is the time the module has, not the request's own.
        let deadline = chrono::DateTime::parse_from_rfc3339(seen["deadline"].as_str().unwrap())
            .expect("an RFC 3339 deadline");
        let left = deadline.signed_duration_since(chrono::Utc::now());
        assert!(
            left > chrono::Duration::seconds(20) && left <= chrono::Duration::seconds(28),
            "{left}"
        );
    }

    let seen = probe(&transport, None, "echo", Value::Null).await;
    let seen: Value = serde_json::from_str(&seen).unwrap();
    assert_eq!(seen["credentials"], Value::Null);
    assert_eq!(seen["cloud"], Value::Null);

    let sandbox = probe(&transport, Some(aws()), "sandbox", Value::Null).await;
    assert!(sandbox.starts_with("env=0 args=0 "), "{sandbox}");
    assert_eq!(sandbox.matches("Err(").count(), 3, "{sandbox}");
    let random = probe(&transport, Some(aws()), "random", Value::Null).await;
    let random_again = probe(&transport, Some(aws()), "random", Value::Null).await;
    assert_ne!(random, random_again);
}

/// The host signs each cloud's calls with the step's credentials, to that cloud's endpoint,
/// and what the module reads back holds none of them.
#[tokio::test(flavor = "multi_thread")]
async fn the_host_signs_a_module_s_calls_with_the_step_s_credentials() {
    let stub = answering(200, "the cloud's answer").await;
    let transport = transport_of(host_routed_to(&stub));

    let mut describe_regions = call("aws", "ec2", "POST", "/");
    describe_regions["body"] = json!("Action=DescribeRegions&Version=2016-11-15");
    describe_regions["headers"] = json!([["content-type", "application/x-www-form-urlencoded"]]);
    let mut list_zones = call(
        "gcp",
        "dns",
        "GET",
        "/dns/v1/projects/the-project/managedZones",
    );
    list_zones["query"] = json!([["maxResults", "10"]]);
    let mut list_groups = call(
        "azure",
        "management",
        "GET",
        "/subscriptions/the-subscription/resourcegroups",
    );
    list_groups["query"] = json!([["api-version", "2021-04-01"]]);

    for (credentials, request) in [
        (aws(), describe_regions),
        (gcp(), list_zones),
        (azure(), list_groups),
    ] {
        let reported = probe(&transport, Some(credentials), "cloud", request).await;
        assert_no_secret(&reported);
        let response = answered(&reported);
        assert_eq!(response["status"], 200);
        assert_eq!(response["body"], "the cloud's answer");
    }

    let received = stub.requests();
    assert_eq!(received.len(), 3);
    let (to_aws, to_gcp, to_azure) = (&received[0], &received[1], &received[2]);

    assert_eq!(to_aws.method, "POST");
    assert_eq!(to_aws.target, "/");
    assert_eq!(to_aws.header("host"), Some("ec2.us-east-1.amazonaws.com"));
    let signature = to_aws.header("authorization").expect("a signature");
    assert!(
        signature.starts_with("AWS4-HMAC-SHA256 Credential=ASIAEXTENSION/")
            && signature.contains("/us-east-1/ec2/aws4_request"),
        "{signature}"
    );
    assert_eq!(to_aws.header("x-amz-security-token"), Some(AWS_SECRETS[1]));
    assert_eq!(
        to_aws.header("x-amz-content-sha256"),
        Some(format!("{:x}", Sha256::digest(&to_aws.body)).as_str())
    );
    assert_eq!(
        to_aws.body_text(),
        "Action=DescribeRegions&Version=2016-11-15"
    );

    assert_eq!(
        to_gcp.target,
        "/dns/v1/projects/the-project/managedZones?maxResults=10"
    );
    assert_eq!(
        to_gcp.header("authorization"),
        Some(format!("Bearer {GCP_SECRET}").as_str())
    );
    assert_eq!(
        to_azure.target,
        "/subscriptions/the-subscription/resourcegroups?api-version=2021-04-01"
    );
    assert_eq!(
        to_azure.header("authorization"),
        Some(format!("Bearer {AZURE_SECRET}").as_str())
    );
}

// ---------------------------------------------------------------- what a module can't reach

/// Whatever a module asks its host to send the step's credentials to, they go only to an API
/// endpoint of the step's own cloud: every other request is refused before anything is sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_cannot_send_the_step_s_credentials_anywhere_but_its_cloud_s_endpoints() {
    let stub = answering(200, "reached").await;
    let transport = transport_of(host_routed_to(&stub));

    let with = |mut request: Value, field: &str, value: Value| {
        request[field] = value;
        request
    };
    let ec2 = || call("aws", "ec2", "GET", "/");
    let dns = || {
        call(
            "gcp",
            "dns",
            "GET",
            "/dns/v1/projects/the-project/managedZones",
        )
    };
    let arm = || call("azure", "management", "GET", "/subscriptions");

    let refused: Vec<(Credentials, Value, &str)> = vec![
        // A host that is not an API endpoint, whatever its name ends with.
        (
            aws(),
            call("aws", "evil.example.com", "GET", "/"),
            "not an AWS endpoint prefix",
        ),
        (
            aws(),
            call("aws", "ec2-203-0-113-25.compute-1", "GET", "/"),
            "not an AWS endpoint prefix",
        ),
        (
            aws(),
            with(
                call("aws", "someones-bucket", "GET", "/"),
                "region",
                json!("s3"),
            ),
            "not an AWS endpoint prefix",
        ),
        (
            aws(),
            with(ec2(), "region", json!("x.evil.example.com/")),
            "not an AWS region",
        ),
        (
            gcp(),
            call("gcp", "dns.googleapis.com.evil.example.com", "GET", "/"),
            "not a Google Cloud API",
        ),
        (
            gcp(),
            call("gcp", "evil-googleapis", "GET", "/"),
            "not a Google Cloud API",
        ),
        (
            azure(),
            call("azure", "management.azure.com.evil.example.com", "GET", "/"),
            "Resource Manager",
        ),
        (
            azure(),
            call("azure", "vault", "GET", "/"),
            "Resource Manager",
        ),
        // An address, the metadata service among them.
        (
            aws(),
            call("aws", "169.254.169.254", "GET", "/latest/meta-data/"),
            "not an AWS endpoint prefix",
        ),
        (
            aws(),
            with(ec2(), "region", json!("169.254.169.254")),
            "not an AWS region",
        ),
        (
            aws(),
            call("aws", "127.0.0.1", "GET", "/"),
            "not an AWS endpoint prefix",
        ),
        (
            aws(),
            call("aws", "[fd00:ec2::254]", "GET", "/"),
            "not an AWS endpoint prefix",
        ),
        (
            gcp(),
            call("gcp", "metadata.google.internal", "GET", "/"),
            "not a Google Cloud API",
        ),
        (
            gcp(),
            call("gcp", "2130706433", "GET", "/"),
            "not a Google Cloud API",
        ),
        (
            azure(),
            call("azure", "169.254.169.254", "GET", "/metadata/identity"),
            "Resource Manager",
        ),
        // Another cloud's endpoint.
        (aws(), dns(), "the step's credentials are for aws"),
        (aws(), arm(), "the step's credentials are for aws"),
        (gcp(), ec2(), "the step's credentials are for gcp"),
        (azure(), ec2(), "the step's credentials are for azure"),
        (
            gcp(),
            call("gcp", "sts.us-east-1.amazonaws", "GET", "/"),
            "not a Google Cloud API",
        ),
        // A path that names another host.
        (
            aws(),
            call("aws", "ec2", "GET", "https://evil.example.com/"),
            "must start with a single '/'",
        ),
        (
            aws(),
            call("aws", "ec2", "GET", "//evil.example.com/"),
            "must start with a single '/'",
        ),
        (
            gcp(),
            call("gcp", "dns", "GET", "/\\evil.example.com"),
            "backslash",
        ),
        (
            aws(),
            call("aws", "ec2", "GET", "/x\r\nHost: evil.example.com"),
            "control character",
        ),
        // A credential or another account in the query.
        (
            aws(),
            with(ec2(), "query", json!([["X-Amz-Security-Token", "theirs"]])),
            "X-Amz-Security-Token",
        ),
        (
            aws(),
            with(
                ec2(),
                "query",
                json!([
                    ["X-Amz-Credential", "AKIATHEIRS/x"],
                    ["X-Amz-Signature", "0"]
                ]),
            ),
            "X-Amz-Credential",
        ),
        (
            gcp(),
            with(dns(), "query", json!([["access_token", "theirs"]])),
            "access_token",
        ),
        (
            gcp(),
            with(dns(), "query", json!([["oauth_token", "theirs"]])),
            "oauth_token",
        ),
        (
            gcp(),
            with(dns(), "query", json!([["key", "theirs"]])),
            "key",
        ),
        (
            gcp(),
            with(dns(), "query", json!([["$key", "theirs"]])),
            "$key",
        ),
        (
            gcp(),
            with(dns(), "query", json!([["userProject", "their-project"]])),
            "userProject",
        ),
        (
            gcp(),
            with(dns(), "query", json!([["$userProject", "their-project"]])),
            "$userProject",
        ),
        // A credential, another identity or another destination in a header.
        (
            aws(),
            with(
                ec2(),
                "headers",
                json!([["Authorization", "AWS4-HMAC-SHA256 Credential=AKIATHEIRS"]]),
            ),
            "Authorization",
        ),
        (
            aws(),
            with(
                ec2(),
                "headers",
                json!([["X-Amz-Security-Token", "theirs"]]),
            ),
            "X-Amz-Security-Token",
        ),
        (
            aws(),
            with(ec2(), "headers", json!([["Host", "evil.example.com"]])),
            "Host",
        ),
        (
            gcp(),
            with(
                dns(),
                "headers",
                json!([["authorization", "Bearer theirs"]]),
            ),
            "authorization",
        ),
        (
            gcp(),
            with(
                dns(),
                "headers",
                json!([["x-goog-user-project", "their-project"]]),
            ),
            "x-goog-user-project",
        ),
        (
            azure(),
            with(
                arm(),
                "headers",
                json!([["x-ms-authorization-auxiliary", "Bearer theirs"]]),
            ),
            "x-ms-authorization-auxiliary",
        ),
        (
            azure(),
            with(
                arm(),
                "headers",
                json!([["X-Forwarded-Host", "evil.example.com"]]),
            ),
            "X-Forwarded-Host",
        ),
        // A field the request does not have.
        (
            aws(),
            with(ec2(), "url", json!("https://evil.example.com/")),
            "not a cloud request",
        ),
        (
            aws(),
            with(ec2(), "host", json!("evil.example.com")),
            "not a cloud request",
        ),
    ];
    for (credentials, request, reason) in refused {
        let reported = probe(&transport, Some(credentials), "cloud", request.clone()).await;
        let refusal = failed(&reported);
        assert!(refusal.starts_with("refused: "), "{request}: {refusal}");
        assert!(refusal.contains(reason), "{request}: {refusal}");
        assert_no_secret(&reported);
    }
    assert!(
        stub.requests().is_empty(),
        "a refused request reached a server: {:?}",
        stub.requests()
    );

    // A step with no credentials sends nothing, and the same stub is reached by a request
    // that passes.
    let reported = probe(&transport, None, "cloud", ec2()).await;
    assert!(
        failed(&reported).contains("no cloud credentials"),
        "{reported}"
    );
    assert!(stub.requests().is_empty());
    let reported = probe(&transport, Some(aws()), "cloud", ec2()).await;
    assert_eq!(answered(&reported)["body"], "reached");
    assert_eq!(stub.requests().len(), 1);
}

/// A cloud's redirect is the module's to read: the host follows none, so the signature or the
/// token goes nowhere the endpoint points.
#[tokio::test(flavor = "multi_thread")]
async fn a_redirect_is_returned_to_the_module_and_not_followed() {
    let elsewhere = answering(200, "the credentials arrived elsewhere").await;
    let location = format!("{}/elsewhere", elsewhere.base_url);
    let redirecting = {
        let location = location.clone();
        capture::start(move |_| Reply::new(307, "text/plain", "").header("location", &location))
            .await
    };
    let host = host_routed_to(&redirecting).allowing_local_egress();
    let transport = Arc::new(host)
        .transport(
            PROJECT,
            "acme",
            module(),
            std::slice::from_ref(&redirecting.base_url),
        )
        .unwrap();

    for (credentials, request) in [
        (Some(aws()), call("aws", "ec2", "POST", "/")),
        (
            Some(gcp()),
            call(
                "gcp",
                "dns",
                "GET",
                "/dns/v1/projects/the-project/managedZones",
            ),
        ),
        (
            Some(azure()),
            call("azure", "management", "GET", "/subscriptions"),
        ),
    ] {
        let reported = probe(&transport, credentials, "cloud", request).await;
        let response = answered(&reported);
        assert_eq!(response["status"], 307, "{reported}");
        assert_eq!(header(&response, "location"), Some(location.as_str()));
    }
    let to_origin = json!({ "method": "GET", "url": format!("{}/v1", redirecting.base_url) });
    let reported = probe(&transport, Some(aws()), "http", to_origin).await;
    assert_eq!(answered(&reported)["status"], 307, "{reported}");

    assert_eq!(redirecting.requests().len(), 4);
    assert!(
        elsewhere.requests().is_empty(),
        "a redirect was followed: {:?}",
        elsewhere.requests()
    );
}

/// A cloud's error can quote the request it refused: AWS quotes the signed headers, the session
/// token among them, when a signature does not match. The module reads the answer without what
/// the host attached.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_that_quotes_the_credential_reaches_the_module_without_it() {
    let quoting = capture::start(|request: &Captured| {
        let quoted: String = request
            .headers
            .iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect();
        Reply::new(403, "text/xml", quoted.clone()).header("x-quoted", &quoted.replace('\n', " "))
    })
    .await;
    let transport = transport_of(host_routed_to(&quoting));

    for (credentials, request) in [
        (Some(aws()), call("aws", "ec2", "POST", "/")),
        (
            Some(gcp()),
            call(
                "gcp",
                "dns",
                "GET",
                "/dns/v1/projects/the-project/managedZones",
            ),
        ),
        (
            Some(azure()),
            call("azure", "management", "GET", "/subscriptions"),
        ),
    ] {
        let reported = probe(&transport, credentials, "cloud", request).await;
        let response = answered(&reported);
        assert_eq!(response["status"], 403);
        assert!(
            response["body"].as_str().unwrap().contains("[redacted]"),
            "{reported}"
        );
        assert_no_secret(&reported);
    }
    // The stub did get them: the answer quoted what was really sent.
    let received = quoting.requests();
    assert_eq!(
        received[0].header("x-amz-security-token"),
        Some(AWS_SECRETS[1])
    );
}

/// An interceptor's response hook reads the answer without the credentials, too: a hook could
/// otherwise encode one into what it returns, where the module's clearing would not find it.
#[tokio::test(flavor = "multi_thread")]
async fn a_response_hook_reads_an_answer_that_quotes_the_credential_without_it() {
    let quoting = capture::start(|request: &Captured| {
        let quoted: String = request
            .headers
            .iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect();
        Reply::new(403, "text/xml", quoted.clone()).header("x-quoted", &quoted.replace('\n', " "))
    })
    .await;
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let interceptors = {
        let seen = seen.clone();
        move |cloud: Cloud, _tenant: Option<&str>| {
            let service = match cloud {
                Cloud::Aws => "ec2",
                Cloud::Gcp => "dns",
                Cloud::Azure => "management",
                Cloud::Kubernetes => "kubernetes",
            };
            let seen = seen.clone();
            chain_of(TestInterceptor::new("reader", cloud, service).on_response(
                move |_, response| {
                    seen.lock()
                        .unwrap()
                        .push(format!("{:?} {}", response.headers, response.body));
                    Ok(())
                },
            ))
        }
    };
    let transport =
        transport_of(host_routed_to(&quoting)).with_interceptors(Arc::new(interceptors));

    for (credentials, request) in [
        (Some(aws()), call("aws", "ec2", "POST", "/")),
        (
            Some(gcp()),
            call(
                "gcp",
                "dns",
                "GET",
                "/dns/v1/projects/the-project/managedZones",
            ),
        ),
        (
            Some(azure()),
            call("azure", "management", "GET", "/subscriptions"),
        ),
    ] {
        probe(&transport, credentials, "cloud", request).await;
    }
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3, "{seen:?}");
    for view in seen.iter() {
        assert!(view.contains("[redacted]"), "{view}");
        assert_no_secret(view);
    }
}

// ---------------------------------------------------------------- Kubernetes

/// A module has no way to a cluster's API: a request for it is refused, with or without cloud
/// credentials in the step, and nothing is sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_kubernetes_request_is_refused() {
    let stub = answering(200, "reached").await;
    let transport = transport_of(host_routed_to(&stub));
    for credentials in [Some(aws()), None] {
        let reported = probe(
            &transport,
            credentials,
            "cloud",
            call("kubernetes", "kubernetes", "GET", "/api/v1/namespaces"),
        )
        .await;
        assert!(failed(&reported).starts_with("refused: "), "{reported}");
    }
    assert!(stub.requests().is_empty());
}

// ---------------------------------------------------------------- declared origins

/// A request through `alien_http_v1::send` reaches only an origin the extension declared, and
/// carries nothing of the deployment's: only what the module itself put on it.
#[tokio::test(flavor = "multi_thread")]
async fn declared_egress_reaches_only_declared_origins_and_carries_no_credential() {
    let vendor = answering(200, "the vendor's answer").await;
    let undeclared = answering(200, "undeclared").await;
    let host = Arc::new(WasmHost::new().unwrap().allowing_local_egress());
    let transport = host
        .transport(
            PROJECT,
            "acme",
            module(),
            std::slice::from_ref(&vendor.base_url),
        )
        .unwrap();

    let mut request = json!({
        "method": "POST", "url": format!("{}/v1/zones?x=1", vendor.base_url),
        "headers": [["content-type", "application/json"]], "body": "{\"zone\":\"a\"}"
    });
    let reported = probe(&transport, Some(aws()), "http", request.clone()).await;
    assert_eq!(answered(&reported)["body"], "the vendor's answer");
    request["headers"] = json!([["Authorization", "Bearer the-vendor's-own-key"]]);
    probe(&transport, Some(aws()), "http", request).await;

    let received = vendor.requests();
    assert_eq!(received.len(), 2);
    assert_eq!(received[0].target, "/v1/zones?x=1");
    assert_eq!(received[0].body_text(), "{\"zone\":\"a\"}");
    assert!(received[0].header("authorization").is_none());
    assert!(received[0].header("x-amz-security-token").is_none());
    assert_eq!(
        received[1].header("authorization"),
        Some("Bearer the-vendor's-own-key")
    );
    for request in &received {
        let sent = format!("{:?} {}", request.headers, request.body_text());
        assert_no_secret(&sent);
        assert!(!sent.contains("ASIAEXTENSION"), "{sent}");
    }

    let port = reqwest::Url::parse(&vendor.base_url)
        .unwrap()
        .port()
        .unwrap();
    for (url, reason) in [
        (
            format!("{}/v1", undeclared.base_url),
            "not an origin the extension declares",
        ),
        (
            format!("http://localhost:{port}/v1"),
            "not an origin the extension declares",
        ),
        (
            format!("http://user:pw@127.0.0.1:{port}/v1"),
            "carries credentials",
        ),
        (
            "https://evil.example.com/".to_string(),
            "not an origin the extension declares",
        ),
        (
            "https://169.254.169.254/latest/meta-data/".to_string(),
            "not an origin the extension declares",
        ),
        ("file:///etc/passwd".to_string(), "not https"),
    ] {
        let reported = probe(
            &transport,
            Some(aws()),
            "http",
            json!({ "method": "GET", "url": url }),
        )
        .await;
        let refusal = failed(&reported);
        assert!(refusal.starts_with("refused: "), "{url}: {refusal}");
        assert!(refusal.contains(reason), "{url}: {refusal}");
    }
    assert!(undeclared.requests().is_empty());
    assert_eq!(vendor.requests().len(), 2);

    // An extension that declares no origin reaches none.
    let closed = host.transport(PROJECT, "acme", module(), &[]).unwrap();
    let reported = probe(
        &closed,
        Some(aws()),
        "http",
        json!({ "method": "GET", "url": format!("{}/v1", vendor.base_url) }),
    )
    .await;
    assert!(failed(&reported).contains("not an origin the extension declares"));
    assert_eq!(vendor.requests().len(), 2);
}

/// A declared origin is reached only at a public address: one that is, or resolves to, a
/// private or loopback address, or the metadata service, is refused when the request is sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_declared_origin_at_a_private_address_is_refused() {
    let listening = answering(200, "reached a private address").await;
    let port = reqwest::Url::parse(&listening.base_url)
        .unwrap()
        .port()
        .unwrap();
    let origins = [
        format!("https://127.0.0.1:{port}"),
        format!("https://localhost:{port}"),
        "https://169.254.169.254".to_string(),
        "https://10.0.0.1".to_string(),
        "https://[fd00:ec2::254]".to_string(),
    ];
    let host = Arc::new(WasmHost::new().unwrap());
    let transport = host.transport(PROJECT, "acme", module(), &origins).unwrap();
    for origin in &origins {
        let reported = probe(
            &transport,
            Some(aws()),
            "http",
            json!({ "method": "GET", "url": format!("{origin}/latest/meta-data/") }),
        )
        .await;
        let refusal = failed(&reported);
        assert!(
            refusal.starts_with("refused: ") && refusal.contains("not a public address"),
            "{origin}: {refusal}"
        );
    }
    assert!(listening.requests().is_empty());

    // What an extension declares is an https origin and nothing more.
    for declared in [
        "http://api.acme.dev",
        "https://api.acme.dev/v1",
        "https://*.acme.dev",
        "api.acme.dev",
    ] {
        let error = host
            .transport(PROJECT, "acme", module(), &[declared.to_string()])
            .expect_err(declared);
        assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED");
    }
}

// ---------------------------------------------------------------- bodies and limits

/// Bytes that are not text cross both ways unchanged, in base64.
#[tokio::test(flavor = "multi_thread")]
async fn binary_bodies_cross_both_ways_unchanged() {
    let object: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
    let stub = {
        let object = object.clone();
        capture::start(move |_| Reply::new(200, "application/octet-stream", object.clone())).await
    };
    let transport = transport_of(
        WasmHost::new()
            .unwrap()
            .routing("https://s3.us-east-1.amazonaws.com", &stub.base_url),
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(&object);

    let mut put = call("aws", "s3", "PUT", "/bucket/key");
    put["bodyBase64"] = json!(encoded);
    let reported = probe(&transport, Some(aws()), "cloud", put.clone()).await;
    let response = answered(&reported);
    assert_eq!(response["body"], "");
    assert_eq!(response["bodyBase64"], json!(encoded));

    let received = stub.requests();
    assert_eq!(received[0].body, object);
    assert_eq!(
        received[0].header("x-amz-content-sha256"),
        Some(format!("{:x}", Sha256::digest(&object)).as_str())
    );

    put["body"] = json!("and text");
    let reported = probe(&transport, Some(aws()), "cloud", put).await;
    assert!(failed(&reported).contains("not both"), "{reported}");
    let mut oversized = call("aws", "s3", "PUT", "/bucket/key");
    oversized["body"] = json!("x".repeat(MAX_REQUEST_BODY + 1));
    let reported = probe(&transport, Some(aws()), "cloud", oversized).await;
    assert!(failed(&reported).contains("larger than"), "{reported}");
    assert_eq!(stub.requests().len(), 1);
}

/// The host reads a response only up to its limit, with or without an interceptor on the call:
/// an interceptor's hook reads a whole body, so the limit comes first.
#[tokio::test(flavor = "multi_thread")]
async fn a_response_over_the_limit_fails_the_call() {
    let oversized = capture::start(|request: &Captured| {
        let size = match request.target.as_str() {
            "/fits" => MAX_RESPONSE_BYTES,
            _ => MAX_RESPONSE_BYTES + 1,
        };
        Reply::new(200, "text/plain", vec![b'x'; size])
    })
    .await;
    let host = Arc::new(host_routed_to(&oversized));
    let plain = host.transport(PROJECT, "acme", module(), &[]).unwrap();
    let asked = Arc::new(AtomicUsize::new(0));
    let intercepted = {
        let asked = asked.clone();
        plain.clone().with_interceptors(Arc::new(move |cloud, _| {
            let asked = asked.clone();
            chain_of(
                TestInterceptor::new("observer", cloud, "ec2").on_response(move |_, _| {
                    asked.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }),
            )
        }))
    };

    for transport in [&plain, &intercepted] {
        let reported = probe(
            transport,
            Some(aws()),
            "cloud",
            call("aws", "ec2", "GET", "/over"),
        )
        .await;
        let refusal = failed(&reported);
        assert!(
            refusal.contains(&format!("larger than {MAX_RESPONSE_BYTES} bytes")),
            "{refusal}"
        );
    }
    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "an interceptor was handed a response over the limit"
    );

    // A response of exactly the limit is read, and the interceptor sees it.
    let reported = probe(
        &intercepted,
        Some(aws()),
        "cloud",
        call("aws", "ec2", "GET", "/fits"),
    )
    .await;
    assert!(reported.starts_with("OK "), "{}", &reported[..64]);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_step_sends_only_so_many_requests() {
    let stub = answering(200, "ok").await;
    let transport = transport_of(host_routed_to(&stub));
    let reported = probe(
        &transport,
        Some(aws()),
        "cloud-many",
        json!({ "count": CALLS_PER_STEP + 10, "request": call("aws", "ec2", "GET", "/") }),
    )
    .await;
    assert_eq!(
        reported,
        format!("ERR after {CALLS_PER_STEP}: refused: a step may send {CALLS_PER_STEP} requests")
    );
    assert_eq!(stub.requests().len(), CALLS_PER_STEP);
}

// ---------------------------------------------------------------- interceptors

/// The deployment's interceptors run on a module's calls as on Alien's own: on the request
/// before it is signed, without its credentials, under the service name Alien's own client for
/// the API uses, and on the response before the module reads it.
#[tokio::test(flavor = "multi_thread")]
async fn the_deployment_s_interceptors_run_on_a_module_s_calls() {
    let stub = answering(200, "from the cloud").await;
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let tenants: Arc<Mutex<Vec<(Cloud, Option<String>)>>> = Arc::default();
    let interceptors = {
        let (seen, tenants) = (seen.clone(), tenants.clone());
        move |cloud: Cloud, tenant: Option<&str>| {
            tenants
                .lock()
                .unwrap()
                .push((cloud, tenant.map(str::to_string)));
            // Alien's own Cloud SQL client goes by `cloudsql`, on sqladmin.googleapis.com.
            let service = match cloud {
                Cloud::Aws => "ec2",
                Cloud::Gcp => "cloudsql",
                Cloud::Azure => "management",
                Cloud::Kubernetes => "kubernetes",
            };
            let seen = seen.clone();
            chain_of(
                TestInterceptor::new("tagger", cloud, service)
                    .on_request(move |request| {
                        seen.lock().unwrap().push(format!(
                            "{} {} {:?}",
                            request.cloud, request.service, request.headers
                        ));
                        request
                            .headers
                            .insert("x-amz-meta-tagged", "by-the-interceptor".parse().unwrap());
                        Ok(())
                    })
                    .on_response(|_, response| {
                        response.body = format!("{} and the interceptor", response.body);
                        Ok(())
                    }),
            )
        }
    };
    let transport = transport_of(host_routed_to(&stub)).with_interceptors(Arc::new(interceptors));

    for (credentials, request) in [
        (Some(aws()), call("aws", "ec2", "POST", "/")),
        (
            Some(gcp()),
            call(
                "gcp",
                "sqladmin",
                "GET",
                "/sql/v1beta4/projects/the-project/instances",
            ),
        ),
        (
            Some(azure()),
            call("azure", "management", "GET", "/subscriptions"),
        ),
    ] {
        let reported = probe(&transport, credentials, "cloud", request).await;
        assert_eq!(
            answered(&reported)["body"],
            "from the cloud and the interceptor"
        );
    }

    let received = stub.requests();
    assert_eq!(received.len(), 3);
    for request in &received {
        assert_eq!(
            request.header("x-amz-meta-tagged"),
            Some("by-the-interceptor")
        );
    }
    // The signature covers what the interceptor added: it ran before the request was signed.
    let signature = received[0].header("authorization").unwrap();
    assert!(signature.contains("x-amz-meta-tagged"), "{signature}");

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    for (view, service) in seen
        .iter()
        .zip(["aws ec2", "gcp cloudsql", "azure management"])
    {
        assert!(view.starts_with(service), "{view}");
        assert!(!view.contains("authorization"), "{view}");
        assert_no_secret(view);
    }
    assert_eq!(
        *tenants.lock().unwrap(),
        [
            (Cloud::Aws, None),
            (Cloud::Gcp, None),
            (Cloud::Azure, Some("the-tenant".to_string())),
        ]
    );
}

/// An interceptor that fails, or that moves the call, stops the call before it is sent.
#[tokio::test(flavor = "multi_thread")]
async fn an_interceptor_s_refusal_stops_a_module_s_call() {
    let stub = answering(200, "reached").await;
    let moving = |cloud: Cloud, _: Option<&str>| {
        chain_of(
            TestInterceptor::new("mover", cloud, "ec2").on_request(|request| {
                request.url = "https://evil.example.com/".parse().unwrap();
                Ok(())
            }),
        )
    };
    let transport = transport_of(host_routed_to(&stub)).with_interceptors(Arc::new(moving));
    let reported = probe(
        &transport,
        Some(aws()),
        "cloud",
        call("aws", "ec2", "POST", "/"),
    )
    .await;
    let refusal = failed(&reported);
    assert!(refusal.starts_with("refused: "), "{refusal}");
    assert!(refusal.contains("mover"), "{refusal}");
    assert!(stub.requests().is_empty());
}

/// A module can't reach an identity service, whatever credentials sign its calls: the host
/// refuses the call and nothing is sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_cannot_call_identity_services() {
    let stub = answering(200, "reached").await;
    let host = host_routed_to(&stub).routing("https://iam.amazonaws.com", &stub.base_url);
    let transport = transport_of(host);
    let mut create_key = call("aws", "iam", "POST", "/");
    create_key["body"] = json!("Action=CreateAccessKey&Version=2010-05-08");
    let reported = probe(&transport, Some(aws()), "cloud", create_key).await;
    let refusal = failed(&reported);
    assert!(refusal.starts_with("refused: "), "{refusal}");
    assert!(refusal.contains("identity"), "{refusal}");
    assert!(stub.requests().is_empty());
}

/// A module's call the deployment's interceptors turned into an identity call, or moved to
/// another subscription, is refused and nothing is sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_s_call_an_interceptor_changed_is_checked_again() {
    const SECRET_MANAGER: &str = "https://secretmanager.googleapis.com";
    let stub = answering(200, "reached").await;
    let rewriting = |cloud: Cloud, _: Option<&str>| {
        match cloud {
        Cloud::Azure => chain_of(
            TestInterceptor::new("rewriter", cloud, "management").on_request(|request| {
                let path = match request.url.path() {
                    "/subscriptions/the-subscription/resourcegroups/g" => "/subscriptions/the-subscription/resourcegroups/g/providers/Microsoft.Authorization/roleAssignments/a",
                    _ => "/subscriptions/another-subscription/resourcegroups/g",
                };
                request.url.set_path(path);
                request.method = reqwest::Method::PUT;
                Ok(())
            }),
        ),
        _ => chain_of(
            TestInterceptor::new("rewriter", cloud, "secretmanager").on_request(|request| {
                let path = format!("{}:setIamPolicy", request.url.path());
                request.url.set_path(&path);
                request.method = reqwest::Method::POST;
                Ok(())
            }),
        ),
    }
    };
    let host = host_routed_to(&stub).routing(SECRET_MANAGER, &stub.base_url);
    let transport = transport_of(host).with_interceptors(Arc::new(rewriting));
    for (credentials, request, reason) in [
        (
            azure(),
            call(
                "azure",
                "management",
                "GET",
                "/subscriptions/the-subscription/resourcegroups/g",
            ),
            "identity",
        ),
        (
            gcp(),
            call(
                "gcp",
                "secretmanager",
                "GET",
                "/v1/projects/the-project/secrets/s",
            ),
            "identity",
        ),
        (
            azure(),
            call(
                "azure",
                "management",
                "GET",
                "/subscriptions/the-subscription/resourcegroups/other",
            ),
            "which resource the call acts on",
        ),
    ] {
        let reported = probe(&transport, Some(credentials), "cloud", request).await;
        let refusal = failed(&reported);
        assert!(refusal.starts_with("refused: "), "{refusal}");
        assert!(refusal.contains(reason), "{refusal}");
    }
    assert!(stub.requests().is_empty(), "{:?}", stub.requests());
}

/// A step whose caller gives up while the deployment's interceptors run on its call sends
/// nothing: the call is refused when they return.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_whose_step_was_given_up_during_its_interceptors_is_not_sent() {
    let stub = answering(200, "reached").await;
    let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let holding = {
        let (entered, release) = (entered.clone(), release.clone());
        move |cloud: Cloud, _: Option<&str>| {
            let (entered, release) = (entered.clone(), release.clone());
            chain_of(
                TestInterceptor::new("slow", cloud, "ec2").on_request(move |_| {
                    entered.store(true, Ordering::SeqCst);
                    // The hook runs on the step's own thread, which nothing else needs.
                    while !release.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Ok(())
                }),
            )
        }
    };
    let host = Arc::new(
        WasmHost::new()
            .unwrap()
            .routing(AWS_EC2, &stub.base_url)
            .with_steps_per_deployment(1),
    );
    let transport = host
        .transport(PROJECT, "acme", module(), &[])
        .unwrap()
        .with_interceptors(Arc::new(holding));
    let given_up = {
        let transport = transport.clone();
        tokio::spawn(async move {
            let request = step_request(
                "dep_given_up",
                "cloud",
                call("aws", "ec2", "GET", "/given-up"),
                &Some(aws()),
            );
            step(&transport, &request, Duration::from_secs(60)).await
        })
    };
    tokio::time::timeout(Duration::from_secs(30), async {
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the interceptor runs on the call");
    given_up.abort();
    assert!(given_up.await.unwrap_err().is_cancelled());
    release.store(true, Ordering::SeqCst);

    // The deployment's one slot is free once the module has stopped.
    let next = spawn_call(&transport, "dep_given_up", "/next");
    let next = tokio::time::timeout(Duration::from_secs(30), next)
        .await
        .expect("the next step runs once the module stopped")
        .unwrap();
    assert_eq!(next, "reached");
    let sent: Vec<String> = stub.requests().into_iter().map(|r| r.target).collect();
    assert_eq!(sent, vec!["/next".to_string()]);
}

/// An interceptor's failure before the call is sent reaches the module as a call it may try
/// again when the interceptor marked it retryable, as a hook stopped at its deadline is, and as
/// a refusal otherwise.
#[tokio::test(flavor = "multi_thread")]
async fn an_interceptor_s_retryable_failure_reaches_the_module_as_a_failed_call() {
    let stub = answering(200, "reached").await;
    for retryable in [true, false] {
        let failing = move |cloud: Cloud, _: Option<&str>| {
            let service = match cloud {
                Cloud::Aws => "ec2",
                _ => "dns",
            };
            chain_of(
                TestInterceptor::new("slow", cloud, service).on_request(move |_| {
                    let mut error = AlienError::new(ClientErrorData::InterceptorFailed {
                        extension: "slow".into(),
                        message: "the hook reached its deadline".into(),
                    });
                    error.retryable = retryable;
                    Err(error)
                }),
            )
        };
        let transport = transport_of(host_routed_to(&stub)).with_interceptors(Arc::new(failing));
        for (credentials, request) in [
            (aws(), call("aws", "ec2", "POST", "/")),
            (
                gcp(),
                call(
                    "gcp",
                    "dns",
                    "GET",
                    "/dns/v1/projects/the-project/managedZones",
                ),
            ),
        ] {
            let reported = probe(&transport, Some(credentials), "cloud", request).await;
            let reason = failed(&reported);
            let expected = if retryable { "failed: " } else { "refused: " };
            assert!(reason.starts_with(expected), "{retryable}: {reason}");
            assert!(reason.contains("slow"), "{reason}");
        }
    }
    assert!(stub.requests().is_empty());
}

/// A response an interceptor refuses belongs to a call that was sent: the module reads that it
/// was, and the step that fails on it is not sent again to repeat the call.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_response_reads_as_a_sent_call_and_fails_the_step_for_good() {
    let stub = answering(200, "created").await;
    let refusing = |cloud: Cloud, _: Option<&str>| {
        chain_of(
            TestInterceptor::new("checker", cloud, "ec2").on_response(|_, _| {
                Err(AlienError::new(ClientErrorData::InterceptorFailed {
                    extension: "checker".into(),
                    message: "the answer is not one it accepts".into(),
                }))
            }),
        )
    };
    let transport = transport_of(host_routed_to(&stub)).with_interceptors(Arc::new(refusing));
    let reported = probe(
        &transport,
        Some(aws()),
        "cloud",
        call("aws", "ec2", "POST", "/"),
    )
    .await;
    let reason = failed(&reported);
    assert!(reason.starts_with("failed: "), "{reason}");
    assert!(reason.contains("was sent"), "{reason}");
    assert!(reason.contains("checker"), "{reason}");

    // The module fails its step and says it may be tried again; the host keeps it final.
    let request = step_request(
        "dep_refused_response",
        "cloud-then-fail",
        json!({ "request": call("aws", "ec2", "POST", "/"), "retryable": true }),
        &Some(aws()),
    );
    let response = step(&transport, &request, Duration::from_secs(30))
        .await
        .expect("the module answers");
    match response.body {
        ResponseBody::Failed { error, .. } => {
            assert_eq!(error.code, "PROBE_FAILED");
            assert!(!error.retryable, "{error:?}");
        }
        other => panic!("expected a failed reply, got {other:?}"),
    }
    assert_eq!(stub.requests().len(), 2);

    // A module that traps after it is no different: a trap is otherwise retried.
    let request = step_request(
        "dep_refused_response",
        "cloud-then",
        json!({ "request": call("aws", "ec2", "POST", "/"), "then": "panic" }),
        &Some(aws()),
    );
    let failure = step(&transport, &request, Duration::from_secs(30))
        .await
        .expect_err("the module traps");
    assert_eq!(failure.code, codes::UNREACHABLE);
    assert!(!failure.retryable, "{}", failure.message);
    assert_eq!(stub.requests().len(), 3);
}

// ---------------------------------------------------------------- a module that misbehaves

/// A module that never answers is stopped, and its step fails in a way the executor retries.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_that_does_not_answer_is_stopped_and_its_step_retried() {
    let transport = transport_of(WasmHost::new().unwrap());
    let run = |transport: WasmTransport, probe: &'static str, timeout: Duration| async move {
        let request = step_request("dep_stopped", probe, Value::Null, &Some(aws()));
        let started = Instant::now();
        let failure = step(&transport, &request, timeout)
            .await
            .expect_err("the step fails");
        (failure, started.elapsed())
    };

    // The step's timeout is the module's deadline plus the host's two seconds of grace. With
    // fuel to spare, the deadline is what stops a module that loops.
    let patient = transport_of(WasmHost::new().unwrap().with_step_fuel(u64::MAX));
    let (failure, took) = run(patient, "loop", Duration::from_secs(3)).await;
    assert_eq!(failure.code, codes::STEP_DEADLINE);
    assert!(failure.retryable);
    assert!(failure.message.contains("deadline"), "{}", failure.message);
    // A deadline is counted in the runtime's 1 ms ticks, so the module may stop up to a tick
    // early.
    assert!(
        took >= Duration::from_millis(2_998) && took < Duration::from_secs(10),
        "{took:?}"
    );

    // With little fuel, a step that only computes is stopped long before its deadline.
    let frugal = transport_of(WasmHost::new().unwrap().with_step_fuel(50_000_000));
    let (failure, took) = run(frugal.clone(), "loop", Duration::from_secs(120)).await;
    assert_eq!(failure.code, codes::STEP_DEADLINE);
    // The same step spends the same fuel again, so it is not retried.
    assert!(!failure.retryable);
    assert!(
        failure.message.contains("computation"),
        "{}",
        failure.message
    );
    assert!(took < Duration::from_secs(60), "{took:?}");
    // The same fuel is plenty for a step that answers.
    assert_eq!(
        probe(&frugal, Some(aws()), "noop", Value::Null).await,
        "noop"
    );

    let (failure, _) = run(transport.clone(), "alloc", Duration::from_secs(30)).await;
    assert_eq!(failure.code, codes::UNREACHABLE);
    assert!(failure.retryable);
    assert!(failure.message.contains("memory"), "{}", failure.message);

    let (failure, _) = run(transport.clone(), "panic", Duration::from_secs(30)).await;
    assert_eq!(failure.code, codes::UNREACHABLE);
    assert!(failure.retryable);
    assert!(
        failure.message.contains("the step panicked on purpose"),
        "{}",
        failure.message
    );

    // The host keeps stepping after each of them.
    assert_eq!(
        probe(&transport, Some(aws()), "noop", Value::Null).await,
        "noop"
    );
}

/// A reply that is not a step reply fails the step for good, and what the module says about it
/// is kept only up to a limit: it is saved with the deployment.
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_that_is_no_step_reply_fails_the_step_for_good() {
    let transport = transport_of(WasmHost::new().unwrap());
    for (probe, arg, message) in [
        ("garbage", Value::Null, "not JSON"),
        (
            "no-reply",
            json!("it could not answer. "),
            "could not answer the step",
        ),
        ("big-reply", json!(17 << 20), "more than 16 MiB"),
    ] {
        let request = step_request("dep_reply", probe, arg, &None);
        let failure = step(&transport, &request, Duration::from_secs(30))
            .await
            .expect_err(probe);
        assert_eq!(failure.code, codes::PROTOCOL_ERROR, "{probe}");
        assert!(!failure.retryable, "{probe}");
        assert!(failure.message.contains(message), "{}", failure.message);
        assert!(failure.message.len() < 1_300, "{}", failure.message.len());
    }
    let request = step_request("dep_reply", "noop", Value::Null, &None);
    let no_time = step(&transport, &request, Duration::from_secs(2))
        .await
        .expect_err("a timeout within the grace leaves the module no time");
    assert_eq!(no_time.code, codes::PROTOCOL_ERROR);
}

/// A call can't outlast the step: one the cloud never answers ends at the module's deadline,
/// so the module can still answer within the host's grace.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_the_cloud_never_answers_ends_at_the_step_s_deadline() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent = format!("http://{}", listener.local_addr().unwrap());
    let held = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            open.push(socket);
        }
    });
    let transport = transport_of(WasmHost::new().unwrap().routing(AWS_EC2, &silent));
    let request = step_request(
        "dep_silent",
        "cloud",
        call("aws", "ec2", "GET", "/"),
        &Some(aws()),
    );
    let started = Instant::now();
    // The module reports the call as failed and steps on; its reply stands.
    let response = step(&transport, &request, Duration::from_secs(4))
        .await
        .expect("a module that answers after its call ran out of time is heard");
    let took = started.elapsed();
    let reported = progress(response);
    assert!(
        failed(&reported).contains("no answer before the step's deadline"),
        "{reported}"
    );
    assert!(
        took >= Duration::from_secs(2) && took < Duration::from_secs(4),
        "{took:?}"
    );
    held.abort();
}

/// A module that fails its step after a call ran out of time keeps its view, and the step is
/// sent again as one that reached its deadline, whatever code the module gave.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_reply_after_a_call_ran_out_of_time_is_retried_as_a_deadline() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent = format!("http://{}", listener.local_addr().unwrap());
    let held = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            open.push(socket);
        }
    });
    let transport = transport_of(WasmHost::new().unwrap().routing(AWS_EC2, &silent));
    let request = step_request(
        "dep_silent_failed",
        "cloud-then-fail",
        json!({ "request": call("aws", "ec2", "GET", "/"), "retryable": false }),
        &Some(aws()),
    );
    let response = step(&transport, &request, Duration::from_secs(4))
        .await
        .expect("the module's failed reply is kept");
    match response.body {
        ResponseBody::Failed { controller, error } => {
            assert_eq!(error.code, codes::STEP_DEADLINE);
            assert!(error.retryable);
            assert!(error.message.contains("PROBE_FAILED"), "{}", error.message);
            assert_eq!(controller.state, json!({ "probe": "cloud-then-fail" }));
        }
        other => panic!("expected a failed reply, got {other:?}"),
    }
    held.abort();
}

/// After a call outlives the step's deadline, a module stopped for its fuel is sent again, as one
/// stopped by the clock is; one that gives no reply fails as it would without that call.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_that_cannot_answer_after_a_call_ran_out_of_time() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent = format!("http://{}", listener.local_addr().unwrap());
    let held = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            open.push(socket);
        }
    });
    // Little fuel, so a module that loops spends it within the host's grace.
    let transport = transport_of(
        WasmHost::new()
            .unwrap()
            .with_step_fuel(50_000_000)
            .routing(AWS_EC2, &silent),
    );
    for (then, code, retryable, message) in [
        ("loop", codes::STEP_DEADLINE, true, "not answered within"),
        (
            "no-reply",
            codes::PROTOCOL_ERROR,
            false,
            "could not answer the step",
        ),
    ] {
        let request = step_request(
            "dep_silent_then",
            "cloud-then",
            json!({ "request": call("aws", "ec2", "GET", "/"), "then": then }),
            &Some(aws()),
        );
        let started = std::time::Instant::now();
        let failure = step(&transport, &request, Duration::from_secs(3))
            .await
            .expect_err(then);
        // The call waited out the step's one-second deadline before the module went on, and the
        // module stopped before the three-second timeout, so the clock didn't stop it.
        let took = started.elapsed();
        assert!(
            took >= Duration::from_secs(1) && took < Duration::from_secs(3),
            "{then}: {took:?}"
        );
        assert_eq!(failure.code, code, "{then}: {}", failure.message);
        assert_eq!(failure.retryable, retryable, "{then}: {}", failure.message);
        assert!(failure.message.contains(message), "{}", failure.message);
    }
    held.abort();
}

/// The deployment's interceptors run before a call is sent; a call they leave no time for is
/// not sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_whose_interceptors_run_past_the_deadline_is_not_sent() {
    let stub = answering(200, "reached").await;
    let slow = |cloud: Cloud, _: Option<&str>| {
        chain_of(TestInterceptor::new("slow", cloud, "ec2").on_request(|_| {
            std::thread::sleep(Duration::from_millis(1_500));
            Ok(())
        }))
    };
    let transport = transport_of(host_routed_to(&stub)).with_interceptors(Arc::new(slow));
    // A one-second deadline: the timeout less the host's two seconds of grace.
    let request = step_request(
        "dep_slow_interceptor",
        "cloud",
        call("aws", "ec2", "POST", "/"),
        &Some(aws()),
    );
    let response = step(&transport, &request, Duration::from_secs(3))
        .await
        .expect("the module answers");
    let reported = progress(response);
    let refusal = failed(&reported);
    assert!(refusal.starts_with("refused: "), "{refusal}");
    assert!(refusal.contains("deadline"), "{refusal}");
    assert!(stub.requests().is_empty());
}

// ---------------------------------------------------------------- deployments in one process

/// Two deployments of one extension share the compiled module and nothing else: neither's step
/// finds what the other's left in memory, and each one's calls are signed with its own
/// credentials.
#[tokio::test(flavor = "multi_thread")]
async fn one_deployment_s_steps_share_nothing_with_another_s() {
    let stub =
        capture::start(|request: &Captured| Reply::new(200, "text/plain", request.target.clone()))
            .await;
    let host = Arc::new(host_routed_to(&stub));
    let transport = |deployment: &'static str| {
        let transport = host.transport(PROJECT, "acme", module(), &[]).unwrap();
        move |probe: &'static str, arg: Value| {
            let transport = transport.clone();
            async move {
                let credentials = Some(aws_named(&format!("ASIA{deployment}")));
                let request = step_request(deployment, probe, arg, &credentials);
                progress(
                    step(&transport, &request, Duration::from_secs(30))
                        .await
                        .expect("the step runs"),
                )
            }
        }
    };
    let (first, second) = (transport("FIRST"), transport("SECOND"));

    for _ in 0..3 {
        let left = first("left-behind", json!("the first deployment's state")).await;
        assert_eq!(left, r#"steps=1 found="""#);
        let left = second("left-behind", json!("the second deployment's state")).await;
        assert_eq!(left, r#"steps=1 found="""#);
    }

    let calls = (0..32).map(|i| {
        let (first, second) = (first.clone(), second.clone());
        async move {
            let (deployment, send) = if i % 2 == 0 {
                (
                    "FIRST",
                    first("cloud", call("aws", "ec2", "GET", "/FIRST")).boxed(),
                )
            } else {
                (
                    "SECOND",
                    second("cloud", call("aws", "ec2", "GET", "/SECOND")).boxed(),
                )
            };
            assert_eq!(answered(&send.await)["body"], format!("/{deployment}"));
        }
    });
    futures::future::join_all(calls).await;
    let received = stub.requests();
    assert_eq!(received.len(), 32);
    for request in received {
        let deployment = request.target.trim_start_matches('/');
        let signature = request.header("authorization").unwrap();
        assert!(
            signature.contains(&format!("Credential=ASIA{deployment}/")),
            "{deployment}: {signature}"
        );
    }
}

/// A cloud that answers `/held` only once the test adds a permit to `release`, and counts the
/// requests it is holding and the most it held at once. `/free` answers at once.
struct HoldingCloud {
    url: String,
    release: Arc<tokio::sync::Semaphore>,
    holding: Arc<AtomicUsize>,
    most_held: Arc<AtomicUsize>,
    /// Every request `/held` got.
    received: Arc<AtomicUsize>,
}

impl HoldingCloud {
    async fn start() -> Self {
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let (holding, most_held) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let received = Arc::new(AtomicUsize::new(0));
        let held = {
            let (release, holding, most_held, received) = (
                release.clone(),
                holding.clone(),
                most_held.clone(),
                received.clone(),
            );
            move || {
                let (release, holding, most_held) =
                    (release.clone(), holding.clone(), most_held.clone());
                received.fetch_add(1, Ordering::SeqCst);
                async move {
                    let now = holding.fetch_add(1, Ordering::SeqCst) + 1;
                    most_held.fetch_max(now, Ordering::SeqCst);
                    release.acquire().await.unwrap().forget();
                    holding.fetch_sub(1, Ordering::SeqCst);
                    "released"
                }
            }
        };
        let app = axum::Router::new()
            .route("/held", axum::routing::get(held))
            .route("/free", axum::routing::get(|| async { "free" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            release,
            holding,
            most_held,
            received,
        }
    }

    /// Waits until the cloud holds `count` requests.
    async fn holds(&self, count: usize) {
        let reached = async {
            while self.holding.load(Ordering::SeqCst) != count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(30), reached)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "the cloud holds {} requests, not {count}",
                    self.holding.load(Ordering::SeqCst)
                )
            });
    }
}

/// Starts a step of `deployment` whose module asks the cloud for `path`, and returns the task
/// that waits for what the cloud answered.
fn spawn_call(
    transport: &WasmTransport,
    deployment: &'static str,
    path: &'static str,
) -> tokio::task::JoinHandle<Value> {
    let transport = transport.clone();
    tokio::spawn(async move {
        let request = step_request(
            deployment,
            "cloud",
            call("aws", "ec2", "GET", path),
            &Some(aws()),
        );
        let response = step(&transport, &request, Duration::from_secs(60)).await;
        answered(&progress(response.expect("the step runs")))["body"].clone()
    })
}

/// One deployment whose module holds every step it is given takes two threads and no more: its
/// other steps wait, and another deployment's steps run meanwhile.
#[tokio::test(flavor = "multi_thread")]
async fn a_deployment_that_floods_steps_holds_up_only_itself() {
    let cloud = HoldingCloud::start().await;
    let host = Arc::new(WasmHost::new().unwrap().routing(AWS_EC2, &cloud.url));
    let transport = host.transport(PROJECT, "acme", module(), &[]).unwrap();

    const FLOOD: usize = 24;
    let flood: Vec<_> = (0..FLOOD)
        .map(|_| spawn_call(&transport, "dep_flooding", "/held"))
        .collect();
    cloud.holds(2).await;

    // Another deployment's steps run to the end while every flooding step is held or waits.
    for _ in 0..8 {
        let other = spawn_call(&transport, "dep_other", "/free");
        let other = tokio::time::timeout(Duration::from_secs(30), other)
            .await
            .expect("another deployment's step is not held up")
            .unwrap();
        assert_eq!(other, "free");
    }
    assert_eq!(cloud.holding.load(Ordering::SeqCst), 2);
    assert!(flood.iter().all(|step| !step.is_finished()));

    cloud.release.add_permits(FLOOD);
    for step in flood {
        assert_eq!(step.await.unwrap(), "released");
    }
    assert_eq!(
        cloud.most_held.load(Ordering::SeqCst),
        2,
        "the flooding deployment ran more steps at once than its limit"
    );
}

/// One project whose many deployments each hold their steps takes only its project's slots: its
/// further steps wait, and another project's steps run meanwhile, though the process has room
/// for every one of the first project's deployments.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_with_many_deployments_holds_up_only_itself() {
    let cloud = HoldingCloud::start().await;
    let host = Arc::new(
        WasmHost::new()
            .unwrap()
            .routing(AWS_EC2, &cloud.url)
            .with_steps_per_project(2)
            .with_steps_per_process(4),
    );
    let flooding = host
        .transport("proj_flooding", "acme", module(), &[])
        .unwrap();
    let other = host.transport("proj_other", "acme", module(), &[]).unwrap();

    const DEPLOYMENTS: [&str; 4] = ["dep_1", "dep_2", "dep_3", "dep_4"];
    let flood: Vec<_> = DEPLOYMENTS
        .iter()
        .map(|deployment| spawn_call(&flooding, deployment, "/held"))
        .collect();
    cloud.holds(2).await;

    for _ in 0..4 {
        let step = spawn_call(&other, "dep_other", "/free");
        let step = tokio::time::timeout(Duration::from_secs(30), step)
            .await
            .expect("another project's step is not held up")
            .unwrap();
        assert_eq!(step, "free");
    }
    assert_eq!(cloud.holding.load(Ordering::SeqCst), 2);

    cloud.release.add_permits(DEPLOYMENTS.len());
    for step in flood {
        assert_eq!(step.await.unwrap(), "released");
    }
    assert_eq!(
        cloud.most_held.load(Ordering::SeqCst),
        2,
        "the project ran more steps at once than its limit"
    );
}

/// A process runs only so many steps at once, across its deployments: with every slot held, a
/// step of a deployment that runs none waits, and runs once one is free.
#[tokio::test(flavor = "multi_thread")]
async fn a_process_runs_only_so_many_steps_at_once() {
    let cloud = HoldingCloud::start().await;
    let host = Arc::new(
        WasmHost::new()
            .unwrap()
            .routing(AWS_EC2, &cloud.url)
            .with_steps_per_process(2),
    );
    let transport = host.transport(PROJECT, "acme", module(), &[]).unwrap();

    let held = [
        spawn_call(&transport, "dep_a", "/held"),
        spawn_call(&transport, "dep_b", "/held"),
    ];
    cloud.holds(2).await;
    let waiting = spawn_call(&transport, "dep_c", "/free");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !waiting.is_finished(),
        "a third step ran past the process's limit"
    );

    cloud.release.add_permits(2);
    for step in held {
        assert_eq!(step.await.unwrap(), "released");
    }
    let waited = tokio::time::timeout(Duration::from_secs(30), waiting)
        .await
        .expect("the waiting step runs once a slot is free")
        .unwrap();
    assert_eq!(waited, "free");
    assert_eq!(cloud.most_held.load(Ordering::SeqCst), 2);
}

/// A step whose caller gave up keeps its slot until its module stops: the module still holds a
/// thread, whoever waits for its reply.
#[tokio::test(flavor = "multi_thread")]
async fn a_step_nobody_waits_for_keeps_its_slot_until_its_module_stops() {
    let cloud = HoldingCloud::start().await;
    let host = Arc::new(WasmHost::new().unwrap().routing(AWS_EC2, &cloud.url));
    let transport = host.transport(PROJECT, "acme", module(), &[]).unwrap();

    let abandoned = [
        spawn_call(&transport, "dep_abandoning", "/held"),
        spawn_call(&transport, "dep_abandoning", "/held"),
    ];
    cloud.holds(2).await;
    for step in &abandoned {
        step.abort();
    }
    for step in abandoned {
        assert!(step.await.unwrap_err().is_cancelled());
    }

    // Both modules are still in their calls, so the deployment's next step waits.
    let next = spawn_call(&transport, "dep_abandoning", "/free");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!next.is_finished());
    assert_eq!(cloud.holding.load(Ordering::SeqCst), 2);

    cloud.release.add_permits(2);
    let next = tokio::time::timeout(Duration::from_secs(30), next)
        .await
        .expect("the step runs once a module stopped")
        .unwrap();
    assert_eq!(next, "free");
}

/// A step whose caller gave up sends no further call: the module's next call is refused, so
/// nothing more is done for a step whose reply nobody reads.
#[tokio::test(flavor = "multi_thread")]
async fn a_step_nobody_waits_for_sends_no_further_call() {
    let cloud = HoldingCloud::start().await;
    let host = Arc::new(
        WasmHost::new()
            .unwrap()
            .routing(AWS_EC2, &cloud.url)
            .with_steps_per_deployment(1),
    );
    let transport = host.transport(PROJECT, "acme", module(), &[]).unwrap();
    let abandoned = {
        let transport = transport.clone();
        tokio::spawn(async move {
            let request = step_request(
                "dep_dropped",
                "cloud-many",
                json!({ "count": 2, "request": call("aws", "ec2", "GET", "/held") }),
                &Some(aws()),
            );
            step(&transport, &request, Duration::from_secs(60)).await
        })
    };
    cloud.holds(1).await;
    abandoned.abort();
    assert!(abandoned.await.unwrap_err().is_cancelled());
    cloud.release.add_permits(2);

    // The deployment's one slot is free once the module has stopped.
    let next = spawn_call(&transport, "dep_dropped", "/free");
    let next = tokio::time::timeout(Duration::from_secs(30), next)
        .await
        .expect("the next step runs once the module stopped")
        .unwrap();
    assert_eq!(next, "free");
    assert_eq!(cloud.received.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------- under the proxy and executor

const PROBE_TYPE: &str = "acme/probe";
const DEPLOYMENT_ID: &str = "dep_wasm";

#[derive(Debug)]
struct FixedCredentials(Credentials);

#[async_trait]
impl CredentialSource for FixedCredentials {
    async fn credentials(
        &self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> alien_infra::Result<Option<Credentials>> {
        Ok(Some(self.0.clone()))
    }
}

fn stack_with(probe: &str, arg: Value) -> Stack {
    serde_json::from_value(json!({
        "id": "acme",
        "resources": { "zone": {
            "config": { "type": PROBE_TYPE, "id": "zone", "probe": probe, "arg": arg },
            "lifecycle": "live", "dependencies": []
        } }
    }))
    .unwrap()
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

/// The executor for `stack`, whose custom resource steps in the module through `transport`
/// with a step deadline of `deadline`.
fn executor(
    stack: &Stack,
    config: &DeploymentConfig,
    transport: WasmTransport,
    deadline: Duration,
) -> StackExecutor {
    executor_under(stack, config, transport, deadline, |_| {
        InterceptorChain::default()
    })
}

/// [`executor`], whose service provider gives `interceptors` for a cloud's calls.
fn executor_under(
    stack: &Stack,
    config: &DeploymentConfig,
    transport: WasmTransport,
    deadline: Duration,
    interceptors: impl Fn(Cloud) -> InterceptorChain + Send + Sync + 'static,
) -> StackExecutor {
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_interceptors()
        .returning(move |cloud, _| interceptors(cloud));
    let factory = ProxyFactory::new(
        DEPLOYMENT_ID,
        Arc::new(transport),
        Some(Arc::new(FixedCredentials(aws()))),
    )
    .with_step_deadline(deadline);
    let mut registry = ResourceRegistry::with_built_ins();
    registry.register_controller_factory(
        ResourceType::from(PROBE_TYPE),
        Platform::Aws,
        Box::new(factory),
    );
    StackExecutor::builder(stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
        .deployment_config(config)
        .resource_registry(Arc::new(registry))
        .service_provider(Arc::new(provider))
        .build()
        .unwrap()
}

async fn step_stack(executor: &StackExecutor, state: StackState) -> StackState {
    executor
        .step(state)
        .await
        .unwrap_or_else(|e| panic!("executor.step failed: {} ({})", e.message, e.code))
        .next_state
}

fn zone(state: &StackState) -> &StackResourceState {
    &state.resources["zone"]
}

fn saved(state: &StackState) -> Value {
    zone(state).internal_state.clone().expect("internal state")
}

/// The deployment's settings reach a module as they reach a controller on a developer's
/// server: without the secrets. A module can send what it reads to its declared origins.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_gets_the_deployment_s_settings_without_their_secrets() {
    let mut config = deployment_config();
    config.environment_variables.variables = vec![
        EnvironmentVariable {
            name: "S".into(),
            value: "SECRETVALUE".into(),
            var_type: EnvironmentVariableType::Secret,
            target_resources: None,
        },
        EnvironmentVariable {
            name: "P".into(),
            value: "PLAINVALUE".into(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        },
    ];
    config.monitoring = Some(OtlpConfig {
        logs_endpoint: "https://manager.example/v1/logs".into(),
        logs_auth_header: "authorization=Bearer WRITETOKEN".into(),
        metrics_endpoint: None,
        metrics_auth_header: None,
        resource_attributes: Default::default(),
    });
    config.deployment_token = Some("REGISTRYTOKEN".into());

    let stack = stack_with("echo", Value::Null);
    let transport = transport_of(WasmHost::new().unwrap());
    let state = step_stack(
        &executor(&stack, &config, transport, Duration::from_secs(30)),
        StackState::new(Platform::Aws),
    )
    .await;
    assert_eq!(zone(&state).status, ResourceStatus::Running);

    let seen = saved(&state)["remote"]["progress"]
        .as_str()
        .expect("the module echoed its step")
        .to_string();
    for secret in ["SECRETVALUE", "WRITETOKEN", "REGISTRYTOKEN"] {
        assert!(!seen.contains(secret), "{secret} in {seen}");
    }
    assert_no_secret(&seen);
    let seen: Value = serde_json::from_str(&seen).unwrap();
    assert_eq!(
        seen["deploymentConfig"],
        serde_json::to_value(deployment_config_for_extension(&config)).unwrap()
    );
    assert!(seen["deploymentConfig"].to_string().contains("PLAINVALUE"));
    assert_eq!(seen["deploymentId"], DEPLOYMENT_ID);
    assert_eq!(seen["credentials"], Value::Null);
    assert_eq!(seen["cloud"]["accountId"], "123456789012");
}

/// What a module's step fails with decides what the executor does next, as for any controller:
/// it retries an error the module calls retryable, and fails the resource on one it doesn't.
#[tokio::test(flavor = "multi_thread")]
async fn a_retryable_failure_is_retried_and_a_terminal_one_fails_the_resource() {
    let config = deployment_config();
    let step_once = |arg: Value| {
        let config = config.clone();
        async move {
            let stack = stack_with("fail", arg);
            let transport = transport_of(WasmHost::new().unwrap());
            step_stack(
                &executor(&stack, &config, transport, Duration::from_secs(30)),
                StackState::new(Platform::Aws),
            )
            .await
        }
    };

    let state = step_once(json!({ "code": "THROTTLED", "retryable": true })).await;
    let resource = zone(&state);
    assert_eq!(resource.status, ResourceStatus::Provisioning);
    assert_eq!(resource.retry_attempt, 1);
    let error = resource.error.as_ref().unwrap();
    assert!(error.retryable, "{error:?}");
    assert_eq!(error.code, "EXTENSION_STEP_FAILED");
    assert!(error.message.contains("(THROTTLED)"), "{}", error.message);

    let state = step_once(json!({ "code": "ZONE_NAME_TAKEN", "retryable": false })).await;
    let resource = zone(&state);
    assert_eq!(resource.status, ResourceStatus::ProvisionFailed);
    let error = resource.error.as_ref().unwrap();
    assert!(!error.retryable);
    assert!(
        error.message.contains("(ZONE_NAME_TAKEN)"),
        "{}",
        error.message
    );
    // A module can't answer with a code of the host's own, such as an unreachable controller.
    let state = step_once(json!({ "code": codes::UNREACHABLE, "retryable": true })).await;
    assert_eq!(
        zone(&state).error.as_ref().unwrap().code,
        "EXTENSION_STEP_FAILED"
    );
}

/// A module stopped after its cloud call left no reply. The proxy sends the step again with the
/// same id and from the same state, and the module, which keeps nothing between steps, runs it
/// again: its calls have to be safe to repeat.
#[tokio::test(flavor = "multi_thread")]
async fn a_step_stopped_after_its_call_is_sent_again_with_the_same_id_and_state() {
    let reports = Arc::new(AtomicUsize::new(0));
    let cloud = {
        let reports = reports.clone();
        capture::start(move |_| {
            // The first report's answer makes the module trap.
            match reports.fetch_add(1, Ordering::SeqCst) {
                0 => Reply::new(200, "text/plain", "trap"),
                _ => Reply::new(200, "text/plain", "go on"),
            }
        })
        .await
    };
    let config = deployment_config();
    let stack = stack_with("report-then", Value::Null);
    let transport = transport_of(host_routed_to(&cloud));
    let executor = executor(&stack, &config, transport, Duration::from_secs(30));

    let first = step_stack(&executor, StackState::new(Platform::Aws)).await;
    let resource = zone(&first);
    assert_eq!(resource.status, ResourceStatus::Provisioning);
    assert_eq!(resource.retry_attempt, 1);
    let error = resource.error.as_ref().unwrap();
    assert_eq!(error.code, codes::UNREACHABLE);
    assert!(error.retryable);
    assert!(
        error.message.contains("the cloud said to trap"),
        "{}",
        error.message
    );
    let in_flight = saved(&first)["inFlightStepId"].clone();
    assert!(in_flight.is_string(), "the lost step keeps its id");

    let second = step_stack(&executor, first).await;
    let resource = zone(&second);
    assert_eq!(resource.status, ResourceStatus::Running);
    assert!(resource.error.is_none(), "{:?}", resource.error);
    assert_eq!(saved(&second)["remote"]["progress"], "reported");
    assert!(saved(&second)["inFlightStepId"].is_null());

    let steps: Vec<Value> = cloud
        .requests()
        .iter()
        .map(|report| serde_json::from_slice(&report.body).expect("the module reports its step"))
        .collect();
    assert_eq!(steps.len(), 2, "the module ran the step twice");
    assert_eq!(steps[0]["stepId"], in_flight);
    assert_eq!(steps[1]["stepId"], in_flight);
    assert_eq!(steps[0]["state"], Value::Null);
    assert_eq!(steps[1]["state"], steps[0]["state"]);
}

/// A module that traps after a call whose response an interceptor refused fails its resource:
/// the call was sent, so the step is not sent again to repeat it.
#[tokio::test(flavor = "multi_thread")]
async fn a_step_that_traps_after_a_refused_response_is_not_sent_again() {
    let cloud = answering(200, "created").await;
    let config = deployment_config();
    let stack = stack_with(
        "cloud-then",
        json!({ "request": call("aws", "ec2", "POST", "/"), "then": "panic" }),
    );
    let transport = transport_of(host_routed_to(&cloud));
    let executor = executor_under(
        &stack,
        &config,
        transport,
        Duration::from_secs(30),
        |cloud| {
            chain_of(
                TestInterceptor::new("checker", cloud, "ec2").on_response(|_, _| {
                    Err(AlienError::new(ClientErrorData::InterceptorFailed {
                        extension: "checker".into(),
                        message: "the answer is not one it accepts".into(),
                    }))
                }),
            )
        },
    );

    let first = step_stack(&executor, StackState::new(Platform::Aws)).await;
    let error = zone(&first).error.as_ref().expect("the step fails");
    assert!(!error.retryable, "{error:?}");
    assert_eq!(zone(&first).status, ResourceStatus::ProvisionFailed);
    step_stack(&executor, first).await;
    assert_eq!(cloud.requests().len(), 1);
}

/// A module's calls run the interceptors of the provider its resource is stepped under, with
/// no word from whoever loaded the module: here one that moves the call, which stops it.
#[tokio::test(flavor = "multi_thread")]
async fn a_stepped_module_s_calls_run_the_stepping_provider_s_interceptors() {
    let cloud = answering(200, "reached").await;
    let config = deployment_config();
    let stack = stack_with("cloud", call("aws", "ec2", "POST", "/"));
    let transport = transport_of(host_routed_to(&cloud));
    let executor = executor_under(
        &stack,
        &config,
        transport,
        Duration::from_secs(30),
        |cloud| {
            chain_of(
                TestInterceptor::new("mover", cloud, "ec2").on_request(|request| {
                    request.url = "https://evil.example.com/".parse().unwrap();
                    Ok(())
                }),
            )
        },
    );

    let state = step_stack(&executor, StackState::new(Platform::Aws)).await;
    let reported = saved(&state)["remote"]["progress"]
        .as_str()
        .expect("the module reports its call")
        .to_string();
    let refusal = failed(&reported);
    assert!(refusal.starts_with("refused: "), "{refusal}");
    assert!(refusal.contains("mover"), "{refusal}");
    assert!(cloud.requests().is_empty());
}

/// A module that loops fails its step at the deadline, the executor retries it, and the
/// resource is still the proxy's to fail once the retries run out.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_stopped_at_the_deadline_is_retried_by_the_executor() {
    let config = deployment_config();
    let stack = stack_with("loop", Value::Null);
    // Fuel to spare, so the deadline is what stops it.
    let transport = transport_of(WasmHost::new().unwrap().with_step_fuel(u64::MAX));
    let executor = executor(&stack, &config, transport, Duration::from_millis(300));

    let started = Instant::now();
    let state = step_stack(&executor, StackState::new(Platform::Aws)).await;
    assert!(started.elapsed() < Duration::from_secs(10));
    let resource = zone(&state);
    assert_eq!(resource.status, ResourceStatus::Provisioning);
    assert_eq!(resource.retry_attempt, 1);
    let error = resource.error.as_ref().unwrap();
    assert_eq!(error.code, codes::STEP_DEADLINE);
    assert!(error.retryable);
    assert!(saved(&state)["inFlightStepId"].is_string());
}

/// A module that answers every delete step that it is still deleting can't hold the customer's
/// delete forever: past the delete's deadline its resource ends DeleteFailed.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_a_module_never_finishes_ends_failed_at_the_delete_s_deadline() {
    let config = deployment_config();
    let transport = transport_of(WasmHost::new().unwrap());
    let created = step_stack(
        &executor(
            &stack_with("never-deleted", Value::Null),
            &config,
            transport.clone(),
            Duration::from_secs(30),
        ),
        StackState::new(Platform::Aws),
    )
    .await;
    assert_eq!(zone(&created).status, ResourceStatus::Running);

    let empty: Stack = serde_json::from_value(json!({ "id": "acme", "resources": {} })).unwrap();
    let deleting = executor(&empty, &config, transport, Duration::from_secs(30));
    let mut state = created;
    for _ in 0..4 {
        state = step_stack(&deleting, state).await;
    }
    assert_eq!(
        zone(&state).status,
        ResourceStatus::Deleting,
        "inside the deadline the delete goes on: {:?}",
        zone(&state).error
    );

    state
        .resources
        .get_mut("zone")
        .unwrap()
        .internal_state
        .as_mut()
        .unwrap()["deleteStartedAt"] = json!((chrono::Utc::now() - DELETE_TIMEOUT).to_rfc3339());
    for _ in 0..3 {
        state = step_stack(&deleting, state).await;
    }
    let resource = zone(&state);
    assert_eq!(
        resource.status,
        ResourceStatus::DeleteFailed,
        "{:?}",
        resource.error
    );
    assert!(
        resource
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("did not finish deleting the resource within 30 minutes"),
        "{:?}",
        resource.error
    );
}

/// A module that deletes its resource ends the delete, as any controller does.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_steps_a_resource_from_create_to_deleted() {
    let config = deployment_config();
    let transport = transport_of(WasmHost::new().unwrap());
    let created = step_stack(
        &executor(
            &stack_with("noop", Value::Null),
            &config,
            transport.clone(),
            Duration::from_secs(30),
        ),
        StackState::new(Platform::Aws),
    )
    .await;
    assert_eq!(zone(&created).status, ResourceStatus::Running);
    assert_eq!(
        saved(&created)["remote"]["state"],
        json!({ "probe": "noop" })
    );

    let empty: Stack = serde_json::from_value(json!({ "id": "acme", "resources": {} })).unwrap();
    let deleting = executor(&empty, &config, transport, Duration::from_secs(30));
    let mut state = created;
    for _ in 0..4 {
        state = step_stack(&deleting, state).await;
        if !state.resources.contains_key("zone") {
            break;
        }
    }
    assert!(
        state
            .resources
            .get("zone")
            .is_none_or(|zone| zone.status == ResourceStatus::Deleted),
        "{:?}",
        state.resources.get("zone").map(|zone| zone.status)
    );
}
