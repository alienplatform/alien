//! Operator end: keeps outbound tunnel connections to the manager open and
//! forwards requests arriving on them to in-cluster services.

use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, RwLock},
    time::Duration,
};

use alien_error::{AlienError, Context, IntoAlienError};
use bytes::Bytes;
use http::{header, HeaderValue, Request, Response, StatusCode, Uri};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full};
use hyper::{body::Incoming, server::conn::http2, service::service_fn};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::{TokioExecutor, TokioIo, TokioTimer},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{
    tungstenite::{
        handshake::{client::generate_key, derive_accept_key},
        protocol::Role,
    },
    WebSocketStream,
};
use tracing::{info, warn};
use url::Url;

use crate::{
    h2_settings, websocket_io, ErrorData, Result, CONNECT_PATH, ERROR_HEADER, SUBPROTOCOL,
};

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type ResponseBody = UnsyncBoxBody<Bytes, BoxError>;

/// In-cluster services reachable through the tunnel, by target name.
///
/// Only names in this table are forwarded; anything else is rejected, so the
/// tunnel reaches exactly the endpoints the stack declares and nothing else in
/// the cluster.
#[derive(Default)]
pub struct TunnelTargets {
    targets: RwLock<HashMap<String, Uri>>,
}

impl TunnelTargets {
    /// Create an empty table.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Replace the table with the endpoints of the current deployment state.
    /// Each value is a base URI such as `http://api.ns.svc.cluster.local:8080`.
    pub fn replace(&self, targets: HashMap<String, Uri>) {
        *self
            .targets
            .write()
            .expect("tunnel targets lock is never held across a panic") = targets;
    }

    /// Target names currently reachable.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.read().keys().cloned().collect();
        names.sort();
        names
    }

    fn resolve(&self, name: &str) -> Option<Uri> {
        self.read().get(name).cloned()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, Uri>> {
        self.targets
            .read()
            .expect("tunnel targets lock is never held across a panic")
    }
}

/// Settings for the operator's tunnel connections.
#[derive(Debug, Clone)]
pub struct TunnelClientConfig {
    /// Manager base URL, e.g. `https://manager.example.com`.
    pub manager_url: Url,
    /// Deployment token the operator authenticates with.
    pub token: String,
    /// Parallel connections to keep open. Two let one reconnect while the
    /// other keeps serving, and spread large transfers.
    pub connections: usize,
}

/// Keep `config.connections` tunnel connections open, reconnecting with
/// backoff when one ends. Runs until the task is aborted.
pub async fn run(config: TunnelClientConfig, targets: Arc<TunnelTargets>) -> Result<()> {
    let http = reqwest::Client::builder()
        // The WebSocket upgrade needs HTTP/1.1 on the outer connection.
        .http1_only()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .into_alien_error()
        .context(ErrorData::ConnectFailed {
            url: config.manager_url.to_string(),
            message: "failed to build HTTP client".to_string(),
        })?;
    let upstream = Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    let tasks: Vec<_> = (0..config.connections.max(1))
        .map(|slot| {
            tokio::spawn(connection_loop(
                slot,
                config.clone(),
                http.clone(),
                upstream.clone(),
                targets.clone(),
            ))
        })
        .collect();
    for task in tasks {
        task.await
            .into_alien_error()
            .context(ErrorData::ConnectionFailed {
                message: "tunnel connection task panicked".to_string(),
            })?;
    }
    Ok(())
}

async fn connection_loop(
    slot: usize,
    config: TunnelClientConfig,
    http: reqwest::Client,
    upstream: Client<HttpConnector, Incoming>,
    targets: Arc<TunnelTargets>,
) {
    let mut backoff = Backoff::default();
    loop {
        match connect(&config, &http).await {
            Ok(io) => {
                backoff.reset();
                info!(slot, "Tunnel connected");
                match serve(io, upstream.clone(), targets.clone()).await {
                    Ok(()) => info!(slot, "Tunnel connection closed by manager"),
                    Err(e) => warn!(slot, error = %e, "Tunnel connection ended"),
                }
            }
            Err(e) => warn!(slot, error = %e, "Tunnel connect failed"),
        }
        tokio::time::sleep(backoff.next_delay(slot)).await;
    }
}

/// Open one tunnel WebSocket to the manager.
async fn connect(
    config: &TunnelClientConfig,
    http: &reqwest::Client,
) -> Result<impl AsyncRead + AsyncWrite + Send + Unpin + 'static> {
    let url = config
        .manager_url
        .join(CONNECT_PATH.trim_start_matches('/'))
        .into_alien_error()
        .context(ErrorData::ConnectFailed {
            url: config.manager_url.to_string(),
            message: "invalid manager URL".to_string(),
        })?;
    let connect_failed = |message: &str| ErrorData::ConnectFailed {
        url: url.to_string(),
        message: message.to_string(),
    };

    let key = generate_key();
    let response = http
        .get(url.clone())
        .header(header::CONNECTION, "Upgrade")
        .header(header::UPGRADE, "websocket")
        .header(header::SEC_WEBSOCKET_VERSION, "13")
        .header(header::SEC_WEBSOCKET_KEY, &key)
        .header(header::SEC_WEBSOCKET_PROTOCOL, SUBPROTOCOL)
        .bearer_auth(&config.token)
        .send()
        .await
        .into_alien_error()
        .context(connect_failed("request failed"))?;

    let status = response.status();
    if status != StatusCode::SWITCHING_PROTOCOLS {
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(connect_failed(&format!(
            "manager answered {status}: {body}"
        ))));
    }
    let accept_ok = response
        .headers()
        .get(header::SEC_WEBSOCKET_ACCEPT)
        .and_then(|v| v.to_str().ok())
        == Some(derive_accept_key(key.as_bytes()).as_str());
    if !accept_ok {
        return Err(AlienError::new(connect_failed(
            "invalid Sec-WebSocket-Accept in upgrade response",
        )));
    }

    let upgraded = response
        .upgrade()
        .await
        .into_alien_error()
        .context(connect_failed("connection upgrade failed"))?;
    let ws = WebSocketStream::from_raw_socket(upgraded, Role::Client, None).await;
    Ok(websocket_io(ws))
}

/// Run the HTTP/2 server on one tunnel connection until it closes.
async fn serve<IO>(
    io: IO,
    upstream: Client<HttpConnector, Incoming>,
    targets: Arc<TunnelTargets>,
) -> Result<()>
where
    IO: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let service = service_fn(move |request| {
        let upstream = upstream.clone();
        let targets = targets.clone();
        async move { Ok::<_, Infallible>(forward(request, &upstream, &targets).await) }
    });
    http2::Builder::new(TokioExecutor::new())
        .timer(TokioTimer::new())
        .initial_stream_window_size(h2_settings::INITIAL_STREAM_WINDOW)
        .initial_connection_window_size(h2_settings::INITIAL_CONNECTION_WINDOW)
        .max_concurrent_streams(h2_settings::MAX_CONCURRENT_STREAMS)
        .keep_alive_interval(h2_settings::KEEPALIVE_INTERVAL)
        .keep_alive_timeout(h2_settings::KEEPALIVE_TIMEOUT)
        .serve_connection(TokioIo::new(io), service)
        .await
        .into_alien_error()
        .context(ErrorData::ConnectionFailed {
            message: "HTTP/2 connection error".to_string(),
        })
}

/// Forward one tunnelled request to the in-cluster service it names.
async fn forward(
    request: Request<Incoming>,
    upstream: &Client<HttpConnector, Incoming>,
    targets: &TunnelTargets,
) -> Response<ResponseBody> {
    let Some(target) = request.uri().host().map(str::to_string) else {
        return tunnel_error(
            StatusCode::BAD_REQUEST,
            "TUNNEL_TARGET_MISSING",
            "tunnel request names no target".to_string(),
        );
    };
    let Some(base) = targets.resolve(&target) else {
        return tunnel_error(
            StatusCode::NOT_FOUND,
            "TUNNEL_TARGET_NOT_FOUND",
            format!("'{target}' is not a tunnel endpoint of this deployment"),
        );
    };

    let (mut parts, body) = request.into_parts();
    let mut uri = base.into_parts();
    uri.path_and_query = parts.uri.path_and_query().cloned();
    parts.uri = match Uri::from_parts(uri) {
        Ok(uri) => uri,
        Err(e) => {
            return tunnel_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "TUNNEL_TARGET_INVALID",
                format!("invalid target URI for '{target}': {e}"),
            )
        }
    };
    // HTTP/2 from the manager, HTTP/1.1 to the service; the client derives
    // Host from the target URI.
    parts.version = http::Version::HTTP_11;
    parts.headers.remove(header::HOST);

    match upstream.request(Request::from_parts(parts, body)).await {
        Ok(response) => response.map(|body| body.map_err(BoxError::from).boxed_unsync()),
        Err(e) => tunnel_error(
            StatusCode::BAD_GATEWAY,
            "TUNNEL_UPSTREAM_UNAVAILABLE",
            format!("'{target}' did not answer: {}", error_chain(&e)),
        ),
    }
}

/// `error: cause: cause` — hyper's top-level errors alone ("client error")
/// don't say what went wrong.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// An error produced by the tunnel itself, marked with [`ERROR_HEADER`] so
/// callers can tell it apart from the service's own responses.
fn tunnel_error(status: StatusCode, code: &'static str, message: String) -> Response<ResponseBody> {
    let body = serde_json::json!({ "code": code, "message": message }).to_string();
    let mut response = Response::new(
        Full::new(Bytes::from(body))
            .map_err(|never: Infallible| match never {})
            .boxed_unsync(),
    );
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(ERROR_HEADER, HeaderValue::from_static(code));
    response
}

/// Exponential backoff from 1s to 30s. The slot offsets connections so they
/// don't reconnect in lockstep.
#[derive(Default)]
struct Backoff {
    attempt: u32,
}

impl Backoff {
    fn reset(&mut self) {
        self.attempt = 0;
    }

    fn next_delay(&mut self, slot: usize) -> Duration {
        let base = Duration::from_secs(1u64 << self.attempt.min(5)).min(Duration::from_secs(30));
        self.attempt = self.attempt.saturating_add(1);
        base + Duration::from_millis(250 * slot as u64)
    }
}
