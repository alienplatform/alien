//! Pull-mode `alien debug` tunnel loop.
//!
//! Polls the manager's `GET /v1/debug/sessions/pending?deploymentId=...`
//! endpoint. When a debug session is pending for this deployment, dials the
//! per-session WebSocket and forwards kubectl request frames to the
//! in-cluster apiserver, using the agent pod's own ServiceAccount token + CA
//! cert.
//!
//! Design rationale lives in
//! `platform/crates/alien-managerx/src/routes/debug.rs`. Summary:
//!
//! - The manager has no path to the customer's cluster (private endpoint, the
//!   customer's AWS account, no creds).
//! - The agent runs inside the cluster, can reach the apiserver via
//!   `https://kubernetes.default.svc`, and dials *out* to the manager — the
//!   only network direction that doesn't require inbound exposure.
//! - Effective RBAC = whatever the operator's ServiceAccount already has. No
//!   new chart RBAC needed; debug power scales with deployment power.
//!
//! Scope of this MVP:
//! - One inbound kubectl request → one outbound apiserver request → one
//!   response frame back. Buffered, not streamed.
//! - No support yet for `kubectl logs -f`, `exec`, `port-forward`, `cp` —
//!   those require HTTP upgrade negotiation through the tunnel. Phase 2.

use std::path::Path as StdPath;
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures::{SinkExt, StreamExt};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use tracing::{debug, error, info, warn};

use crate::AgentState;

/// How often the loop checks the manager for pending debug sessions when
/// idle. Cheap GET; 5s is responsive enough for an interactive workflow
/// (`alien debug` waits at most this long before the agent picks up).
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Standard in-cluster paths Kubernetes projects into every pod.
const SA_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";
const SA_CA_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt";
const SA_NAMESPACE_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/namespace";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimResponse {
    session_id: String,
    agent_token: String,
    tunnel_url: String,
}

/// Wire frames mirror [`alien_managerx::providers::debug_session_registry`].
/// Defined locally so the OSS agent doesn't depend on platform crates.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TunnelRequestFrame {
    request_id: String,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body_b64: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TunnelResponseFrame {
    request_id: String,
    status: u16,
    headers: Vec<(String, String)>,
    body_b64: String,
}

/// Run the debug-session loop forever. Pulls the manager URL + agent token
/// from `state.config.sync` (no-op when the agent is airgapped) and resolves
/// the deployment id from the local DB.
pub async fn run_debug_session_loop(state: std::sync::Arc<AgentState>) {
    let Some(sync) = state.config.sync.clone() else {
        debug!("Airgapped — debug-session loop disabled");
        return;
    };
    let manager_url = sync.url.to_string();
    let manager_bearer = sync.token.clone();

    // Wait until the agent has been registered and knows its deployment id.
    // The sync loop writes this on first successful sync; until then there's
    // nothing to scope debug sessions to.
    let deployment_id = loop {
        match state.db.get_deployment_id().await {
            Ok(Some(id)) => break id,
            Ok(None) => {
                debug!("Debug-session loop waiting for deployment registration");
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            Err(e) => {
                warn!(error = %e, "Failed to read deployment id from agent DB");
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    };
    info!(deployment_id = %deployment_id, "Debug-session loop started");

    let http = match build_manager_http_client() {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to build manager HTTP client; debug loop disabled");
            return;
        }
    };

    let apiserver_client = match build_apiserver_client() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            warn!(
                error = %e,
                "Failed to build in-cluster apiserver client; debug-session loop will keep \
                 polling and retry on each session, but kubectl calls will fail until this \
                 resolves. This usually means the pod is missing the projected ServiceAccount \
                 token at {SA_TOKEN_PATH} or CA cert at {SA_CA_PATH}."
            );
            return;
        }
    };

    loop {
        match claim_next_session(&http, &manager_url, &deployment_id, &manager_bearer).await {
            Ok(Some(claim)) => {
                info!(session_id = %claim.session_id, "Picked up pending debug session");
                if let Err(e) = serve_session(&claim, Arc::clone(&apiserver_client)).await {
                    warn!(
                        session_id = %claim.session_id,
                        error = %e,
                        "Debug session ended with error"
                    );
                } else {
                    info!(session_id = %claim.session_id, "Debug session ended cleanly");
                }
                // Immediately re-poll in case more sessions are queued.
                continue;
            }
            Ok(None) => {}
            Err(e) => {
                debug!(error = %e, "Debug session poll failed (will retry)");
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn claim_next_session(
    http: &reqwest::Client,
    manager_url: &str,
    deployment_id: &str,
    bearer: &str,
) -> Result<Option<ClaimResponse>, String> {
    let url = format!(
        "{}/v1/debug/sessions/pending?deploymentId={}",
        manager_url.trim_end_matches('/'),
        deployment_id
    );
    let resp = http
        .get(&url)
        .bearer_auth(bearer)
        .send()
        .await
        .map_err(|e| format!("poll {url}: {e}"))?;

    if resp.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("poll {url}: HTTP {status} {body}"));
    }
    let claim: ClaimResponse = resp
        .json()
        .await
        .map_err(|e| format!("decode claim response: {e}"))?;
    Ok(Some(claim))
}

async fn serve_session(
    claim: &ClaimResponse,
    apiserver: Arc<reqwest::Client>,
) -> Result<(), String> {
    // The tunnel URL the manager handed us is the manager's external URL with
    // an `https://` scheme. Convert to `wss://` for the WebSocket dial and
    // append the agent-token query parameter.
    let ws_url = http_to_ws_url(&claim.tunnel_url)?;
    let ws_url_with_token = format!("{}?token={}", ws_url, urlencoding(&claim.agent_token));

    let request = ws_url_with_token
        .as_str()
        .into_client_request()
        .map_err(|e| format!("invalid tunnel URL '{ws_url_with_token}': {e}"))?;

    let (ws_stream, _) = connect_async(request)
        .await
        .map_err(|e| format!("dial tunnel: {e}"))?;

    info!(
        session_id = %claim.session_id,
        tunnel_url = %ws_url,
        "Tunnel WebSocket connected"
    );

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // Outbound response frames are produced concurrently by per-request
    // handler tasks; one writer task drains them into the WebSocket.
    let (response_tx, mut response_rx) = mpsc::channel::<TunnelResponseFrame>(64);

    let writer = tokio::spawn(async move {
        while let Some(frame) = response_rx.recv().await {
            let text = match serde_json::to_string(&frame) {
                Ok(t) => t,
                Err(e) => {
                    warn!(error = %e, "Failed to serialize tunnel response frame");
                    continue;
                }
            };
            if let Err(e) = ws_sink.send(Message::Text(text.into())).await {
                warn!(error = %e, "Failed to write tunnel frame to WebSocket");
                break;
            }
        }
        let _ = ws_sink.close().await;
    });

    while let Some(msg) = ws_stream.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "Tunnel WebSocket read error");
                break;
            }
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(b) => String::from_utf8(b.to_vec()).map(Into::into).unwrap_or_default(),
            Message::Close(_) => break,
            _ => continue,
        };
        let frame: TunnelRequestFrame = match serde_json::from_str(&text) {
            Ok(f) => f,
            Err(e) => {
                warn!(error = %e, "Manager sent malformed request frame");
                continue;
            }
        };

        let response_tx = response_tx.clone();
        let apiserver = Arc::clone(&apiserver);
        tokio::spawn(async move {
            let frame = forward_to_apiserver(apiserver, frame).await;
            if response_tx.send(frame).await.is_err() {
                warn!("Tunnel writer dropped before response could be queued");
            }
        });
    }

    drop(response_tx);
    let _ = writer.await;
    Ok(())
}

/// Forward one kubectl request frame to the in-cluster apiserver, build a
/// response frame from the result (or an error response on failure).
async fn forward_to_apiserver(
    apiserver: Arc<reqwest::Client>,
    frame: TunnelRequestFrame,
) -> TunnelResponseFrame {
    let request_id = frame.request_id.clone();

    let body = match BASE64.decode(&frame.body_b64) {
        Ok(b) => b,
        Err(e) => {
            return error_response(request_id, 400, format!("undecodable body: {e}"));
        }
    };

    let method = match reqwest::Method::from_bytes(frame.method.as_bytes()) {
        Ok(m) => m,
        Err(e) => {
            return error_response(request_id, 400, format!("invalid method: {e}"));
        }
    };

    let mut headers = HeaderMap::new();
    for (name, value) in &frame.headers {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else { continue };
        let Ok(value) = HeaderValue::from_str(value) else { continue };
        headers.insert(name, value);
    }

    // Re-authenticate with the pod's SA token. The manager's proxy stripped
    // the inbound Authorization header so we can layer our own credentials
    // here without leaking the manager-side bearer to the apiserver.
    match load_sa_token() {
        Ok(token) => {
            if let Ok(v) = HeaderValue::from_str(&format!("Bearer {token}")) {
                headers.insert(AUTHORIZATION, v);
            }
        }
        Err(e) => {
            return error_response(request_id, 500, format!("missing SA token: {e}"));
        }
    }

    let url = format!("https://kubernetes.default.svc{}", frame.path);

    let response = apiserver
        .request(method, &url)
        .headers(headers)
        .body(body)
        .send()
        .await;

    let response = match response {
        Ok(r) => r,
        Err(e) => {
            return error_response(request_id, 502, format!("apiserver: {e}"));
        }
    };

    let status = response.status().as_u16();
    let resp_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            let v = value.to_str().ok()?;
            Some((name.as_str().to_string(), v.to_string()))
        })
        .collect();
    let body_bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return error_response(request_id, 502, format!("read response body: {e}"));
        }
    };

    TunnelResponseFrame {
        request_id,
        status,
        headers: resp_headers,
        body_b64: BASE64.encode(&body_bytes),
    }
}

fn error_response(request_id: String, status: u16, message: String) -> TunnelResponseFrame {
    let body = serde_json::json!({
        "kind": "Status",
        "apiVersion": "v1",
        "status": "Failure",
        "message": message,
        "code": status,
    });
    TunnelResponseFrame {
        request_id,
        status,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body_b64: BASE64.encode(body.to_string().as_bytes()),
    }
}

fn build_manager_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("build manager client: {e}"))
}

fn build_apiserver_client() -> Result<reqwest::Client, String> {
    let ca_bytes = std::fs::read(SA_CA_PATH)
        .map_err(|e| format!("read in-cluster CA at {SA_CA_PATH}: {e}"))?;
    let cert = reqwest::Certificate::from_pem(&ca_bytes)
        .map_err(|e| format!("parse in-cluster CA: {e}"))?;
    reqwest::Client::builder()
        .add_root_certificate(cert)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("build apiserver client: {e}"))
}

fn load_sa_token() -> Result<String, String> {
    let raw = std::fs::read_to_string(SA_TOKEN_PATH)
        .map_err(|e| format!("read SA token at {SA_TOKEN_PATH}: {e}"))?;
    Ok(raw.trim().to_string())
}

/// Read the pod's projected namespace (kept for future use when the agent
/// wants to surface the default-namespace hint back to the manager).
#[allow(dead_code)]
fn pod_namespace() -> Option<String> {
    std::fs::read_to_string(SA_NAMESPACE_PATH)
        .ok()
        .map(|s| s.trim().to_string())
}

fn http_to_ws_url(url: &str) -> Result<String, String> {
    if let Some(rest) = url.strip_prefix("https://") {
        Ok(format!("wss://{rest}"))
    } else if let Some(rest) = url.strip_prefix("http://") {
        Ok(format!("ws://{rest}"))
    } else {
        Err(format!("tunnel URL '{url}' must start with http(s)://"))
    }
}

fn urlencoding(input: &str) -> String {
    // Minimal URL-encode for the agent-token query parameter. The token is
    // generated as `<prefix>_<uuid_simple>`, both of which are URL-safe;
    // implemented anyway so this stays correct if the token shape ever changes.
    input
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{:02X}", other),
        })
        .collect()
}

// Silence unused-path warnings on platforms where this isn't conditionally
// compiled — the loop is platform-agnostic but `pod_namespace` is currently
// only reachable through a future hook.
#[allow(dead_code)]
fn _absolute_paths_marker() -> (&'static StdPath, &'static StdPath, &'static StdPath) {
    (
        StdPath::new(SA_TOKEN_PATH),
        StdPath::new(SA_CA_PATH),
        StdPath::new(SA_NAMESPACE_PATH),
    )
}
