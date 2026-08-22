//! Loss-aware translation between the three public inference protocols.
//!
//! Translation is deliberately JSON-to-JSON. Provider schemas evolve faster than this
//! runtime, and preserving unknown fields on a native fast path is important. Cross-
//! protocol requests, however, accept only fields whose meaning can be represented by
//! the destination protocol. Unsupported stateful or provider-specific features fail
//! before credentials are resolved or an upstream request is made.

use alien_core::ai_catalog::{ClientApi, ProviderApi};
use alien_error::AlienError;
use serde_json::{json, Map, Value};

use crate::error::{ErrorData, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WireProtocol {
    ChatCompletions,
    Responses,
    Messages,
}

impl From<ClientApi> for WireProtocol {
    fn from(value: ClientApi) -> Self {
        match value {
            ClientApi::OpenAiChatCompletions => Self::ChatCompletions,
            ClientApi::OpenAiResponses => Self::Responses,
            ClientApi::AnthropicMessages => Self::Messages,
        }
    }
}

impl From<ProviderApi> for WireProtocol {
    fn from(value: ProviderApi) -> Self {
        match value {
            ProviderApi::OpenAi => Self::ChatCompletions,
            ProviderApi::OpenAiResponses => Self::Responses,
            ProviderApi::Anthropic => Self::Messages,
        }
    }
}

impl WireProtocol {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "/v1/chat/completions",
            Self::Responses => "/v1/responses",
            Self::Messages => "/v1/messages",
        }
    }
}

fn invalid(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InvalidRequest {
        message: message.into(),
    })
}

fn object(value: Value, context: &str) -> Result<Map<String, Value>> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| invalid(format!("{context} must be a JSON object")))
}

fn reject_fields(
    obj: &Map<String, Value>,
    fields: &[&str],
    destination: WireProtocol,
) -> Result<()> {
    if let Some(field) = fields.iter().find(|field| obj.contains_key(**field)) {
        return Err(invalid(format!(
            "`{field}` cannot be represented by {}; use a model with a native {} endpoint or remove that field",
            destination.path(),
            destination.path()
        )));
    }
    Ok(())
}

fn reject_non_default(
    obj: &Map<String, Value>,
    field: &str,
    accepted: impl FnOnce(&Value) -> bool,
    destination: WireProtocol,
) -> Result<()> {
    if obj.get(field).is_some_and(|value| !accepted(value)) {
        return Err(invalid(format!(
            "`{field}` cannot be represented by {}; use a model with a native {} endpoint or remove that field",
            destination.path(),
            destination.path()
        )));
    }
    Ok(())
}

pub(crate) fn translate_request(
    payload: Value,
    source: WireProtocol,
    destination: WireProtocol,
) -> Result<Value> {
    if source == destination {
        return Ok(payload);
    }
    let chat = match source {
        WireProtocol::ChatCompletions => payload,
        WireProtocol::Responses => responses_request_to_chat(payload)?,
        WireProtocol::Messages => messages_request_to_chat(payload)?,
    };
    match destination {
        WireProtocol::ChatCompletions => Ok(chat),
        WireProtocol::Responses => chat_request_to_responses(chat),
        WireProtocol::Messages => chat_request_to_messages(chat),
    }
}

pub(crate) fn translate_response(
    payload: Value,
    source: WireProtocol,
    destination: WireProtocol,
    requested_model: &str,
) -> Result<Value> {
    if source == destination {
        return Ok(payload);
    }
    let chat = match source {
        WireProtocol::ChatCompletions => payload,
        WireProtocol::Responses => responses_response_to_chat(payload, requested_model)?,
        WireProtocol::Messages => messages_response_to_chat(payload, requested_model)?,
    };
    match destination {
        WireProtocol::ChatCompletions => Ok(chat),
        WireProtocol::Responses => chat_response_to_responses(chat, requested_model),
        WireProtocol::Messages => chat_response_to_messages(chat, requested_model),
    }
}

/// Stateful SSE translator. It accepts arbitrarily split transport chunks and
/// emits only complete downstream events; no JSON event is parsed until its SSE
/// frame terminator arrives.
pub(crate) struct SseTranslator {
    source: WireProtocol,
    destination: WireProtocol,
    requested_model: String,
    buffer: Vec<u8>,
    started: bool,
    text_block_started: bool,
}

impl SseTranslator {
    pub(crate) fn new(
        source: WireProtocol,
        destination: WireProtocol,
        requested_model: &str,
    ) -> Self {
        Self {
            source,
            destination,
            requested_model: requested_model.to_string(),
            buffer: Vec::new(),
            started: false,
            text_block_started: false,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        self.buffer.extend_from_slice(bytes);
        let mut output = Vec::new();
        while let Some((end, delimiter_len)) = next_sse_frame(&self.buffer) {
            let frame: Vec<u8> = self.buffer.drain(..end + delimiter_len).collect();
            output.extend(self.translate_frame(&frame)?);
        }
        Ok(output)
    }

    pub(crate) fn finish(self) -> Result<Vec<u8>> {
        if self.buffer.iter().all(u8::is_ascii_whitespace) {
            return Ok(Vec::new());
        }
        Err(invalid(
            "provider stream ended in the middle of an SSE event",
        ))
    }

    fn translate_frame(&mut self, frame: &[u8]) -> Result<Vec<u8>> {
        let text = std::str::from_utf8(frame)
            .map_err(|_| invalid("provider stream contains invalid UTF-8"))?;
        let data = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            return Ok(Vec::new());
        }
        if data == "[DONE]" {
            return Ok(self.finish_events());
        }
        let event: Value = serde_json::from_str(&data)
            .map_err(|_| invalid("provider stream contains an invalid JSON event"))?;
        Ok(self.translate_event(&event))
    }

    fn translate_event(&mut self, event: &Value) -> Vec<u8> {
        if self.source == self.destination {
            return sse_data(event);
        }
        match (self.source, self.destination) {
            (WireProtocol::Messages, WireProtocol::ChatCompletions) => {
                self.messages_to_chat_event(event)
            }
            (WireProtocol::Messages, WireProtocol::Responses) => {
                self.messages_to_responses_event(event)
            }
            (WireProtocol::ChatCompletions, WireProtocol::Messages) => {
                self.chat_to_messages_event(event)
            }
            (WireProtocol::ChatCompletions, WireProtocol::Responses) => {
                self.chat_to_responses_event(event)
            }
            (WireProtocol::Responses, WireProtocol::ChatCompletions) => {
                self.responses_to_chat_event(event)
            }
            (WireProtocol::Responses, WireProtocol::Messages) => {
                self.responses_to_messages_event(event)
            }
            _ => Vec::new(),
        }
    }

    fn messages_to_chat_event(&mut self, event: &Value) -> Vec<u8> {
        let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
        let id = event
            .pointer("/message/id")
            .or_else(|| event.get("id"))
            .cloned()
            .unwrap_or_else(|| json!("chatcmpl_translated"));
        match kind {
            "message_start" => sse_data(
                &json!({ "id": id, "object": "chat.completion.chunk", "model": self.requested_model, "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "" }, "finish_reason": null }] }),
            ),
            "content_block_start"
                if event.pointer("/content_block/type").and_then(Value::as_str)
                    == Some("tool_use") =>
            {
                sse_data(
                    &json!({ "id": "chatcmpl_translated", "object": "chat.completion.chunk", "model": self.requested_model, "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": event.get("index").cloned().unwrap_or_else(|| json!(0)), "id": event.pointer("/content_block/id").cloned().unwrap_or_else(|| json!("call")), "type": "function", "function": { "name": event.pointer("/content_block/name").cloned().unwrap_or_else(|| json!("")), "arguments": "" } }] }, "finish_reason": null }] }),
                )
            }
            "content_block_delta"
                if event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") =>
            {
                sse_data(
                    &json!({ "id": "chatcmpl_translated", "object": "chat.completion.chunk", "model": self.requested_model, "choices": [{ "index": 0, "delta": { "content": event.pointer("/delta/text").cloned().unwrap_or_else(|| json!("")) }, "finish_reason": null }] }),
                )
            }
            "content_block_delta"
                if event.pointer("/delta/type").and_then(Value::as_str)
                    == Some("input_json_delta") =>
            {
                sse_data(
                    &json!({ "id": "chatcmpl_translated", "object": "chat.completion.chunk", "model": self.requested_model, "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": event.get("index").cloned().unwrap_or_else(|| json!(0)), "function": { "arguments": event.pointer("/delta/partial_json").cloned().unwrap_or_else(|| json!("")) } }] }, "finish_reason": null }] }),
                )
            }
            "message_delta" => {
                let finish = match event.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    Some("max_tokens") => "length",
                    Some("tool_use") => "tool_calls",
                    _ => "stop",
                };
                sse_data(
                    &json!({ "id": "chatcmpl_translated", "object": "chat.completion.chunk", "model": self.requested_model, "choices": [{ "index": 0, "delta": {}, "finish_reason": finish }], "usage": { "completion_tokens": event.pointer("/usage/output_tokens").cloned().unwrap_or_else(|| json!(0)) } }),
                )
            }
            "message_stop" => b"data: [DONE]\n\n".to_vec(),
            _ => Vec::new(),
        }
    }

    fn messages_to_responses_event(&mut self, event: &Value) -> Vec<u8> {
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => sse_data(
                &json!({ "type": "response.created", "response": { "id": event.pointer("/message/id").cloned().unwrap_or_else(|| json!("resp_translated")), "object": "response", "status": "in_progress", "model": self.requested_model, "output": [] } }),
            ),
            "content_block_start"
                if event.pointer("/content_block/type").and_then(Value::as_str) == Some("text") =>
            {
                sse_data(
                    &json!({ "type": "response.output_item.added", "output_index": 0, "item": { "type": "message", "id": "msg_translated", "status": "in_progress", "role": "assistant", "content": [] } }),
                )
            }
            "content_block_delta"
                if event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") =>
            {
                sse_data(
                    &json!({ "type": "response.output_text.delta", "item_id": "msg_translated", "output_index": 0, "content_index": 0, "delta": event.pointer("/delta/text").cloned().unwrap_or_else(|| json!("")) }),
                )
            }
            "message_stop" => sse_data(
                &json!({ "type": "response.completed", "response": { "id": "resp_translated", "object": "response", "status": "completed", "model": self.requested_model, "output": [] } }),
            ),
            _ => Vec::new(),
        }
    }

    fn chat_to_messages_event(&mut self, event: &Value) -> Vec<u8> {
        let mut output = Vec::new();
        if !self.started {
            self.started = true;
            output.extend(sse_named("message_start", &json!({ "type": "message_start", "message": { "id": event.get("id").cloned().unwrap_or_else(|| json!("msg_translated")), "type": "message", "role": "assistant", "model": self.requested_model, "content": [], "stop_reason": null, "stop_sequence": null, "usage": { "input_tokens": 0, "output_tokens": 0 } } })));
        }
        let delta = event.pointer("/choices/0/delta").unwrap_or(&Value::Null);
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            if !self.text_block_started {
                self.text_block_started = true;
                output.extend(sse_named("content_block_start", &json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } })));
            }
            output.extend(sse_named("content_block_delta", &json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } })));
        }
        if let Some(reason) = event
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
        {
            if self.text_block_started {
                output.extend(sse_named(
                    "content_block_stop",
                    &json!({ "type": "content_block_stop", "index": 0 }),
                ));
            }
            let reason = if reason == "length" {
                "max_tokens"
            } else if reason == "tool_calls" {
                "tool_use"
            } else {
                "end_turn"
            };
            output.extend(sse_named("message_delta", &json!({ "type": "message_delta", "delta": { "stop_reason": reason, "stop_sequence": null }, "usage": { "output_tokens": event.pointer("/usage/completion_tokens").cloned().unwrap_or_else(|| json!(0)) } })));
            output.extend(sse_named(
                "message_stop",
                &json!({ "type": "message_stop" }),
            ));
        }
        output
    }

    fn chat_to_responses_event(&mut self, event: &Value) -> Vec<u8> {
        let mut output = Vec::new();
        if !self.started {
            self.started = true;
            output.extend(sse_data(&json!({ "type": "response.created", "response": { "id": event.get("id").cloned().unwrap_or_else(|| json!("resp_translated")), "object": "response", "status": "in_progress", "model": self.requested_model, "output": [] } })));
        }
        if let Some(text) = event
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
        {
            output.extend(sse_data(&json!({ "type": "response.output_text.delta", "item_id": "msg_translated", "output_index": 0, "content_index": 0, "delta": text })));
        }
        if event
            .pointer("/choices/0/finish_reason")
            .is_some_and(|value| !value.is_null())
        {
            output.extend(sse_data(&json!({ "type": "response.completed", "response": { "id": event.get("id").cloned().unwrap_or_else(|| json!("resp_translated")), "object": "response", "status": "completed", "model": self.requested_model, "output": [] } })));
        }
        output
    }

    fn responses_to_chat_event(&mut self, event: &Value) -> Vec<u8> {
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "response.created" => sse_data(
                &json!({ "id": event.pointer("/response/id").cloned().unwrap_or_else(|| json!("chatcmpl_translated")), "object": "chat.completion.chunk", "model": self.requested_model, "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "" }, "finish_reason": null }] }),
            ),
            "response.output_text.delta" => sse_data(
                &json!({ "id": "chatcmpl_translated", "object": "chat.completion.chunk", "model": self.requested_model, "choices": [{ "index": 0, "delta": { "content": event.get("delta").cloned().unwrap_or_else(|| json!("")) }, "finish_reason": null }] }),
            ),
            "response.completed" => {
                let mut out = sse_data(
                    &json!({ "id": "chatcmpl_translated", "object": "chat.completion.chunk", "model": self.requested_model, "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] }),
                );
                out.extend_from_slice(b"data: [DONE]\n\n");
                out
            }
            _ => Vec::new(),
        }
    }

    fn responses_to_messages_event(&mut self, event: &Value) -> Vec<u8> {
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "response.created" => sse_named(
                "message_start",
                &json!({ "type": "message_start", "message": { "id": event.pointer("/response/id").cloned().unwrap_or_else(|| json!("msg_translated")), "type": "message", "role": "assistant", "model": self.requested_model, "content": [], "stop_reason": null, "usage": { "input_tokens": 0, "output_tokens": 0 } } }),
            ),
            "response.output_text.delta" => {
                let mut out = Vec::new();
                if !self.text_block_started {
                    self.text_block_started = true;
                    out.extend(sse_named("content_block_start", &json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } })));
                }
                out.extend(sse_named("content_block_delta", &json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": event.get("delta").cloned().unwrap_or_else(|| json!("")) } })));
                out
            }
            "response.completed" => {
                let mut out = Vec::new();
                if self.text_block_started {
                    out.extend(sse_named(
                        "content_block_stop",
                        &json!({ "type": "content_block_stop", "index": 0 }),
                    ));
                }
                out.extend(sse_named("message_delta", &json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": { "output_tokens": event.pointer("/response/usage/output_tokens").cloned().unwrap_or_else(|| json!(0)) } })));
                out.extend(sse_named(
                    "message_stop",
                    &json!({ "type": "message_stop" }),
                ));
                out
            }
            _ => Vec::new(),
        }
    }

    fn finish_events(&mut self) -> Vec<u8> {
        match self.destination {
            WireProtocol::ChatCompletions => b"data: [DONE]\n\n".to_vec(),
            _ => Vec::new(),
        }
    }
}

fn next_sse_frame(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (Some(_), Some(right)) => Some((right, 4)),
        (Some(end), None) => Some((end, 2)),
        (None, Some(end)) => Some((end, 4)),
        (None, None) => None,
    }
}

fn sse_data(value: &Value) -> Vec<u8> {
    format!("data: {}\n\n", value).into_bytes()
}

fn sse_named(name: &str, value: &Value) -> Vec<u8> {
    format!("event: {name}\ndata: {}\n\n", value).into_bytes()
}

fn responses_request_to_chat(payload: Value) -> Result<Value> {
    let mut obj = object(payload, "Responses request")?;
    reject_fields(
        &obj,
        &[
            "context_management",
            "max_tool_calls",
            "moderation",
            "prompt_cache_key",
            "prompt_cache_options",
            "safety_identifier",
        ],
        WireProtocol::ChatCompletions,
    )?;
    for field in ["previous_response_id", "conversation", "prompt"] {
        reject_non_default(&obj, field, Value::is_null, WireProtocol::ChatCompletions)?;
        obj.remove(field);
    }
    for field in ["include", "metadata"] {
        reject_non_default(
            &obj,
            field,
            |value| match value {
                Value::Null => true,
                Value::Array(items) => items.is_empty(),
                Value::Object(entries) => entries.is_empty(),
                _ => false,
            },
            WireProtocol::ChatCompletions,
        )?;
        obj.remove(field);
    }
    reject_non_default(
        &obj,
        "background",
        |value| value.is_null() || value == false,
        WireProtocol::ChatCompletions,
    )?;
    obj.remove("background");
    reject_non_default(
        &obj,
        "store",
        |value| value.is_null() || value == false,
        WireProtocol::ChatCompletions,
    )?;
    reject_non_default(
        &obj,
        "truncation",
        |value| value.is_null() || value == "disabled",
        WireProtocol::ChatCompletions,
    )?;
    obj.remove("store");
    obj.remove("truncation");
    let input = obj
        .remove("input")
        .ok_or_else(|| invalid("Responses request has no `input` field"))?;
    let mut messages = Vec::new();
    if let Some(instructions) = obj.remove("instructions") {
        messages.push(json!({ "role": "system", "content": instructions }));
    }
    match input {
        Value::String(text) => messages.push(json!({ "role": "user", "content": text })),
        Value::Array(items) => {
            for item in items {
                messages.push(responses_input_item_to_chat(item)?);
            }
        }
        _ => return Err(invalid("Responses `input` must be a string or an array")),
    }
    obj.insert("messages".to_string(), Value::Array(messages));
    rename(&mut obj, "max_output_tokens", "max_completion_tokens");
    if let Some(reasoning) = obj.remove("reasoning") {
        reject_fields(
            reasoning
                .as_object()
                .ok_or_else(|| invalid("Responses `reasoning` must be an object"))?,
            &["summary", "generate_summary"],
            WireProtocol::ChatCompletions,
        )?;
        let effort = reasoning.get("effort").cloned();
        if let Some(effort) = effort {
            obj.insert("reasoning_effort".to_string(), effort);
        }
    }
    if let Some(text) = obj.remove("text") {
        reject_fields(
            text.as_object()
                .ok_or_else(|| invalid("Responses `text` must be an object"))?,
            &["verbosity"],
            WireProtocol::ChatCompletions,
        )?;
        if let Some(format) = text.get("format") {
            obj.insert("response_format".to_string(), format.clone());
        }
    }
    if let Some(tools) = obj.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            let tool_obj = tool
                .as_object_mut()
                .ok_or_else(|| invalid("Responses tools must be objects"))?;
            if tool_obj.get("type").and_then(Value::as_str) != Some("function") {
                return Err(invalid(
                    "built-in Responses tools cannot be represented by Chat Completions",
                ));
            }
            let name = tool_obj
                .remove("name")
                .ok_or_else(|| invalid("tool has no `name`"))?;
            let description = tool_obj.remove("description");
            let parameters = tool_obj.remove("parameters").unwrap_or_else(|| json!({}));
            let strict = tool_obj.remove("strict");
            let mut function = json!({ "name": name, "parameters": parameters });
            if let Some(description) = description {
                function["description"] = description;
            }
            if let Some(strict) = strict {
                function["strict"] = strict;
            }
            *tool = json!({ "type": "function", "function": function });
        }
    }
    Ok(Value::Object(obj))
}

fn responses_input_item_to_chat(item: Value) -> Result<Value> {
    let obj = object(item, "Responses input item")?;
    match obj.get("type").and_then(Value::as_str).unwrap_or("message") {
        "message" => {
            let role = obj.get("role").cloned().unwrap_or_else(|| json!("user"));
            let content = obj
                .get("content")
                .cloned()
                .unwrap_or(Value::String(String::new()));
            Ok(json!({ "role": role, "content": responses_content_to_chat(content)? }))
        }
        "function_call" => Ok(json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": obj.get("call_id").or_else(|| obj.get("id")).cloned().unwrap_or_else(|| json!("call")),
                "type": "function",
                "function": {
                    "name": obj.get("name").cloned().unwrap_or_else(|| json!("")),
                    "arguments": obj.get("arguments").cloned().unwrap_or_else(|| json!("{}"))
                }
            }]
        })),
        "function_call_output" => Ok(json!({
            "role": "tool",
            "tool_call_id": obj.get("call_id").cloned().unwrap_or_else(|| json!("call")),
            "content": obj.get("output").cloned().unwrap_or_else(|| json!(""))
        })),
        kind => Err(invalid(format!(
            "Responses input item type `{kind}` cannot be represented by Chat Completions"
        ))),
    }
}

fn responses_content_to_chat(content: Value) -> Result<Value> {
    let Value::Array(parts) = content else {
        return Ok(content);
    };
    let mut converted = Vec::new();
    for part in parts {
        let obj = object(part, "Responses content part")?;
        match obj.get("type").and_then(Value::as_str) {
            Some("input_text") | Some("output_text") => converted.push(json!({
                "type": "text",
                "text": obj.get("text").cloned().unwrap_or_else(|| json!(""))
            })),
            Some("input_image") => converted.push(json!({
                "type": "image_url",
                "image_url": { "url": obj.get("image_url").cloned().unwrap_or_else(|| json!("")) }
            })),
            Some(kind) => {
                return Err(invalid(format!(
                    "Responses content type `{kind}` is not portable"
                )))
            }
            None => return Err(invalid("Responses content part has no `type`")),
        }
    }
    Ok(Value::Array(converted))
}

fn messages_request_to_chat(payload: Value) -> Result<Value> {
    let mut obj = object(payload, "Messages request")?;
    reject_fields(
        &obj,
        &[
            "cache_control",
            "thinking",
            "container",
            "mcp_servers",
            "context_management",
            "inference_geo",
            "output_config",
            "service_tier",
            "top_k",
        ],
        WireProtocol::ChatCompletions,
    )?;
    let messages = obj
        .remove("messages")
        .and_then(|value| value.as_array().cloned())
        .ok_or_else(|| invalid("Messages request has no `messages` array"))?;
    let mut output = Vec::new();
    if let Some(system) = obj.remove("system") {
        output.push(json!({ "role": "system", "content": anthropic_content_to_chat(system)? }));
    }
    for message in messages {
        let message = object(message, "Messages message")?;
        let role = message
            .get("role")
            .cloned()
            .ok_or_else(|| invalid("Messages message has no `role`"))?;
        let content = message.get("content").cloned().unwrap_or_else(|| json!(""));
        let (content, tool_calls, tool_results) = anthropic_message_content_to_chat(content)?;
        if !tool_results.is_empty() {
            output.extend(tool_results);
            if !content_is_empty(&content) {
                output.push(json!({ "role": role, "content": content }));
            }
        } else {
            let mut converted = json!({ "role": role, "content": content });
            if !tool_calls.is_empty() {
                converted["tool_calls"] = Value::Array(tool_calls);
            }
            output.push(converted);
        }
    }
    obj.insert("messages".to_string(), Value::Array(output));
    rename(&mut obj, "max_tokens", "max_completion_tokens");
    rename(&mut obj, "stop_sequences", "stop");
    if let Some(metadata) = obj.remove("metadata") {
        let metadata = metadata
            .as_object()
            .ok_or_else(|| invalid("Messages `metadata` must be an object"))?;
        reject_fields(metadata, &["other"], WireProtocol::ChatCompletions)?;
        if metadata.keys().any(|key| key != "user_id") {
            return Err(invalid(
                "Messages metadata other than `user_id` is not portable",
            ));
        }
        if let Some(user_id) = metadata.get("user_id") {
            obj.insert("user".to_string(), user_id.clone());
        }
    }
    if let Some(tools) = obj.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            let tool_obj = object(tool.take(), "Messages tool")?;
            *tool = json!({
                "type": "function",
                "function": {
                    "name": tool_obj.get("name").cloned().unwrap_or_else(|| json!("")),
                    "description": tool_obj.get("description").cloned().unwrap_or(Value::Null),
                    "parameters": tool_obj.get("input_schema").cloned().unwrap_or_else(|| json!({}))
                }
            });
        }
    }
    if let Some(choice) = obj.remove("tool_choice") {
        obj.insert(
            "tool_choice".to_string(),
            anthropic_tool_choice_to_chat(choice)?,
        );
    }
    Ok(Value::Object(obj))
}

fn anthropic_message_content_to_chat(content: Value) -> Result<(Value, Vec<Value>, Vec<Value>)> {
    let Value::Array(parts) = content else {
        return Ok((content, Vec::new(), Vec::new()));
    };
    let mut content_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    for part in parts {
        let obj = object(part, "Messages content block")?;
        match obj.get("type").and_then(Value::as_str) {
            Some("text") => content_parts.push(json!({
                "type": "text",
                "text": obj.get("text").cloned().unwrap_or_else(|| json!(""))
            })),
            Some("image") => {
                let source = obj.get("source").and_then(Value::as_object).ok_or_else(|| invalid("image has no source"))?;
                let media_type = source.get("media_type").and_then(Value::as_str).unwrap_or("image/png");
                let data = source.get("data").and_then(Value::as_str).ok_or_else(|| invalid("image source has no data"))?;
                content_parts.push(json!({ "type": "image_url", "image_url": { "url": format!("data:{media_type};base64,{data}") } }));
            }
            Some("tool_use") => tool_calls.push(json!({
                "id": obj.get("id").cloned().unwrap_or_else(|| json!("call")),
                "type": "function",
                "function": {
                    "name": obj.get("name").cloned().unwrap_or_else(|| json!("")),
                    "arguments": serde_json::to_string(obj.get("input").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".to_string())
                }
            })),
            Some("tool_result") => tool_results.push(json!({
                "role": "tool",
                "tool_call_id": obj.get("tool_use_id").cloned().unwrap_or_else(|| json!("call")),
                "content": anthropic_content_to_chat(obj.get("content").cloned().unwrap_or_else(|| json!("")))?
            })),
            Some(kind) => return Err(invalid(format!("Messages content type `{kind}` is not portable"))),
            None => return Err(invalid("Messages content block has no `type`")),
        }
    }
    Ok((Value::Array(content_parts), tool_calls, tool_results))
}

fn anthropic_content_to_chat(content: Value) -> Result<Value> {
    let (content, tool_calls, tool_results) = anthropic_message_content_to_chat(content)?;
    if !tool_calls.is_empty() || !tool_results.is_empty() {
        return Err(invalid(
            "tool blocks are not valid in this Messages content field",
        ));
    }
    Ok(content)
}

fn anthropic_tool_choice_to_chat(choice: Value) -> Result<Value> {
    let obj = object(choice, "Messages tool_choice")?;
    match obj.get("type").and_then(Value::as_str) {
        Some("auto") => Ok(json!("auto")),
        Some("any") => Ok(json!("required")),
        Some("none") => Ok(json!("none")),
        Some("tool") => Ok(json!({
            "type": "function",
            "function": { "name": obj.get("name").cloned().unwrap_or_else(|| json!("")) }
        })),
        Some(kind) => Err(invalid(format!(
            "Messages tool choice `{kind}` is not portable"
        ))),
        None => Err(invalid("Messages tool_choice has no `type`")),
    }
}

fn chat_request_to_messages(payload: Value) -> Result<Value> {
    let mut obj = object(payload, "Chat Completions request")?;
    reject_fields(
        &obj,
        &[
            "audio",
            "function_call",
            "functions",
            "frequency_penalty",
            "logit_bias",
            "logprobs",
            "modalities",
            "n",
            "prediction",
            "presence_penalty",
            "reasoning_effort",
            "response_format",
            "seed",
            "service_tier",
            "stream_options",
            "top_logprobs",
            "web_search_options",
        ],
        WireProtocol::Messages,
    )?;
    let messages = obj
        .remove("messages")
        .and_then(|value| value.as_array().cloned())
        .ok_or_else(|| invalid("Chat Completions request has no `messages` array"))?;
    let mut system = Vec::new();
    let mut output = Vec::new();
    for message in messages {
        let message = object(message, "Chat Completions message")?;
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("message has no `role`"))?;
        if role == "system" || role == "developer" {
            system.extend(chat_content_to_anthropic(
                message.get("content").cloned().unwrap_or_else(|| json!("")),
            )?);
            continue;
        }
        if role == "tool" {
            output.push(json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": message.get("tool_call_id").cloned().unwrap_or_else(|| json!("call")),
                    "content": message.get("content").cloned().unwrap_or_else(|| json!(""))
                }]
            }));
            continue;
        }
        let mut content = chat_content_to_anthropic(
            message.get("content").cloned().unwrap_or_else(|| json!("")),
        )?;
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                let function = call
                    .get("function")
                    .and_then(Value::as_object)
                    .ok_or_else(|| invalid("tool call has no function"))?;
                let input = function
                    .get("arguments")
                    .and_then(Value::as_str)
                    .map(serde_json::from_str)
                    .transpose()
                    .map_err(|_| invalid("tool call arguments are not valid JSON"))?
                    .unwrap_or_else(|| json!({}));
                content.push(json!({
                    "type": "tool_use",
                    "id": call.get("id").cloned().unwrap_or_else(|| json!("call")),
                    "name": function.get("name").cloned().unwrap_or_else(|| json!("")),
                    "input": input
                }));
            }
        }
        output.push(json!({ "role": role, "content": content }));
    }
    obj.insert("messages".to_string(), Value::Array(output));
    if !system.is_empty() {
        obj.insert("system".to_string(), Value::Array(system));
    }
    rename(&mut obj, "max_completion_tokens", "max_tokens");
    rename(&mut obj, "max_tokens", "max_tokens");
    rename(&mut obj, "stop", "stop_sequences");
    if let Some(user) = obj.remove("user") {
        obj.insert("metadata".to_string(), json!({ "user_id": user }));
    }
    obj.remove("n");
    obj.remove("frequency_penalty");
    obj.remove("presence_penalty");
    obj.remove("logit_bias");
    obj.remove("logprobs");
    obj.remove("top_logprobs");
    obj.remove("seed");
    obj.remove("service_tier");
    if let Some(tools) = obj.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            let function = tool
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid("Chat Completions tool has no function"))?;
            *tool = json!({
                "name": function.get("name").cloned().unwrap_or_else(|| json!("")),
                "description": function.get("description").cloned().unwrap_or(Value::Null),
                "input_schema": function.get("parameters").cloned().unwrap_or_else(|| json!({}))
            });
        }
    }
    if let Some(choice) = obj.remove("tool_choice") {
        obj.insert(
            "tool_choice".to_string(),
            chat_tool_choice_to_anthropic(choice)?,
        );
    }
    Ok(Value::Object(obj))
}

fn chat_content_to_anthropic(content: Value) -> Result<Vec<Value>> {
    match content {
        Value::Null => Ok(Vec::new()),
        Value::String(text) => Ok(vec![json!({ "type": "text", "text": text })]),
        Value::Array(parts) => parts.into_iter().map(|part| {
            let obj = object(part, "Chat Completions content part")?;
            match obj.get("type").and_then(Value::as_str) {
                Some("text") => Ok(json!({ "type": "text", "text": obj.get("text").cloned().unwrap_or_else(|| json!("")) })),
                Some("image_url") => {
                    let url = obj.get("image_url").and_then(|v| v.get("url")).and_then(Value::as_str).ok_or_else(|| invalid("image_url has no URL"))?;
                    let (metadata, data) = url.split_once(",").ok_or_else(|| invalid("Messages translation supports only base64 data image URLs"))?;
                    let media_type = metadata.strip_prefix("data:").and_then(|v| v.strip_suffix(";base64")).ok_or_else(|| invalid("image URL is not base64 data"))?;
                    Ok(json!({ "type": "image", "source": { "type": "base64", "media_type": media_type, "data": data } }))
                }
                Some(kind) => Err(invalid(format!("Chat content type `{kind}` is not portable"))),
                None => Err(invalid("Chat content part has no `type`")),
            }
        }).collect(),
        _ => Err(invalid("Chat message content must be a string or array")),
    }
}

fn chat_tool_choice_to_anthropic(choice: Value) -> Result<Value> {
    if let Some(choice) = choice.as_str() {
        return match choice {
            "auto" => Ok(json!({ "type": "auto" })),
            "required" => Ok(json!({ "type": "any" })),
            "none" => Ok(json!({ "type": "none" })),
            other => Err(invalid(format!(
                "Chat tool choice `{other}` is not portable"
            ))),
        };
    }
    let name = choice
        .get("function")
        .and_then(|v| v.get("name"))
        .cloned()
        .ok_or_else(|| invalid("Chat tool choice has no function name"))?;
    Ok(json!({ "type": "tool", "name": name }))
}

fn chat_request_to_responses(payload: Value) -> Result<Value> {
    let mut obj = object(payload, "Chat Completions request")?;
    reject_fields(
        &obj,
        &[
            "audio",
            "frequency_penalty",
            "function_call",
            "functions",
            "logit_bias",
            "logprobs",
            "modalities",
            "n",
            "prediction",
            "presence_penalty",
            "seed",
            "stop",
            "top_logprobs",
            "user",
            "web_search_options",
        ],
        WireProtocol::Responses,
    )?;
    let messages = obj
        .remove("messages")
        .ok_or_else(|| invalid("Chat Completions request has no `messages` array"))?;
    obj.insert("input".to_string(), chat_messages_to_responses(messages)?);
    rename(&mut obj, "max_completion_tokens", "max_output_tokens");
    rename(&mut obj, "max_tokens", "max_output_tokens");
    if let Some(effort) = obj.remove("reasoning_effort") {
        obj.insert("reasoning".to_string(), json!({ "effort": effort }));
    }
    if let Some(format) = obj.remove("response_format") {
        obj.insert("text".to_string(), json!({ "format": format }));
    }
    obj.remove("n");
    obj.remove("frequency_penalty");
    obj.remove("presence_penalty");
    obj.remove("logit_bias");
    obj.remove("logprobs");
    obj.remove("top_logprobs");
    obj.remove("seed");
    if let Some(tools) = obj.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            let function = tool
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid("Chat Completions tool has no function"))?;
            *tool = json!({
                "type": "function",
                "name": function.get("name").cloned().unwrap_or_else(|| json!("")),
                "description": function.get("description").cloned().unwrap_or(Value::Null),
                "parameters": function.get("parameters").cloned().unwrap_or_else(|| json!({})),
                "strict": function.get("strict").cloned().unwrap_or_else(|| json!(false))
            });
        }
    }
    Ok(Value::Object(obj))
}

fn chat_messages_to_responses(messages: Value) -> Result<Value> {
    let messages = messages
        .as_array()
        .ok_or_else(|| invalid("Chat Completions `messages` must be an array"))?;
    let mut output = Vec::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("message has no role"))?;
        if role == "tool" {
            output.push(json!({
                "type": "function_call_output",
                "call_id": message.get("tool_call_id").cloned().unwrap_or_else(|| json!("call")),
                "output": message.get("content").cloned().unwrap_or_else(|| json!(""))
            }));
            continue;
        }
        output.push(json!({
            "type": "message",
            "role": role,
            "content": chat_content_to_responses(message.get("content").cloned().unwrap_or_else(|| json!("")), role)?
        }));
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                output.push(json!({
                    "type": "function_call",
                    "call_id": call.get("id").cloned().unwrap_or_else(|| json!("call")),
                    "name": call.get("function").and_then(|v| v.get("name")).cloned().unwrap_or_else(|| json!("")),
                    "arguments": call.get("function").and_then(|v| v.get("arguments")).cloned().unwrap_or_else(|| json!("{}"))
                }));
            }
        }
    }
    Ok(Value::Array(output))
}

fn chat_content_to_responses(content: Value, role: &str) -> Result<Value> {
    let prefix = if role == "assistant" {
        "output"
    } else {
        "input"
    };
    match content {
        Value::Null => Ok(Value::Array(Vec::new())),
        Value::String(text) => Ok(json!([{ "type": format!("{prefix}_text"), "text": text }])),
        Value::Array(parts) => Ok(Value::Array(parts.into_iter().map(|part| {
            let obj = object(part, "Chat content part")?;
            match obj.get("type").and_then(Value::as_str) {
                Some("text") => Ok(json!({ "type": format!("{prefix}_text"), "text": obj.get("text").cloned().unwrap_or_else(|| json!("")) })),
                Some("image_url") if prefix == "input" => Ok(json!({ "type": "input_image", "image_url": obj.get("image_url").and_then(|v| v.get("url")).cloned().unwrap_or_else(|| json!("")) })),
                Some(kind) => Err(invalid(format!("Chat content type `{kind}` is not portable to Responses"))),
                None => Err(invalid("Chat content part has no type")),
            }
        }).collect::<Result<Vec<_>>>()?)),
        _ => Err(invalid("Chat message content must be a string or array")),
    }
}

fn messages_response_to_chat(payload: Value, requested_model: &str) -> Result<Value> {
    let obj = object(payload, "Messages response")?;
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    for block in obj
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Messages response has no content array"))?
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => content.push_str(block.get("text").and_then(Value::as_str).unwrap_or("")),
            Some("tool_use") => tool_calls.push(json!({
                "id": block.get("id").cloned().unwrap_or_else(|| json!("call")),
                "type": "function",
                "function": {
                    "name": block.get("name").cloned().unwrap_or_else(|| json!("")),
                    "arguments": serde_json::to_string(block.get("input").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".to_string())
                }
            })),
            Some("thinking") | Some("redacted_thinking") => {}
            Some(kind) => return Err(invalid(format!("Messages response content `{kind}` is not portable"))),
            None => return Err(invalid("Messages response content has no type")),
        }
    }
    let mut message = json!({ "role": "assistant", "content": content });
    let has_tool_calls = !tool_calls.is_empty();
    if has_tool_calls {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let stop = match obj.get("stop_reason").and_then(Value::as_str) {
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        _ => "stop",
    };
    let input = Value::Object(obj.clone())
        .pointer("/usage/input_tokens")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let output = Value::Object(obj.clone())
        .pointer("/usage/output_tokens")
        .cloned()
        .unwrap_or_else(|| json!(0));
    Ok(json!({
        "id": obj.get("id").cloned().unwrap_or_else(|| json!("msg_translated")),
        "object": "chat.completion",
        "created": 0,
        "model": requested_model,
        "choices": [{ "index": 0, "message": message, "finish_reason": stop }],
        "usage": { "prompt_tokens": input, "completion_tokens": output, "total_tokens": numeric_sum(&input, &output) }
    }))
}

fn responses_response_to_chat(payload: Value, requested_model: &str) -> Result<Value> {
    let obj = object(payload, "Responses response")?;
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    for item in obj
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Responses response has no output array"))?
    {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => for part in item.get("content").and_then(Value::as_array).unwrap_or(&Vec::new()) {
                if matches!(part.get("type").and_then(Value::as_str), Some("output_text" | "text")) {
                    content.push_str(part.get("text").and_then(Value::as_str).unwrap_or(""));
                }
            },
            Some("function_call") => tool_calls.push(json!({
                "id": item.get("call_id").or_else(|| item.get("id")).cloned().unwrap_or_else(|| json!("call")),
                "type": "function",
                "function": { "name": item.get("name").cloned().unwrap_or_else(|| json!("")), "arguments": item.get("arguments").cloned().unwrap_or_else(|| json!("{}")) }
            })),
            Some("reasoning") => {}
            Some(kind) => return Err(invalid(format!("Responses output `{kind}` is not portable"))),
            None => return Err(invalid("Responses output item has no type")),
        }
    }
    let mut message = json!({ "role": "assistant", "content": content });
    let has_tool_calls = !tool_calls.is_empty();
    if has_tool_calls {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let input = Value::Object(obj.clone())
        .pointer("/usage/input_tokens")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let output = Value::Object(obj.clone())
        .pointer("/usage/output_tokens")
        .cloned()
        .unwrap_or_else(|| json!(0));
    Ok(json!({
        "id": obj.get("id").cloned().unwrap_or_else(|| json!("resp_translated")), "object": "chat.completion", "created": obj.get("created_at").cloned().unwrap_or_else(|| json!(0)), "model": requested_model,
        "choices": [{ "index": 0, "message": message, "finish_reason": if has_tool_calls { "tool_calls" } else { "stop" } }],
        "usage": { "prompt_tokens": input, "completion_tokens": output, "total_tokens": numeric_sum(&input, &output) }
    }))
}

fn chat_response_to_messages(payload: Value, requested_model: &str) -> Result<Value> {
    let obj = object(payload, "Chat Completions response")?;
    let choice = Value::Object(obj.clone())
        .pointer("/choices/0")
        .cloned()
        .ok_or_else(|| invalid("Chat response has no first choice"))?;
    let message = choice
        .get("message")
        .ok_or_else(|| invalid("Chat choice has no message"))?;
    let mut content =
        chat_content_to_anthropic(message.get("content").cloned().unwrap_or(Value::Null))?;
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let arguments = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let input: Value = serde_json::from_str(arguments)
                .map_err(|_| invalid("upstream tool arguments are not valid JSON"))?;
            content.push(json!({ "type": "tool_use", "id": call.get("id").cloned().unwrap_or_else(|| json!("call")), "name": call.pointer("/function/name").cloned().unwrap_or_else(|| json!("")), "input": input }));
        }
    }
    let input = Value::Object(obj.clone())
        .pointer("/usage/prompt_tokens")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let output = Value::Object(obj.clone())
        .pointer("/usage/completion_tokens")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let stop_reason = match choice.get("finish_reason").and_then(Value::as_str) {
        Some("length") => "max_tokens",
        Some("tool_calls") => "tool_use",
        _ => "end_turn",
    };
    Ok(
        json!({ "id": obj.get("id").cloned().unwrap_or_else(|| json!("msg_translated")), "type": "message", "role": "assistant", "model": requested_model, "content": content, "stop_reason": stop_reason, "stop_sequence": null, "usage": { "input_tokens": input, "output_tokens": output } }),
    )
}

fn chat_response_to_responses(payload: Value, requested_model: &str) -> Result<Value> {
    let obj = object(payload, "Chat Completions response")?;
    let choice = Value::Object(obj.clone())
        .pointer("/choices/0")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let message = choice.get("message").cloned().unwrap_or_else(|| json!({}));
    let content = chat_content_to_responses(
        message.get("content").cloned().unwrap_or(Value::Null),
        "assistant",
    )?;
    let mut output = vec![
        json!({ "type": "message", "id": "msg_translated", "status": "completed", "role": "assistant", "content": content }),
    ];
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        output.extend(calls.iter().map(|call| json!({ "type": "function_call", "id": call.get("id").cloned().unwrap_or_else(|| json!("call")), "call_id": call.get("id").cloned().unwrap_or_else(|| json!("call")), "name": call.pointer("/function/name").cloned().unwrap_or_else(|| json!("")), "arguments": call.pointer("/function/arguments").cloned().unwrap_or_else(|| json!("{}")), "status": "completed" })));
    }
    let input = Value::Object(obj.clone())
        .pointer("/usage/prompt_tokens")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let output_tokens = Value::Object(obj.clone())
        .pointer("/usage/completion_tokens")
        .cloned()
        .unwrap_or_else(|| json!(0));
    Ok(
        json!({ "id": obj.get("id").cloned().unwrap_or_else(|| json!("resp_translated")), "object": "response", "created_at": obj.get("created").cloned().unwrap_or_else(|| json!(0)), "status": "completed", "model": requested_model, "output": output, "usage": { "input_tokens": input, "output_tokens": output_tokens, "total_tokens": numeric_sum(&input, &output_tokens) } }),
    )
}

fn rename(obj: &mut Map<String, Value>, from: &str, to: &str) {
    if from == to {
        return;
    }
    if let Some(value) = obj.remove(from) {
        obj.insert(to.to_string(), value);
    }
}

fn numeric_sum(left: &Value, right: &Value) -> u64 {
    left.as_u64().unwrap_or(0) + right.as_u64().unwrap_or(0)
}

fn content_is_empty(content: &Value) -> bool {
    matches!(content, Value::Null)
        || matches!(content, Value::String(value) if value.is_empty())
        || matches!(content, Value::Array(value) if value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_tools_round_trip_through_messages() {
        let request = json!({
            "model": "model", "max_completion_tokens": 100,
            "messages": [
                { "role": "system", "content": "Be concise" },
                { "role": "assistant", "content": null, "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "weather", "arguments": "{\"city\":\"SF\"}" } }] },
                { "role": "tool", "tool_call_id": "call_1", "content": "sunny" }
            ],
            "tools": [{ "type": "function", "function": { "name": "weather", "parameters": { "type": "object" } } }]
        });
        let messages = translate_request(
            request,
            WireProtocol::ChatCompletions,
            WireProtocol::Messages,
        )
        .unwrap();
        assert_eq!(messages["system"][0]["text"], "Be concise");
        assert_eq!(messages["messages"][0]["content"][0]["type"], "tool_use");
        assert_eq!(messages["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(messages["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn responses_string_input_becomes_chat_message() {
        let translated = translate_request(
            json!({ "model": "model", "input": "hello", "max_output_tokens": 42 }),
            WireProtocol::Responses,
            WireProtocol::ChatCompletions,
        )
        .unwrap();
        assert_eq!(
            translated["messages"][0],
            json!({ "role": "user", "content": "hello" })
        );
        assert_eq!(translated["max_completion_tokens"], 42);
    }

    #[test]
    fn stateful_responses_input_is_rejected_instead_of_losing_context() {
        let error = translate_request(
            json!({ "model": "model", "input": "continue", "previous_response_id": "resp_1" }),
            WireProtocol::Responses,
            WireProtocol::Messages,
        )
        .unwrap_err();
        assert!(error.to_string().contains("previous_response_id"));
    }

    #[test]
    fn explicitly_stateless_responses_options_are_accepted() {
        let translated = translate_request(
            json!({ "model": "model", "input": "hello", "store": false, "truncation": "disabled" }),
            WireProtocol::Responses,
            WireProtocol::Messages,
        )
        .unwrap();
        assert!(translated.get("store").is_none());
        assert!(translated.get("truncation").is_none());
    }

    #[test]
    fn response_verbosity_is_rejected_instead_of_silently_dropped() {
        let error = translate_request(
            json!({ "model": "model", "input": "hello", "text": { "verbosity": "low" } }),
            WireProtocol::Responses,
            WireProtocol::ChatCompletions,
        )
        .unwrap_err();
        assert!(error.to_string().contains("verbosity"));
    }

    #[test]
    fn non_function_responses_tool_is_rejected() {
        let error = translate_request(
            json!({ "model": "model", "input": "search", "tools": [{ "type": "web_search_preview" }] }),
            WireProtocol::Responses,
            WireProtocol::ChatCompletions,
        ).unwrap_err();
        assert!(error.to_string().contains("built-in Responses tools"));
    }

    #[test]
    fn malformed_tool_arguments_fail_before_upstream() {
        let error = translate_request(
            json!({ "model": "model", "messages": [{ "role": "assistant", "tool_calls": [{ "id": "call_1", "function": { "name": "x", "arguments": "not json" } }] }] }),
            WireProtocol::ChatCompletions,
            WireProtocol::Messages,
        ).unwrap_err();
        assert!(error.to_string().contains("not valid JSON"));
    }

    #[test]
    fn anthropic_usage_and_tool_call_become_chat_response() {
        let translated = translate_response(
            json!({ "id": "msg_1", "content": [{ "type": "tool_use", "id": "call_1", "name": "weather", "input": { "city": "SF" } }], "stop_reason": "tool_use", "usage": { "input_tokens": 7, "output_tokens": 3 } }),
            WireProtocol::Messages,
            WireProtocol::ChatCompletions,
            "byo/claude",
        ).unwrap();
        assert_eq!(translated["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            translated["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "weather"
        );
        assert_eq!(translated["usage"]["total_tokens"], 10);
        assert_eq!(translated["model"], "byo/claude");
    }

    #[test]
    fn fragmented_messages_sse_becomes_chat_completions_sse() {
        let mut translator = SseTranslator::new(
            WireProtocol::Messages,
            WireProtocol::ChatCompletions,
            "byo/claude",
        );
        assert!(translator
            .push(b"event: content_block_delta\ndata: {\"type\":\"content_block_")
            .unwrap()
            .is_empty());
        let output = translator
            .push(b"delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n")
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("chat.completion.chunk"));
        assert!(
            output.contains("\\\"content\\\":\\\"hi\\\"") || output.contains("\"content\":\"hi\"")
        );
        assert!(translator.finish().unwrap().is_empty());
    }

    #[test]
    fn crlf_delimited_sse_is_translated() {
        let mut translator = SseTranslator::new(
            WireProtocol::Messages,
            WireProtocol::ChatCompletions,
            "byo/claude",
        );
        let output = translator
            .push(b"event: content_block_delta\r\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\r\n\r\n")
            .unwrap();
        assert!(String::from_utf8(output)
            .unwrap()
            .contains("chat.completion.chunk"));
        assert!(translator.finish().unwrap().is_empty());
    }

    #[test]
    fn incomplete_sse_event_fails_loudly_at_eof() {
        let mut translator =
            SseTranslator::new(WireProtocol::Responses, WireProtocol::Messages, "byo/gpt");
        assert!(translator.push(b"data: {\"type\":").unwrap().is_empty());
        assert!(translator
            .finish()
            .unwrap_err()
            .to_string()
            .contains("middle of an SSE event"));
    }
}
