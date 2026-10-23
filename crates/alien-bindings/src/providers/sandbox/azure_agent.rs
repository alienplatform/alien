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

impl AzureSandbox {
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
                let response = self
                    .send(
                        sandbox_id,
                        self.request(sandbox_id, reqwest::Method::GET, "/v1/health?version=1")
                            .await?,
                        "sandbox.health",
                    )
                    .await?;
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
        // Only a definite 401 permits replay: transport failures and 5xx can hide
        // an already-started command and must never cause another execution.
        let mut replay = request
            .try_clone()
            .ok_or_else(invalid_route)?
            .build()
            .into_alien_error()
            .context(unreachable())?;
        let response = agent_protocol::send_raw(request, operation).await?;
        if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            return agent_protocol::check_response(response, operation).await;
        }
        let slot = self.connection_slot(sandbox_id).await;
        let mut cached = slot.lock().await;
        let current = cached.as_ref().ok_or_else(invalid_route)?;
        let old_authorization = replay.headers().get(reqwest::header::AUTHORIZATION);
        if old_authorization
            .is_some_and(|value| value.as_bytes() == format!("Bearer {}", current.token).as_bytes())
        {
            let fresh = self.fetch_connection(sandbox_id).await?;
            if fresh.token == current.token {
                return agent_protocol::check_response(response, operation).await;
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
        let renewed = reqwest::RequestBuilder::from_parts(connection.client, replay)
            .bearer_auth(connection.token);
        agent_protocol::send(renewed, operation).await
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

    #[tokio::test]
    async fn reconnect_activates_a_fresh_boot_and_does_not_retry_authorization_refusal() {
        for refused in [false, true] {
            let token = STANDARD.encode([4u8; 32]);
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let activation_calls = calls.clone();
            let health_calls = calls.clone();
            let router = Router::new()
                .route("/v1/activate", post(move || async move {
                    activation_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if refused { axum::http::StatusCode::UNAUTHORIZED } else { axum::http::StatusCode::NO_CONTENT }
                }))
                .route("/v1/health", axum::routing::get(move || async move {
                    assert_eq!(health_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
                    Json(serde_json::json!({ "protocolVersion": 1, "status": "ready", "imageCommand": { "status": "running" } }))
                }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
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
            // A fresh process is awaiting activation even though the provider is Running.
            // Session metadata must also select it through a rollback binding.
            let result = provider
                .get_or_create(CreateSandboxRequest {
                    sandbox_id: Some("session".into()),
                    ..Default::default()
                })
                .await;
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
}
