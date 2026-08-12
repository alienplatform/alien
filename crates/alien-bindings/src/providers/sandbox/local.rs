//! Local sandbox provider.
//!
//! Speaks to the local sandbox manager over its authenticated loopback route. It cannot call
//! the manager in process — `alien-local` depends on this crate, so a direct call would be a
//! dependency cycle — and handing the workload a Docker socket instead would give every
//! application the ability to escape its own sandbox.

use std::collections::BTreeMap;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures::stream::{self, BoxStream};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;

use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, CommandOutput, CreateSessionRequest, PreviewCapability, RunCommandRequest, Sandbox,
    SandboxSession, SandboxSessionState,
};
use alien_core::bindings::LocalSandboxBinding;
use alien_core::{Platform, SandboxCapabilities};
use alien_error::{AlienError, Context, IntoAlienError};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionBody {
    session_id: String,
    #[allow(dead_code)]
    container_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "stream", content = "dataBase64")]
enum OutputFrame {
    Stdout(String),
    Stderr(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecResponse {
    output: Vec<OutputFrame>,
    exit_code: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadFileResponse {
    contents_base64: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewResponse {
    endpoint: String,
    allowed_ports: Vec<u16>,
}

/// A Sandbox backed by the local manager.
#[derive(Debug)]
pub struct LocalSandbox {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl LocalSandbox {
    /// Builds a provider from its binding, reading the route token from the path it names.
    ///
    /// The binding carries a path rather than the token itself: a binding is serialized into
    /// the workload's environment, and a secret there is a secret in state.
    pub async fn new(binding_name: &str, binding: &LocalSandboxBinding) -> Result<Self> {
        let base_url = binding
            .manager_url
            .clone()
            .into_value(binding_name, "managerUrl")
            .map_err(|error| {
                AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    env_var: alien_core::bindings::binding_env_var_name(binding_name),
                    reason: error.to_string(),
                })
            })?;

        let token_path = binding
            .token_path
            .clone()
            .into_value(binding_name, "tokenPath")
            .map_err(|error| {
                AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    env_var: alien_core::bindings::binding_env_var_name(binding_name),
                    reason: error.to_string(),
                })
            })?;

        let token = tokio::fs::read_to_string(&token_path)
            .await
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                env_var: alien_core::bindings::binding_env_var_name(binding_name),
                reason: format!("could not read the sandbox route token at '{token_path}'"),
            })?;

        Ok(Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.trim().to_string(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    async fn send<T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::RequestBuilder,
        operation: &str,
    ) -> Result<T> {
        let response = self
            .request(request, operation)
            .await?
            .json::<T>()
            .await
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "local-sandbox".to_string(),
                binding_name: operation.to_string(),
                field: "body".to_string(),
                response_json: "the sandbox route returned a body this provider cannot parse"
                    .to_string(),
            })?;

        Ok(response)
    }

    async fn request(
        &self,
        request: reqwest::RequestBuilder,
        operation: &str,
    ) -> Result<reqwest::Response> {
        let response = request
            .bearer_auth(&self.token)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::OperationNotSupported {
                operation: operation.to_string(),
                reason: "the local sandbox route is unreachable".to_string(),
            })?;

        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        // Read the body before reporting: the route puts the actual cause there, and a bare
        // status turns a specific failure into a guess.
        let body = response.text().await.unwrap_or_default();
        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: operation.to_string(),
            reason: format!("the local sandbox route returned {status}: {body}"),
        }))
    }

    fn unsupported(&self, capability: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: capability.to_string(),
            reason: "not supported on local".to_string(),
        })
    }
}

impl Binding for LocalSandbox {}

#[async_trait]
impl Sandbox for LocalSandbox {
    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::for_platform(Platform::Local).expect("Local has a sandbox backend")
    }

    async fn create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        let session_id = request
            .session_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());

        // Only the id. Image, limits, egress and preview ports come from the controller's
        // template, so an application cannot raise its own ceilings.
        let created: SessionBody = self
            .send(
                self.client
                    .post(self.url("/v1/sessions"))
                    .json(&json!({ "sessionId": session_id })),
                "sandbox.create",
            )
            .await?;

        Ok(SandboxSession {
            session_id: created.session_id,
            state: SandboxSessionState::Running,
            generation: 1,
        })
    }

    async fn get(&self, session_id: &str) -> Result<Option<SandboxSession>> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .find(|session| session.session_id == session_id))
    }

    async fn get_or_create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        if let Some(id) = request.session_id.as_deref() {
            if let Some(existing) = self.get(id).await? {
                return Ok(existing);
            }
        }

        self.create(request).await
    }

    async fn list(&self) -> Result<Vec<SandboxSession>> {
        let sessions: Vec<SessionBody> = self
            .send(self.client.get(self.url("/v1/sessions")), "sandbox.list")
            .await?;

        Ok(sessions
            .into_iter()
            .map(|session| SandboxSession {
                session_id: session.session_id,
                state: SandboxSessionState::Running,
                generation: 1,
            })
            .collect())
    }

    async fn run_command(
        &self,
        session_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        if request.deadline.is_zero() {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox.runCommand".to_string(),
                reason: "a command must carry a non-zero deadline".to_string(),
            }));
        }

        // The deadline bounds the untrusted code, not the caller's patience. The route runs the
        // command to completion, so the only lever that actually stops one past its ceiling is
        // ending the session — a command that overran took the session with it.
        let response: ExecResponse = match tokio::time::timeout(
            request.deadline,
            self.send(
                self.client
                    .post(self.url(&format!("/v1/sessions/{session_id}/exec")))
                    .json(&json!({ "command": request.command })),
                "sandbox.runCommand",
            ),
        )
        .await
        {
            Ok(inner) => inner?,
            Err(_) => {
                self.terminate(session_id).await?;
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "deadlineExceeded".to_string(),
                    reason: format!(
                        "the command exceeded its {}s deadline and the session was terminated",
                        request.deadline.as_secs()
                    ),
                }));
            }
        };

        let mut frames: Vec<Result<CommandOutput>> = Vec::new();
        for (index, frame) in response.output.into_iter().enumerate() {
            let seq = index as u64;
            let decoded = match &frame {
                OutputFrame::Stdout(data) | OutputFrame::Stderr(data) => BASE64
                    .decode(data)
                    .into_alien_error()
                    .context(ErrorData::UnexpectedResponseFormat {
                        provider: "local-sandbox".to_string(),
                        binding_name: "sandbox.runCommand".to_string(),
                        field: "output".to_string(),
                        response_json: "an output frame was not valid base64".to_string(),
                    })?,
            };

            frames.push(Ok(match frame {
                OutputFrame::Stdout(_) => CommandOutput::Stdout { seq, data: decoded },
                OutputFrame::Stderr(_) => CommandOutput::Stderr { seq, data: decoded },
            }));
        }

        frames.push(Ok(CommandOutput::Exit {
            code: response.exit_code as i32,
            truncated: false,
        }));

        Ok(Box::pin(stream::iter(frames)))
    }

    async fn read_file(&self, session_id: &str, path: &str) -> Result<Vec<u8>> {
        let response: ReadFileResponse = self
            .send(
                self.client
                    .get(self.url(&format!("/v1/sessions/{session_id}/files")))
                    .query(&[("path", path)]),
                "sandbox.readFile",
            )
            .await?;

        BASE64
            .decode(response.contents_base64)
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "local-sandbox".to_string(),
                binding_name: "sandbox.readFile".to_string(),
                field: "contentsBase64".to_string(),
                response_json: "file contents were not valid base64".to_string(),
            })
    }

    async fn write_files(&self, session_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        for (path, contents) in files {
            self.request(
                self.client
                    .put(self.url(&format!("/v1/sessions/{session_id}/files")))
                    .json(&json!({
                        "path": path,
                        "contentsBase64": BASE64.encode(contents),
                    })),
                "sandbox.writeFiles",
            )
            .await?;
        }

        Ok(())
    }

    async fn mkdir(&self, session_id: &str, path: &str) -> Result<()> {
        let request = RunCommandRequest {
            command: vec!["/bin/mkdir".to_string(), "-p".to_string(), path.to_string()],
            working_directory: None,
            env: BTreeMap::new(),
            deadline: std::time::Duration::from_secs(30),
        };

        // Drain to the terminal frame: the command has already run by the time the stream is
        // built, but a non-zero exit means the directory does not exist and the caller must hear
        // about it rather than discover it on the next write.
        let mut frames = self.run_command(session_id, request).await?;
        while let Some(frame) = frames.next().await {
            if let CommandOutput::Exit { code, .. } = frame? {
                if code != 0 {
                    return Err(AlienError::new(ErrorData::OperationNotSupported {
                        operation: "sandbox.mkdir".to_string(),
                        reason: format!("mkdir '{path}' exited with {code}"),
                    }));
                }
            }
        }

        Ok(())
    }

    async fn preview(&self, session_id: &str, port: u16) -> Result<PreviewCapability> {
        let response: PreviewResponse = self
            .send(
                self.client
                    .get(self.url(&format!("/v1/sessions/{session_id}/preview")))
                    .query(&[("port", port.to_string())]),
                "sandbox.preview",
            )
            .await?;

        Ok(PreviewCapability {
            endpoint: response.endpoint,
            // The port is published on loopback, so reaching it needs no credential beyond
            // being on the developer's machine. Stated rather than implied by an empty map.
            headers: BTreeMap::new(),
            allowed_ports: response.allowed_ports,
            expires_in_seconds: 0,
        })
    }

    async fn suspend(&self, _session_id: &str) -> Result<()> {
        Err(self.unsupported("suspendResume"))
    }

    async fn resume(&self, _session_id: &str) -> Result<()> {
        Err(self.unsupported("suspendResume"))
    }

    async fn snapshot(&self, _session_id: &str) -> Result<String> {
        Err(self.unsupported("snapshot"))
    }

    async fn terminate(&self, session_id: &str) -> Result<()> {
        self.request(
            self.client
                .delete(self.url(&format!("/v1/sessions/{session_id}"))),
            "sandbox.terminate",
        )
        .await?;

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
