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

use aws_credential_types::provider::ProvideCredentials;
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
// Pending-session poll cadence. Used to be 5s; tightened to 1s because the
// CLI's perceived `alien debug` startup latency is bounded by how long it
// takes the agent to notice a freshly-created (or re-attachable) session.
// One outbound poll per second per operator is negligible load on the
// manager and shaves up to ~5s off every cold or post-WS-reset invocation.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

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

/// Wire frames mirror `alien_managerx::providers::debug_session_registry`.
/// Defined locally so the OSS agent doesn't depend on platform crates.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TunnelRequestFrame {
    request_id: String,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body_b64: String,
    /// Where to route the request. Defaults to `Kube` for backward compat
    /// with manager builds that don't yet set the target.
    #[serde(default)]
    target: FrameTarget,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum FrameTarget {
    /// Forward to the in-cluster apiserver (existing kubectl flow).
    #[default]
    Kube,
    /// Forward to a cloud API endpoint after signing with the agent's
    /// in-cluster cloud identity.
    Cloud {
        provider: String,
    },
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
            // Dispatch by target — Kube goes to the apiserver, Cloud goes
            // to the cloud API using the pod's in-cluster identity.
            let frame = match &frame.target {
                FrameTarget::Kube => forward_to_apiserver(apiserver, frame).await,
                FrameTarget::Cloud { provider } => match provider.as_str() {
                    "aws" => forward_to_aws(frame).await,
                    "gcp" => forward_to_gcp(frame).await,
                    "azure" => forward_to_azure(frame).await,
                    other => error_response(
                        frame.request_id.clone(),
                        501,
                        format!("Cloud provider '{other}' not implemented yet"),
                    ),
                },
            };
            if response_tx.send(frame).await.is_err() {
                warn!("Tunnel writer dropped before response could be queued");
            }
        });
    }

    drop(response_tx);
    let _ = writer.await;
    Ok(())
}

/// Forward one cloud-API request frame to the real AWS endpoint, signed
/// with the pod's IRSA-derived credentials. Used in pull-mode K8s
/// deployments where the customer's cluster (and AWS account) are
/// unreachable from the engineer's laptop — the agent's pod IRSA identity
/// is the only identity available, and what it can do is bounded by the
/// IAM role the customer bound to the operator's ServiceAccount.
async fn forward_to_aws(frame: TunnelRequestFrame) -> TunnelResponseFrame {
    use aws_config::BehaviorVersion;

    let request_id = frame.request_id.clone();

    let body = match BASE64.decode(&frame.body_b64) {
        Ok(b) => b,
        Err(e) => return error_response(request_id, 400, format!("undecodable body: {e}")),
    };
    let method = match reqwest::Method::from_bytes(frame.method.as_bytes()) {
        Ok(m) => m,
        Err(e) => return error_response(request_id, 400, format!("invalid method: {e}")),
    };
    let url: reqwest::Url = match frame.path.parse() {
        Ok(u) => u,
        Err(e) => {
            return error_response(
                request_id,
                400,
                format!("invalid target URL '{}': {e}", frame.path),
            )
        }
    };

    // Load IRSA-derived credentials via the AWS SDK default chain. The chain
    // honors `AWS_WEB_IDENTITY_TOKEN_FILE` + `AWS_ROLE_ARN` which is what
    // IRSA-projected pods get from the EKS Pod Identity webhook — same env
    // vars we already saw in your operator pod's spec.
    let aws_config = aws_config::defaults(BehaviorVersion::latest()).load().await;
    let creds_provider = match aws_config.credentials_provider() {
        Some(p) => p,
        None => {
            return error_response(
                request_id,
                500,
                "agent has no AWS credentials provider configured (IRSA not set up?)".to_string(),
            );
        }
    };
    let creds = match creds_provider.provide_credentials().await {
        Ok(c) => c,
        Err(e) => {
            return error_response(request_id, 500, format!("mint AWS credentials: {e}"));
        }
    };

    // Region: prefer what the SDK resolved (region resolver chain), fall
    // back to extracting it from the URL host (`<svc>.<region>.amazonaws.com`).
    let resolved_region = aws_config.region().map(|r| r.as_ref().to_string());
    let (service, signing_region) =
        extract_aws_service_and_region(&url, resolved_region.as_deref().unwrap_or("us-east-1"));

    // SigV4 is exquisitely sensitive to any header that's in the SignedHeaders
    // list but whose value differs on the wire vs. what we hashed. To remove
    // that whole class of bug, we only carry through `content-type` (which AWS
    // requires for some payloads and which reqwest preserves verbatim) and
    // let the SigV4 library add the rest (`host`, `x-amz-date`,
    // `x-amz-content-sha256`, `x-amz-security-token`, `authorization`).
    //
    // Notable strips:
    // - `host` / `authorization` / `x-amz-*`: re-issued by the signer.
    // - `content-length`: reqwest computes it from the body; if our forwarded
    //   value mismatched (e.g. body re-encoding through the WS), the wire
    //   value would differ from the signed value → SignatureDoesNotMatch.
    // - `user-agent`: AWS CLI's UA isn't needed for signature verification on
    //   AWS's side; keeping it in the signed set just risks reqwest tweaking
    //   the value at send time.
    // - `accept-encoding` / `accept` / `expect`: reqwest sets its own.
    // - `connection` / `upgrade` / `te` / `transfer-encoding`: hop-by-hop.
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in &frame.headers {
        let Ok(n) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) else { continue };
        let Ok(v) = reqwest::header::HeaderValue::from_str(value) else { continue };
        let lower = n.as_str().to_ascii_lowercase();
        // Allowlist: only forward `content-type` from the source request.
        if lower != "content-type" {
            continue;
        }
        headers.insert(n, v);
    }

    if let Err(e) = aws_sigv4_sign(&creds, &method, &url, &mut headers, &body, service, &signing_region)
    {
        return error_response(request_id, 500, format!("SigV4 sign: {e}"));
    }

    // Use a long-lived HTTP/1.1 client so reqwest doesn't surprise us with
    // HTTP/2 frame layouts that some signing edge-cases trip on. (Also: no
    // default headers, so the signed set is exactly what hits the wire.)
    let resp = match build_aws_outbound_client()
        .request(method, url)
        .headers(headers)
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return error_response(request_id, 502, format!("AWS endpoint: {e}")),
    };

    let status = resp.status().as_u16();
    let resp_headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_string(), value.to_str().ok()?.to_string())))
        .collect();
    let body_bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return error_response(request_id, 502, format!("read body: {e}")),
    };

    TunnelResponseFrame {
        request_id,
        status,
        headers: resp_headers,
        body_b64: BASE64.encode(&body_bytes),
    }
}

fn aws_sigv4_sign(
    creds: &aws_credential_types::Credentials,
    method: &reqwest::Method,
    url: &reqwest::Url,
    headers: &mut reqwest::header::HeaderMap,
    body: &[u8],
    service: &str,
    signing_region: &str,
) -> Result<(), String> {
    use aws_sigv4::{
        http_request::{sign, SignableBody, SignableRequest, SigningSettings},
        sign::v4,
    };

    let identity = creds.clone().into();
    let signing_settings = SigningSettings::default();
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .region(signing_region)
        .name(service)
        .time(std::time::SystemTime::now())
        .settings(signing_settings)
        .build()
        .map_err(|e| format!("SigV4 params: {e}"))?
        .into();

    let header_pairs: Vec<(&str, &str)> = headers
        .iter()
        .filter_map(|(k, v)| Some((k.as_str(), v.to_str().ok()?)))
        .collect();

    let signable = SignableRequest::new(
        method.as_str(),
        url.as_str().to_string(),
        header_pairs.into_iter(),
        SignableBody::Bytes(body),
    )
    .map_err(|e| format!("SignableRequest: {e}"))?;

    let (instructions, _sig) =
        sign(signable, &signing_params).map_err(|e| format!("sign: {e}"))?.into_parts();

    for (name, value) in instructions.headers() {
        let Ok(n) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) else { continue };
        let Ok(v) = reqwest::header::HeaderValue::from_str(value) else { continue };
        headers.insert(n, v);
    }
    Ok(())
}

/// Derive `(service, region)` from an AWS endpoint URL host.
fn extract_aws_service_and_region(url: &reqwest::Url, fallback_region: &str) -> (&'static str, String) {
    let host = url.host_str().unwrap_or("");
    let labels: Vec<&str> = host.split('.').collect();
    let amz_idx = labels.iter().rposition(|l| *l == "amazonaws");
    let Some(amz_idx) = amz_idx else {
        return ("execute-api", fallback_region.to_string());
    };
    let pre_amz: Vec<&str> = labels[..amz_idx].iter().copied().collect();
    let (service, region) = match pre_amz.as_slice() {
        [_bucket_or_subdomain @ .., service, region]
            if region.contains('-') && service.len() <= 8 =>
        {
            (*service, region.to_string())
        }
        [_subdomain @ .., service] => (*service, fallback_region.to_string()),
        _ => ("execute-api", fallback_region.to_string()),
    };
    let static_service: &'static str = match service {
        "sts" => "sts",
        "iam" => "iam",
        "ec2" => "ec2",
        "lambda" => "lambda",
        "s3" => "s3",
        "dynamodb" => "dynamodb",
        "sqs" => "sqs",
        "sns" => "sns",
        "ecr" => "ecr",
        "eks" => "eks",
        "ecs" => "ecs",
        "cloudformation" => "cloudformation",
        "cloudwatch" => "monitoring",
        "logs" => "logs",
        "ssm" => "ssm",
        "secretsmanager" => "secretsmanager",
        "kms" => "kms",
        "events" | "eventbridge" => "events",
        "apigateway" => "apigateway",
        "execute-api" => "execute-api",
        _ => "execute-api",
    };
    (static_service, region)
}

/// Forward a cloud-API request frame to a GCP endpoint, signed with the
/// pod's GKE Workload Identity bearer token. The pod must have the WI
/// webhook's projected JWT mounted; we exchange it at Google's STS for a
/// short-lived OAuth2 access token and attach as `Authorization: Bearer …`.
async fn forward_to_gcp(frame: TunnelRequestFrame) -> TunnelResponseFrame {
    let request_id = frame.request_id.clone();

    let body = match BASE64.decode(&frame.body_b64) {
        Ok(b) => b,
        Err(e) => return error_response(request_id, 400, format!("undecodable body: {e}")),
    };
    let method = match reqwest::Method::from_bytes(frame.method.as_bytes()) {
        Ok(m) => m,
        Err(e) => return error_response(request_id, 400, format!("invalid method: {e}")),
    };
    let url: reqwest::Url = match frame.path.parse() {
        Ok(u) => u,
        Err(e) => {
            return error_response(request_id, 400, format!("invalid target URL '{}': {e}", frame.path))
        }
    };

    // GKE Workload Identity projects a short-lived KSA JWT at this path by
    // default. The Google Cloud "Workload Identity Federation" pool exchange
    // accepts that token at sts.googleapis.com and returns an OAuth2 access
    // token usable on `*.googleapis.com`.
    let projected_token_path = std::env::var("GCP_PROJECTED_TOKEN_FILE")
        .unwrap_or_else(|_| "/var/run/secrets/tokens/gcp-ksa/token".to_string());
    let projected_jwt = match std::fs::read_to_string(&projected_token_path) {
        Ok(s) => s.trim().to_string(),
        Err(e) => {
            return error_response(
                request_id,
                500,
                format!("read GCP projected token at {projected_token_path}: {e}"),
            )
        }
    };

    let audience = match std::env::var("GCP_WORKLOAD_IDENTITY_AUDIENCE") {
        Ok(a) if !a.is_empty() => a,
        _ => {
            return error_response(
                request_id,
                500,
                "GCP_WORKLOAD_IDENTITY_AUDIENCE not set; the helm chart's serviceAccount.annotations must include iam.gke.io/gcp-service-account + workload-identity-pool config".to_string(),
            )
        }
    };

    let access_token = match exchange_gcp_sts(&projected_jwt, &audience).await {
        Ok(t) => t,
        Err(e) => return error_response(request_id, 502, format!("GCP STS exchange: {e}")),
    };

    let mut headers = build_cloud_headers(&frame.headers);
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {access_token}"))
            .expect("bearer value valid"),
    );

    send_cloud_request(request_id, method, url, headers, body).await
}

/// Forward a cloud-API request frame to an Azure endpoint, signed with the
/// pod's Workload Identity federated token. Exchanges the projected JWT for
/// an AAD access token at `login.microsoftonline.com`.
async fn forward_to_azure(frame: TunnelRequestFrame) -> TunnelResponseFrame {
    let request_id = frame.request_id.clone();

    let body = match BASE64.decode(&frame.body_b64) {
        Ok(b) => b,
        Err(e) => return error_response(request_id, 400, format!("undecodable body: {e}")),
    };
    let method = match reqwest::Method::from_bytes(frame.method.as_bytes()) {
        Ok(m) => m,
        Err(e) => return error_response(request_id, 400, format!("invalid method: {e}")),
    };
    let url: reqwest::Url = match frame.path.parse() {
        Ok(u) => u,
        Err(e) => {
            return error_response(request_id, 400, format!("invalid target URL '{}': {e}", frame.path))
        }
    };

    // Azure Workload Identity standard env vars (set by the WI webhook).
    let federated_token_path = match std::env::var("AZURE_FEDERATED_TOKEN_FILE") {
        Ok(p) if !p.is_empty() => p,
        _ => return error_response(request_id, 500, "AZURE_FEDERATED_TOKEN_FILE not set".to_string()),
    };
    let client_id = match std::env::var("AZURE_CLIENT_ID") {
        Ok(c) if !c.is_empty() => c,
        _ => return error_response(request_id, 500, "AZURE_CLIENT_ID not set".to_string()),
    };
    let tenant_id = match std::env::var("AZURE_TENANT_ID") {
        Ok(t) if !t.is_empty() => t,
        _ => return error_response(request_id, 500, "AZURE_TENANT_ID not set".to_string()),
    };

    let assertion = match std::fs::read_to_string(&federated_token_path) {
        Ok(s) => s.trim().to_string(),
        Err(e) => {
            return error_response(
                request_id,
                500,
                format!("read Azure federated token at {federated_token_path}: {e}"),
            )
        }
    };

    // Scope: `<resource>/.default`, derived from the target URL host so a
    // single call to ARM, Storage, Key Vault etc. each get the right token.
    let scope = format!(
        "{}://{}/.default",
        url.scheme(),
        url.host_str().unwrap_or("management.azure.com")
    );

    let access_token =
        match exchange_azure_aad(&tenant_id, &client_id, &assertion, &scope).await {
            Ok(t) => t,
            Err(e) => return error_response(request_id, 502, format!("Azure AAD exchange: {e}")),
        };

    let mut headers = build_cloud_headers(&frame.headers);
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {access_token}"))
            .expect("bearer value valid"),
    );

    send_cloud_request(request_id, method, url, headers, body).await
}

/// Copy inbound frame headers, dropping host/auth/x-amz-* (loopback-side
/// placeholders). Used by the GCP/Azure forwarders before they attach the
/// real bearer.
fn build_cloud_headers(input: &[(String, String)]) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in input {
        let Ok(n) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) else { continue };
        let Ok(v) = reqwest::header::HeaderValue::from_str(value) else { continue };
        let lower = n.as_str().to_ascii_lowercase();
        if lower == "authorization"
            || lower == "host"
            || lower.starts_with("x-amz-")
            || lower == "connection"
            || lower == "upgrade"
        {
            continue;
        }
        headers.insert(n, v);
    }
    headers
}

/// Send a cloud request and wrap the result in a response frame.
async fn send_cloud_request(
    request_id: String,
    method: reqwest::Method,
    url: reqwest::Url,
    headers: reqwest::header::HeaderMap,
    body: Vec<u8>,
) -> TunnelResponseFrame {
    let resp = match reqwest::Client::new()
        .request(method, url)
        .headers(headers)
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return error_response(request_id, 502, format!("cloud endpoint: {e}")),
    };

    let status = resp.status().as_u16();
    let resp_headers: Vec<(String, String)> = resp
        .headers()
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_string(), value.to_str().ok()?.to_string())))
        .collect();
    let body_bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return error_response(request_id, 502, format!("read body: {e}")),
    };

    TunnelResponseFrame {
        request_id,
        status,
        headers: resp_headers,
        body_b64: BASE64.encode(&body_bytes),
    }
}

/// GCP Workload Identity STS exchange: trade a projected KSA JWT for a
/// short-lived OAuth2 access token usable on `*.googleapis.com`.
async fn exchange_gcp_sts(projected_jwt: &str, audience: &str) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
    }
    let form = [
        ("audience", audience),
        ("grant_type", "urn:ietf:params:oauth:grant-type:token-exchange"),
        ("requested_token_type", "urn:ietf:params:oauth:token-type:access_token"),
        ("scope", "https://www.googleapis.com/auth/cloud-platform"),
        ("subject_token_type", "urn:ietf:params:oauth:token-type:jwt"),
        ("subject_token", projected_jwt),
    ];
    let resp = reqwest::Client::new()
        .post("https://sts.googleapis.com/v1/token")
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("sts request: {e}"))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {s}: {body}"));
    }
    let token: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("parse sts response: {e}"))?;
    Ok(token.access_token)
}

/// Azure Workload Identity AAD exchange: trade a federated client assertion
/// for an AAD access token scoped to the target resource.
async fn exchange_azure_aad(
    tenant_id: &str,
    client_id: &str,
    assertion: &str,
    scope: &str,
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
    }
    let form = [
        ("client_id", client_id),
        ("scope", scope),
        ("client_assertion_type", "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"),
        ("client_assertion", assertion),
        ("grant_type", "client_credentials"),
    ];
    let url = format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token");
    let resp = reqwest::Client::new()
        .post(&url)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("aad request: {e}"))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {s}: {body}"));
    }
    let token: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("parse aad response: {e}"))?;
    Ok(token.access_token)
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

/// HTTP client used by the SigV4 forward path. Pinned to HTTP/1.1 (some AWS
/// endpoints respond differently under HTTP/2's HPACK header compression with
/// strict signing) and explicitly disables reqwest's gzip / brotli /
/// `Accept-Encoding` defaults so the request that hits the wire has the
/// exact header set we signed — nothing more, nothing less.
fn build_aws_outbound_client() -> reqwest::Client {
    reqwest::Client::builder()
        .http1_only()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
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
