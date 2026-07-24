//! Live verification against real AWS Bedrock. Reads the shared alien-test-target
//! credentials from the workspace-root `.env.test`, so it runs in the credentialed
//! e2e job (and locally whenever that file is present).
//!
//! This is the end-to-end proof that the Rust SigV4 signer produces a signature
//! Bedrock accepts: the gateway rewrites the public model id, signs with the SDK
//! default credential chain (service `bedrock`), and forwards to the real
//! `/openai/v1/chat/completions` endpoint, with no static key and no mock.

use std::net::Ipv4Addr;

use alien_core::Platform;
use alien_ai_gateway::{build_router, AmbientCred, AwsSigV4Cred, GatewayRoute};
use serde_json::{json, Value};

/// Load the shared alien-test-target credentials from the workspace-root
/// `.env.test` and expose the AWS target account under the SDK default-chain
/// variable names, so the SigV4 signer resolves them exactly as a deployed
/// workload's ambient identity would.
fn load_test_env() {
    let root = workspace_root::get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("load .env.test from the workspace root");
    for (from, to) in [
        ("AWS_TARGET_ACCESS_KEY_ID", "AWS_ACCESS_KEY_ID"),
        ("AWS_TARGET_SECRET_ACCESS_KEY", "AWS_SECRET_ACCESS_KEY"),
    ] {
        // Fail loud rather than fall through to whatever AWS creds are already
        // ambient (e.g. a developer's own profile): this test must sign as the
        // alien-test-target account, so a missing var is a setup error.
        let value = std::env::var(from).unwrap_or_else(|_| panic!("{from} must be set in .env.test"));
        std::env::set_var(to, value);
    }
}

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
async fn live_bedrock_openai_chat() {
    load_test_env();
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

    // A 429 rate-limit is exactly what this test proves: Bedrock accepted the Rust
    // SigV4 signature and routed the request (it reached the per-account daily token
    // quota, well past auth). A signing regression would surface as 401/403 instead.
    assert!(
        status.is_success() || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
        "Bedrock must accept the Rust-signed request (2xx, or 429 when the daily token quota is spent); got {status}: {body}"
    );
    // Only assert on the completion text when a completion actually came back.
    if status.is_success() {
        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default();
        assert!(
            content.to_lowercase().contains("pong"),
            "expected a 'pong' completion, got: {content:?}"
        );
    }
}

#[tokio::test]
#[ignore = "Bedrock Claude via classic InvokeModel; reads .env.test but needs a Bedrock Claude model grant on alien-test-target (pending); un-ignore once granted"]
async fn live_bedrock_claude_streaming() {
    load_test_env();
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
