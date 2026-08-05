//! Azure Foundry: serve Claude through the `/anthropic/v1` endpoint — the native
//! Anthropic Messages API with the model in the body and a version header.

use alien_error::{Context, IntoAlienError};
use axum::http::HeaderMap;
use axum::response::Response;
use serde_json::Value;

use super::bedrock::filtered_header_betas;
use super::{forward_response, missing_field, sign_and_execute, GatewayRoute};
use crate::error::{ErrorData, Result};

/// The Messages API version the gateway bridges to Foundry's Anthropic endpoint;
/// Foundry reads it from the standard `anthropic-version` header.
pub(crate) const FOUNDRY_ANTHROPIC_VERSION: &str = "2023-06-01";

/// Serve a Claude request through Foundry's Anthropic endpoint. The closest arm to
/// the plain passthrough: the model stays in the body (rewritten to the Foundry
/// deployment name), streaming is the standard body field, and the reply is native
/// Anthropic JSON/SSE — only the version header and the `/anthropic/v1` path
/// distinguish it from the OpenAI arm.
pub(crate) async fn proxy_foundry_anthropic(
    client: &reqwest::Client,
    route: &GatewayRoute,
    upstream_id: &str,
    mut payload: Value,
    headers: &HeaderMap,
) -> Result<Response> {
    let endpoint = route
        .azure_endpoint
        .as_deref()
        .ok_or_else(|| missing_field(route, "endpoint"))?;

    payload["model"] = Value::String(upstream_id.to_string());
    let upstream_body =
        serde_json::to_vec(&payload)
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
