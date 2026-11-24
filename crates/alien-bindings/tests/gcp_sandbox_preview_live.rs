//! Preview on a real GCP Agent Platform sandbox, as owner access: HTTP and WebSocket through the
//! returned URL on every declared port with the caller's own bearer reaching the daemon, a refresh
//! after the 300 second edge credential ends, and the refusals for an identity without execute.
//!
//! Ignored: it needs a project with a reasoning engine, an image that carries the agent and the
//! conformance fixture (`crates/alien-sandbox-conformance/contract`), and four identities:
//! - the owner, which holds execute and may sign as the edge signer (`GCP_OWNER_TOKEN`);
//! - the edge signer (`GCP_PREVIEW_SA`), which holds execute on the engine;
//! - an admin, which provisions the template and the sandbox and signs for the identity that has
//!   no right to execute (`GCP_ADMIN_TOKEN`);
//! - that identity without execute (`GCP_NOEXEC_SA`, token in `GCP_NOEXEC_TOKEN`).
//!
//! Env: `GCP_PROJECT`, `GCP_REGION`, `GCP_ENGINE` (bare id), `GCP_IMAGE`, `GCP_PREVIEW_SA`,
//! `GCP_OWNER_TOKEN`, `GCP_ADMIN_TOKEN`, `GCP_NOEXEC_SA`, `GCP_NOEXEC_TOKEN`. Optional:
//! `GCP_PREVIEW_PORTS` (default `8080,8081,8090`), `GCP_DAEMON_TOKEN` (the fixture's bearer).
//!
//! Two tests, each run by its exact name, because they need different images:
//!
//! ```text
//! # The conformance fixture, on every declared port (the image above).
//! cargo test -p alien-bindings --features gcp --test gcp_sandbox_preview_live -- \
//!     --ignored --nocapture --exact live_preview_is_owner_access
//!
//! # A workload's own daemon on 8080 (an image that carries it); also needs `GCP_APP_BEARER`,
//! # `GCP_APP_HTTP_PATH`, `GCP_APP_WS_PATH`, `GCP_APP_SUBPROTOCOLS` and `GCP_APP_PICKS`, and
//! # optionally `GCP_APP_HTTP_HEADER`. It names any that are missing before it creates anything.
//! cargo test -p alien-bindings --features gcp --test gcp_sandbox_preview_live -- \
//!     --ignored --nocapture --exact live_preview_reaches_a_daemon_with_its_own_protocol
//! ```

#![cfg(feature = "gcp")]

use std::sync::Arc;
use std::time::Duration;

use alien_bindings::providers::sandbox::gcp_agent_platform::{
    EdgeTokenSigner, GcpAgentPlatformSandbox, IamEdgeTokenSigner, SignedEdgeToken,
};
use alien_bindings::traits::{CreateSandboxRequest, PreviewCapability, Sandbox};
use alien_core::{GcpClientConfig, GcpCredentials};
use alien_gcp_clients::gcp::agent_platform::{
    AgentPlatformApi, AgentPlatformClient, ContainerPort, ContainerResources,
    CustomContainerEnvironment, CustomContainerSpec, EgressControlConfig, PollBudget,
    SandboxEnvironmentTemplate,
};
use futures::{FutureExt, SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

/// The agent's own port, the one the edge reaches `/v1/exec` on.
const AGENT_PORT: u16 = alien_core::sandbox_image::AGENT_PORT;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

/// Names every missing variable at once, before a test creates anything in the cloud.
fn require_env(names: &[&str]) {
    let missing: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| std::env::var(name).is_err())
        .collect();
    assert!(
        missing.is_empty(),
        "this test needs these variables set, and nothing was created: {}",
        missing.join(", ")
    );
}

fn config(token: String) -> GcpClientConfig {
    GcpClientConfig {
        project_id: env("GCP_PROJECT"),
        region: env("GCP_REGION"),
        credentials: GcpCredentials::AccessToken { token },
        service_overrides: None,
        project_number: None,
    }
}

fn client(token: String) -> AgentPlatformClient {
    AgentPlatformClient::new(reqwest::Client::new(), config(token))
}

/// A sandbox binding for the identity that holds `token`, signing as `signer_account`.
fn provider(
    token: String,
    engine: &str,
    template: &str,
    signer_account: &str,
    ports: &[u16],
) -> GcpAgentPlatformSandbox {
    let iam = alien_gcp_clients::iam::IamClient::new(reqwest::Client::new(), config(token.clone()));
    GcpAgentPlatformSandbox::new(
        Arc::new(client(token)),
        engine.to_string(),
        template.to_string(),
        Some(3600),
    )
    .with_preview_signer(signer(iam, signer_account))
    .with_preview_ports(ports.to_vec())
}

/// Signs the edge credential through IAM, or, when `GCP_SIGNER_KEY_FILE` names the signer's own key
/// file (a project where nobody may impersonate the signer), with that key through `openssl`.
fn signer(iam: alien_gcp_clients::iam::IamClient, account: &str) -> Arc<dyn EdgeTokenSigner> {
    match std::env::var("GCP_SIGNER_KEY_FILE") {
        Ok(key_file) => Arc::new(KeyFileSigner {
            key_file,
            account: account.to_string(),
        }),
        Err(_) => Arc::new(IamEdgeTokenSigner::new(Arc::new(iam), account)),
    }
}

#[derive(Debug)]
struct KeyFileSigner {
    key_file: String,
    account: String,
}

#[async_trait::async_trait]
impl EdgeTokenSigner for KeyFileSigner {
    async fn sign(&self) -> alien_bindings::Result<SignedEdgeToken> {
        use base64::Engine as _;
        use std::io::Write;
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let now = chrono::Utc::now().timestamp();
        let key: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.key_file).expect("key file"))
                .expect("json");
        let header = b64(
            serde_json::json!({"alg": "RS256", "typ": "JWT", "kid": key["private_key_id"]})
                .to_string()
                .as_bytes(),
        );
        let claims = b64(serde_json::json!({
            "iat": now, "exp": now + 300, "iss": self.account, "sub": self.account,
            "nonce": uuid::Uuid::new_v4().as_u128() % 1_000_000_000 + 1,
            "aud": "https://aiplatform.googleapis.com/",
        })
        .to_string()
        .as_bytes());
        let input = format!("{header}.{claims}");
        let pem = std::env::temp_dir().join(format!("m1g-{}.pem", uuid::Uuid::new_v4()));
        std::fs::write(&pem, key["private_key"].as_str().unwrap()).expect("pem");
        std::fs::set_permissions(&pem, std::os::unix::fs::PermissionsExt::from_mode(0o600)).ok();
        let mut child = std::process::Command::new("openssl")
            .args(["dgst", "-sha256", "-sign"])
            .arg(&pem)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("openssl");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let signature = child.wait_with_output().expect("signature").stdout;
        std::fs::remove_file(&pem).ok();
        Ok(SignedEdgeToken {
            jwt: format!("{input}.{}", b64(&signature)),
            expires_at: now + 300,
        })
    }
}

/// Sends one request with the preview's headers, and the caller's bearer under the header the
/// contract names.
async fn request(
    preview: &PreviewCapability,
    method: reqwest::Method,
    path: &str,
    bearer: Option<&str>,
    body: Option<serde_json::Value>,
) -> (u16, String) {
    request_with(preview, method, path, bearer, body, &[]).await
}

/// [`request`] with headers of the daemon's own besides.
async fn request_with(
    preview: &PreviewCapability,
    method: reqwest::Method,
    path: &str,
    bearer: Option<&str>,
    body: Option<serde_json::Value>,
    extra: &[(&str, &str)],
) -> (u16, String) {
    let mut request = reqwest::Client::new().request(method, format!("{}{path}", preview.endpoint));
    for (name, value) in &preview.headers {
        request = request.header(name, value);
    }
    if let Some(bearer) = bearer {
        request = request.header(&preview.caller_authorization_header, bearer);
    }
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.expect("the edge answers");
    (
        response.status().as_u16(),
        response.text().await.unwrap_or_default(),
    )
}

fn sha256(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// HTTP to one declared port: the daemon sees the caller's bearer unchanged, and answers its own
/// 401 to a wrong one.
async fn assert_http(preview: &PreviewCapability, port: u16, token: &str) {
    let bearer = format!("Bearer {token}");
    // The daemon starts after the agent; the agent answers 502 until it listens.
    let mut last = (0, String::new());
    for _ in 0..30 {
        last = request(
            preview,
            reqwest::Method::GET,
            "/whoami",
            Some(&bearer),
            None,
        )
        .await;
        if last.0 != 502 {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert_eq!(last.0, 200, "port {port}: {}", last.1);
    let seen: serde_json::Value = serde_json::from_str(&last.1).expect("daemon json");
    assert_eq!(seen["port"], port, "{seen}");
    assert_eq!(
        seen["authorizationSha256"],
        sha256(&bearer),
        "the caller's bearer must arrive at the daemon unchanged"
    );
    let (wrong, _) = request(
        preview,
        reqwest::Method::GET,
        "/whoami",
        Some("Bearer wrong"),
        None,
    )
    .await;
    assert_eq!(wrong, 401, "the daemon, not the broker, checks the bearer");
    eprintln!("PASS http port {port}: 200, bearer unchanged, wrong bearer 401");
}

/// Opens a WebSocket through a preview: the credentials ride the subprotocol list after the
/// app's own protocols, and the caller's bearer travels in its own header.
async fn open_socket(
    preview: &PreviewCapability,
    path: &str,
    own_protocols: &[&str],
    bearer: &str,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>,
    ),
    tokio_tungstenite::tungstenite::Error,
> {
    let socket_url = preview
        .endpoint
        .replacen("https://", "wss://", 1)
        .trim_end_matches('/')
        .to_string()
        + path;
    let mut request = socket_url
        .as_str()
        .into_client_request()
        .expect("a request");
    for (name, value) in &preview.headers {
        // Header credentials are refused on an upgrade; they ride the subprotocol list instead.
        if name.eq_ignore_ascii_case("authorization") {
            continue;
        }
        request.headers_mut().insert(
            tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(name.as_bytes())
                .expect("a header name"),
            HeaderValue::from_str(value).expect("a header value"),
        );
    }
    request.headers_mut().insert(
        tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(
            preview.caller_authorization_header.as_bytes(),
        )
        .expect("a header name"),
        HeaderValue::from_str(bearer).expect("a header value"),
    );
    assert!(!preview.subprotocols.is_empty(), "GCP needs subprotocols");
    let protocols = own_protocols
        .iter()
        .copied()
        .chain(preview.subprotocols.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(", ");
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_str(&protocols).expect("a protocol list"),
    );
    tokio_tungstenite::connect_async(request).await
}

/// WebSocket to one declared port of the fixture, whose daemon picks the first protocol offered.
async fn assert_websocket(preview: &PreviewCapability, port: u16, token: &str) {
    let (mut socket, response) = open_socket(
        preview,
        "/ws",
        &["alien.fixture.v1"],
        &format!("Bearer {token}"),
    )
    .await
    .unwrap_or_else(|error| panic!("port {port}: the upgrade failed: {error}"));
    let payload = vec![port as u8; 4096];
    socket
        .send(Message::Binary(payload.clone().into()))
        .await
        .expect("send");
    let echoed = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(data))) => return data,
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                other => panic!("port {port}: unexpected {other:?}"),
            }
        }
    })
    .await
    .expect("the echo arrives");
    assert_eq!(echoed.as_ref(), payload.as_slice());
    socket.close(None).await.ok();
    eprintln!(
        "PASS websocket port {port}: upgrade {} and a 4 KiB echo",
        response.status()
    );
}

/// The edge answered "not allowed" for a credential that must not work.
fn refused(status: u16) -> bool {
    [401, 403].contains(&status)
}

/// A template, a sandbox cut from it, and the identities that reach it.
struct Live {
    engine: String,
    signer_account: String,
    region: String,
    admin: AgentPlatformClient,
    admin_provider: GcpAgentPlatformSandbox,
    owner: GcpAgentPlatformSandbox,
    template: String,
    template_id: String,
    sandbox_id: String,
}

impl Live {
    /// The admin provisions; the owner is the identity under test.
    async fn start(image: &str, ports: &[u16]) -> Self {
        let engine = env("GCP_ENGINE");
        let signer_account = env("GCP_PREVIEW_SA");
        let region = env("GCP_REGION");

        // The template declares the agent's port first, then the preview ports, as the controller's
        // does.
        let admin = client(env("GCP_ADMIN_TOKEN"));
        let budget = PollBudget {
            interval: Duration::from_secs(2),
            max_attempts: 150,
        };
        let operation = admin
            .create_template(
                &engine,
                SandboxEnvironmentTemplate {
                    name: None,
                    display_name: Some("a2657p-preview".to_string()),
                    custom_container_environment: Some(CustomContainerEnvironment {
                        custom_container_spec: Some(CustomContainerSpec {
                            image_uri: image.to_string(),
                            extra: Default::default(),
                        }),
                        resources: Some(ContainerResources {
                            requests: None,
                            limits: Some(
                                [("cpu", "2"), ("memory", "4Gi")]
                                    .into_iter()
                                    .map(|(k, v)| (k.to_string(), v.to_string()))
                                    .collect(),
                            ),
                        }),
                        ports: std::iter::once(AGENT_PORT)
                            .chain(ports.iter().copied())
                            .map(|port| ContainerPort {
                                port: i32::from(port),
                                protocol: Some("TCP".to_string()),
                            })
                            .collect(),
                        extra: Default::default(),
                    }),
                    egress_control_config: Some(EgressControlConfig {
                        internet_access: Some(false),
                        network_attachment: None,
                        extra: Default::default(),
                    }),
                    ingress_control_config: None,
                    state: None,
                    extra: Default::default(),
                },
            )
            .await
            .expect("template create accepted");
        let created: SandboxEnvironmentTemplate = admin
            .await_operation(&operation, budget.clone())
            .await
            .expect("template create resolves");
        let template = created.name.expect("a template name");
        let template_id = template.rsplit('/').next().unwrap().to_string();
        admin
            .await_template_active(&engine, &template_id, budget)
            .await
            .expect("the template reaches ACTIVE");
        eprintln!("template {template_id} active");

        // The admin creates the sandbox; the owner previews and runs commands in it.
        let admin_provider = provider(
            env("GCP_ADMIN_TOKEN"),
            &engine,
            &template,
            &signer_account,
            ports,
        );
        let owner = provider(
            env("GCP_OWNER_TOKEN"),
            &engine,
            &template,
            &signer_account,
            ports,
        );
        let sandbox = admin_provider
            .create(CreateSandboxRequest::default())
            .await
            .expect("the sandbox starts");
        let sandbox_id = sandbox.sandbox_id.clone();
        eprintln!("sandbox {sandbox_id}");

        Self {
            engine,
            signer_account,
            region,
            admin,
            admin_provider,
            owner,
            template,
            template_id,
            sandbox_id,
        }
    }

    async fn stop(&self) {
        if let Err(error) = self.admin_provider.terminate(&self.sandbox_id).await {
            eprintln!("terminate {}: {error}", self.sandbox_id);
        }
        if let Err(error) = self
            .admin
            .delete_template(&self.engine, &self.template_id)
            .await
        {
            eprintln!("delete template {}: {error}", self.template_id);
        }
    }
}

#[tokio::test]
#[ignore = "needs a live GCP project, an engine, an image and four identities"]
async fn live_preview_is_owner_access() {
    // The WebSocket client needs a process-wide TLS provider.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ports: Vec<u16> = std::env::var("GCP_PREVIEW_PORTS")
        .unwrap_or_else(|_| "8080,8081,8090".to_string())
        .split(',')
        .map(|port| port.trim().parse().expect("a port"))
        .collect();
    let daemon_token = std::env::var("GCP_DAEMON_TOKEN")
        .unwrap_or_else(|_| "conformance-bearer-token".to_string());
    let live = Live::start(&env("GCP_IMAGE"), &ports).await;
    let Live {
        engine,
        signer_account,
        region,
        owner,
        template,
        sandbox_id,
        ..
    } = &live;

    let outcome = std::panic::AssertUnwindSafe(async {
        // The owner reaches every declared port over HTTP and WebSocket.
        let mut first = Vec::new();
        for port in &ports {
            let preview = owner.preview(&sandbox_id, *port).await.expect("owner preview");
            assert_eq!(preview.allowed_ports, vec![*port]);
            assert_eq!(
                preview.caller_authorization_header,
                "x-alien-caller-authorization"
            );
            assert!(
                preview.expires_in_seconds <= 300,
                "the 300 second edge credential bounds the lifetime: {}",
                preview.expires_in_seconds
            );
            assert_http(&preview, *port, &daemon_token).await;
            assert_websocket(&preview, *port, &daemon_token).await;
            first.push(preview);
        }
        let issued = std::time::Instant::now();

        // Owner access is command access, and the proof says so plainly: the same credentials
        // run a command on the agent's own port.
        let host = first[0]
            .endpoint
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let mut exec = reqwest::Client::new()
            .post(format!("https://{host}/v1/exec"))
            .json(&serde_json::json!({"command": ["id"], "timeoutMs": 5000}));
        for (name, value) in &first[0].headers {
            exec = exec.header(name, value);
        }
        let exec = exec.send().await.expect("the edge answers");
        eprintln!(
            "NOTE owner POST /v1/exec with the preview credentials: {} (preview is command access)",
            exec.status()
        );

        // An identity without execute gets nothing from the bindings: the refusal names the rule
        // and no header is returned.
        let denied = provider(
            env("GCP_NOEXEC_TOKEN"),
            &engine,
            &template,
            &signer_account,
            &ports,
        )
        .preview(&sandbox_id, ports[0])
        .await
        .expect_err("an identity without execute must not get a preview");
        assert!(
            denied
                .to_string()
                .contains(alien_core::GCP_PREVIEW_IS_COMMAND_ACCESS),
            "{denied}"
        );
        eprintln!("PASS bindings refuse a preview for an identity without execute: {}", denied.code);

        // The edge refuses a JWT signed by that identity on the preview port, on the agent port
        // and on Google's own `:execute`.
        let noexec_jwt = IamEdgeTokenSigner::new(
            Arc::new(alien_gcp_clients::iam::IamClient::new(
                reqwest::Client::new(),
                config(env("GCP_ADMIN_TOKEN")),
            )),
            env("GCP_NOEXEC_SA"),
        )
        .sign()
        .await
        .expect("the admin can sign for the identity without execute")
        .jwt;
        let routing = first[0].headers["X-Sandbox-Routing-Token"].clone();
        for (label, port) in [("preview port", ports[0]), ("agent port", AGENT_PORT)] {
            let denied = reqwest::Client::new()
                .get(format!("https://{host}/v1/preview/{}", ports[0]))
                .header("Authorization", format!("Bearer {noexec_jwt}"))
                .header("X-Sandbox-Routing-Token", &routing)
                .header("X-Sandbox-Port", port.to_string())
                .send()
                .await
                .expect("the edge answers");
            assert!(
                refused(denied.status().as_u16()),
                "{label}: the edge must refuse a JWT without execute: {}",
                denied.status()
            );
            eprintln!("PASS edge refuses a JWT without execute on the {label}: {}", denied.status());
        }
        let exec = reqwest::Client::new()
            .post(format!("https://{host}/v1/exec"))
            .header("Authorization", format!("Bearer {noexec_jwt}"))
            .header("X-Sandbox-Routing-Token", &routing)
            .header("X-Sandbox-Port", AGENT_PORT.to_string())
            .json(&serde_json::json!({"command": ["id"], "timeoutMs": 5000}))
            .send()
            .await
            .expect("the edge answers");
        assert!(
            refused(exec.status().as_u16()),
            "POST /v1/exec with a JWT without execute: {}",
            exec.status()
        );
        eprintln!("PASS edge refuses POST /v1/exec for a JWT without execute: {}", exec.status());
        let api = reqwest::Client::new()
            .post(format!(
                "https://{region}-aiplatform.googleapis.com/v1/projects/{}/locations/{region}/reasoningEngines/{engine}/sandboxEnvironments/{sandbox_id}:execute",
                env("GCP_PROJECT")
            ))
            .header("Authorization", format!("Bearer {noexec_jwt}"))
            .json(&serde_json::json!({"inputs": [{"data": "e30=", "mimeType": "application/json"}]}))
            .send()
            .await
            .expect("the API answers");
        assert!(
            refused(api.status().as_u16()),
            ":execute with a JWT without execute: {}",
            api.status()
        );
        eprintln!("PASS :execute refuses a JWT without execute: {}", api.status());

        // The edge credential ends after 300 seconds. The old headers stop working; a fresh call
        // returns working ones and the daemon keeps answering over HTTP and WebSocket.
        let wait = Duration::from_secs(310).saturating_sub(issued.elapsed());
        eprintln!("waiting {}s for the edge credential to end", wait.as_secs());
        tokio::time::sleep(wait).await;
        let (status, _) = request(
            &first[0],
            reqwest::Method::GET,
            "/whoami",
            Some(&format!("Bearer {daemon_token}")),
            None,
        )
        .await;
        assert!(
            refused(status),
            "the edge credential must be refused after it ends: {status}"
        );
        eprintln!("PASS the credential issued before the wait is refused after 300 s: {status}");
        let refreshed = owner
            .preview(&sandbox_id, ports[0])
            .await
            .expect("a refreshed preview");
        assert_http(&refreshed, ports[0], &daemon_token).await;
        assert_websocket(&refreshed, ports[0], &daemon_token).await;
        eprintln!("PASS refresh after 300 s: HTTP and WebSocket continue");
    })
    .catch_unwind()
    .await;

    live.stop().await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// The workload's own daemon, which authenticates in its own way: the caller's bearer over HTTP,
/// and a credential carried in a subprotocol of its own over WebSocket, which it answers with a
/// protocol of its choosing. The names come from the environment so the test knows no daemon:
/// `GCP_IMAGE`, `GCP_APP_BEARER`, `GCP_APP_HTTP_HEADER` (default `Authorization`),
/// `GCP_APP_HTTP_PATH` (POSTed), `GCP_APP_WS_PATH`,
/// `GCP_APP_SUBPROTOCOLS` (comma separated, offered first) and `GCP_APP_PICKS` (the one the daemon
/// answers with). The daemon listens on 8080.
#[tokio::test]
#[ignore = "needs a live GCP project, an engine, an image that carries the daemon and four identities"]
async fn live_preview_reaches_a_daemon_with_its_own_protocol() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    require_env(&[
        "GCP_PROJECT",
        "GCP_REGION",
        "GCP_ENGINE",
        "GCP_IMAGE",
        "GCP_PREVIEW_SA",
        "GCP_OWNER_TOKEN",
        "GCP_ADMIN_TOKEN",
        "GCP_APP_BEARER",
        "GCP_APP_HTTP_PATH",
        "GCP_APP_WS_PATH",
        "GCP_APP_SUBPROTOCOLS",
        "GCP_APP_PICKS",
    ]);
    let live = Live::start(&env("GCP_IMAGE"), &[8080]).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let preview = live
            .owner
            .preview(&live.sandbox_id, 8080)
            .await
            .expect("owner preview");
        // The caller's bearer, under the header the daemon reads it from (`Authorization` unless
        // `GCP_APP_HTTP_HEADER` names another), sent with the caller's own header and no other.
        let header =
            std::env::var("GCP_APP_HTTP_HEADER").unwrap_or_else(|_| "Authorization".into());
        let credential = env("GCP_APP_BEARER");
        let path = env("GCP_APP_HTTP_PATH");
        let post = |credential: String| {
            let (preview, path, header) = (&preview, &path, &header);
            async move {
                // The edge consumes `Authorization` for its own credential, and the agent reads
                // the caller's bearer from `caller_authorization_header` only. A daemon that
                // reads `Authorization` therefore gets the credential through `bearer`; a
                // daemon that reads another header gets it as that header, unchanged.
                let (bearer, extra): (Option<&str>, Vec<(&str, &str)>) =
                    if header.eq_ignore_ascii_case("authorization") {
                        (Some(credential.as_str()), Vec::new())
                    } else {
                        (None, vec![(header.as_str(), credential.as_str())])
                    };
                request_with(
                    preview,
                    reqwest::Method::POST,
                    path,
                    bearer,
                    Some(serde_json::json!({})),
                    &extra,
                )
                .await
            }
        };
        // The daemon starts after the agent; the agent answers 502 until it listens.
        let mut answer = (0, String::new());
        for _ in 0..60 {
            answer = post(credential.clone()).await;
            if answer.0 != 502 {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        assert!(
            !refused(answer.0) && answer.0 != 502,
            "the daemon must accept the caller's own bearer: {}",
            answer.0
        );
        let (wrong, _) = post("wrong".to_string()).await;
        assert!(
            refused(wrong),
            "the daemon, not the broker, checks the bearer: {wrong}"
        );
        eprintln!(
            "PASS own daemon http: accepted bearer {}, wrong bearer {wrong}",
            answer.0
        );

        let own = env("GCP_APP_SUBPROTOCOLS");
        let own: Vec<&str> = own.split(',').map(str::trim).collect();
        let (mut socket, response) = open_socket(&preview, &env("GCP_APP_WS_PATH"), &own, "")
            .await
            .expect("the daemon's own WebSocket upgrade through the preview");
        let picked = response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert_eq!(
            picked,
            env("GCP_APP_PICKS"),
            "the daemon's own protocol is the one answered"
        );
        socket.close(None).await.ok();
        eprintln!(
            "PASS own daemon websocket: {} and the daemon answered its own protocol",
            response.status()
        );
    })
    .catch_unwind()
    .await;
    live.stop().await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// Interactive commands through the edge on a real sandbox: a command that streams for longer than
/// the 300 second edge credential lives, takes input, ends with its own exit code, then a signal
/// and a closed stdin. Same env and image as `live_preview_is_owner_access`; the image needs
/// `/bin/sh` and coreutils.
///
/// ```text
/// cargo test -p alien-bindings --features gcp --test gcp_sandbox_preview_live -- \
///     --ignored --nocapture --exact live_interactive_commands_outlive_the_edge_credential
/// ```
#[tokio::test]
#[ignore = "needs a live GCP project, an engine, an image and four identities"]
async fn live_interactive_commands_outlive_the_edge_credential() {
    use alien_bindings::traits::{CommandOutput, CommandSignal, RunCommandRequest};

    let live = Live::start(&env("GCP_IMAGE"), &[8080]).await;
    let sandbox_id = live.sandbox_id.clone();
    let owner = &live.owner;
    let sh = |script: &str, timeout: Duration| RunCommandRequest {
        command: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        cwd: None,
        env: Default::default(),
        timeout,
    };

    let outcome = std::panic::AssertUnwindSafe(async {
        assert!(
            owner.capabilities().interactive_commands,
            "a binding with the edge signer reports interactive commands"
        );

        // One tick a minute for seven minutes, then a line from stdin, then exit 5.
        let started = std::time::Instant::now();
        let mut command = owner
            .run_interactive(
                &sandbox_id,
                sh(
                    "i=0; while [ $i -lt 7 ]; do echo tick$i; i=$((i+1)); sleep 60; done; \
                     read line; echo got:$line; exit 5",
                    Duration::from_secs(900),
                ),
            )
            .await
            .expect("the stream opens");
        let mut seen = Vec::new();
        let mut last_tick = Duration::ZERO;
        let mut sent = false;
        let mut exit = None;
        while let Some(item) = command.output.next().await {
            match item.expect("a frame, not a transport failure") {
                CommandOutput::Stdout { data, .. } => {
                    let text = String::from_utf8_lossy(&data).to_string();
                    eprintln!("[{:>4}s] {}", started.elapsed().as_secs(), text.trim_end());
                    if text.contains("tick6") {
                        last_tick = started.elapsed();
                        // The write comes seven minutes in, long after the first credential ended.
                        command.input.write(b"late\n").await.expect("late write");
                        sent = true;
                    }
                    seen.push(text);
                }
                CommandOutput::Exit { code, .. } => exit = Some(code),
                _ => {}
            }
        }
        assert!(
            sent && last_tick > Duration::from_secs(330),
            "ticks reached past 5 minutes"
        );
        assert!(
            seen.iter().any(|line| line.contains("got:late")),
            "{seen:?}"
        );
        assert_eq!(exit, Some(5));
        eprintln!(
            "PASS long command: {}s, exit 5",
            started.elapsed().as_secs()
        );

        // A signal ends a running command, and closing stdin ends a reader.
        let command = owner
            .run_interactive(&sandbox_id, sh("exec sleep 600", Duration::from_secs(900)))
            .await
            .expect("sleep starts");
        command
            .input
            .signal(CommandSignal::Terminate)
            .await
            .expect("signal");
        let ended: Vec<_> = command.output.collect().await;
        let code = ended
            .iter()
            .find_map(|item| match item {
                Ok(CommandOutput::Exit { code, .. }) => Some(*code),
                _ => None,
            })
            .expect("a terminal frame");
        assert_ne!(code, 0, "a signalled command does not exit cleanly");
        eprintln!("PASS signal: exit {code}");

        let command = owner
            .run_interactive(&sandbox_id, sh("cat", Duration::from_secs(60)))
            .await
            .expect("cat starts");
        command.input.write(b"x\n").await.expect("write");
        command.input.close_stdin().await.expect("close");
        let ended: Vec<_> = command.output.collect().await;
        assert!(ended
            .iter()
            .any(|item| matches!(item, Ok(CommandOutput::Exit { code: 0, .. }))));
        eprintln!("PASS close stdin");
    })
    .catch_unwind()
    .await;

    live.stop().await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
