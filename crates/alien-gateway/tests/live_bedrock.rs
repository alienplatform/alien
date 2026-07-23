//! Live verification against real AWS Bedrock. Ignored by default — it makes a
//! real inference call and needs ambient AWS credentials in the environment.
//!
//! Run it with:
//!   eval "$(aws configure export-credentials --profile <p> --format env)"
//!   cargo test -p alien-gateway --test live_bedrock -- --ignored --nocapture
//!
//! This is the end-to-end proof that the Rust SigV4 signer produces a signature
//! Bedrock accepts: the gateway rewrites the public model id, signs with the SDK
//! default credential chain (service `bedrock`), and forwards to the real
//! `/openai/v1/chat/completions` endpoint — no static key, no mock.

use std::net::Ipv4Addr;

use alien_core::Platform;
use alien_gateway::{build_router, AmbientCred, AwsSigV4Cred, GatewayRoute};
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

#[tokio::test]
#[ignore = "hits real AWS Bedrock; needs ambient AWS credentials in the environment"]
async fn live_bedrock_openai_chat() {
    let cred = AmbientCred::Aws(
        AwsSigV4Cred::new("us-east-2")
            .await
            .expect("resolve AWS credentials from the default chain"),
    );
    let route = GatewayRoute {
        name: "llm".to_string(),
        cloud: Platform::Aws,
        region: Some("us-east-2".to_string()),
        project: None,
        azure_endpoint: None,
        cred,
        upstream_base_override: None,
        tuned: None,
    };

    let base = serve(build_router(vec![route])).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/llm/v1/chat/completions"))
        .json(&json!({
            "model": "gpt-oss-20b",
            "messages": [{ "role": "user", "content": "Reply with exactly: pong" }],
            "max_completion_tokens": 1024,
            "reasoning_effort": "low"
        }))
        .send()
        .await
        .expect("request to the gateway");

    let status = resp.status();
    let body: Value = resp.json().await.expect("gateway response should be JSON");
    eprintln!("live bedrock status={status} body={body}");

    assert!(
        status.is_success(),
        "Bedrock must accept the Rust-signed request; got {status}: {body}"
    );
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    assert!(
        content.to_lowercase().contains("pong"),
        "expected a 'pong' completion, got: {content:?}"
    );
}

#[tokio::test]
#[ignore = "hits real AWS Bedrock Claude via classic InvokeModel; needs a console model grant + ambient AWS credentials"]
async fn live_bedrock_claude_streaming() {
    // The end-to-end proof for the Claude path: classic Bedrock InvokeModel with
    // response streaming, whose event-stream frames the gateway decodes back into
    // Anthropic SSE. Unlike the mock tests (which replay only the frames we chose),
    // this exercises the decoder's frame-shape classification against Bedrock's REAL
    // frame sequence — the one thing that reveals whether a benign non-chunk frame
    // would be misclassified as an error and truncate a healthy stream.
    let cred = AmbientCred::Aws(
        AwsSigV4Cred::new("us-east-2")
            .await
            .expect("resolve AWS credentials from the default chain"),
    );
    let route = GatewayRoute {
        name: "llm".to_string(),
        cloud: Platform::Aws,
        region: Some("us-east-2".to_string()),
        project: None,
        azure_endpoint: None,
        cred,
        upstream_base_override: None,
        tuned: None,
    };

    let base = serve(build_router(vec![route])).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/llm/v1/messages"))
        .json(&json!({
            "model": "claude-haiku-4.5",
            "stream": true,
            "max_tokens": 64,
            "messages": [{ "role": "user", "content": "Reply with exactly: pong" }]
        }))
        .send()
        .await
        .expect("request to the gateway");

    let status = resp.status();
    let body = resp.text().await.expect("gateway response body");
    eprintln!("live claude stream status={status}\n{body}");

    assert!(
        status.is_success(),
        "Claude via classic Bedrock must accept the request; got {status}: {body}"
    );
    assert!(
        body.contains("event: content_block_delta") || body.contains("\"text\""),
        "expected decoded Anthropic SSE deltas: {body}"
    );
    // A healthy stream must NOT trip the decoder's error branch: if it does, the
    // payload-shape frame heuristic misclassified a real Bedrock frame.
    assert!(
        !body.contains("event: error"),
        "a healthy Claude stream must not surface a decoder error: {body}"
    );
    // The reply text streams as separate text_delta events ("p" then "ong"), so
    // reconstruct it before asserting rather than expecting a contiguous substring.
    let reply: String = body
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .filter(|event| event["type"] == "content_block_delta")
        .filter_map(|event| event["delta"]["text"].as_str().map(str::to_owned))
        .collect();
    assert!(
        reply.to_lowercase().contains("pong"),
        "expected a 'pong' completion; reconstructed {reply:?} from stream: {body}"
    );
}
