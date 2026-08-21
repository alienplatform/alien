//! End-to-end gateway routing across two clouds with mocked upstreams.
//!
//! Builds the real router with an AWS binding and an Azure binding pointed at two
//! mock upstream servers, then drives requests through the running loopback server
//! and asserts each is routed to the right upstream with the model id rewritten (per
//! the alien-core catalog), an ambient auth header injected, and the body streamed
//! back unchanged. Credentials are static (no metadata/network) so the test is
//! hermetic; the live ambient-credential resolution is exercised separately.

use std::net::Ipv4Addr;

use alien_ai_gateway::{
    build_router, route_from_direct_anthropic, route_from_direct_openai, AmbientCred, AwsSigV4Cred,
    BearerTokenCred, GatewayRoute, GatewayTarget,
};
use alien_core::Platform;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;
use httpmock::prelude::*;
use serde_json::{json, Value};

fn aws_cred() -> AmbientCred {
    let creds = Credentials::new(
        "AKIAIOSFODNN7EXAMPLE",
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        None,
        None,
        "test",
    );
    AmbientCred::Aws(AwsSigV4Cred::with_provider(
        "us-east-2",
        SharedCredentialsProvider::new(creds),
    ))
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
async fn routes_two_clouds_with_rewrite_auth_and_passthrough() {
    let aws_upstream = MockServer::start_async().await;
    let aws_mock = aws_upstream
        .mock_async(|when, then| {
            when.method(POST)
                .path("/openai/v1/chat/completions")
                // Rewritten to the upstream id, and SigV4-signed.
                .body_contains("openai.gpt-oss-20b-1:0")
                .header_exists("authorization");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":"aws","choices":[{"message":{"content":"aws-pong"}}]}"#);
        })
        .await;
    let azure_upstream = MockServer::start_async().await;
    let azure_mock = azure_upstream
        .mock_async(|when, then| {
            when.method(POST)
                .path("/openai/v1/chat/completions")
                .body_contains("gpt-4.1")
                // The static bearer token is injected verbatim.
                .header("authorization", "Bearer test-azure-token");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":"az","choices":[{"message":{"content":"az-pong"}}]}"#);
        })
        .await;

    let routes = vec![
        GatewayRoute {
            name: "llm".to_string(),
            target: GatewayTarget::Cloud(Platform::Aws),
            region: Some("us-east-2".to_string()),
            project: None,
            azure_endpoint: None,
            cred: aws_cred(),
            upstream_base_override: Some(aws_upstream.base_url()),
            additional_headers: Default::default(),
        },
        GatewayRoute {
            name: "azllm".to_string(),
            target: GatewayTarget::Cloud(Platform::Azure),
            region: None,
            project: None,
            azure_endpoint: Some(azure_upstream.base_url()),
            cred: AmbientCred::Bearer(BearerTokenCred::static_token("test-azure-token")),
            upstream_base_override: Some(azure_upstream.base_url()),
            additional_headers: Default::default(),
        },
    ];

    let base = serve(build_router(routes)).await;
    let client = reqwest::Client::new();

    // AWS binding: gpt-oss-20b -> openai.gpt-oss-20b-1:0 on the AWS upstream.
    let aws_resp = client
        .post(format!("{base}/llm/v1/chat/completions"))
        .json(&json!({"model":"gpt-oss-20b","messages":[{"role":"user","content":"hi"}]}))
        .send()
        .await
        .expect("aws request");
    assert_eq!(aws_resp.status(), 200);
    assert!(aws_resp.text().await.unwrap().contains("aws-pong"));
    aws_mock.assert_async().await;

    // Azure binding: gpt-4.1 on the Azure upstream with the bearer token.
    let az_resp = client
        .post(format!("{base}/azllm/v1/chat/completions"))
        .json(&json!({"model":"gpt-4.1","messages":[{"role":"user","content":"hi"}]}))
        .send()
        .await
        .expect("azure request");
    assert_eq!(az_resp.status(), 200);
    assert!(az_resp.text().await.unwrap().contains("az-pong"));
    azure_mock.assert_async().await;

    // Each binding lists its own cloud's curated catalog.
    let aws_models: Value = client
        .get(format!("{base}/llm/v1/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let aws_ids: Vec<&str> = aws_models["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(aws_ids.contains(&"byo/gpt-oss-20b"));
    assert!(aws_ids.contains(&"byo/claude-opus-4.8"));
    assert!(
        !aws_ids.contains(&"byo/gpt-4.1"),
        "AWS catalog must not list the Azure model"
    );

    let az_models: Value = client
        .get(format!("{base}/azllm/v1/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let az_ids: Vec<&str> = az_models["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(az_ids.contains(&"byo/gpt-4.1"));
    assert!(
        !az_ids.contains(&"byo/gpt-oss-20b"),
        "Azure catalog must not list the AWS model"
    );
}

#[tokio::test]
async fn large_body_reaches_the_upstream_instead_of_413() {
    // A permissive upstream that accepts the chat/completions path regardless of size.
    let upstream = MockServer::start_async().await;
    let upstream_mock = upstream
        .mock_async(|when, then| {
            when.method(POST).path("/openai/v1/chat/completions");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":"ok","choices":[{"message":{"content":"pong"}}]}"#);
        })
        .await;

    let routes = vec![GatewayRoute {
        name: "llm".to_string(),
        target: GatewayTarget::Cloud(Platform::Aws),
        region: Some("us-east-2".to_string()),
        project: None,
        azure_endpoint: None,
        cred: aws_cred(),
        upstream_base_override: Some(upstream.base_url()),
        additional_headers: Default::default(),
    }];
    let base = serve(build_router(routes)).await;
    let client = reqwest::Client::new();

    // Past axum's 2 MB default, which one base64 vision image already exceeds, but well
    // under our own cap.
    let big_prompt = "x".repeat(3 * 1024 * 1024);
    let resp = client
        .post(format!("{base}/llm/v1/chat/completions"))
        .json(&json!({"model":"gpt-oss-20b","messages":[{"role":"user","content":big_prompt}]}))
        .send()
        .await
        .expect("large request");

    assert_eq!(
        resp.status(),
        200,
        "a >2 MB body must reach the upstream, not be rejected as 413"
    );
    assert!(resp.text().await.unwrap().contains("pong"));
    upstream_mock.assert_async().await;
}

#[tokio::test]
async fn body_past_the_cap_never_reaches_the_upstream() {
    // The upstream would accept any size, so a 413 here can only come from our own cap.
    let upstream = MockServer::start_async().await;
    let upstream_mock = upstream
        .mock_async(|when, then| {
            when.method(POST).path("/openai/v1/chat/completions");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":"ok","choices":[{"message":{"content":"pong"}}]}"#);
        })
        .await;

    let routes = vec![GatewayRoute {
        name: "llm".to_string(),
        target: GatewayTarget::Cloud(Platform::Aws),
        region: Some("us-east-2".to_string()),
        project: None,
        azure_endpoint: None,
        cred: aws_cred(),
        upstream_base_override: Some(upstream.base_url()),
        additional_headers: Default::default(),
    }];
    let base = serve(build_router(routes)).await;
    let client = reqwest::Client::new();

    let past_cap = "x".repeat(33 * 1024 * 1024);
    let resp = client
        .post(format!("{base}/llm/v1/chat/completions"))
        .json(&json!({"model":"gpt-oss-20b","messages":[{"role":"user","content":past_cap}]}))
        .send()
        .await
        .expect("oversized request");

    assert_eq!(
        resp.status(),
        413,
        "a body past the cap must be rejected, not buffered"
    );
    let error: serde_json::Value = resp.json().await.expect("a structured error body");
    assert_eq!(
        error["code"], "GATEWAY_REQUEST_TOO_LARGE",
        "the rejection must carry this crate's error envelope, not axum's plain text"
    );
    assert_eq!(
        upstream_mock.hits_async().await,
        0,
        "the oversized body must be rejected before any upstream call"
    );
}

#[tokio::test]
async fn direct_anthropic_is_fixed_to_messages_and_injects_only_its_api_key() {
    let upstream = MockServer::start_async().await;
    let messages = upstream
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/messages")
                .header("x-api-key", "sk-ant-api03-test-secret")
                .header("anthropic-version", "2023-06-01")
                .body_contains("claude-sonnet-4-6");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":"msg_direct","content":[{"type":"text","text":"pong"}]}"#);
        })
        .await;

    let mut route = route_from_direct_anthropic("direct", "sk-ant-api03-test-secret")
        .expect("standard API key");
    route.upstream_base_override = Some(upstream.base_url());
    let base = serve(build_router(vec![route])).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/direct/v1/messages"))
        .json(&json!({
            "model": "claude-sonnet-4.6",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("direct request");
    assert_eq!(response.status(), 200);
    assert!(response.text().await.unwrap().contains("msg_direct"));
    messages.assert_async().await;

    let wrong_protocol = client
        .post(format!("{base}/direct/v1/chat/completions"))
        .json(&json!({"model": "claude-sonnet-4.6", "messages": []}))
        .send()
        .await
        .expect("wrong protocol response");
    assert_eq!(wrong_protocol.status(), 400);
    assert_eq!(messages.hits_async().await, 1);

    assert!(route_from_direct_anthropic("direct", "sk-ant-admin-test").is_err());
}

#[tokio::test]
async fn direct_openai_is_fixed_to_openai_endpoints_and_injects_bearer_auth() {
    let upstream = MockServer::start_async().await;
    let chat = upstream
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/chat/completions")
                .header("authorization", "Bearer sk-proj-test-secret")
                .body_contains("gpt-4.1-mini");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":"chat_direct","choices":[]}"#);
        })
        .await;
    let responses = upstream
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/responses")
                .header("authorization", "Bearer sk-proj-test-secret")
                .body_contains("gpt-4.1-mini");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"id":"resp_direct","output":[]}"#);
        })
        .await;

    let mut route =
        route_from_direct_openai("direct", "sk-proj-test-secret").expect("valid API key");
    route.upstream_base_override = Some(upstream.base_url());
    let base = serve(build_router(vec![route])).await;
    let client = reqwest::Client::new();

    let chat_response = client
        .post(format!("{base}/direct/v1/chat/completions"))
        .json(&json!({"model": "gpt-4.1-mini", "messages": []}))
        .send()
        .await
        .expect("chat request");
    assert_eq!(chat_response.status(), 200);

    let responses_response = client
        .post(format!("{base}/direct/v1/responses"))
        .json(&json!({"model": "gpt-4.1-mini", "input": "hello"}))
        .send()
        .await
        .expect("responses request");
    assert_eq!(responses_response.status(), 200);

    let wrong_protocol = client
        .post(format!("{base}/direct/v1/messages"))
        .json(&json!({"model": "gpt-4.1-mini", "messages": []}))
        .send()
        .await
        .expect("wrong protocol response");
    assert_eq!(wrong_protocol.status(), 400);
    assert_eq!(chat.hits_async().await, 1);
    chat.assert_async().await;
    responses.assert_async().await;
    assert!(route_from_direct_openai("direct", "contains whitespace").is_err());
}
