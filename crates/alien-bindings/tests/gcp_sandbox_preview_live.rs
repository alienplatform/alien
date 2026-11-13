//! Preview on a real GCP Agent Platform sandbox: HTTP and WebSocket on three declared ports, the
//! caller's own bearer reaching the daemon, and every refusal. Ignored: it needs a project with a
//! reasoning engine, a template that declares the ports, an image with the agent and a daemon on
//! those ports, and a service account the edge accepts.
//!
//! Env: `GCP_ACCESS_TOKEN`, `GCP_PROJECT`, `GCP_REGION`, `GCP_ENGINE` and `GCP_TEMPLATE` (full
//! resource names), `GCP_PREVIEW_SA` (the identity the caller may sign as), `GCP_PREVIEW_PORTS`
//! (comma separated), `GCP_UNDECLARED_PORT` (a port the daemon serves but the template does not
//! declare), `GCP_PREVIEW_WS_CHECK` (a program that drives the WebSocket checks from the preview
//! JSON file it is given).
//!
//! ```text
//! cargo test -p alien-bindings --features gcp --test gcp_sandbox_preview_live -- --ignored --nocapture
//! ```

#![cfg(feature = "gcp")]

use std::sync::Arc;

use alien_bindings::providers::sandbox::gcp_agent_platform::{
    GcpAgentPlatformSandbox, IamEdgeTokenSigner,
};
use alien_bindings::traits::{CreateSandboxRequest, PreviewCapability, Sandbox};
use alien_core::{GcpClientConfig, GcpCredentials};
use alien_gcp_clients::gcp::agent_platform::{AgentPlatformApi, AgentPlatformClient};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn config() -> GcpClientConfig {
    GcpClientConfig {
        project_id: env("GCP_PROJECT"),
        region: env("GCP_REGION"),
        credentials: GcpCredentials::AccessToken {
            token: env("GCP_ACCESS_TOKEN"),
        },
        service_overrides: None,
        project_number: None,
    }
}

fn provider(ports: &[u16]) -> GcpAgentPlatformSandbox {
    let iam = alien_gcp_clients::iam::IamClient::new(reqwest::Client::new(), config());
    let client = AgentPlatformClient::new(reqwest::Client::new(), config());
    GcpAgentPlatformSandbox::new(
        Arc::new(client),
        env("GCP_ENGINE"),
        env("GCP_TEMPLATE"),
        Some(3600),
    )
    .with_preview_signer(Arc::new(IamEdgeTokenSigner::new(
        Arc::new(iam),
        env("GCP_PREVIEW_SA"),
    )))
    .with_preview_ports(ports.to_vec())
}

async fn get(
    preview: &PreviewCapability,
    path: &str,
    bearer: Option<&str>,
    drop_capability: bool,
    override_headers: &[(&str, &str)],
) -> (u16, String) {
    let mut request = reqwest::Client::new().get(format!("{}{path}", preview.endpoint));
    for (name, value) in &preview.headers {
        if drop_capability && name.starts_with("x-alien-preview") {
            continue;
        }
        if override_headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            continue;
        }
        request = request.header(name, value);
    }
    for (name, value) in override_headers {
        request = request.header(*name, *value);
    }
    if let Some(bearer) = bearer {
        request = request.header(&preview.caller_authorization_header, bearer);
    }
    let response = request.send().await.expect("the edge answers");
    (
        response.status().as_u16(),
        response.text().await.unwrap_or_default(),
    )
}

#[tokio::test]
#[ignore = "needs a live GCP project, an engine, a template and an image"]
async fn live_preview_http_websocket_and_refusals() {
    let ports: Vec<u16> = env("GCP_PREVIEW_PORTS")
        .split(',')
        .map(|port| port.trim().parse().expect("a port"))
        .collect();
    let undeclared: u16 = env("GCP_UNDECLARED_PORT").parse().expect("a port");
    let provider = provider(&ports);

    let first = provider
        .create(CreateSandboxRequest::default())
        .await
        .expect("sandbox 1 starts");
    let second = provider
        .create(CreateSandboxRequest::default())
        .await
        .expect("sandbox 2 starts");
    eprintln!("sandboxes {} and {}", first.sandbox_id, second.sandbox_id);

    let outcome = std::panic::AssertUnwindSafe(async {
        let mut previews = Vec::new();
        for port in &ports {
            let preview = provider.preview(&first.sandbox_id, *port).await.expect("preview");
            assert_eq!(preview.allowed_ports, vec![*port]);
            assert_eq!(preview.caller_authorization_header, "x-alien-caller-authorization");
            assert!(preview.expires_in_seconds <= 900);
            // HTTP: the caller's bearer arrives as Authorization, nothing of ours does.
            let bearer = format!("Bearer daemon-secret-{port}");
            let (status, body) = get(&preview, "/hello?x=1", Some(&bearer), false, &[]).await;
            assert_eq!(status, 200, "port {port}: {body}");
            let seen: serde_json::Value = serde_json::from_str(&body).expect("daemon json");
            assert_eq!(seen["port"], *port, "{body}");
            assert_eq!(seen["path"], "/hello?x=1", "{body}");
            assert_eq!(seen["auth"], bearer, "the caller's bearer must arrive unchanged: {body}");
            eprintln!("PASS http port {port}: 200, daemon saw path {} and Authorization {}", seen["path"], seen["auth"]);
            previews.push(preview);
        }

        // A port the template does not declare is refused by the binding without a call.
        let error = provider.preview(&first.sandbox_id, undeclared).await.expect_err("undeclared");
        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
        eprintln!("PASS undeclared port {undeclared}: refused by the binding without a call");

        // Wrong port: a capability for the first port, aimed at another declared port, and at the
        // port the daemon serves but nobody declared.
        let (status, _) = get(
            &previews[0],
            &format!("/x"),
            Some("Bearer x"),
            false,
            &[("x-alien-preview-port", &ports[1].to_string())],
        )
        .await;
        assert!([401, 403].contains(&status), "header names another port: {status}");
        eprintln!("PASS capability for port {} with the header naming port {}: {status}", ports[0], ports[1]);
        let mut aimed = previews[0].clone();
        aimed.endpoint = aimed.endpoint.replace(
            &format!("/v1/preview/{}", ports[0]),
            &format!("/v1/preview/{}", ports[1]),
        );
        let (status, _) = get(&aimed, "/x", Some("Bearer x"), false, &[]).await;
        assert!([401, 403].contains(&status), "path names another port: {status}");
        eprintln!("PASS capability for port {} aimed at path port {}: {status}", ports[0], ports[1]);
        let mut aimed = previews[0].clone();
        aimed.endpoint = aimed.endpoint.replace(
            &format!("/v1/preview/{}", ports[0]),
            &format!("/v1/preview/{undeclared}"),
        );
        let (status, _) = get(
            &aimed,
            "/x",
            Some("Bearer x"),
            false,
            &[("x-alien-preview-port", &undeclared.to_string())],
        )
        .await;
        assert!([401, 403].contains(&status), "undeclared port: {status}");
        eprintln!("PASS capability aimed at the undeclared port {undeclared} the daemon serves: {status}");
        // No capability at all.
        let (status, _) = get(&previews[0], "/x", Some("Bearer x"), true, &[]).await;
        assert!([401, 403].contains(&status), "no capability: {status}");
        eprintln!("PASS no capability: {status}");

        // Another sandbox's capability: the second sandbox's endpoint and edge credentials with
        // the first sandbox's capability.
        let other = provider.preview(&second.sandbox_id, ports[0]).await.expect("preview 2");
        let capability = previews[0].headers["x-alien-preview-capability"].clone();
        let (status, _) = get(
            &other,
            "/x",
            Some("Bearer x"),
            false,
            &[("x-alien-preview-capability", &capability)],
        )
        .await;
        assert!([401, 403].contains(&status), "another sandbox's capability: {status}");
        eprintln!("PASS another sandbox's capability: {status}");
        // ... and the second sandbox's own capability still works.
        let (status, body) = get(&other, "/x", Some("Bearer y"), false, &[]).await;
        assert_eq!(status, 200, "{body}");

        // Expired: a grant that ends in seconds, asked of the agent directly.
        let client = AgentPlatformClient::new(reqwest::Client::new(), config());
        let engine = env("GCP_ENGINE");
        let engine = engine.rsplit('/').next().unwrap().to_string();
        let soon = chrono::Utc::now().timestamp() + 4;
        let granted = client
            .execute(
                &engine,
                &first.sandbox_id,
                serde_json::json!({"v": 1, "op": "previewGrant", "sandbox": first.sandbox_id, "port": ports[0], "deadline": soon, "declaredPorts": ports})
                    .to_string()
                    .as_bytes(),
            )
            .await
            .expect("short grant");
        let granted: serde_json::Value = serde_json::from_slice(&granted).expect("grant json");
        let short = granted["capability"].as_str().expect("capability").to_string();
        let (status, _) = get(
            &previews[0],
            "/x",
            Some("Bearer z"),
            false,
            &[("x-alien-preview-capability", &short)],
        )
        .await;
        assert_eq!(status, 200, "the short grant works while it lives");
        eprintln!("PASS short-lived capability works while it lives");
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        let (status, _) = get(
            &previews[0],
            "/x",
            Some("Bearer z"),
            false,
            &[("x-alien-preview-capability", &short)],
        )
        .await;
        assert!([401, 403].contains(&status), "expired capability: {status}");
        eprintln!("PASS expired capability: {status}");

        // WebSocket: the preview contracts go to the driver, which speaks the protocol.
        let mut fresh = Vec::new();
        for port in &ports {
            fresh.push(provider.preview(&first.sandbox_id, *port).await.expect("preview"));
        }
        let path = std::env::temp_dir().join("a2657-previews.json");
        std::fs::write(&path, serde_json::to_vec(&fresh).unwrap()).unwrap();
        let driver = env("GCP_PREVIEW_WS_CHECK");
        let result = std::process::Command::new(driver).arg(&path).output().expect("driver runs");
        let _ = std::fs::remove_file(&path);
        eprintln!("{}", String::from_utf8_lossy(&result.stdout));
        eprintln!("{}", String::from_utf8_lossy(&result.stderr));
        assert!(result.status.success(), "the WebSocket checks failed");
    })
    .catch_unwind_async()
    .await;

    for id in [&first.sandbox_id, &second.sandbox_id] {
        if let Err(error) = provider.terminate(id).await {
            eprintln!("terminate {id}: {error}");
        }
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

trait CatchUnwindAsync: std::future::Future + Sized {
    async fn catch_unwind_async(self) -> Result<Self::Output, Box<dyn std::any::Any + Send>>;
}

impl<F: std::future::Future + std::panic::UnwindSafe> CatchUnwindAsync for F {
    async fn catch_unwind_async(self) -> Result<F::Output, Box<dyn std::any::Any + Send>> {
        futures::FutureExt::catch_unwind(self).await
    }
}
