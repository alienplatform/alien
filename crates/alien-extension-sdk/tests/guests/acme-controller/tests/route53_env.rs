//! A Route53 client built from `ctx.aws_sdk_config()`, as the docs' controller builds one, with
//! every AWS environment variable and profile setting pointing somewhere else: the step alone
//! decides where it calls, how it signs and how it retries. Its own process, since it sets the
//! environment.

use acme_types::DnsZone;
#[allow(unused_imports)]
use alien_extension_sdk::controller_prelude::*;
use alien_infra::step_protocol::host::{step_client_config, ControllerHost};
use alien_infra::step_protocol::protocol::{ResponseBody, StepRequest};
use alien_infra::DefaultPlatformServiceProvider;
use aws_sdk_route53::config::interceptors::BeforeTransmitInterceptorContextRef;
use aws_sdk_route53::config::{ConfigBag, Intercept, RuntimeComponents};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The request the client would send, taken before it leaves: URI, signature and session token.
#[derive(Debug, Default, Clone)]
struct Capture(Arc<Mutex<Option<(String, String, String)>>>);

impl Intercept for Capture {
    fn name(&self) -> &'static str {
        "capture"
    }

    fn read_before_transmit(
        &self,
        ctx: &BeforeTransmitInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> std::result::Result<(), aws_sdk_route53::error::BoxError> {
        let request = ctx.request();
        let header = |name: &str| request.headers().get(name).unwrap_or("").to_string();
        *self.0.lock().unwrap() = Some((
            request.uri().to_string(),
            header("authorization"),
            header("x-amz-security-token"),
        ));
        Err("captured before transmit".into())
    }
}

/// Calls CreateHostedZone with a client from the step's config: captured before it leaves, or
/// sent to `throttle_endpoint` when that is set.
#[controller]
pub struct Route53Probe {
    throttle_endpoint: Option<String>,
    uri: Option<String>,
    authorization: Option<String>,
    token: Option<String>,
    error: Option<String>,
    sdk_endpoint: Option<String>,
    sdk_fips: Option<bool>,
    sdk_dual_stack: Option<bool>,
    sdk_retry: Option<String>,
    client_region: Option<String>,
}

#[controller]
impl Route53Probe {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let sdk = ctx.aws_sdk_config().await?;
        self.sdk_endpoint = sdk.endpoint_url().map(str::to_string);
        self.sdk_fips = sdk.use_fips();
        self.sdk_dual_stack = sdk.use_dual_stack();
        self.sdk_retry = sdk.retry_config().map(|retry| format!("{retry:?}"));
        let capture = Capture::default();
        let mut builder = aws_sdk_route53::config::Builder::from(&sdk);
        builder = match &self.throttle_endpoint {
            Some(endpoint) => builder.endpoint_url(endpoint),
            None => builder.interceptor(capture.clone()),
        };
        let client = aws_sdk_route53::Client::from_conf(builder.build());
        self.client_region = client.config().region().map(|region| region.to_string());
        let result = client
            .create_hosted_zone()
            .name("internal.acme.dev")
            .caller_reference("ref")
            .send()
            .await;
        self.error = result
            .err()
            .map(|e| format!("{}", aws_sdk_route53::error::DisplayErrorContext(e)));
        if let Some((uri, authorization, token)) = capture.0.lock().unwrap().clone() {
            self.uri = Some(uri);
            self.authorization = Some(authorization);
            self.token = Some(token);
        }
        Ok(HandlerAction::Continue {
            state: Done,
            suggested_delay: None,
        })
    }

    #[flow_entry(Update)]
    #[handler(state = UpdateStart, on_failure = CreateFailed, status = ResourceStatus::Updating)]
    async fn update_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Done,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = CreateFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(state = Done, status = ResourceStatus::Running);
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        None
    }
}

fn request() -> StepRequest {
    serde_json::from_value(json!({
        "protocol": 1, "stepId": "s1", "deploymentId": "dep_acme",
        "resourceId": "zone", "resourceType": "acme/dns-zone", "platform": "aws",
        "resourcePrefix": "q6aa0b26", "initialSetupAuthority": "importedHandoff",
        "lifecycle": "live",
        "resource": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": {
            "stackSettings": {},
            "environmentVariables": { "variables": [], "hash": "", "createdAt": "" },
            "externalBindings": {},
            "allowFrozenChanges": false
        },
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": {
            "platform": "aws", "accountId": "123456789012", "region": "eu-west-1",
            "accessKeyId": "ASIASTEPKEY", "secretAccessKey": "step-secret",
            "sessionToken": "step-session", "expiresAt": null
        },
        "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap()
}

async fn step(controller: Route53Probe) -> Value {
    let mut host = ControllerHost::new(
        step_client_config,
        Arc::new(DefaultPlatformServiceProvider::default()),
        None,
    );
    host.register::<DnsZone, _>(
        DnsZone::RESOURCE_TYPE.resource_type(),
        Platform::Aws,
        controller,
    )
    .unwrap();
    match host.handle(request()).await.body {
        ResponseBody::Stepped { controller, .. } => controller.state,
        other => panic!("expected a step, got {other:?}"),
    }
}

/// A server that answers every request with Route53's Throttling error and records each
/// request's head.
async fn throttling_server() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let heads = Arc::new(Mutex::new(Vec::new()));
    let recorded = heads.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let heads = recorded.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65536];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                heads
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).to_string());
                let body = "<ErrorResponse xmlns=\"https://route53.amazonaws.com/doc/2013-04-01/\"><Error><Type>Sender</Type><Code>Throttling</Code><Message>Rate exceeded</Message></Error><RequestId>r1</RequestId></ErrorResponse>";
                let reply = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/xml\r\nx-amzn-requestid: r1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (url, heads)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_step_alone_decides_where_a_route53_client_calls_and_how_it_retries() {
    let (local, heads) = throttling_server().await;
    let dir = tempfile_dir();
    let profile = format!(
        "[default]\nregion = ap-south-1\nendpoint_url = {local}/profile-default\nuse_fips_endpoint = true\nuse_dualstack_endpoint = true\nmax_attempts = 1\nretry_mode = adaptive\nca_bundle = /nonexistent/ca.pem\n\n[profile probe]\nregion = ap-south-1\nendpoint_url = {local}/profile-probe\nuse_fips_endpoint = true\nuse_dualstack_endpoint = true\nmax_attempts = 1\nretry_mode = adaptive\nca_bundle = /nonexistent/ca.pem\nservices = probe-svc\n\n[services probe-svc]\nroute_53 =\n  endpoint_url = {local}/profile-service\n"
    );
    std::fs::write(dir.join("config"), profile).unwrap();
    std::fs::write(
        dir.join("credentials"),
        "[default]\naws_access_key_id = AKIAPROFILE\naws_secret_access_key = profile-secret\n[probe]\naws_access_key_id = AKIAPROFILE\naws_secret_access_key = profile-secret\n",
    )
    .unwrap();
    // SAFETY: set before any step runs, and this is the only test in its binary.
    unsafe {
        for (name, value) in [
            ("AWS_ENDPOINT_URL", format!("{local}/env")),
            ("AWS_ENDPOINT_URL_ROUTE_53", format!("{local}/env-route53")),
            ("AWS_USE_FIPS_ENDPOINT", "true".into()),
            ("AWS_USE_DUALSTACK_ENDPOINT", "true".into()),
            ("AWS_MAX_ATTEMPTS", "1".into()),
            ("AWS_RETRY_MODE", "adaptive".into()),
            ("AWS_CA_BUNDLE", "/nonexistent/ca.pem".into()),
            ("AWS_CONFIG_FILE", dir.join("config").display().to_string()),
            (
                "AWS_SHARED_CREDENTIALS_FILE",
                dir.join("credentials").display().to_string(),
            ),
            ("AWS_PROFILE", "probe".into()),
            ("AWS_REGION", "ap-south-1".into()),
            ("AWS_DEFAULT_REGION", "ap-south-1".into()),
            ("AWS_ACCESS_KEY_ID", "AKIAENVIRONMENT".into()),
            ("AWS_SECRET_ACCESS_KEY", "env-secret".into()),
            ("AWS_SESSION_TOKEN", "env-session".into()),
            ("AWS_IGNORE_CONFIGURED_ENDPOINT_URLS", "false".into()),
            ("HTTPS_PROXY", format!("{local}/proxy")),
            ("HTTP_PROXY", format!("{local}/proxy")),
        ] {
            std::env::set_var(name, value);
        }
    }

    // Where CreateHostedZone would go, and how it is signed: Route53's global endpoint, signed
    // for us-east-1 with the step's key, from a step in eu-west-1.
    let state = step(Route53Probe::default()).await;
    assert_eq!(
        state["uri"],
        "https://route53.amazonaws.com/2013-04-01/hostedzone"
    );
    let authorization = state["authorization"].as_str().unwrap();
    assert!(
        authorization.contains("Credential=ASIASTEPKEY/")
            && authorization.contains("/us-east-1/route53/aws4_request"),
        "{authorization}"
    );
    assert_eq!(state["token"], "step-session");
    assert_eq!(state["clientRegion"], "eu-west-1");
    assert_eq!(state["sdkEndpoint"], Value::Null);
    assert_eq!(state["sdkFips"], Value::Null);
    assert_eq!(state["sdkDualStack"], Value::Null);
    let retry = state["sdkRetry"].as_str().unwrap();
    assert!(
        retry.contains("Standard") && retry.contains("max_attempts: 3"),
        "{retry}"
    );
    assert!(
        heads.lock().unwrap().is_empty(),
        "a request reached an endpoint or proxy from the environment or the profile"
    );

    // Pointed at a server that always throttles: three attempts, each straight to that server
    // (no proxy), then the Throttling error.
    let state = step(Route53Probe {
        throttle_endpoint: Some(format!("{local}/explicit")),
        ..Default::default()
    })
    .await;
    let heads = heads.lock().unwrap().clone();
    assert_eq!(heads.len(), 3, "{heads:?}");
    for (attempt, head) in heads.iter().enumerate() {
        assert!(
            head.starts_with("POST /explicit/2013-04-01/hostedzone HTTP/1.1"),
            "{head}"
        );
        let attempt_header = format!("attempt={}; max=3", attempt + 1);
        assert!(
            head.to_ascii_lowercase().contains(&attempt_header),
            "{head}"
        );
    }
    let error = state["error"].as_str().unwrap();
    assert!(error.contains("Throttling"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("route53-env-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
