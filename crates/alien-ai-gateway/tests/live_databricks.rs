//! Live Databricks verification through Alien's three public APIs.
//!
//! Run with:
//!   cargo test -p alien-ai-gateway --test live_databricks -- --ignored --nocapture

use std::net::Ipv4Addr;
use std::time::Duration;

use alien_ai_gateway::{build_router, route_from_direct_databricks};
use serde_json::{json, Value};

fn load_test_env() {
    let root = workspace_root::get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("load .env.test from the repository root");
}

async fn access_token(client: &reqwest::Client, workspace: &str) -> String {
    let client_id =
        std::env::var("DATABRICKS_TEST_CLIENT_ID").expect("DATABRICKS_TEST_CLIENT_ID must be set");
    let client_secret = std::env::var("DATABRICKS_TEST_CLIENT_SECRET")
        .expect("DATABRICKS_TEST_CLIENT_SECRET must be set");
    let response = client
        .post(format!("{workspace}/oidc/v1/token"))
        .basic_auth(client_id, Some(client_secret))
        .form(&[("grant_type", "client_credentials"), ("scope", "all-apis")])
        .send()
        .await
        .expect("Databricks OAuth request");
    let status = response.status();
    let body: Value = response.json().await.expect("Databricks OAuth JSON");
    assert!(
        status.is_success(),
        "Databricks OAuth failed: {status}: {body}"
    );
    body["access_token"]
        .as_str()
        .expect("Databricks access_token")
        .to_string()
}

async fn gateway() -> String {
    load_test_env();
    let workspace = std::env::var("DATABRICKS_TEST_WORKSPACE_URL")
        .expect("DATABRICKS_TEST_WORKSPACE_URL must be set")
        .trim_end_matches('/')
        .to_string();
    let client = reqwest::Client::new();
    let token = access_token(&client, &workspace).await;
    let route = route_from_direct_databricks("llm", &workspace, token).expect("Databricks route");
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind test gateway");
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, build_router(vec![route]))
            .await
            .unwrap();
    });
    url
}

async fn json_response(response: reqwest::Response, scenario: &str) -> Value {
    let status = response.status();
    let text = response.text().await.expect("read gateway response");
    eprintln!("{scenario}: status={status} body={text}");
    assert!(status.is_success(), "{scenario} failed: {status}: {text}");
    serde_json::from_str(&text).expect("gateway response must be JSON")
}

fn assert_alien_ok(value: &Value, scenario: &str) {
    assert!(
        value.to_string().to_ascii_uppercase().contains("ALIEN_OK"),
        "{scenario} did not contain ALIEN_OK: {value}"
    );
}

async fn exercise_all_public_apis(base: &str, model: &str) {
    let client = reqwest::Client::new();
    let chat = json_response(
        client
            .post(format!("{base}/llm/v1/chat/completions"))
            .json(&json!({
                "model": model,
                "messages": [{"role": "user", "content": "Reply with exactly ALIEN_OK"}],
                "max_tokens": 128
            }))
            .send()
            .await
            .expect("send Chat request"),
        &format!("Databricks {model} Chat"),
    )
    .await;
    assert_alien_ok(&chat, &format!("Databricks {model} Chat"));
    tokio::time::sleep(Duration::from_secs(1)).await;

    let responses = json_response(
        client
            .post(format!("{base}/llm/v1/responses"))
            .json(&json!({
                "model": model,
                "input": "Reply with exactly ALIEN_OK",
                "max_output_tokens": 128
            }))
            .send()
            .await
            .expect("send Responses request"),
        &format!("Databricks {model} Responses"),
    )
    .await;
    assert_alien_ok(&responses, &format!("Databricks {model} Responses"));
    tokio::time::sleep(Duration::from_secs(1)).await;

    let messages = json_response(
        client
            .post(format!("{base}/llm/v1/messages"))
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model,
                "messages": [{"role": "user", "content": "Reply with exactly ALIEN_OK"}],
                "max_tokens": 128
            }))
            .send()
            .await
            .expect("send Messages request"),
        &format!("Databricks {model} Messages"),
    )
    .await;
    assert_alien_ok(&messages, &format!("Databricks {model} Messages"));
    tokio::time::sleep(Duration::from_secs(1)).await;
}

#[tokio::test]
#[ignore = "hits real Databricks serving endpoints using OAuth from .env.test"]
async fn live_databricks_routes_across_public_apis_tools_and_streaming() {
    let base = gateway().await;
    exercise_all_public_apis(&base, "gpt-oss-120b").await;

    let client = reqwest::Client::new();
    let tool = json_response(
        client
            .post(format!("{base}/llm/v1/responses"))
            .json(&json!({
                "model": "gpt-oss-120b",
                "input": "Use get_code for Paris.",
                "tools": [{
                    "type": "function",
                    "name": "get_code",
                    "description": "Get a fixed city code",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}},
                        "required": ["city"],
                        "additionalProperties": false
                    }
                }],
                "tool_choice": {"type": "function", "name": "get_code"},
                "max_output_tokens": 128
            }))
            .send()
            .await
            .expect("send Responses tool request"),
        "Databricks GPT-OSS Responses tool call",
    )
    .await;
    assert!(
        tool.to_string().contains("get_code"),
        "Databricks GPT-OSS returned no get_code call: {tool}"
    );
    tokio::time::sleep(Duration::from_secs(1)).await;

    let stream = client
        .post(format!("{base}/llm/v1/messages"))
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": "gpt-oss-120b",
            "messages": [{"role": "user", "content": "Reply with exactly ALIEN_OK"}],
            "max_tokens": 128,
            "stream": true
        }))
        .send()
        .await
        .expect("send translated stream");
    let status = stream.status();
    let body = stream.text().await.expect("read translated stream");
    eprintln!("Databricks GPT-OSS Messages stream: status={status} body={body}");
    assert!(status.is_success(), "translated stream failed: {body}");
    assert!(
        body.contains("message_start"),
        "missing Messages start: {body}"
    );
    assert!(
        body.contains("message_stop"),
        "missing Messages stop: {body}"
    );
    let content = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter_map(|event| {
            event
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<String>();
    assert!(
        content.to_ascii_uppercase().contains("ALIEN_OK"),
        "translated stream did not produce ALIEN_OK; content={content:?}, stream={body}"
    );
    tokio::time::sleep(Duration::from_secs(1)).await;

    exercise_all_public_apis(&base, "gemma-3-12b").await;
}
