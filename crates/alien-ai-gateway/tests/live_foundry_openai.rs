//! Live verification of the Azure OpenAI deployments through the gateway:
//! Responses with the hosted `web_search` tool, and embeddings. Ignored by
//! default because it makes real inference calls.
//!
//! Run it with:
//!   export AZURE_AI_ENDPOINT="https://<resource>.cognitiveservices.azure.com/"
//!   export AZURE_ACCESS_TOKEN="<Entra token, audience https://ai.azure.com>"
//!   cargo test -p alien-ai-gateway --test live_foundry_openai -- --ignored --nocapture

use std::net::Ipv4Addr;

use alien_ai_gateway::{build_router, AmbientCred, BearerTokenCred, GatewayRoute, GatewayTarget};
use alien_core::Platform;
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
    GatewayRoute {
        name: "llm".to_string(),
        target: GatewayTarget::Cloud(Platform::Azure),
        region: None,
        project: None,
        azure_endpoint: Some(std::env::var("AZURE_AI_ENDPOINT").expect("AZURE_AI_ENDPOINT")),
        cred: AmbientCred::Bearer(BearerTokenCred::static_token(
            std::env::var("AZURE_ACCESS_TOKEN").expect("AZURE_ACCESS_TOKEN"),
        )),
        upstream_base_override: None,
        additional_headers: Default::default(),
    }
}

#[tokio::test]
#[ignore = "hits real Azure OpenAI deployments; needs AZURE_AI_ENDPOINT and AZURE_ACCESS_TOKEN"]
async fn live_responses_web_search_and_embeddings() {
    let base = serve(build_router(vec![foundry_route()])).await;
    let client = reqwest::Client::new();

    for model in ["gpt-5.5", "gpt-5.6-sol"] {
        let resp = client
            .post(format!("{base}/llm/v1/responses"))
            .json(&json!({
                "model": model,
                "input": "Use web search: what is the latest stable version of the Rust programming language? Answer in one sentence.",
                "tools": [{"type": "web_search"}],
            }))
            .send()
            .await
            .expect("responses request");
        let status = resp.status();
        let text = resp.text().await.unwrap();
        eprintln!("{model} responses status={status} body={text}");
        assert!(status.is_success(), "Responses failed: {status}: {text}");
        let body: Value = serde_json::from_str(&text).unwrap();
        let used_search = body["output"]
            .as_array()
            .map(|items| items.iter().any(|i| i["type"] == "web_search_call"))
            .unwrap_or(false);
        assert!(
            used_search,
            "the response must contain a web_search_call item"
        );
    }

    let resp = client
        .post(format!("{base}/llm/v1/embeddings"))
        .json(&json!({"model": "text-embedding-3-large", "input": ["alpha", "beta"]}))
        .send()
        .await
        .expect("embeddings request");
    let status = resp.status();
    let text = resp.text().await.unwrap();
    assert!(status.is_success(), "Embeddings failed: {status}: {text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    let data = body["data"].as_array().expect("data array");
    assert_eq!(data.len(), 2);
    let dims = data[0]["embedding"].as_array().expect("embedding").len();
    eprintln!(
        "embeddings: 2 vectors of {dims} dimensions, usage={}",
        body["usage"]
    );
    assert_eq!(dims, 3072);
}
