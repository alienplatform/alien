//! GCP Vertex AI: serve Claude through `rawPredict` / `streamRawPredict`, the native
//! Anthropic Messages API with the model id in the URL and Vertex's version marker.

use alien_error::{AlienError, Context, IntoAlienError};
use axum::http::HeaderMap;
use axum::response::Response;
use serde_json::{json, Value};

use super::bedrock::filtered_header_betas;
use super::{forward_response, missing_field, parse_stream_flag, sign_and_execute, GatewayRoute};
use crate::error::{ErrorData, Result};

/// The Vertex AI Platform host for a location: the global endpoint is the
/// un-prefixed host; a region prefixes it. The path carries `locations/{location}`
/// either way.
pub(crate) fn vertex_host(location: &str) -> String {
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
pub(crate) async fn proxy_vertex_anthropic(
    client: &reqwest::Client,
    route: &GatewayRoute,
    upstream_id: &str,
    mut payload: Value,
    headers: &HeaderMap,
) -> Result<Response> {
    let location = route
        .region
        .as_deref()
        .ok_or_else(|| missing_field(route, "location"))?;
    let project = route
        .project
        .as_deref()
        .ok_or_else(|| missing_field(route, "project"))?;

    let obj = payload.as_object_mut().ok_or_else(|| {
        AlienError::new(ErrorData::InvalidRequest {
            message: "request body must be a JSON object".to_string(),
        })
    })?;
    obj.remove("model");
    obj.insert("anthropic_version".to_string(), json!("vertex-2023-10-16"));
    // The `stream` field stays in the body; Vertex accepts it alongside the verb.
    let stream = parse_stream_flag(obj.get("stream").cloned())?;
    let verb = if stream {
        "streamRawPredict"
    } else {
        "rawPredict"
    };

    let base = route
        .upstream_base_override
        .clone()
        .unwrap_or_else(|| vertex_host(location));
    let url = format!(
        "{}/v1/projects/{project}/locations/{location}/publishers/anthropic/models/{upstream_id}:{verb}",
        base.trim_end_matches('/')
    );

    let upstream_body =
        serde_json::to_vec(&payload)
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
    let upstream = sign_and_execute(client, route, &url, "", upstream_body, &extra_headers).await?;
    forward_response(upstream).await
}

/// Inputs one embeddings request may carry. Vertex's `gemini-embedding-001` takes a single input
/// per `:predict` call, so each input is its own upstream call; the bound keeps one request from
/// fanning out without limit.
const MAX_EMBEDDING_INPUTS: usize = 256;
/// Upstream calls in flight at once for one embeddings request.
const EMBEDDING_CONCURRENCY: usize = 8;

/// The texts an OpenAI embeddings `input` names: a string or an array of strings. Token arrays
/// are refused, since the model reads text.
fn embedding_inputs(payload: &Value) -> Result<Vec<String>> {
    let refuse = |message: &str| {
        AlienError::new(ErrorData::InvalidRequest {
            message: message.to_string(),
        })
    };
    let inputs: Vec<String> = match payload.get("input") {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string))
            .collect::<Option<_>>()
            .ok_or_else(|| refuse("`input` must be a string or an array of strings"))?,
        _ => {
            return Err(refuse(
                "request body has no `input` string or array of strings",
            ))
        }
    };
    if inputs.is_empty() {
        return Err(refuse("`input` must not be empty"));
    }
    if inputs.len() > MAX_EMBEDDING_INPUTS {
        return Err(refuse(&format!(
            "`input` holds {} items; this model takes at most {MAX_EMBEDDING_INPUTS} per request",
            inputs.len()
        )));
    }
    Ok(inputs)
}

/// Serve an OpenAI embeddings request through the Vertex publisher model's `:predict`, answering in
/// the OpenAI shape: `data[i].embedding` per input, and `usage` from the model's own token counts,
/// so usage is reported as it is for every other embeddings reply. `dimensions` becomes
/// `outputDimensionality`.
pub(crate) async fn proxy_vertex_embeddings(
    client: &reqwest::Client,
    route: &GatewayRoute,
    upstream_id: &str,
    requested_model: &str,
    payload: &Value,
) -> Result<Response> {
    use axum::response::IntoResponse;
    use futures::{stream, StreamExt};

    let location = route
        .region
        .as_deref()
        .ok_or_else(|| missing_field(route, "location"))?;
    let project = route
        .project
        .as_deref()
        .ok_or_else(|| missing_field(route, "project"))?;
    let inputs = embedding_inputs(payload)?;
    if let Some(format) = payload.get("encoding_format").and_then(Value::as_str) {
        if format != "float" {
            return Err(AlienError::new(ErrorData::InvalidRequest {
                message: "this model returns `float` embeddings only".to_string(),
            }));
        }
    }
    let dimensions = match payload.get("dimensions") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().filter(|d| *d > 0).ok_or_else(|| {
            AlienError::new(ErrorData::InvalidRequest {
                message: "`dimensions` must be a positive integer".to_string(),
            })
        })?),
    };

    let base = route
        .upstream_base_override
        .clone()
        .unwrap_or_else(|| vertex_host(location));
    let url = format!(
        "{}/v1/projects/{project}/locations/{location}/publishers/google/models/{upstream_id}:predict",
        base.trim_end_matches('/')
    );

    let calls = inputs.into_iter().map(|text| {
        let url = url.clone();
        async move {
            let mut body = json!({ "instances": [{ "content": text }] });
            if let Some(dimensions) = dimensions {
                body["parameters"] = json!({ "outputDimensionality": dimensions });
            }
            let body = serde_json::to_vec(&body)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "could not serialize the embeddings request".to_string(),
                })?;
            sign_and_execute(client, route, &url, "", body, &[]).await
        }
    });
    // In order, so `index` is the input's position.
    let replies: Vec<Result<reqwest::Response>> = stream::iter(calls)
        .buffered(EMBEDDING_CONCURRENCY)
        .collect()
        .await;

    let mut data = Vec::with_capacity(replies.len());
    let mut tokens: u64 = 0;
    for (index, reply) in replies.into_iter().enumerate() {
        let upstream = reply?;
        if !upstream.status().is_success() {
            // The first refusal is the answer, mapped like every other upstream error.
            return forward_response(upstream).await;
        }
        let reply: Value =
            upstream
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::UpstreamFailed {
                    message: "the embeddings provider returned a reply that is not JSON"
                        .to_string(),
                })?;
        let embeddings = reply.pointer("/predictions/0/embeddings").ok_or_else(|| {
            AlienError::new(ErrorData::UpstreamFailed {
                message: "the embeddings reply has no `predictions[0].embeddings`".to_string(),
            })
        })?;
        let values = embeddings.get("values").cloned().ok_or_else(|| {
            AlienError::new(ErrorData::UpstreamFailed {
                message: "the embeddings reply has no `values`".to_string(),
            })
        })?;
        tokens += embeddings
            .pointer("/statistics/token_count")
            .and_then(|count| count.as_f64())
            .map(|count| count as u64)
            .unwrap_or(0);
        data.push(json!({ "object": "embedding", "index": index, "embedding": values }));
    }

    Ok(axum::Json(json!({
        "object": "list",
        "data": data,
        "model": requested_model,
        "usage": { "prompt_tokens": tokens, "total_tokens": tokens },
    }))
    .into_response())
}
