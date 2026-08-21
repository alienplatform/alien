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
use crate::providers::sandbox::{guard_for, Bounded, DeadlineReport};
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

    /// Runs one argv under the client-side guard.
    ///
    /// The guard is the deadline plus the grace the in-session `timeout` needs to report back.
    /// When it fires the session itself did not end the command, so the session is ended — a
    /// force-remove, so the call returns one kill later — and the caller hears that it was.
    /// `deadlineExceeded` means the command has stopped, never that a stop was requested; the
    /// path is reached only by a session that could not run `timeout`.
    async fn exec_within(
        &self,
        session_id: &str,
        command: &[String],
        request: &RunCommandRequest,
    ) -> Result<ExecResponse> {
        match tokio::time::timeout(
            guard_for(request.deadline)?,
            self.send(
                self.client
                    .post(self.url(&format!("/v1/sessions/{session_id}/exec")))
                    .json(&json!({ "command": command })),
                "sandbox.runCommand",
            ),
        )
        .await
        {
            Ok(inner) => inner,
            Err(_) => {
                self.terminate(session_id).await?;
                Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "deadlineExceeded".to_string(),
                    reason: format!(
                        "the command exceeded its {}s deadline and the session could not end it, so the session was terminated",
                        request.deadline.as_secs()
                    ),
                }))
            }
        }
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
        // command to completion, so the deadline is enforced inside the session: the wrapper kills
        // the command at it, the session survives, and the call lands right after — the shape the
        // agent-supervised backends give. The client-side guard is the backstop for a route that
        // never answers at all; there the only lever left is ending the session, which the manager
        // does with a force-remove, one kill after the deadline.
        // Passed to `sh` as arguments, so nothing re-parses the command's text.
        let mut argv = vec![
            "sh".to_string(),
            "-c".to_string(),
            DeadlineReport::bounded_program(request.deadline),
            "sh".to_string(),
        ];
        argv.extend(request.command.iter().cloned());
        let response = self.exec_within(session_id, &argv, &request).await?;

        // Each stream is joined before it is read: the route returns a finished result rather
        // than a live stream, and the wrapper's announcement and its repeat can land in separate
        // frames, so reading a frame at a time would miss the pair.
        let mut stdout = Vec::new();
        let mut stderr = String::new();
        for frame in response.output {
            let (data, is_stdout) = match &frame {
                OutputFrame::Stdout(data) => (data, true),
                OutputFrame::Stderr(data) => (data, false),
            };
            let decoded = BASE64.decode(data).into_alien_error().context(
                ErrorData::UnexpectedResponseFormat {
                    provider: "local-sandbox".to_string(),
                    binding_name: "sandbox.runCommand".to_string(),
                    field: "output".to_string(),
                    response_json: "an output frame was not valid base64".to_string(),
                },
            )?;
            if is_stdout {
                stdout.extend_from_slice(&decoded);
            } else {
                stderr.push_str(&String::from_utf8_lossy(&decoded));
            }
        }
        let (deadline_exceeded, stderr) =
            match DeadlineReport::read(i32::try_from(response.exit_code).ok(), &stderr) {
                Bounded::Ran { killed, stderr } => (killed, stderr),
                Bounded::NotRun { reason } => {
                    return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "commandNotBounded".to_string(),
                        reason,
                    }))
                }
            };

        let mut frames: Vec<Result<CommandOutput>> = Vec::new();
        if !stdout.is_empty() {
            frames.push(Ok(CommandOutput::Stdout {
                seq: 0,
                data: stdout,
            }));
        }
        if !stderr.is_empty() {
            frames.push(Ok(CommandOutput::Stderr {
                seq: frames.len() as u64,
                data: stderr.into_bytes(),
            }));
        }

        if deadline_exceeded {
            // The output is kept and the terminal item says why it ends, as the agent-backed
            // providers do; the session is untouched.
            frames.push(Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "deadlineExceeded".to_string(),
                reason: format!(
                    "the command exceeded its {}s deadline and was killed; the session is still usable",
                    request.deadline.as_secs()
                ),
            })));
        } else {
            frames.push(Ok(CommandOutput::Exit {
                code: response.exit_code as i32,
                truncated: false,
            }));
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, State};
    use axum::routing::post;
    use axum::{Json, Router};
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    /// A route double: exec answers from a script, in order, and hangs once it runs out; delete
    /// is recorded. What the provider sent is kept so a test can read the argv it built.
    #[derive(Default)]
    struct Route {
        execs: Mutex<std::collections::VecDeque<serde_json::Value>>,
        commands: Mutex<Vec<Vec<String>>>,
        deleted: Mutex<Vec<String>>,
    }

    /// Stands in for the wrapper's kill in a scripted response.
    const DEADLINE_PLACEHOLDER: &str = "<deadline>";
    /// The nonce a session would draw. Announced on the first line of stderr, and repeated by
    /// the killer, exactly as the wrapper does.
    const SESSION_NONCE: &str = "a1b2c3d4";

    /// Wraps a scripted stderr the way a bounded session would return it.
    fn as_session_stderr(stderr: &str) -> String {
        match stderr {
            DEADLINE_PLACEHOLDER => format!("{SESSION_NONCE}\npartial-err{SESSION_NONCE}"),
            other => format!("{SESSION_NONCE}\n{other}"),
        }
    }

    fn exec_response(exit_code: i64, stdout: &str, stderr: &str) -> serde_json::Value {
        let mut output = Vec::new();
        if !stdout.is_empty() {
            output.push(json!({ "stream": "stdout", "dataBase64": BASE64.encode(stdout) }));
        }
        output.push(json!({
            "stream": "stderr",
            "dataBase64": BASE64.encode(as_session_stderr(stderr)),
        }));
        json!({ "output": output, "exitCode": exit_code })
    }

    async fn serve(route: Arc<Route>) -> LocalSandbox {
        async fn exec(
            State(route): State<Arc<Route>>,
            Path(_session): Path<String>,
            Json(body): Json<serde_json::Value>,
        ) -> Json<serde_json::Value> {
            let command: Vec<String> =
                serde_json::from_value(body["command"].clone()).expect("argv");
            route.commands.lock().expect("commands").push(command);
            let next = route.execs.lock().expect("execs").pop_front();
            match next {
                Some(response) => Json(response),
                None => std::future::pending().await,
            }
        }
        async fn delete(
            State(route): State<Arc<Route>>,
            Path(session): Path<String>,
        ) -> Json<serde_json::Value> {
            route.deleted.lock().expect("deleted").push(session);
            Json(json!({}))
        }

        let router = Router::new()
            .route("/v1/sessions/{session}/exec", post(exec))
            .route("/v1/sessions/{session}", axum::routing::delete(delete))
            .with_state(route);
        let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });

        LocalSandbox {
            client: reqwest::Client::new(),
            base_url: format!("http://{address}"),
            token: "test-token".to_string(),
        }
    }

    fn command(deadline_secs: u64) -> RunCommandRequest {
        RunCommandRequest {
            command: vec!["sleep".to_string(), "forever".to_string()],
            working_directory: None,
            env: BTreeMap::new(),
            deadline: std::time::Duration::from_secs(deadline_secs),
        }
    }

    /// The deadline is enforced inside the session: the argv is prefixed with `timeout`, and when
    /// it fires the output is kept, the stream ends in `deadlineExceeded`, and the session is not
    /// touched.
    ///
    /// The wrapper reports its own kill, so the double answers with that report rather than the
    /// test leaning on timing.
    #[tokio::test]
    async fn a_command_past_its_deadline_is_killed_in_place_and_the_session_survives() {
        let route = Arc::new(Route::default());
        route.execs.lock().expect("execs").push_back(exec_response(
            137,
            "partial\n",
            DEADLINE_PLACEHOLDER,
        ));
        let sandbox = serve(route.clone()).await;

        let frames: Vec<Result<CommandOutput>> = sandbox
            .run_command("s1", command(30))
            .await
            .expect("the deadline is reported in the stream")
            .collect()
            .await;

        assert!(
            matches!(&frames[0], Ok(CommandOutput::Stdout { data, .. }) if data == b"partial\n"),
            "{frames:?}"
        );
        let terminal = frames
            .last()
            .expect("frames")
            .as_ref()
            .expect_err("the stream must end in the deadline error, not an exit frame");
        assert!(
            terminal.to_string().contains("deadlineExceeded"),
            "{terminal}"
        );
        assert!(
            route.deleted.lock().expect("deleted").is_empty(),
            "the session survives an in-session kill"
        );
        let sent = route.commands.lock().expect("commands").clone();
        assert_eq!(sent.len(), 1, "one command: {sent:?}");
        assert_eq!(sent[0][0], "sh");
        assert_eq!(sent[0][1], "-c");
        assert!(sent[0][2].contains("sleep 30"), "{:?}", sent[0]);
        assert_eq!(
            &sent[0][3..],
            &["sh".to_string(), "sleep".to_string(), "forever".to_string()],
            "the command is passed as arguments, not pasted into the program"
        );
    }

    /// The route hands stderr back in whatever frames Docker produced, so the announcement and
    /// the repeat can arrive separately. Read a frame at a time, the pair would be missed and a
    /// killed command would come back as an ordinary exit with protocol bytes in its output.
    #[tokio::test]
    async fn a_deadline_split_across_stderr_frames_is_still_read() {
        let route = Arc::new(Route::default());
        route.execs.lock().expect("execs").push_back(json!({
            "output": [
                { "stream": "stderr", "dataBase64": BASE64.encode(format!("{SESSION_NONCE}\n")) },
                { "stream": "stderr", "dataBase64": BASE64.encode("partial-err") },
                { "stream": "stderr", "dataBase64": BASE64.encode(SESSION_NONCE) },
            ],
            "exitCode": 137,
        }));
        let sandbox = serve(route.clone()).await;

        let frames: Vec<Result<CommandOutput>> = sandbox
            .run_command("s1", command(30))
            .await
            .expect("the deadline is reported in the stream")
            .collect()
            .await;

        assert!(
            matches!(&frames[0], Ok(CommandOutput::Stderr { data, .. }) if data == b"partial-err"),
            "the caller keeps its output, without the protocol bytes: {frames:?}"
        );
        let terminal = frames
            .last()
            .expect("frames")
            .as_ref()
            .expect_err("a killed command ends in the deadline error");
        assert!(
            terminal.to_string().contains("deadlineExceeded"),
            "{terminal}"
        );
    }

    /// 124 is an ordinary exit status. Without the wrapper's report the command exited on its
    /// own, and saying otherwise would tell the caller its command was killed.
    #[tokio::test]
    async fn a_command_exiting_124_of_its_own_accord_is_an_exit_not_a_deadline() {
        let route = Arc::new(Route::default());
        route
            .execs
            .lock()
            .expect("execs")
            .push_back(exec_response(124, "done\n", ""));
        let sandbox = serve(route.clone()).await;

        let frames: Vec<Result<CommandOutput>> = sandbox
            .run_command("s1", command(300))
            .await
            .expect("runs")
            .collect()
            .await;

        assert!(matches!(
            frames.last().expect("frames"),
            Ok(CommandOutput::Exit { code: 124, .. })
        ));
    }

    /// When the session cannot end the command — the route never answers — the guard ends the
    /// session and reports the deadline. Time is paused, so the guard fires instantly.
    #[tokio::test(start_paused = true)]
    async fn a_command_the_session_cannot_end_takes_the_session_with_it() {
        let route = Arc::new(Route::default());
        let sandbox = serve(route.clone()).await;

        let error = sandbox
            .run_command("s1", command(30))
            .await
            .err()
            .expect("a command that outran its deadline has not succeeded");

        assert!(error.to_string().contains("deadlineExceeded"), "{error}");
        assert_eq!(
            route.deleted.lock().expect("deleted").clone(),
            vec!["s1".to_string()],
            "the session must actually be removed, not merely reported as terminated"
        );
    }
}
