//! Provider-neutral AI usage events.
//!
//! The gateway reports only request metadata and provider-supplied token counts.
//! It never includes prompts, responses, headers, credentials, or provider error bodies.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Receives completed request observations. Implementations must return quickly;
/// inference must never wait for telemetry delivery. A typical implementation uses
/// a bounded channel and drops the event when that channel is full.
pub trait AiUsageObserver: Send + Sync + 'static {
    fn observe(&self, event: AiUsageEvent);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiUsageProvider {
    AwsBedrock,
    GcpVertex,
    AzureFoundry,
    Anthropic,
    OpenAi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiUsageClientApi {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiUsageOutcome {
    Success,
    ProviderError,
    GatewayError,
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct AiUsageEvent {
    pub request_id: String,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub binding: String,
    pub provider: AiUsageProvider,
    pub public_model: String,
    pub provider_model: String,
    pub client_api: AiUsageClientApi,
    pub provider_region: Option<String>,
    pub status: u16,
    pub outcome: AiUsageOutcome,
    pub tokens: AiTokenUsage,
}

/// Extract token counts from a complete JSON response or from the JSON payloads
/// carried by an SSE response. Unknown response fields are ignored. Missing usage
/// remains `None`; it is never converted to zero.
pub fn parse_ai_token_usage(body: &[u8], client_api: AiUsageClientApi) -> AiTokenUsage {
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        return usage_from_value(&value, client_api);
    }

    let mut usage = AiTokenUsage::default();
    for line in body.split(|byte| *byte == b'\n') {
        let line = trim_ascii(line);
        let Some(data) = line.strip_prefix(b"data:") else {
            continue;
        };
        let data = trim_ascii(data);
        if data == b"[DONE]" {
            continue;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(data) {
            merge_usage(&mut usage, usage_from_value(&value, client_api));
        }
    }
    usage
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

fn usage_from_value(value: &Value, client_api: AiUsageClientApi) -> AiTokenUsage {
    match client_api {
        AiUsageClientApi::OpenAiChatCompletions => openai_usage(value.get("usage")),
        AiUsageClientApi::OpenAiResponses => {
            let response = value.get("response").unwrap_or(value);
            openai_responses_usage(response.get("usage"))
        }
        AiUsageClientApi::AnthropicMessages => anthropic_usage(value),
    }
}

fn openai_usage(value: Option<&Value>) -> AiTokenUsage {
    let Some(value) = value else {
        return AiTokenUsage::default();
    };
    AiTokenUsage {
        input_tokens: uint(value, "prompt_tokens"),
        output_tokens: uint(value, "completion_tokens"),
        cache_read_tokens: value
            .get("prompt_tokens_details")
            .and_then(|details| uint(details, "cached_tokens")),
        cache_write_tokens: None,
        reasoning_tokens: value
            .get("completion_tokens_details")
            .and_then(|details| uint(details, "reasoning_tokens")),
    }
}

fn openai_responses_usage(value: Option<&Value>) -> AiTokenUsage {
    let Some(value) = value else {
        return AiTokenUsage::default();
    };
    AiTokenUsage {
        input_tokens: uint(value, "input_tokens"),
        output_tokens: uint(value, "output_tokens"),
        cache_read_tokens: value
            .get("input_tokens_details")
            .and_then(|details| uint(details, "cached_tokens")),
        cache_write_tokens: None,
        reasoning_tokens: value
            .get("output_tokens_details")
            .and_then(|details| uint(details, "reasoning_tokens")),
    }
}

fn anthropic_usage(value: &Value) -> AiTokenUsage {
    let usage = value.get("usage").or_else(|| {
        value
            .get("message")
            .and_then(|message| message.get("usage"))
    });
    let Some(usage) = usage else {
        return AiTokenUsage::default();
    };
    AiTokenUsage {
        input_tokens: uint(usage, "input_tokens"),
        output_tokens: uint(usage, "output_tokens"),
        cache_read_tokens: uint(usage, "cache_read_input_tokens"),
        cache_write_tokens: uint(usage, "cache_creation_input_tokens"),
        reasoning_tokens: None,
    }
}

fn uint(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn merge_usage(current: &mut AiTokenUsage, next: AiTokenUsage) {
    if next.input_tokens.is_some() {
        current.input_tokens = next.input_tokens;
    }
    if next.output_tokens.is_some() {
        current.output_tokens = next.output_tokens;
    }
    if next.cache_read_tokens.is_some() {
        current.cache_read_tokens = next.cache_read_tokens;
    }
    if next.cache_write_tokens.is_some() {
        current.cache_write_tokens = next.cache_write_tokens;
    }
    if next.reasoning_tokens.is_some() {
        current.reasoning_tokens = next.reasoning_tokens;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_non_streaming_usage() {
        let usage = parse_ai_token_usage(
            br#"{"usage":{"prompt_tokens":12,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":4},"completion_tokens_details":{"reasoning_tokens":2}}}"#,
            AiUsageClientApi::OpenAiChatCompletions,
        );
        assert_eq!(
            usage,
            AiTokenUsage {
                input_tokens: Some(12),
                output_tokens: Some(7),
                cache_read_tokens: Some(4),
                cache_write_tokens: None,
                reasoning_tokens: Some(2),
            }
        );
    }

    #[test]
    fn extracts_openai_responses_stream_usage() {
        let body = br#"event: response.completed
data: {"type":"response.completed","response":{"usage":{"input_tokens":20,"output_tokens":8,"input_tokens_details":{"cached_tokens":5},"output_tokens_details":{"reasoning_tokens":3}}}}

data: [DONE]
"#;
        let usage = parse_ai_token_usage(body, AiUsageClientApi::OpenAiResponses);
        assert_eq!(usage.input_tokens, Some(20));
        assert_eq!(usage.output_tokens, Some(8));
        assert_eq!(usage.cache_read_tokens, Some(5));
        assert_eq!(usage.reasoning_tokens, Some(3));
    }

    #[test]
    fn merges_anthropic_stream_usage_without_inventing_missing_counts() {
        let body = br#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":30,"cache_creation_input_tokens":6,"cache_read_input_tokens":9}}}

event: message_delta
data: {"type":"message_delta","usage":{"output_tokens":11}}
"#;
        let usage = parse_ai_token_usage(body, AiUsageClientApi::AnthropicMessages);
        assert_eq!(usage.input_tokens, Some(30));
        assert_eq!(usage.output_tokens, Some(11));
        assert_eq!(usage.cache_read_tokens, Some(9));
        assert_eq!(usage.cache_write_tokens, Some(6));
        assert_eq!(usage.reasoning_tokens, None);
    }

    #[test]
    fn malformed_or_missing_usage_is_unknown_not_zero() {
        let malformed = parse_ai_token_usage(b"not json", AiUsageClientApi::AnthropicMessages);
        let missing =
            parse_ai_token_usage(br#"{"id":"message"}"#, AiUsageClientApi::AnthropicMessages);
        assert_eq!(malformed, AiTokenUsage::default());
        assert_eq!(missing, AiTokenUsage::default());
    }
}
