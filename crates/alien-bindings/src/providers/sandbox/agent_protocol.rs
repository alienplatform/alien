//! Speaking the sandbox agent protocol, once, for every backend that ships an agent.
//!
//! AWS and Kubernetes both talk to the same agent over HTTP and differ in exactly one thing:
//! how a request is authorized. AWS mints an endpoint token scoped to one MicroVM and an
//! explicit port set; Kubernetes claims a pod and presents a capability scoped to that session. So
//! the transport is the trait and the protocol is written once over it.
//!
//! The decoding is the reason this is shared rather than copied. A body that ends without a
//! terminal frame is a **transport failure**, not a command that finished, and a stream that
//! quietly stopped would report a truncated response as a successful command.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;

use crate::error::{ErrorData, Result};
use crate::traits::{CommandOutput, RunCommandRequest};
use alien_error::{AlienError, Context, IntoAlienError};

pub use alien_core::sandbox_process::AGENT_PORT;

/// Named once: `send` treats it as the one operation a 5xx must not be retried for.
const RUN_COMMAND: &str = "sandbox.runCommand";

/// How long a request to the agent may take to answer with its headers.
///
/// Bounds reaching the agent, not what it then streams: a command's own deadline governs its
/// output. Wrapped around `send()` alone rather than set as the request's timeout, which reqwest
/// runs until the whole body has arrived and so would cut off any command outliving it. A caller
/// cancelling a command waits for this request to settle before it can close the stream, so an
/// unbounded one would leave that cancel unable to complete.
#[cfg(not(test))]
const AGENT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// Short in tests so a stalled agent is exercised in milliseconds rather than waited out.
#[cfg(test)]
const AGENT_RESPONSE_TIMEOUT: Duration = Duration::from_millis(200);

/// How a backend turns a session id into an authorized request.
///
/// The only thing AWS and Kubernetes disagree on.
#[async_trait]
pub trait AgentTransport: Send + Sync + std::fmt::Debug {
    /// Builds a request to `path` on the session's agent, carrying whatever authorizes it.
    async fn request(
        &self,
        session_id: &str,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder>;

    /// Name used in errors, so a failure says which backend refused.
    fn provider(&self) -> &'static str;
}

/// A frame as the agent writes it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "t")]
enum AgentFrame {
    Stdout {
        seq: u64,
        data: String,
    },
    Stderr {
        seq: u64,
        data: String,
    },
    Exit {
        code: i32,
        #[serde(default)]
        truncated: bool,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadFileResponse {
    contents_base64: String,
}

/// Runs a command, streaming frames as the agent produces them.
pub async fn run_command<T: AgentTransport + ?Sized>(
    transport: &T,
    session_id: &str,
    request: RunCommandRequest,
) -> Result<BoxStream<'static, Result<CommandOutput>>> {
    // Checked after conversion, not on the Duration: a sub-millisecond deadline is non-zero here
    // and floors to `deadlineMs: 0`, which the agent then refuses as invalid.
    if deadline_millis(request.deadline) == 0 {
        return Err(AlienError::new(ErrorData::SandboxCommandFailed {
            failure: "invalidRequest".to_string(),
            reason: "a command must carry a non-zero deadline".to_string(),
        }));
    }

    let body = json!({
        "command": request.command,
        "deadlineMs": deadline_millis(request.deadline),
        "workingDirectory": request.working_directory,
        "env": request.env,
    });

    let response = send(
        transport
            .request(session_id, reqwest::Method::POST, "/v1/exec")
            .await?
            .json(&body),
        RUN_COMMAND,
    )
    .await?;

    Ok(frame_stream(response, transport.provider()))
}

/// Reads a file out of the sandbox.
pub async fn read_file<T: AgentTransport + ?Sized>(
    transport: &T,
    session_id: &str,
    path: &str,
) -> Result<Vec<u8>> {
    let response = send(
        transport
            .request(session_id, reqwest::Method::GET, "/v1/files")
            .await?
            .query(&[("path", path)]),
        "sandbox.readFile",
    )
    .await?;

    let body: ReadFileResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: transport.provider().to_string(),
                binding_name: "sandbox.readFile".to_string(),
                field: "body".to_string(),
                response_json: "the agent returned a body this provider cannot parse".to_string(),
            })?;

    decode(
        &body.contents_base64,
        transport.provider(),
        "sandbox.readFile",
        "contentsBase64",
    )
}

/// Writes files into the sandbox, one request per path.
pub async fn write_files<T: AgentTransport + ?Sized>(
    transport: &T,
    session_id: &str,
    files: BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    for (path, contents) in files {
        send(
            transport
                .request(session_id, reqwest::Method::PUT, "/v1/files")
                .await?
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

/// Creates a directory inside the sandbox.
pub async fn mkdir<T: AgentTransport + ?Sized>(
    transport: &T,
    session_id: &str,
    path: &str,
) -> Result<()> {
    send(
        transport
            .request(session_id, reqwest::Method::POST, "/v1/mkdir")
            .await?
            .json(&json!({ "path": path })),
        "sandbox.mkdir",
    )
    .await?;

    Ok(())
}

/// Milliseconds, saturated rather than wrapped.
///
/// A deadline long enough to overflow `u64` milliseconds is not a deadline anyone meant, and
/// wrapping it would turn "effectively forever" into "immediately".
fn deadline_millis(deadline: Duration) -> u64 {
    u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX)
}

/// What a request whose outcome is unknown becomes: no answer before its headers, or a body
/// that failed or ended short of its last frame.
///
/// For a file operation every one of those is safe to repeat. For `run_command` the agent may
/// have started the command — and past the headers it certainly did — so a repeat could run it
/// twice, and the refusal must not carry the retry signal.
fn unanswered(operation: &str, reason: &str) -> ErrorData {
    if operation == RUN_COMMAND {
        return ErrorData::SandboxCommandFailed {
            failure: "outcomeUnknown".to_string(),
            reason: format!("{reason}; the command may have started, its outcome is unknown"),
        };
    }
    ErrorData::SandboxUnreachable {
        operation: operation.to_string(),
        reason: reason.to_string(),
    }
}

/// Sends a request and turns a non-success into a typed error carrying the agent's own reason.
///
/// A transport failure here is marked retryable, which holds for the file operations but not for
/// `run_command` — that request may have already started the command. Nothing retries on this
/// path today; whoever adds a retry layer has to treat `run_command` as the exception.
pub async fn send(request: reqwest::RequestBuilder, operation: &str) -> Result<reqwest::Response> {
    let response = match tokio::time::timeout(AGENT_RESPONSE_TIMEOUT, request.send()).await {
        Ok(sent) => sent
            .into_alien_error()
            .context(unanswered(operation, "the request never reached the agent"))?,
        Err(_) => {
            return Err(AlienError::new(unanswered(
                operation,
                &format!(
                    "the agent did not answer within {}s",
                    AGENT_RESPONSE_TIMEOUT.as_secs()
                ),
            )));
        }
    };

    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    // Read the body first: the agent puts the actual cause there, and a bare status turns a
    // specific refusal into a guess. A read that *fails* is not an empty body — collapsing the
    // two would let a dropped connection claim the request never arrived.
    let body = match response.text().await {
        Ok(body) => body,
        // Unknown, and said so: for `run_command` a repeat could run it twice, so it is marked
        // non-retryable; the file operations are idempotent and keep the retryable classification
        // they had before the read failed.
        Err(error) if operation == RUN_COMMAND => {
            return Err(error)
                .into_alien_error()
                .context(ErrorData::SandboxCommandFailed {
                    failure: "outcomeUnknown".to_string(),
                    reason: format!(
                        "{operation} returned {status} and its body could not be read, so whether \
                         it ran is unknown"
                    ),
                })
        }
        Err(error) => {
            return Err(error)
                .into_alien_error()
                .context(ErrorData::SandboxUnreachable {
                    operation: operation.to_string(),
                    reason: format!("{operation} returned {status} and its body could not be read"),
                })
        }
    };

    // A 5xx with no body is the cloud's proxy, not the agent — it answers 502/503/504 that way
    // when it cannot reach the guest, but can also synthesize one after delivering the request,
    // so `run_command` reports an unknown outcome rather than never-delivered.
    if status.is_server_error() && body.trim().is_empty() {
        if operation == RUN_COMMAND {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "outcomeUnknown".to_string(),
                reason: format!(
                    "the sandbox host returned {status} with no response from the agent, so \
                     whether the command ran is unknown"
                ),
            }));
        }
        return Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: operation.to_string(),
            reason: format!(
                "the sandbox host returned {status} before the request reached the agent"
            ),
        }));
    }

    // A 5xx is the agent failing to complete a request it accepted, which is worth another
    // attempt — except for `run_command`, where the command may already be running and a retry
    // would run it twice. A 4xx is a refusal: repeating it repeats the refusal.
    if status.is_server_error() && operation != RUN_COMMAND {
        return Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: operation.to_string(),
            reason: format!("the agent returned {status}: {body}"),
        }));
    }

    Err(AlienError::new(ErrorData::SandboxCommandFailed {
        // The cause, not the operation: `reason` already names the operation, and a caller
        // branching on `failure` gets an agent error code from every other construction site.
        failure: "agentRefused".to_string(),
        reason: format!("{operation} returned {status}: {body}"),
    }))
}

/// Turns the agent's NDJSON body into output frames.
fn frame_stream(
    response: reqwest::Response,
    provider: &'static str,
) -> BoxStream<'static, Result<CommandOutput>> {
    struct State {
        bytes: BoxStream<'static, reqwest::Result<bytes::Bytes>>,
        buffer: Vec<u8>,
        finished: bool,
        saw_terminal: bool,
        provider: &'static str,
    }

    let state = State {
        bytes: response.bytes_stream().boxed(),
        buffer: Vec::new(),
        finished: false,
        saw_terminal: false,
        provider,
    };

    futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(index) = state.buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = state.buffer.drain(..=index).collect();
                let line = &line[..line.len() - 1];
                if line.is_empty() {
                    continue;
                }

                let frame = match serde_json::from_slice::<AgentFrame>(line) {
                    Ok(frame) => frame,
                    Err(error) => {
                        state.finished = true;
                        let failure = malformed(&error.to_string(), state.provider);
                        return Some((Err(failure), state));
                    }
                };

                if matches!(frame, AgentFrame::Exit { .. } | AgentFrame::Error { .. }) {
                    state.saw_terminal = true;
                }

                let output = frame.into_output(state.provider);
                return Some((output, state));
            }

            if state.finished {
                return None;
            }

            match state.bytes.next().await {
                Some(Ok(chunk)) => state.buffer.extend_from_slice(&chunk),
                // The command has started — frames were arriving — and its end is now unknown.
                // That is the strongest case for not inviting a retry, so both a failing body and
                // one that ends short of its terminal frame refuse the same way a lost answer does.
                Some(Err(error)) => {
                    state.finished = true;
                    return Some((
                        Err(AlienError::new(unanswered(
                            RUN_COMMAND,
                            &format!("the output stream failed: {error}"),
                        ))),
                        state,
                    ));
                }
                None => {
                    state.finished = true;
                    if !state.saw_terminal {
                        return Some((
                            Err(AlienError::new(unanswered(
                                RUN_COMMAND,
                                "the output stream ended without a terminal frame",
                            ))),
                            state,
                        ));
                    }
                    return None;
                }
            }
        }
    })
    .boxed()
}

impl AgentFrame {
    fn into_output(self, provider: &'static str) -> Result<CommandOutput> {
        match self {
            Self::Stdout { seq, data } => Ok(CommandOutput::Stdout {
                seq,
                data: decode(&data, provider, RUN_COMMAND, "data")?,
            }),
            Self::Stderr { seq, data } => Ok(CommandOutput::Stderr {
                seq,
                data: decode(&data, provider, RUN_COMMAND, "data")?,
            }),
            Self::Exit { code, truncated } => Ok(CommandOutput::Exit { code, truncated }),
            // An error frame is the command's outcome, so it surfaces as an error rather than
            // as a stream that simply stopped.
            Self::Error { code, message } => {
                Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: code,
                    reason: message,
                }))
            }
        }
    }
}

/// `binding_name` and `field` are the caller's, not this function's: `read_file` decodes through
/// here too, and a corrupt file read reported as a runCommand output frame sends the reader to
/// the wrong place.
fn decode(data: &str, provider: &'static str, binding_name: &str, field: &str) -> Result<Vec<u8>> {
    BASE64
        .decode(data)
        .into_alien_error()
        .context(ErrorData::UnexpectedResponseFormat {
            provider: provider.to_string(),
            binding_name: binding_name.to_string(),
            field: field.to_string(),
            response_json: format!("{field} was not valid base64"),
        })
}

fn malformed(reason: &str, provider: &'static str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::UnexpectedResponseFormat {
        provider: provider.to_string(),
        binding_name: RUN_COMMAND.to_string(),
        field: "frame".to_string(),
        response_json: format!("an output frame did not parse: {reason}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::CommandOutput;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::Router;
    use std::net::SocketAddr;

    async fn serve_frames(chunks: Vec<&'static str>) -> String {
        let handler = move || {
            let chunks = chunks.clone();
            async move {
                let stream = futures::stream::iter(
                    chunks
                        .into_iter()
                        .map(|chunk| Ok::<_, std::io::Error>(bytes::Bytes::from(chunk))),
                );
                axum::body::Body::from_stream(stream).into_response()
            }
        };

        let router = Router::new().route("/v1/exec", post(handler));
        let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });

        format!("http://{address}")
    }

    async fn frames_from(chunks: Vec<&'static str>) -> Vec<Result<CommandOutput>> {
        let base = serve_frames(chunks).await;
        let response = reqwest::Client::new()
            .post(format!("{base}/v1/exec"))
            .send()
            .await
            .expect("responds");

        frame_stream(response, "test-sandbox")
            .collect::<Vec<_>>()
            .await
    }

    /// Serves `status` with `body` on /v1/exec and returns what `send` made of it.
    async fn send_status(status: StatusCode, body: &'static str) -> AlienError<ErrorData> {
        let handler = move || async move { (status, body).into_response() };
        let router = Router::new().route("/v1/exec", post(handler));
        let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });

        send(
            reqwest::Client::new().post(format!("http://{address}/v1/exec")),
            RUN_COMMAND,
        )
        .await
        .expect_err("a non-success must be an error")
    }

    /// AWS answers 502 with an empty body while a MicroVM's snapshot is still restoring, so the
    /// request never reached the agent and the command never ran. Calling that `agentRefused`
    /// sends a reader into the agent for a fault that was never there.
    #[tokio::test]
    async fn a_bodyless_server_error_is_not_reported_as_the_agent_refusing() {
        let error = send_status(StatusCode::BAD_GATEWAY, "").await;
        let rendered = error.to_string();

        assert!(
            !rendered.contains("agentRefused"),
            "a proxy 502 is not the agent refusing: {rendered}"
        );
        assert!(
            rendered.contains("outcomeUnknown"),
            "a proxy can synthesize a 502 after the agent accepted the request, so the caller has \
             to be told the outcome is unknown rather than that it is safe to repeat: {rendered}"
        );
    }

    /// The counterpart: the agent puts its cause in the body, and that is a real refusal which
    /// must stay distinguishable from the transport failing to deliver the request.
    #[tokio::test]
    async fn a_server_error_carrying_the_agents_own_cause_stays_a_refusal() {
        let error = send_status(StatusCode::INTERNAL_SERVER_ERROR, "spawn failed: ENOMEM").await;
        let rendered = error.to_string();

        assert!(
            rendered.contains("spawn failed: ENOMEM"),
            "the agent's cause has to survive: {rendered}"
        );
        assert!(
            !rendered.contains("outcomeUnknown"),
            "the agent answered with its own cause, so the outcome is not unknown: {rendered}"
        );
    }

    /// A cancel waits for this request before it can close the stream, so a request that never
    /// answers would leave the cancel unable to complete. Headers that never come are refused.
    #[tokio::test]
    async fn an_agent_that_never_answers_is_refused_within_the_bound() {
        let handler = || async {
            tokio::time::sleep(AGENT_RESPONSE_TIMEOUT * 20).await;
            "late".into_response()
        };
        let router = Router::new().route("/v1/exec", post(handler));
        let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });

        let started = std::time::Instant::now();
        let error = send(
            reqwest::Client::new().post(format!("http://{address}/v1/exec")),
            RUN_COMMAND,
        )
        .await
        .expect_err("a stalled agent must be refused, not waited on");

        // The agent would answer at 20x the bound; refusing well before that is the property.
        // The margin is wide because setup shares a runtime with the rest of the suite.
        assert!(
            started.elapsed() < AGENT_RESPONSE_TIMEOUT * 10,
            "refused at the bound, not at the agent's leisure: {:?}",
            started.elapsed()
        );
        assert!(
            error.to_string().contains("did not answer"),
            "the refusal says the agent stalled: {error}"
        );
    }

    /// The bound is on reaching the agent, not on the command: output that arrives slowly, long
    /// after the headers, is the ordinary shape of a command that runs for a while.
    #[tokio::test]
    async fn a_slow_body_after_prompt_headers_is_not_cut_off() {
        let handler = || async {
            let frames = async_stream_frames(vec![
                "{\"t\":\"stdout\",\"seq\":0,\"data\":\"aGk=\"}\n",
                "{\"t\":\"exit\",\"code\":0,\"truncated\":false}\n",
            ]);
            axum::body::Body::from_stream(frames).into_response()
        };
        let router = Router::new().route("/v1/exec", post(handler));
        let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });

        let response = send(
            reqwest::Client::new().post(format!("http://{address}/v1/exec")),
            RUN_COMMAND,
        )
        .await
        .expect("headers arrive at once");
        let outputs = frame_stream(response, "test-sandbox")
            .collect::<Vec<_>>()
            .await;

        assert_eq!(outputs.len(), 2, "every frame arrived: {outputs:?}");
        assert_eq!(
            outputs[1].as_ref().expect("exit"),
            &CommandOutput::Exit {
                code: 0,
                truncated: false
            }
        );
    }

    /// Frames emitted one at a time, each after a pause longer than the response bound, so the
    /// body as a whole takes several bounds to finish.
    fn async_stream_frames(
        chunks: Vec<&'static str>,
    ) -> impl futures::Stream<Item = std::result::Result<&'static str, std::io::Error>> {
        futures::stream::iter(chunks).then(|chunk| async move {
            tokio::time::sleep(AGENT_RESPONSE_TIMEOUT * 2).await;
            Ok(chunk)
        })
    }

    /// An agent that accepts any request and never sends its headers.
    /// The agent may have started the command before its headers stalled, so a retry could run
    /// A file operation is idempotent, so a stalled one is safe to repeat and says so.
    /// An agent that accepts any request and never sends its headers.
    /// A connection that drops before any headers is the same unknown as a stall: the agent may
    /// The agent may have started the command before its headers stalled, so a retry could run
    /// A file operation is idempotent, so a stalled one is safe to repeat and says so.
    /// An agent that accepts any request and never sends its headers.
    async fn stalled_agent() -> String {
        let handler = || async {
            tokio::time::sleep(AGENT_RESPONSE_TIMEOUT * 20).await;
            "late".into_response()
        };
        let router = Router::new().fallback(handler);
        let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });

        format!("http://{address}")
    }

    /// A connection that drops before any headers is the same unknown as a stall: the agent may
    /// have taken the command before the socket went. It must not read as safe to repeat.
    #[tokio::test]
    async fn a_dropped_run_command_connection_is_not_retryable() {
        let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        // Accept and close at once: the request may have been read, no response ever comes.
        tokio::spawn(async move {
            loop {
                if let Ok((socket, _)) = listener.accept().await {
                    drop(socket);
                }
            }
        });

        let error = send(
            reqwest::Client::new().post(format!("http://{address}/v1/exec")),
            RUN_COMMAND,
        )
        .await
        .expect_err("a dropped connection is a refusal, not a response");

        assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "got: {error}");
        assert!(
            !error.retryable,
            "a command that may have started must not be retried: {error}"
        );
        assert!(
            error.to_string().contains("may have started"),
            "the refusal says the outcome is unknown: {error}"
        );
    }

    /// The agent may have started the command before its headers stalled, so a retry could run
    /// it twice: the refusal must not invite one.
    #[tokio::test]
    async fn a_stalled_run_command_is_not_retryable() {
        let base = stalled_agent().await;

        let error = send(
            reqwest::Client::new().post(format!("{base}/v1/exec")),
            RUN_COMMAND,
        )
        .await
        .expect_err("a stalled agent must be refused, not waited on");

        assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "got: {error}");
        assert!(
            !error.retryable,
            "a command with an unknown outcome must not be retried: {error}"
        );
        assert!(
            error.to_string().contains("did not answer")
                && error.to_string().contains("may have started"),
            "the refusal says the agent stalled and the outcome is unknown: {error}"
        );
    }

    /// A file operation is idempotent, so a stalled one is safe to repeat and says so.
    #[tokio::test]
    async fn a_stalled_file_operation_stays_retryable() {
        let base = stalled_agent().await;

        let error = send(
            reqwest::Client::new().get(format!("{base}/v1/files")),
            "sandbox.readFile",
        )
        .await
        .expect_err("a stalled agent must be refused, not waited on");

        assert_eq!(error.code, "SANDBOX_UNREACHABLE", "got: {error}");
        assert!(
            error.retryable,
            "a stalled file read is safe to repeat: {error}"
        );
        assert!(
            error.to_string().contains("did not answer"),
            "the refusal says the agent stalled: {error}"
        );
    }

    #[tokio::test]
    async fn frames_decode_in_order_with_a_real_exit_code() {
        let outputs = frames_from(vec![
            "{\"t\":\"stdout\",\"seq\":0,\"data\":\"aGk=\"}\n",
            "{\"t\":\"stderr\",\"seq\":1,\"data\":\"b29wcw==\"}\n",
            "{\"t\":\"exit\",\"code\":7,\"truncated\":false}\n",
        ])
        .await;

        assert_eq!(outputs.len(), 3);
        assert_eq!(
            outputs[0].as_ref().expect("stdout"),
            &CommandOutput::Stdout {
                seq: 0,
                data: b"hi".to_vec()
            }
        );
        assert_eq!(
            outputs[1].as_ref().expect("stderr"),
            &CommandOutput::Stderr {
                seq: 1,
                data: b"oops".to_vec()
            }
        );
        assert_eq!(
            outputs[2].as_ref().expect("exit"),
            &CommandOutput::Exit {
                code: 7,
                truncated: false
            }
        );
    }

    /// The protocol says a frame is never split across chunks; TCP makes no such promise. This
    /// is the case a naive per-chunk parser gets wrong, and it fails as a parse error on
    /// perfectly valid output.
    #[tokio::test]
    async fn a_frame_split_across_chunks_is_reassembled() {
        let outputs = frames_from(vec![
            "{\"t\":\"stdo",
            "ut\",\"seq\":0,\"data\":\"aGk=\"}\n{\"t\":\"ex",
            "it\",\"code\":0,\"truncated\":false}\n",
        ])
        .await;

        assert_eq!(
            outputs.len(),
            2,
            "a split frame must not become two frames or an error"
        );
        assert_eq!(
            outputs[0].as_ref().expect("stdout"),
            &CommandOutput::Stdout {
                seq: 0,
                data: b"hi".to_vec()
            }
        );
        assert_eq!(
            outputs[1].as_ref().expect("exit"),
            &CommandOutput::Exit {
                code: 0,
                truncated: false
            }
        );
    }

    /// A body that stops early looks exactly like a command that produced less output — the
    /// difference is only visible in the missing terminal frame. The command had started, so
    /// its end is unknown, and a retry could run it twice: the refusal must not invite one.
    #[tokio::test]
    async fn a_stream_without_a_terminal_frame_is_an_unknown_outcome() {
        let outputs = frames_from(vec!["{\"t\":\"stdout\",\"seq\":0,\"data\":\"aGk=\"}\n"]).await;

        assert_eq!(outputs.len(), 2);
        outputs[0].as_ref().expect("the stdout frame still arrives");
        let error = outputs[1]
            .as_ref()
            .expect_err("a truncated stream must not read as success");
        assert!(
            error.to_string().contains("without a terminal frame"),
            "the failure must name the cause: {error}"
        );
        assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "got: {error}");
        assert!(
            !error.retryable,
            "a command that started and whose end was lost must not be retried: {error}"
        );
    }

    #[tokio::test]
    async fn an_error_frame_surfaces_as_an_error_not_a_silent_end() {
        let outputs = frames_from(vec![
            "{\"t\":\"error\",\"code\":\"deadlineExceeded\",\"message\":\"exceeded its 300ms deadline\"}\n",
        ])
        .await;

        assert_eq!(outputs.len(), 1);
        let error = outputs[0]
            .as_ref()
            .expect_err("an error frame is a failure");
        assert!(error.to_string().contains("deadlineExceeded"), "{error}");
    }
}
