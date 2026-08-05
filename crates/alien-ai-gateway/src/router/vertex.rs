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
    let upstream =
        sign_and_execute(client, &route.cred, &url, "", upstream_body, &extra_headers).await?;
    forward_response(upstream)
}
