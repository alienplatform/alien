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
//! A 429 (rate-limit) means the endpoint authed and routed the request, so the model
//! is enabled and merely throttled. A 400 does NOT: Bedrock answers "The provided
//! model identifier is invalid" with 400 for a model the account cannot address, so
//! counting 400 as enabled lists models that then fail on the first real call. The
//! probe body is minimal and well formed, so a 400 is about the model, not the body.
//! 400/401/403/404 therefore all mean the model is off. Probing is lazy (first
//! `/v1/models` per binding) and cached, so it never
//! gates the gateway bind. Fail-open by design: `available_models` never returns
//! an error and never fails a deploy. A model that cannot be probed conclusively
//! stays listed (never worse than the old static catalog) and the result is left
//! uncached so the next call re-probes.

use std::time::Duration;

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

/// `join_all` below waits for every probe and the HTTP client has no timeout of its
/// own, so an upstream that accepts the connection and never answers would hang
/// `/v1/models` for the process lifetime.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// The outcome of probing one model.
enum Availability {
    /// Reached, authed, and served (2xx, or a 429 rate-limit).
    Available,
    /// Definitively off: rejected the model or the caller (400/401/403/404).
    Unavailable,
    /// Could not tell (transport error, timeout, 5xx, or the route lacked a field to
    /// build the probe). Kept in the list for this response, but not cached.
    Indeterminate,
}

/// The filtered set plus whether every probe reached a definite verdict. When not
/// fully resolved, the caller must not cache the result (a transient error must not
/// stick a diminished list until redeploy).
pub(crate) struct ProbeResult {
    pub models: Vec<&'static CatalogModel>,
    pub fully_resolved: bool,
}

/// Classify an upstream HTTP status. See the module doc for why 429 is "available"
/// and why 400 is not.
fn classify_status(code: u16) -> Availability {
    match code {
        200..=299 | 429 => Availability::Available,
        400 | 401 | 403 | 404 => Availability::Unavailable,
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
        .map(|cm| async move { (cm, probe_model(route, client, cm, PROBE_TIMEOUT).await) })
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
    ProbeResult {
        models,
        fully_resolved,
    }
}

/// Send one `max_tokens: 1` request to the model's native endpoint and classify the
/// status. Any failure to build or send the probe is `Indeterminate`, never a panic.
async fn probe_model(
    route: &GatewayRoute,
    client: &reqwest::Client,
    cm: &CatalogModel,
    timeout: Duration,
) -> Availability {
    let built = match cm.protocol {
        Protocol::OpenAi => openai_probe(route, cm),
        Protocol::Anthropic => anthropic_probe(route, cm),
        Protocol::OpenAiResponses => responses_probe(route, cm),
    };
    let (url, service, body, extra_headers) = match built {
        Ok(probe) => probe,
        Err(error) => {
            debug!(model = cm.public_id, %error, "could not build the availability probe");
            return Availability::Indeterminate;
        }
    };
    let header_refs: Vec<(&str, &str)> = extra_headers
        .iter()
        .map(|(k, v)| (*k, v.as_str()))
        .collect();
    let sent = sign_and_execute(client, &route.cred, &url, service, body, &header_refs);
    match tokio::time::timeout(timeout, sent).await {
        Ok(Ok(resp)) => classify_status(resp.status().as_u16()),
        Ok(Err(error)) => {
            debug!(model = cm.public_id, %error, "availability probe did not reach the upstream");
            Availability::Indeterminate
        }
        Err(_) => {
            debug!(model = cm.public_id, "availability probe timed out");
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

/// The same mantle endpoint `proxy_responses` forwards to, with a minimal body.
/// `max_output_tokens` is the API floor rather than 1: below it the request is a
/// 400, which classifies as unavailable and would hide a live model.
fn responses_probe(route: &GatewayRoute, cm: &CatalogModel) -> Result<Probe> {
    if route.cloud != Platform::Aws {
        return Err(missing_field(route, "AWS cloud"));
    }
    let target = ai_catalog::responses_target(cm.public_id)
        .ok_or_else(|| missing_field(route, "Responses endpoint"))?;
    let region = route
        .region
        .as_deref()
        .ok_or_else(|| missing_field(route, "region"))?;
    let base = route
        .upstream_base_override
        .clone()
        .unwrap_or_else(|| format!("https://bedrock-mantle.{region}.api.aws"));
    let body = json!({
        "model": target.upstream_id,
        "input": "ping",
        "max_output_tokens": 16,
    })
    .to_string()
    .into_bytes();
    Ok((
        format!("{}{}", base.trim_end_matches('/'), target.path),
        "bedrock-mantle",
        body,
        Vec::new(),
    ))
}

/// Build the same per-cloud Claude endpoint the proxy uses (Bedrock InvokeModel /
/// Vertex rawPredict / Foundry Anthropic), with a minimal body.
fn anthropic_probe(route: &GatewayRoute, cm: &CatalogModel) -> Result<Probe> {
    match route.cloud {
        Platform::Aws => {
            let region = route
                .region
                .as_deref()
                .ok_or_else(|| missing_field(route, "region"))?;
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
            Ok((
                url,
                "bedrock",
                anthropic_body("bedrock-2023-05-31"),
                Vec::new(),
            ))
        }
        Platform::Gcp => {
            let location = route
                .region
                .as_deref()
                .ok_or_else(|| missing_field(route, "location"))?;
            let project = route
                .project
                .as_deref()
                .ok_or_else(|| missing_field(route, "project"))?;
            let base = route
                .upstream_base_override
                .clone()
                .unwrap_or_else(|| vertex_host(location));
            let url = format!(
                "{}/v1/projects/{project}/locations/{location}/publishers/anthropic/models/{}:rawPredict",
                base.trim_end_matches('/'),
                cm.upstream_id
            );
            Ok((url, "", anthropic_body("vertex-2023-10-16"), Vec::new()))
        }
        Platform::Azure => {
            let endpoint = route
                .azure_endpoint
                .as_deref()
                .ok_or_else(|| missing_field(route, "endpoint"))?;
            let base = route
                .upstream_base_override
                .clone()
                .unwrap_or_else(|| endpoint.to_string());
            let url = format!("{}/anthropic/v1/messages", base.trim_end_matches('/'));
            let body = json!({
                "model": cm.upstream_id,
                "max_tokens": 1,
                "messages": [{ "role": "user", "content": "ping" }],
            })
            .to_string()
            .into_bytes();
            Ok((
                url,
                "",
                body,
                vec![("anthropic-version", FOUNDRY_ANTHROPIC_VERSION.to_string())],
            ))
        }
        cloud => Err(AlienError::new(ErrorData::Other {
            message: format!("{cloud:?} does not serve the Anthropic protocol"),
        })),
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::time::Instant;

    use aws_credential_types::provider::SharedCredentialsProvider;
    use aws_credential_types::Credentials;

    use super::*;
    use crate::creds::{AmbientCred, AwsSigV4Cred};

    /// An upstream that accepts the connection and never writes a byte.
    async fn silent_upstream() -> String {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind the silent upstream");
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            // Hold every socket: dropping one closes the connection, which the probe
            // reads as a transport error rather than the timeout this test pins.
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        url
    }

    #[tokio::test]
    async fn a_silent_upstream_times_out_instead_of_hanging_the_probe() {
        let route = GatewayRoute {
            name: "llm".to_string(),
            cloud: Platform::Aws,
            region: Some("us-east-1".to_string()),
            project: None,
            azure_endpoint: None,
            cred: AmbientCred::Aws(AwsSigV4Cred::with_provider(
                "us-east-1",
                SharedCredentialsProvider::new(Credentials::new(
                    "AKIAIOSFODNN7EXAMPLE",
                    "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                    None,
                    None,
                    "test",
                )),
            )),
            upstream_base_override: Some(silent_upstream().await),
        };
        let cm = ai_catalog::lookup("gpt-oss-20b").expect("a known OpenAI-protocol model");

        let started = Instant::now();
        let verdict = probe_model(
            &route,
            &reqwest::Client::new(),
            cm,
            Duration::from_millis(200),
        )
        .await;

        assert!(
            matches!(verdict, Availability::Indeterminate),
            "a probe that never gets an answer cannot judge the model, so it stays listed"
        );
        // Every other path to Indeterminate — a route that won't build, a refused
        // connection, a 5xx — returns well inside the deadline, so the lower bound is
        // what pins this to the timeout.
        assert!(
            started.elapsed() >= Duration::from_millis(200),
            "the probe returned in {:?}, before the timeout could fire, so it took some other path",
            started.elapsed()
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the probe returned after {:?}, so the timeout is not bounding it",
            started.elapsed()
        );
    }

    #[test]
    fn classify_treats_429_as_available_and_400_as_off() {
        // 2xx, plus the throttled-but-enabled case.
        assert!(matches!(classify_status(200), Availability::Available));
        assert!(matches!(classify_status(429), Availability::Available));
        // 400 is the model, not the body: Bedrock returns it as "The provided model
        // identifier is invalid", and listing those breaks the invocable contract.
        assert!(matches!(classify_status(400), Availability::Unavailable));
        // Auth / entitlement / not-found: definitively off.
        assert!(matches!(classify_status(401), Availability::Unavailable));
        assert!(matches!(classify_status(403), Availability::Unavailable));
        assert!(matches!(classify_status(404), Availability::Unavailable));
        // Anything else: can't tell.
        assert!(matches!(classify_status(500), Availability::Indeterminate));
        assert!(matches!(classify_status(503), Availability::Indeterminate));
    }
}
