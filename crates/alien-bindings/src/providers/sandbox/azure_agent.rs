//! Azure native file access authenticates the agent HTTP route without privileged native exec.

use std::time::Duration;

use alien_core::sandbox_image::{AGENT_PORT, NATIVE_TOKEN_PATH};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};

use super::AzureSandbox;
use crate::{
    error::{ErrorData, Result},
    providers::sandbox::agent_protocol::AgentTransport,
};

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
                super::super::agent_protocol::send(
                    self.request(sandbox_id, reqwest::Method::POST, "/v1/activate")
                        .await?,
                    "sandbox.activate",
                )
                .await?;
                let response = super::super::agent_protocol::send(
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
        let endpoint = self
            .client
            .agent_port_url(&self.sandbox_group, sandbox_id, AGENT_PORT)
            .await
            .map_err(|error| Self::failed("sandbox.agent", error))?;
        let mut url = reqwest::Url::parse(&endpoint).map_err(|_| invalid_route())?;
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
        url = url.join(path).map_err(|_| invalid_route())?;
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
        Ok(client.request(method, url).bearer_auth(token))
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
            let count = if refused { 1 } else { 2 };
            let mut native = MockSandboxDataPlaneApi::new();
            native.expect_get_sandbox().times(2).returning(|_, id| {
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
    async fn commands_cross_the_agent_port_without_native_exec() {
        let token = STANDARD.encode([7u8; 32]);
        let expected = format!("Bearer {token}");
        let router = Router::new().route("/v1/exec", post(move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
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
        native.expect_get_sandbox().times(1).returning(|_, id| {
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
}
