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
use crate::traits::{CommandOutput, JobError, JobExit, JobPoll, JobStart, RunCommandRequest};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};

pub use alien_core::sandbox_process::AGENT_PORT;

/// Named once: `send` treats it as the one operation a 5xx must not be retried for.
const RUN_COMMAND: &str = "sandbox.runCommand";

const JOB_START: &str = "sandbox.jobStart";
const JOB_POLL: &str = "sandbox.jobPoll";
const JOB_CANCEL: &str = "sandbox.jobCancel";

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobStartResponse {
    job_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobPollResponse {
    running: bool,
    #[serde(default)]
    frames: Vec<AgentFrame>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    truncated: Option<bool>,
    #[serde(default)]
    error: Option<JobErrorResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobErrorResponse {
    code: String,
    message: String,
}

/// Runs a command, streaming frames as the agent produces them.
pub async fn run_command<T: AgentTransport + ?Sized>(
    transport: &T,
    session_id: &str,
    request: RunCommandRequest,
) -> Result<BoxStream<'static, Result<CommandOutput>>> {
    let body = exec_body(&request)?;

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

/// Starts a command as a job the agent owns until it is polled to its end or cancelled.
pub async fn start_job<T: AgentTransport + ?Sized>(
    transport: &T,
    session_id: &str,
    request: RunCommandRequest,
) -> Result<JobStart> {
    let body = exec_body(&request)?;

    let response = send(
        transport
            .request(session_id, reqwest::Method::POST, "/v1/jobs/start")
            .await?
            .json(&body),
        JOB_START,
    )
    .await?;

    let started: JobStartResponse = response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::UnexpectedResponseFormat {
            provider: transport.provider().to_string(),
            binding_name: JOB_START.to_string(),
            field: "jobId".to_string(),
            response_json: "the agent returned a body this provider cannot parse".to_string(),
        })
        // The job is running and nothing can now poll or cancel it, so its end is unestablished
        // rather than merely unreadable.
        .context(unanswered(
            JOB_START,
            "the job started and its id could not be read",
        ))?;

    Ok(JobStart {
        job_id: started.job_id,
    })
}

/// Reads a job's output after `since_seq`, and its ending once it has one.
pub async fn poll_job<T: AgentTransport + ?Sized>(
    transport: &T,
    session_id: &str,
    job_id: &str,
    since_seq: Option<u64>,
) -> Result<JobPoll> {
    let response = send(
        transport
            .request(session_id, reqwest::Method::POST, "/v1/jobs/poll")
            .await?
            .json(&json!({ "jobId": job_id, "sinceSeq": since_seq })),
        JOB_POLL,
    )
    .await?;

    let JobPollResponse {
        running,
        frames,
        exit_code,
        truncated,
        error,
    } = response
        .json()
        .await
        .into_alien_error()
        .context(unanswered(JOB_POLL, "the poll's body could not be read"))?;

    Ok(JobPoll {
        running,
        frames: frames
            .into_iter()
            .map(|frame| frame.into_output(transport.provider()))
            .collect::<Result<Vec<_>>>()?,
        exit: exit_code.map(|code| JobExit {
            code,
            truncated: truncated.unwrap_or(false),
        }),
        error: error.map(|error| JobError {
            code: error.code,
            message: error.message,
        }),
    })
}

/// Cancels a job, stopping the command it runs.
pub async fn cancel_job<T: AgentTransport + ?Sized>(
    transport: &T,
    session_id: &str,
    job_id: &str,
) -> Result<()> {
    send(
        transport
            .request(session_id, reqwest::Method::POST, "/v1/jobs/cancel")
            .await?
            .json(&json!({ "jobId": job_id })),
        JOB_CANCEL,
    )
    .await?;

    Ok(())
}

/// The body `/v1/exec` and `/v1/jobs/start` both take.
fn exec_body(request: &RunCommandRequest) -> Result<serde_json::Value> {
    // Checked after conversion, not on the Duration: a sub-millisecond deadline is non-zero here
    // and floors to `deadlineMs: 0`, which the agent then refuses as invalid.
    if deadline_millis(request.deadline) == 0 {
        return Err(AlienError::new(ErrorData::SandboxCommandFailed {
            failure: "invalidRequest".to_string(),
            reason: "a command must carry a non-zero deadline".to_string(),
        }));
    }

    Ok(json!({
        "command": request.command,
        "deadlineMs": deadline_millis(request.deadline),
        "workingDirectory": request.working_directory,
        "env": request.env,
    }))
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
    // Both take a command the agent may already be running, so an unanswered one leaves the
    // outcome unestablished. Every other operation is idempotent and safe to send again.
    if operation == RUN_COMMAND || operation == JOB_START {
        return ErrorData::SandboxOutcomeUnknown {
            operation: operation.to_string(),
            reason: reason.to_string(),
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
        Err(error) => {
            return Err(error).into_alien_error().context(unanswered(
                operation,
                &format!("{operation} returned {status} and its body could not be read"),
            ))
        }
    };

    // A 5xx never proves the agent answered, with or without a body: the cloud's proxy emits them
    // too, and a 504 means the request reached the guest and no answer came back in time. A body
    // is no evidence either way — a proxy's error page is a body. So a 5xx is the unanswered case,
    // which for the idempotent file operations is still safe to repeat and for `run_command` is
    // not. A 4xx is the agent refusing before it does anything, which is an answer.
    if status.is_server_error() {
        return Err(AlienError::new(unanswered(
            operation,
            &format!("the sandbox host returned {status}: {body}"),
        )));
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
                        state.buffer.clear();
                        // Classified like the decode arms in `into_output`, and for the same
                        // reason: the frame arrived, so the command ran.
                        let failure = malformed(&error.to_string(), state.provider)
                            .context(unanswered(RUN_COMMAND, "an output frame did not parse"));
                        return Some((Err(failure), state));
                    }
                };

                if matches!(frame, AgentFrame::Exit { .. } | AgentFrame::Error { .. }) {
                    state.saw_terminal = true;
                }

                let output = frame.into_output(state.provider);
                // A failure here ends the stream. Yielding whatever the buffer still holds would
                // let a later exit frame answer the question this item just reported as
                // unanswerable, and which of the two a caller believes would depend only on
                // whether it stopped at the first error.
                if output.is_err() {
                    state.finished = true;
                    state.buffer.clear();
                }
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
            // A frame that arrived is proof the command ran, so a payload that will not decode
            // leaves the outcome unestablished rather than merely malformed — reading it as a
            // format problem would let a caller repeat a command that already executed.
            Self::Stdout { seq, data } => Ok(CommandOutput::Stdout {
                seq,
                data: decode(&data, provider, RUN_COMMAND, "data")
                    .context(unanswered(RUN_COMMAND, "an output frame did not decode"))?,
            }),
            Self::Stderr { seq, data } => Ok(CommandOutput::Stderr {
                seq,
                data: decode(&data, provider, RUN_COMMAND, "data")
                    .context(unanswered(RUN_COMMAND, "an output frame did not decode"))?,
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
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::{Json, Router};
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

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
        assert_eq!(
            error.code, "SANDBOX_OUTCOME_UNKNOWN",
            "a proxy can synthesize a 502 after the agent accepted the request, so the caller has \
             to be told the outcome is unknown rather than that it is safe to repeat: {rendered}"
        );
    }

    /// A body is not evidence of who sent it: a proxy's error page is a body too, and a 504 in
    /// particular means the request reached the guest. The cause still has to survive for whoever
    /// reads the failure.
    #[tokio::test]
    async fn a_server_error_is_unknown_however_much_body_it_carries() {
        let error = send_status(StatusCode::INTERNAL_SERVER_ERROR, "spawn failed: ENOMEM").await;
        let rendered = error.to_string();

        assert!(
            rendered.contains("spawn failed: ENOMEM"),
            "the cause has to survive: {rendered}"
        );
        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "got: {rendered}");
    }

    /// The scenario the rule above exists for: a gateway timing out on a command that is still
    /// running, and answering with its own HTML.
    #[tokio::test]
    async fn a_gateway_timeout_with_an_error_page_does_not_read_as_the_agent_refusing() {
        let error = send_status(
            StatusCode::GATEWAY_TIMEOUT,
            "<html><body>504 Gateway Time-out</body></html>",
        )
        .await;

        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "got: {error}");
        assert!(
            !error.retryable,
            "the command may still be running behind the gateway: {error}"
        );
    }

    /// A 4xx is the agent refusing before it does anything, which is an answer.
    #[tokio::test]
    async fn a_client_error_is_the_agent_answering() {
        let error = send_status(StatusCode::BAD_REQUEST, "the command was empty").await;

        assert_ne!(
            error.code, "SANDBOX_OUTCOME_UNKNOWN",
            "a refusal before dispatch establishes that nothing ran: {error}"
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

        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "got: {error}");
        assert!(
            !error.retryable,
            "a command that may have started must not be retried: {error}"
        );
        assert!(
            error.to_string().contains("may have taken effect"),
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

        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "got: {error}");
        assert!(
            !error.retryable,
            "a command with an unknown outcome must not be retried: {error}"
        );
        assert!(
            error.to_string().contains("did not answer")
                && error.to_string().contains("may have taken effect"),
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

    /// A frame that arrived is proof the command ran, so a payload that will not decode leaves the
    /// outcome unestablished. Reported as a format problem it would read as safe to repeat, and the
    /// repeat would be a second execution.
    #[tokio::test]
    async fn a_frame_that_arrives_but_does_not_decode_leaves_the_outcome_unknown() {
        let outputs = frames_from(vec![
            "{\"t\":\"stdout\",\"seq\":0,\"data\":\"!!not base64!!\"}\n",
        ])
        .await;

        let error = outputs[0]
            .as_ref()
            .expect_err("a payload that does not decode is not output");
        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "got: {error}");
        assert!(
            !error.retryable,
            "the command ran; a repeat would run it twice: {error}"
        );
        assert!(
            error.to_string().contains("base64"),
            "the decode failure must stay in the chain: {error}"
        );
    }

    /// A line that does not parse arrives the same way a decodable one does, so it carries the same
    /// proof that the command ran.
    #[tokio::test]
    async fn a_frame_that_does_not_parse_leaves_the_outcome_unknown() {
        let outputs = frames_from(vec!["{not json at all}\n"]).await;

        let error = outputs[0]
            .as_ref()
            .expect_err("a malformed frame is not output");
        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "got: {error}");
        assert!(!error.retryable, "the command ran: {error}");
        assert!(
            error.to_string().contains("did not parse"),
            "the parse failure must stay in the chain: {error}"
        );
    }

    /// One chunk can hold both a frame that will not parse and the exit frame after it. The stream
    /// has to stop at the first: yielding the exit as well would answer the question the failure
    /// just reported as unanswerable, and a caller that reads to the end would believe the wrong
    /// one of the two.
    #[tokio::test]
    async fn an_unestablished_outcome_ends_the_stream_mid_chunk() {
        let outputs = frames_from(vec![
            "{not json at all}\n{\"t\":\"exit\",\"code\":0,\"truncated\":false}\n",
        ])
        .await;

        assert_eq!(
            outputs.len(),
            1,
            "the exit frame must not follow the failure"
        );
        let error = outputs[0]
            .as_ref()
            .expect_err("a malformed frame is not output");
        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "got: {error}");
    }

    /// The same rule for a payload that arrives whole and will not decode.
    #[tokio::test]
    async fn a_decode_failure_ends_the_stream_mid_chunk() {
        let outputs = frames_from(vec![
            "{\"t\":\"stdout\",\"seq\":0,\"data\":\"!!\"}\n{\"t\":\"exit\",\"code\":0,\"truncated\":false}\n",
        ])
        .await;

        assert_eq!(
            outputs.len(),
            1,
            "the exit frame must not follow the failure"
        );
        assert_eq!(
            outputs[0]
                .as_ref()
                .expect_err("a bad payload is not output")
                .code,
            "SANDBOX_OUTCOME_UNKNOWN"
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
        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "got: {error}");
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

    /// A transport that authorizes nothing, so the tests exercise the protocol rather than a
    /// backend's credentials.
    #[derive(Debug)]
    struct TestTransport(String);

    #[async_trait]
    impl AgentTransport for TestTransport {
        async fn request(
            &self,
            _session_id: &str,
            method: reqwest::Method,
            path: &str,
        ) -> Result<reqwest::RequestBuilder> {
            Ok(reqwest::Client::new().request(method, format!("{}{path}", self.0)))
        }

        fn provider(&self) -> &'static str {
            "test-sandbox"
        }
    }

    async fn serve(router: Router) -> TestTransport {
        let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });
        TestTransport(format!("http://{address}"))
    }

    /// What the agent was asked for, so a test can assert the cursor it received rather than only
    /// the frames it chose to send back.
    type Cursors = Arc<Mutex<Vec<Option<u64>>>>;

    /// An agent running one job: it reports a frame while running, then its exit.
    async fn job_agent(cursors: Cursors) -> TestTransport {
        async fn poll(
            State(cursors): State<Cursors>,
            Json(body): Json<serde_json::Value>,
        ) -> impl IntoResponse {
            let since = body.get("sinceSeq").and_then(serde_json::Value::as_u64);
            cursors.lock().expect("cursors").push(since);
            match since {
                None => Json(json!({
                    "running": true,
                    "frames": [{ "t": "stdout", "seq": 0, "data": "aGk=" }],
                })),
                Some(_) => Json(json!({
                    "running": false,
                    "frames": [],
                    "exitCode": 7,
                    "truncated": false,
                })),
            }
        }

        serve(
            Router::new()
                .route(
                    "/v1/jobs/start",
                    post(|| async { Json(json!({"jobId": "j1"})) }),
                )
                .route("/v1/jobs/poll", post(poll))
                .route("/v1/jobs/cancel", post(|| async { Json(json!({})) }))
                .with_state(cursors),
        )
        .await
    }

    /// The whole job round trip: a start that names the job, a first poll that reads from the
    /// beginning, a second that asks only for what is new, and a cancel the agent accepts.
    #[tokio::test]
    async fn a_job_starts_polls_from_its_cursor_and_cancels() {
        let cursors: Cursors = Arc::new(Mutex::new(Vec::new()));
        let transport = job_agent(Arc::clone(&cursors)).await;

        let started = start_job(&transport, "s1", command(Duration::from_secs(600)))
            .await
            .expect("the job starts");
        assert_eq!(started.job_id, "j1");

        let first = poll_job(&transport, "s1", &started.job_id, None)
            .await
            .expect("the first poll answers");
        assert!(first.running, "the job is still running: {first:?}");
        assert_eq!(
            first.frames,
            vec![CommandOutput::Stdout {
                seq: 0,
                data: b"hi".to_vec()
            }],
            "the agent's base64 frame is decoded"
        );
        assert!(first.exit.is_none() && first.error.is_none());

        let second = poll_job(&transport, "s1", &started.job_id, Some(0))
            .await
            .expect("the second poll answers");
        assert!(!second.running);
        assert!(second.frames.is_empty(), "nothing follows the last frame");
        assert_eq!(
            second.exit,
            Some(crate::traits::JobExit {
                code: 7,
                truncated: false
            }),
            "the ending is the envelope's, not a frame's"
        );

        cancel_job(&transport, "s1", &started.job_id)
            .await
            .expect("the cancel is accepted");

        assert_eq!(
            *cursors.lock().expect("cursors"),
            vec![None, Some(0)],
            "the cursor a caller passes has to reach the agent, or every poll replays the whole \
             output and a caller sees each frame twice"
        );
    }

    /// A start the agent may have taken is the one job call that must not invite a retry.
    #[tokio::test]
    async fn a_server_error_on_a_start_leaves_the_outcome_unknown() {
        let transport = serve(Router::new().route(
            "/v1/jobs/start",
            post(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "spawn failed").into_response() }),
        ))
        .await;

        let error = start_job(&transport, "s1", command(Duration::from_secs(600)))
            .await
            .expect_err("a 5xx is not a job that started");

        assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN", "got: {error}");
        assert!(
            !error.retryable,
            "the agent may have taken the command, and a repeat would run it twice: {error}"
        );
        assert!(
            error.to_string().contains("sandbox.jobStart"),
            "the operation reaches callers and telemetry, so a failed start has to be tellable \
             apart from a failed streaming command: {error}"
        );
    }

    /// A poll changes nothing about the job, so a failed one is worth repeating — the opposite of
    /// the rule above, and the reason the two carry different operation names.
    #[tokio::test]
    async fn a_server_error_on_a_poll_stays_retryable() {
        let transport = serve(Router::new().route(
            "/v1/jobs/poll",
            post(|| async { (StatusCode::SERVICE_UNAVAILABLE, "").into_response() }),
        ))
        .await;

        let error = poll_job(&transport, "s1", "j1", Some(4))
            .await
            .expect_err("a 5xx is not a poll that answered");

        assert_eq!(error.code, "SANDBOX_UNREACHABLE", "got: {error}");
        assert!(error.retryable, "the job is untouched: {error}");
    }

    /// A body that stops half way is indistinguishable from a poll that never answered, and the
    /// job is still there to be polled again.
    #[tokio::test]
    async fn a_poll_body_that_ends_early_is_retryable() {
        let transport = serve(Router::new().route(
            "/v1/jobs/poll",
            post(|| async {
                axum::body::Body::from_stream(futures::stream::iter(vec![
                    Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"{\"running\":tr")),
                    Err(std::io::Error::other("the connection went")),
                ]))
                .into_response()
            }),
        ))
        .await;

        let error = poll_job(&transport, "s1", "j1", None)
            .await
            .expect_err("a truncated body is not a poll");

        assert_eq!(error.code, "SANDBOX_UNREACHABLE", "got: {error}");
        assert!(error.retryable, "polling again costs nothing: {error}");
    }

    /// A cancel the agent refuses — an unknown job, say — is an answer, so it reaches the caller
    /// as the agent's refusal rather than as a sandbox that could not be reached.
    #[tokio::test]
    async fn a_refused_cancel_is_the_agent_answering() {
        let transport = serve(Router::new().route(
            "/v1/jobs/cancel",
            post(|| async { (StatusCode::NOT_FOUND, "JOB_NOT_FOUND").into_response() }),
        ))
        .await;

        let error = cancel_job(&transport, "s1", "j1")
            .await
            .expect_err("a 404 is not a cancel that landed");

        assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "got: {error}");
        assert!(
            error.to_string().contains("JOB_NOT_FOUND"),
            "the agent's own reason has to survive: {error}"
        );
    }

    fn command(deadline: Duration) -> RunCommandRequest {
        RunCommandRequest {
            command: vec!["/bin/sleep".to_string(), "600".to_string()],
            working_directory: None,
            env: BTreeMap::new(),
            deadline,
        }
    }
}
