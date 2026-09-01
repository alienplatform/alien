//! Live public-API verification for the direct OpenAI and Anthropic routes.
//!
//! Every request enters through Alien's loopback HTTP router. The suite verifies
//! provider authentication, model rewriting, request/response translation, tools,
//! multi-turn state, and streaming against the real providers.
//!
//! Run with:
//!   cargo test -p alien-ai-gateway --test live_direct_providers -- --ignored --nocapture

use std::net::Ipv4Addr;

use alien_ai_gateway::{
    build_router, route_from_direct_anthropic, route_from_direct_openai, GatewayRoute,
};
use serde_json::{json, Value};

const OPENAI_MODEL: &str = "gpt-4.1-mini";
const ANTHROPIC_MODEL: &str = "claude-haiku-4.5";

fn load_test_env() {
    let root = workspace_root::get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("load .env.test from the repository root");
}

async fn serve(route: GatewayRoute) -> String {
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

async fn exercise_public_text_apis(base: &str, model: &str, provider: &str) {
    let client = reqwest::Client::new();
    let chat = json_response(
        client
            .post(format!("{base}/llm/v1/chat/completions"))
            .json(&json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": "Always follow the requested output exactly."},
                    {"role": "user", "content": "Reply with exactly ALIEN_OK"}
                ],
                "max_tokens": 32
            }))
            .send()
            .await
            .expect("send Chat request"),
        &format!("{provider} Chat"),
    )
    .await;
    assert_alien_ok(&chat, &format!("{provider} Chat"));

    let responses = json_response(
        client
            .post(format!("{base}/llm/v1/responses"))
            .json(&json!({
                "model": model,
                "instructions": "Always follow the requested output exactly.",
                "input": "Reply with exactly ALIEN_OK",
                "max_output_tokens": 32
            }))
            .send()
            .await
            .expect("send Responses request"),
        &format!("{provider} Responses"),
    )
    .await;
    assert_alien_ok(&responses, &format!("{provider} Responses"));

    let messages = json_response(
        client
            .post(format!("{base}/llm/v1/messages"))
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model,
                "system": "Always follow the requested output exactly.",
                "messages": [{"role": "user", "content": "Reply with exactly ALIEN_OK"}],
                "max_tokens": 32
            }))
            .send()
            .await
            .expect("send Messages request"),
        &format!("{provider} Messages"),
    )
    .await;
    assert_alien_ok(&messages, &format!("{provider} Messages"));
}

async fn exercise_chat_tool_and_continuation(base: &str, model: &str, provider: &str) {
    let client = reqwest::Client::new();
    let first = json_response(
        client
            .post(format!("{base}/llm/v1/chat/completions"))
            .json(&json!({
                "model": model,
                "messages": [{"role": "user", "content": "Use get_code for Paris."}],
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "get_code",
                        "description": "Get a fixed city code",
                        "parameters": {
                            "type": "object",
                            "properties": {"city": {"type": "string"}},
                            "required": ["city"],
                            "additionalProperties": false
                        }
                    }
                }],
                "tool_choice": {"type": "function", "function": {"name": "get_code"}},
                "max_tokens": 128
            }))
            .send()
            .await
            .expect("send tool request"),
        &format!("{provider} tool call"),
    )
    .await;
    let call = &first["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(call["function"]["name"], "get_code");
    let call_id = call["id"].as_str().expect("tool call id");

    let second = json_response(
        client
            .post(format!("{base}/llm/v1/chat/completions"))
            .json(&json!({
                "model": model,
                "messages": [
                    {"role": "user", "content": "Use get_code for Paris."},
                    {"role": "assistant", "content": null, "tool_calls": [call]},
                    {"role": "tool", "tool_call_id": call_id, "content": "PAR-75"}
                ],
                "max_tokens": 64
            }))
            .send()
            .await
            .expect("send tool continuation"),
        &format!("{provider} tool continuation"),
    )
    .await;
    assert!(
        second.to_string().contains("PAR-75"),
        "{provider} did not use the tool result: {second}"
    );
}

async fn exercise_chat_stream(base: &str, model: &str, provider: &str) {
    let response = reqwest::Client::new()
        .post(format!("{base}/llm/v1/chat/completions"))
        .json(&json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with exactly ALIEN_OK"}],
            "stream": true,
            "max_tokens": 32
        }))
        .send()
        .await
        .expect("send streaming request");
    let status = response.status();
    let body = response.text().await.expect("read stream");
    eprintln!("{provider} streaming: status={status} body={body}");
    assert!(status.is_success(), "{provider} stream failed: {body}");
    assert!(
        body.contains("data:"),
        "{provider} returned no SSE data: {body}"
    );
    let content = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter_map(|event| {
            event["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_string)
        })
        .collect::<String>();
    assert!(
        content.to_ascii_uppercase().contains("ALIEN_OK"),
        "{provider} stream did not produce ALIEN_OK; content={content:?}, stream={body}"
    );
}

#[tokio::test]
#[ignore = "hits real OpenAI using OPENAI_TEST_API_KEY from .env.test"]
async fn live_openai_across_public_apis_and_scenarios() {
    load_test_env();
    let key = std::env::var("OPENAI_TEST_API_KEY").expect("OPENAI_TEST_API_KEY must be set");
    let base = serve(route_from_direct_openai("llm", key).expect("OpenAI route")).await;

    exercise_public_text_apis(&base, OPENAI_MODEL, "OpenAI").await;
    exercise_chat_tool_and_continuation(&base, OPENAI_MODEL, "OpenAI").await;
    exercise_chat_stream(&base, OPENAI_MODEL, "OpenAI").await;
}

#[tokio::test]
#[ignore = "hits real Anthropic using ANTHROPIC_TEST_API_KEY from .env.test"]
async fn live_anthropic_across_public_apis_and_scenarios() {
    load_test_env();
    let key = std::env::var("ANTHROPIC_TEST_API_KEY").expect("ANTHROPIC_TEST_API_KEY must be set");
    let base = serve(route_from_direct_anthropic("llm", key).expect("Anthropic route")).await;

    exercise_public_text_apis(&base, ANTHROPIC_MODEL, "Anthropic").await;
    exercise_chat_tool_and_continuation(&base, ANTHROPIC_MODEL, "Anthropic").await;
    exercise_chat_stream(&base, ANTHROPIC_MODEL, "Anthropic").await;
}
