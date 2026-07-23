//! Live verification of Claude on Azure Foundry. Ignored by default — it makes a
//! real inference call and needs a Foundry endpoint + Entra token in the
//! environment.
//!
//! Run it with:
//!   export AZURE_AI_ENDPOINT="https://<resource>.cognitiveservices.azure.com/"
//!   export AZURE_ACCESS_TOKEN="$(az account get-access-token --resource https://ai.azure.com --query accessToken -o tsv)"
//!   cargo test -p alien-gateway --test live_foundry_claude -- --ignored --nocapture
//!
//! Besides proving the arm end-to-end, this settles the host/audience question the
//! code left open. AZURE_AI_ENDPOINT must be the account endpoint the AiBinding
//! carries in production (the AIServices account's `properties.endpoint`, the
//! `cognitiveservices.azure.com` shape) — a green run against the
//! `services.ai.azure.com` host would validate a host production bindings never
//! use. Probe order: (1) the binding-carried host with the `ai.azure.com` token
//! above; (2) on 404, the `services.ai.azure.com` host — meaning the arm needs a
//! host derivation; (3) on 401, retry the token with
//! `--resource https://cognitiveservices.azure.com` — meaning the audience swap
//! is unnecessary. Record which combination Foundry accepted.

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

fn foundry_route() -> GatewayRoute {
    let endpoint =
        std::env::var("AZURE_AI_ENDPOINT").expect("AZURE_AI_ENDPOINT must name the account");
    let token =
        std::env::var("AZURE_ACCESS_TOKEN").expect("AZURE_ACCESS_TOKEN must hold an Entra token");
    GatewayRoute {
        name: "llm".to_string(),
        cloud: Platform::Azure,
        region: None,
        project: None,
        azure_endpoint: Some(endpoint),
        cred: AmbientCred::Bearer(BearerTokenCred::static_token(token)),
        upstream_base_override: None,
        tuned: None,
        finetune: None,
    }
}

#[tokio::test]
#[ignore = "hits real Foundry Claude; needs AZURE_AI_ENDPOINT + AZURE_ACCESS_TOKEN and a Claude deployment on the resource"]
async fn live_foundry_claude_messages() {
    let base = serve(build_router(vec![foundry_route()])).await;

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
    eprintln!("live foundry claude status={status} body={text}");
    let body: Value = serde_json::from_str(&text).expect("gateway response should be JSON");

    assert!(
        status.is_success(),
        "Foundry must accept the Messages request; got {status}: {body}"
    );
    let content = body["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        content.to_lowercase().contains("pong"),
        "expected a 'pong' reply, got: {content:?}"
    );
}

#[tokio::test]
#[ignore = "hits real Foundry Claude streaming; needs AZURE_AI_ENDPOINT + AZURE_ACCESS_TOKEN and a Claude deployment on the resource"]
async fn live_foundry_claude_streaming() {
    // Standard Anthropic streaming on the body's `stream` flag; the reply must be
    // native Anthropic SSE passed through untouched.
    let base = serve(build_router(vec![foundry_route()])).await;

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
    assert!(status.is_success(), "Foundry streaming must return 2xx; got {status}");
    let body = resp.text().await.expect("stream body");
    let head: String = body.chars().take(400).collect();
    eprintln!("live foundry claude stream head: {head}");
    assert!(body.contains("message_start"), "SSE must open with message_start: {body}");
    assert!(body.contains("message_stop"), "SSE must close with message_stop: {body}");
}
