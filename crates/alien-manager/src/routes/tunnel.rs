//! Reverse HTTP tunnel into deployments.
//!
//! Operators inside deployments open `GET /v1/tunnel/connect` (a WebSocket
//! carrying HTTP/2, see `alien-tunnel`). Callers then reach a container's
//! declared tunnel port with
//! `ANY /v1/deployments/{deployment}/tunnels/{container}/{*path}`; the request
//! and response stream over the operator's outbound connection.
//!
//! Callers authenticate to the manager with `Proxy-Authorization`, so the
//! request's own `Authorization` header reaches the service untouched.
//!
//! These routes exist only when the manager is built with
//! [`crate::AlienManagerBuilder::tunnels`].

use std::sync::Arc;

use alien_error::AlienError;
use alien_tunnel::{
    manager::{TunnelBody, TunnelRegistry},
    websocket_io, ERROR_HEADER, SUBPROTOCOL,
};
use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use tokio_tungstenite::{
    tungstenite::{handshake::derive_accept_key, protocol::Role},
    WebSocketStream,
};
use tracing::{debug, warn};

use super::{auth, AppState};
use crate::{
    auth::{Scope, Subject},
    error::ErrorData,
    traits::{DeploymentFilter, DeploymentRecord},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(alien_tunnel::CONNECT_PATH, get(connect))
        .route(
            "/v1/deployments/{deployment}/tunnels/{resource}",
            any(forward_root),
        )
        .route(
            "/v1/deployments/{deployment}/tunnels/{resource}/{*path}",
            any(forward_path),
        )
}

fn registry(state: &AppState) -> &Arc<TunnelRegistry> {
    state
        .tunnels
        .as_ref()
        .expect("tunnel routes are mounted only when tunnels are enabled")
}

// ---------------------------------------------------------------------------
// Operator side
// ---------------------------------------------------------------------------

/// Accept a tunnel connection from an operator (deployment token required).
async fn connect(State(state): State<AppState>, mut request: Request) -> Response {
    let subject = match auth::require_auth(&state, request.headers()).await {
        Ok(subject) => subject,
        Err(e) => return e.into_response(),
    };
    let deployment_id = match &subject.scope {
        Scope::Deployment { deployment_id, .. } => deployment_id.clone(),
        _ => {
            return ErrorData::forbidden("Tunnel connections require a deployment token")
                .into_response()
        }
    };

    let headers = request.headers();
    let header_has = |name: HeaderName, needle: &str| {
        headers.get_all(name).iter().any(|value| {
            value.to_str().is_ok_and(|v| {
                v.split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case(needle))
            })
        })
    };
    if !header_has(header::UPGRADE, "websocket") || !header_has(header::CONNECTION, "upgrade") {
        return ErrorData::bad_request("Tunnel connections must upgrade to WebSocket")
            .into_response();
    }
    if !header_has(header::SEC_WEBSOCKET_PROTOCOL, SUBPROTOCOL) {
        return ErrorData::bad_request(format!(
            "Tunnel connections must use the '{SUBPROTOCOL}' WebSocket subprotocol"
        ))
        .into_response();
    }
    if headers
        .get(header::SEC_WEBSOCKET_VERSION)
        .map(|v| v.as_bytes())
        != Some(b"13")
    {
        return ErrorData::bad_request("Unsupported WebSocket version").into_response();
    }
    let Some(key) = headers.get(header::SEC_WEBSOCKET_KEY) else {
        return ErrorData::bad_request("Missing Sec-WebSocket-Key").into_response();
    };
    let accept = derive_accept_key(key.as_bytes());

    let on_upgrade = hyper::upgrade::on(&mut request);
    let registry = registry(&state).clone();
    tokio::spawn(async move {
        let upgraded = match on_upgrade.await {
            Ok(upgraded) => upgraded,
            Err(e) => {
                warn!(deployment_id = %deployment_id, error = %e, "Tunnel upgrade failed");
                return;
            }
        };
        let ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, None).await;
        if let Err(e) = registry
            .serve(deployment_id.clone(), websocket_io(ws))
            .await
        {
            debug!(deployment_id = %deployment_id, error = %e, "Tunnel connection ended");
        }
    });

    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(header::UPGRADE, "websocket")
        .header(header::CONNECTION, "Upgrade")
        .header(header::SEC_WEBSOCKET_ACCEPT, accept)
        .header(header::SEC_WEBSOCKET_PROTOCOL, SUBPROTOCOL)
        .body(Body::empty())
        .expect("static upgrade response is valid")
}

// ---------------------------------------------------------------------------
// Caller side
// ---------------------------------------------------------------------------

async fn forward_root(
    State(state): State<AppState>,
    Path((deployment, resource)): Path<(String, String)>,
    request: Request,
) -> Response {
    forward(state, deployment, resource, request).await
}

async fn forward_path(
    State(state): State<AppState>,
    Path((deployment, resource, _path)): Path<(String, String, String)>,
    request: Request,
) -> Response {
    forward(state, deployment, resource, request).await
}

async fn forward(
    state: AppState,
    deployment: String,
    resource: String,
    request: Request,
) -> Response {
    let subject = match authenticate_caller(&state, request.headers()).await {
        Ok(subject) => subject,
        Err(response) => return response,
    };
    let record = match find_deployment(&state, &subject, &deployment).await {
        Ok(record) => record,
        Err(response) => return response,
    };
    if !state.authz.can_call_tunnel(&subject, &record) {
        return ErrorData::forbidden("Caller cannot use this deployment's tunnel").into_response();
    }

    let prefix = format!("/v1/deployments/{deployment}/tunnels/{resource}");
    let (mut parts, body) = request.into_parts();
    let rest = parts
        .uri
        .path()
        .strip_prefix(&prefix)
        .filter(|rest| !rest.is_empty())
        .unwrap_or("/")
        .to_string();
    let path_and_query = match parts.uri.query() {
        Some(query) => format!("{rest}?{query}"),
        None => rest,
    };
    // The target container rides in the authority; the operator resolves it
    // against the deployment's stack.
    parts.uri = match Uri::builder()
        .scheme("http")
        .authority(resource.as_str())
        .path_and_query(path_and_query)
        .build()
    {
        Ok(uri) => uri,
        Err(e) => {
            return ErrorData::bad_request(format!("Invalid tunnel target: {e}")).into_response()
        }
    };
    parts.version = http::Version::HTTP_2;
    strip_hop_by_hop(&mut parts.headers);
    parts.headers.remove(header::PROXY_AUTHORIZATION);
    add_forwarded_headers(&mut parts.headers, &state);

    let body: TunnelBody = body
        .map_err(|e| Box::new(e) as alien_tunnel::manager::BoxError)
        .boxed_unsync();
    match registry(&state)
        .send(&record.id, http::Request::from_parts(parts, body))
        .await
    {
        Ok(response) => {
            let (mut parts, body) = response.into_parts();
            strip_hop_by_hop(&mut parts.headers);
            Response::from_parts(parts, Body::new(body))
        }
        Err(e) => tunnel_error_response(e),
    }
}

/// Tunnel callers authenticate with `Proxy-Authorization`, leaving
/// `Authorization` to the application behind the tunnel.
async fn authenticate_caller(state: &AppState, headers: &HeaderMap) -> Result<Subject, Response> {
    let Some(credentials) = headers.get(header::PROXY_AUTHORIZATION) else {
        return Err(ErrorData::unauthorized(
            "Tunnel requests authenticate with 'Proxy-Authorization: Bearer <token>'",
        )
        .into_response());
    };
    let mut auth_headers = HeaderMap::new();
    auth_headers.insert(header::AUTHORIZATION, credentials.clone());
    auth::require_auth(state, &auth_headers)
        .await
        .map_err(IntoResponse::into_response)
}

/// Resolve a deployment by ID, or by name when the name is unique among the
/// deployments the caller can see.
async fn find_deployment(
    state: &AppState,
    subject: &Subject,
    deployment: &str,
) -> Result<DeploymentRecord, Response> {
    let not_found = || {
        AlienError::new(ErrorData::DeploymentNotFound {
            deployment_id: deployment.to_string(),
        })
        .into_response()
    };
    let store_failed = |e: AlienError| {
        ErrorData::internal(format!("Failed to look up deployment: {e}")).into_response()
    };
    if let Some(record) = state
        .deployment_store
        .get_deployment(subject, deployment)
        .await
        .map_err(store_failed)?
    {
        return Ok(record);
    }
    let mut matches = state
        .deployment_store
        .list_deployments(
            subject,
            &DeploymentFilter {
                name: Some(deployment.to_string()),
                ..Default::default()
            },
        )
        .await
        .map_err(store_failed)?;
    matches.retain(|record| record.name == deployment);
    match matches.len() {
        0 => Err(not_found()),
        1 => Ok(matches.remove(0)),
        _ => Err(ErrorData::bad_request(format!(
            "Deployment name '{deployment}' is used in several deployment groups; use the deployment ID"
        ))
        .into_response()),
    }
}

/// Headers that describe one hop and must not be forwarded (RFC 9110 §7.6.1).
fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let listed: Vec<HeaderName> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|name| HeaderName::try_from(name.trim()).ok())
        .collect();
    for name in listed {
        headers.remove(name);
    }
    for name in [
        header::CONNECTION,
        header::PROXY_AUTHENTICATE,
        header::TE,
        header::TRAILER,
        header::TRANSFER_ENCODING,
        header::UPGRADE,
        HeaderName::from_static("keep-alive"),
        HeaderName::from_static("proxy-connection"),
    ] {
        headers.remove(name);
    }
}

fn add_forwarded_headers(headers: &mut HeaderMap, state: &AppState) {
    let base_url = state.config.base_url();
    if let Ok(url) = url::Url::parse(&base_url) {
        if let Ok(proto) = HeaderValue::from_str(url.scheme()) {
            headers.insert("x-forwarded-proto", proto);
        }
        if let Some(host) = url.host_str() {
            let host = match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            };
            if let Ok(host) = HeaderValue::from_str(&host) {
                headers.insert("x-forwarded-host", host);
            }
        }
    }
}

fn tunnel_error_response(error: AlienError<alien_tunnel::ErrorData>) -> Response {
    let status = error
        .http_status_code
        .and_then(|code| StatusCode::from_u16(code).ok())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let code = error.code.clone();
    let body = serde_json::json!({ "code": code, "message": error.message });
    let mut response = (status, axum::Json(body)).into_response();
    if let Ok(value) = HeaderValue::from_str(&code) {
        response.headers_mut().insert(ERROR_HEADER, value);
    }
    response
}
