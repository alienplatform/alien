//! The agent's HTTP surface, served from inside the sandbox.
//!
//! Every route that can reach session contents is authorised first, by whichever of the two
//! modes in [`AgentAuthorization`] was fixed at session start. Where the transport cannot say
//! which session a caller may reach, a signed capability says it.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use bytes::Bytes;
use ed25519_compact::PublicKey;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::ErrorData;
use crate::exec::{self, ExecIdentity, ExecRequest, Frame, FRAME_CHANNEL_DEPTH};
use crate::files;
use crate::jobs::{JobOutcome, JobRegistry, JobSnapshot};
use crate::paths::resolve_within_root;
use alien_core::sandbox_capability::{SandboxOperationClass, SandboxSessionIdentity};
use alien_core::sandbox_capability_token;
use alien_error::AlienError;

/// Prefix the MicroVM build and lifecycle probes call inside the guest.
///
/// Not `/ready`: the service's error message names the hook `(/ready)` as a *label*, and the
/// real path carries this prefix. An agent serving the short path 404s every probe, and the
/// image build fails after several minutes with nothing in the logs.
pub const HOOK_PREFIX: &str = "/aws/lambda-microvms/runtime/v1";

/// Full path for one lifecycle hook.
pub fn hook_path(hook: &str) -> String {
    format!("{HOOK_PREFIX}/{hook}")
}

/// The protocol version this agent speaks.
///
/// The agent ships inside the image and outlives the deployment that built it, so this is
/// negotiated rather than assumed — see [`health`].
pub const PROTOCOL_VERSION: u32 = 1;

/// What proves a request may reach this session.
///
/// Two modes because the platforms genuinely differ, and collapsing them would mean either
/// carrying a signing key where the cloud already solves the problem, or trusting a transport
/// that proves nothing. Which one applies is decided at session start, not per request.
pub enum AgentAuthorization {
    /// Every request must carry a capability signed by the session's issuer.
    ///
    /// Kubernetes and Local: reaching the agent proves only that the caller reached the pod or
    /// the loopback route, and a session id is a name a caller could guess.
    Capability {
        /// Public half of the issuer's signing key. The private half never enters a sandbox.
        public_key: PublicKey,
        /// The session this agent serves, and the generation it started under
        identity: SandboxSessionIdentity,
    },

    /// The transport already authorised the caller for exactly this session.
    ///
    /// AWS: the proxy validates a JWE minted with the workload's own IAM identity and scoped to
    /// one MicroVM, an explicit port set, and an expiry — a port outside that set is rejected.
    /// One MicroVM is one session, so the scope the capability would add is already enforced,
    /// and terminate is `TerminateMicrovm`, which destroys the VM rather than fencing it.
    Transport,
}

/// Everything the agent knows about itself, fixed at session start.
pub struct AgentState {
    /// Directory every path is resolved against. Canonical.
    pub session_root: PathBuf,
    /// What a request must present to reach this session
    pub authorization: AgentAuthorization,
    /// The unprivileged identity commands run as, never the agent's own
    pub exec_identity: ExecIdentity,
    /// Bytes of each stream kept before output is truncated
    pub output_cap: usize,
    /// Detached jobs this session is running or retaining for later polls
    pub jobs: JobRegistry,
}

/// Liveness, the version the agent speaks, and the container it runs in.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    /// The protocol version this agent implements
    pub protocol_version: u32,
    /// The kernel boot id of the container this agent runs in. Stable across calls and processes
    /// on one kernel, so a caller compares it across reads to tell its container from a blank one
    /// that replaced it under the same session name.
    pub boot_id: String,
}

/// Optional version assertion from the caller.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthQuery {
    /// The version the caller intends to speak
    pub version: Option<u32>,
}

/// Which file to read.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileQuery {
    /// Path inside the session
    pub path: String,
}

/// File contents on the way out.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileResponse {
    /// Contents, base64 because a file is arbitrary bytes
    pub contents_base64: String,
}

/// A file on the way in.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFileBody {
    /// Path inside the session
    pub path: String,
    /// Contents, base64
    pub contents_base64: String,
}

/// A directory to create.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MkdirBody {
    /// Path inside the session
    pub path: String,
}

/// The id a started job answers to.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStartResponse {
    /// Identifier for later polls and cancellation
    pub job_id: String,
}

/// Which job to poll, and from where.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobPollBody {
    /// The job to read
    pub job_id: String,
    /// Return frames strictly after this sequence; absent returns from the first frame
    #[serde(default)]
    pub since_seq: Option<u64>,
}

/// A job's output so far, and how it ended once it has.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobPollResponse {
    /// Whether the command is still running
    pub running: bool,
    /// Output frames after the polled sequence
    pub frames: Vec<Frame>,
    /// Exit code, present once a job has exited on its own
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Set when output was cut short by the output cap; present once a job has exited
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    /// How a job failed, present when it ended without exiting normally
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JobErrorBody>,
}

/// Why a job did not exit normally.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobErrorBody {
    /// Machine-readable cause, e.g. `deadlineExceeded`
    pub code: String,
    /// Human-readable detail
    pub message: String,
}

impl From<JobSnapshot> for JobPollResponse {
    fn from(snapshot: JobSnapshot) -> Self {
        match snapshot.outcome {
            None => Self {
                running: true,
                frames: snapshot.frames,
                exit_code: None,
                truncated: None,
                error: None,
            },
            Some(JobOutcome::Exited { code, truncated }) => Self {
                running: false,
                frames: snapshot.frames,
                exit_code: Some(code),
                truncated: Some(truncated),
                error: None,
            },
            Some(JobOutcome::Failed { code, message }) => Self {
                running: false,
                frames: snapshot.frames,
                exit_code: None,
                truncated: None,
                error: Some(JobErrorBody { code, message }),
            },
        }
    }
}

/// Which job to cancel.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobCancelBody {
    /// The job to cancel
    pub job_id: String,
}

/// The discriminating fields of an [`agent_platform`] envelope, read before its operation body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvelopeHead {
    /// Protocol version the caller intends to speak. Refused when unsupported, never guessed.
    v: Option<u32>,
    /// Which operation the body carries. Absent or unknown is refused, never defaulted.
    op: Option<String>,
}

/// Builds the agent's router.
pub fn router(state: Arc<AgentState>) -> Router {
    // Base64 inflates by 4/3; the rest is the JSON envelope. Without this axum's 2MB default
    // would reject writes far below the limit `files` documents and enforces itself.
    let body_limit = (crate::files::MAX_TRANSFER_BYTES as usize / 3) * 4 + 4096;

    Router::new()
        .route("/v1/health", get(health))
        .route(&hook_path("ready"), get(hook_ready).post(hook_ready))
        .route(&hook_path("validate"), get(hook_ready).post(hook_ready))
        .route(&hook_path("run"), get(hook_lifecycle).post(hook_lifecycle))
        .route(&hook_path("resume"), get(hook_lifecycle).post(hook_lifecycle))
        .route(&hook_path("suspend"), get(hook_lifecycle).post(hook_lifecycle))
        .route(&hook_path("terminate"), get(hook_lifecycle).post(hook_lifecycle))
        .route("/v1/exec", post(run_command))
        .route("/v1/files", get(read_file).put(write_file))
        .route("/v1/mkdir", post(mkdir))
        .route("/v1/jobs/start", post(job_start))
        .route("/v1/jobs/poll", post(job_poll))
        .route("/v1/jobs/cancel", post(job_cancel))
        // The GCP Agent Platform proxies `:execute` to `POST /` with the body verbatim and can set
        // neither path nor method, so the one route it can reach carries every operation, chosen
        // by `op`. Placed before the body-limit layer so an envelope `writeFile` shares the same
        // ceiling as `/v1/files` rather than falling back to axum's default.
        .route("/", post(agent_platform))
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
        .with_state(state)
}

/// Liveness, and the one place protocol versions are reconciled.
///
/// Unauthenticated: it reports the version and the container boot id — kernel identity, not session
/// contents — so requiring a capability would only stop a liveness probe from working.
async fn health(
    Query(query): Query<HealthQuery>,
) -> std::result::Result<Json<HealthResponse>, ApiError> {
    // A mismatch is named, not negotiated down. Guessing which fields an older peer understands
    // is how a protocol acquires undocumented dialects.
    if let Some(requested) = query.version {
        if requested != PROTOCOL_VERSION {
            return Err(ApiError::from(AlienError::new(
                ErrorData::ProtocolVersionMismatch {
                    requested,
                    supported: PROTOCOL_VERSION,
                },
            )));
        }
    }

    // Failing closed: a caller that cannot read the container identity must not reconnect to a
    // possibly-replaced container, so an unreadable boot id is an error, not a blank field.
    let boot_id = container_boot_id().map_err(|error| {
        ApiError::from(AlienError::new(ErrorData::OperationFailed {
            operation: "read container boot id".to_string(),
            reason: error.to_string(),
        }))
    })?;

    Ok(Json(HealthResponse {
        protocol_version: PROTOCOL_VERSION,
        boot_id,
    }))
}

/// The kernel boot id of the container this agent runs in.
///
/// `/proc/sys/kernel/random/boot_id` is stable across calls and processes on one kernel and
/// changes only when the container is replaced, which is the identity a caller's `generation` is
/// derived from.
#[cfg(target_os = "linux")]
fn container_boot_id() -> std::io::Result<String> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id").map(|id| id.trim().to_string())
}

/// A non-Linux dev build has no `/proc` boot id and never runs a real sandbox reconnect, so a
/// fixed sentinel stands in rather than a per-run value that would read as a fresh container.
#[cfg(not(target_os = "linux"))]
fn container_boot_id() -> std::io::Result<String> {
    Ok("dev-build-no-boot-id".to_string())
}

/// The image's readiness and validation hooks.
///
/// AWS snapshots the MicroVM once this answers 200, and every later MicroVM boots from that
/// snapshot — 503 means "not yet". Reaching this handler *is* the readiness signal: the router
/// is live by then, so there is nothing further to wait for.
///
/// Unauthenticated, like `/v1/health`: the MicroVM service calls it, not a session caller, and it
/// reveals nothing about the session.
async fn hook_ready() -> StatusCode {
    StatusCode::OK
}

/// The run / resume / suspend / terminate hooks.
///
/// Enabled because a declared hook that cannot be reached fails the image build. Two MicroVMs
/// restored from one image differ in `/dev/urandom` while `boot_id` is identical, so entropy
/// separation comes from the platform and the residue is kernel identity, which userspace cannot
/// reset. This acknowledges and claims nothing more.
async fn hook_lifecycle() -> StatusCode {
    StatusCode::OK
}

async fn run_command(
    State(state): State<Arc<AgentState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<ExecRequest>,
) -> std::result::Result<Response, ApiError> {
    authorize(&state, peer, &headers, SandboxOperationClass::Execute)?;

    // Resolved before anything is spawned, so a refused directory is an error response rather
    // than a stream whose first frame is a failure.
    let working_directory = match &request.working_directory {
        Some(path) => resolve_within_root(&state.session_root, path)?,
        None => state.session_root.clone(),
    };

    let (sender, receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);
    let output_cap = state.output_cap;
    let identity = state.exec_identity;
    tokio::spawn(async move {
        exec::stream(&request, Some(&working_directory), identity, output_cap, sender).await;
    });

    let frames = futures::stream::unfold(receiver, |mut receiver| async move {
        let frame = receiver.recv().await?;
        Some((encode_frame(&frame), receiver))
    });

    Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(frames))
        .map_err(|error| {
            ApiError::from(AlienError::new(ErrorData::OperationFailed {
                operation: "stream command output".to_string(),
                reason: error.to_string(),
            }))
        })
}

/// Serializes one frame as an NDJSON line.
///
/// A serialization failure aborts the body rather than skipping the frame: a caller that sees a
/// truncated stream reports a transport failure, where a silently dropped frame could look like
/// a command that produced less output than it did.
fn encode_frame(frame: &Frame) -> std::result::Result<Bytes, std::io::Error> {
    let mut line = serde_json::to_vec(frame).map_err(std::io::Error::other)?;
    line.push(b'\n');
    Ok(Bytes::from(line))
}

async fn read_file(
    State(state): State<Arc<AgentState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<FileQuery>,
) -> std::result::Result<Json<ReadFileResponse>, ApiError> {
    authorize(&state, peer, &headers, SandboxOperationClass::Execute)?;

    let contents = files::read(&state.session_root, &query.path).await?;

    Ok(Json(ReadFileResponse {
        contents_base64: BASE64.encode(contents),
    }))
}

async fn write_file(
    State(state): State<Arc<AgentState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<WriteFileBody>,
) -> std::result::Result<StatusCode, ApiError> {
    authorize(&state, peer, &headers, SandboxOperationClass::Execute)?;

    let contents = BASE64.decode(&body.contents_base64).map_err(|error| {
        AlienError::new(ErrorData::RequestInvalid {
            reason: format!("contents are not valid base64: {error}"),
        })
    })?;

    files::write(&state.session_root, &body.path, &contents).await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn mkdir(
    State(state): State<Arc<AgentState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<MkdirBody>,
) -> std::result::Result<StatusCode, ApiError> {
    authorize(&state, peer, &headers, SandboxOperationClass::Execute)?;

    files::mkdir(&state.session_root, &body.path).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Starts a command as a detached job whose output is polled for rather than streamed.
///
/// The provider chooses this over `/v1/exec` when a command's deadline is longer than one proxied
/// call can stay open. The command runs under the same deadline; only its lifetime is detached.
async fn job_start(
    State(state): State<Arc<AgentState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<ExecRequest>,
) -> std::result::Result<Json<JobStartResponse>, ApiError> {
    authorize(&state, peer, &headers, SandboxOperationClass::Execute)?;

    // Resolved here, as in `run_command`, so a refused directory answers with an error rather than a
    // job whose first frame is a failure.
    let working_directory = match &request.working_directory {
        Some(path) => resolve_within_root(&state.session_root, path)?,
        None => state.session_root.clone(),
    };

    let job_id = state
        .jobs
        .start(request, working_directory, state.exec_identity, state.output_cap)?;

    Ok(Json(JobStartResponse { job_id }))
}

/// Returns a job's output after a sequence, and its ending once it has one.
async fn job_poll(
    State(state): State<Arc<AgentState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<JobPollBody>,
) -> std::result::Result<Json<JobPollResponse>, ApiError> {
    authorize(&state, peer, &headers, SandboxOperationClass::Execute)?;

    let snapshot = state.jobs.poll(&body.job_id, body.since_seq).ok_or_else(|| {
        ApiError::from(AlienError::new(ErrorData::JobNotFound {
            job_id: body.job_id.clone(),
        }))
    })?;

    Ok(Json(JobPollResponse::from(snapshot)))
}

/// Cancels a job, killing its process group.
async fn job_cancel(
    State(state): State<Arc<AgentState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<JobCancelBody>,
) -> std::result::Result<Json<serde_json::Value>, ApiError> {
    authorize(&state, peer, &headers, SandboxOperationClass::Execute)?;

    if !state.jobs.cancel(&body.job_id) {
        return Err(ApiError::from(AlienError::new(ErrorData::JobNotFound {
            job_id: body.job_id,
        })));
    }

    Ok(Json(serde_json::json!({})))
}

/// The single endpoint the GCP Agent Platform can reach, dispatching by the envelope's `op`.
///
/// The version is reconciled and the `op` resolved before any handler runs; each arm then hands
/// off to the matching `/v1/*` handler, so the response — the exec NDJSON stream included — is the
/// same bytes that route produces, and authorization stays that handler's job.
async fn agent_platform(
    State(state): State<Arc<AgentState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Response, ApiError> {
    let head: EnvelopeHead = serde_json::from_slice(&body).map_err(|error| {
        ApiError::from(AlienError::new(ErrorData::RequestInvalid {
            reason: format!("envelope is not valid JSON: {error}"),
        }))
    })?;

    let requested = head.v.ok_or_else(|| {
        ApiError::from(AlienError::new(ErrorData::RequestInvalid {
            reason: "an envelope must carry a protocol version 'v'".to_string(),
        }))
    })?;
    if requested != PROTOCOL_VERSION {
        return Err(ApiError::from(AlienError::new(
            ErrorData::ProtocolVersionMismatch {
                requested,
                supported: PROTOCOL_VERSION,
            },
        )));
    }

    let op = head.op.ok_or_else(|| {
        ApiError::from(AlienError::new(ErrorData::RequestInvalid {
            reason: "an envelope must name an 'op'".to_string(),
        }))
    })?;

    // The operation's fields are re-read from the same bytes into the body the matching `/v1/*`
    // handler takes; that works only because those types ignore the envelope's `v`/`op`. Adding
    // `deny_unknown_fields` to one would break this dispatch at runtime, with nothing to catch it.
    match op.as_str() {
        "exec" => run_command(State(state), ConnectInfo(peer), headers, Json(reparse(&body)?)).await,
        "readFile" => Ok(read_file(State(state), ConnectInfo(peer), headers, Query(reparse(&body)?))
            .await?
            .into_response()),
        "writeFile" => Ok(
            write_file(State(state), ConnectInfo(peer), headers, Json(reparse(&body)?))
                .await?
                .into_response(),
        ),
        "mkdir" => Ok(mkdir(State(state), ConnectInfo(peer), headers, Json(reparse(&body)?))
            .await?
            .into_response()),
        "jobStart" => Ok(
            job_start(State(state), ConnectInfo(peer), headers, Json(reparse(&body)?))
                .await?
                .into_response(),
        ),
        "jobPoll" => Ok(
            job_poll(State(state), ConnectInfo(peer), headers, Json(reparse(&body)?))
                .await?
                .into_response(),
        ),
        "jobCancel" => Ok(
            job_cancel(State(state), ConnectInfo(peer), headers, Json(reparse(&body)?))
                .await?
                .into_response(),
        ),
        "health" => Ok(health(Query(HealthQuery { version: None })).await?.into_response()),
        other => Err(ApiError::from(AlienError::new(ErrorData::RequestInvalid {
            reason: format!("unknown op '{other}'"),
        }))),
    }
}

/// Reads the operation's own fields out of an envelope body once its `op` has selected the type.
fn reparse<T: serde::de::DeserializeOwned>(body: &Bytes) -> std::result::Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|error| {
        ApiError::from(AlienError::new(ErrorData::RequestInvalid {
            reason: format!("envelope body did not match its op: {error}"),
        }))
    })
}

/// Verifies the request may reach this session, or refuses it.
fn authorize(
    state: &AgentState,
    peer: SocketAddr,
    headers: &HeaderMap,
    required: SandboxOperationClass,
) -> std::result::Result<(), ApiError> {
    let AgentAuthorization::Capability {
        public_key,
        identity,
    } = &state.authorization
    else {
        // Transport mode trusts what arrives through the transport. The command this agent
        // spawned shares the guest's network stack and reaches the same port without it, so a
        // caller has to be from off the machine, or in the guest under some other user.
        if !crate::peer::transport_may_serve(peer, state.exec_identity.uid) {
            return Err(ApiError {
                status: StatusCode::FORBIDDEN,
                message: "the agent does not serve the code it is running".to_string(),
            });
        }
        return Ok(());
    };

    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "a capability is required".to_string(),
        })?;

    sandbox_capability_token::verify(
        token,
        public_key,
        identity,
        required,
        chrono::Utc::now().timestamp(),
    )
    .map_err(ApiError::from)?;

    Ok(())
}

/// An error on its way back to the caller.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl<T> From<AlienError<T>> for ApiError
where
    T: alien_error::AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
{
    fn from(error: AlienError<T>) -> Self {
        let status = error
            .http_status_code
            .and_then(|code| StatusCode::from_u16(code).ok())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        // Whatever shares the pod's network namespace can reach this surface, so an error marked
        // internal is reported by code alone. No variant is internal today; the gate is here so
        // that adding one does not silently start leaking.
        let message = if error.internal {
            error.code.to_string()
        } else {
            error.to_string()
        };

        Self { status, message }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}
