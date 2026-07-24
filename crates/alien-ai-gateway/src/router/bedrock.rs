//! AWS Bedrock: serve Claude through classic `InvokeModel`, normalizing the Anthropic
//! Messages body to the pinned InvokeModel schema and decoding the event-stream reply.

use alien_error::{AlienError, Context, IntoAlienError};
use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use futures::StreamExt;
use serde_json::{json, Map, Value};
use tracing::warn;

use super::eventstream::EventStreamToSse;
use super::{forward_response, missing_field, parse_stream_flag, sign_and_execute, GatewayRoute};
use crate::error::{ErrorData, Result};

/// The client-executed tool families Bedrock hosts on classic `InvokeModel`
/// (verified against AWS docs). Anything else typed is server-executed by Anthropic's
/// own API servers, which Bedrock is not, so it is dropped rather than 400'd.
const BEDROCK_HOSTED_TOOL_PREFIXES: &[&str] =
    &["bash_", "text_editor_", "computer_", "memory_", "tool_search_"];

/// The `anthropic_beta` families Bedrock's classic `InvokeModel` accepts in the body
/// (each live-verified: an accepted tag returns 200, an unknown one is a
/// ValidationException "invalid beta flag"). Anthropic-API-side markers — notably
/// `oauth-2025-04-20`, which every OAuth-authenticated Claude Code request declares —
/// are rejected, so the header bridge folds only these families across.
const BEDROCK_BETA_PREFIXES: &[&str] = &[
    "claude-code-",
    "computer-use-",
    "context-1m-",
    "context-management-",
    "fine-grained-tool-streaming-",
    "interleaved-thinking-",
    "output-128k-",
    "token-efficient-tools-",
    "tool-examples-",
];

/// Serve a Claude request through classic Bedrock `InvokeModel`. The Anthropic
/// Messages body *is* the InvokeModel body, but the model id travels in the URL
/// (as a cross-region inference profile) and streaming is chosen by the URL
/// suffix — so, unlike the passthrough path, the body carries neither, and the
/// streamed reply arrives as AWS event-stream framing we decode back into the
/// Anthropic SSE the client expects.
///
/// This whole function is a protocol shim, kept only until Bedrock's mantle
/// endpoint serves Claude on the same standard model access InvokeModel already
/// grants. Where mantle does not yet serve Claude for a given account/region it
/// returns 403 while the same model and credential serve 200 via InvokeModel;
/// this shim bridges that gap. Drop it (and the decoder below) in favor of the
/// plain mantle passthrough once mantle serves Claude directly — to check whether
/// a region already does:
///
/// ```text
/// curl --aws-sigv4 "aws:amz:<region>:bedrock-mantle" --user "$KEY:$SECRET" \
///   -H "x-amz-security-token: $TOKEN" -H "content-type: application/json" \
///   -d '{"model":"anthropic.claude-haiku-4-5","max_tokens":16,
///        "messages":[{"role":"user","content":"Say ok"}]}' \
///   https://bedrock-mantle.<region>.api.aws/anthropic/v1/messages
/// ```
///
/// Bedrock's Converse API was evaluated (live, 2026-07-16) and rejected: it
/// works on standard access, but it speaks AWS's own schema in both directions,
/// so it would replace these targeted fixups with a full Anthropic⇄Converse
/// codec (content taxonomy, toolSpec, synthesized message ids, a Converse-event
/// stream translation) while still needing the event-stream decoder, the system
/// fold, the server-tool filter, and the beta bridge in relocated form.
pub(crate) async fn proxy_bedrock_anthropic(
    client: &reqwest::Client,
    route: &GatewayRoute,
    upstream_id: &str,
    mut payload: Value,
    headers: &HeaderMap,
) -> Result<Response> {
    let region = route.region.as_deref().ok_or_else(|| missing_field(route, "region"))?;

    let obj = payload.as_object_mut().ok_or_else(|| {
        AlienError::new(ErrorData::InvalidRequest {
            message: "request body must be a JSON object".to_string(),
        })
    })?;
    // The model is in the URL and streaming is chosen by the URL suffix, so neither
    // belongs in the body; Bedrock requires its own version marker there instead.
    obj.remove("model");
    // Bedrock's schema rejects a body `stream` field, so it is removed here.
    let stream = parse_stream_flag(obj.remove("stream"))?;
    obj.insert("anthropic_version".to_string(), json!("bedrock-2023-05-31"));

    // Claude clients declare betas in the `anthropic-beta` HTTP header, but classic
    // InvokeModel reads only the body's `anthropic_beta`. Bridge the Bedrock-known
    // families across so a beta-gated tool we forward (computer_*, memory_*) arrives
    // with the beta it needs — and drop the rest, which Bedrock's body validation
    // rejects (see merge_beta_headers).
    merge_beta_headers(obj, headers);

    // Bedrock's InvokeModel schema (pinned to `bedrock-2023-05-31`) predates the
    // newest Anthropic Messages fields, so a latest client (Claude Code) sends fields
    // it rejects. Drop the ones outside its schema so the request isn't a 400 — the
    // gateway is bridging a protocol-version gap, not the raw native endpoint.
    obj.remove("output_config");
    obj.remove("context_management");
    // Bedrock supports only `enabled`/`disabled` extended thinking; drop a newer mode
    // (e.g. `adaptive`) rather than let Bedrock reject the whole request.
    let thinking_unsupported = obj
        .get("thinking")
        .and_then(|t| t.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|t| t != "enabled" && t != "disabled");
    if thinking_unsupported {
        obj.remove("thinking");
    }
    // Anthropic *server*-executed tool types (web_search, code_execution, web fetch,
    // advisor) run on Anthropic's own API servers, which InvokeModel is not, so
    // Bedrock rejects them; drop those and keep the families Bedrock hosts. Dropping
    // (rather than a 400) is deliberate bridge behavior — Claude Code declares web
    // tools by default, so rejecting would fail its every request — but it must stay
    // visible in the logs and leave the body coherent: Bedrock rejects a `tool_choice`
    // that forces a tool that is no longer declared, and an emptied `tools` array.
    // A kept beta-gated family still needs its `anthropic_beta` entry, which
    // merge_beta_headers has already bridged from the header above. Also strip
    // `defer_loading`, a client-tool field Claude Code's on-demand tool loading adds
    // that the pinned schema rejects as an extra input.
    //
    // Residue blocks a *previous* server-tool turn left in `messages` (e.g.
    // `web_search_tool_result` from a conversation started on Anthropic's API) are
    // deliberately NOT rewritten: Bedrock knows those block types and rejects foreign
    // ones loudly (live-verified: Anthropic-issued `encrypted_content` fails its
    // validation), and that 400 reaches the client via forward_response — a loud,
    // honest failure, where stripping would silently alter the conversation.
    let mut dropped_tools: Vec<String> = Vec::new();
    let mut tools_remaining = true;
    if let Some(tools) = obj.get_mut("tools").and_then(Value::as_array_mut) {
        tools.retain(|tool| {
            let keep = match tool.get("type").and_then(Value::as_str) {
                // Plain client tools carry no type, or `custom`.
                None | Some("custom") => true,
                Some(tag) => BEDROCK_HOSTED_TOOL_PREFIXES.iter().any(|p| tag.starts_with(p)),
            };
            if !keep {
                let label = tool
                    .get("name")
                    .or_else(|| tool.get("type"))
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed");
                dropped_tools.push(label.to_string());
            }
            keep
        });
        for tool in tools.iter_mut() {
            if let Some(obj) = tool.as_object_mut() {
                obj.remove("defer_loading");
            }
        }
        tools_remaining = !tools.is_empty();
    }
    if !dropped_tools.is_empty() {
        warn!(
            binding = %route.name,
            tools = %dropped_tools.join(", "),
            "dropped Anthropic server-executed tools that Bedrock InvokeModel cannot serve"
        );
        if !tools_remaining {
            obj.remove("tools");
            obj.remove("tool_choice");
        } else {
            let forces_dropped = obj
                .get("tool_choice")
                .and_then(|choice| choice.get("name"))
                .and_then(Value::as_str)
                .is_some_and(|name| dropped_tools.iter().any(|dropped| dropped == name));
            if forces_dropped {
                obj.remove("tool_choice");
            }
        }
    }
    // The pinned schema also predates mid-conversation `system` roles inside
    // `messages` (top-level `system` is its only sanctioned spot) and enforces
    // user/assistant alternation. Re-tag those turns as `user` where they stand —
    // their position in the conversation is what carries the meaning — then fold
    // same-role neighbors into one message so alternation still holds.
    if let Some(messages) = obj.get_mut("messages").and_then(Value::as_array_mut) {
        let originals = std::mem::take(messages);
        for mut message in originals {
            if message.get("role").and_then(Value::as_str) == Some("system") {
                message["role"] = json!("user");
            }
            let same_role_as_last = messages
                .last()
                .and_then(|previous| previous.get("role"))
                .is_some_and(|role| Some(role) == message.get("role"));
            if let Some(previous) = messages.last_mut().filter(|_| same_role_as_last) {
                let addition = take_content_blocks(&mut message)?;
                let merged = ensure_block_content(previous)?;
                // A tool_use turn must be answered by tool_result blocks at the START
                // of the next message (live-verified: Bedrock 400s on `[text,
                // tool_result]`), so when the folded-in neighbor carries results —
                // e.g. a downgraded system turn landed between a tool call and its
                // result — they slot in right after any results already leading.
                let (tool_results, rest): (Vec<Value>, Vec<Value>) =
                    addition.into_iter().partition(|block| {
                        block.get("type").and_then(Value::as_str) == Some("tool_result")
                    });
                let leading = merged
                    .iter()
                    .take_while(|block| {
                        block.get("type").and_then(Value::as_str) == Some("tool_result")
                    })
                    .count();
                merged.splice(leading..leading, tool_results);
                merged.extend(rest);
            } else {
                messages.push(message);
            }
        }
    }

    let upstream_body = serde_json::to_vec(&payload)
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not re-serialize the Bedrock request body".to_string(),
        })?;

    let suffix = if stream { "invoke-with-response-stream" } else { "invoke" };
    let model_id = format!("{}.{}", bedrock_geo(region), upstream_id);
    let base = route
        .upstream_base_override
        .clone()
        .unwrap_or_else(|| format!("https://bedrock-runtime.{region}.amazonaws.com"));
    let url = format!("{}/model/{}/{}", base.trim_end_matches('/'), model_id, suffix);

    let upstream = sign_and_execute(client, &route.cred, &url, "bedrock", upstream_body, &[]).await?;

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    // Only a 2xx streaming reply is event-stream framed. A non-2xx (throttling,
    // missing model access, bad request) and every non-streaming reply are plain
    // JSON, so forward them untouched — the client sees the real status and body.
    if !upstream.status().is_success() || !stream {
        return forward_response(upstream);
    }

    // Decode the event-stream frames into Anthropic SSE as they arrive (the decoder
    // buffers across network chunks, so partial frames don't corrupt the output),
    // then flush once the upstream closes: a stream that ended mid-frame surfaces a
    // loud error via finish() instead of a silently truncated reply.
    let sse = futures::stream::unfold(
        (Box::pin(upstream.bytes_stream()), EventStreamToSse::default(), false),
        |(mut body, mut decoder, done)| async move {
            if done {
                return None;
            }
            match body.next().await {
                Some(Ok(bytes)) => {
                    Some((Ok(Bytes::from(decoder.push(&bytes))), (body, decoder, false)))
                }
                Some(Err(err)) => Some((Err(err), (body, decoder, true))),
                // Upstream closed: emit the end-of-stream flush, then stop.
                None => Some((Ok(Bytes::from(decoder.finish())), (body, decoder, true))),
            }
        },
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(sse))
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not build the streamed response".to_string(),
        })
}

/// Merge the request's `anthropic-beta` headers into the body's `anthropic_beta`.
/// The header may repeat and each value may be comma-separated; the body field takes
/// an array or a single string. Body entries are kept and duplicates dropped, so a
/// client may declare betas any of those ways.
///
/// Only BEDROCK_BETA_PREFIXES families are bridged. Bedrock ignores unknown tags in
/// the *header* but validates the *body* list, so folding an Anthropic-API-side
/// marker across turns the whole request into a ValidationException. Body entries
/// stay unfiltered: a client authoring Bedrock-dialect JSON asked for exactly that
/// list and gets Bedrock's loud answer.
fn merge_beta_headers(obj: &mut Map<String, Value>, headers: &HeaderMap) {
    let mut betas: Vec<String> = match obj.get("anthropic_beta") {
        Some(Value::Array(list)) => {
            list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()
        }
        Some(Value::String(tag)) => vec![tag.clone()],
        _ => Vec::new(),
    };
    for beta in filtered_header_betas(headers) {
        if !betas.iter().any(|existing| existing == &beta) {
            betas.push(beta);
        }
    }
    if !betas.is_empty() {
        obj.insert("anthropic_beta".to_string(), json!(betas));
    }
}

/// The client's `anthropic-beta` declarations that pass the allowlist. The header
/// may repeat and each value may be comma-separated. Filtering is an allowlist
/// because these endpoints validate what they are handed: an Anthropic-API-side
/// marker (notably `oauth-2025-04-20`, declared by every OAuth Claude Code
/// request) turns the whole request into a 400. Vertex and Foundry reuse the
/// Bedrock-verified families until their own live probes verify per-upstream
/// lists.
pub(crate) fn filtered_header_betas(headers: &HeaderMap) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    for value in headers.get_all("anthropic-beta") {
        let Ok(raw) = value.to_str() else { continue };
        for beta in raw.split(',').map(str::trim).filter(|b| !b.is_empty()) {
            if !BEDROCK_BETA_PREFIXES.iter().any(|p| beta.starts_with(p)) {
                dropped.push(beta.to_string());
                continue;
            }
            if !kept.iter().any(|existing| existing == beta) {
                kept.push(beta.to_string());
            }
        }
    }
    if !dropped.is_empty() {
        warn!(
            betas = %dropped.join(", "),
            "dropped anthropic-beta tags outside the allowlisted families"
        );
    }
    kept
}

/// Normalize a message's `content` to a block array and hand the array back, so two
/// messages folding into one can concatenate their block lists. A string becomes a
/// single text block; an existing array is kept; any other shape (missing, null, an
/// object) is a malformed message the native Anthropic endpoint would reject — fail
/// loud rather than fold the turn into an empty array and answer a conversation the
/// client didn't send.
pub(crate) fn ensure_block_content(message: &mut Value) -> Result<&mut Vec<Value>> {
    match message.get("content") {
        Some(Value::Array(_)) => {}
        Some(Value::String(text)) => {
            message["content"] = json!([{ "type": "text", "text": text }]);
        }
        _ => {
            return Err(AlienError::new(ErrorData::InvalidRequest {
                message: "every message `content` must be a string or an array of blocks"
                    .to_string(),
            }))
        }
    }
    Ok(message["content"]
        .as_array_mut()
        .expect("content was just normalized to an array"))
}

/// Take a message's content as a block list, leaving the message with an empty one.
fn take_content_blocks(message: &mut Value) -> Result<Vec<Value>> {
    Ok(std::mem::take(ensure_block_content(message)?))
}

/// The cross-region inference-profile geo prefix for a Bedrock region. Claude on
/// Bedrock is invocable only through a geo profile (e.g. `us.anthropic.…`).
///
/// us / us-gov regions keep their own geo. Every other commercial region routes via
/// the region-agnostic `global` profile: current-generation Claude models publish a
/// `global.` inference profile invocable from any commercial region (verified against
/// live Bedrock), and do NOT publish `eu.`/`apac.` profiles, so a per-continent
/// prefix would build a non-existent id. (An older model that publishes only a `us.`
/// profile, e.g. opus-4.1, stays us-region-only either way.)
pub(crate) fn bedrock_geo(region: &str) -> &'static str {
    if region.starts_with("us-gov-") {
        "us-gov"
    } else if region.starts_with("us-") {
        "us"
    } else {
        "global"
    }
}
