//! Short-lived log export credentials, obtained from the manager with a proof of the
//! Worker's cloud identity (see `identity`).
//!
//! The batch log processor calls the exporter from its own thread, outside any tokio
//! runtime. So the credentials live in shared state: a background task refreshes them
//! before they expire, and the exporter's HTTP client reads that state on every request,
//! sending through a client that runs on the runtime captured at startup.

use super::identity::IdentityProver;
use crate::error::{ErrorData, Result};
use alien_core::OtlpCredentials;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::{header::AUTHORIZATION, HeaderValue, Request, Response};
use opentelemetry_http::{HttpClient, HttpError};
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;
use std::{
    fmt,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tracing::{debug, warn};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 16 * 1024;
/// How long startup waits for the first credentials before the app starts without them.
const FIRST_EXCHANGE_WAIT: Duration = Duration::from_secs(5);
/// Credentials are refreshed this long before they expire (or halfway, if they live less).
const REFRESH_MARGIN: Duration = Duration::from_secs(60);
/// Credentials closer to expiry than this are not sent: they could expire in transit.
const MIN_REMAINING: Duration = Duration::from_secs(5);
const CHECK_INTERVAL: Duration = Duration::from_secs(5);
const BACKOFF_START: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct Grant {
    endpoint: Url,
    authorization: HeaderValue,
    expires_at: DateTime<Utc>,
    refresh_at: DateTime<Utc>,
}

impl fmt::Debug for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grant")
            .field("endpoint", &self.endpoint.as_str())
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Grant {
    fn remaining(&self) -> Duration {
        (self.expires_at - Utc::now()).to_std().unwrap_or_default()
    }

    fn refresh_due(&self) -> bool {
        Utc::now() >= self.refresh_at
    }
}

#[derive(Default)]
struct State {
    grant: Option<Grant>,
    failures: u32,
    retry_at: Option<Instant>,
}

struct Inner {
    url: Url,
    audience: String,
    deployment_id: String,
    prover: Arc<dyn IdentityProver>,
    http: reqwest::Client,
    state: Mutex<State>,
    one_exchange_at_a_time: tokio::sync::Mutex<()>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeResponse {
    logs: ExchangedLogs,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangedLogs {
    endpoint: String,
    authorization: String,
    expires_at: DateTime<Utc>,
}

fn failed(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::TelemetryCredentialsFailed {
        message: message.into(),
    })
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

impl Inner {
    /// Credentials that can still be sent, if any.
    fn usable(&self) -> Option<Grant> {
        let state = self.state.lock().expect("telemetry credentials poisoned");
        state
            .grant
            .clone()
            .filter(|grant| grant.remaining() > MIN_REMAINING)
    }

    fn refresh_due(&self) -> bool {
        let state = self.state.lock().expect("telemetry credentials poisoned");
        if state.retry_at.is_some_and(|at| Instant::now() < at) {
            return false;
        }
        state.grant.as_ref().is_none_or(Grant::refresh_due)
    }

    /// Gets credentials that can be sent, exchanging a new proof when needed. At most one
    /// exchange runs at a time, and after a failure the next waits out a growing backoff.
    async fn current(&self) -> Result<Grant> {
        if let Some(grant) = self.usable() {
            if !grant.refresh_due() {
                return Ok(grant);
            }
        }
        let _exchanging = self.one_exchange_at_a_time.lock().await;
        let held = self.usable();
        if let Some(grant) = &held {
            if !grant.refresh_due() {
                return Ok(grant.clone());
            }
        }
        let wait = {
            let state = self.state.lock().expect("telemetry credentials poisoned");
            state
                .retry_at
                .and_then(|at| at.checked_duration_since(Instant::now()))
        };
        if wait.is_some() {
            return held.ok_or_else(|| failed("waiting to retry after a failed exchange"));
        }

        match self.exchange().await {
            Ok(grant) => {
                let mut state = self.state.lock().expect("telemetry credentials poisoned");
                state.grant = Some(grant.clone());
                state.failures = 0;
                state.retry_at = None;
                Ok(grant)
            }
            Err(error) => {
                let backoff = {
                    let mut state = self.state.lock().expect("telemetry credentials poisoned");
                    state.failures += 1;
                    let backoff = BACKOFF_START
                        .saturating_mul(1 << state.failures.min(5).saturating_sub(1))
                        .min(BACKOFF_MAX);
                    state.retry_at = Some(Instant::now() + backoff);
                    backoff
                };
                warn!(
                    error = %error,
                    retry_in_secs = backoff.as_secs(),
                    "Could not refresh log export credentials"
                );
                // Credentials that are still valid keep working until they expire. The
                // exchange may have run long enough for the old ones to run out.
                self.usable().ok_or(error)
            }
        }
    }

    /// Forgets credentials the data service refused, so the next export gets new ones.
    fn discard(&self, refused: &Grant) {
        let mut state = self.state.lock().expect("telemetry credentials poisoned");
        if state
            .grant
            .as_ref()
            .is_some_and(|grant| grant.expires_at == refused.expires_at)
        {
            state.grant = None;
        }
    }

    async fn exchange(&self) -> Result<Grant> {
        let proof = self.prover.prove(&self.audience).await?;
        let mut response = self
            .http
            .post(self.url.clone())
            .json(&json!({ "deploymentId": self.deployment_id, "proof": proof }))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "The manager could not be reached".to_string(),
            })?;
        if !response.status().is_success() {
            return Err(failed(format!(
                "The manager answered {}",
                response.status()
            )));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.into_alien_error().context(
            ErrorData::TelemetryCredentialsFailed {
                message: "The manager's reply could not be read".to_string(),
            },
        )? {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(failed("The manager's reply is too large"));
            }
            body.extend_from_slice(&chunk);
        }
        let reply: ExchangeResponse = serde_json::from_slice(&body).into_alien_error().context(
            ErrorData::TelemetryCredentialsFailed {
                message: "The manager's reply is not a credentials response".to_string(),
            },
        )?;

        let endpoint = Url::parse(&reply.logs.endpoint)
            .into_alien_error()
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "The manager named an invalid log endpoint".to_string(),
            })?;
        // The bearer token is only ever sent where the manager that issued it lives.
        if !same_origin(&endpoint, &self.url) {
            return Err(failed("The manager named a log endpoint on another host"));
        }
        let mut authorization = HeaderValue::from_str(&reply.logs.authorization)
            .into_alien_error()
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "The manager sent an invalid authorization".to_string(),
            })?;
        authorization.set_sensitive(true);
        let lifetime = (reply.logs.expires_at - Utc::now())
            .to_std()
            .unwrap_or_default();
        if lifetime <= MIN_REMAINING {
            return Err(failed(
                "The manager sent credentials that are about to expire",
            ));
        }
        let refresh_margin = REFRESH_MARGIN.min(lifetime / 2);
        let grant = Grant {
            endpoint,
            authorization,
            expires_at: reply.logs.expires_at,
            refresh_at: reply.logs.expires_at
                - chrono::Duration::from_std(refresh_margin).unwrap_or_default(),
        };
        debug!(
            expires_in_secs = grant.remaining().as_secs(),
            "Got log export credentials"
        );
        Ok(grant)
    }
}

/// The log export credentials of one Worker, kept fresh in the background.
#[derive(Clone)]
pub struct TelemetryGrants {
    inner: Arc<Inner>,
}

impl fmt::Debug for TelemetryGrants {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryGrants")
            .field("url", &self.inner.url.as_str())
            .field("deployment_id", &self.inner.deployment_id)
            .finish_non_exhaustive()
    }
}

impl TelemetryGrants {
    /// Gets the first credentials, waiting a few seconds at most, and keeps them fresh until
    /// the last clone is dropped. If the first exchange fails the app still starts, and the
    /// refresher keeps trying: the logs of that window are dropped, not sent unauthenticated.
    pub async fn start(
        credentials: &OtlpCredentials,
        deployment_id: String,
        prover: Arc<dyn IdentityProver>,
    ) -> Result<Self> {
        let url = Url::parse(&credentials.url).into_alien_error().context(
            ErrorData::ConfigurationInvalid {
                message: "The telemetry credentials URL is invalid".to_string(),
                field: Some("credentials.url".to_string()),
            },
        )?;
        if url.scheme() != "https" && !(url.scheme() == "http" && is_loopback(&url)) {
            return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: "The telemetry credentials URL must use https".to_string(),
                field: Some("credentials.url".to_string()),
            }));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .into_alien_error()
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "The HTTP client could not be built".to_string(),
            })?;
        let inner = Arc::new(Inner {
            url,
            audience: credentials.audience.clone(),
            deployment_id,
            prover,
            http,
            state: Mutex::new(State::default()),
            one_exchange_at_a_time: tokio::sync::Mutex::new(()),
        });

        match tokio::time::timeout(FIRST_EXCHANGE_WAIT, inner.current()).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => warn!(
                error = %error,
                "Starting without log export credentials; retrying in the background"
            ),
            Err(_) => warn!(
                "Log export credentials are slow to arrive; starting without them and retrying in the background"
            ),
        }
        tokio::spawn(keep_fresh(Arc::downgrade(&inner)));
        Ok(Self { inner })
    }

    /// An HTTP client for the OTLP exporter that sends every request to the current
    /// credentials' endpoint with their authorization. Must be created inside the runtime
    /// that should do the network work.
    pub fn http_client(&self) -> GrantedHttpClient {
        GrantedHttpClient {
            inner: self.inner.clone(),
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

async fn keep_fresh(inner: Weak<Inner>) {
    loop {
        tokio::time::sleep(CHECK_INTERVAL).await;
        let Some(inner) = inner.upgrade() else {
            return;
        };
        if inner.refresh_due() {
            // `current` logs its own failures and schedules the retry.
            let _ = inner.current().await;
        }
    }
}

/// Sends OTLP requests to the endpoint and with the authorization of the current
/// credentials. The URL the exporter was configured with is not used.
pub struct GrantedHttpClient {
    inner: Arc<Inner>,
    runtime: tokio::runtime::Handle,
}

impl fmt::Debug for GrantedHttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrantedHttpClient").finish_non_exhaustive()
    }
}

#[async_trait]
impl HttpClient for GrantedHttpClient {
    async fn send_bytes(
        &self,
        request: Request<Bytes>,
    ) -> std::result::Result<Response<Bytes>, HttpError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move {
                let grant = inner.current().await.map_err(|error| error.to_string())?;
                let (mut parts, body) = request.into_parts();
                parts.uri = grant
                    .endpoint
                    .as_str()
                    .parse()
                    .map_err(|error| format!("invalid endpoint: {error}"))?;
                parts
                    .headers
                    .insert(AUTHORIZATION, grant.authorization.clone());
                let request: reqwest::Request = Request::from_parts(parts, body)
                    .try_into()
                    .map_err(|error| format!("invalid request: {error}"))?;

                let mut response = inner
                    .http
                    .execute(request)
                    .await
                    .map_err(|error| format!("export failed: {error}"))?;
                if matches!(response.status().as_u16(), 401 | 403) {
                    inner.discard(&grant);
                }
                let status = response.status();
                let headers = std::mem::take(response.headers_mut());
                let body = response
                    .bytes()
                    .await
                    .map_err(|error| format!("export reply failed: {error}"))?;
                let mut response = Response::builder()
                    .status(status)
                    .body(body)
                    .map_err(|error| format!("invalid reply: {error}"))?;
                *response.headers_mut() = headers;
                Ok::<_, String>(response)
            })
            .await
            .map_err(|error| -> HttpError { Box::new(std::io::Error::other(error.to_string())) })?
            .map_err(|message| -> HttpError { Box::new(std::io::Error::other(message)) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use serde_json::Value;
    use std::collections::HashMap;

    const DATA_PATH: &str = "/v2/databases/logs-db/tables/logs/append";

    #[derive(Debug)]
    struct StubProver;

    #[async_trait]
    impl IdentityProver for StubProver {
        async fn prove(&self, audience: &str) -> Result<Value> {
            Ok(json!({ "gcp": { "token": format!("token-for-{audience}") } }))
        }
    }

    /// A manager and data endpoint on one origin, like a deployment's real manager.
    #[derive(Default)]
    struct Stub {
        /// What `POST /v1/telemetry/credentials` receives.
        exchanges: Mutex<Vec<Value>>,
        /// `(authorization, content-type, body)` of each append.
        appends: Mutex<Vec<(String, String, Vec<u8>)>>,
        /// Answers for the next exchanges; the last one repeats.
        plan: Mutex<Vec<Plan>>,
        /// The `authorization` values the data endpoint rejects.
        rejected: Mutex<Vec<String>>,
        redirect_hits: Mutex<u32>,
        origin: Mutex<String>,
    }

    #[derive(Clone)]
    enum Plan {
        Grant {
            token: &'static str,
            lifetime_secs: i64,
        },
        GrantElsewhere {
            endpoint: &'static str,
        },
        Status(u16),
        /// Answers the status after 1.2 seconds.
        SlowStatus(u16),
        Redirect,
    }

    struct Server {
        origin: String,
        stub: Arc<Stub>,
    }

    impl Server {
        async fn start(plan: Vec<Plan>) -> Self {
            let stub = Arc::new(Stub {
                plan: Mutex::new(plan),
                ..Default::default()
            });
            let app = Router::new()
                .route("/v1/telemetry/credentials", post(exchange))
                .route(DATA_PATH, post(append))
                .route("/elsewhere", post(elsewhere))
                .with_state(stub.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            *stub.origin.lock().unwrap() = origin.clone();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            Self { origin, stub }
        }

        fn credentials(&self) -> OtlpCredentials {
            OtlpCredentials {
                url: format!("{}/v1/telemetry/credentials", self.origin),
                audience: "mgr_test".to_string(),
            }
        }

        fn set_plan(&self, plan: Vec<Plan>) {
            *self.stub.plan.lock().unwrap() = plan;
        }

        fn exchanges(&self) -> Vec<Value> {
            self.stub.exchanges.lock().unwrap().clone()
        }

        fn appends(&self) -> Vec<(String, String, Vec<u8>)> {
            self.stub.appends.lock().unwrap().clone()
        }
    }

    async fn exchange(
        State(stub): State<Arc<Stub>>,
        Json(body): Json<Value>,
    ) -> (StatusCode, HeaderMap, String) {
        let origin = stub.origin.lock().unwrap().clone();
        stub.exchanges.lock().unwrap().push(body);
        let attempt = stub.exchanges.lock().unwrap().len();
        let plan = {
            let plan = stub.plan.lock().unwrap();
            plan[(attempt - 1).min(plan.len() - 1)].clone()
        };
        let mut headers = HeaderMap::new();
        match plan {
            Plan::Status(code) => (StatusCode::from_u16(code).unwrap(), headers, String::new()),
            Plan::SlowStatus(code) => {
                tokio::time::sleep(Duration::from_millis(1200)).await;
                (StatusCode::from_u16(code).unwrap(), headers, String::new())
            }
            Plan::Redirect => {
                headers.insert("location", "/elsewhere".parse().unwrap());
                (StatusCode::TEMPORARY_REDIRECT, headers, String::new())
            }
            Plan::Grant { token, lifetime_secs } => (
                StatusCode::OK,
                headers,
                json!({ "logs": {
                    "endpoint": format!("{origin}{DATA_PATH}"),
                    "authorization": format!("Bearer {token}"),
                    "expiresAt": (Utc::now() + chrono::Duration::seconds(lifetime_secs)).to_rfc3339(),
                }})
                .to_string(),
            ),
            Plan::GrantElsewhere { endpoint } => (
                StatusCode::OK,
                headers,
                json!({ "logs": {
                    "endpoint": endpoint,
                    "authorization": "Bearer stolen",
                    "expiresAt": (Utc::now() + chrono::Duration::seconds(300)).to_rfc3339(),
                }})
                .to_string(),
            ),
        }
    }

    async fn append(
        State(stub): State<Arc<Stub>>,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> StatusCode {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        let authorization = header("authorization");
        stub.appends.lock().unwrap().push((
            authorization.clone(),
            header("content-type"),
            body.to_vec(),
        ));
        if stub.rejected.lock().unwrap().contains(&authorization) {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::OK
        }
    }

    async fn elsewhere(State(stub): State<Arc<Stub>>) -> StatusCode {
        *stub.redirect_hits.lock().unwrap() += 1;
        StatusCode::OK
    }

    fn export(body: &'static [u8]) -> Request<Bytes> {
        Request::post("https://legacy.invalid/v1/logs")
            .header("content-type", "application/x-protobuf")
            .header("authorization", "Bearer legacy-key")
            .body(Bytes::from_static(body))
            .unwrap()
    }

    async fn start(server: &Server) -> (TelemetryGrants, GrantedHttpClient) {
        let grants = TelemetryGrants::start(
            &server.credentials(),
            "dep_1".to_string(),
            Arc::new(StubProver),
        )
        .await
        .unwrap();
        let client = grants.http_client();
        (grants, client)
    }

    #[tokio::test]
    async fn exports_go_to_the_granted_endpoint_with_the_granted_authorization() {
        let server = Server::start(vec![Plan::Grant {
            token: "grant-1",
            lifetime_secs: 300,
        }])
        .await;
        let (_grants, client) = start(&server).await;

        // The exporter's own URL and authorization are replaced; the payload is untouched.
        let response = client.send_bytes(export(b"\x0a\x01")).await.unwrap();

        assert_eq!(response.status(), 200);
        assert_eq!(
            server.exchanges(),
            vec![json!({
                "deploymentId": "dep_1",
                "proof": { "gcp": { "token": "token-for-mgr_test" } },
            })]
        );
        assert_eq!(
            server.appends(),
            vec![(
                "Bearer grant-1".to_string(),
                "application/x-protobuf".to_string(),
                b"\x0a\x01".to_vec()
            )]
        );
    }

    #[tokio::test]
    async fn one_grant_serves_many_exports() {
        let server = Server::start(vec![Plan::Grant {
            token: "grant-1",
            lifetime_secs: 300,
        }])
        .await;
        let (_grants, client) = start(&server).await;

        for _ in 0..3 {
            client.send_bytes(export(b"x")).await.unwrap();
        }

        assert_eq!(server.exchanges().len(), 1);
        assert_eq!(server.appends().len(), 3);
    }

    #[tokio::test]
    async fn a_log_endpoint_on_another_host_is_never_used() {
        let server = Server::start(vec![Plan::GrantElsewhere {
            endpoint: "http://127.0.0.2:9/v2/databases/x/tables/logs/append",
        }])
        .await;
        let (grants, client) = start(&server).await;

        let error = grants.inner.exchange().await.unwrap_err();
        assert!(error.to_string().contains("another host"), "{error}");
        client.send_bytes(export(b"x")).await.unwrap_err();
        assert!(server.appends().is_empty());
    }

    #[tokio::test]
    async fn credentials_about_to_expire_are_not_accepted() {
        let server = Server::start(vec![Plan::Grant {
            token: "late",
            lifetime_secs: 3,
        }])
        .await;
        let (grants, client) = start(&server).await;

        let error = grants.inner.exchange().await.unwrap_err();
        assert!(error.to_string().contains("about to expire"), "{error}");
        client.send_bytes(export(b"x")).await.unwrap_err();
        assert!(server.appends().is_empty());
    }

    #[tokio::test]
    async fn expired_credentials_are_replaced_before_the_next_export() {
        let server = Server::start(vec![
            Plan::Grant {
                token: "grant-1",
                lifetime_secs: 7,
            },
            Plan::Grant {
                token: "grant-2",
                lifetime_secs: 300,
            },
        ])
        .await;
        let (_grants, client) = start(&server).await;
        client.send_bytes(export(b"a")).await.unwrap();

        // Grant 1 stops being sent once less than MIN_REMAINING is left.
        tokio::time::sleep(Duration::from_millis(2300)).await;
        client.send_bytes(export(b"b")).await.unwrap();

        let appends = server.appends();
        assert_eq!(appends[0].0, "Bearer grant-1");
        assert_eq!(appends[1].0, "Bearer grant-2");
        assert_eq!(server.exchanges().len(), 2);
    }

    #[tokio::test]
    async fn credentials_that_ran_out_during_a_failed_refresh_are_not_sent() {
        let server = Server::start(vec![
            Plan::Grant {
                token: "grant-1",
                lifetime_secs: 12,
            },
            Plan::SlowStatus(500),
        ])
        .await;
        let (_grants, client) = start(&server).await;

        // Refresh is due 6 seconds before expiry, while the grant is still sendable.
        // The manager then takes long enough to fail that fewer than 5 seconds remain.
        tokio::time::sleep(Duration::from_millis(6300)).await;
        let error = client.send_bytes(export(b"a")).await.unwrap_err();

        assert!(error.to_string().contains("500"), "{error}");
        assert!(server.appends().is_empty(), "the old grant was sent");
    }

    #[tokio::test]
    async fn a_failed_exchange_waits_out_a_backoff_and_never_falls_back() {
        let server = Server::start(vec![Plan::Status(403)]).await;
        let (grants, client) = start(&server).await;
        assert_eq!(server.exchanges().len(), 1, "startup tried once");

        let first = client.send_bytes(export(b"a")).await.unwrap_err();
        let second = client.send_bytes(export(b"b")).await.unwrap_err();

        assert!(first.to_string().contains("retry"), "{first}");
        assert!(second.to_string().contains("retry"), "{second}");
        assert_eq!(
            server.exchanges().len(),
            1,
            "no second attempt inside the backoff"
        );
        assert!(
            server.appends().is_empty(),
            "nothing is sent without credentials"
        );
        let refused = grants.inner.exchange().await.unwrap_err();
        assert!(refused.to_string().contains("403"), "{refused}");

        server.set_plan(vec![Plan::Grant {
            token: "grant-1",
            lifetime_secs: 300,
        }]);
        tokio::time::sleep(BACKOFF_START + Duration::from_millis(200)).await;
        client.send_bytes(export(b"c")).await.unwrap();
        assert_eq!(server.appends()[0].0, "Bearer grant-1");
    }

    #[tokio::test]
    async fn credentials_the_data_service_refuses_are_dropped() {
        let server = Server::start(vec![
            Plan::Grant {
                token: "grant-1",
                lifetime_secs: 300,
            },
            Plan::Grant {
                token: "grant-2",
                lifetime_secs: 300,
            },
        ])
        .await;
        server
            .stub
            .rejected
            .lock()
            .unwrap()
            .push("Bearer grant-1".to_string());
        let (_grants, client) = start(&server).await;

        let refused = client.send_bytes(export(b"a")).await.unwrap();
        let accepted = client.send_bytes(export(b"b")).await.unwrap();

        assert_eq!(refused.status(), 403);
        assert_eq!(accepted.status(), 200);
        assert_eq!(server.appends()[1].0, "Bearer grant-2");
    }

    #[tokio::test]
    async fn a_redirect_from_the_manager_is_not_followed() {
        let server = Server::start(vec![Plan::Redirect]).await;
        let (grants, client) = start(&server).await;

        let error = grants.inner.exchange().await.unwrap_err();
        assert!(error.to_string().contains("307"), "{error}");
        client.send_bytes(export(b"x")).await.unwrap_err();
        assert_eq!(*server.stub.redirect_hits.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn the_background_task_replaces_credentials_before_they_expire() {
        let server = Server::start(vec![
            Plan::Grant {
                token: "grant-1",
                lifetime_secs: 8,
            },
            Plan::Grant {
                token: "grant-2",
                lifetime_secs: 300,
            },
        ])
        .await;
        let (_grants, _client) = start(&server).await;
        assert_eq!(server.exchanges().len(), 1);

        // Refresh is due at half the lifetime (4 s); the task checks every 5 s.
        tokio::time::sleep(CHECK_INTERVAL + Duration::from_millis(1500)).await;

        assert_eq!(server.exchanges().len(), 2);
    }

    #[tokio::test]
    async fn plain_http_is_refused_outside_loopback() {
        let credentials = OtlpCredentials {
            url: "http://manager.example/v1/telemetry/credentials".to_string(),
            audience: "mgr_test".to_string(),
        };

        let error = TelemetryGrants::start(&credentials, "dep_1".to_string(), Arc::new(StubProver))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("https"), "{error}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_real_otlp_exporter_sends_batches_with_the_granted_credentials() {
        let server = Server::start(vec![Plan::Grant {
            token: "grant-1",
            lifetime_secs: 300,
        }])
        .await;
        let (grants, _client) = start(&server).await;
        let logger = crate::otlp::OwnedOtlpLogger::from_config(crate::otlp::OtlpConfig {
            endpoint: "https://legacy.invalid/v1/logs".to_string(),
            headers: HashMap::from([(
                "authorization".to_string(),
                "Bearer legacy-key".to_string(),
            )]),
            service_name: "test-worker".to_string(),
            service_version: "1".to_string(),
            grants: Some(grants),
        })
        .unwrap();

        logger.emit_log("stdout", "hello from the worker", 1_700_000_000_000_000_000);
        logger.flush().await.unwrap();

        let appends = server.appends();
        assert_eq!(appends.len(), 1);
        assert_eq!(appends[0].0, "Bearer grant-1");
        assert_eq!(appends[0].1, "application/x-protobuf");
        assert!(
            String::from_utf8_lossy(&appends[0].2).contains("hello from the worker"),
            "the OTLP payload carries the log line"
        );
    }
}
