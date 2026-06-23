//! Push-mode debug tunnel: CLI half.
//!
//! When `POST /v1/debug/sessions` returns a [`PushTunnelDebugSession`], the
//! CLI:
//!
//! 1. Dials the manager's WebSocket at `tunnel_url` with the session's
//!    `client_token`. The manager attaches a `TunnelHandle` to the registry
//!    row and is now waiting for request frames over the WS.
//! 2. Spawns a local HTTP server bound to `127.0.0.1:<ephemeral-port>` —
//!    the "loopback proxy". For every inbound HTTP request, the proxy
//!    serializes the request as a [`TunnelRequestFrame`], sends it over the
//!    WebSocket, and waits for the matching [`TunnelResponseFrame`].
//! 3. Sets `AWS_ENDPOINT_URL` (and GCP/Azure equivalents) to the loopback
//!    address, plus dummy credentials so the cloud CLI signs *something*
//!    before sending (the manager strips the dummy SigV4 and re-signs with
//!    the impersonated identity).
//! 4. Execs the user's command (e.g. `aws sts get-caller-identity`) with
//!    those env vars set. The child process talks to `localhost:<port>`,
//!    thinking it's AWS; bytes flow through the WS to the manager and back.
//! 5. On child exit, the WebSocket closes and the local server shuts down.
//!
//! Multiplexing across concurrent requests uses `request_id` correlation,
//! same as the pull tunnel — the WS is a fan-in / fan-out channel.

use crate::error::{ErrorData, Result};
use alien_debug_session::PushTunnelDebugSession;
use alien_error::{AlienError, Context as _, IntoAlienError};
use axum::{
    body::Bytes,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use uuid::Uuid;

/// Wire frames mirror `alien-managerx`'s `TunnelRequestFrame` /
/// `TunnelResponseFrame`. We re-declare them locally so the OSS CLI doesn't
/// depend on the platform crate.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TunnelRequestFrame {
    request_id: String,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body_b64: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TunnelResponseFrame {
    request_id: String,
    status: u16,
    headers: Vec<(String, String)>,
    body_b64: String,
}

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<TunnelResponseFrame>>>>;

/// Shared state the loopback HTTP server uses to fan out into the WebSocket.
#[derive(Clone)]
struct ProxyState {
    /// mpsc to the WS writer task — every inbound HTTP request gets framed
    /// and pushed here.
    outbound: mpsc::Sender<TunnelRequestFrame>,
    /// Awaiters keyed by `request_id`. The WS reader task pops each
    /// response and resolves the matching awaiter.
    pending: PendingMap,
}

/// Spin up the loopback proxy + WS tunnel for the given push-tunnel session,
/// then return the merged env that the caller should exec the user's child
/// command with. The returned [`PushTunnelGuard`] owns the WS dial + the
/// local server; drop it after the child exits to shut everything down.
pub async fn spawn_push_tunnel(
    tunnel: &PushTunnelDebugSession,
) -> Result<(BTreeMap<String, String>, PushTunnelGuard)> {
    // 1. dial the manager WS
    // The manager hands us an `https://manager/.../push-tunnel` URL; the
    // WebSocket client wants `wss://` (or `ws://` for local dev). Convert
    // before dialing.
    let ws_base = http_to_ws_url(&tunnel.tunnel_url).map_err(|e| {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("Invalid push-tunnel URL: {e}"),
            url: Some(tunnel.tunnel_url.clone()),
        })
    })?;
    let ws_url_with_token = format!("{}?token={}", ws_base, urlencoding(&tunnel.client_token));
    let request = ws_url_with_token
        .as_str()
        .into_client_request()
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("Invalid push-tunnel URL '{ws_url_with_token}'"),
            url: Some(tunnel.tunnel_url.clone()),
        })?;
    let (ws_stream, _) =
        connect_async(request)
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to dial push-tunnel WebSocket".to_string(),
                url: Some(tunnel.tunnel_url.clone()),
            })?;

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // 2. set up the per-tunnel channels
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<TunnelRequestFrame>(64);
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

    // 3. WS writer: drain mpsc into the WebSocket
    let writer_handle = tokio::spawn(async move {
        while let Some(frame) = outbound_rx.recv().await {
            let payload = match serde_json::to_string(&frame) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if ws_sink.send(Message::Text(payload.into())).await.is_err() {
                break;
            }
        }
        let _ = ws_sink.close().await;
    });

    // 4. WS reader: dispatch responses to awaiters
    let pending_for_reader = Arc::clone(&pending);
    let reader_handle = tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            let Ok(msg) = msg else { break };
            let text = match msg {
                Message::Text(t) => t,
                Message::Binary(b) => match String::from_utf8(b.to_vec()) {
                    Ok(s) => s.into(),
                    Err(_) => continue,
                },
                Message::Close(_) => break,
                _ => continue,
            };
            let Ok(frame) = serde_json::from_str::<TunnelResponseFrame>(&text) else { continue };
            let mut guard = pending_for_reader.lock().await;
            if let Some(tx) = guard.remove(&frame.request_id) {
                let _ = tx.send(frame);
            }
        }
    });

    // 5. bind the loopback HTTP server on an ephemeral port
    let state = ProxyState {
        outbound: outbound_tx,
        pending: Arc::clone(&pending),
    };
    let app: Router = Router::new()
        .route("/{*path}", any(handle_loopback_request))
        .route("/", any(handle_loopback_request))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to bind loopback proxy".to_string(),
            url: None,
        })?;
    let local_addr: SocketAddr =
        listener
            .local_addr()
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to read loopback proxy bound address".to_string(),
                url: None,
            })?;
    let endpoint_url = format!("http://{}", local_addr);

    let server_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // 6. build the env. Dummy creds so AWS/GCP/Azure CLIs sign *something*
    //    before sending — the manager strips and re-signs.
    let env = build_provider_env(&tunnel.provider, &endpoint_url);

    Ok((
        env,
        PushTunnelGuard {
            writer: writer_handle,
            reader: reader_handle,
            server: server_handle,
        },
    ))
}

/// RAII handle that owns the WS + local server tasks. Dropping aborts them.
pub struct PushTunnelGuard {
    writer: tokio::task::JoinHandle<()>,
    reader: tokio::task::JoinHandle<()>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for PushTunnelGuard {
    fn drop(&mut self) {
        self.writer.abort();
        self.reader.abort();
        self.server.abort();
    }
}

impl PushTunnelGuard {
    /// Combine several guards into one so the caller can keep a single
    /// handle alive for the child process's run. Useful when a single
    /// `alien debug` session spawns multiple loopbacks (e.g. AWS + GCP +
    /// Azure all enabled for a pull-mode K8s deployment).
    pub fn merge(guards: Vec<PushTunnelGuard>) -> PushTunnelGuard {
        // We don't have a "fan-out" abort primitive; collect every task
        // handle into a parent guard whose three slots are themselves
        // tokio tasks that await + propagate.
        let handles: Vec<tokio::task::JoinHandle<()>> = guards
            .into_iter()
            .flat_map(|mut g| {
                // Replace each field with a no-op handle so the `Drop` on
                // the moved-from guard doesn't double-abort the same task.
                let writer = std::mem::replace(&mut g.writer, tokio::spawn(async {}));
                let reader = std::mem::replace(&mut g.reader, tokio::spawn(async {}));
                let server = std::mem::replace(&mut g.server, tokio::spawn(async {}));
                std::mem::forget(g); // skip the no-op aborts in Drop
                [writer, reader, server]
            })
            .collect();
        let abort_handles: Vec<tokio::task::AbortHandle> =
            handles.iter().map(|h| h.abort_handle()).collect();
        // Spawn a "supervisor" task that just owns the handles; aborting it
        // doesn't free the children, so install a custom abort flow via the
        // three slot tasks each holding an `AbortOnDrop` of the underlying
        // join handles.
        let abort_clone1 = abort_handles.clone();
        let writer = tokio::spawn(async move {
            // park forever; aborted on drop
            let _abort_clone1 = abort_clone1;
            std::future::pending::<()>().await
        });
        let reader = tokio::spawn(async move {
            std::future::pending::<()>().await
        });
        let server = tokio::spawn(async move {
            let _abort_handles = abort_handles;
            std::future::pending::<()>().await
        });
        PushTunnelGuard {
            writer,
            reader,
            server,
        }
    }
}

/// Pull-mode AWS loopback. Used when the session is a regular pull-mode
/// kubectl tunnel that *also* advertises an `aws_endpoint_url` — meaning the
/// manager will accept AWS bytes at that URL and ride them into the same
/// pull tunnel for the agent (IRSA-driven signing happens cluster-side).
///
/// Architecturally distinct from `spawn_push_tunnel`: there's no separate
/// WebSocket. We bring up a local HTTP server on `127.0.0.1:0`, set
/// `AWS_ENDPOINT_URL` at it, and just forward each inbound request to the
/// manager's per-session cloud-proxy URL with a bearer + an
/// `X-Alien-Target-Url` header naming the cloud endpoint the child actually
/// intended.
pub async fn spawn_pull_aws_loopback(
    aws_proxy_base: &str,
    client_token: &str,
) -> Result<(BTreeMap<String, String>, PushTunnelGuard)> {
    #[derive(Clone)]
    struct PullAwsState {
        proxy_base: String,
        token: String,
        http: reqwest::Client,
    }

    async fn handle(State(state): State<PullAwsState>, req: Request) -> Response {
        let (parts, body) = req.into_parts();
        let uri = parts.uri.clone();
        let body_bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response(),
        };

        // Recover the AWS endpoint the child *intended* to call. The aws SDK
        // we redirected sees `127.0.0.1:<port>` as the apparent host; the
        // real target's been lost. Heuristic: use the Host the SDK would
        // normally have sent (look for `Host: …amazonaws.com`) if there's
        // one; else fall back to reconstructing from the original AWS
        // service the user invoked, using the user agent + URL path as
        // hints. For the MVP we require the SDK to leak the intended host
        // via the `X-Amz-Target` style header set, which aws-cli does for
        // most services. Without it, default to STS (so at minimum
        // `aws sts get-caller-identity` works).
        let intended_host = parts
            .headers
            .get("x-amz-target")
            .and_then(|v| v.to_str().ok())
            .and_then(|target| {
                // X-Amz-Target is `<ServiceName>.<Operation>`. Map service
                // to a likely endpoint host.
                let svc = target.split('.').next().unwrap_or("");
                aws_service_to_default_host(svc)
            })
            .unwrap_or_else(|| "sts.us-east-1.amazonaws.com".to_string());

        let path_and_query = uri
            .path_and_query()
            .map(|p| p.as_str())
            .unwrap_or("/");
        let target_url = format!("https://{intended_host}{path_and_query}");

        // Forward to the manager. The manager's cloud-aws handler reads
        // X-Alien-Target-Url and frames the request with target=Cloud{aws}.
        let url = format!("{}/passthrough", state.proxy_base.trim_end_matches('/'));
        let mut req_builder = state
            .http
            .request(parts.method.clone(), &url)
            .bearer_auth(&state.token)
            .header("x-alien-target-url", &target_url)
            .body(body_bytes.to_vec());
        for (name, value) in parts.headers.iter() {
            // Don't forward host/authorization; we set our own.
            if matches!(name.as_str(), "host" | "authorization") {
                continue;
            }
            req_builder = req_builder.header(name.clone(), value.clone());
        }

        let resp = match req_builder.send().await {
            Ok(r) => r,
            Err(e) => return (StatusCode::BAD_GATEWAY, format!("manager proxy: {e}")).into_response(),
        };
        let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let resp_headers = resp.headers().clone();
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => return (StatusCode::BAD_GATEWAY, format!("read body: {e}")).into_response(),
        };

        let mut out = Response::builder().status(status);
        if let Some(h) = out.headers_mut() {
            for (n, v) in resp_headers.iter() {
                if matches!(n.as_str(), "connection" | "transfer-encoding") {
                    continue;
                }
                h.insert(n.clone(), v.clone());
            }
        }
        out.body(axum::body::Body::from(body))
            .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "build response").into_response())
    }

    let state = PullAwsState {
        proxy_base: aws_proxy_base.to_string(),
        token: client_token.to_string(),
        http: reqwest::Client::new(),
    };
    let app: Router = Router::new()
        .route("/{*path}", any(handle))
        .route("/", any(handle))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to bind AWS loopback".to_string(),
            url: None,
        })?;
    let local_addr = listener
        .local_addr()
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to read AWS loopback address".to_string(),
            url: None,
        })?;
    let endpoint_url = format!("http://{}", local_addr);

    let server_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let mut env = BTreeMap::new();
    env.insert("AWS_ENDPOINT_URL".to_string(), endpoint_url);
    env.insert(
        "AWS_ACCESS_KEY_ID".to_string(),
        "ALIEN_DEBUG_PLACEHOLDER".to_string(),
    );
    env.insert(
        "AWS_SECRET_ACCESS_KEY".to_string(),
        "ALIEN_DEBUG_PLACEHOLDER".to_string(),
    );

    // We don't have a WS for the pull-AWS path — the proxy forwards via
    // HTTP. Reuse PushTunnelGuard's structure but with no writer/reader
    // tasks; just the server lifetime matters.
    Ok((
        env,
        PushTunnelGuard {
            writer: tokio::spawn(async {}),
            reader: tokio::spawn(async {}),
            server: server_handle,
        },
    ))
}

/// Crude service-name → default host map for use when the AWS CLI doesn't
/// leak the intended host. Production should switch to per-service endpoint
/// overrides on the AWS SDK side so this guess isn't needed.
fn aws_service_to_default_host(service: &str) -> Option<String> {
    let svc = service.to_ascii_lowercase();
    let host = match svc.as_str() {
        "sts" | "stsservice" => "sts.us-east-1.amazonaws.com",
        "iam" | "iamservice" => "iam.amazonaws.com",
        "s3" | "s3service" => "s3.amazonaws.com",
        "ec2" => "ec2.us-east-1.amazonaws.com",
        "lambda" => "lambda.us-east-1.amazonaws.com",
        _ => return None,
    };
    Some(host.to_string())
}

/// Pull-mode GCP loopback. Mirrors [`spawn_pull_aws_loopback`] but for
/// `*.googleapis.com`. Engineer's `gcloud` / GCP SDK calls hit the loopback;
/// the loopback forwards to the manager's `cloud-gcp` endpoint, which frames
/// with `target=Cloud{gcp}` and rides the existing pull WebSocket. The
/// agent exchanges its projected WI token at GCP's STS and attaches the
/// resulting bearer.
pub async fn spawn_pull_gcp_loopback(
    gcp_proxy_base: &str,
    client_token: &str,
) -> Result<(BTreeMap<String, String>, PushTunnelGuard)> {
    let (endpoint_url, guard) =
        spawn_generic_cloud_loopback(gcp_proxy_base, client_token, "googleapis.com").await?;
    let mut env = BTreeMap::new();
    // gcloud / gsutil don't honor a single global env knob for endpoint;
    // these two cover the common per-service paths (compute, storage, etc.).
    env.insert("GOOGLE_CLOUD_API_ENDPOINT".to_string(), endpoint_url.clone());
    env.insert(
        "CLOUDSDK_CORE_UNIVERSE_DOMAIN".to_string(),
        endpoint_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string(),
    );
    Ok((env, guard))
}

/// Pull-mode Azure loopback. Same pattern. Engineer's `az` / Azure SDK
/// requests hit the loopback; agent exchanges the federated token at
/// `login.microsoftonline.com` and attaches the AAD bearer.
pub async fn spawn_pull_azure_loopback(
    azure_proxy_base: &str,
    client_token: &str,
) -> Result<(BTreeMap<String, String>, PushTunnelGuard)> {
    let (endpoint_url, guard) =
        spawn_generic_cloud_loopback(azure_proxy_base, client_token, "management.azure.com").await?;
    let mut env = BTreeMap::new();
    env.insert("AZURE_RESOURCE_MANAGER_ENDPOINT".to_string(), endpoint_url);
    Ok((env, guard))
}

/// Shared scaffolding for GCP/Azure pull-mode loopbacks. Brings up an HTTP
/// server on a random localhost port, forwards every inbound request to the
/// manager's per-provider cloud proxy with `X-Alien-Target-Url` naming the
/// intended cloud endpoint host (or a sensible fallback).
async fn spawn_generic_cloud_loopback(
    proxy_base: &str,
    client_token: &str,
    default_host: &'static str,
) -> Result<(String, PushTunnelGuard)> {
    #[derive(Clone)]
    struct CloudState {
        proxy_base: String,
        token: String,
        default_host: &'static str,
        http: reqwest::Client,
    }

    async fn handle(State(state): State<CloudState>, req: Request) -> Response {
        let (parts, body) = req.into_parts();
        let uri = parts.uri.clone();
        let body_bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response(),
        };

        // Recover the intended host. We honor `X-Alien-Target-Host` if the
        // cloud SDK or a user-set header carries it; otherwise default.
        let host = parts
            .headers
            .get("x-alien-target-host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(state.default_host);
        let path_and_query = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        let target_url = format!("https://{host}{path_and_query}");

        let url = format!("{}/passthrough", state.proxy_base.trim_end_matches('/'));
        let mut req_builder = state
            .http
            .request(parts.method.clone(), &url)
            .bearer_auth(&state.token)
            .header("x-alien-target-url", &target_url)
            .body(body_bytes.to_vec());
        for (name, value) in parts.headers.iter() {
            if matches!(name.as_str(), "host" | "authorization") {
                continue;
            }
            req_builder = req_builder.header(name.clone(), value.clone());
        }

        let resp = match req_builder.send().await {
            Ok(r) => r,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, format!("manager proxy: {e}")).into_response()
            }
        };
        let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let resp_headers = resp.headers().clone();
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => return (StatusCode::BAD_GATEWAY, format!("read body: {e}")).into_response(),
        };
        let mut out = Response::builder().status(status);
        if let Some(h) = out.headers_mut() {
            for (n, v) in resp_headers.iter() {
                if matches!(n.as_str(), "connection" | "transfer-encoding") {
                    continue;
                }
                h.insert(n.clone(), v.clone());
            }
        }
        out.body(axum::body::Body::from(body)).unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "build response").into_response()
        })
    }

    let state = CloudState {
        proxy_base: proxy_base.to_string(),
        token: client_token.to_string(),
        default_host,
        http: reqwest::Client::new(),
    };
    let app: Router = Router::new()
        .route("/{*path}", any(handle))
        .route("/", any(handle))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to bind cloud loopback".to_string(),
            url: None,
        })?;
    let local_addr = listener
        .local_addr()
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to read cloud loopback address".to_string(),
            url: None,
        })?;
    let endpoint_url = format!("http://{}", local_addr);

    let server_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((
        endpoint_url,
        PushTunnelGuard {
            writer: tokio::spawn(async {}),
            reader: tokio::spawn(async {}),
            server: server_handle,
        },
    ))
}

fn build_provider_env(provider: &str, endpoint_url: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    match provider {
        "aws" => {
            // Newer aws CLI / SDKs honor `AWS_ENDPOINT_URL` globally — sets
            // every service's endpoint to this URL. Pre-existing per-service
            // overrides (`AWS_ENDPOINT_URL_SERVICE`) still win if the user
            // set them; we don't try to override those.
            env.insert("AWS_ENDPOINT_URL".to_string(), endpoint_url.to_string());
            // Dummy creds — the CLI needs *something* to SigV4 with, even
            // though the manager strips and re-signs. Marker prefix so it's
            // obvious in logs that these aren't real.
            env.insert(
                "AWS_ACCESS_KEY_ID".to_string(),
                "ALIEN_DEBUG_PLACEHOLDER".to_string(),
            );
            env.insert(
                "AWS_SECRET_ACCESS_KEY".to_string(),
                "ALIEN_DEBUG_PLACEHOLDER".to_string(),
            );
            // Region must match what the manager re-signs against; we leave
            // it to whatever the deployment's region is (set by the user's
            // shell or aws config). The manager's signer derives the real
            // region from the URL host anyway.
        }
        "gcp" => {
            // gcloud honors `CLOUDSDK_API_ENDPOINT_OVERRIDES_*` per-service,
            // not a single global. For Phase 2 we set the universe-domain
            // override which catches the common service set; users who need
            // per-service granularity can set additional vars themselves.
            env.insert(
                "CLOUDSDK_CORE_UNIVERSE_DOMAIN".to_string(),
                strip_scheme(endpoint_url),
            );
            env.insert(
                "GOOGLE_CLOUD_API_ENDPOINT".to_string(),
                endpoint_url.to_string(),
            );
        }
        "azure" => {
            // `az` CLI honors `AZURE_RESOURCE_MANAGER_ENDPOINT` for ARM and
            // pulls service endpoints from the cloud profile otherwise. For
            // Phase 2 we set the ARM endpoint; other services need explicit
            // per-service overrides.
            env.insert(
                "AZURE_RESOURCE_MANAGER_ENDPOINT".to_string(),
                endpoint_url.to_string(),
            );
        }
        _ => {}
    }
    env
}

/// Strip `http://` / `https://` for env vars that expect a bare host.
fn strip_scheme(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

/// axum handler — every HTTP request from the child cloud-CLI flows through
/// here. We frame it, send via WS, await response, return as HTTP.
async fn handle_loopback_request(State(state): State<ProxyState>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let body_bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::PAYLOAD_TOO_LARGE, "request body too large").into_response();
        }
    };

    // The CLI's loopback receives requests against `http://127.0.0.1:<port>`,
    // but the manager needs the ORIGINAL AWS URL to re-sign. The aws CLI /
    // SDK already encodes the target service + region into its placeholder
    // SigV4 Authorization header:
    //   `Authorization: AWS4-HMAC-SHA256 Credential=KEY/DATE/REGION/SERVICE/aws4_request, ...`
    // We parse that to reconstruct the real `https://<service>.<region>.amazonaws.com`
    // endpoint. (Header preserved as-is; manager strips it during re-signing.)
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    let inferred_host = parts
        .headers
        .get(reqwest::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_aws_sigv4_credential)
        .map(|(region, service)| aws_endpoint_host(&service, &region));
    let full_url = match inferred_host {
        Some(host) => format!("https://{host}{path_and_query}"),
        // Fallback: assume STS in the deployment's default region. STS-only
        // is enough for `sts get-caller-identity` smoke tests; richer
        // service inference is the next iteration.
        None => format!("https://sts.us-east-1.amazonaws.com{path_and_query}"),
    };

    let header_pairs: Vec<(String, String)> = parts
        .headers
        .iter()
        .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
        .collect();

    let frame = TunnelRequestFrame {
        request_id: Uuid::new_v4().simple().to_string(),
        method: parts.method.as_str().to_string(),
        path: full_url,
        headers: header_pairs,
        body_b64: BASE64.encode(&body_bytes),
    };

    let (tx, rx) = oneshot::channel();
    state.pending.lock().await.insert(frame.request_id.clone(), tx);

    if state.outbound.send(frame.clone()).await.is_err() {
        state.pending.lock().await.remove(&frame.request_id);
        return (StatusCode::BAD_GATEWAY, "push-tunnel writer closed").into_response();
    }

    let response = tokio::time::timeout(std::time::Duration::from_secs(60), rx).await;
    let response_frame = match response {
        Ok(Ok(f)) => f,
        _ => {
            state.pending.lock().await.remove(&frame.request_id);
            return (StatusCode::GATEWAY_TIMEOUT, "push-tunnel response timed out").into_response();
        }
    };

    let status = StatusCode::from_u16(response_frame.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = match BASE64.decode(&response_frame.body_b64) {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_GATEWAY, "undecodable response body").into_response(),
    };

    let mut response = Response::builder().status(status);
    {
        let resp_headers = response.headers_mut().expect("response builder is valid");
        for (name, value) in response_frame.headers {
            let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else { continue };
            let Ok(value) = HeaderValue::from_str(&value) else { continue };
            if matches!(name.as_str(), "connection" | "transfer-encoding") {
                continue;
            }
            resp_headers.insert(name, value);
        }
    }
    response
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to build response").into_response()
        })
}

/// Parse an inbound SigV4 `Authorization` header and pull out
/// `(region, service)` from the `Credential=` field. The AWS CLI / SDK
/// always sets this — even when the actual creds are the placeholder ones
/// the loopback handed it — because SigV4 requires it.
///
/// Example: `AWS4-HMAC-SHA256 Credential=ALIEN_DEBUG_PLACEHOLDER/20260622/us-east-1/sts/aws4_request, SignedHeaders=..., Signature=...`
/// →  Some(("us-east-1", "sts"))
fn parse_aws_sigv4_credential(auth_header: &str) -> Option<(String, String)> {
    let credential_part = auth_header.split(',').find(|p| {
        let p = p.trim_start_matches("AWS4-HMAC-SHA256").trim_start();
        p.starts_with("Credential=")
    })?;
    let value = credential_part
        .trim_start_matches("AWS4-HMAC-SHA256")
        .trim_start()
        .trim_start_matches("Credential=");
    // value = `KEY/DATE/REGION/SERVICE/aws4_request`
    let parts: Vec<&str> = value.split('/').collect();
    if parts.len() < 5 {
        return None;
    }
    Some((parts[2].to_string(), parts[3].to_string()))
}

/// Map an AWS `(service, region)` pair to the canonical HTTPS endpoint host.
/// Global services (IAM, S3 in us-east-1) collapse to the region-less
/// hostname; everything else gets the standard `<service>.<region>.amazonaws.com`.
fn aws_endpoint_host(service: &str, region: &str) -> String {
    match service {
        "iam" => "iam.amazonaws.com".to_string(),
        // STS supports both `sts.amazonaws.com` and the regional endpoint;
        // we prefer regional so the SigV4 signature's region matches.
        _ => format!("{service}.{region}.amazonaws.com"),
    }
}

fn http_to_ws_url(url: &str) -> std::result::Result<String, String> {
    if let Some(rest) = url.strip_prefix("https://") {
        Ok(format!("wss://{rest}"))
    } else if let Some(rest) = url.strip_prefix("http://") {
        Ok(format!("ws://{rest}"))
    } else if url.starts_with("wss://") || url.starts_with("ws://") {
        Ok(url.to_string())
    } else {
        Err(format!("URL '{url}' must start with http(s):// or ws(s)://"))
    }
}

fn urlencoding(input: &str) -> String {
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

// Suppress unused-imports warnings on the trait imports we use indirectly
// through their methods.
#[allow(dead_code)]
fn _trait_markers(_h: HeaderMap, _b: Bytes, _m: Method) {}
