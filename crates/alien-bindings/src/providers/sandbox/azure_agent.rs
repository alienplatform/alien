//! Azure native file access authenticates the agent HTTP route without privileged native exec.

use std::{collections::HashMap, sync::Arc, time::Duration};

use alien_core::sandbox_image::{AGENT_PORT, NATIVE_TOKEN_PATH};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use tokio::sync::Mutex;

use super::AzureSandbox;
use crate::{
    error::{ErrorData, Result},
    providers::sandbox::agent_protocol::{self, AgentTransport},
};

pub(super) type Connections = Mutex<HashMap<String, Arc<Mutex<Option<Connection>>>>>;

#[derive(Clone)]
pub(super) struct Connection {
    client: reqwest::Client,
    url: reqwest::Url,
    token: String,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl Connection {
    fn matches_request(&self, request: &reqwest::Request) -> bool {
        request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .is_some_and(|value| value.as_bytes() == format!("Bearer {}", self.token).as_bytes())
            && request.url().origin() == self.url.origin()
    }

    fn request(&self, method: reqwest::Method, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.url.join(path).map_err(|_| invalid_route())?;
        Ok(self.client.request(method, url).bearer_auth(&self.token))
    }
}

pub(super) const TRANSPORT_LABEL: &str = "alienAgentTransport";
pub(super) const TRANSPORT_VERSION: &str = "native-file-v1";

/// Select from authenticated provider metadata, never from an HTTP failure or current release.
/// Missing means a session created by a released native binding; unknown versions fail closed.
pub(super) fn uses_agent(
    record: &alien_azure_clients::azure::sandbox_data_plane::Sandbox,
) -> Result<bool> {
    match record.labels.get(TRANSPORT_LABEL).map(String::as_str) {
        None => Ok(false),
        Some(TRANSPORT_VERSION) => Ok(true),
        Some(_) => Err(invalid_route()),
    }
}

/// A provider route miss is safe to refresh; a typed agent refusal is the final answer.
async fn not_found_response(
    response: reqwest::Response,
    operation: &str,
) -> Result<(AlienError<ErrorData>, bool)> {
    let status = response.status();
    let body = agent_protocol::error_body(response, operation).await?;
    // ApiError sends coded plain text for these errors, not the jobs JSON envelope.
    let from_agent = body.starts_with("PATH_NOT_FOUND:") || body.starts_with("JOB_NOT_FOUND:");
    Ok((
        AlienError::new(ErrorData::SandboxCommandFailed {
            failure: "agentRefused".into(),
            reason: format!("{operation} returned {status}: {body}"),
        }),
        from_agent,
    ))
}

impl AzureSandbox {
    async fn invalidate_connection(&self, sandbox_id: &str, request: &reqwest::Request) {
        let slot = self.connection_slot(sandbox_id).await;
        let mut cached = slot.lock().await;
        // A concurrent request may already have refreshed the route.
        if cached
            .as_ref()
            .is_some_and(|current| current.matches_request(request))
        {
            *cached = None;
        }
    }

    /// Select the agent transport only for images packaged with the new boot contract.
    pub fn with_agent(mut self, enabled: bool) -> Self {
        self.agent = enabled;
        self
    }

    pub(super) async fn session_uses_agent(
        &self,
        operation: &str,
        sandbox_id: &str,
    ) -> Result<bool> {
        Self::checked_sandbox_id(operation, sandbox_id)?;
        let record = self
            .read_sandbox(operation, sandbox_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "sandboxGone".into(),
                    reason: format!("{operation}: sandbox '{sandbox_id}' does not exist"),
                })
            })?;
        uses_agent(&record)
    }

    pub(super) async fn wait_for_agent(&self, sandbox_id: &str) -> Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            let ready = async {
                let request = self
                    .request(sandbox_id, reqwest::Method::GET, "/v1/health?version=1")
                    .await?;
                let sent = request
                    .try_clone()
                    .ok_or_else(invalid_route)?
                    .build()
                    .into_alien_error()
                    .context(unreachable())?;
                let response = self.send(sandbox_id, request, "sandbox.health").await?;
                let body: serde_json::Value = response
                    .json()
                    .await
                    .into_alien_error()
                    .context(unreachable())?;
                if body["protocolVersion"] == 1
                    && body["status"] == "ready"
                    && body["imageCommand"]["status"] != "failed"
                {
                    return Ok(());
                }
                if body["protocolVersion"] == 1 && body["status"] == "awaiting-network-verification"
                {
                    // Health is public and cannot reject a previous boot's token. A fresh
                    // boot at the same URL needs a new credential and policy-checked activation.
                    self.invalidate_connection(sandbox_id, &sent).await;
                }
                Err(AlienError::new(unreachable()))
            };
            match tokio::time::timeout_at(deadline, ready).await {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(error)) if error.retryable && tokio::time::Instant::now() < deadline => {}
                Ok(Err(error)) => return Err(error),
                Err(_) => return Err(AlienError::new(unreachable())),
            }
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + Duration::from_millis(500)).min(deadline),
            )
            .await;
        }
    }
}

fn unreachable() -> ErrorData {
    ErrorData::SandboxUnreachable {
        operation: "sandbox.agent".into(),
        reason: "the packaged Azure agent is not ready".into(),
    }
}

fn invalid_route() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxCommandFailed {
        failure: "agentTransportInvalid".into(),
        reason: "Azure did not report a valid agent HTTPS route and per-boot credential".into(),
    })
}

impl AzureSandbox {
    async fn connection_slot(&self, sandbox_id: &str) -> Arc<Mutex<Option<Connection>>> {
        self.agent_connections
            .lock()
            .await
            .entry(sandbox_id.to_owned())
            .or_default()
            .clone()
    }

    async fn fetch_connection(&self, sandbox_id: &str) -> Result<Connection> {
        let endpoint = self
            .client
            .agent_port_url(&self.sandbox_group, sandbox_id, AGENT_PORT)
            .await
            .map_err(|error| Self::failed("sandbox.agent", error))?;
        let url = reqwest::Url::parse(&endpoint).map_err(|_| invalid_route())?;
        // The URL comes from an authenticated provider response. Refuse redirects and unexpected
        // origins before attaching even this sandbox-scoped credential.
        let host = url.host_str().unwrap_or_default();
        let provider_route = url.scheme() == "https"
            && (host.ends_with(".adcproxy.io") || host.ends_with(".azurecontainerapps.io"));
        #[cfg(test)]
        let provider_route = provider_route || (url.scheme() == "http" && host == "127.0.0.1");
        if !provider_route
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(invalid_route());
        }
        let bytes = self
            .client
            .read_file(&self.sandbox_group, sandbox_id, NATIVE_TOKEN_PATH)
            .await
            .map_err(|error| {
                if super::is_not_found(&error) {
                    error.context(unreachable())
                } else {
                    Self::failed("sandbox.agentCredentials", error)
                }
            })?;
        let token = std::str::from_utf8(&bytes).map_err(|_| invalid_route())?;
        if token.len() != 44 || !STANDARD.decode(token).is_ok_and(|bytes| bytes.len() == 32) {
            return Err(invalid_route());
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .build()
            .into_alien_error()
            .context(unreachable())?;
        Ok(Connection {
            client,
            url,
            token: token.to_owned(),
        })
    }

    async fn activate_connection(&self, sandbox_id: &str, connection: &Connection) -> Result<()> {
        // Any operation can be the first one on a fresh boot. Activation starts image
        // code, so revalidate provider policy here even for a read-only file request.
        let record = self.judged_sandbox("sandbox.activate", sandbox_id).await?;
        if !uses_agent(&record)? {
            return Err(invalid_route());
        }
        agent_protocol::send(
            connection.request(reqwest::Method::POST, "/v1/activate")?,
            "sandbox.activate",
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl AgentTransport for AzureSandbox {
    fn provider(&self) -> &'static str {
        "azure"
    }

    async fn request(
        &self,
        sandbox_id: &str,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder> {
        Self::checked_sandbox_id("sandbox.agent", sandbox_id)?;
        let slot = self.connection_slot(sandbox_id).await;
        let mut cached = slot.lock().await;
        if cached.is_none() {
            let connection = self.fetch_connection(sandbox_id).await?;
            self.activate_connection(sandbox_id, &connection).await?;
            *cached = Some(connection);
        }
        cached
            .as_ref()
            .ok_or_else(invalid_route)?
            .request(method, path)
    }

    async fn send(
        &self,
        sandbox_id: &str,
        request: reqwest::RequestBuilder,
        operation: &str,
    ) -> Result<reqwest::Response> {
        // A failed connect cannot have delivered the request. A 401 or untyped 404 refuses it;
        // timeouts, broken responses and 5xx may hide an executed command and never replay.
        let mut replay = request
            .try_clone()
            .ok_or_else(invalid_route)?
            .build()
            .into_alien_error()
            .context(unreachable())?;
        let first =
            tokio::time::timeout(agent_protocol::AGENT_RESPONSE_TIMEOUT, request.send()).await;
        let response = match first {
            Ok(Ok(response)) if response.status() == reqwest::StatusCode::NOT_FOUND => {
                match not_found_response(response, operation).await {
                    Ok((error, true)) => return Err(error),
                    Ok((_, false)) => None,
                    Err(error) => {
                        self.invalidate_connection(sandbox_id, &replay).await;
                        return Err(error);
                    }
                }
            }
            Ok(Ok(response)) if response.status() == reqwest::StatusCode::UNAUTHORIZED => {
                Some(response)
            }
            Ok(Ok(response)) => {
                let result = agent_protocol::check_response(response, operation).await;
                if result.is_err() {
                    self.invalidate_connection(sandbox_id, &replay).await;
                }
                return result;
            }
            Ok(Err(error)) if error.is_connect() => None,
            Ok(Err(error)) => {
                self.invalidate_connection(sandbox_id, &replay).await;
                return Err(error)
                    .into_alien_error()
                    .context(agent_protocol::unanswered(
                        operation,
                        "the agent connection failed after connecting",
                    ));
            }
            Err(_) => {
                self.invalidate_connection(sandbox_id, &replay).await;
                return Err(AlienError::new(agent_protocol::unanswered(
                    operation,
                    "the agent did not answer before the response deadline",
                )));
            }
        };
        let slot = self.connection_slot(sandbox_id).await;
        let mut cached = slot.lock().await;
        let stale = cached
            .as_ref()
            .is_none_or(|current| current.matches_request(&replay));
        if stale {
            let previous = cached.take();
            let fresh = self.fetch_connection(sandbox_id).await?;
            if response
                .as_ref()
                .is_some_and(|response| response.status() == reqwest::StatusCode::UNAUTHORIZED)
                && previous
                    .as_ref()
                    .is_some_and(|current| fresh.token == current.token && fresh.url == current.url)
            {
                *cached = previous;
                drop(cached);
                return agent_protocol::check_response(
                    response.ok_or_else(invalid_route)?,
                    operation,
                )
                .await;
            }
            self.activate_connection(sandbox_id, &fresh).await?;
            *cached = Some(fresh);
        }
        let connection = cached.as_ref().ok_or_else(invalid_route)?.clone();
        drop(cached);
        let path = replay.url().path().to_owned();
        let query = replay.url().query().map(str::to_owned);
        *replay.url_mut() = connection.url.join(&path).map_err(|_| invalid_route())?;
        replay.url_mut().set_query(query.as_deref());
        replay.headers_mut().remove(reqwest::header::AUTHORIZATION);
        let renewed = reqwest::RequestBuilder::from_parts(connection.client.clone(), replay)
            .bearer_auth(&connection.token);
        let response = match agent_protocol::send_raw(renewed, operation).await {
            Ok(response) if response.status() == reqwest::StatusCode::NOT_FOUND => {
                match not_found_response(response, operation).await {
                    // The fresh route answered. Keep it for the next operation too.
                    Ok((error, true)) => return Err(error),
                    Ok((error, false)) | Err(error) => Err(error),
                }
            }
            Ok(response) => agent_protocol::check_response(response, operation).await,
            Err(error) => Err(error),
        };
        // A failed retry must not pin the stale route for the next operation.
        if response.is_err() {
            let mut cached = slot.lock().await;
            if cached.as_ref().is_some_and(|current| {
                current.url == connection.url && current.token == connection.token
            }) {
                *cached = None;
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest, Sandbox};
    use alien_azure_clients::azure::sandbox_data_plane::{
        MockSandboxDataPlaneApi, Sandbox as AzureRecord,
    };
    use alien_core::SandboxEgress;
    use axum::{http::HeaderMap, routing::post, Json, Router};
    use futures::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::{collections::BTreeMap, sync::Arc};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn reconnect_activates_a_fresh_boot_and_does_not_retry_authorization_refusal() {
        for (cached_boot, refused, resume) in [
            (false, false, false),
            (false, true, false),
            (true, false, false),
            (true, true, false),
            (true, false, true),
            (true, true, true),
        ] {
            let token = STANDARD.encode([4u8; 32]);
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let activation_calls = calls.clone();
            let health_calls = calls.clone();
            let router = Router::new()
                .route("/v1/activate", post(move |headers: HeaderMap| async move {
                    assert_eq!(headers["authorization"], format!("Bearer {}", STANDARD.encode([4u8; 32])));
                    activation_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if refused { axum::http::StatusCode::UNAUTHORIZED } else { axum::http::StatusCode::NO_CONTENT }
                }))
                .route("/v1/health", axum::routing::get(move || async move {
                    // Health is public: an old bearer token still receives HTTP 200.
                    let status = if health_calls.load(std::sync::atomic::Ordering::SeqCst) == 0 { "awaiting-network-verification" } else { "ready" };
                    Json(serde_json::json!({ "protocolVersion": 1, "status": status, "imageCommand": { "status": "running" } }))
                }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            let cached_url = url.clone();
            let count = 1;
            let mut native = MockSandboxDataPlaneApi::new();
            native.expect_get_sandbox().times(3).returning(|_, id| {
                Ok(AzureRecord {
                    id: id.into(),
                    labels: BTreeMap::from([(TRANSPORT_LABEL.into(), TRANSPORT_VERSION.into())]),
                    vnet_connection_name: None,
                    egress_policy: super::super::egress_policy(&SandboxEgress::None),
                    state: Some("Running".into()),
                })
            });
            native.expect_execute_shell_command().never();
            native
                .expect_agent_port_url()
                .times(count)
                .returning(move |_, _, _| Ok(url.clone()));
            native
                .expect_read_file()
                .times(count)
                .returning(move |_, _, path| {
                    assert_eq!(path, NATIVE_TOKEN_PATH);
                    Ok(token.as_bytes().to_vec())
                });
            let provider = AzureSandbox::new(
                Arc::new(native),
                "group".into(),
                "image".into(),
                SandboxEgress::None,
                None,
                "1000m".into(),
                "2048Mi".into(),
                None,
            )
            .with_agent(false);
            if cached_boot {
                *provider.connection_slot("session").await.lock().await = Some(Connection {
                    client: reqwest::Client::builder().no_proxy().build().unwrap(),
                    url: cached_url.parse().unwrap(),
                    token: STANDARD.encode([3u8; 32]),
                });
            }
            // A fresh process is awaiting activation even though the provider is Running.
            // Session metadata must also select it through a rollback binding.
            let ready = async {
                if resume {
                    provider.resume("session").await
                } else {
                    provider
                        .get_or_create(CreateSandboxRequest {
                            sandbox_id: Some("session".into()),
                            ..Default::default()
                        })
                        .await
                        .map(|_| ())
                }
            };
            let result = tokio::time::timeout(Duration::from_secs(5), ready)
                .await
                .expect("cached health must refresh and activate a fresh boot");
            if refused {
                let error = result.unwrap_err();
                assert_eq!(error.code, "SANDBOX_COMMAND_FAILED");
                assert!(!error.retryable);
            } else {
                result.unwrap();
            }
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            server.abort();
        }
    }

    #[tokio::test]
    async fn failed_resume_stops_only_the_session_it_woke() {
        for wake in [false, true] {
            for unknown_transport in [false, true] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("http://{}", listener.local_addr().unwrap());
                let router = Router::new().route(
                    "/v1/activate",
                    post(|| async { axum::http::StatusCode::FORBIDDEN }),
                );
                let server = tokio::spawn(async move {
                    axum::serve(listener, router).await.unwrap();
                });
                let mut native = MockSandboxDataPlaneApi::new();
                let mut reads = 0;
                native.expect_get_sandbox().returning(move |_, id| {
                    reads += 1;
                    Ok(AzureRecord {
                        id: id.into(),
                        labels: BTreeMap::from([(
                            TRANSPORT_LABEL.into(),
                            if unknown_transport {
                                "unknown"
                            } else {
                                TRANSPORT_VERSION
                            }
                            .into(),
                        )]),
                        vnet_connection_name: None,
                        egress_policy: super::super::egress_policy(&SandboxEgress::None),
                        state: Some(
                            if wake && reads <= 2 {
                                "Stopped"
                            } else {
                                "Running"
                            }
                            .into(),
                        ),
                    })
                });
                native
                    .expect_resume_sandbox()
                    .times(usize::from(wake))
                    .returning(|_, _| Ok(()));
                native
                    .expect_stop_sandbox()
                    .times(usize::from(wake))
                    .returning(|_, id| {
                        assert_eq!(id, "session");
                        Ok(())
                    });
                native.expect_delete_sandbox().never();
                native
                    .expect_agent_port_url()
                    .times(usize::from(!unknown_transport))
                    .returning(move |_, _, _| Ok(url.clone()));
                native
                    .expect_read_file()
                    .times(usize::from(!unknown_transport))
                    .returning(|_, _, _| Ok(STANDARD.encode([4u8; 32]).into_bytes()));
                let provider = AzureSandbox::new(
                    Arc::new(native),
                    "group".into(),
                    "image".into(),
                    SandboxEgress::None,
                    None,
                    "1000m".into(),
                    "2048Mi".into(),
                    None,
                );
                let error = provider.resume("session").await.unwrap_err();
                assert_eq!(error.code, "SANDBOX_COMMAND_FAILED");
                assert!(!error.retryable);
                server.abort();
            }
        }
    }

    #[tokio::test]
    async fn commands_cross_the_agent_port_without_native_exec() {
        let token = STANDARD.encode([7u8; 32]);
        let expected = format!("Bearer {token}");
        let router = Router::new().route("/v1/activate", post(|| async { axum::http::StatusCode::NO_CONTENT })).route("/v1/exec", post(move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
            let expected = expected.clone();
            async move {
                assert_eq!(headers["authorization"], expected);
                assert_eq!(body["command"], serde_json::json!(["echo", "a; $(false)"]));
                assert_eq!(body["timeoutMs"], 5000);
                "{\"t\":\"stdout\",\"seq\":0,\"data\":\"b2s=\"}\n{\"t\":\"exit\",\"code\":0,\"truncated\":false}\n"
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut native = MockSandboxDataPlaneApi::new();
        native.expect_execute_shell_command().never();
        native.expect_get_sandbox().times(3).returning(|_, id| {
            Ok(AzureRecord {
                id: id.into(),
                labels: BTreeMap::from([(TRANSPORT_LABEL.into(), TRANSPORT_VERSION.into())]),
                vnet_connection_name: None,
                egress_policy: super::super::egress_policy(&SandboxEgress::None),
                state: Some("Running".into()),
            })
        });
        native
            .expect_agent_port_url()
            .times(1)
            .returning(move |group, id, port| {
                assert_eq!((group, id, port), ("group", "session", AGENT_PORT));
                Ok(url.clone())
            });
        native
            .expect_read_file()
            .times(1)
            .returning(move |group, id, path| {
                assert_eq!((group, id, path), ("group", "session", NATIVE_TOKEN_PATH));
                Ok(token.as_bytes().to_vec())
            });
        let provider = AzureSandbox::new(
            Arc::new(native),
            "group".into(),
            "image".into(),
            SandboxEgress::None,
            None,
            "1000m".into(),
            "2048Mi".into(),
            None,
        )
        .with_agent(false);
        assert!(
            !provider.capabilities().jobs,
            "new sessions use the rollback binding"
        );
        assert!(
            provider.session_capabilities("session").await.unwrap().jobs,
            "the existing agent session supports jobs"
        );
        // A rollback binding still uses the session's agent; it must never native-exec it.
        let mut output = provider
            .run_command(
                "session",
                RunCommandRequest {
                    command: "echo".into(),
                    args: vec!["a; $(false)".into()],
                    cwd: None,
                    env: BTreeMap::new(),
                    timeout: Duration::from_secs(5),
                },
            )
            .await
            .unwrap();
        assert!(
            matches!(output.next().await.unwrap().unwrap(), CommandOutput::Stdout { data, .. } if data == b"ok")
        );
        assert!(matches!(
            output.next().await.unwrap().unwrap(),
            CommandOutput::Exit { code: 0, .. }
        ));
        assert!(output.next().await.is_none());
        server.abort();
    }
    #[tokio::test]
    async fn a_cached_connection_activates_new_boots_and_only_replays_a_changed_credential() {
        #[derive(Default)]
        struct Boot {
            generation: AtomicUsize,
            activated: AtomicUsize,
            executions: AtomicUsize,
            activations: AtomicUsize,
            refusal: AtomicUsize,
        }
        let boot = Arc::new(Boot::default());
        let activate = boot.clone();
        let execute = boot.clone();
        let router = Router::new()
            .route(
                "/v1/activate",
                post(move |headers: HeaderMap| {
                    let boot = activate.clone();
                    async move {
                        let generation = boot.generation.load(Ordering::SeqCst);
                        assert_eq!(
                            headers["authorization"],
                            format!("Bearer {}", STANDARD.encode([generation as u8; 32]))
                        );
                        boot.activated.store(generation + 1, Ordering::SeqCst);
                        boot.activations.fetch_add(1, Ordering::SeqCst);
                        axum::http::StatusCode::NO_CONTENT
                    }
                }),
            )
            .route(
                "/v1/exec",
                post(move |headers: HeaderMap| {
                    let boot = execute.clone();
                    async move {
                        let generation = boot.generation.load(Ordering::SeqCst);
                        if headers["authorization"]
                            != format!("Bearer {}", STANDARD.encode([generation as u8; 32]))
                            || boot.refusal.load(Ordering::SeqCst) == 1
                        {
                            return (axum::http::StatusCode::UNAUTHORIZED, "refused");
                        }
                        if boot.activated.load(Ordering::SeqCst) != generation + 1
                            || boot.refusal.load(Ordering::SeqCst) == 2
                        {
                            return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "not activated");
                        }
                        boot.executions.fetch_add(1, Ordering::SeqCst);
                        (axum::http::StatusCode::OK, "{\"t\":\"exit\",\"code\":0}\n")
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut native = MockSandboxDataPlaneApi::new();
        native.expect_execute_shell_command().never();
        // Five commands retain their policy reads; only two distinct boots activate.
        native.expect_get_sandbox().times(7).returning(|_, id| {
            Ok(AzureRecord {
                id: id.into(),
                labels: BTreeMap::from([(TRANSPORT_LABEL.into(), TRANSPORT_VERSION.into())]),
                vnet_connection_name: None,
                egress_policy: super::super::egress_policy(&SandboxEgress::None),
                state: Some("Running".into()),
            })
        });
        native
            .expect_agent_port_url()
            .times(3)
            .returning(move |_, _, _| Ok(url.clone()));
        let credentials = boot.clone();
        native
            .expect_read_file()
            .times(3)
            .returning(move |_, _, path| {
                assert_eq!(path, NATIVE_TOKEN_PATH);
                Ok(STANDARD
                    .encode([credentials.generation.load(Ordering::SeqCst) as u8; 32])
                    .into_bytes())
            });
        let provider = AzureSandbox::new(
            Arc::new(native),
            "group".into(),
            "image".into(),
            SandboxEgress::None,
            None,
            "1000m".into(),
            "2048Mi".into(),
            None,
        )
        .with_agent(false);
        let request = || RunCommandRequest {
            command: "true".into(),
            args: vec![],
            cwd: None,
            env: BTreeMap::new(),
            timeout: Duration::from_secs(5),
        };
        for generation in [0, 0, 1] {
            boot.generation.store(generation, Ordering::SeqCst);
            let mut stream = provider.run_command("session", request()).await.unwrap();
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                CommandOutput::Exit { code: 0, .. }
            ));
            assert!(stream.next().await.is_none());
        }
        assert_eq!(boot.activations.load(Ordering::SeqCst), 2);
        assert_eq!(boot.executions.load(Ordering::SeqCst), 3);
        boot.refusal.store(1, Ordering::SeqCst);
        let refusal = provider
            .run_command("session", request())
            .await
            .err()
            .expect("unchanged credential remains refused");
        assert_eq!(refusal.code, "SANDBOX_COMMAND_FAILED");
        assert!(!refusal.retryable);
        boot.refusal.store(2, Ordering::SeqCst);
        let unknown = provider
            .run_command("session", request())
            .await
            .err()
            .expect("5xx never replays a command");
        assert_eq!(unknown.code, "SANDBOX_OUTCOME_UNKNOWN");
        assert_eq!(boot.activations.load(Ordering::SeqCst), 2);
        assert_eq!(boot.executions.load(Ordering::SeqCst), 3);
        server.abort();
    }

    #[tokio::test]
    async fn a_native_session_does_not_advertise_jobs_through_an_upgraded_binding() {
        let mut native = MockSandboxDataPlaneApi::new();
        native.expect_get_sandbox().times(2).returning(|_, id| {
            Ok(AzureRecord {
                id: id.into(),
                labels: BTreeMap::new(),
                vnet_connection_name: None,
                egress_policy: super::super::egress_policy(&SandboxEgress::None),
                state: Some("Running".into()),
            })
        });
        native.expect_execute_shell_command().never();
        native.expect_agent_port_url().never();
        let provider = AzureSandbox::new(
            Arc::new(native),
            "group".into(),
            "image".into(),
            SandboxEgress::None,
            None,
            "1000m".into(),
            "2048Mi".into(),
            None,
        )
        .with_agent(true);
        assert!(
            provider.capabilities().jobs,
            "new sessions use the agent image"
        );
        let existing = provider.session_capabilities("native").await.unwrap();
        assert!(!existing.jobs);
        assert!(!existing.supervisor_isolation);
        let error = provider
            .start_job(
                "native",
                RunCommandRequest {
                    command: "true".into(),
                    args: vec![],
                    cwd: None,
                    env: BTreeMap::new(),
                    timeout: Duration::from_secs(5),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    }
    #[tokio::test]
    async fn cached_routes_refresh_once_after_not_found_or_connect_failure() {
        for connect_failure in [false, true] {
            for replacement_missing in [false, true] {
                let calls = Arc::new(AtomicUsize::new(0));
                let observed = calls.clone();
                let router = Router::new()
                    .route(
                        "/v1/activate",
                        post(|| async { axum::http::StatusCode::NO_CONTENT }),
                    )
                    .route(
                        "/v1/exec",
                        post(move || {
                            let observed = observed.clone();
                            async move {
                                observed.fetch_add(1, Ordering::SeqCst);
                                if replacement_missing {
                                    (axum::http::StatusCode::NOT_FOUND, "gone")
                                } else {
                                    (axum::http::StatusCode::OK, "{\"t\":\"exit\",\"code\":0}\n")
                                }
                            }
                        }),
                    );
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("http://{}/", listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    axum::serve(listener, router).await.unwrap();
                });
                let stale_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let stale_url = format!("http://{}/", stale_listener.local_addr().unwrap());
                let stale_server = if connect_failure {
                    drop(stale_listener);
                    None
                } else {
                    Some(tokio::spawn(async move {
                        axum::serve(stale_listener, Router::new()).await.unwrap();
                    }))
                };
                let mut native = MockSandboxDataPlaneApi::new();
                native.expect_execute_shell_command().never();
                native.expect_get_sandbox().times(2).returning(|_, id| {
                    Ok(AzureRecord {
                        id: id.into(),
                        labels: BTreeMap::from([(
                            TRANSPORT_LABEL.into(),
                            TRANSPORT_VERSION.into(),
                        )]),
                        vnet_connection_name: None,
                        egress_policy: super::super::egress_policy(&SandboxEgress::None),
                        state: Some("Running".into()),
                    })
                });
                native
                    .expect_agent_port_url()
                    .times(1)
                    .returning(move |_, _, _| Ok(url.clone()));
                native
                    .expect_read_file()
                    .times(1)
                    .returning(|_, _, _| Ok(STANDARD.encode([7u8; 32]).into_bytes()));
                let provider = AzureSandbox::new(
                    Arc::new(native),
                    "group".into(),
                    "image".into(),
                    SandboxEgress::None,
                    None,
                    "1000m".into(),
                    "2048Mi".into(),
                    None,
                );
                // An already-activated connection becomes stale when the provider moves the port.
                *provider.connection_slot("session").await.lock().await = Some(Connection {
                    client: reqwest::Client::builder().no_proxy().build().unwrap(),
                    url: stale_url.parse().unwrap(),
                    token: STANDARD.encode([7u8; 32]),
                });
                let result = provider
                    .run_command(
                        "session",
                        RunCommandRequest {
                            command: "true".into(),
                            args: vec![],
                            cwd: None,
                            env: BTreeMap::new(),
                            timeout: Duration::from_secs(5),
                        },
                    )
                    .await;
                if replacement_missing {
                    let error = result
                        .err()
                        .expect("replacement 404 must stop after one retry");
                    assert_eq!(error.code, "SANDBOX_COMMAND_FAILED");
                    assert!(provider
                        .connection_slot("session")
                        .await
                        .lock()
                        .await
                        .is_none());
                } else {
                    let mut stream = result.unwrap_or_else(|error| {
                        panic!("stale route connect={connect_failure}: {error}")
                    });
                    assert!(matches!(
                        stream.next().await.unwrap().unwrap(),
                        CommandOutput::Exit { code: 0, .. }
                    ));
                    assert!(stream.next().await.is_none());
                }
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                server.abort();
                if let Some(server) = stale_server {
                    server.abort();
                }
            }
        }
    }

    #[tokio::test]
    async fn ambiguous_delivery_is_not_replayed_and_the_next_call_refreshes() {
        for timeout in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            let router = Router::new()
                .route(
                    "/v1/activate",
                    post(|| async { axum::http::StatusCode::NO_CONTENT }),
                )
                .route(
                    "/v1/exec",
                    post(move || {
                        let observed = observed.clone();
                        async move {
                            observed.fetch_add(1, Ordering::SeqCst);
                            "{\"t\":\"exit\",\"code\":0}\n"
                        }
                    }),
                );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            let stale_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let stale_url = format!("http://{}/", stale_listener.local_addr().unwrap());
            let delivered = Arc::new(AtomicUsize::new(0));
            let observed = delivered.clone();
            let stale_server = tokio::spawn(async move {
                loop {
                    let (mut socket, _) = stale_listener.accept().await.unwrap();
                    let observed = observed.clone();
                    tokio::spawn(async move {
                        let mut buffer = [0u8; 4096];
                        assert!(socket.read(&mut buffer).await.unwrap() > 0);
                        observed.fetch_add(1, Ordering::SeqCst);
                        // The request reached the server, but no response reaches the caller.
                        if timeout {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        drop(socket);
                    });
                }
            });
            let mut native = MockSandboxDataPlaneApi::new();
            native.expect_execute_shell_command().never();
            native.expect_get_sandbox().times(3).returning(|_, id| {
                Ok(AzureRecord {
                    id: id.into(),
                    labels: BTreeMap::from([(TRANSPORT_LABEL.into(), TRANSPORT_VERSION.into())]),
                    vnet_connection_name: None,
                    egress_policy: super::super::egress_policy(&SandboxEgress::None),
                    state: Some("Running".into()),
                })
            });
            native
                .expect_agent_port_url()
                .times(1)
                .returning(move |_, _, _| Ok(url.clone()));
            native
                .expect_read_file()
                .times(1)
                .returning(|_, _, _| Ok(STANDARD.encode([7u8; 32]).into_bytes()));
            let provider = AzureSandbox::new(
                Arc::new(native),
                "group".into(),
                "image".into(),
                SandboxEgress::None,
                None,
                "1000m".into(),
                "2048Mi".into(),
                None,
            );
            *provider.connection_slot("session").await.lock().await = Some(Connection {
                client: reqwest::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_millis(100))
                    .build()
                    .unwrap(),
                url: stale_url.parse().unwrap(),
                token: STANDARD.encode([7u8; 32]),
            });
            let request = || RunCommandRequest {
                command: "true".into(),
                args: vec![],
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(5),
            };
            let error = provider
                .run_command("session", request())
                .await
                .err()
                .expect("delivery is ambiguous");
            assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN");
            assert!(!error.retryable);
            assert_eq!(delivered.load(Ordering::SeqCst), 1);
            assert_eq!(calls.load(Ordering::SeqCst), 0, "must not replay");
            let mut stream = provider
                .run_command("session", request())
                .await
                .unwrap_or_else(|error| {
                    panic!("next call must refresh timeout={timeout}: {error}")
                });
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                CommandOutput::Exit { code: 0, .. }
            ));
            assert!(stream.next().await.is_none());
            assert_eq!(delivered.load(Ordering::SeqCst), 1);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            stale_server.abort();
            server.abort();
        }
    }
    #[tokio::test]
    async fn typed_not_found_preserves_route_without_replaying() {
        for code in ["PATH_NOT_FOUND", "JOB_NOT_FOUND"] {
            for stale_first in [false, true] {
                let calls = Arc::new(AtomicUsize::new(0));
                let observed = calls.clone();
                let router = Router::new()
                    .route(
                        "/v1/activate",
                        post(|| async { axum::http::StatusCode::NO_CONTENT }),
                    )
                    .route(
                        "/v1/missing",
                        axum::routing::get(move || {
                            observed.fetch_add(1, Ordering::SeqCst);
                            async move {
                                let error = match code {
                                    "PATH_NOT_FOUND" => {
                                        alien_sandbox_agent::error::ErrorData::PathNotFound {
                                            path: "missing".into(),
                                        }
                                    }
                                    "JOB_NOT_FOUND" => {
                                        alien_sandbox_agent::error::ErrorData::JobNotFound {
                                            job_id: "missing".into(),
                                        }
                                    }
                                    _ => unreachable!(),
                                };
                                alien_sandbox_agent::server::ApiError::from(AlienError::new(error))
                            }
                        }),
                    );
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("http://{}/", listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    axum::serve(listener, router).await.unwrap();
                });
                let stale_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let stale_url = format!("http://{}/", stale_listener.local_addr().unwrap());
                let stale_server = tokio::spawn(async move {
                    axum::serve(stale_listener, Router::new()).await.unwrap();
                });
                let mut native = MockSandboxDataPlaneApi::new();
                native.expect_execute_shell_command().never();
                native
                    .expect_get_sandbox()
                    .times(usize::from(stale_first))
                    .returning(|_, id| {
                        Ok(AzureRecord {
                            id: id.into(),
                            labels: BTreeMap::from([(
                                TRANSPORT_LABEL.into(),
                                TRANSPORT_VERSION.into(),
                            )]),
                            vnet_connection_name: None,
                            egress_policy: super::super::egress_policy(&SandboxEgress::None),
                            state: Some("Running".into()),
                        })
                    });
                let fresh_url = url.clone();
                native
                    .expect_agent_port_url()
                    .times(usize::from(stale_first))
                    .returning(move |_, _, _| Ok(fresh_url.clone()));
                native
                    .expect_read_file()
                    .times(usize::from(stale_first))
                    .returning(|_, _, _| Ok(STANDARD.encode([7u8; 32]).into_bytes()));
                let provider = AzureSandbox::new(
                    Arc::new(native),
                    "group".into(),
                    "image".into(),
                    SandboxEgress::None,
                    None,
                    "1000m".into(),
                    "2048Mi".into(),
                    None,
                );
                *provider.connection_slot("session").await.lock().await = Some(Connection {
                    client: reqwest::Client::builder().no_proxy().build().unwrap(),
                    url: if stale_first {
                        stale_url.parse().unwrap()
                    } else {
                        url.parse().unwrap()
                    },
                    token: STANDARD.encode([7u8; 32]),
                });
                for expected_calls in 1..=2 {
                    let request = provider
                        .request("session", reqwest::Method::GET, "/v1/missing")
                        .await
                        .unwrap();
                    let error = provider
                        .send("session", request, "sandbox.readFile")
                        .await
                        .unwrap_err();
                    assert_eq!(error.code, "SANDBOX_COMMAND_FAILED");
                    assert!(error.to_string().contains(code), "{error}");
                    assert!(!error.retryable);
                    assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
                    let slot = provider.connection_slot("session").await;
                    let cached = slot.lock().await;
                    assert_eq!(
                        cached.as_ref().expect("typed 404 keeps route").url.as_str(),
                        url
                    );
                }
                server.abort();
                stale_server.abort();
            }
        }
    }
    #[tokio::test]
    async fn oversized_error_bodies_are_bounded_without_replay() {
        for status in [
            "401 Unauthorized",
            "403 Forbidden",
            "404 Not Found",
            "500 Internal Server Error",
        ] {
            for operation in ["sandbox.runCommand", "sandbox.jobStart", "sandbox.readFile"] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("http://{}/", listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = [0; 4096];
                    assert!(socket.read(&mut request).await.unwrap() > 0);
                    let headers = format!("HTTP/1.1 {status}\r\nContent-Length: 100000\r\n\r\n");
                    socket.write_all(headers.as_bytes()).await.unwrap();
                    socket.write_all(&[b'x'; 4097]).await.unwrap();
                    tokio::time::sleep(agent_protocol::AGENT_RESPONSE_TIMEOUT * 10).await;
                });
                let mut native = MockSandboxDataPlaneApi::new();
                // A 401 may check the credential, but no failure may replay the operation.
                native
                    .expect_read_file()
                    .returning(|_, _, _| Ok(STANDARD.encode([7u8; 32]).into_bytes()));
                let fetched_url = url.clone();
                native
                    .expect_agent_port_url()
                    .returning(move |_, _, _| Ok(fetched_url.clone()));
                let provider = AzureSandbox::new(
                    Arc::new(native),
                    "group".into(),
                    "image".into(),
                    SandboxEgress::None,
                    None,
                    "1000m".into(),
                    "2048Mi".into(),
                    None,
                );
                *provider.connection_slot("session").await.lock().await = Some(Connection {
                    client: reqwest::Client::builder().no_proxy().build().unwrap(),
                    url: url.parse().unwrap(),
                    token: STANDARD.encode([7u8; 32]),
                });
                let request = provider
                    .request("session", reqwest::Method::GET, "/v1/missing")
                    .await
                    .unwrap();
                let error = provider
                    .send("session", request, operation)
                    .await
                    .unwrap_err();
                assert!(error.to_string().contains("4096"), "{status}: {error}");
                assert_eq!(
                    error.code,
                    if operation == "sandbox.readFile" {
                        "SANDBOX_UNREACHABLE"
                    } else {
                        "SANDBOX_OUTCOME_UNKNOWN"
                    }
                );
                assert_eq!(error.retryable, operation == "sandbox.readFile");
                if !status.starts_with("401") {
                    assert!(provider
                        .connection_slot("session")
                        .await
                        .lock()
                        .await
                        .is_none());
                }
                server.abort();
            }
        }
    }

    #[tokio::test]
    async fn unfinished_not_found_body_is_bounded_and_never_replayed() {
        for disconnect in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                assert!(socket.read(&mut request).await.unwrap() > 0);
                socket
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 100\r\n\r\nPATH_NOT_FOUND:",
                    )
                    .await
                    .unwrap();
                if !disconnect {
                    tokio::time::sleep(agent_protocol::AGENT_RESPONSE_TIMEOUT * 10).await;
                }
            });
            let mut native = MockSandboxDataPlaneApi::new();
            native.expect_agent_port_url().never();
            let provider = AzureSandbox::new(
                Arc::new(native),
                "group".into(),
                "image".into(),
                SandboxEgress::None,
                None,
                "1000m".into(),
                "2048Mi".into(),
                None,
            );
            *provider.connection_slot("session").await.lock().await = Some(Connection {
                client: reqwest::Client::builder().no_proxy().build().unwrap(),
                url: url.parse().unwrap(),
                token: STANDARD.encode([7u8; 32]),
            });
            let request = provider
                .request("session", reqwest::Method::GET, "/v1/missing")
                .await
                .unwrap();
            let result = tokio::time::timeout(
                agent_protocol::AGENT_RESPONSE_TIMEOUT * 3,
                provider.send("session", request, "sandbox.runCommand"),
            )
            .await
            .expect("an unfinished 404 must not hang the operation");
            let error = result.unwrap_err();
            assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN");
            assert!(!error.retryable);
            assert!(provider
                .connection_slot("session")
                .await
                .lock()
                .await
                .is_none());
            server.abort();
        }
    }
    #[tokio::test]
    async fn stalled_error_body_is_bounded_and_never_replayed() {
        for status in ["403 Forbidden", "500 Internal Server Error"] {
            for operation in ["sandbox.runCommand", "sandbox.jobStart", "sandbox.readFile"] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("http://{}/", listener.local_addr().unwrap());
                let server = tokio::spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = [0; 4096];
                    assert!(socket.read(&mut request).await.unwrap() > 0);
                    socket
                        .write_all(
                            format!(
                                "HTTP/1.1 {status}\r\nContent-Length: 100\r\n\r\npartial error"
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                    std::future::pending::<()>().await;
                });
                let mut native = MockSandboxDataPlaneApi::new();
                native.expect_agent_port_url().never();
                let provider = AzureSandbox::new(
                    Arc::new(native),
                    "group".into(),
                    "image".into(),
                    SandboxEgress::None,
                    None,
                    "1000m".into(),
                    "2048Mi".into(),
                    None,
                );
                *provider.connection_slot("session").await.lock().await = Some(Connection {
                    client: reqwest::Client::builder().no_proxy().build().unwrap(),
                    url: url.parse().unwrap(),
                    token: STANDARD.encode([7u8; 32]),
                });
                let request = provider
                    .request("session", reqwest::Method::POST, "/v1/test")
                    .await
                    .unwrap();
                let result = tokio::time::timeout(
                    agent_protocol::AGENT_RESPONSE_TIMEOUT * 3,
                    provider.send("session", request, operation),
                )
                .await
                .expect("a stalled error body must not hang the operation");
                let error = result.unwrap_err();
                let idempotent = operation == "sandbox.readFile";
                assert_eq!(
                    error.code,
                    if idempotent {
                        "SANDBOX_UNREACHABLE"
                    } else {
                        "SANDBOX_OUTCOME_UNKNOWN"
                    }
                );
                assert_eq!(error.retryable, idempotent);
                assert!(error.to_string().contains("body"), "{error}");
                assert!(provider
                    .connection_slot("session")
                    .await
                    .lock()
                    .await
                    .is_none());
                server.abort();
                assert!(server.await.unwrap_err().is_cancelled());
            }
        }
    }

    #[tokio::test]
    async fn stalled_unauthorized_body_does_not_block_same_session() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let router = Router::new()
            .route(
                "/v1/first",
                post(move || {
                    observed.fetch_add(1, Ordering::SeqCst);
                    async {
                        (
                            axum::http::StatusCode::UNAUTHORIZED,
                            axum::body::Body::from_stream(futures::stream::pending::<
                                std::result::Result<bytes::Bytes, std::io::Error>,
                            >()),
                        )
                    }
                }),
            )
            .route(
                "/v1/second",
                post(|| async { axum::http::StatusCode::NO_CONTENT }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let refreshed = Arc::new(tokio::sync::Notify::new());
        let notify_refresh = refreshed.clone();
        let mut native = MockSandboxDataPlaneApi::new();
        let fresh_url = url.clone();
        native
            .expect_agent_port_url()
            .times(1)
            .returning(move |_, _, _| Ok(fresh_url.clone()));
        native
            .expect_read_file()
            .times(1)
            .returning(move |_, _, _| {
                notify_refresh.notify_one();
                Ok(STANDARD.encode([7u8; 32]).into_bytes())
            });
        native.expect_get_sandbox().never();
        let provider = AzureSandbox::new(
            Arc::new(native),
            "group".into(),
            "image".into(),
            SandboxEgress::None,
            None,
            "1000m".into(),
            "2048Mi".into(),
            None,
        );
        *provider.connection_slot("session").await.lock().await = Some(Connection {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            url: url.parse().unwrap(),
            token: STANDARD.encode([7u8; 32]),
        });
        let request = provider
            .request("session", reqwest::Method::POST, "/v1/first")
            .await
            .unwrap();
        let first = provider.send("session", request, "sandbox.runCommand");
        tokio::pin!(first);
        tokio::time::timeout(agent_protocol::AGENT_RESPONSE_TIMEOUT * 3, async {
            tokio::select! {
                result = &mut first => panic!("first call should be reading its stalled body: {result:?}"),
                _ = refreshed.notified() => {}
            }
        }).await.expect("the first call must refresh its credential");
        // Leave the first future parked in its body read. The second call must finish
        // without needing the first call to finish or its body deadline to expire.
        let second = tokio::time::timeout(agent_protocol::AGENT_RESPONSE_TIMEOUT * 3, async {
            let request = provider
                .request("session", reqwest::Method::POST, "/v1/second")
                .await
                .unwrap();
            provider.send("session", request, "sandbox.readFile").await
        })
        .await
        .expect("a stalled 401 body must not hold the session cache lock")
        .unwrap();
        assert_eq!(second.status(), reqwest::StatusCode::NO_CONTENT);
        let error = tokio::time::timeout(agent_protocol::AGENT_RESPONSE_TIMEOUT * 3, first)
            .await
            .expect("the stalled 401 must meet its body deadline")
            .unwrap_err();
        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN");
        assert!(!error.retryable);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the first operation must not replay"
        );
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}
