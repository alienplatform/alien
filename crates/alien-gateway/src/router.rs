//! The pure proxy: route a request to the model's native cloud endpoint, inject the
//! workload's ambient credential, and stream the response back without translating
//! the body. The only edit to the request body is rewriting the public model id to
//! the catalog's upstream id; the response (JSON or SSE) is passed through byte-for-byte.

use std::collections::HashMap;
use std::sync::Arc;

use alien_core::ai_catalog::{self, Protocol};
use alien_core::Platform;
use alien_error::{AlienError, Context, IntoAlienError};
use aws_smithy_eventstream::frame::{DecodedFrame, MessageFrameDecoder};
use aws_smithy_types::event_stream::Message;
use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures::StreamExt;
use serde_json::{json, Map, Value};
use tracing::warn;

use crate::creds::AmbientCred;
use crate::error::{ErrorData, Result};

/// One binding resolved into everything the proxy needs to serve it: the cloud (for
/// catalog filtering and upstream selection), the location fields used to build the
/// upstream URL, and the ambient credential.
pub struct GatewayRoute {
    /// The binding name — the first path segment the app calls (`/<name>/...`).
    pub name: String,
    pub cloud: Platform,
    /// AWS region or GCP location.
    pub region: Option<String>,
    /// GCP project id.
    pub project: Option<String>,
    /// Azure account endpoint, e.g. `https://acct.openai.azure.com/`.
    pub azure_endpoint: Option<String>,
    pub cred: AmbientCred,
    /// When set, upstream requests target this base URL instead of the cloud-derived
    /// host (the per-protocol path is still appended). Lets tests aim a binding at a
    /// mock upstream.
    pub upstream_base_override: Option<String>,
}

struct AppState {
    routes: HashMap<String, GatewayRoute>,
    client: reqwest::Client,
}

/// Build the axum router serving every binding under `/<name>/...`:
/// `POST /<name>/v1/chat/completions` (OpenAI), `POST /<name>/v1/messages`
/// (Anthropic), and `GET /<name>/v1/models`.
pub fn build_router(routes: Vec<GatewayRoute>) -> Router {
    let routes = routes.into_iter().map(|r| (r.name.clone(), r)).collect();
    let state = Arc::new(AppState {
        routes,
        client: reqwest::Client::new(),
    });
    Router::new()
        .route("/{binding}/v1/chat/completions", post(proxy))
        .route("/{binding}/v1/messages", post(proxy))
        .route("/{binding}/v1/responses", post(proxy_responses))
        .route("/{binding}/v1/models", get(list_models))
        .with_state(state)
}

/// Parse a proxied request body as JSON and pull out its required `model` field.
/// Both the chat/completions|messages handler and the Responses handler route on
/// the request's `model`, so they share this preamble.
fn parse_model_request(body: &[u8]) -> Result<(Value, String)> {
    let payload: Value = serde_json::from_slice(body)
        .into_alien_error()
        .context(ErrorData::InvalidRequest {
            message: "request body is not valid JSON".to_string(),
        })?;
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidRequest {
                message: "request body has no \"model\" field".to_string(),
            })
        })?
        .to_string();
    Ok((payload, model))
}

/// Forward an upstream reply to the client untouched: its status, content-type, and
/// body, streamed straight through. Streaming the body works identically for a
/// single JSON object and for an SSE stream.
fn forward_response(upstream: reqwest::Response) -> Result<Response> {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let mut response = Response::builder().status(status);
    if let Some(ct) = content_type {
        response = response.header(header::CONTENT_TYPE, ct);
    }
    response
        .body(Body::from_stream(upstream.bytes_stream()))
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not build the proxied response".to_string(),
        })
}

/// Build a JSON POST to `url`, sign it with the ambient credential for `service`,
/// and execute it. The handlers differ only in URL, signing service, body, and any
/// protocol-required header, so the build + sign + execute + upstream-error
/// scaffolding lives here once.
async fn sign_and_execute(
    client: &reqwest::Client,
    cred: &AmbientCred,
    url: &str,
    service: &str,
    body: Vec<u8>,
    extra_headers: &[(&str, &str)],
) -> Result<reqwest::Response> {
    let mut builder = client
        .post(url)
        .header(header::CONTENT_TYPE, "application/json");
    for (name, value) in extra_headers {
        builder = builder.header(*name, *value);
    }
    let mut req = builder
        .body(body)
        .build()
        .into_alien_error()
        .context(ErrorData::Other {
            // The url names which upstream failed; the handlers otherwise share
            // this message and a bare one cannot be traced back to a path.
            message: format!("could not build the upstream request to {url}"),
        })?;
    cred.authorize(&mut req, service).await?;
    client
        .execute(req)
        .await
        .into_alien_error()
        .context(ErrorData::UpstreamFailed {
            message: format!("request to {url} failed"),
        })
}

/// Proxy a chat/completions or messages request. Routes purely by the request's
/// `model` (the catalog is the single source of truth for protocol + cloud), so the
/// same handler serves both the OpenAI and Anthropic entry paths.
async fn proxy(
    State(state): State<Arc<AppState>>,
    Path(binding): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let route = state.routes.get(&binding).ok_or_else(|| {
        AlienError::new(ErrorData::UnknownBinding {
            binding: binding.clone(),
        })
    })?;

    let (mut payload, model) = parse_model_request(&body)?;

    // Cloud-scoped resolution: Claude ids appear once per cloud, so a first-match
    // resolve would always land on another cloud's entry and fail the cloud filter.
    let cm = ai_catalog::resolve_for(&model, route.cloud).ok_or_else(|| {
        AlienError::new(ErrorData::ModelNotAvailable {
            model: model.clone(),
            binding: binding.clone(),
        })
    })?;

    // AWS serves Claude through classic Bedrock InvokeModel, not the passthrough
    // endpoint: the model id travels in the URL and the streamed reply is AWS
    // event-stream framing, so it needs its own request/response shape.
    if route.cloud == Platform::Aws && cm.protocol == Protocol::Anthropic {
        return proxy_bedrock_anthropic(&state.client, route, cm.upstream_id, payload, &headers)
            .await;
    }
    // GCP serves Claude through Vertex rawPredict: the model id travels in the URL
    // and streaming is chosen by the URL verb, but the reply is native Anthropic
    // JSON/SSE — no decoder needed, unlike Bedrock.
    if route.cloud == Platform::Gcp && cm.protocol == Protocol::Anthropic {
        return proxy_vertex_anthropic(&state.client, route, cm.upstream_id, payload, &headers)
            .await;
    }
    // Azure serves Claude through Foundry's Anthropic endpoint: standard Messages
    // in both directions, on the `/anthropic/v1` path with the version header.
    if route.cloud == Platform::Azure && cm.protocol == Protocol::Anthropic {
        return proxy_foundry_anthropic(&state.client, route, cm.upstream_id, payload, &headers)
            .await;
    }

    payload["model"] = Value::String(cm.upstream_id.to_string());
    let upstream_body = serde_json::to_vec(&payload)
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not re-serialize the rewritten request body".to_string(),
        })?;

    let (url, aws_service) = upstream_target(route, cm.protocol)?;

    let upstream =
        sign_and_execute(&state.client, &route.cred, &url, aws_service, upstream_body, &[]).await?;

    forward_response(upstream)
}

/// Proxy an OpenAI Responses request (`POST /<name>/v1/responses`, used by Codex).
/// AWS serves the Responses API natively on the bedrock-mantle endpoint, so this is
/// the same pure passthrough as `proxy` — rewrite the model id, sign, stream back —
/// but aimed at the mantle endpoint. Only AWS OpenAI-protocol models are servable here: the
/// other clouds don't expose a Responses endpoint, and Claude on mantle is
/// Messages-only.
async fn proxy_responses(
    State(state): State<Arc<AppState>>,
    Path(binding): Path<String>,
    body: Bytes,
) -> Result<Response> {
    let route = state.routes.get(&binding).ok_or_else(|| {
        AlienError::new(ErrorData::UnknownBinding {
            binding: binding.clone(),
        })
    })?;

    let (mut payload, model) = parse_model_request(&body)?;

    // The Responses table implies AWS; the binding's cloud must still match so a
    // GCP/Azure binding doesn't forward to an AWS endpoint it has no credential for.
    let upstream_id = ai_catalog::responses_upstream_id(&model)
        .filter(|_| route.cloud == Platform::Aws)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ModelNotAvailable {
                model: model.clone(),
                binding: binding.clone(),
            })
        })?;

    payload["model"] = Value::String(upstream_id.to_string());
    let upstream_body = serde_json::to_vec(&payload)
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not re-serialize the rewritten request body".to_string(),
        })?;

    let region = route.region.as_deref().ok_or_else(|| missing_field(route, "region"))?;
    let base = route
        .upstream_base_override
        .clone()
        .unwrap_or_else(|| format!("https://bedrock-mantle.{region}.api.aws"));
    let url = format!("{}/v1/responses", base.trim_end_matches('/'));

    let upstream =
        sign_and_execute(&state.client, &route.cred, &url, "bedrock-mantle", upstream_body, &[])
            .await?;

    forward_response(upstream)
}

/// `GET /<name>/v1/models` — the binding's cloud's curated catalog, in OpenAI list shape.
async fn list_models(
    State(state): State<Arc<AppState>>,
    Path(binding): Path<String>,
) -> Result<Response> {
    let route = state.routes.get(&binding).ok_or_else(|| {
        AlienError::new(ErrorData::UnknownBinding { binding })
    })?;
    let data: Vec<Value> = ai_catalog::models_for(route.cloud)
        .iter()
        .map(|m| json!({ "id": m.public_id, "object": "model" }))
        .collect();
    Ok(Json(json!({ "object": "list", "data": data })).into_response())
}

/// The error for a binding missing a field a handler needs.
fn missing_field(route: &GatewayRoute, field: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::BindingConfigInvalid {
        binding: route.name.clone(),
        message: format!("it is missing its {field}"),
    })
}

/// The upstream URL and (for AWS) the SigV4 service name for a binding + protocol.
fn upstream_target(route: &GatewayRoute, protocol: Protocol) -> Result<(String, &'static str)> {
    let (host, path, aws_service) = match (route.cloud, protocol) {
        (Platform::Aws, Protocol::OpenAi) => {
            let region = route.region.as_deref().ok_or_else(|| missing_field(route, "region"))?;
            (
                format!("https://bedrock-runtime.{region}.amazonaws.com"),
                "/openai/v1/chat/completions".to_string(),
                "bedrock",
            )
        }
        (Platform::Gcp, Protocol::OpenAi) => {
            let location =
                route.region.as_deref().ok_or_else(|| missing_field(route, "location"))?;
            let project =
                route.project.as_deref().ok_or_else(|| missing_field(route, "project"))?;
            (
                vertex_host(location),
                format!(
                    "/v1/projects/{project}/locations/{location}/endpoints/openapi/chat/completions"
                ),
                "",
            )
        }
        (Platform::Azure, Protocol::OpenAi) => {
            let endpoint =
                route.azure_endpoint.as_deref().ok_or_else(|| missing_field(route, "endpoint"))?;
            (
                endpoint.trim_end_matches('/').to_string(),
                "/openai/v1/chat/completions".to_string(),
                "",
            )
        }
        (cloud, proto) => {
            return Err(AlienError::new(ErrorData::Other {
                message: format!("{cloud:?} does not serve the {proto:?} protocol"),
            }))
        }
    };

    let base = route
        .upstream_base_override
        .clone()
        .unwrap_or(host);
    Ok((format!("{}{}", base.trim_end_matches('/'), path), aws_service))
}

/// Read a request's `stream` field. Streaming picks between two different
/// upstream shapes, so a malformed value must be a loud 400 — coercing it would
/// answer an SSE client with a JSON body it can only interpret as a hang.
fn parse_stream_flag(value: Option<Value>) -> Result<bool> {
    match value {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(value),
        Some(_) => Err(AlienError::new(ErrorData::InvalidRequest {
            message: "the `stream` field must be a boolean".to_string(),
        })),
    }
}

/// The Vertex AI Platform host for a location: the global endpoint is the
/// un-prefixed host; a region prefixes it. The path carries `locations/{location}`
/// either way.
fn vertex_host(location: &str) -> String {
    if location == "global" {
        "https://aiplatform.googleapis.com".to_string()
    } else {
        format!("https://{location}-aiplatform.googleapis.com")
    }
}

/// Serve a Claude request through Vertex `rawPredict`. Nearly the Anthropic
/// Messages API: the model id travels in the URL, streaming picks the URL verb
/// (`:streamRawPredict`), and the body carries Vertex's version marker instead of
/// a `model`. The reply is native Anthropic JSON/SSE, so unlike the Bedrock shim
/// there is no event-stream decoder — and betas ride the standard `anthropic-beta`
/// header rather than a body field, since Vertex speaks the native Messages API.
async fn proxy_vertex_anthropic(
    client: &reqwest::Client,
    route: &GatewayRoute,
    upstream_id: &str,
    mut payload: Value,
    headers: &HeaderMap,
) -> Result<Response> {
    let location = route.region.as_deref().ok_or_else(|| missing_field(route, "location"))?;
    let project = route.project.as_deref().ok_or_else(|| missing_field(route, "project"))?;

    let obj = payload.as_object_mut().ok_or_else(|| {
        AlienError::new(ErrorData::InvalidRequest {
            message: "request body must be a JSON object".to_string(),
        })
    })?;
    obj.remove("model");
    obj.insert("anthropic_version".to_string(), json!("vertex-2023-10-16"));
    // The `stream` field stays in the body; Vertex accepts it alongside the verb.
    let stream = parse_stream_flag(obj.get("stream").cloned())?;
    let verb = if stream { "streamRawPredict" } else { "rawPredict" };

    let base = route.upstream_base_override.clone().unwrap_or_else(|| vertex_host(location));
    let url = format!(
        "{}/v1/projects/{project}/locations/{location}/publishers/anthropic/models/{upstream_id}:{verb}",
        base.trim_end_matches('/')
    );

    let upstream_body = serde_json::to_vec(&payload)
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not re-serialize the rewritten request body".to_string(),
        })?;

    // Vertex is the native Messages API, so betas ride the standard header —
    // filtered through the same allowlist that keeps Anthropic-API-side markers
    // (notably oauth-2025-04-20) from turning the request into a 400.
    let betas = filtered_header_betas(headers).join(",");
    let mut extra_headers: Vec<(&str, &str)> = Vec::new();
    if !betas.is_empty() {
        extra_headers.push(("anthropic-beta", betas.as_str()));
    }
    let upstream =
        sign_and_execute(client, &route.cred, &url, "", upstream_body, &extra_headers).await?;
    forward_response(upstream)
}

/// The Messages API version the gateway bridges to Foundry's Anthropic endpoint;
/// Foundry reads it from the standard `anthropic-version` header.
const FOUNDRY_ANTHROPIC_VERSION: &str = "2023-06-01";

/// Serve a Claude request through Foundry's Anthropic endpoint. The closest arm to
/// the plain passthrough: the model stays in the body (rewritten to the Foundry
/// deployment name), streaming is the standard body field, and the reply is native
/// Anthropic JSON/SSE — only the version header and the `/anthropic/v1` path
/// distinguish it from the OpenAI arm.
async fn proxy_foundry_anthropic(
    client: &reqwest::Client,
    route: &GatewayRoute,
    upstream_id: &str,
    mut payload: Value,
    headers: &HeaderMap,
) -> Result<Response> {
    let endpoint =
        route.azure_endpoint.as_deref().ok_or_else(|| missing_field(route, "endpoint"))?;

    payload["model"] = Value::String(upstream_id.to_string());
    let upstream_body = serde_json::to_vec(&payload)
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not re-serialize the rewritten request body".to_string(),
        })?;

    // The binding carries the AIServices account endpoint; the Anthropic path
    // serves on that account. Whether the account host also needs the Entra
    // audience swapped to https://ai.azure.com is settled by the live Foundry
    // probe — the credential keeps the account audience until that probe says
    // otherwise.
    let base = route
        .upstream_base_override
        .clone()
        .unwrap_or_else(|| endpoint.to_string());
    let url = format!("{}/anthropic/v1/messages", base.trim_end_matches('/'));

    // Foundry speaks the standard Anthropic protocol, so betas ride the standard
    // header — filtered through the same allowlist that keeps Anthropic-API-side
    // markers from turning the request into a 400.
    let betas = filtered_header_betas(headers).join(",");
    let mut extra_headers = vec![("anthropic-version", FOUNDRY_ANTHROPIC_VERSION)];
    if !betas.is_empty() {
        extra_headers.push(("anthropic-beta", betas.as_str()));
    }
    let upstream =
        sign_and_execute(client, &route.cred, &url, "", upstream_body, &extra_headers).await?;
    forward_response(upstream)
}

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
async fn proxy_bedrock_anthropic(
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
fn filtered_header_betas(headers: &HeaderMap) -> Vec<String> {
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
fn ensure_block_content(message: &mut Value) -> Result<&mut Vec<Value>> {
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
fn bedrock_geo(region: &str) -> &'static str {
    if region.starts_with("us-gov-") {
        "us-gov"
    } else if region.starts_with("us-") {
        "us"
    } else {
        "global"
    }
}

/// Decoder turning Bedrock's `vnd.amazon.eventstream` framing into Anthropic SSE.
/// A normal chunk frame's payload is `{"bytes": base64(<anthropic event json>)}`;
/// the decoded event carries a `type` we surface as the SSE `event:` name. Network
/// chunks can split or merge frames, so bytes are buffered until each frame is
/// whole. Frame parsing (prelude, headers, both CRC32 checks) is
/// aws-smithy-eventstream's `MessageFrameDecoder` — AWS's own decoder for this
/// wire format — so a corrupted or desynced stream fails its CRCs instead of
/// decoding to garbage.
#[derive(Default)]
struct EventStreamToSse {
    buf: Vec<u8>,
    /// Set once a frame fails to decode (a CRC mismatch or malformed prelude).
    /// From then on the buffer can never be drained, so we stop parsing rather than
    /// spin on it forever.
    failed: bool,
}

impl EventStreamToSse {
    /// Append a network chunk and return the SSE for every frame it now completes.
    fn push(&mut self, chunk: &[u8]) -> String {
        if self.failed {
            return String::new();
        }
        self.buf.extend_from_slice(chunk);
        let mut out = String::new();
        // A fresh decoder scans the buffer from the top each push, and only the bytes
        // of fully decoded frames are drained: MessageFrameDecoder consumes a prelude
        // into internal state before its frame completes, so reusing one across
        // pushes would strand those bytes between the two buffers.
        let mut decoder = MessageFrameDecoder::new();
        let mut cursor: &[u8] = &self.buf;
        let mut consumed = 0;
        loop {
            match decoder.decode_frame(&mut cursor) {
                Ok(DecodedFrame::Complete(message)) => {
                    consumed = self.buf.len() - cursor.len();
                    out.push_str(&message_to_sse(&message));
                }
                Ok(DecodedFrame::Incomplete) => break,
                Err(_) => {
                    // A CRC mismatch or malformed prelude can never recover: the byte
                    // stream desynced. Surface it loudly and stop, rather than
                    // silently stall on bytes we can never drain (which would
                    // truncate the reply under an already-sent 200).
                    self.failed = true;
                    out.push_str(&error_sse("the model response stream could not be decoded"));
                    break;
                }
            }
        }
        self.buf.drain(0..consumed);
        out
    }

    /// Flush at end of stream. A non-empty buffer here means the upstream closed
    /// mid-frame (a truncated or desynced stream), so surface a loud error rather
    /// than drop the tail; a clean boundary (empty buffer) emits nothing.
    fn finish(&mut self) -> String {
        if self.failed || self.buf.is_empty() {
            return String::new();
        }
        self.failed = true;
        error_sse("the model response stream ended before the final frame completed")
    }
}

/// One event-stream message rendered as the Anthropic SSE the client expects.
///
/// A normal chunk wraps the event as `{"bytes": base64(...)}`. Anything else on an
/// InvokeModelWithResponseStream reply is an exception frame: Bedrock signals
/// mid-stream failures (throttlingException, modelStreamErrorException,
/// internalServerException) this way, with the exception body as the raw payload.
/// Such a frame is surfaced as an Anthropic `error` SSE event rather than dropped,
/// because dropping it would truncate the reply under an already-sent 200 with no
/// error reaching the client.
fn message_to_sse(message: &Message) -> String {
    let outer: Option<Value> = serde_json::from_slice(message.payload()).ok();
    if let Some(sse) = outer.as_ref().and_then(chunk_to_sse) {
        return sse;
    }
    // Exception / error frame: forward Bedrock's own message so the client sees why.
    let message = outer
        .as_ref()
        .and_then(|o| o.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("the model returned an error mid-stream");
    error_sse(message)
}

/// A normal `{"bytes": base64(<anthropic event>)}` chunk rendered as its SSE line,
/// or `None` if the frame is not a well-formed chunk.
fn chunk_to_sse(outer: &Value) -> Option<String> {
    let event_bytes = STANDARD.decode(outer.get("bytes")?.as_str()?).ok()?;
    let event: Value = serde_json::from_slice(&event_bytes).ok()?;
    let event_type = event.get("type")?.as_str()?;
    let data = std::str::from_utf8(&event_bytes).ok()?;
    Some(format!("event: {event_type}\ndata: {data}\n\n"))
}

/// An Anthropic `error` SSE event carrying `message`, so a mid-stream failure
/// reaches the client as a loud error instead of a silently truncated reply.
fn error_sse(message: &str) -> String {
    let event = json!({ "type": "error", "error": { "type": "api_error", "message": message } });
    format!("event: error\ndata: {event}\n\n")
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use aws_credential_types::provider::SharedCredentialsProvider;
    use aws_credential_types::Credentials;
    use aws_smithy_eventstream::frame::write_message_to;
    use httpmock::prelude::*;

    use super::*;
    use crate::creds::{AwsSigV4Cred, BearerTokenCred};

    fn test_aws_cred() -> AmbientCred {
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

    async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind test server");
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        url
    }

    fn aws_route(upstream: &str) -> GatewayRoute {
        GatewayRoute {
            name: "llm".to_string(),
            cloud: Platform::Aws,
            region: Some("us-east-2".to_string()),
            project: None,
            azure_endpoint: None,
            cred: test_aws_cred(),
            upstream_base_override: Some(upstream.to_string()),
        }
    }

    fn gcp_route(location: &str) -> GatewayRoute {
        GatewayRoute {
            name: "llm".to_string(),
            cloud: Platform::Gcp,
            region: Some(location.to_string()),
            project: Some("my-proj".to_string()),
            azure_endpoint: None,
            cred: AmbientCred::Bearer(BearerTokenCred::static_token("t")),
            upstream_base_override: None,
        }
    }

    #[test]
    fn gcp_vertex_url_regional_vs_global() {
        // A region prefixes the host; `global` uses the un-prefixed host. The path always
        // carries `locations/{location}`.
        let (regional, _) = upstream_target(&gcp_route("us-central1"), Protocol::OpenAi).unwrap();
        assert_eq!(
            regional,
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-proj/locations/us-central1/endpoints/openapi/chat/completions"
        );
        let (global, _) = upstream_target(&gcp_route("global"), Protocol::OpenAi).unwrap();
        assert_eq!(
            global,
            "https://aiplatform.googleapis.com/v1/projects/my-proj/locations/global/endpoints/openapi/chat/completions"
        );
    }

    #[tokio::test]
    async fn vertex_claude_rewrites_body_and_url() {
        // Claude on Vertex: the model travels in the URL (as the Vertex `@date` id,
        // resolved from Claude Code's dashed spelling), the body carries Vertex's
        // version marker instead of a `model`, and the bearer credential rides along.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/projects/my-proj/locations/us-east5/publishers/anthropic/models/claude-haiku-4-5@20251001:rawPredict")
                    .matches(|req: &HttpMockRequest| {
                        let body: Value =
                            serde_json::from_slice(req.body.as_deref().unwrap_or_default())
                                .unwrap_or(Value::Null);
                        body.get("model").is_none()
                            && body["anthropic_version"] == "vertex-2023-10-16"
                    })
                    .matches(|req: &HttpMockRequest| {
                        req.headers.as_ref().is_some_and(|headers| {
                            headers.iter().any(|(name, value)| {
                                name.eq_ignore_ascii_case("authorization")
                                    && value.starts_with("Bearer ")
                            })
                        })
                    });
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"id":"msg_1","content":[{"type":"text","text":"pong"}]}"#);
            })
            .await;

        let mut route = gcp_route("us-east5");
        route.upstream_base_override = Some(server.base_url());
        let url = serve(build_router(vec![route])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-haiku-4-5-20251001",
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("\"pong\""), "upstream body must pass through: {text}");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn vertex_claude_streaming_uses_stream_verb() {
        // `stream: true` picks the `:streamRawPredict` verb, and Vertex's native
        // Anthropic SSE passes through byte-for-byte — no event-stream decode.
        let sse = "event: message_start\ndata: {\"type\":\"message_start\"}\n\n\
                   event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/projects/my-proj/locations/us-east5/publishers/anthropic/models/claude-opus-4-8:streamRawPredict");
                then.status(200)
                    .header("content-type", "text/event-stream")
                    .body(sse);
            })
            .await;

        let mut route = gcp_route("us-east5");
        route.upstream_base_override = Some(server.base_url());
        let url = serve(build_router(vec![route])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-opus-4.8",
                "stream": true,
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        assert_eq!(resp.headers().get("content-type").unwrap(), "text/event-stream");
        assert_eq!(resp.text().await.unwrap(), sse, "SSE must stream through byte-for-byte");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn vertex_claude_rejects_non_boolean_stream() {
        // A malformed `stream` picks between two upstream verbs, so it must be a
        // loud 400, not a coerced guess.
        let mut route = gcp_route("us-east5");
        route.upstream_base_override = Some("http://unused.invalid".to_string());
        let url = serve(build_router(vec![route])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({"model": "claude-opus-4.8", "stream": "yes", "messages": []}))
            .send()
            .await
            .expect("proxy request");
        assert_eq!(resp.status(), 400);
        assert!(
            resp.text().await.unwrap().contains("GATEWAY_INVALID_REQUEST"),
            "must fail on the stream-validation path, not some other 400"
        );
    }

    #[tokio::test]
    async fn vertex_claude_forwards_allowlisted_betas_as_header() {
        // Vertex is the native Messages API: betas ride the standard header, not a
        // body field. An allowlisted family crosses over; the OAuth marker every
        // wrapped Claude Code session declares does not.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header("anthropic-beta", "computer-use-2025-01-24")
                    .matches(|req: &HttpMockRequest| {
                        let body: Value =
                            serde_json::from_slice(req.body.as_deref().unwrap_or_default())
                                .unwrap_or(Value::Null);
                        // Betas do NOT go in the body for Vertex.
                        body.get("anthropic_beta").is_none()
                    });
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"id":"msg_1","content":[]}"#);
            })
            .await;

        let mut route = gcp_route("us-east5");
        route.upstream_base_override = Some(server.base_url());
        let url = serve(build_router(vec![route])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .header("anthropic-beta", "computer-use-2025-01-24, oauth-2025-04-20")
            .json(&json!({"model": "claude-opus-4.8", "max_tokens": 16, "messages": []}))
            .send()
            .await
            .expect("proxy request");
        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    fn azure_route(endpoint: &str) -> GatewayRoute {
        GatewayRoute {
            name: "llm".to_string(),
            cloud: Platform::Azure,
            region: None,
            project: None,
            azure_endpoint: Some(endpoint.to_string()),
            cred: AmbientCred::Bearer(BearerTokenCred::static_token("t")),
            upstream_base_override: None,
        }
    }

    #[tokio::test]
    async fn foundry_claude_rewrites_model_and_sends_version_header() {
        // Claude on Foundry: the model stays in the body, rewritten to the Foundry
        // deployment name, on the `/anthropic/v1` path with the version header and
        // the bearer credential.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/anthropic/v1/messages")
                    .header("anthropic-version", "2023-06-01")
                    .matches(|req: &HttpMockRequest| {
                        let body: Value =
                            serde_json::from_slice(req.body.as_deref().unwrap_or_default())
                                .unwrap_or(Value::Null);
                        body["model"] == "claude-opus-4-8"
                    })
                    .matches(|req: &HttpMockRequest| {
                        req.headers.as_ref().is_some_and(|headers| {
                            headers.iter().any(|(name, value)| {
                                name.eq_ignore_ascii_case("authorization")
                                    && value.starts_with("Bearer ")
                            })
                        })
                    });
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"id":"msg_1","content":[{"type":"text","text":"pong"}]}"#);
            })
            .await;

        let url = serve(build_router(vec![azure_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-opus-4.8",
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("\"pong\""), "upstream body must pass through: {text}");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn foundry_claude_forwards_allowlisted_betas_as_header() {
        // Foundry takes the standard header; the allowlist still drops the OAuth
        // marker so the request is not rejected wholesale.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/anthropic/v1/messages")
                    .header("anthropic-beta", "computer-use-2025-01-24");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"id":"msg_1","content":[]}"#);
            })
            .await;

        let url = serve(build_router(vec![azure_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .header("anthropic-beta", "computer-use-2025-01-24, oauth-2025-04-20")
            .json(&json!({"model": "claude-opus-4.8", "max_tokens": 16, "messages": []}))
            .send()
            .await
            .expect("proxy request");
        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn rewrites_model_signs_and_returns_body() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/openai/v1/chat/completions")
                    // The gateway rewrote gpt-oss-20b to the upstream id and injected a credential.
                    .body_contains("openai.gpt-oss-20b-1:0")
                    .header_exists("authorization");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"id":"cmpl-1","choices":[{"message":{"content":"pong"}}]}"#);
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({"model":"gpt-oss-20b","messages":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("\"pong\""), "upstream body must pass through: {text}");
        // The mock only matches when the body carries the rewritten upstream id and an
        // Authorization header, so a hit proves the model rewrite and cred injection.
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn streams_sse_through_unchanged() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"po\"}}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\"ng\"}}]}\n\n\
                   data: [DONE]\n\n";
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST).path("/openai/v1/chat/completions");
                then.status(200)
                    .header("content-type", "text/event-stream")
                    .body(sse);
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({"model":"gpt-oss-20b","stream":true,"messages":[]}))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let body = resp.text().await.unwrap();
        assert_eq!(body, sse, "SSE must stream through byte-for-byte");
        mock.assert_async().await;
    }

    /// Build a single `vnd.amazon.eventstream` frame wrapping `event_json`, the way
    /// Bedrock's invoke-with-response-stream does: payload `{"bytes": base64(event)}`.
    /// Encoded with aws-smithy-eventstream so the CRCs are real — the decoder
    /// validates them.
    fn eventstream_frame(event_json: &str) -> Vec<u8> {
        let payload = format!(r#"{{"bytes":"{}"}}"#, STANDARD.encode(event_json));
        raw_payload_frame(&payload)
    }

    /// Build an event-stream frame whose payload is `payload` verbatim, with no
    /// `{"bytes": ...}` wrapper — the shape of a Bedrock mid-stream exception frame.
    fn raw_payload_frame(payload: &str) -> Vec<u8> {
        let message = Message::new(Bytes::from(payload.to_string()));
        let mut frame = Vec::new();
        write_message_to(&message, &mut frame).expect("encode test frame");
        frame
    }

    #[test]
    fn decoder_surfaces_bedrock_exception_frame_as_error() {
        // A Bedrock mid-stream exception frame's payload is the raw exception JSON,
        // NOT wrapped in {"bytes": ...}. It must surface as an Anthropic error event
        // rather than be dropped, which would truncate the reply under a 200.
        let mut decoder = EventStreamToSse::default();
        let out = decoder.push(&raw_payload_frame(r#"{"message":"Model stream timed out"}"#));
        assert!(out.contains("event: error"), "exception frame must surface an error: {out}");
        assert!(
            out.contains("Model stream timed out"),
            "the upstream error message must reach the client: {out}"
        );
    }

    #[test]
    fn decoder_emits_normal_chunk_then_surfaces_a_following_exception() {
        let mut decoder = EventStreamToSse::default();
        let mut bytes =
            eventstream_frame(r#"{"type":"content_block_delta","delta":{"text":"hi"}}"#);
        bytes.extend_from_slice(&raw_payload_frame(r#"{"message":"throttled"}"#));
        let out = decoder.push(&bytes);
        assert!(out.contains("event: content_block_delta"), "the normal delta must decode: {out}");
        assert!(
            out.contains("event: error") && out.contains("throttled"),
            "a trailing exception frame must still surface: {out}"
        );
    }

    #[test]
    fn decoder_fails_loud_on_desynced_frame() {
        // A prelude whose CRC does not match (here: an impossible declared length
        // with zeroed CRCs) can never be valid; the decoder must emit an error and
        // stop, not silently stall on undrainable bytes.
        let mut decoder = EventStreamToSse::default();
        let mut bytes = 8u32.to_be_bytes().to_vec(); // total=8 (<16): impossible
        bytes.extend_from_slice(&[0u8; 12]);
        let out = decoder.push(&bytes);
        assert!(out.contains("event: error"), "a desynced frame must surface an error: {out}");
        // A desync is unrecoverable, so further input is ignored rather than decoded
        // mid-stream as if nothing were wrong.
        let after = decoder.push(&eventstream_frame(r#"{"type":"message_stop"}"#));
        assert_eq!(after, "", "decoder must stop after a desync");
    }

    #[test]
    fn decoder_fails_loud_on_corrupted_frame() {
        // A bit-flip inside a valid frame fails the CRC check: the corruption must
        // surface as an error, not decode to garbage misattributed to Bedrock.
        let mut bytes = eventstream_frame(r#"{"type":"content_block_delta","delta":{"text":"hi"}}"#);
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xFF;
        let mut decoder = EventStreamToSse::default();
        let out = decoder.push(&bytes);
        assert!(out.contains("event: error"), "a corrupted frame must surface an error: {out}");
    }

    #[test]
    fn decoder_flushes_incomplete_trailing_frame_as_error() {
        // The upstream closed after only part of a frame arrived (a truncated stream);
        // finish() must surface a loud error rather than drop the buffered tail.
        let mut decoder = EventStreamToSse::default();
        let full = eventstream_frame(r#"{"type":"content_block_delta","delta":{"text":"hi"}}"#);
        let partial = &full[..full.len() - 5];
        assert_eq!(decoder.push(partial), "", "an incomplete frame emits nothing until it completes");
        let flushed = decoder.finish();
        assert!(
            flushed.contains("event: error"),
            "EOF with a buffered partial frame must surface an error: {flushed}"
        );
    }

    #[test]
    fn decoder_finish_is_silent_on_a_clean_boundary() {
        // Every frame consumed: finish() must NOT inject a spurious error event.
        let mut decoder = EventStreamToSse::default();
        let out = decoder.push(&eventstream_frame(r#"{"type":"message_stop"}"#));
        assert!(out.contains("event: message_stop"));
        assert_eq!(decoder.finish(), "", "a clean stream end must not emit an error");
    }

    #[test]
    fn ensure_block_content_normalizes_valid_shapes_and_rejects_the_rest() {
        // The same-role fold extends the previous message's content ARRAY, so
        // ensure_block_content must yield an array for the two valid shapes — and
        // fail loud on a malformed one instead of folding the turn into [].
        let mut s = json!({"role": "user", "content": "hi"});
        ensure_block_content(&mut s).expect("string content is valid");
        assert_eq!(s["content"], json!([{"type": "text", "text": "hi"}]));

        let mut arr = json!({"role": "user", "content": [{"type": "text", "text": "x"}]});
        ensure_block_content(&mut arr).expect("array content is valid");
        assert_eq!(arr["content"], json!([{"type": "text", "text": "x"}]));

        let mut missing = json!({"role": "user"});
        ensure_block_content(&mut missing).expect_err("missing content must be rejected");

        let mut object = json!({"role": "user", "content": {"type": "text", "text": "hi"}});
        ensure_block_content(&mut object).expect_err("object content must be rejected");
    }

    #[test]
    fn bedrock_geo_routes_non_us_regions_via_global() {
        assert_eq!(bedrock_geo("us-east-2"), "us");
        assert_eq!(bedrock_geo("us-west-2"), "us");
        assert_eq!(bedrock_geo("us-gov-west-1"), "us-gov");
        assert_eq!(bedrock_geo("eu-west-1"), "global");
        assert_eq!(bedrock_geo("ap-southeast-2"), "global");
        assert_eq!(bedrock_geo("ca-central-1"), "global");
        assert_eq!(bedrock_geo("sa-east-1"), "global");
        assert_eq!(bedrock_geo("mx-central-1"), "global");
    }

    #[tokio::test]
    async fn claude_streams_through_bedrock_invoke_as_sse() {
        // A Claude model on an AWS binding must route to classic InvokeModel — the
        // model as a geo inference profile in the URL, no model/stream in the body —
        // and the event-stream reply must be decoded back into Anthropic SSE.
        let event = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"pong"}}"#;
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream")
                    .body_contains("bedrock-2023-05-31")
                    .header_exists("authorization");
                then.status(200)
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .body(eventstream_frame(event));
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({"model":"claude-haiku-4.5","stream":true,"max_tokens":16,"messages":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("event: content_block_delta"),
            "event-stream must be decoded to Anthropic SSE: {body}"
        );
        assert!(body.contains(r#""text":"pong""#), "delta text must survive: {body}");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn claude_non_streaming_returns_json_through_invoke() {
        // Without stream, a Claude request must hit the classic InvokeModel `invoke`
        // suffix (not invoke-with-response-stream) and its JSON reply passes straight
        // through untouched — the event-stream decoder is only for the streaming path.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke")
                    .body_contains("bedrock-2023-05-31")
                    .header_exists("authorization");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"type":"message","content":[{"type":"text","text":"pong"}]}"#);
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-haiku-4.5",
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );
        let text = resp.text().await.unwrap();
        assert!(text.contains(r#""pong""#), "non-streaming JSON must pass through: {text}");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn claude_stream_truncated_midframe_surfaces_error_to_client() {
        // The upstream sends a complete HTTP body that ends mid event-stream frame
        // (a truncated stream). End to end, the client must receive an `event: error`
        // rather than a stream that just stops. This exercises the real
        // Body::from_stream + unfold finish() plumbing that the decoder unit tests
        // (which call finish() directly) do not cover.
        let full = eventstream_frame(r#"{"type":"content_block_delta","delta":{"text":"partial"}}"#);
        let truncated = full[..full.len() - 6].to_vec(); // an incomplete final frame
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST).path(
                    "/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream",
                );
                then.status(200)
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .body(truncated);
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({"model":"claude-haiku-4.5","stream":true,"max_tokens":16,"messages":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("event: error"),
            "a truncated upstream stream must surface an error to the client: {body}"
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn bedrock_path_drops_fields_bedrock_rejects() {
        // A latest Claude Code body carries newer Anthropic fields that Bedrock's
        // classic schema rejects; the gateway must strip them (the mock only matches,
        // and thus 200s, when they are absent from the upstream body).
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream")
                    .matches(|req: &HttpMockRequest| {
                        let body = req
                            .body
                            .as_deref()
                            .map(String::from_utf8_lossy)
                            .unwrap_or_default();
                        !body.contains("output_config")
                            && !body.contains("context_management")
                            && !body.contains("adaptive")
                    });
                then.status(200)
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .body(eventstream_frame(r#"{"type":"message_stop"}"#));
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-haiku-4.5",
                "stream": true,
                "max_tokens": 16,
                "output_config": {"effort": "xhigh"},
                "context_management": {"edits": []},
                "thinking": {"type": "adaptive"},
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn bedrock_path_drops_server_tools_it_cannot_host() {
        // Anthropic *server*-executed tools (advisor, web search) run on Anthropic's
        // API servers; Bedrock rejects their tags. Client tools and the
        // client-executed types Bedrock DOES host (text editor, computer use, …)
        // must survive untouched.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream")
                    .matches(|req: &HttpMockRequest| {
                        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap_or_default())
                            .unwrap_or_default();
                        let tools = body["tools"].as_array().cloned().unwrap_or_default();
                        // read_file + text_editor + computer survive; advisor + web_search drop.
                        tools.len() == 3
                            && tools[0]["name"] == "read_file"
                            // `defer_loading` stripped from the surviving client tool.
                            && tools[0].get("defer_loading").is_none()
                            && tools[1]["type"] == "text_editor_20250728"
                            && tools[2]["type"] == "computer_20250124"
                    });
                then.status(200)
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .body(eventstream_frame(r#"{"type":"message_stop"}"#));
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-haiku-4.5",
                "stream": true,
                "max_tokens": 16,
                "tools": [
                    {"name": "read_file", "description": "reads", "input_schema": {"type": "object"}, "defer_loading": true},
                    {"type": "text_editor_20250728", "name": "str_replace_based_edit_tool"},
                    {"type": "computer_20250124", "name": "computer"},
                    {"type": "advisor_20260301", "name": "advisor"},
                    {"type": "web_search_20250305", "name": "web_search"}
                ],
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn bedrock_path_bridges_the_anthropic_beta_header_into_the_body() {
        // Clients declare betas as the `anthropic-beta` HTTP header, but classic
        // InvokeModel reads only the body's `anthropic_beta`. Without the bridge, a
        // forwarded beta-gated tool (computer_*) reaches Bedrock with no beta and 400s.
        // A body-declared beta must survive alongside the bridged one, and header tags
        // outside BEDROCK_BETA_PREFIXES (`oauth-2025-04-20` is on every OAuth Claude
        // Code request; Bedrock rejects it as "invalid beta flag") must NOT be bridged.
        // The mock only matches — and so only 200s — when the kept betas and the tool
        // arrived and the rejected tags did not.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream")
                    .matches(|req: &HttpMockRequest| {
                        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap_or_default())
                            .unwrap_or_default();
                        let betas = body["anthropic_beta"].as_array().cloned().unwrap_or_default();
                        let tools = body["tools"].as_array().cloned().unwrap_or_default();
                        betas.iter().any(|b| b == "context-management-2025-06-27")
                            && betas.iter().any(|b| b == "computer-use-2025-01-24")
                            && !betas.iter().any(|b| b == "oauth-2025-04-20")
                            && !betas.iter().any(|b| b == "tool-search-2025-10-02")
                            && tools.len() == 1
                            && tools[0]["type"] == "computer_20250124"
                    });
                then.status(200)
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .body(eventstream_frame(r#"{"type":"message_stop"}"#));
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .header(
                "anthropic-beta",
                "computer-use-2025-01-24,oauth-2025-04-20,tool-search-2025-10-02",
            )
            .json(&json!({
                "model": "claude-haiku-4.5",
                "stream": true,
                "max_tokens": 16,
                "anthropic_beta": ["context-management-2025-06-27"],
                "tools": [{"type": "computer_20250124", "name": "computer"}],
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn bedrock_path_downgrades_system_role_messages() {
        // Claude Code (mid-conversation-system beta) puts `role:"system"` turns
        // inside `messages`; Bedrock's pinned schema allows only user/assistant
        // there and enforces alternation. The gateway must re-tag the turn as
        // `user` in place and fold it into its same-role neighbor.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream")
                    .matches(|req: &HttpMockRequest| {
                        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap_or_default())
                            .unwrap_or_default();
                        let messages = body["messages"].as_array().cloned().unwrap_or_default();
                        // One merged user turn: original user text + the downgraded
                        // system turn's text, in conversation order.
                        messages.len() == 1
                            && messages[0]["role"] == "user"
                            && messages[0]["content"][0]["text"] == "hi"
                            && messages[0]["content"][1]["text"] == "hook output"
                    });
                then.status(200)
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .body(eventstream_frame(r#"{"type":"message_stop"}"#));
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-haiku-4.5",
                "stream": true,
                "max_tokens": 16,
                "messages": [
                    {"role": "user", "content": "hi"},
                    {"role": "system", "content": "hook output"}
                ]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn bedrock_path_folds_tool_results_ahead_of_downgraded_system_text() {
        // A hook can emit a system turn between a tool call and its result:
        // [assistant(tool_use), system(text), user(tool_result)]. The fold merges
        // the downgraded system turn with the tool_result turn — and the result
        // block must lead the merged message (live-verified: Bedrock rejects
        // `[text, tool_result]` with "'tool_use' ids were found without
        // 'tool_result' blocks immediately after").
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream")
                    .matches(|req: &HttpMockRequest| {
                        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap_or_default())
                            .unwrap_or_default();
                        let messages = body["messages"].as_array().cloned().unwrap_or_default();
                        messages.len() == 3
                            && messages[2]["role"] == "user"
                            && messages[2]["content"][0]["type"] == "tool_result"
                            && messages[2]["content"][1]["text"] == "hook output"
                    });
                then.status(200)
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .body(eventstream_frame(r#"{"type":"message_stop"}"#));
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-haiku-4.5",
                "stream": true,
                "max_tokens": 16,
                "messages": [
                    {"role": "user", "content": "What time is it?"},
                    {"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_01", "name": "get_time", "input": {}}]},
                    {"role": "system", "content": "hook output"},
                    {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_01", "content": "12:00"}]}
                ]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn bedrock_path_drops_tool_choice_with_the_last_server_tool() {
        // When every declared tool is server-executed, stripping them leaves
        // `tools: []` plus a tool_choice forcing a tool that no longer exists —
        // both of which Bedrock rejects outright. The whole pair must go.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream")
                    .matches(|req: &HttpMockRequest| {
                        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap_or_default())
                            .unwrap_or_default();
                        body.get("tools").is_none() && body.get("tool_choice").is_none()
                    });
                then.status(200)
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .body(eventstream_frame(r#"{"type":"message_stop"}"#));
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-haiku-4.5",
                "stream": true,
                "max_tokens": 16,
                "tools": [{"type": "web_search_20250305", "name": "web_search"}],
                "tool_choice": {"type": "tool", "name": "web_search"},
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn bedrock_path_rejects_a_non_boolean_stream_flag() {
        // `stream` chooses between two upstream endpoints; a malformed value used
        // to be coerced to `false`, answering an SSE client with a JSON body it
        // reads as a hang. It must be a loud 400 instead.
        let server = MockServer::start_async().await;
        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({
                "model": "claude-haiku-4.5",
                "stream": "true",
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 400);
        let body = resp.text().await.expect("response body");
        assert!(
            body.contains("GATEWAY_INVALID_REQUEST"),
            "the 400 must be the gateway's own validation error: {body}"
        );
    }

    #[tokio::test]
    async fn bedrock_path_keeps_a_string_form_body_beta_alongside_header_betas() {
        // The body's `anthropic_beta` also takes a single string; merging header
        // betas used to overwrite that form entirely, silently dropping the beta
        // the client's tool depended on.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream")
                    .matches(|req: &HttpMockRequest| {
                        let body: Value = serde_json::from_slice(req.body.as_deref().unwrap_or_default())
                            .unwrap_or_default();
                        let betas = body["anthropic_beta"].as_array().cloned().unwrap_or_default();
                        betas.iter().any(|b| b == "context-management-2025-06-27")
                            && betas.iter().any(|b| b == "computer-use-2025-01-24")
                    });
                then.status(200)
                    .header("content-type", "application/vnd.amazon.eventstream")
                    .body(eventstream_frame(r#"{"type":"message_stop"}"#));
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .header("anthropic-beta", "computer-use-2025-01-24")
            .json(&json!({
                "model": "claude-haiku-4.5",
                "stream": true,
                "max_tokens": 16,
                "anthropic_beta": "context-management-2025-06-27",
                "messages": [{"role": "user", "content": "hi"}]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn responses_pass_through_to_mantle() {
        // Codex's /v1/responses must forward byte-for-byte to the mantle Responses
        // endpoint with the model id rewritten and a SigV4 credential attached, and
        // the Responses SSE must come back unchanged.
        let sse = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"po\"}\n\n\
                   data: {\"type\":\"response.completed\"}\n\n";
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/responses")
                    // Mantle's Responses id drops the chat endpoint's version suffix.
                    .body_contains("\"openai.gpt-oss-20b\"")
                    // The SigV4 credential must be scoped to the bedrock-mantle service,
                    // not plain bedrock: mantle rejects a signature scoped to the wrong
                    // service. The scope segment appears verbatim in the credential.
                    .matches(|req: &HttpMockRequest| {
                        req.headers.as_ref().is_some_and(|headers| {
                            headers.iter().any(|(name, value)| {
                                name.eq_ignore_ascii_case("authorization")
                                    && value.contains("/bedrock-mantle/")
                            })
                        })
                    });
                then.status(200)
                    .header("content-type", "text/event-stream")
                    .body(sse);
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/responses"))
            .json(&json!({"model":"gpt-oss-20b","stream":true,"input":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), sse, "Responses SSE must pass through byte-for-byte");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn claude_over_responses_is_404() {
        // Claude on mantle is Messages-only; a Claude id over /v1/responses must be
        // rejected by the gateway, not forwarded.
        let server = MockServer::start_async().await;
        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/responses"))
            .json(&json!({"model":"claude-haiku-4.5","input":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .expect("proxy request");
        assert_eq!(resp.status(), 404);
        // The mock upstream answers unmatched requests with its own 404, so the
        // status alone cannot prove the gateway rejected the model rather than
        // forwarding the request — the body must carry the gateway's error code.
        let body = resp.text().await.expect("response body");
        assert!(
            body.contains("GATEWAY_MODEL_NOT_AVAILABLE"),
            "the 404 must be the gateway's own rejection, not a forwarded upstream 404: {body}"
        );
    }

    #[tokio::test]
    async fn unknown_model_is_404() {
        let server = MockServer::start_async().await;
        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({"model":"not-a-real-model","messages":[]}))
            .send()
            .await
            .expect("proxy request");
        assert_eq!(resp.status(), 404);
        let body = resp.text().await.expect("response body");
        assert!(
            body.contains("GATEWAY_MODEL_NOT_AVAILABLE"),
            "the 404 must be the gateway's own rejection, not a forwarded upstream 404: {body}"
        );
    }

    #[tokio::test]
    async fn models_lists_the_clouds_catalog() {
        let url = serve(build_router(vec![aws_route("https://unused.example")])).await;
        let resp = reqwest::get(format!("{url}/llm/v1/models")).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["object"], "list");
        let ids: Vec<&str> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"gpt-oss-20b"), "AWS catalog must include gpt-oss-20b: {ids:?}");
        assert!(ids.contains(&"claude-opus-4.8"), "AWS catalog must include Claude: {ids:?}");
    }
}
