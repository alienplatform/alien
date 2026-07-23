//! Live verification of Claude on GCP Vertex. Ignored by default — it makes a
//! real inference call and needs a project + access token in the environment.
//!
//! Run it with:
//!   export GCP_PROJECT=<project-id>
//!   export GCP_ACCESS_TOKEN="$(gcloud auth print-access-token)"
//!   cargo test -p alien-gateway --test live_vertex_claude -- --ignored --nocapture
//!
//! This is the end-to-end proof for the Vertex Claude arm: the gateway moves the
//! model id into the `:rawPredict` URL, injects the Vertex version marker, and the
//! project's Model Garden entitlement accepts the call — the one thing code
//! reading cannot establish.

use std::net::Ipv4Addr;

use alien_core::Platform;
use alien_gateway::{build_router, AmbientCred, BearerTokenCred, GatewayRoute};
use serde_json::{json, Value};

async fn serve(router: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind test server");
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    url
}

fn vertex_route() -> GatewayRoute {
    let project = std::env::var("GCP_PROJECT").expect("GCP_PROJECT must name the target project");
    let token =
        std::env::var("GCP_ACCESS_TOKEN").expect("GCP_ACCESS_TOKEN must hold a bearer token");
    GatewayRoute {
        name: "llm".to_string(),
        cloud: Platform::Gcp,
        region: Some("global".to_string()),
        project: Some(project),
        azure_endpoint: None,
        cred: AmbientCred::Bearer(BearerTokenCred::static_token(token)),
        upstream_base_override: None,
        tuned: None,
        finetune: None,
    }
}

#[tokio::test]
#[ignore = "hits real Vertex Claude; needs GCP_PROJECT + GCP_ACCESS_TOKEN and a Model Garden grant"]
async fn live_vertex_claude_messages() {
    let base = serve(build_router(vec![vertex_route()])).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/llm/v1/messages"))
        .json(&json!({
            "model": "claude-haiku-4.5",
            "max_tokens": 64,
            "messages": [{ "role": "user", "content": "Reply with exactly: pong" }]
        }))
        .send()
        .await
        .expect("request to the gateway");

    let status = resp.status();
    let text = resp.text().await.expect("gateway response body");
    eprintln!("live vertex claude status={status} body={text}");
    let body: Value = serde_json::from_str(&text).expect("gateway response should be JSON");

    assert!(
        status.is_success(),
        "Vertex must accept the rawPredict request; got {status}: {body}"
    );
    let content = body["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        content.to_lowercase().contains("pong"),
        "expected a 'pong' reply, got: {content:?}"
    );
}

#[tokio::test]
#[ignore = "hits real Vertex Claude streaming; needs GCP_PROJECT + GCP_ACCESS_TOKEN and a Model Garden grant"]
async fn live_vertex_claude_streaming() {
    // Streaming picks the `:streamRawPredict` verb and must come back as native
    // Anthropic SSE — message_start through message_stop, passed through untouched.
    let base = serve(build_router(vec![vertex_route()])).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/llm/v1/messages"))
        .json(&json!({
            "model": "claude-haiku-4.5",
            "max_tokens": 64,
            "stream": true,
            "messages": [{ "role": "user", "content": "Reply with exactly: pong" }]
        }))
        .send()
        .await
        .expect("request to the gateway");

    let status = resp.status();
    assert!(status.is_success(), "Vertex streaming must return 2xx; got {status}");
    let body = resp.text().await.expect("stream body");
    let head: String = body.chars().take(400).collect();
    eprintln!("live vertex claude stream head: {head}");
    assert!(body.contains("message_start"), "SSE must open with message_start: {body}");
    assert!(body.contains("message_stop"), "SSE must close with message_stop: {body}");
}
