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

/// Sends a request and turns a non-success into a typed error carrying the agent's own reason.
///
/// A transport failure here is marked retryable, which holds for the file operations but not for
/// `run_command` — that request may have already started the command. Nothing retries on this
/// path today; whoever adds a retry layer has to treat `run_command` as the exception.
pub async fn send(request: reqwest::RequestBuilder, operation: &str) -> Result<reqwest::Response> {
    let response =
        request
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::SandboxUnreachable {
                operation: operation.to_string(),
                reason: "the request never reached the agent".to_string(),
            })?;

    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    // Read the body first: the agent puts the actual cause there, and a bare status turns a
    // specific refusal into a guess.
    let body = response.text().await.unwrap_or_default();

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
                Some(Err(error)) => {
                    state.finished = true;
                    return Some((
                        Err(AlienError::new(ErrorData::SandboxUnreachable {
                            operation: RUN_COMMAND.to_string(),
                            reason: format!("the output stream failed: {error}"),
                        })),
                        state,
                    ));
                }
                None => {
                    state.finished = true;
                    if !state.saw_terminal {
                        return Some((
                            Err(AlienError::new(ErrorData::SandboxUnreachable {
                                operation: RUN_COMMAND.to_string(),
                                reason: "the output stream ended without a terminal frame"
                                    .to_string(),
                            })),
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

    /// A body that stops early looks exactly like a command that produced less
    /// output — the difference is only visible in the missing terminal frame.
    #[tokio::test]
    async fn a_stream_without_a_terminal_frame_is_a_transport_failure() {
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
        // Asserted on the code, not just the message: a dropped connection is retryable, and the
        // message reads the same whichever variant carries it.
        assert_eq!(error.code, "SANDBOX_UNREACHABLE", "got: {error}");
        assert!(error.retryable, "a dropped stream must stay retryable");
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
