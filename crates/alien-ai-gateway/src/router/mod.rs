//! The pure proxy: route a request to the model's native cloud endpoint, inject the
//! workload's ambient credential, and stream the response back without translating
//! the body. The only edit to the request body is rewriting the public model id to
//! the catalog's upstream id. Successful JSON/SSE responses stream through
//! byte-for-byte; provider error bodies are captured for usage diagnostics and replaced with a
//! stable safe error for the caller.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alien_core::ai_catalog::{self, ClientApi, ProviderApi};
use alien_core::Platform;
use alien_error::{AlienError, Context, IntoAlienError};
use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{DefaultBodyLimit, Extension, FromRequest, Path, Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use futures::StreamExt;
use serde_json::{json, Value};

use crate::creds::{AmbientCred, AnthropicApiKeyCred, OpenAiApiKeyCred};
use crate::error::{ErrorData, Result};
use crate::protocol::{translate_request, translate_response, SseTranslator, WireProtocol};
use crate::usage::{
    observe_gateway_error, observe_response, AiGatewayRequestTiming, AiUsageClientApi,
    AiUsageContext, AiUsageObserver, AiUsageProvider, ProviderErrorBody,
};

mod bedrock;
mod eventstream;
mod foundry;
mod vertex;

use bedrock::proxy_bedrock_anthropic;
use foundry::proxy_foundry_anthropic;
use vertex::proxy_vertex_anthropic;

// Re-exported so availability.rs and the test module below can resolve these per-provider
// items. `vertex_host` is also used by `upstream_target` here; `ensure_block_content` and
// `EventStreamToSse` are test-only.
#[cfg(test)]
pub(crate) use bedrock::{bedrock_geo, ensure_block_content};
#[cfg(test)]
pub(crate) use eventstream::EventStreamToSse;
pub(crate) use vertex::vertex_host;

/// Clears the largest upstream request limit we serve (Bedrock `InvokeModel`, 25 MB) so the
/// upstream still owns rejecting its own oversized payloads, while keeping the buffer finite.
/// It bounds one request, not total memory: parsing then re-serializing then SigV4-hashing a
/// body costs several multiples of it, and nothing here caps concurrency.
const MAX_REQUEST_BODY: usize = 32 * 1024 * 1024;

/// Bounds provider error bodies retained for customer diagnostics.
const MAX_PROVIDER_ERROR_BODY: usize = 64 * 1024;

/// Surfaces an oversized body as this crate's structured error. axum's own rejection is bare
/// plain text, which would make this the one gateway failure a caller cannot parse as JSON.
struct ProxyBody(Bytes);

impl<S> FromRequest<S> for ProxyBody
where
    S: Send + Sync,
{
    type Rejection = AlienError<ErrorData>;

    async fn from_request(req: Request, state: &S) -> std::result::Result<Self, Self::Rejection> {
        match Bytes::from_request(req, state).await {
            Ok(body) => Ok(Self(body)),
            Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                Err(AlienError::new(ErrorData::RequestTooLarge {
                    limit_bytes: MAX_REQUEST_BODY,
                }))
            }
            Err(rejection) => Err(AlienError::new(ErrorData::InvalidRequest {
                message: rejection.body_text(),
            })),
        }
    }
}

/// One binding resolved into everything the proxy needs to serve it: the cloud (for
/// catalog filtering and upstream selection), the location fields used to build the
/// upstream URL, and the ambient credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayTarget {
    Cloud(Platform),
    DirectAnthropic,
    DirectDatabricks,
    DirectOpenAi,
}

pub struct GatewayRoute {
    /// The binding name — the first path segment the app calls (`/<name>/...`).
    pub name: String,
    pub target: GatewayTarget,
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
    /// Validated static headers attached to every upstream request.
    ///
    /// The inner map is intentionally opaque so callers cannot bypass the
    /// gateway-owned header checks in [`GatewayRoute::with_additional_headers`].
    pub additional_headers: AdditionalHeaders,
}

#[derive(Default)]
pub struct AdditionalHeaders(HeaderMap);

impl GatewayRoute {
    /// Attach validated static headers to every upstream request on this route.
    pub fn with_additional_headers(
        mut self,
        headers: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self> {
        for (name, value) in headers {
            let normalized = name.to_ascii_lowercase();
            if is_reserved_additional_header(&normalized) {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding: self.name,
                    message: format!("header `{name}` is managed by the AI gateway"),
                }));
            }
            let name = HeaderName::from_bytes(normalized.as_bytes())
                .into_alien_error()
                .context(ErrorData::BindingConfigInvalid {
                    binding: self.name.clone(),
                    message: format!("header name `{name}` is invalid"),
                })?;
            let value = HeaderValue::from_str(&value).into_alien_error().context(
                ErrorData::BindingConfigInvalid {
                    binding: self.name.clone(),
                    message: format!("header `{name}` has an invalid value"),
                },
            )?;
            self.additional_headers.0.insert(name, value);
        }
        Ok(self)
    }
}

fn is_reserved_additional_header(name: &str) -> bool {
    matches!(
        name,
        "accept"
            | "authorization"
            | "connection"
            | "content-length"
            | "content-type"
            | "cookie"
            | "host"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "x-api-key"
            | "api-key"
            | "anthropic-version"
            | "anthropic-beta"
            | "x-amz-content-sha256"
            | "x-amz-date"
            | "x-amz-security-token"
            | "x-amz-target"
            | "x-goog-api-key"
    ) || name.starts_with("x-forwarded-")
        || name.starts_with("x-alien-")
}

/// Build the only static-key route supported by the gateway. The provider and
/// host are fixed here; callers cannot turn an encrypted binding into a generic
/// credential-forwarding proxy.
pub fn route_from_direct_anthropic(
    name: impl Into<String>,
    api_key: impl Into<String>,
) -> Result<GatewayRoute> {
    Ok(GatewayRoute {
        name: name.into(),
        target: GatewayTarget::DirectAnthropic,
        region: None,
        project: None,
        azure_endpoint: None,
        cred: AmbientCred::AnthropicApiKey(AnthropicApiKeyCred::new(api_key)?),
        upstream_base_override: None,
        additional_headers: AdditionalHeaders::default(),
    })
}

/// Build the fixed-host OpenAI static-key route. Keeping this separate from a
/// generic bearer route prevents a stored provider key from being forwarded to
/// a caller-controlled host.
pub fn route_from_direct_openai(
    name: impl Into<String>,
    api_key: impl Into<String>,
) -> Result<GatewayRoute> {
    Ok(GatewayRoute {
        name: name.into(),
        target: GatewayTarget::DirectOpenAi,
        region: None,
        project: None,
        azure_endpoint: None,
        cred: AmbientCred::OpenAiApiKey(OpenAiApiKeyCred::new(api_key)?),
        upstream_base_override: None,
        additional_headers: AdditionalHeaders::default(),
    })
}

/// Build a Databricks Unity AI Gateway route for one validated workspace.
///
/// Databricks uses an OpenAI-compatible request and bearer credential, but its
/// upstream host belongs to the customer's workspace. Only canonical Databricks
/// workspace hosts are accepted so this cannot become a generic bearer-token proxy.
pub fn route_from_direct_databricks(
    name: impl Into<String>,
    workspace_url: &str,
    access_token: impl Into<String>,
) -> Result<GatewayRoute> {
    let name = name.into();
    let mut upstream = reqwest::Url::parse(workspace_url)
        .into_alien_error()
        .context(ErrorData::BindingConfigInvalid {
            binding: name.clone(),
            message: "the Databricks workspace URL is invalid".to_string(),
        })?;
    let route_name = upstream.host_str().map(str::to_string).ok_or_else(|| {
        AlienError::new(ErrorData::BindingConfigInvalid {
            binding: name.clone(),
            message: "the Databricks workspace URL has no host".to_string(),
        })
    })?;
    let valid_host = [
        ".cloud.databricks.com",
        ".azuredatabricks.net",
        ".gcp.databricks.com",
    ]
    .iter()
    .any(|suffix| route_name.ends_with(suffix));
    if upstream.scheme() != "https"
        || !valid_host
        || upstream.port().is_some()
        || !upstream.username().is_empty()
        || upstream.password().is_some()
        || upstream.query().is_some()
        || upstream.fragment().is_some()
    {
        return Err(AlienError::new(ErrorData::BindingConfigInvalid {
            binding: name,
            message: "the Databricks workspace URL must be an HTTPS Databricks workspace host"
                .to_string(),
        }));
    }
    upstream.set_path("");

    Ok(GatewayRoute {
        name,
        target: GatewayTarget::DirectDatabricks,
        region: None,
        project: None,
        azure_endpoint: None,
        cred: AmbientCred::OpenAiApiKey(OpenAiApiKeyCred::new(access_token)?),
        upstream_base_override: Some(upstream.to_string().trim_end_matches('/').to_string()),
        additional_headers: AdditionalHeaders::default(),
    })
}

struct AppState {
    routes: HashMap<String, GatewayRoute>,
    client: reqwest::Client,
    /// Account-specific, read-only control-plane observations supplied by the
    /// hosted route resolver. `None` keeps embedded gateways catalog-only.
    observed_models: Option<ObservedModels>,
    usage_observer: Option<Arc<dyn AiUsageObserver>>,
}

/// Available public model IDs keyed by binding name.
pub type AvailableModels = HashMap<String, HashSet<String>>;

/// Request-time access state for one model in a resolved customer connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservedModelAvailability {
    Available,
    Blocked { reason: String },
    Unknown,
}

/// Observed public model access keyed by binding name and public model ID.
pub type ObservedModels = HashMap<String, HashMap<String, ObservedModelAvailability>>;

/// Build the axum router serving every binding under `/<name>/...`:
/// `POST /<name>/v1/chat/completions`, `POST /<name>/v1/responses`,
/// `POST /<name>/v1/messages`, and `GET /<name>/v1/models`.
pub fn build_router(routes: Vec<GatewayRoute>) -> Router {
    build_router_inner(routes, None, None)
}

/// Build a router that reports completed requests to a non-blocking observer.
pub fn build_router_with_observer(
    routes: Vec<GatewayRoute>,
    usage_observer: Arc<dyn AiUsageObserver>,
) -> Router {
    build_router_inner(routes, None, Some(usage_observer))
}

/// Build a router whose model listing and inference paths are restricted by a
/// bounded availability observation supplied by the control plane.
pub fn build_router_with_availability(
    routes: Vec<GatewayRoute>,
    available_models: AvailableModels,
) -> Router {
    build_router_inner(
        routes,
        Some(observed_from_available(available_models)),
        None,
    )
}

/// Build a router that preserves provider-reported model blocker state.
pub fn build_router_with_observed_models(
    routes: Vec<GatewayRoute>,
    observed_models: ObservedModels,
) -> Router {
    build_router_inner(routes, Some(observed_models), None)
}

/// Build a hosted router with both bounded model availability and usage observation.
pub fn build_router_with_availability_and_observer(
    routes: Vec<GatewayRoute>,
    available_models: AvailableModels,
    usage_observer: Arc<dyn AiUsageObserver>,
) -> Router {
    build_router_inner(
        routes,
        Some(observed_from_available(available_models)),
        Some(usage_observer),
    )
}

/// Build a hosted router that preserves provider-reported model blocker state.
pub fn build_router_with_observed_models_and_observer(
    routes: Vec<GatewayRoute>,
    observed_models: ObservedModels,
    usage_observer: Arc<dyn AiUsageObserver>,
) -> Router {
    build_router_inner(routes, Some(observed_models), Some(usage_observer))
}

fn build_router_inner(
    routes: Vec<GatewayRoute>,
    observed_models: Option<ObservedModels>,
    usage_observer: Option<Arc<dyn AiUsageObserver>>,
) -> Router {
    let routes: HashMap<String, GatewayRoute> =
        routes.into_iter().map(|r| (r.name.clone(), r)).collect();
    let state = Arc::new(AppState {
        routes,
        client: reqwest::Client::new(),
        observed_models,
        usage_observer,
    });
    Router::new()
        .route(
            "/{binding}/v1/chat/completions",
            post(proxy_chat_completions),
        )
        .route("/{binding}/v1/messages", post(proxy_messages))
        .route("/{binding}/v1/responses", post(proxy_responses_universal))
        .route("/{binding}/v1/models", get(list_models))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY))
        .with_state(state)
}

fn observed_from_available(available_models: AvailableModels) -> ObservedModels {
    available_models
        .into_iter()
        .map(|(binding, models)| {
            (
                binding,
                models
                    .into_iter()
                    .map(|model| (model, ObservedModelAvailability::Available))
                    .collect(),
            )
        })
        .collect()
}

/// Parse a proxied request body as JSON and pull out its required `model` field.
/// All three inference handlers route on the request's `model`, so they share
/// this preamble.
///
/// AI SDK conventions namespace model ids as `vendor/model` (for example
/// `openai/gpt-5.6-luna`). The gateway's namespace is `byo/` — every model runs
/// on the caller's own provider account — so `byo/<id>` is accepted as an alias
/// for the bare catalog id, and the payload is normalized so downstream routing,
/// availability checks, and usage reporting all see one spelling.
fn parse_model_request(body: &[u8]) -> Result<(Value, String, String)> {
    let mut payload: Value =
        serde_json::from_slice(body)
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
    let requested_model = model.clone();
    let model = match model.strip_prefix("byo/") {
        Some(bare) => {
            let bare = bare.to_string();
            payload["model"] = Value::String(bare.clone());
            bare
        }
        None => model,
    };
    Ok((payload, requested_model, model))
}

/// Stream a successful provider reply unchanged. Capture a bounded provider error body for
/// customer diagnostics while returning one stable client-compatible error envelope.
async fn forward_response(mut upstream: reqwest::Response) -> Result<Response> {
    let provider_status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !provider_status.is_success() {
        let retry_after = upstream.headers().get(header::RETRY_AFTER).cloned();
        let provider_error_body = capture_provider_error_body(&mut upstream).await;
        let (status, code, message, retryable) = match provider_status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => (
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_access_unavailable",
                "The customer model connection is unavailable",
                false,
            ),
            StatusCode::TOO_MANY_REQUESTS => (
                StatusCode::TOO_MANY_REQUESTS,
                "provider_rate_limited",
                "The customer model provider rate limit was reached",
                true,
            ),
            StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => (
                StatusCode::GATEWAY_TIMEOUT,
                "provider_timeout",
                "The customer model provider timed out",
                true,
            ),
            status if status.is_server_error() => (
                StatusCode::BAD_GATEWAY,
                "provider_unavailable",
                "The customer model provider is unavailable",
                true,
            ),
            status => (
                status,
                "provider_request_rejected",
                "The customer model provider rejected the request",
                false,
            ),
        };
        let mut response = (
            status,
            Json(json!({
                "type": "error",
                "error": { "type": code, "code": code, "message": message },
                "retryable": retryable
            })),
        )
            .into_response();
        if let Some(provider_error_body) = provider_error_body {
            response.extensions_mut().insert(provider_error_body);
        }
        if let Some(value) = retry_after {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        return Ok(response);
    }

    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let mut response = Response::builder().status(provider_status);
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

async fn capture_provider_error_body(
    upstream: &mut reqwest::Response,
) -> Option<ProviderErrorBody> {
    let mut body = Vec::new();
    let mut truncated = false;
    while body.len() <= MAX_PROVIDER_ERROR_BODY {
        match upstream.chunk().await {
            Ok(Some(chunk)) => {
                let remaining = (MAX_PROVIDER_ERROR_BODY + 1).saturating_sub(body.len());
                body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                if body.len() > MAX_PROVIDER_ERROR_BODY {
                    truncated = true;
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {
                truncated = true;
                break;
            }
        }
    }
    if body.is_empty() {
        return None;
    }
    body.truncate(MAX_PROVIDER_ERROR_BODY);
    Some(ProviderErrorBody {
        body: String::from_utf8_lossy(&body).into_owned(),
        truncated,
    })
}

async fn translate_success_response(
    response: Response,
    source: WireProtocol,
    destination: WireProtocol,
    requested_model: &str,
) -> Result<Response> {
    if source == destination || !response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let is_stream = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"));
    if is_stream {
        let stream = futures::stream::unfold(
            (
                Box::pin(response.into_body().into_data_stream()),
                Some(SseTranslator::new(source, destination, requested_model)),
            ),
            |(mut body, mut translator)| async move {
                if translator.is_none() {
                    return None;
                }
                match body.next().await {
                    Some(Ok(bytes)) => {
                        let result = translator
                            .as_mut()
                            .expect("translator exists while the body is open")
                            .push(&bytes)
                            .map(Bytes::from)
                            .map_err(|error| std::io::Error::other(error.to_string()));
                        Some((result, (body, translator)))
                    }
                    Some(Err(error)) => {
                        Some((Err(std::io::Error::other(error.to_string())), (body, None)))
                    }
                    None => translator.take().map(|translator| {
                        (
                            translator
                                .finish()
                                .map(Bytes::from)
                                .map_err(|error| std::io::Error::other(error.to_string())),
                            (body, None),
                        )
                    }),
                }
            },
        );
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(stream))
            .into_alien_error()
            .context(ErrorData::Other {
                message: "could not build translated provider stream".to_string(),
            });
    }
    let body = to_bytes(response.into_body(), MAX_REQUEST_BODY)
        .await
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not read the provider response for protocol translation".to_string(),
        })?;
    let payload =
        serde_json::from_slice(&body)
            .into_alien_error()
            .context(ErrorData::UpstreamFailed {
                message: "provider returned invalid JSON for protocol translation".to_string(),
            })?;
    let translated = translate_response(payload, source, destination, requested_model)?;
    Ok((status, Json(translated)).into_response())
}

fn provider_protocol(client_apis: &[ClientApi], requested: ClientApi) -> WireProtocol {
    if client_apis.contains(&requested) {
        return requested.into();
    }
    client_apis
        .first()
        .copied()
        .map(WireProtocol::from)
        .unwrap_or(WireProtocol::ChatCompletions)
}

fn translate_for_provider(
    payload: Value,
    client_api: ClientApi,
    provider_api: WireProtocol,
) -> Result<Value> {
    let client_protocol = WireProtocol::from(client_api);
    if client_protocol != provider_api
        && payload.get("stream").and_then(Value::as_bool) == Some(true)
        && payload
            .get("tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| !tools.is_empty())
    {
        return Err(AlienError::new(ErrorData::InvalidRequest {
            message: "streaming tool calls require the model's native API; use a non-streaming request or omit tools".to_string(),
        }));
    }
    translate_request(payload, client_protocol, provider_api)
}

/// Databricks' serving-endpoint and MLflow Chat APIs reject OpenAI's newer
/// `max_completion_tokens` field and still require `max_tokens`.
fn normalize_databricks_chat_payload(payload: &mut Value) {
    let Some(obj) = payload.as_object_mut() else {
        return;
    };
    if let Some(limit) = obj.remove("max_completion_tokens") {
        obj.entry("max_tokens".to_string()).or_insert(limit);
    }
}

fn usage_client_api(client_api: ClientApi) -> AiUsageClientApi {
    match client_api {
        ClientApi::OpenAiChatCompletions => AiUsageClientApi::OpenAiChatCompletions,
        ClientApi::OpenAiResponses => AiUsageClientApi::OpenAiResponses,
        ClientApi::AnthropicMessages => AiUsageClientApi::AnthropicMessages,
    }
}

fn observe_result(
    result: Result<Response>,
    observer: Option<&Arc<dyn AiUsageObserver>>,
    context: AiUsageContext,
) -> Result<Response> {
    match result {
        Ok(response) => Ok(observe_response(response, observer, context)),
        Err(error) => {
            observe_gateway_error(observer, context, error.http_status_code.unwrap_or(500));
            Err(error)
        }
    }
}

fn cloud_usage_provider(cloud: Platform) -> AiUsageProvider {
    match cloud {
        Platform::Aws => AiUsageProvider::AwsBedrock,
        Platform::Gcp => AiUsageProvider::GcpVertex,
        Platform::Azure => AiUsageProvider::AzureFoundry,
        _ => unreachable!("AI cloud routes are available only on AWS, GCP, and Azure"),
    }
}

/// Build a JSON POST to `url`, sign it with the ambient credential for `service`,
/// and execute it. The handlers differ only in URL, signing service, body, and any
/// protocol-required header, so the build + sign + execute + upstream-error
/// scaffolding lives here once.
pub(crate) async fn sign_and_execute(
    client: &reqwest::Client,
    route: &GatewayRoute,
    url: &str,
    service: &str,
    body: Vec<u8>,
    extra_headers: &[(&str, &str)],
) -> Result<reqwest::Response> {
    let mut req = client
        .post(url)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .build()
        .into_alien_error()
        .context(ErrorData::Other {
            // The url names which upstream failed; the handlers otherwise share
            // this message and a bare one cannot be traced back to a path.
            message: format!("could not build the upstream request to {url}"),
        })?;
    req.headers_mut().extend(route.additional_headers.0.clone());
    for (name, value) in extra_headers {
        req.headers_mut().insert(
            HeaderName::from_bytes(name.as_bytes())
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("could not build protocol header name `{name}`"),
                })?,
            HeaderValue::from_str(value)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: format!("could not build protocol header `{name}`"),
                })?,
        );
    }
    route.cred.authorize(&mut req, service).await?;
    client
        .execute(req)
        .await
        .into_alien_error()
        .context(ErrorData::UpstreamFailed {
            message: format!("request to {url} failed"),
        })
}

async fn proxy_chat_completions(
    state: State<Arc<AppState>>,
    binding: Path<String>,
    timing: Option<Extension<AiGatewayRequestTiming>>,
    headers: HeaderMap,
    body: ProxyBody,
) -> Result<Response> {
    proxy(
        state,
        binding,
        timing.map(|Extension(timing)| timing),
        headers,
        body,
        ClientApi::OpenAiChatCompletions,
    )
    .await
}

async fn proxy_messages(
    state: State<Arc<AppState>>,
    binding: Path<String>,
    timing: Option<Extension<AiGatewayRequestTiming>>,
    headers: HeaderMap,
    body: ProxyBody,
) -> Result<Response> {
    proxy(
        state,
        binding,
        timing.map(|Extension(timing)| timing),
        headers,
        body,
        ClientApi::AnthropicMessages,
    )
    .await
}

async fn proxy_responses_universal(
    state: State<Arc<AppState>>,
    binding: Path<String>,
    timing: Option<Extension<AiGatewayRequestTiming>>,
    headers: HeaderMap,
    body: ProxyBody,
) -> Result<Response> {
    proxy(
        state,
        binding,
        timing.map(|Extension(timing)| timing),
        headers,
        body,
        ClientApi::OpenAiResponses,
    )
    .await
}

/// Proxy a Chat Completions or Messages request after binding the HTTP path to
/// its exact client protocol. A model sent to the wrong API is rejected before
/// credential lookup, signing, or an upstream request.
async fn proxy(
    State(state): State<Arc<AppState>>,
    Path(binding): Path<String>,
    gateway_timing: Option<AiGatewayRequestTiming>,
    headers: HeaderMap,
    ProxyBody(body): ProxyBody,
    client_api: ClientApi,
) -> Result<Response> {
    let (mut payload, requested_model, model) = parse_model_request(&body)?;
    let usage_context = |binding: &str,
                         provider,
                         public_model: &str,
                         provider_model: &str,
                         client_api,
                         provider_region| {
        AiUsageContext::new(
            binding,
            provider,
            public_model,
            provider_model,
            client_api,
            provider_region,
        )
        .with_gateway_timing(gateway_timing.clone())
        .with_requested_model(&requested_model)
    };
    let route = state.routes.get(&binding).ok_or_else(|| {
        AlienError::new(ErrorData::UnknownBinding {
            binding: binding.clone(),
        })
    })?;

    // Cloud-scoped resolution: Claude ids appear once per cloud, so a first-match
    // resolve would always land on another cloud's entry and fail the cloud filter.
    match route.target {
        GatewayTarget::DirectAnthropic => {
            let provider_model = ai_catalog::resolve_direct_anthropic(&model)
                .map(|resolved| resolved.upstream_id)
                .unwrap_or(model.as_str());
            let descriptor = usage_context(
                &binding,
                AiUsageProvider::Anthropic,
                &model,
                provider_model,
                usage_client_api(client_api),
                None,
            );
            if let Err(error) = ensure_model_available(&state, &binding, &model) {
                return observe_result(Err(error), state.usage_observer.as_ref(), descriptor);
            }
            let client_protocol = WireProtocol::from(client_api);
            let provider_protocol = WireProtocol::Messages;
            let response = async {
                payload = translate_for_provider(payload, client_api, provider_protocol)?;
                let response =
                    proxy_direct_anthropic(&state.client, route, payload, &model, &headers).await?;
                translate_success_response(
                    response,
                    provider_protocol,
                    client_protocol,
                    &requested_model,
                )
                .await
            }
            .await;
            return observe_result(response, state.usage_observer.as_ref(), descriptor);
        }
        GatewayTarget::DirectOpenAi => {
            let direct = ai_catalog::resolve_direct_openai(&model).ok_or_else(|| {
                AlienError::new(ErrorData::ModelNotAvailable {
                    model: model.clone(),
                    binding: binding.clone(),
                })
            })?;
            let provider_protocol = provider_protocol(direct.client_apis, client_api);
            let descriptor = usage_context(
                &binding,
                AiUsageProvider::OpenAi,
                &model,
                &model,
                usage_client_api(client_api),
                None,
            );
            if let Err(error) = ensure_model_available(&state, &binding, &model) {
                return observe_result(Err(error), state.usage_observer.as_ref(), descriptor);
            }
            let client_protocol = WireProtocol::from(client_api);
            let response = async {
                payload = translate_for_provider(payload, client_api, provider_protocol)?;
                let response = proxy_direct_openai(
                    &state.client,
                    route,
                    payload,
                    &model,
                    provider_protocol.path(),
                )
                .await?;
                translate_success_response(
                    response,
                    provider_protocol,
                    client_protocol,
                    &requested_model,
                )
                .await
            }
            .await;
            return observe_result(response, state.usage_observer.as_ref(), descriptor);
        }
        GatewayTarget::DirectDatabricks => {
            let direct = ai_catalog::resolve_direct_databricks(&model).ok_or_else(|| {
                AlienError::new(ErrorData::ModelNotAvailable {
                    model: model.clone(),
                    binding: binding.clone(),
                })
            })?;
            let provider_model = direct.upstream.model_id();
            let provider_protocol = WireProtocol::from(direct.upstream.client_api());
            let descriptor = usage_context(
                &binding,
                AiUsageProvider::Databricks,
                &model,
                provider_model,
                usage_client_api(client_api),
                None,
            );
            if let Err(error) = ensure_model_available(&state, &binding, &model) {
                return observe_result(Err(error), state.usage_observer.as_ref(), descriptor);
            }
            let client_protocol = WireProtocol::from(client_api);
            let response = async {
                payload = translate_for_provider(payload, client_api, provider_protocol)?;
                if provider_protocol == WireProtocol::ChatCompletions {
                    normalize_databricks_chat_payload(&mut payload);
                }
                let response = match direct.upstream {
                    ai_catalog::DirectDatabricksUpstream::ServingEndpointChat { endpoint } => {
                        debug_assert_eq!(endpoint, provider_model);
                        let path = format!("/serving-endpoints/{provider_model}/invocations");
                        proxy_direct_openai(&state.client, route, payload, provider_model, &path)
                            .await
                    }
                    ai_catalog::DirectDatabricksUpstream::ModelServiceChat { service } => {
                        debug_assert_eq!(service, provider_model);
                        proxy_direct_openai(
                            &state.client,
                            route,
                            payload,
                            provider_model,
                            "/ai-gateway/mlflow/v1/chat/completions",
                        )
                        .await
                    }
                    ai_catalog::DirectDatabricksUpstream::ModelServiceMessages { service } => {
                        debug_assert_eq!(service, provider_model);
                        proxy_direct_anthropic_at(
                            &state.client,
                            route,
                            payload,
                            provider_model,
                            &headers,
                            "/ai-gateway/anthropic/v1/messages",
                        )
                        .await
                    }
                }?;
                translate_success_response(
                    response,
                    provider_protocol,
                    client_protocol,
                    &requested_model,
                )
                .await
            }
            .await;
            return observe_result(response, state.usage_observer.as_ref(), descriptor);
        }
        GatewayTarget::Cloud(_) => {}
    }
    let cloud = match route.target {
        GatewayTarget::Cloud(cloud) => cloud,
        GatewayTarget::DirectAnthropic
        | GatewayTarget::DirectDatabricks
        | GatewayTarget::DirectOpenAi => {
            unreachable!("handled above")
        }
    };
    let cm = ai_catalog::resolve_for(&model, cloud).ok_or_else(|| {
        AlienError::new(ErrorData::ModelNotAvailable {
            model: model.clone(),
            binding: binding.clone(),
        })
    })?;
    let descriptor = usage_context(
        &binding,
        cloud_usage_provider(cloud),
        &model,
        cm.upstream_id,
        usage_client_api(client_api),
        route.region.clone(),
    );

    if let Err(error) = ensure_model_available(&state, &binding, &model) {
        return observe_result(Err(error), state.usage_observer.as_ref(), descriptor);
    }

    let client_protocol = WireProtocol::from(client_api);
    let effective_provider_api = if client_api == ClientApi::OpenAiResponses
        && ai_catalog::responses_target(cm.public_id).is_some()
    {
        ProviderApi::OpenAiResponses
    } else {
        cm.provider_api
    };
    let provider_protocol = WireProtocol::from(effective_provider_api);
    payload = match translate_for_provider(payload, client_api, provider_protocol) {
        Ok(payload) => payload,
        Err(error) => {
            return observe_result(Err(error), state.usage_observer.as_ref(), descriptor);
        }
    };

    // AWS serves Claude through classic Bedrock InvokeModel, not the passthrough
    // endpoint: the model id travels in the URL and the streamed reply is AWS
    // event-stream framing, so it needs its own request/response shape.
    if cloud == Platform::Aws && cm.provider_api == ProviderApi::Anthropic {
        let response =
            proxy_bedrock_anthropic(&state.client, route, cm.upstream_id, payload, &headers).await;
        let response = match response {
            Ok(response) => {
                translate_success_response(
                    response,
                    provider_protocol,
                    client_protocol,
                    &requested_model,
                )
                .await
            }
            Err(error) => Err(error),
        };
        return observe_result(response, state.usage_observer.as_ref(), descriptor);
    }
    // GCP serves Claude through Vertex rawPredict: the model id travels in the URL
    // and streaming is chosen by the URL verb, but the reply is native Anthropic
    // JSON/SSE — no decoder needed, unlike Bedrock.
    if cloud == Platform::Gcp && cm.provider_api == ProviderApi::Anthropic {
        let response =
            proxy_vertex_anthropic(&state.client, route, cm.upstream_id, payload, &headers).await;
        let response = match response {
            Ok(response) => {
                translate_success_response(
                    response,
                    provider_protocol,
                    client_protocol,
                    &requested_model,
                )
                .await
            }
            Err(error) => Err(error),
        };
        return observe_result(response, state.usage_observer.as_ref(), descriptor);
    }
    // Azure serves Claude through Foundry's Anthropic endpoint: standard Messages
    // in both directions, on the `/anthropic/v1` path with the version header.
    if cloud == Platform::Azure && cm.provider_api == ProviderApi::Anthropic {
        let response =
            proxy_foundry_anthropic(&state.client, route, cm.upstream_id, payload, &headers).await;
        let response = match response {
            Ok(response) => {
                translate_success_response(
                    response,
                    provider_protocol,
                    client_protocol,
                    &requested_model,
                )
                .await
            }
            Err(error) => Err(error),
        };
        return observe_result(response, state.usage_observer.as_ref(), descriptor);
    }

    let response = async {
        let (provider_model, url, aws_service) =
            if effective_provider_api == ProviderApi::OpenAiResponses {
                let target = ai_catalog::responses_target(cm.public_id).ok_or_else(|| {
                    AlienError::new(ErrorData::ModelNotAvailable {
                        model: model.clone(),
                        binding: binding.clone(),
                    })
                })?;
                let region = route
                    .region
                    .as_deref()
                    .ok_or_else(|| missing_field(route, "region"))?;
                let base = route
                    .upstream_base_override
                    .clone()
                    .unwrap_or_else(|| format!("https://bedrock-mantle.{region}.api.aws"));
                (
                    target.upstream_id,
                    format!("{}{}", base.trim_end_matches('/'), target.path),
                    "bedrock-mantle",
                )
            } else {
                let (url, service) = upstream_target(route, effective_provider_api)?;
                (cm.upstream_id, url, service)
            };
        payload["model"] = Value::String(provider_model.to_string());
        let upstream_body =
            serde_json::to_vec(&payload)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "could not re-serialize the rewritten request body".to_string(),
                })?;
        let upstream =
            sign_and_execute(&state.client, route, &url, aws_service, upstream_body, &[]).await?;
        forward_response(upstream).await
    }
    .await;
    let response = match response {
        Ok(response) => {
            translate_success_response(
                response,
                provider_protocol,
                client_protocol,
                &requested_model,
            )
            .await
        }
        Err(error) => Err(error),
    };
    observe_result(response, state.usage_observer.as_ref(), descriptor)
}

/// `GET /<name>/v1/models`: the qualified catalog, intersected with the bounded
/// account observation when the hosted control plane supplied one. Ids are
/// listed in the `byo/` namespace — the spelling the chat endpoints accept —
/// and the bare id remains accepted as an alias.
async fn list_models(
    State(state): State<Arc<AppState>>,
    Path(binding): Path<String>,
) -> Result<Response> {
    let route = state.routes.get(&binding).ok_or_else(|| {
        AlienError::new(ErrorData::UnknownBinding {
            binding: binding.clone(),
        })
    })?;
    let observed = state
        .observed_models
        .as_ref()
        .map(|by_binding| by_binding.get(&binding));

    let data: Vec<Value> = match route.target {
        GatewayTarget::Cloud(cloud) => ai_catalog::models_for(cloud)
            .into_iter()
            .filter(|model| {
                observed.is_none_or(|models| {
                    models.is_some_and(|models| {
                        models.get(model.public_id) == Some(&ObservedModelAvailability::Available)
                    })
                })
            })
            .map(|model| {
                json!({
                    "id": format!("byo/{}", model.public_id),
                    "object": "model",
                    "provider": model.provider(),
                    "displayName": model.display_name(),
                })
            })
            .collect(),
        GatewayTarget::DirectAnthropic => ai_catalog::direct_anthropic_models()
            .into_iter()
            .filter(|model| {
                observed.is_none_or(|models| {
                    models.is_some_and(|models| {
                        models.get(model.public_id) == Some(&ObservedModelAvailability::Available)
                    })
                })
            })
            .map(|model| {
                json!({
                    "id": format!("byo/{}", model.public_id),
                    "object": "model",
                    "provider": "anthropic",
                    "displayName": model.display_name(),
                })
            })
            .collect(),
        GatewayTarget::DirectOpenAi => {
            let mut models = observed
                .into_iter()
                .flatten()
                .flat_map(|models| models.iter())
                .filter_map(|(model, availability)| {
                    (availability == &ObservedModelAvailability::Available).then_some(model)
                })
                .collect::<Vec<_>>();
            models.sort();
            models
                .into_iter()
                .map(|model| {
                    json!({
                        "id": format!("byo/{model}"),
                        "object": "model",
                        "provider": "openai",
                        "displayName": model,
                    })
                })
                .collect()
        }
        GatewayTarget::DirectDatabricks => {
            let mut models = observed
                .into_iter()
                .flatten()
                .flat_map(|models| models.iter())
                .filter_map(|(model, availability)| {
                    (availability == &ObservedModelAvailability::Available).then_some(model)
                })
                .collect::<Vec<_>>();
            models.sort();
            models
                .into_iter()
                .map(|model| {
                    json!({
                        "id": format!("byo/{model}"),
                        "object": "model",
                        "provider": "databricks",
                        "displayName": model,
                    })
                })
                .collect()
        }
    };
    Ok(Json(json!({ "object": "list", "data": data })).into_response())
}

fn ensure_model_available(state: &AppState, binding: &str, model: &str) -> Result<()> {
    let Some(by_binding) = state.observed_models.as_ref() else {
        return Ok(());
    };
    match by_binding.get(binding).and_then(|models| models.get(model)) {
        Some(ObservedModelAvailability::Available) => Ok(()),
        Some(ObservedModelAvailability::Blocked { reason }) => {
            Err(AlienError::new(ErrorData::ModelAccessRequired {
                model: model.to_string(),
                binding: binding.to_string(),
                reason: reason.clone(),
            }))
        }
        Some(ObservedModelAvailability::Unknown) => {
            Err(AlienError::new(ErrorData::ModelAvailabilityUnknown {
                model: model.to_string(),
                binding: binding.to_string(),
            }))
        }
        None => Err(AlienError::new(ErrorData::ModelNotAvailable {
            model: model.to_string(),
            binding: binding.to_string(),
        })),
    }
}

async fn proxy_direct_anthropic(
    client: &reqwest::Client,
    route: &GatewayRoute,
    payload: Value,
    model: &str,
    headers: &HeaderMap,
) -> Result<Response> {
    let direct = ai_catalog::resolve_direct_anthropic(model).ok_or_else(|| {
        AlienError::new(ErrorData::ModelNotAvailable {
            model: model.to_string(),
            binding: route.name.clone(),
        })
    })?;
    proxy_direct_anthropic_at(
        client,
        route,
        payload,
        direct.upstream_id,
        headers,
        "/v1/messages",
    )
    .await
}

async fn proxy_direct_anthropic_at(
    client: &reqwest::Client,
    route: &GatewayRoute,
    mut payload: Value,
    provider_model: &str,
    headers: &HeaderMap,
    path: &str,
) -> Result<Response> {
    payload["model"] = Value::String(provider_model.to_string());
    let body = serde_json::to_vec(&payload)
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not serialize the Anthropic request".to_string(),
        })?;
    let base = route
        .upstream_base_override
        .as_deref()
        .unwrap_or("https://api.anthropic.com");
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let version = headers
        .get("anthropic-version")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("2023-06-01");
    let betas = bedrock::filtered_header_betas(headers).join(",");
    let mut extra_headers = vec![("anthropic-version", version)];
    if !betas.is_empty() {
        extra_headers.push(("anthropic-beta", betas.as_str()));
    }
    let upstream = sign_and_execute(client, route, &url, "", body, &extra_headers).await?;
    forward_response(upstream).await
}

async fn proxy_direct_openai(
    client: &reqwest::Client,
    route: &GatewayRoute,
    mut payload: Value,
    model: &str,
    path: &str,
) -> Result<Response> {
    payload["model"] = Value::String(model.to_string());
    let body = serde_json::to_vec(&payload)
        .into_alien_error()
        .context(ErrorData::Other {
            message: "could not serialize the OpenAI request".to_string(),
        })?;
    let base = route
        .upstream_base_override
        .as_deref()
        .unwrap_or("https://api.openai.com");
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let upstream = sign_and_execute(client, route, &url, "", body, &[]).await?;
    forward_response(upstream).await
}

/// The error for a binding missing a field a handler needs.
pub(crate) fn missing_field(route: &GatewayRoute, field: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::BindingConfigInvalid {
        binding: route.name.clone(),
        message: format!("it is missing its {field}"),
    })
}

/// The upstream URL and (for AWS) the SigV4 service name for a binding + protocol.
pub(crate) fn upstream_target(
    route: &GatewayRoute,
    provider_api: ProviderApi,
) -> Result<(String, &'static str)> {
    let cloud = match route.target {
        GatewayTarget::Cloud(cloud) => cloud,
        GatewayTarget::DirectAnthropic
        | GatewayTarget::DirectDatabricks
        | GatewayTarget::DirectOpenAi => {
            return Err(AlienError::new(ErrorData::Other {
                message: "direct providers do not use a cloud upstream target".to_string(),
            }))
        }
    };
    let (host, path, aws_service) = match (cloud, provider_api) {
        (Platform::Aws, ProviderApi::OpenAi) => {
            let region = route
                .region
                .as_deref()
                .ok_or_else(|| missing_field(route, "region"))?;
            (
                format!("https://bedrock-runtime.{region}.amazonaws.com"),
                "/openai/v1/chat/completions".to_string(),
                "bedrock",
            )
        }
        (Platform::Gcp, ProviderApi::OpenAi) => {
            let location = route
                .region
                .as_deref()
                .ok_or_else(|| missing_field(route, "location"))?;
            let project = route
                .project
                .as_deref()
                .ok_or_else(|| missing_field(route, "project"))?;
            (
                vertex_host(location),
                format!(
                    "/v1/projects/{project}/locations/{location}/endpoints/openapi/chat/completions"
                ),
                "",
            )
        }
        (Platform::Azure, ProviderApi::OpenAi) => {
            let endpoint = route
                .azure_endpoint
                .as_deref()
                .ok_or_else(|| missing_field(route, "endpoint"))?;
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

    let base = route.upstream_base_override.clone().unwrap_or(host);
    Ok((
        format!("{}{}", base.trim_end_matches('/'), path),
        aws_service,
    ))
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

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::mpsc;
    use std::time::Duration;

    use aws_credential_types::provider::SharedCredentialsProvider;
    use aws_credential_types::Credentials;
    use aws_smithy_eventstream::frame::write_message_to;
    use aws_smithy_types::event_stream::Message;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use httpmock::prelude::*;

    use super::*;
    use crate::creds::{AwsSigV4Cred, BearerTokenCred};
    use crate::usage::{AiTokenUsage, AiUsageEvent, AiUsageOutcome};

    struct TestUsageObserver(mpsc::Sender<AiUsageEvent>);

    impl AiUsageObserver for TestUsageObserver {
        fn observe(&self, event: AiUsageEvent) {
            let _ = self.0.send(event);
        }
    }

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
            target: GatewayTarget::Cloud(Platform::Aws),
            region: Some("us-east-2".to_string()),
            project: None,
            azure_endpoint: None,
            cred: test_aws_cred(),
            upstream_base_override: Some(upstream.to_string()),
            additional_headers: AdditionalHeaders::default(),
        }
    }

    fn gcp_route(location: &str) -> GatewayRoute {
        GatewayRoute {
            name: "llm".to_string(),
            target: GatewayTarget::Cloud(Platform::Gcp),
            region: Some(location.to_string()),
            project: Some("my-proj".to_string()),
            azure_endpoint: None,
            cred: AmbientCred::Bearer(BearerTokenCred::static_token("t")),
            upstream_base_override: None,
            additional_headers: AdditionalHeaders::default(),
        }
    }

    fn direct_openai_route(upstream: &str) -> GatewayRoute {
        let mut route = route_from_direct_openai("llm", "sk-test").expect("direct OpenAI route");
        route.upstream_base_override = Some(upstream.to_string());
        route
    }

    fn direct_databricks_route(upstream: &str) -> GatewayRoute {
        let mut route = route_from_direct_databricks(
            "llm",
            "https://dbc-123.cloud.databricks.com",
            "temporary-oauth-token",
        )
        .expect("direct Databricks route");
        route.upstream_base_override = Some(upstream.to_string());
        route
    }

    #[test]
    fn additional_headers_reject_gateway_and_credential_headers() {
        for name in [
            "authorization",
            "X-Api-Key",
            "x-amz-date",
            "x-goog-api-key",
            "x-forwarded-for",
            "x-alien-external-id",
        ] {
            let error = match direct_openai_route("http://localhost")
                .with_additional_headers([(name.to_string(), "value".to_string())])
            {
                Ok(_) => panic!("reserved header `{name}` must be rejected"),
                Err(error) => error,
            };
            assert!(error.to_string().contains("managed by the AI gateway"));
        }
    }

    #[tokio::test]
    async fn additional_headers_are_forwarded_to_the_provider() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/responses")
                    .header("x-partner-id", "partner-123")
                    .header("user-agent", "alien-customer/1.0")
                    .header("authorization", "Bearer sk-test");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"id":"resp_1","usage":{"input_tokens":1,"output_tokens":1}}"#);
            })
            .await;
        let route = direct_openai_route(&server.base_url())
            .with_additional_headers([
                ("x-partner-id".to_string(), "partner-123".to_string()),
                ("user-agent".to_string(), "alien-customer/1.0".to_string()),
            ])
            .expect("valid additional headers");
        let url = serve(build_router(vec![route])).await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/responses"))
            .json(&json!({"model": "gpt-4.1-mini", "input": "hi"}))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(response.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn direct_openai_responses_are_observed() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/responses")
                    .body_contains("gpt-4.1-mini")
                    .header("authorization", "Bearer sk-test");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"id":"resp_1","usage":{"input_tokens":12,"output_tokens":7}}"#);
            })
            .await;
        let (sender, receiver) = mpsc::channel();
        let observer: Arc<dyn AiUsageObserver> = Arc::new(TestUsageObserver(sender));
        let url = serve(build_router_with_observer(
            vec![direct_openai_route(&server.base_url())],
            observer,
        ))
        .await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/responses"))
            .json(&json!({"model": "gpt-4.1-mini", "input": "hi"}))
            .send()
            .await
            .expect("proxy request");
        assert_eq!(response.status(), 200);
        response.bytes().await.expect("consume response body");

        let event = receiver.try_recv().expect("completed usage observation");
        assert_eq!(event.client_api, AiUsageClientApi::OpenAiResponses);
        assert_eq!(event.provider, AiUsageProvider::OpenAi);
        assert_eq!(event.outcome, AiUsageOutcome::Success);
        assert_eq!(event.status, 200);
        assert_eq!(event.tokens.input_tokens, Some(12));
        assert_eq!(event.tokens.output_tokens, Some(7));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn gateway_failures_are_observed_without_swallowing_the_error() {
        let mut route = route_from_direct_openai("llm", "sk-test").unwrap();
        route.upstream_base_override = Some("http://[invalid".to_string());
        let (sender, receiver) = mpsc::channel();
        let observer: Arc<dyn AiUsageObserver> = Arc::new(TestUsageObserver(sender));
        let url = serve(build_router_with_observer(vec![route], observer)).await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/responses"))
            .json(&json!({"model": "gpt-4.1-mini", "input": "hi"}))
            .send()
            .await
            .expect("gateway response");
        assert_eq!(response.status(), 500);

        let event = receiver.try_recv().expect("gateway error observation");
        assert_eq!(event.client_api, AiUsageClientApi::OpenAiResponses);
        assert_eq!(event.outcome, AiUsageOutcome::GatewayError);
        assert_eq!(event.status, 500);
        assert_eq!(event.tokens, AiTokenUsage::default());
    }

    #[tokio::test]
    async fn attributable_validation_failures_are_observed() {
        let route = route_from_direct_openai("llm", "sk-test").unwrap();
        let (sender, receiver) = mpsc::channel();
        let observer: Arc<dyn AiUsageObserver> = Arc::new(TestUsageObserver(sender));
        let url = serve(build_router_with_availability_and_observer(
            vec![route],
            HashMap::from([("llm".to_string(), HashSet::new())]),
            observer,
        ))
        .await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/responses"))
            .json(&json!({"model": "gpt-4.1-mini", "input": "hi"}))
            .send()
            .await
            .expect("gateway response");
        assert_eq!(response.status(), 404);

        let event = receiver.try_recv().expect("validation error observation");
        assert_eq!(event.binding, "llm");
        assert_eq!(event.provider, AiUsageProvider::OpenAi);
        assert_eq!(event.public_model, "gpt-4.1-mini");
        assert_eq!(event.provider_model, "gpt-4.1-mini");
        assert_eq!(event.client_api, AiUsageClientApi::OpenAiResponses);
        assert_eq!(event.outcome, AiUsageOutcome::GatewayError);
        assert_eq!(event.status, 404);
        assert_eq!(event.tokens, AiTokenUsage::default());
    }

    #[tokio::test]
    async fn translated_client_api_requests_are_observed_under_the_client_protocol() {
        let server = MockServer::start_async().await;
        let upstream = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/chat/completions");
                then.status(200).header("content-type", "application/json").json_body(json!({
                    "id": "chatcmpl_1", "choices": [{ "message": { "role": "assistant", "content": "ok" }, "finish_reason": "stop" }],
                    "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
                }));
            })
            .await;
        let mut route = route_from_direct_openai("llm", "sk-test").unwrap();
        route.upstream_base_override = Some(server.base_url());
        let (sender, receiver) = mpsc::channel();
        let observer: Arc<dyn AiUsageObserver> = Arc::new(TestUsageObserver(sender));
        let url = serve(build_router_with_observer(vec![route], observer)).await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .json(&json!({"model": "gpt-4.1-mini", "messages": []}))
            .send()
            .await
            .expect("gateway response");
        assert_eq!(response.status(), 200);
        let _: Value = response.json().await.expect("translated Messages response");
        upstream.assert_async().await;

        let event = receiver.try_recv().expect("validation error observation");
        assert_eq!(event.provider, AiUsageProvider::OpenAi);
        assert_eq!(event.public_model, "gpt-4.1-mini");
        assert_eq!(event.client_api, AiUsageClientApi::AnthropicMessages);
        assert_eq!(event.outcome, AiUsageOutcome::Success);
        assert_eq!(event.status, 200);
    }

    #[tokio::test]
    async fn databricks_responses_use_the_models_canonical_serving_endpoint_route() {
        let server = MockServer::start_async().await;
        let upstream = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/serving-endpoints/databricks-gpt-5-6-sol/invocations")
                    .header("authorization", "Bearer temporary-oauth-token")
                    .body_contains("databricks-gpt-5-6-sol")
                    .body_contains("messages")
                    .body_contains("max_tokens");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(json!({
                        "id": "completion",
                        "model": "databricks-gpt-5-6-sol",
                        "choices": [{
                            "index": 0,
                            "message": {"role": "assistant", "content": "pong"},
                            "finish_reason": "stop"
                        }],
                        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                    }));
            })
            .await;
        let url = serve(build_router_with_availability(
            vec![direct_databricks_route(&server.base_url())],
            HashMap::from([(
                "llm".to_string(),
                HashSet::from(["gpt-5.6-sol".to_string()]),
            )]),
        ))
        .await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/responses"))
            .json(&json!({
                "model": "gpt-5.6-sol",
                "input": "ping",
                "max_output_tokens": 32
            }))
            .send()
            .await
            .expect("proxy response");

        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.expect("Responses body");
        assert_eq!(body["model"], "gpt-5.6-sol");
        assert_eq!(body["output"][0]["content"][0]["text"], "pong");
        upstream.assert_async().await;
    }

    #[tokio::test]
    async fn databricks_chat_rewrites_the_public_model_id() {
        let server = MockServer::start_async().await;
        let upstream = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/ai-gateway/mlflow/v1/chat/completions")
                    .header("authorization", "Bearer temporary-oauth-token")
                    .body_contains("system.ai.gemma-3-12b");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(json!({"id": "completion", "choices": []}));
            })
            .await;
        let url = serve(build_router_with_availability(
            vec![direct_databricks_route(&server.base_url())],
            HashMap::from([(
                "llm".to_string(),
                HashSet::from(["gemma-3-12b".to_string()]),
            )]),
        ))
        .await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({"model": "gemma-3-12b", "messages": []}))
            .send()
            .await
            .expect("proxy response");

        assert_eq!(response.status(), 200);
        upstream.assert_async().await;
    }

    #[tokio::test]
    async fn databricks_messages_rewrites_claude_and_forwards_version() {
        let server = MockServer::start_async().await;
        let upstream = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/ai-gateway/anthropic/v1/messages")
                    .header("authorization", "Bearer temporary-oauth-token")
                    .header("anthropic-version", "2023-06-01")
                    .body_contains("system.ai.claude-sonnet-5");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(json!({"id": "message", "content": []}));
            })
            .await;
        let url = serve(build_router_with_availability(
            vec![direct_databricks_route(&server.base_url())],
            HashMap::from([(
                "llm".to_string(),
                HashSet::from(["claude-sonnet-5".to_string()]),
            )]),
        ))
        .await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/messages"))
            .header("anthropic-version", "2023-06-01")
            .json(&json!({"model": "claude-sonnet-5", "max_tokens": 16, "messages": []}))
            .send()
            .await
            .expect("proxy response");

        assert_eq!(response.status(), 200);
        upstream.assert_async().await;
    }

    #[tokio::test]
    async fn observes_usage_from_a_completed_response_body() {
        let server = MockServer::start_async().await;
        let upstream = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/chat/completions");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(json!({
                        "id": "response",
                        "usage": {
                            "prompt_tokens": 13,
                            "completion_tokens": 5,
                            "prompt_tokens_details": { "cached_tokens": 3 }
                        }
                    }));
            })
            .await;
        let (sender, receiver) = mpsc::channel();
        let observer: Arc<dyn AiUsageObserver> = Arc::new(TestUsageObserver(sender));
        let url = serve(build_router_with_observer(
            vec![direct_openai_route(&server.base_url())],
            observer,
        ))
        .await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({ "model": "byo/gpt-4.1-mini", "messages": [] }))
            .send()
            .await
            .expect("proxy response");
        let body = response.bytes().await.expect("response body");
        assert!(body
            .windows(b"prompt_tokens".len())
            .any(|part| part == b"prompt_tokens"));

        let event = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("completed usage observation");
        assert_eq!(event.provider, AiUsageProvider::OpenAi);
        assert_eq!(event.requested_model, "byo/gpt-4.1-mini");
        assert_eq!(event.public_model, "gpt-4.1-mini");
        assert_eq!(event.client_api, AiUsageClientApi::OpenAiChatCompletions);
        assert_eq!(event.outcome, AiUsageOutcome::Success);
        assert_eq!(event.status, 200);
        assert_eq!(event.tokens.input_tokens, Some(13));
        assert_eq!(event.tokens.output_tokens, Some(5));
        assert_eq!(event.tokens.cache_read_tokens, Some(3));
        upstream.assert_async().await;
    }

    #[tokio::test]
    async fn observes_provider_error_body_without_exposing_it_to_the_caller() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/chat/completions");
                then.status(429)
                    .header("retry-after", "2")
                    .json_body(json!({ "secret_provider_detail": "must not escape" }));
            })
            .await;
        let (sender, receiver) = mpsc::channel();
        let observer: Arc<dyn AiUsageObserver> = Arc::new(TestUsageObserver(sender));
        let url = serve(build_router_with_observer(
            vec![direct_openai_route(&server.base_url())],
            observer,
        ))
        .await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({ "model": "gpt-4.1-mini", "messages": [] }))
            .send()
            .await
            .expect("proxy response");
        assert_eq!(response.status(), 429);
        let body = response.text().await.expect("safe error body");
        assert!(!body.contains("secret_provider_detail"));

        let event = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("provider error observation");
        assert_eq!(event.outcome, AiUsageOutcome::ProviderError);
        assert_eq!(event.status, 429);
        assert_eq!(
            event.provider_error_body.as_deref(),
            Some(r#"{"secret_provider_detail":"must not escape"}"#)
        );
        assert!(!event.provider_error_body_truncated);
        assert_eq!(event.tokens, AiTokenUsage::default());
    }

    #[tokio::test]
    async fn truncates_provider_error_body_for_diagnostics() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/chat/completions");
                then.status(400)
                    .body("x".repeat(MAX_PROVIDER_ERROR_BODY + 1));
            })
            .await;
        let (sender, receiver) = mpsc::channel();
        let observer: Arc<dyn AiUsageObserver> = Arc::new(TestUsageObserver(sender));
        let url = serve(build_router_with_observer(
            vec![direct_openai_route(&server.base_url())],
            observer,
        ))
        .await;

        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({ "model": "gpt-4.1-mini", "messages": [] }))
            .send()
            .await
            .expect("proxy response");
        assert_eq!(response.status(), 400);
        response.bytes().await.expect("safe error body");

        let event = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("provider error observation");
        assert_eq!(
            event
                .provider_error_body
                .as_deref()
                .expect("provider error body")
                .len(),
            MAX_PROVIDER_ERROR_BODY
        );
        assert!(event.provider_error_body_truncated);
    }

    #[test]
    fn gcp_vertex_url_regional_vs_global() {
        // A region prefixes the host; `global` uses the un-prefixed host. The path always
        // carries `locations/{location}`.
        let (regional, _) =
            upstream_target(&gcp_route("us-central1"), ProviderApi::OpenAi).unwrap();
        assert_eq!(
            regional,
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-proj/locations/us-central1/endpoints/openapi/chat/completions"
        );
        let (global, _) = upstream_target(&gcp_route("global"), ProviderApi::OpenAi).unwrap();
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
        assert!(
            text.contains("\"pong\""),
            "upstream body must pass through: {text}"
        );
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
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        assert_eq!(
            resp.text().await.unwrap(),
            sse,
            "SSE must stream through byte-for-byte"
        );
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
            resp.text()
                .await
                .unwrap()
                .contains("GATEWAY_INVALID_REQUEST"),
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
            .header(
                "anthropic-beta",
                "computer-use-2025-01-24, oauth-2025-04-20",
            )
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
            target: GatewayTarget::Cloud(Platform::Azure),
            region: None,
            project: None,
            azure_endpoint: Some(endpoint.to_string()),
            cred: AmbientCred::Bearer(BearerTokenCred::static_token("t")),
            upstream_base_override: None,
            additional_headers: AdditionalHeaders::default(),
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
        assert!(
            text.contains("\"pong\""),
            "upstream body must pass through: {text}"
        );
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
            .header(
                "anthropic-beta",
                "computer-use-2025-01-24, oauth-2025-04-20",
            )
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
        assert!(
            text.contains("\"pong\""),
            "upstream body must pass through: {text}"
        );
        // The mock only matches when the body carries the rewritten upstream id and an
        // Authorization header, so a hit proves the model rewrite and cred injection.
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn byo_namespace_routes_like_the_bare_model_id() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/openai/v1/chat/completions")
                    // The namespace never reaches the provider: the body carries the
                    // rewritten upstream id, exactly as for the bare public id.
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
            .json(&json!({"model":"byo/gpt-oss-20b","messages":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await;

        // The namespace is `byo/` only — any other vendor prefix stays an unknown model,
        // so ids never resolve to a provider the customer didn't connect.
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({"model":"openai/gpt-oss-20b","messages":[]}))
            .send()
            .await
            .expect("proxy request");
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn chat_completions_translates_to_a_responses_only_model() {
        let server = MockServer::start_async().await;
        let upstream = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/openai/v1/responses")
                    .body_contains("\"input\"");
                then.status(200).header("content-type", "application/json").json_body(json!({
                    "id": "resp_1",
                    "object": "response",
                    "output": [{ "type": "message", "content": [{ "type": "output_text", "text": "hello" }] }],
                    "usage": { "input_tokens": 3, "output_tokens": 1 }
                }));
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({"model":"gpt-5.5","messages":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["object"], "chat.completion");
        assert_eq!(body["choices"][0]["message"]["content"], "hello");
        assert_eq!(body["usage"]["total_tokens"], 4);
        upstream.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn each_client_protocol_can_call_a_model_with_another_native_protocol() {
        let anthropic = MockServer::start_async().await;
        let anthropic_upstream = anthropic
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/messages")
                    .body_contains("\"max_tokens\":32");
                then.status(200).header("content-type", "application/json").json_body(json!({
                    "id": "msg_1", "type": "message", "content": [{ "type": "text", "text": "hello" }],
                    "stop_reason": "end_turn", "usage": { "input_tokens": 2, "output_tokens": 1 }
                }));
            })
            .await;
        let mut anthropic_route = route_from_direct_anthropic("llm", "sk-ant-api-test").unwrap();
        anthropic_route.upstream_base_override = Some(anthropic.base_url());
        let anthropic_url = serve(build_router(vec![anthropic_route])).await;

        let chat_response: Value = reqwest::Client::new()
            .post(format!("{anthropic_url}/llm/v1/chat/completions"))
            .json(&json!({"model":"claude-opus-4.8","max_completion_tokens":32,"messages":[{"role":"user","content":"hi"}]}))
            .send().await.expect("chat request").json().await.expect("chat response");
        assert_eq!(chat_response["object"], "chat.completion");
        assert_eq!(chat_response["choices"][0]["message"]["content"], "hello");
        anthropic_upstream.assert_async().await;

        let openai = MockServer::start_async().await;
        let openai_upstream = openai
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/chat/completions")
                    .body_contains("\"messages\"");
                then.status(200).header("content-type", "application/json").json_body(json!({
                    "id": "chatcmpl_1", "object": "chat.completion", "created": 1,
                    "choices": [{ "index": 0, "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
                    "usage": { "prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3 }
                }));
            })
            .await;
        let mut openai_route = route_from_direct_openai("llm", "test-key").unwrap();
        openai_route.upstream_base_override = Some(openai.base_url());
        let openai_url = serve(build_router(vec![openai_route])).await;
        let messages_response: Value = reqwest::Client::new()
            .post(format!("{openai_url}/llm/v1/messages"))
            .json(&json!({"model":"gpt-4.1-mini","max_tokens":32,"messages":[{"role":"user","content":"hi"}]}))
            .send().await.expect("messages request").json().await.expect("messages response");
        assert_eq!(messages_response["type"], "message");
        assert_eq!(messages_response["content"][0]["text"], "hello");
        openai_upstream.assert_async().await;
    }

    #[test]
    fn translated_streaming_tools_fail_before_the_provider() {
        let error = translate_for_provider(
            json!({
                "model": "claude-opus-4.8",
                "stream": true,
                "messages": [{ "role": "user", "content": "hi" }],
                "tools": [{ "type": "function", "function": { "name": "lookup", "parameters": {} } }]
            }),
            ClientApi::OpenAiChatCompletions,
            WireProtocol::Messages,
        )
        .unwrap_err();
        assert!(error.to_string().contains("streaming tool calls"));
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
        let out = decoder.push(&raw_payload_frame(
            r#"{"message":"seeded-stream-canary-62fa91"}"#,
        ));
        assert!(
            out.contains("event: error"),
            "exception frame must surface an error: {out}"
        );
        assert!(out.contains("provider interrupted the response"));
        assert!(!out.contains("seeded-stream-canary-62fa91"));
    }

    #[test]
    fn decoder_emits_normal_chunk_then_surfaces_a_following_exception() {
        let mut decoder = EventStreamToSse::default();
        let mut bytes =
            eventstream_frame(r#"{"type":"content_block_delta","delta":{"text":"hi"}}"#);
        bytes.extend_from_slice(&raw_payload_frame(
            r#"{"message":"seeded-trailing-canary-ef39a2"}"#,
        ));
        let out = decoder.push(&bytes);
        assert!(
            out.contains("event: content_block_delta"),
            "the normal delta must decode: {out}"
        );
        assert!(
            out.contains("event: error") && !out.contains("seeded-trailing-canary-ef39a2"),
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
        assert!(
            out.contains("event: error"),
            "a desynced frame must surface an error: {out}"
        );
        // A desync is unrecoverable, so further input is ignored rather than decoded
        // mid-stream as if nothing were wrong.
        let after = decoder.push(&eventstream_frame(r#"{"type":"message_stop"}"#));
        assert_eq!(after, "", "decoder must stop after a desync");
    }

    #[test]
    fn decoder_fails_loud_on_corrupted_frame() {
        // A bit-flip inside a valid frame fails the CRC check: the corruption must
        // surface as an error, not decode to garbage misattributed to Bedrock.
        let mut bytes =
            eventstream_frame(r#"{"type":"content_block_delta","delta":{"text":"hi"}}"#);
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xFF;
        let mut decoder = EventStreamToSse::default();
        let out = decoder.push(&bytes);
        assert!(
            out.contains("event: error"),
            "a corrupted frame must surface an error: {out}"
        );
    }

    #[test]
    fn decoder_flushes_incomplete_trailing_frame_as_error() {
        // The upstream closed after only part of a frame arrived (a truncated stream);
        // finish() must surface a loud error rather than drop the buffered tail.
        let mut decoder = EventStreamToSse::default();
        let full = eventstream_frame(r#"{"type":"content_block_delta","delta":{"text":"hi"}}"#);
        let partial = &full[..full.len() - 5];
        assert_eq!(
            decoder.push(partial),
            "",
            "an incomplete frame emits nothing until it completes"
        );
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
        assert_eq!(
            decoder.finish(),
            "",
            "a clean stream end must not emit an error"
        );
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
        assert!(
            body.contains(r#""text":"pong""#),
            "delta text must survive: {body}"
        );
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
        assert!(
            text.contains(r#""pong""#),
            "non-streaming JSON must pass through: {text}"
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn claude_stream_truncated_midframe_surfaces_error_to_client() {
        // The upstream sends a complete HTTP body that ends mid event-stream frame
        // (a truncated stream). End to end, the client must receive an `event: error`
        // rather than a stream that just stops. This exercises the real
        // Body::from_stream + unfold finish() plumbing that the decoder unit tests
        // (which call finish() directly) do not cover.
        let full =
            eventstream_frame(r#"{"type":"content_block_delta","delta":{"text":"partial"}}"#);
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
    async fn bedrock_path_drops_thinking_display_but_keeps_the_thinking() {
        // Opus 4.1 answers `thinking.enabled.display: Extra inputs are not permitted`,
        // so Claude Code cannot reach it at all while the field is forwarded. Dropping
        // the whole thinking block instead would silently turn extended thinking off.
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
                        !body.contains("display") && body.contains("budget_tokens")
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
                "max_tokens": 2048,
                "thinking": {"type": "enabled", "budget_tokens": 1024, "display": "omitted"},
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
        // The body's `anthropic_beta` can be a single string; merging header
        // betas must keep that string form so a beta the client set on the body
        // still reaches Bedrock alongside the header betas.
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
        assert_eq!(
            resp.text().await.unwrap(),
            sse,
            "Responses SSE must pass through byte-for-byte"
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn gpt5_responses_use_the_openai_prefixed_path() {
        // The GPT-5 family serves on `/openai/v1/responses`, not the `/v1/responses`
        // the open-weight models use. Sending it to the shared path 404s upstream.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/openai/v1/responses")
                    .body_contains("\"openai.gpt-5.6-sol\"");
                then.status(200)
                    .header("content-type", "application/json")
                    .body("{}");
            })
            .await;

        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/responses"))
            .json(&json!({"model":"gpt-5.6-sol","input":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn responses_protocol_translates_to_a_messages_only_claude_model() {
        let server = MockServer::start_async().await;
        let upstream = server
            .mock_async(|when, then| {
                when.method(POST).body_contains("\"messages\"");
                then.status(200).header("content-type", "application/json").json_body(json!({
                    "id": "msg_1", "type": "message", "content": [{ "type": "text", "text": "hello" }],
                    "stop_reason": "end_turn", "usage": { "input_tokens": 2, "output_tokens": 1 }
                }));
            })
            .await;
        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let resp = reqwest::Client::new()
            .post(format!("{url}/llm/v1/responses"))
            .json(&json!({"model":"claude-haiku-4.5","input":[{"role":"user","content":"hi"}]}))
            .send()
            .await
            .expect("proxy request");
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.expect("response body");
        assert_eq!(body["object"], "response");
        assert_eq!(body["output"][0]["content"][0]["text"], "hello");
        upstream.assert_async().await;
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
    async fn embedded_model_listing_is_catalog_only_and_never_invokes_a_model() {
        let server = MockServer::start_async().await;
        let responses = server
            .mock_async(|when, then| {
                when.method(POST).path("/openai/v1/responses");
                then.status(200)
                    .header("content-type", "application/json")
                    .body("{}");
            })
            .await;
        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let body: Value = reqwest::get(format!("{url}/llm/v1/models"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let ids: Vec<&str> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();

        assert!(
            ids.contains(&"byo/gpt-5.6-sol"),
            "byo/gpt-5.6-sol must be listed: {ids:?}"
        );
        assert_eq!(
            responses.hits_async().await,
            0,
            "listing must not spend quota"
        );
        let sol = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "byo/gpt-5.6-sol")
            .expect("byo/gpt-5.6-sol entry");
        assert_eq!(sol["displayName"], "GPT-5.6 Sol");
    }

    #[tokio::test]
    async fn supplied_availability_gates_listing_and_inference_without_probing() {
        let server = MockServer::start_async().await;
        let openai = server
            .mock_async(|when, then| {
                when.method(POST).path("/openai/v1/chat/completions");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"choices":[]}"#);
            })
            .await;
        let availability = HashMap::from([(
            "llm".to_string(),
            HashSet::from(["gpt-oss-20b".to_string()]),
        )]);
        let url = serve(build_router_with_availability(
            vec![aws_route(&server.base_url())],
            availability,
        ))
        .await;

        let resp = reqwest::get(format!("{url}/llm/v1/models")).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["object"], "list");
        let data = body["data"].as_array().unwrap();
        let ids: Vec<&str> = data.iter().map(|m| m["id"].as_str().unwrap()).collect();

        assert_eq!(ids, vec!["byo/gpt-oss-20b"]);
        let gpt = data
            .iter()
            .find(|m| m["id"] == "byo/gpt-oss-20b")
            .expect("byo/gpt-oss-20b entry");
        assert_eq!(gpt["provider"], "openai");
        assert_eq!(gpt["displayName"], "GPT-OSS 20B");
        assert_eq!(
            openai.hits_async().await,
            0,
            "listing must not probe upstream"
        );

        let denied = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({ "model": "gpt-oss-120b", "messages": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            openai.hits_async().await,
            0,
            "blocked inference must stay local"
        );
    }

    #[tokio::test]
    async fn observed_model_blockers_are_actionable_and_unknown_is_retryable() {
        let server = MockServer::start_async().await;
        let observed = HashMap::from([(
            "llm".to_string(),
            HashMap::from([
                (
                    "gpt-oss-20b".to_string(),
                    ObservedModelAvailability::Blocked {
                        reason: "provider agreement required".to_string(),
                    },
                ),
                (
                    "gpt-oss-120b".to_string(),
                    ObservedModelAvailability::Unknown,
                ),
            ]),
        )]);
        let url = serve(build_router_with_observed_models(
            vec![aws_route(&server.base_url())],
            observed,
        ))
        .await;
        let client = reqwest::Client::new();

        let blocked = client
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({ "model": "gpt-oss-20b", "messages": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
        let blocked_body = blocked.text().await.unwrap();
        assert!(blocked_body.contains("GATEWAY_MODEL_ACCESS_REQUIRED"));
        assert!(blocked_body.contains("provider agreement required"));
        assert!(blocked_body.contains("\"retryable\":false"));

        let unknown = client
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({ "model": "gpt-oss-120b", "messages": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(unknown.status(), StatusCode::SERVICE_UNAVAILABLE);
        let unknown_body = unknown.text().await.unwrap();
        assert!(unknown_body.contains("GATEWAY_MODEL_AVAILABILITY_UNKNOWN"));
        assert!(unknown_body.contains("\"retryable\":true"));
    }

    #[tokio::test]
    async fn provider_error_body_never_reaches_the_caller() {
        const PAYLOAD_CANARY: &str = "seeded-prompt-canary-7d84b1";
        const PROVIDER_CANARY: &str = "seeded-provider-secret-canary-942ca0";

        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/openai/v1/chat/completions");
                then.status(400)
                    .header("content-type", "application/json")
                    .body(
                    json!({
                        "error": {
                            "message": format!(
                                "invalid prompt {PAYLOAD_CANARY}; account detail {PROVIDER_CANARY}"
                            )
                        }
                    })
                    .to_string(),
                );
            })
            .await;
        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({
                "model": "gpt-oss-20b",
                "messages": [{ "role": "user", "content": PAYLOAD_CANARY }]
            }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.text().await.expect("safe error body");
        assert!(body.contains("provider_request_rejected"));
        assert!(!body.contains(PAYLOAD_CANARY));
        assert!(!body.contains(PROVIDER_CANARY));
    }

    #[tokio::test]
    async fn provider_authorization_failure_is_connection_unavailable() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/openai/v1/chat/completions");
                then.status(403)
                    .header("content-type", "application/json")
                    .body(r#"{"error":{"message":"provider account 123 is forbidden"}}"#);
            })
            .await;
        let url = serve(build_router(vec![aws_route(&server.base_url())])).await;
        let response = reqwest::Client::new()
            .post(format!("{url}/llm/v1/chat/completions"))
            .json(&json!({ "model": "gpt-oss-20b", "messages": [] }))
            .send()
            .await
            .expect("proxy request");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = response.text().await.expect("safe error body");
        assert!(body.contains("provider_access_unavailable"));
        assert!(!body.contains("account 123"));
    }
}
