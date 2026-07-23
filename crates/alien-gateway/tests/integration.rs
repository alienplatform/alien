//! End-to-end gateway routing across two clouds with mocked upstreams.
//!
//! Builds the real router with an AWS binding and an Azure binding pointed at two
//! mock upstream servers, then drives requests through the running loopback server
//! and asserts each is routed to the right upstream with the model id rewritten (per
//! the alien-core catalog), an ambient auth header injected, and the body streamed
//! back unchanged. Credentials are static (no metadata/network) so the test is
//! hermetic; the live ambient-credential resolution is exercised separately.

use std::net::Ipv4Addr;

use alien_core::Platform;
use alien_gateway::{build_router, AmbientCred, AwsSigV4Cred, BearerTokenCred, GatewayRoute};
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
            cloud: Platform::Aws,
            region: Some("us-east-2".to_string()),
            project: None,
            azure_endpoint: None,
            cred: aws_cred(),
            upstream_base_override: Some(aws_upstream.base_url()),
            tuned: None,
        },
        GatewayRoute {
            name: "azllm".to_string(),
            cloud: Platform::Azure,
            region: None,
            project: None,
            azure_endpoint: Some(azure_upstream.base_url()),
            cred: AmbientCred::Bearer(BearerTokenCred::static_token("test-azure-token")),
            upstream_base_override: Some(azure_upstream.base_url()),
            tuned: None,
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
    assert!(aws_ids.contains(&"gpt-oss-20b"));
    assert!(aws_ids.contains(&"claude-opus-4.8"));
    assert!(!aws_ids.contains(&"gpt-4.1"), "AWS catalog must not list the Azure model");

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
    assert!(az_ids.contains(&"gpt-4.1"));
    assert!(!az_ids.contains(&"gpt-oss-20b"), "Azure catalog must not list the AWS model");
}
