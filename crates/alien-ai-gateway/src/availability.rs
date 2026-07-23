//! Runtime model-availability filtering for `/v1/models`.
//!
//! The catalog (`ai_catalog`) is the static superset of what each cloud CAN serve.
//! What a specific deployment can ACTUALLY invoke depends on per-account/region
//! enablement (a Bedrock model-access grant, a Vertex Model Garden entitlement, an
//! Azure Foundry deployment) that no uniform cloud API reports. So we probe: a tiny
//! `max_tokens: 1` request per candidate model, signed with the workload's own
//! ambient credential, classified by status. This needs no permission beyond the
//! inference grant the workload already holds (`ai/invoke`).
//!
//! A 429 (rate-limit) or 400 (our minimal body rejected) both mean the endpoint
//! authed and routed the request, so the model is enabled; only 401/403/404 mean it
//! is off. Probing is lazy (first `/v1/models` per binding) and cached, so it never
//! gates the gateway bind. Fail-open by design: `available_models` never returns
//! an error and never fails a deploy. A model that cannot be probed conclusively
//! stays listed (never worse than the old static catalog) and the result is left
//! uncached so the next call re-probes.

use alien_core::{
    ai_catalog::{self, CatalogModel, Protocol},
    Platform,
};
use alien_error::AlienError;
use serde_json::json;
use tracing::{debug, warn};

use crate::error::{ErrorData, Result};
use crate::router::{
    bedrock_geo, missing_field, sign_and_execute, upstream_target, vertex_host, GatewayRoute,
    FOUNDRY_ANTHROPIC_VERSION,
};

/// The outcome of probing one model.
enum Availability {
    /// Reached and authed (2xx, or a 429 rate-limit, or a 400 body rejection).
    Available,
    /// Definitively off: an auth/entitlement/not-found status (401/403/404).
    Unavailable,
    /// Could not tell (transport error, 5xx, or the route lacked a field to build
    /// the probe). Kept in the list for this response, but the result is not cached.
    Indeterminate,
}

/// The filtered set plus whether every probe reached a definite verdict. When not
/// fully resolved, the caller must not cache the result (a transient error must not
/// stick a diminished list until redeploy).
pub(crate) struct ProbeResult {
    pub models: Vec<&'static CatalogModel>,
    pub fully_resolved: bool,
}

/// Classify an upstream HTTP status. See the module doc for why 429/400 are
/// "available": both prove the request authed and routed to a real model.
fn classify_status(code: u16) -> Availability {
    match code {
        200..=299 | 400 | 429 => Availability::Available,
        401 | 403 | 404 => Availability::Unavailable,
        _ => Availability::Indeterminate,
    }
}

/// Probe every catalog model for the route's cloud concurrently and keep the
/// enabled ones (plus any that could not be judged). Never errors.
pub(crate) async fn available_models(
    route: &GatewayRoute,
    client: &reqwest::Client,
) -> ProbeResult {
    // Probe every candidate concurrently. join_all preserves input order, so the
    // list stays in catalog order across calls.
    let candidates = ai_catalog::models_for(route.cloud);
    let probes: Vec<_> = candidates
        .iter()
        .copied()
        .map(|cm| async move { (cm, probe_model(route, client, cm).await) })
        .collect();
    let verdicts = futures::future::join_all(probes).await;

    let mut models = Vec::new();
    let mut fully_resolved = true;
    for (cm, verdict) in verdicts {
        match verdict {
            Availability::Available => models.push(cm),
            Availability::Unavailable => {
                debug!(model = cm.public_id, cloud = ?route.cloud, "model not enabled, dropping");
            }
            Availability::Indeterminate => {
                warn!(model = cm.public_id, cloud = ?route.cloud, "availability undetermined; keeping the model listed and leaving the result uncached");
                models.push(cm);
                fully_resolved = false;
            }
        }
    }
    ProbeResult { models, fully_resolved }
}

/// Send one `max_tokens: 1` request to the model's native endpoint and classify the
/// status. Any failure to build or send the probe is `Indeterminate`, never a panic.
async fn probe_model(
    route: &GatewayRoute,
    client: &reqwest::Client,
    cm: &CatalogModel,
) -> Availability {
    let built = match cm.protocol {
        Protocol::OpenAi => openai_probe(route, cm),
        Protocol::Anthropic => anthropic_probe(route, cm),
    };
    let (url, service, body, extra_headers) = match built {
        Ok(probe) => probe,
        Err(error) => {
            debug!(model = cm.public_id, %error, "could not build the availability probe");
            return Availability::Indeterminate;
        }
    };
    let header_refs: Vec<(&str, &str)> =
        extra_headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    match sign_and_execute(client, &route.cred, &url, service, body, &header_refs).await {
        Ok(resp) => classify_status(resp.status().as_u16()),
        Err(error) => {
            debug!(model = cm.public_id, %error, "availability probe did not reach the upstream");
            Availability::Indeterminate
        }
    }
}

/// A minimal Chat Completions probe body: one user turn, one output token.
fn openai_body(upstream_id: &str) -> Vec<u8> {
    json!({
        "model": upstream_id,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": "ping" }],
    })
    .to_string()
    .into_bytes()
}

/// A minimal Anthropic Messages probe body for a given wire-version marker.
fn anthropic_body(version: &str) -> Vec<u8> {
    json!({
        "anthropic_version": version,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": "ping" }],
    })
    .to_string()
    .into_bytes()
}

type Probe = (String, &'static str, Vec<u8>, Vec<(&'static str, String)>);

fn openai_probe(route: &GatewayRoute, cm: &CatalogModel) -> Result<Probe> {
    let (url, service) = upstream_target(route, Protocol::OpenAi)?;
    Ok((url, service, openai_body(cm.upstream_id), Vec::new()))
}

/// Build the same per-cloud Claude endpoint the proxy uses (Bedrock InvokeModel /
/// Vertex rawPredict / Foundry Anthropic), with a minimal body.
fn anthropic_probe(route: &GatewayRoute, cm: &CatalogModel) -> Result<Probe> {
    match route.cloud {
        Platform::Aws => {
            let region = route.region.as_deref().ok_or_else(|| missing_field(route, "region"))?;
            let base = route
                .upstream_base_override
                .clone()
                .unwrap_or_else(|| format!("https://bedrock-runtime.{region}.amazonaws.com"));
            let url = format!(
                "{}/model/{}.{}/invoke",
                base.trim_end_matches('/'),
                bedrock_geo(region),
                cm.upstream_id
            );
            Ok((url, "bedrock", anthropic_body("bedrock-2023-05-31"), Vec::new()))
        }
        Platform::Gcp => {
            let location = route.region.as_deref().ok_or_else(|| missing_field(route, "location"))?;
            let project = route.project.as_deref().ok_or_else(|| missing_field(route, "project"))?;
            let base =
                route.upstream_base_override.clone().unwrap_or_else(|| vertex_host(location));
            let url = format!(
                "{}/v1/projects/{project}/locations/{location}/publishers/anthropic/models/{}:rawPredict",
                base.trim_end_matches('/'),
                cm.upstream_id
            );
            Ok((url, "", anthropic_body("vertex-2023-10-16"), Vec::new()))
        }
        Platform::Azure => {
            let endpoint =
                route.azure_endpoint.as_deref().ok_or_else(|| missing_field(route, "endpoint"))?;
            let base =
                route.upstream_base_override.clone().unwrap_or_else(|| endpoint.to_string());
            let url = format!("{}/anthropic/v1/messages", base.trim_end_matches('/'));
            let body = json!({
                "model": cm.upstream_id,
                "max_tokens": 1,
                "messages": [{ "role": "user", "content": "ping" }],
            })
            .to_string()
            .into_bytes();
            Ok((url, "", body, vec![("anthropic-version", FOUNDRY_ANTHROPIC_VERSION.to_string())]))
        }
        cloud => Err(AlienError::new(ErrorData::Other {
            message: format!("{cloud:?} does not serve the Anthropic protocol"),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_treats_429_and_400_as_available() {
        // 2xx, plus the two "reached and authed" statuses.
        assert!(matches!(classify_status(200), Availability::Available));
        assert!(matches!(classify_status(429), Availability::Available));
        assert!(matches!(classify_status(400), Availability::Available));
        // Auth / entitlement / not-found: definitively off.
        assert!(matches!(classify_status(401), Availability::Unavailable));
        assert!(matches!(classify_status(403), Availability::Unavailable));
        assert!(matches!(classify_status(404), Availability::Unavailable));
        // Anything else: can't tell.
        assert!(matches!(classify_status(500), Availability::Indeterminate));
        assert!(matches!(classify_status(503), Availability::Indeterminate));
    }
}
