//! Interactive commands over the agent protocol, written once for every backend that speaks it.
//!
//! A backend supplies a [`BoundAgent`]: one sandbox's agent, reachable for as long as the command
//! runs. Binding once matters because input is many small requests. A backend that authorizes
//! each request with a cloud call would otherwise pay that call per keystroke.
//!
//! The wire is the agent's: `POST /v1/interactive` streams NDJSON frames, decoded exactly as
//! `run_command`'s are, and input, close and signal are short requests naming the command by an
//! id this client chooses. Input carries its offset in the command's stdin, so a write whose
//! answer was lost is sent again and applied once.

use std::sync::Arc;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;

use super::agent_protocol::{self, RUN_INTERACTIVE};
use crate::error::{ErrorData, Result};
use crate::traits::{CommandInput, CommandSignal, InteractiveCommand, RunCommandRequest};
use alien_error::{AlienError, Context, IntoAlienError};

/// The largest piece of input sent in one request. Base64 makes it a third larger on the wire,
/// which keeps every request well inside each backend's body limit.
const MAX_WRITE_BYTES: usize = 256 * 1024;

/// How many times one piece of input is sent before its failure is reported. Only a request that
/// went unanswered is sent again; the offset makes that safe.
const WRITE_ATTEMPTS: usize = 4;

const WRITE_STDIN: &str = "sandbox.interactive.write";
const CLOSE_STDIN: &str = "sandbox.interactive.closeStdin";
const SIGNAL: &str = "sandbox.interactive.signal";

/// One sandbox's agent, authorized for every request an interactive command makes.
#[async_trait]
pub trait BoundAgent: Send + Sync + std::fmt::Debug {
    /// Builds a request to `path` on the agent, carrying whatever authorizes it.
    async fn request(&self, method: reqwest::Method, path: &str)
        -> Result<reqwest::RequestBuilder>;

    /// Name used in errors, so a failure says which backend refused.
    fn provider(&self) -> &'static str;
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StdinResponse {
    accepted: u64,
}

/// Starts an interactive command on `agent`.
pub async fn run_interactive(
    agent: Arc<dyn BoundAgent>,
    request: RunCommandRequest,
) -> Result<InteractiveCommand> {
    let id = uuid::Uuid::new_v4().to_string();
    let mut body = agent_protocol::exec_body(&request)?;
    body["id"] = json!(id);

    let response = agent_protocol::send(
        agent
            .request(reqwest::Method::POST, "/v1/interactive")
            .await?
            .json(&body),
        RUN_INTERACTIVE,
    )
    .await?;

    let output = agent_protocol::frame_stream(response, agent.provider(), request.timeout);
    Ok(InteractiveCommand {
        input: Arc::new(AgentInput {
            agent,
            id,
            state: tokio::sync::Mutex::new(InputState {
                offset: 0,
                interrupted: false,
            }),
        }),
        output,
    })
}

/// The input half, holding how much of the command's stdin the agent has taken.
#[derive(Debug)]
struct AgentInput {
    agent: Arc<dyn BoundAgent>,
    id: String,
    /// Locked for a whole write, so concurrent writers never interleave and the offset each sends
    /// is the one the agent expects.
    state: tokio::sync::Mutex<InputState>,
}

#[derive(Debug)]
struct InputState {
    /// Bytes of input the agent has acknowledged
    offset: u64,
    /// Set while a write is in flight and left set if it ends without its last acknowledgement,
    /// because it was cancelled or failed. The agent may then have taken bytes this client never
    /// heard about, and the next write at the old offset would be deduplicated against them.
    interrupted: bool,
}

/// What the agent's refusal names, read from the code its body starts with. A status alone is
/// not enough: a proxy answers 404 for a sandbox that is gone, and that is not a finished command.
fn named_failure(body: &str) -> Option<&'static str> {
    [
        ("COMMAND_NOT_RUNNING", "commandNotRunning"),
        ("STDIN_CLOSED", "stdinClosed"),
        ("STDIN_OFFSET_MISMATCH", "stdinOffsetMismatch"),
        ("INTERACTIVE_LIMIT_REACHED", "interactiveLimitReached"),
    ]
    .into_iter()
    .find(|(code, _)| body.starts_with(code))
    .map(|(_, failure)| failure)
}

impl AgentInput {
    /// Sends one control request and maps the agent's answers that name the command's state.
    async fn control(
        &self,
        path: &str,
        body: serde_json::Value,
        operation: &str,
    ) -> Result<reqwest::Response> {
        let response = agent_protocol::send_raw(
            self.agent
                .request(reqwest::Method::POST, path)
                .await?
                .json(&body),
            operation,
        )
        .await?;

        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let body = agent_protocol::error_body(response, operation).await?;
        if let Some(failure) = named_failure(&body) {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: failure.to_string(),
                reason: format!("{operation}: {body}"),
            }));
        }
        // Anything else is classified as every other agent request is: a 5xx never proves the
        // agent answered, and a 4xx it did not name is a refusal.
        if status.is_server_error() {
            return Err(AlienError::new(agent_protocol::unanswered(
                operation,
                &format!("the sandbox host returned {status}: {body}"),
            )));
        }
        Err(AlienError::new(ErrorData::SandboxCommandFailed {
            failure: "agentRefused".to_string(),
            reason: format!("{operation} returned {status}: {body}"),
        }))
    }

    /// Sends one piece of input at `offset` and returns the agent's total, retrying only a request
    /// that went unanswered.
    async fn send_piece(&self, offset: u64, piece: &[u8]) -> Result<u64> {
        let body = json!({ "id": self.id, "offset": offset, "data": BASE64.encode(piece) });
        let mut attempt = 0;
        let response = loop {
            attempt += 1;
            match self
                .control("/v1/interactive/stdin", body.clone(), WRITE_STDIN)
                .await
            {
                Ok(response) => break response,
                // Unanswered, so the agent may or may not have taken the bytes. The offset makes
                // sending them again apply them once either way.
                Err(error) if error.retryable && attempt < WRITE_ATTEMPTS => continue,
                Err(error) => return Err(error),
            }
        };
        let taken: StdinResponse = response.json().await.into_alien_error().context(
            ErrorData::UnexpectedResponseFormat {
                provider: self.agent.provider().to_string(),
                binding_name: WRITE_STDIN.to_string(),
                field: "accepted".to_string(),
                response_json: "the agent returned a body this provider cannot parse".to_string(),
            },
        )?;
        Ok(taken.accepted)
    }
}

#[async_trait]
impl CommandInput for AgentInput {
    async fn write(&self, data: &[u8]) -> Result<()> {
        let mut state = self.state.lock().await;

        // An earlier write ended without its last acknowledgement. Ask where the input stands
        // before sending anything: an empty write at the known offset answers with the agent's
        // total and applies nothing.
        if state.interrupted {
            let taken = self.send_piece(state.offset, &[]).await?;
            state.interrupted = false;
            if taken != state.offset {
                let lost = taken.saturating_sub(state.offset);
                state.offset = taken.max(state.offset);
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "stdinInterrupted".to_string(),
                    reason: format!(
                        "an earlier write was cancelled or failed after the program took {lost} of \
                         its bytes; this write was not sent, and the next one continues after them"
                    ),
                }));
            }
        }

        let start = state.offset;
        let end = start + data.len() as u64;
        state.interrupted = true;
        while state.offset < end {
            let from = usize::try_from(state.offset - start).expect("within data");
            let piece = &data[from..(from + MAX_WRITE_BYTES).min(data.len())];
            let taken = self.send_piece(state.offset, piece).await?;
            // Never backwards, and never past what was sent: either would mean this client and
            // the agent disagree about the stream, and continuing would corrupt it silently.
            if taken < state.offset || taken > end {
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "stdinOffsetMismatch".to_string(),
                    reason: format!(
                        "the agent reports {taken} bytes of input taken after a write of bytes \
                         {}..{end}",
                        state.offset
                    ),
                }));
            }
            // Short of the piece means the command stopped reading for a while: send the rest.
            state.offset = taken;
        }
        state.interrupted = false;

        Ok(())
    }

    async fn close_stdin(&self) -> Result<()> {
        // Behind the write lock, so a close never overtakes bytes a concurrent writer is sending.
        let _state = self.state.lock().await;
        match self
            .control(
                "/v1/interactive/close-stdin",
                json!({ "id": self.id }),
                CLOSE_STDIN,
            )
            .await
        {
            Ok(_) => Ok(()),
            // A command that has ended reads nothing, so what the close asked for is already
            // true. A program that exits on end of input makes this the common second close.
            Err(error)
                if matches!(
                    &error.error,
                    Some(ErrorData::SandboxCommandFailed { failure, .. })
                        if failure == "commandNotRunning"
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn signal(&self, signal: CommandSignal) -> Result<()> {
        // Sent once, never retried: a second interrupt is not the same request as the first.
        self.control(
            "/v1/interactive/signal",
            json!({ "id": self.id, "signal": signal }),
            SIGNAL,
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::CommandOutput;
    use alien_core::sandbox_capability::{
        SandboxCapabilityClaims, SandboxOperationClass, SandboxSessionIdentity,
    };
    use alien_sandbox_agent::exec::ExecIdentity;
    use alien_sandbox_agent::server::{router, AgentAuthorization, AgentState};
    use ed25519_compact::{KeyPair, Seed};
    use futures::StreamExt;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    const SESSION: &str = "interactive-session";

    /// The real agent on loopback, reached with a capability, so the client is proven against the
    /// server it ships with rather than against a description of it.
    #[derive(Debug)]
    struct LoopbackAgent {
        url: String,
        token: String,
        /// Stdin requests to send to a port nobody listens on before the real one, so a lost
        /// write is exercised for real.
        unanswered_writes: AtomicUsize,
        _root: tempfile::TempDir,
    }

    #[async_trait]
    impl BoundAgent for LoopbackAgent {
        async fn request(
            &self,
            method: reqwest::Method,
            path: &str,
        ) -> Result<reqwest::RequestBuilder> {
            let base = if path == "/v1/interactive/stdin"
                && self
                    .unanswered_writes
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        left.checked_sub(1)
                    })
                    .is_ok()
            {
                "http://127.0.0.1:9".to_string()
            } else {
                self.url.clone()
            };
            Ok(reqwest::Client::new()
                .request(method, format!("{base}{path}"))
                .bearer_auth(&self.token))
        }

        fn provider(&self) -> &'static str {
            "loopback-agent"
        }
    }

    async fn agent(unanswered_writes: usize) -> Arc<LoopbackAgent> {
        let root = tempfile::TempDir::new().unwrap();
        let keys = KeyPair::from_seed(Seed::new([7u8; 32]));
        let state = Arc::new(AgentState {
            image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
            #[cfg(target_os = "linux")]
            gateway_egress: None,
            session_root: root.path().canonicalize().unwrap(),
            authorization: AgentAuthorization::Capability {
                public_key: keys.pk,
                identity: SandboxSessionIdentity {
                    session_id: SESSION.to_string(),
                    generation: 1,
                },
            },
            // The test's own identity, read from a directory it just made: dropping to it is
            // permitted unprivileged, so the real spawn path runs.
            exec_identity: {
                use std::os::unix::fs::MetadataExt;
                let metadata = std::fs::metadata(root.path()).unwrap();
                ExecIdentity {
                    uid: metadata.uid(),
                    gid: metadata.gid(),
                }
            },
            output_cap: 1 << 24,
            jobs: Default::default(),
            interactive: Default::default(),
            agent_binary: std::path::PathBuf::from("/bin/false"),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(
                listener,
                router(state).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let token = alien_core::sandbox_capability_token::mint(
            &SandboxCapabilityClaims {
                session_id: SESSION.to_string(),
                operation: SandboxOperationClass::Execute,
                generation: 1,
                expires_at: chrono::Utc::now().timestamp() + 300,
                key_id: "k1".to_string(),
            },
            &keys.sk,
        )
        .unwrap();
        Arc::new(LoopbackAgent {
            url,
            token,
            unanswered_writes: AtomicUsize::new(unanswered_writes),
            _root: root,
        })
    }

    fn command(program: &str, args: &[&str]) -> RunCommandRequest {
        RunCommandRequest {
            command: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            cwd: None,
            env: Default::default(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Reads stdout until it ends with `expected`.
    async fn read_until(command: &mut InteractiveCommand, stdout: &mut Vec<u8>, expected: &str) {
        while !String::from_utf8_lossy(stdout).ends_with(expected) {
            match tokio::time::timeout(Duration::from_secs(10), command.output.next())
                .await
                .expect("output arrives")
                .expect("the stream is still open")
                .expect("a frame")
            {
                CommandOutput::Stdout { data, .. } => stdout.extend(data),
                CommandOutput::Stderr { .. } => {}
                other => panic!("ended before {expected:?}: {other:?}"),
            }
        }
    }

    async fn finish(mut command: InteractiveCommand, stdout: &mut Vec<u8>) -> CommandOutput {
        let mut terminal = None;
        while let Some(item) = tokio::time::timeout(Duration::from_secs(10), command.output.next())
            .await
            .expect("the stream ends")
        {
            match item.expect("a frame") {
                CommandOutput::Stdout { data, .. } => stdout.extend(data),
                CommandOutput::Stderr { .. } => {}
                exit => {
                    assert!(terminal.is_none(), "exactly one terminal frame");
                    terminal = Some(exit);
                }
            }
        }
        terminal.expect("a terminal frame")
    }

    #[tokio::test]
    async fn a_program_is_driven_turn_by_turn_through_the_binding() {
        let agent = agent(0).await;
        let mut command = run_interactive(
            agent,
            command(
                "/bin/sh",
                &[
                    "-c",
                    "printf '> '; while read l; do echo \"=$l\"; printf '> '; done",
                ],
            ),
        )
        .await
        .expect("starts");
        let mut stdout = Vec::new();

        read_until(&mut command, &mut stdout, "> ").await;
        command.input.write(b"1+1\n").await.expect("writes");
        read_until(&mut command, &mut stdout, "=1+1\n> ").await;
        command.input.close_stdin().await.expect("closes");
        command
            .input
            .close_stdin()
            .await
            .expect("closing twice is fine");

        assert_eq!(
            finish(command, &mut stdout).await,
            CommandOutput::Exit {
                code: 0,
                truncated: false
            }
        );
        assert_eq!(String::from_utf8_lossy(&stdout), "> =1+1\n> ");
    }

    /// Input far larger than one request carries is split and arrives whole, and a write whose
    /// request never reached the agent is sent again rather than failing the caller or leaving a
    /// gap. Backpressure itself is the agent's, and its own tests hold a writer against a command
    /// that is not reading.
    #[tokio::test]
    async fn large_input_arrives_whole_across_a_lost_request() {
        let agent = agent(1).await;
        let command = run_interactive(agent, command("/usr/bin/wc", &["-c"]))
            .await
            .expect("starts");

        command
            .input
            .write(&vec![b'x'; 3 * MAX_WRITE_BYTES + 17])
            .await
            .expect("every piece is taken");
        command.input.close_stdin().await.expect("closes");

        let mut stdout = Vec::new();
        assert!(matches!(
            finish(command, &mut stdout).await,
            CommandOutput::Exit { code: 0, .. }
        ));
        assert_eq!(
            String::from_utf8_lossy(&stdout).trim(),
            (3 * MAX_WRITE_BYTES + 17).to_string()
        );
    }

    #[tokio::test]
    async fn a_signal_after_the_command_ended_says_it_is_not_running() {
        let agent = agent(0).await;
        let command = run_interactive(agent, command("/bin/sh", &["-c", "exit 3"]))
            .await
            .expect("starts");
        let input = Arc::clone(&command.input);
        let mut stdout = Vec::new();
        assert!(matches!(
            finish(command, &mut stdout).await,
            CommandOutput::Exit { code: 3, .. }
        ));

        let error = input
            .signal(CommandSignal::Interrupt)
            .await
            .expect_err("nothing is running");
        assert!(
            matches!(&error.error, Some(ErrorData::SandboxCommandFailed { failure, .. }) if failure == "commandNotRunning"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn an_interrupt_reaches_a_running_program() {
        let agent = agent(0).await;
        let mut command = run_interactive(
            agent,
            command(
                "/bin/sh",
                &[
                    "-c",
                    "trap 'echo stopped; exit 130' INT; echo up; while :; do sleep 0.05; done",
                ],
            ),
        )
        .await
        .expect("starts");
        let mut stdout = Vec::new();
        read_until(&mut command, &mut stdout, "up\n").await;
        command
            .input
            .signal(CommandSignal::Interrupt)
            .await
            .expect("signals");
        assert!(matches!(
            finish(command, &mut stdout).await,
            CommandOutput::Exit { code: 130, .. }
        ));
        assert!(String::from_utf8_lossy(&stdout).ends_with("stopped\n"));
    }

    /// A write the caller cancels part-way may already have reached the program. The next write
    /// must say so rather than be deduplicated against those bytes, which would silently drop its
    /// own start while reporting success; the write after continues the stream exactly.
    #[tokio::test]
    async fn a_write_after_a_cancelled_one_reports_the_interruption_instead_of_losing_bytes() {
        let agent = agent(0).await;
        // Counts only the second write's bytes, so a lost start shows as a short count.
        let command = run_interactive(
            agent,
            command("/bin/sh", &["-c", "sleep 0.5; tr -cd y | wc -c"]),
        )
        .await
        .expect("starts");

        // Cancelled while the program is not reading: the pipe has taken some of it.
        let cancelled = tokio::time::timeout(
            Duration::from_millis(100),
            command.input.write(&vec![b'x'; 2 * MAX_WRITE_BYTES]),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "the write was still waiting on the reader"
        );

        // Longer than anything the cancelled write can have delivered, which is the case a
        // deduplicating agent would accept in silence.
        let second = vec![b'y'; 3 * MAX_WRITE_BYTES];
        let error = command
            .input
            .write(&second)
            .await
            .expect_err("the interruption is reported, not hidden");
        assert!(
            matches!(&error.error, Some(ErrorData::SandboxCommandFailed { failure, .. }) if failure == "stdinInterrupted"),
            "{error:?}"
        );

        command
            .input
            .write(&second)
            .await
            .expect("the stream continues");
        command.input.close_stdin().await.expect("closes");
        let mut stdout = Vec::new();
        assert!(matches!(
            finish(command, &mut stdout).await,
            CommandOutput::Exit { code: 0, .. }
        ));
        assert_eq!(
            String::from_utf8_lossy(&stdout).trim(),
            second.len().to_string(),
            "the write after the report arrives whole"
        );
    }
}
