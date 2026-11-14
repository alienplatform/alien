//! Loopback relay for the application's OTLP metrics and traces.
//!
//! The application exports with its own OpenTelemetry SDK. The runtime holds
//! the credentials (a static header or a refreshing runtime identity) and the
//! application never sees them: the child's metrics and traces endpoints point
//! at this relay, which adds the credential and forwards the request to the
//! manager.
//!
//! Telemetry must never decide whether the application runs or block it. The
//! relay bounds body size and in-flight requests, gives each upstream request a
//! deadline, and when it cannot forward a request it answers with an error, drops
//! the batch and counts it.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use alien_client_core::runtime_credentials::RuntimeCredentials;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, HeaderMap, HeaderName, HeaderValue, Request, StatusCode},
    routing::post,
    Router,
};
use tokio::{net::TcpListener, sync::Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

pub const ENV_METRICS_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT";
pub const ENV_TRACES_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT";

/// Largest request body the relay accepts.
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
/// Requests forwarded at the same time. More are dropped, not queued.
const MAX_IN_FLIGHT: usize = 16;
/// Deadline for one upstream request.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(10);
/// Deadline for the application to finish uploading one request body.
const BODY_TIMEOUT: Duration = Duration::from_secs(10);
/// How often a non-zero drop count is reported.
const DROP_REPORT_INTERVAL: Duration = Duration::from_secs(60);

/// One signal's destination and the static headers that authorize it.
#[derive(Clone, Debug)]
pub struct RelayTarget {
    pub endpoint: String,
    pub headers: HashMap<String, String>,
}

/// Where the relay forwards each signal. A signal without a target is not
/// relayed and its endpoint is left alone.
#[derive(Clone, Debug, Default)]
pub struct RelayConfig {
    pub metrics: Option<RelayTarget>,
    pub traces: Option<RelayTarget>,
}

#[derive(Clone)]
struct RelayState {
    client: reqwest::Client,
    credentials: Option<Arc<RuntimeCredentials>>,
    in_flight: Arc<Semaphore>,
    dropped: Arc<AtomicU64>,
    shutdown: CancellationToken,
    target: RelayTarget,
}

/// A running relay. Dropping it stops the listener and cancels requests that
/// are still in flight.
pub struct TelemetryRelay {
    address: SocketAddr,
    dropped: Arc<AtomicU64>,
    shutdown: CancellationToken,
}

impl Drop for TelemetryRelay {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

impl TelemetryRelay {
    /// Starts the relay on a loopback port. With `credentials`, every
    /// forwarded request carries the current bearer token (one retry after a
    /// 401) for targets at the manager's origin; any other target gets its
    /// static headers.
    pub async fn start(
        config: RelayConfig,
        credentials: Option<Arc<RuntimeCredentials>>,
    ) -> std::io::Result<Self> {
        Self::start_reporting(config, credentials, DROP_REPORT_INTERVAL).await
    }

    async fn start_reporting(
        config: RelayConfig,
        credentials: Option<Arc<RuntimeCredentials>>,
        report_interval: Duration,
    ) -> std::io::Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(UPSTREAM_TIMEOUT)
            .build()
            .map_err(std::io::Error::other)?;
        let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
        let dropped = Arc::new(AtomicU64::new(0));
        let shutdown = CancellationToken::new();
        let state = |target: RelayTarget| RelayState {
            client: client.clone(),
            credentials: credentials.clone(),
            in_flight: in_flight.clone(),
            dropped: dropped.clone(),
            shutdown: shutdown.clone(),
            target,
        };

        let mut router = Router::new();
        if let Some(target) = config.metrics {
            router = router.merge(
                Router::new()
                    .route("/v1/metrics", post(forward))
                    .with_state(state(target)),
            );
        }
        if let Some(target) = config.traces {
            router = router.merge(
                Router::new()
                    .route("/v1/traces", post(forward))
                    .with_state(state(target)),
            );
        }
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        let stop = shutdown.clone();
        tokio::spawn(async move {
            let served =
                axum::serve(listener, router).with_graceful_shutdown(stop.cancelled_owned());
            if let Err(error) = served.await {
                warn!(error = %error, "Telemetry relay stopped");
            }
        });
        tokio::spawn(report_drops(
            dropped.clone(),
            shutdown.clone(),
            report_interval,
            |count| {
                warn!(
                    dropped = count,
                    "Telemetry relay dropped metric or trace batches it could not forward"
                )
            },
        ));
        Ok(Self {
            address,
            dropped,
            shutdown,
        })
    }

    /// The URL the application sends `path` (`/v1/metrics` or `/v1/traces`) to.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.address, path)
    }

    /// Batches the relay could not forward.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Calls `report` with the number of drops since the last report, at most once
/// per `interval` and only when the count is not zero, until `stop` fires.
async fn report_drops(
    dropped: Arc<AtomicU64>,
    stop: CancellationToken,
    interval: Duration,
    report: impl Fn(u64),
) {
    let mut reported = 0;
    let mut ticker = tokio::time::interval(interval);
    ticker.tick().await;
    loop {
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = ticker.tick() => {}
        }
        let total = dropped.load(Ordering::Relaxed);
        if total > reported {
            report(total - reported);
            reported = total;
        }
    }
}

async fn forward(State(state): State<RelayState>, request: Request<Body>) -> StatusCode {
    // The permit comes before the body is read, so the memory held by bodies
    // is bounded by MAX_IN_FLIGHT * MAX_BODY_BYTES however slowly the
    // application uploads. It is held until forwarding ends.
    let Ok(_permit) = state.in_flight.clone().try_acquire_owned() else {
        state.dropped.fetch_add(1, Ordering::Relaxed);
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let shutdown = state.shutdown.clone();
    tokio::select! {
        _ = shutdown.cancelled() => StatusCode::SERVICE_UNAVAILABLE,
        status = receive_and_send(&state, request) => status,
    }
}

async fn receive_and_send(state: &RelayState, request: Request<Body>) -> StatusCode {
    let (parts, body) = request.into_parts();
    let body = match tokio::time::timeout(BODY_TIMEOUT, axum::body::to_bytes(body, MAX_BODY_BYTES))
        .await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE,
        Err(_) => {
            state.dropped.fetch_add(1, Ordering::Relaxed);
            return StatusCode::REQUEST_TIMEOUT;
        }
    };
    let headers = parts.headers;
    match send(state, &headers, body).await {
        Ok(status) if status.is_success() => StatusCode::OK,
        Ok(status) => {
            state.dropped.fetch_add(1, Ordering::Relaxed);
            debug!(%status, "Manager refused relayed telemetry");
            StatusCode::BAD_GATEWAY
        }
        Err(error) => {
            state.dropped.fetch_add(1, Ordering::Relaxed);
            debug!(error = %error, "Relayed telemetry was not delivered");
            StatusCode::BAD_GATEWAY
        }
    }
}

async fn send(
    state: &RelayState,
    incoming: &HeaderMap,
    body: Bytes,
) -> Result<reqwest::StatusCode, Box<dyn std::error::Error + Send + Sync>> {
    let build = |authorization: Option<&str>| {
        let mut request = state.client.post(&state.target.endpoint).body(body.clone());
        // Only the content headers pass through: the application's own
        // authorization, if any, is never forwarded.
        for name in [header::CONTENT_TYPE, header::CONTENT_ENCODING] {
            if let Some(value) = incoming.get(&name) {
                request = request.header(name, value);
            }
        }
        match authorization {
            Some(token) => request.bearer_auth(token),
            None => {
                for (key, value) in &state.target.headers {
                    if let (Ok(key), Ok(value)) =
                        (HeaderName::try_from(key), HeaderValue::try_from(value))
                    {
                        request = request.header(key, value);
                    }
                }
                request
            }
        }
    };

    let Some(credentials) = &state.credentials else {
        return Ok(build(None).send().await?.status());
    };
    if !credentials.allows_endpoint(&state.target.endpoint) {
        // Never hand the manager credential to an endpoint it is not for;
        // such an endpoint keeps its own static headers.
        return Ok(build(None).send().await?.status());
    }
    let token = credentials.token().await?;
    let response = build(Some(&token)).send().await?;
    if response.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Ok(response.status());
    }
    let replacement = credentials.token_after_rejection(&token).await?;
    let response = build(Some(&replacement)).send().await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        credentials.reject(&replacement).await;
    }
    Ok(response.status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_client_core::runtime_credentials::{RuntimeCredentialConfig, RuntimeProofProvider};
    use async_trait::async_trait;
    use axum::{extract::State, routing::post, Json};
    use serde_json::{json, Value};
    use std::sync::Mutex;

    /// What the fake manager saw: (path, authorization, content type, body).
    type Seen = Arc<Mutex<Vec<(String, Option<String>, Option<String>, Vec<u8>)>>>;

    #[derive(Clone)]
    struct Manager {
        seen: Seen,
        status: StatusCode,
    }

    async fn record(
        State(manager): State<Manager>,
        uri: axum::http::Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> StatusCode {
        let text = |name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        manager.seen.lock().unwrap().push((
            uri.path().to_string(),
            text("authorization"),
            text("content-type"),
            body.to_vec(),
        ));
        manager.status
    }

    async fn exchange() -> Json<Value> {
        Json(json!({
            "token": "bounded-token",
            "expiresAt": chrono::Utc::now() + chrono::TimeDelta::minutes(30),
        }))
    }

    async fn fake_manager(status: StatusCode) -> (String, Seen) {
        let seen = Seen::default();
        let app = Router::new()
            .route("/v1/identity/exchange", post(exchange))
            .route("/v1/metrics", post(record))
            .route("/v1/traces", post(record))
            .with_state(Manager {
                seen: seen.clone(),
                status,
            });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}"), seen)
    }

    struct Proof;
    #[async_trait]
    impl RuntimeProofProvider for Proof {
        async fn proof(&self, _audience: &str) -> alien_client_core::Result<Value> {
            Ok(json!({"type": "session", "token": "parent"}))
        }
    }

    fn relay_config(manager: &str) -> RelayConfig {
        RelayConfig {
            metrics: Some(RelayTarget {
                endpoint: format!("{manager}/v1/metrics"),
                headers: HashMap::from([("authorization".into(), "Bearer static-metrics".into())]),
            }),
            traces: Some(RelayTarget {
                endpoint: format!("{manager}/v1/traces"),
                headers: HashMap::from([("authorization".into(), "Bearer static-traces".into())]),
            }),
        }
    }

    async fn app_posts(relay: &TelemetryRelay, path: &str, body: &'static [u8]) -> StatusCode {
        // The application's own authorization header must not be forwarded.
        reqwest::Client::new()
            .post(relay.url(path))
            .header("content-type", "application/x-protobuf")
            .header("authorization", "Bearer from-the-app")
            .body(body)
            .send()
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn static_mode_forwards_metrics_and_traces_with_each_signals_header() {
        let (manager, seen) = fake_manager(StatusCode::OK).await;
        let relay = TelemetryRelay::start(relay_config(&manager), None)
            .await
            .unwrap();

        assert_eq!(app_posts(&relay, "/v1/metrics", b"metric-batch").await, 200);
        assert_eq!(app_posts(&relay, "/v1/traces", b"span-batch").await, 200);

        let seen = seen.lock().unwrap();
        assert_eq!(
            *seen,
            vec![
                (
                    "/v1/metrics".to_string(),
                    Some("Bearer static-metrics".to_string()),
                    Some("application/x-protobuf".to_string()),
                    b"metric-batch".to_vec()
                ),
                (
                    "/v1/traces".to_string(),
                    Some("Bearer static-traces".to_string()),
                    Some("application/x-protobuf".to_string()),
                    b"span-batch".to_vec()
                ),
            ]
        );
        assert_eq!(relay.dropped(), 0);
    }

    #[tokio::test]
    async fn identity_mode_sends_the_exchanged_token_and_never_a_static_header() {
        let (manager, seen) = fake_manager(StatusCode::OK).await;
        let credentials = RuntimeCredentials::new(
            RuntimeCredentialConfig {
                manager_url: manager.clone(),
                audience: "test-manager".into(),
                deployment_id: "deployment-one".into(),
                purpose: json!({"type": "telemetry"}),
            },
            Arc::new(Proof),
        )
        .unwrap();
        let mut config = relay_config(&manager);
        // Identity mode has no static header at all.
        config.metrics.as_mut().unwrap().headers.clear();
        config.traces.as_mut().unwrap().headers.clear();
        let relay = TelemetryRelay::start(config, Some(Arc::new(credentials)))
            .await
            .unwrap();

        assert_eq!(app_posts(&relay, "/v1/metrics", b"m").await, 200);
        assert_eq!(app_posts(&relay, "/v1/traces", b"t").await, 200);

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        for (_, authorization, _, _) in seen.iter() {
            assert_eq!(authorization.as_deref(), Some("Bearer bounded-token"));
        }
    }

    #[tokio::test]
    async fn manager_errors_and_outages_drop_and_count_without_blocking() {
        let (manager, seen) = fake_manager(StatusCode::INTERNAL_SERVER_ERROR).await;
        let relay = TelemetryRelay::start(relay_config(&manager), None)
            .await
            .unwrap();
        assert_eq!(app_posts(&relay, "/v1/metrics", b"m").await, 502);
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(relay.dropped(), 1);

        // Nothing listens on the manager's port any more.
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let down = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let relay = TelemetryRelay::start(relay_config(&down), None)
            .await
            .unwrap();
        assert_eq!(app_posts(&relay, "/v1/traces", b"t").await, 502);
        assert_eq!(relay.dropped(), 1);
    }

    #[tokio::test]
    async fn oversized_bodies_are_refused_and_unconfigured_signals_have_no_route() {
        let (manager, seen) = fake_manager(StatusCode::OK).await;
        let mut config = relay_config(&manager);
        config.traces = None;
        let relay = TelemetryRelay::start(config, None).await.unwrap();

        let too_big = vec![0u8; MAX_BODY_BYTES + 1];
        let status = reqwest::Client::new()
            .post(relay.url("/v1/metrics"))
            .body(too_big)
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, 413);
        assert_eq!(app_posts(&relay, "/v1/traces", b"t").await, 404);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn runtime_points_the_app_at_the_relay_and_keeps_headers_out_of_its_env() {
        let (manager, seen) = fake_manager(StatusCode::OK).await;
        let env = HashMap::from([
            (
                ENV_METRICS_ENDPOINT.to_string(),
                format!("{manager}/v1/metrics"),
            ),
            (
                ENV_TRACES_ENDPOINT.to_string(),
                format!("{manager}/v1/traces"),
            ),
        ]);
        // Vault-loaded runtime-only secrets: the shared header, and a
        // metrics-specific one.
        let secrets = HashMap::from([
            (
                "OTEL_EXPORTER_OTLP_HEADERS".to_string(),
                "authorization=Bearer shared".to_string(),
            ),
            (
                "OTEL_EXPORTER_OTLP_METRICS_HEADERS".to_string(),
                "authorization=Bearer metrics-only".to_string(),
            ),
        ]);

        let (overrides, _relay) =
            crate::runtime::start_telemetry_relay(&env, &secrets, None, true).await;
        let overrides: HashMap<_, _> = overrides.into_iter().collect();

        assert_eq!(overrides.len(), 2);
        for (name, value) in &overrides {
            assert!(value.starts_with("http://127.0.0.1:"), "{name}={value}");
            assert!(!value.contains("Bearer"));
        }
        let status = reqwest::Client::new()
            .post(&overrides[ENV_METRICS_ENDPOINT])
            .body("m")
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, 200);
        reqwest::Client::new()
            .post(&overrides[ENV_TRACES_ENDPOINT])
            .body("t")
            .send()
            .await
            .unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].1.as_deref(), Some("Bearer metrics-only"));
        assert_eq!(seen[1].1.as_deref(), Some("Bearer shared"));
    }

    #[tokio::test]
    async fn nothing_is_relayed_when_runtime_export_is_off_or_no_endpoint_is_set() {
        let env = HashMap::from([(
            ENV_METRICS_ENDPOINT.to_string(),
            "https://manager.test/v1/metrics".to_string(),
        )]);
        let (overrides, relay) =
            crate::runtime::start_telemetry_relay(&env, &HashMap::new(), None, false).await;
        assert!(overrides.is_empty() && relay.is_none());
        let (overrides, relay) =
            crate::runtime::start_telemetry_relay(&HashMap::new(), &HashMap::new(), None, true)
                .await;
        assert!(overrides.is_empty() && relay.is_none());
    }

    /// Opens a connection that announces a body and sends only part of it.
    async fn slow_upload(address: &str) -> tokio::net::TcpStream {
        use tokio::io::AsyncWriteExt;
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                b"POST /v1/metrics HTTP/1.1\r\nHost: x\r\nContent-Length: 1000\r\n\r\npartial",
            )
            .await
            .unwrap();
        stream
    }

    #[tokio::test]
    async fn slow_uploads_beyond_the_in_flight_limit_are_refused_before_their_body_is_read() {
        use tokio::io::AsyncReadExt;
        let (manager, _seen) = fake_manager(StatusCode::OK).await;
        let relay = TelemetryRelay::start(relay_config(&manager), None)
            .await
            .unwrap();
        let address = relay
            .url("/")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();

        let mut streams = Vec::new();
        for _ in 0..MAX_IN_FLIGHT + 4 {
            streams.push(slow_upload(&address).await);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;

        // None of the bodies is complete, so the 16 admitted uploads wait for
        // more data. The four over the limit are answered at once.
        let mut refused = 0;
        for stream in &mut streams {
            let mut buffer = [0u8; 64];
            if let Ok(Ok(read)) =
                tokio::time::timeout(Duration::from_millis(100), stream.read(&mut buffer)).await
            {
                if read > 0 && buffer.starts_with(b"HTTP/1.1 503") {
                    refused += 1;
                }
            }
        }
        assert_eq!(refused, 4);
        assert_eq!(relay.dropped(), 4);
    }

    #[tokio::test]
    async fn the_port_refuses_connections_after_the_relay_is_dropped() {
        let (manager, _seen) = fake_manager(StatusCode::OK).await;
        let relay = TelemetryRelay::start(relay_config(&manager), None)
            .await
            .unwrap();
        let address = relay
            .url("/")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        let mut upload = slow_upload(&address).await;
        assert_eq!(app_posts(&relay, "/v1/metrics", b"m").await, 200);

        drop(relay);
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert!(tokio::net::TcpStream::connect(&address).await.is_err());
        // The upload that was still in flight is ended too.
        use tokio::io::AsyncReadExt;
        let mut buffer = Vec::new();
        let ended =
            tokio::time::timeout(Duration::from_secs(2), upload.read_to_end(&mut buffer)).await;
        assert!(ended.is_ok(), "in-flight upload was left open");
    }

    #[tokio::test(start_paused = true)]
    async fn drops_are_reported_once_per_interval_and_only_when_non_zero() {
        let dropped = Arc::new(AtomicU64::new(0));
        let stop = CancellationToken::new();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let sink = reports.clone();
        let task = tokio::spawn(report_drops(
            dropped.clone(),
            stop.clone(),
            Duration::from_secs(60),
            move |count| sink.lock().unwrap().push(count),
        ));

        tokio::time::sleep(Duration::from_secs(61)).await;
        assert!(reports.lock().unwrap().is_empty());

        dropped.fetch_add(3, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_secs(60)).await;
        dropped.fetch_add(2, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert_eq!(*reports.lock().unwrap(), vec![3]);
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert_eq!(*reports.lock().unwrap(), vec![3, 2]);

        stop.cancel();
        task.await.unwrap();
    }
}
