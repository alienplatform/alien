//! GCP Agent Platform sandbox provider.
//!
//! Sessions are `sandboxEnvironments` created under a durable reasoning engine and reached from
//! outside the guest through the `:execute` proxy, which forwards one request to the agent's
//! `POST /` envelope and returns its body verbatim. So every command, file operation and health
//! check is one envelope over that proxy, and the lifecycle verbs are long-running operations
//! polled to completion.
//!
//! Unregistered on purpose: it is compiled and unit-tested but no factory selects it, so no
//! declaration can reach it until the cutover wires it in.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures::stream::{self, BoxStream};
use serde::Deserialize;
use serde_json::json;
use tracing::warn;

use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, CommandOutput, CreateSessionRequest, PreviewCapability, RunCommandRequest, Sandbox,
    SandboxSession, SandboxSessionState,
};
use alien_core::{SandboxCapabilities, SandboxEgress};
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::gcp::agent_platform::{
    AgentPlatformApi, AgentPlatformErrorData, EgressControlConfig, SandboxCreateRequest,
    SandboxEnvironment, SandboxSnapshot,
};
use alien_gcp_clients::gcp::longrunning::{Operation, OperationResult};

/// The envelope protocol version this provider speaks. It matches the agent's `PROTOCOL_VERSION`;
/// a peer that answers a different one is refused rather than guessed at.
const AGENT_PROTOCOL_VERSION: u32 = 1;

/// The proxy holds one `:execute` request open for roughly this long, so a command whose deadline
/// is within it runs synchronously and anything longer is detached as a job and polled. Set below
/// the measured ceiling, because a command that overruns a synchronous execute is lost, where an
/// overrun job is still reachable by a later poll.
const MAX_SYNCHRONOUS_DEADLINE: Duration = Duration::from_secs(30);

/// Longest session id this provider will place in a proxy URL. A bound on what is handed back to a
/// caller, not on what the API mints — the names seen are far shorter.
const MAX_SESSION_ID: usize = 63;

/// How long a created sandbox has to reach `STATE_RUNNING`, and how often that is checked.
const SESSION_READY_ATTEMPTS: u32 = 150;
const SESSION_READY_INTERVAL: Duration = Duration::from_secs(2);

/// How long a lifecycle operation (`create`, `:pause`, `:resume`, `:snapshot`) is polled before it
/// is reported incomplete rather than waited on forever.
const OPERATION_POLL_ATTEMPTS: u32 = 150;
const OPERATION_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How long `terminate` polls the sandbox to `not-found`, turning an accepted delete into a
/// confirmed one.
const TERMINATE_POLL_ATTEMPTS: u32 = 30;
const TERMINATE_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How often a detached job is polled for new output.
const JOB_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The grace a job's poll loop allows past the command's own deadline before it cancels the job:
/// the agent kills the command at the deadline and the next poll reports it, and this covers the
/// round trips to observe that.
const JOB_POLL_GRACE: Duration = Duration::from_secs(15);

const CREATE: &str = "sandbox.create";
const GET: &str = "sandbox.get";
const GET_OR_CREATE: &str = "sandbox.getOrCreate";
const RUN_COMMAND: &str = "sandbox.runCommand";
const TERMINATE: &str = "sandbox.terminate";

/// The generation of a session whose live container identity was not established: a state with no
/// reachable agent, or a bulk `list` that does not probe each session. Never a value
/// `generation_from_boot_id` returns, so a real identity is always distinguishable from an
/// unprobed one.
const NO_GENERATION: u64 = 0;

/// A single health probe is bounded to this, because the client sets no per-request timeout and an
/// agent that accepts the connection but never answers would otherwise hang `get()` and `create()`
/// forever. Set above the proxy's ~30s synchronous window (see `MAX_SYNCHRONOUS_DEADLINE`) rather
/// than tight to the round trip: too tight reports a healthy session unreachable, and
/// `get_or_create` then provisions a fresh sandbox and loses the caller's filesystem — the failure
/// this task exists to prevent — where too loose only delays an already-broken session.
const AGENT_PROBE_BUDGET: Duration = Duration::from_secs(60);

/// Maps a declared egress mode onto the template's `egressControlConfig`, or refuses one the API
/// cannot express.
///
/// `internetAccess` is a single boolean, so `AllowDomains` has no representation and is refused
/// rather than approximated into `allow` (which would open more than was asked) or `deny` (which
/// would close a caller out of hosts it named). Not called by the runtime verbs — the template is
/// pre-created — but this is the mapping the template controller uses, kept beside the provider so
/// the two agree on what a mode means. `sandbox_label` names the offending sandbox in the refusal.
pub fn egress_control_config(
    sandbox_label: &str,
    egress: &SandboxEgress,
) -> Result<EgressControlConfig> {
    let Some(internet_access) = egress.internet_access_switch() else {
        return Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: "sandbox.template".to_string(),
            details: format!(
                "sandbox '{sandbox_label}' asked for domain-scoped egress, which Agent \
                 Platform cannot express; it offers only 'allow' (open) and 'deny' (closed)"
            ),
            field_name: Some("egress".to_string()),
        }));
    };

    Ok(EgressControlConfig {
        internet_access: Some(internet_access),
        extra: Default::default(),
    })
}

/// A Sandbox backed by the Vertex AI Agent Platform.
#[derive(Debug)]
pub struct GcpAgentPlatformSandbox {
    client: Arc<dyn AgentPlatformApi>,
    /// Bare reasoning-engine id the client interpolates into its paths. The binding may carry a
    /// full resource name, so it is reduced to its last segment once, here.
    engine: String,
    /// Template every session is cut from, as a resource name the create body carries unchanged.
    template: String,
    /// Session lifetime in seconds, from the declaration; absent takes the service default.
    session_ttl_seconds: Option<u32>,
}

impl GcpAgentPlatformSandbox {
    /// Builds a provider bound to one engine and template.
    ///
    /// The engine is normalised to its last path segment because the client builds the full
    /// resource path itself; passing the whole name would double it and address nothing.
    pub fn new(
        client: Arc<dyn AgentPlatformApi>,
        engine: String,
        template: String,
        session_ttl_seconds: Option<u32>,
    ) -> Self {
        let engine = engine.rsplit('/').next().unwrap_or(&engine).to_string();
        Self {
            client,
            engine,
            template,
            session_ttl_seconds,
        }
    }

    /// The engine id sent to the client. Exists so a test can prove the binding's full resource
    /// name was reduced to a bare segment — a doubled path is invisible against the mock otherwise.
    #[cfg(test)]
    pub(crate) fn engine(&self) -> &str {
        &self.engine
    }

    fn unsupported(&self, capability: &str, reason: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: capability.to_string(),
            reason: reason.to_string(),
        })
    }

    /// A session id that stays a single path segment.
    ///
    /// The id is interpolated into the proxy URL, so one carrying `/`, `..`, `?` or `#` would
    /// address a different sandbox — a resource the same engine grant can reach. The API mints
    /// these; this bounds the ones a caller hands back.
    fn checked_session_id(operation: &str, session_id: &str) -> Result<()> {
        if is_addressable_id(session_id) {
            return Ok(());
        }
        Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: operation.to_string(),
            details: format!(
                "session id '{session_id}' must be a single segment of letters, digits, '-' and \
                 '_', at most {MAX_SESSION_ID} characters"
            ),
            field_name: Some("sessionId".to_string()),
        }))
    }

    /// Reads a sandbox, or `None` when it is gone, without judging it.
    async fn read_sandbox(
        &self,
        operation: &str,
        session_id: &str,
    ) -> Result<Option<SandboxEnvironment>> {
        match self.client.get_sandbox(&self.engine, session_id).await {
            Ok(sandbox) => Ok(Some(sandbox)),
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(error.context(ErrorData::SandboxUnreachable {
                operation: operation.to_string(),
                reason: "the Agent Platform API did not answer a sandbox read".to_string(),
            })),
        }
    }

    /// Polls a lifecycle operation to completion, returning its response payload.
    ///
    /// Bounded rather than open-ended: a caller waiting forever is its own outage, and the
    /// operation name is carried so an incomplete one can be resumed rather than lost.
    async fn await_operation(
        &self,
        operation: &str,
        started: Operation,
    ) -> Result<serde_json::Value> {
        let Some(name) = started.name.clone() else {
            return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: operation.to_string(),
                field: "name".to_string(),
                response_json: "the operation carried no resource name to poll".to_string(),
            }));
        };

        let mut current = started;
        for _ in 0..OPERATION_POLL_ATTEMPTS {
            if current.done == Some(true) {
                return finish_operation(operation, &name, current);
            }
            tokio::time::sleep(OPERATION_POLL_INTERVAL).await;
            current = self
                .client
                .get_operation(&name)
                .await
                .context(ErrorData::SandboxUnreachable {
                    operation: operation.to_string(),
                    reason: format!("could not read operation '{name}'"),
                })?;
        }

        if current.done == Some(true) {
            return finish_operation(operation, &name, current);
        }
        Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: operation.to_string(),
            reason: format!("operation '{name}' did not complete within its polling budget"),
        }))
    }

    /// Sends one envelope through the `:execute` proxy and returns the agent's body verbatim.
    ///
    /// A client error is a transport failure — the proxy could not deliver or the API refused. A
    /// body the op's parser cannot read is the agent's own reason, handled by each verb. A
    /// not-found is reported as a gone session so a caller does not read it as a live one.
    async fn execute_op(
        &self,
        session_id: &str,
        operation: &str,
        envelope: serde_json::Value,
    ) -> Result<Vec<u8>> {
        let body = serde_json::to_vec(&envelope).map_err(|error| {
            AlienError::new(ErrorData::SerializationFailed {
                message: format!("could not encode the {operation} envelope: {error}"),
            })
        })?;

        self.client
            .execute(&self.engine, session_id, &body)
            .await
            .map_err(|error| Self::execute_failed(operation, error))
    }

    fn execute_failed(
        operation: &str,
        error: AlienError<AgentPlatformErrorData>,
    ) -> AlienError<ErrorData> {
        if is_not_found(&error) {
            return error.context(ErrorData::SandboxCommandFailed {
                failure: "sessionGone".to_string(),
                reason: format!("{operation}: the session does not exist"),
            });
        }
        // Non-retryable across the board: a `:execute` is single-attempt because it may already
        // have started the command, and the client does not tell a delivered-but-failed call apart
        // from an undelivered one. The cause stays on the chain rather than in `reason`, keeping a
        // redacted request body out of an externally visible message.
        error.context(ErrorData::SandboxCommandFailed {
            failure: "executeFailed".to_string(),
            reason: format!("{operation} could not be completed against the session"),
        })
    }

    /// Confirms the agent answers and speaks the protocol, and returns the session's generation.
    ///
    /// A sandbox can report `STATE_RUNNING` while every `:execute` fails, so a state read is not a
    /// health check; the agent has to answer for the session to be usable. The reply carries the
    /// container boot id, from which the generation is derived so a caller can detect a container
    /// that was replaced under a stable session name.
    async fn probe_agent(&self, operation: &str, session_id: &str) -> Result<u64> {
        // Mapped to unreachable whatever the failure — a refused delivery, a probe that outran its
        // budget, an unparseable body, a protocol mismatch — because a health probe is idempotent
        // and the caller acts on the same thing each way: the agent cannot be reached, so
        // `get_or_create` provisions a fresh one rather than destroying a session it did not create.
        let unreachable = |reason: String| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: operation.to_string(),
                reason,
            })
        };

        let body = tokio::time::timeout(
            AGENT_PROBE_BUDGET,
            self.client.execute(
                &self.engine,
                session_id,
                &serde_json::to_vec(&json!({ "v": AGENT_PROTOCOL_VERSION, "op": "health" }))
                    .unwrap_or_default(),
            ),
        )
        .await
        .map_err(|_| {
            unreachable(format!(
                "the session's agent did not answer a health probe within {}s",
                AGENT_PROBE_BUDGET.as_secs()
            ))
        })?
        .map_err(|error| {
            error.context(ErrorData::SandboxUnreachable {
                operation: operation.to_string(),
                reason: "the session's agent did not answer a health probe".to_string(),
            })
        })?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Health {
            protocol_version: u32,
            boot_id: String,
        }

        let health: Health = serde_json::from_slice(&body).map_err(|_| {
            unreachable(format!(
                "the session's agent answered a health probe with a body this provider cannot \
                 read: {}",
                truncated(&body)
            ))
        })?;

        if health.protocol_version != AGENT_PROTOCOL_VERSION {
            return Err(unreachable(format!(
                "the session's agent speaks protocol {} where this provider speaks {}",
                health.protocol_version, AGENT_PROTOCOL_VERSION
            )));
        }
        // An agent that answers without a boot id cannot be told apart from a replaced container,
        // so the session is refused rather than reconnected to a possibly-blank one.
        if health.boot_id.is_empty() {
            return Err(unreachable(
                "the session's agent reported no container boot id, so its identity cannot be \
                 established"
                    .to_string(),
            ));
        }
        Ok(generation_from_boot_id(&health.boot_id))
    }

    /// Deletes a sandbox the caller will never receive, keeping the reason it is discarded.
    ///
    /// Every failure after the sandbox exists reaches here, so `create` has one delete rather than
    /// one beside each `?`. The delete's own failure names the leak without replacing the finding
    /// that caused it. A not-found delete is already success in the client.
    async fn discard(&self, session_id: &str, reason: AlienError<ErrorData>) -> AlienError<ErrorData> {
        let Err(error) = self.client.delete_sandbox(&self.engine, session_id).await else {
            return reason;
        };
        warn!(
            session = %session_id,
            %error,
            "could not delete a sandbox that was never handed to its caller"
        );
        reason.context(ErrorData::SandboxCommandFailed {
            failure: "sandboxLeftBehind".to_string(),
            reason: format!(
                "session '{session_id}' was not handed to its caller and could not be deleted, so \
                 it is still running"
            ),
        })
    }

    /// Waits for a created sandbox to reach `STATE_RUNNING`, confirms its agent answers, and returns
    /// the session's generation.
    ///
    /// The running record is judged, not the create accept: a sandbox still coming up need not be
    /// addressable yet, and reading that as a failure would delete every one that answered early.
    async fn settle(&self, session_id: &str) -> Result<u64> {
        for _ in 0..SESSION_READY_ATTEMPTS {
            let Some(sandbox) = self.read_sandbox(CREATE, session_id).await? else {
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "sessionGone".to_string(),
                    reason: format!(
                        "session '{session_id}' disappeared while it was coming up"
                    ),
                }));
            };
            match session_state(CREATE, sandbox.state.as_deref())? {
                SandboxSessionState::Running => {
                    return self.probe_agent(CREATE, session_id).await;
                }
                SandboxSessionState::Terminated => {
                    return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "sessionTerminated".to_string(),
                        reason: format!("session '{session_id}' reached a terminal state while starting"),
                    }));
                }
                // Waited on rather than woken: a fresh sandbox has no idle-suspend policy to pause
                // it before its first command — the binding carries no such field — so a suspended
                // reading here is a transient step on the way up, not a resting state to resume.
                SandboxSessionState::Starting | SandboxSessionState::Suspended => {}
            }
            tokio::time::sleep(SESSION_READY_INTERVAL).await;
        }
        Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: CREATE.to_string(),
            reason: format!(
                "session '{session_id}' was not running after {}s",
                SESSION_READY_ATTEMPTS as u64 * SESSION_READY_INTERVAL.as_secs()
            ),
        }))
    }

    /// Runs a command inside the proxy's synchronous window, streaming the buffered NDJSON body.
    async fn run_synchronous(
        &self,
        session_id: &str,
        request: &RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        let envelope = exec_envelope("exec", session_id, request);
        let body = self.execute_op(session_id, RUN_COMMAND, envelope).await?;
        let frames = parse_exec_frames(&body)?;
        Ok(Box::pin(stream::iter(frames)))
    }

    /// Runs a command as a detached job whose output is polled for until it ends.
    async fn run_detached(
        &self,
        session_id: &str,
        request: &RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        let envelope = exec_envelope("jobStart", session_id, request);
        let body = self.execute_op(session_id, RUN_COMMAND, envelope).await?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct JobStart {
            job_id: String,
        }
        let started: JobStart = serde_json::from_slice(&body).map_err(|_| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: RUN_COMMAND.to_string(),
                field: "jobId".to_string(),
                response_json: truncated(&body),
            })
        })?;

        let state = JobPollState {
            client: self.client.clone(),
            engine: self.engine.clone(),
            session_id: session_id.to_string(),
            job_id: started.job_id,
            since_seq: None,
            pending: VecDeque::new(),
            finished: false,
            deadline_at: tokio::time::Instant::now() + request.deadline + JOB_POLL_GRACE,
        };

        Ok(Box::pin(stream::unfold(state, job_poll_step)))
    }
}

impl Binding for GcpAgentPlatformSandbox {}

#[async_trait]
impl Sandbox for GcpAgentPlatformSandbox {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::gcp_agent_platform()
    }

    async fn create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        // A session inherits no per-session environment: `SandboxCreateRequest` has no env field,
        // so silently dropping one would run the caller's code without the variables it asked for.
        // They travel per command through `run_command` instead.
        if !request.env.is_empty() {
            return Err(AlienError::new(ErrorData::InvalidInput {
                operation_context: CREATE.to_string(),
                details: "Agent Platform carries no per-session environment; pass variables on \
                          each command instead"
                    .to_string(),
                field_name: Some("env".to_string()),
            }));
        }

        let started = self
            .client
            .create_sandbox(
                &self.engine,
                SandboxCreateRequest {
                    display_name: request.session_id.clone(),
                    sandbox_environment_template: Some(self.template.clone()),
                    sandbox_environment_snapshot: None,
                    ttl: self.session_ttl_seconds.map(|seconds| format!("{seconds}s")),
                },
            )
            .await
            .context(ErrorData::SandboxUnreachable {
                operation: CREATE.to_string(),
                reason: "the Agent Platform API refused a sandbox create".to_string(),
            })?;

        let created: SandboxEnvironment = serde_json::from_value(self.await_operation(CREATE, started).await?)
            .map_err(|error| {
                AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp-agent-platform".to_string(),
                    binding_name: CREATE.to_string(),
                    field: "response".to_string(),
                    response_json: format!("the create operation resolved to a non-sandbox: {error}"),
                })
            })?;

        // The caller's requested id is not authoritative — the API allocates the name, and the
        // last segment is the id every later verb addresses it by. One this client cannot send is
        // one nothing can reach or reap, so an unreadable name is reported without a delete it
        // cannot target.
        let Some(session_id) = created.name.as_deref().and_then(session_segment) else {
            return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: CREATE.to_string(),
                field: "name".to_string(),
                response_json: format!("{:?}", created.name),
            }));
        };
        let session_id = session_id.to_string();

        // Past here a sandbox exists the caller has no id for, so every failure deletes it.
        match self.settle(&session_id).await {
            Ok(generation) => Ok(SandboxSession {
                session_id,
                state: SandboxSessionState::Running,
                generation,
            }),
            Err(error) => Err(self.discard(&session_id, error).await),
        }
    }

    async fn get(&self, session_id: &str) -> Result<Option<SandboxSession>> {
        Self::checked_session_id(GET, session_id)?;
        let Some(sandbox) = self.read_sandbox(GET, session_id).await? else {
            return Ok(None);
        };

        let state = session_state(GET, sandbox.state.as_deref())?;
        // Only a running session carries a reachable agent, and a state read is not health: a
        // running record whose agent does not answer is not reported as usable. A non-running
        // session has no live container to identify, so it carries no generation.
        let generation = if state == SandboxSessionState::Running {
            self.probe_agent(GET, session_id).await?
        } else {
            NO_GENERATION
        };

        Ok(Some(SandboxSession {
            session_id: session_id.to_string(),
            state,
            generation,
        }))
    }

    async fn get_or_create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        if let Some(id) = request.session_id.as_deref() {
            // A running, reachable session is handed back; anything else is served by a fresh
            // session rather than by destroying one this call did not create, which may be
            // another revision's.
            match self.get(id).await {
                Ok(Some(session)) if session.state == SandboxSessionState::Running => {
                    return Ok(session)
                }
                // The ordinary resting state for a reconnect: a suspended session is woken and
                // confirmed, and handed back if it comes up healthy. A wake this call made that
                // cannot be confirmed is put back to sleep before a fresh session is provisioned —
                // the paused one may be another revision's, and a second live sandbox beside it is
                // a leak the caller never receives an id for.
                Ok(Some(session)) if session.state == SandboxSessionState::Suspended => {
                    if self.resume(id).await.is_ok() {
                        match self.get(id).await {
                            Ok(Some(woken)) if woken.state == SandboxSessionState::Running => {
                                return Ok(woken)
                            }
                            _ => {
                                if let Err(error) = self.suspend(id).await {
                                    warn!(session = %id, %error, "could not re-suspend a session this call woke");
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(error) if error.code == "SANDBOX_UNREACHABLE" => {}
                Err(error) => {
                    return Err(error.context(ErrorData::SandboxCommandFailed {
                        failure: "getOrCreateFailed".to_string(),
                        reason: format!("{GET_OR_CREATE}: reaching session '{id}' failed"),
                    }))
                }
            }
        }

        self.create(request).await
    }

    async fn list(&self) -> Result<Vec<SandboxSession>> {
        let sandboxes = self
            .client
            .list_sandboxes(&self.engine)
            .await
            .context(ErrorData::SandboxUnreachable {
                operation: "sandbox.list".to_string(),
                reason: "the Agent Platform API did not answer a sandbox list".to_string(),
            })?;

        // A sandbox this provider cannot fully read — an unaddressable name or an unrecognised
        // state — is left out rather than surfaced as a handle to nothing or failing the whole
        // enumeration; one odd sandbox must not hide every other from an orphan sweep. Both halves
        // are skipped for the same reason, so leniency is consistent across the record.
        Ok(sandboxes
            .into_iter()
            .filter_map(|sandbox| {
                let session_id = sandbox.name.as_deref().and_then(session_segment)?;
                let state = session_state("sandbox.list", sandbox.state.as_deref()).ok()?;
                // A bulk list does not probe each agent, so it reports no generation; a caller that
                // needs one reads the single session through `get`.
                Some(SandboxSession {
                    session_id: session_id.to_string(),
                    state,
                    generation: NO_GENERATION,
                })
            })
            .collect())
    }

    async fn run_command(
        &self,
        session_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        Self::checked_session_id(RUN_COMMAND, session_id)?;
        if request.command.is_empty() {
            return Err(AlienError::new(ErrorData::InvalidInput {
                operation_context: RUN_COMMAND.to_string(),
                details: "a command must name a program to run".to_string(),
                field_name: Some("command".to_string()),
            }));
        }
        // Refused rather than defaulted, and refused where it floors to zero milliseconds too: the
        // agent rejects a `deadlineMs` of 0, and a defaulted deadline is a hang waiting for a slow
        // day in a session running code the caller does not control.
        if deadline_millis(request.deadline) == 0 {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "invalidRequest".to_string(),
                reason: "a command must carry a deadline of at least one millisecond".to_string(),
            }));
        }

        // The synchronous window is the proxy's, not the command's: a command that outlives one
        // `:execute` is detached as a job so a later poll can still reach its output.
        if request.deadline <= MAX_SYNCHRONOUS_DEADLINE {
            self.run_synchronous(session_id, &request).await
        } else {
            self.run_detached(session_id, &request).await
        }
    }

    async fn read_file(&self, session_id: &str, path: &str) -> Result<Vec<u8>> {
        Self::checked_session_id("sandbox.readFile", session_id)?;
        let body = self
            .execute_op(
                session_id,
                "sandbox.readFile",
                json!({ "v": AGENT_PROTOCOL_VERSION, "op": "readFile", "path": path }),
            )
            .await?;

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ReadFile {
            contents_base64: String,
        }
        let read: ReadFile = serde_json::from_slice(&body).map_err(|_| {
            AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "agentRefused".to_string(),
                reason: format!("sandbox.readFile was refused: {}", truncated(&body)),
            })
        })?;

        BASE64
            .decode(read.contents_base64.as_bytes())
            .map_err(|error| {
                AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp-agent-platform".to_string(),
                    binding_name: "sandbox.readFile".to_string(),
                    field: "contentsBase64".to_string(),
                    response_json: format!("the agent returned data that is not base64: {error}"),
                })
            })
    }

    async fn write_files(&self, session_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        Self::checked_session_id("sandbox.writeFiles", session_id)?;
        // One request per path, stopping at the first failure — the partial application every
        // backend performs, so a caller sees one contract rather than several. The agent's field
        // is `contentsBase64`; `contents` is dropped silently.
        for (path, contents) in files {
            let body = self
                .execute_op(
                    session_id,
                    "sandbox.writeFiles",
                    json!({
                        "v": AGENT_PROTOCOL_VERSION,
                        "op": "writeFile",
                        "path": path,
                        "contentsBase64": BASE64.encode(&contents),
                    }),
                )
                .await?;
            confirm_empty_ok("sandbox.writeFiles", &body)?;
        }
        Ok(())
    }

    async fn mkdir(&self, session_id: &str, path: &str) -> Result<()> {
        Self::checked_session_id("sandbox.mkdir", session_id)?;
        let body = self
            .execute_op(
                session_id,
                "sandbox.mkdir",
                json!({ "v": AGENT_PROTOCOL_VERSION, "op": "mkdir", "path": path }),
            )
            .await?;
        confirm_empty_ok("sandbox.mkdir", &body)
    }

    async fn preview(&self, _session_id: &str, _port: u16) -> Result<PreviewCapability> {
        Err(self.unsupported(
            "preview",
            "Agent Platform mints no port-scoped ingress capability; the only ingress is :execute",
        ))
    }

    async fn suspend(&self, session_id: &str) -> Result<()> {
        Self::checked_session_id("sandbox.suspend", session_id)?;
        let started = self
            .client
            .pause(&self.engine, session_id)
            .await
            .context(ErrorData::SandboxCommandFailed {
                failure: "suspendFailed".to_string(),
                reason: format!("sandbox.suspend: session '{session_id}' could not be paused"),
            })?;
        self.await_operation("sandbox.suspend", started).await?;
        Ok(())
    }

    async fn resume(&self, session_id: &str) -> Result<()> {
        Self::checked_session_id("sandbox.resume", session_id)?;
        let started = self
            .client
            .resume(&self.engine, session_id)
            .await
            .context(ErrorData::SandboxCommandFailed {
                failure: "resumeFailed".to_string(),
                reason: format!("sandbox.resume: session '{session_id}' could not be resumed"),
            })?;
        self.await_operation("sandbox.resume", started).await?;
        Ok(())
    }

    async fn snapshot(&self, session_id: &str) -> Result<String> {
        Self::checked_session_id("sandbox.snapshot", session_id)?;
        // A generated display name, because the API takes one and the caller does not supply it.
        // The trait has no restore verb, so the returned name is not yet consumable through it —
        // restore is `create` from a snapshot, which this backend can do but the trait cannot ask.
        let display_name = format!("snap-{}", uuid::Uuid::new_v4().simple());
        let started = self
            .client
            .snapshot(&self.engine, session_id, &display_name)
            .await
            .context(ErrorData::SandboxCommandFailed {
                failure: "snapshotFailed".to_string(),
                reason: format!("sandbox.snapshot: session '{session_id}' could not be captured"),
            })?;

        let snapshot: SandboxSnapshot = serde_json::from_value(
            self.await_operation("sandbox.snapshot", started).await?,
        )
        .map_err(|error| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: "sandbox.snapshot".to_string(),
                field: "response".to_string(),
                response_json: format!("the snapshot operation resolved to a non-snapshot: {error}"),
            })
        })?;

        snapshot.name.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: "sandbox.snapshot".to_string(),
                field: "name".to_string(),
                response_json: "the snapshot completed without a resource name".to_string(),
            })
        })
    }

    async fn terminate(&self, session_id: &str) -> Result<()> {
        Self::checked_session_id(TERMINATE, session_id)?;
        // Accepted, not completed: the client returns before the sandbox is gone. Returning here
        // would report containment while the code may still run, which is the whole point of
        // terminate — so the delete is confirmed by polling to not-found.
        self.client
            .delete_sandbox(&self.engine, session_id)
            .await
            .context(ErrorData::SandboxUnreachable {
                operation: TERMINATE.to_string(),
                reason: format!("the delete of session '{session_id}' was not accepted"),
            })?;

        for _ in 0..TERMINATE_POLL_ATTEMPTS {
            match self.client.get_sandbox(&self.engine, session_id).await {
                Err(error) if is_not_found(&error) => return Ok(()),
                // A read that fails is not a session that is gone, and one throttled response must
                // not end the poll: the attempt budget decides.
                Err(error) => {
                    warn!(session = %session_id, %error, "could not confirm a sandbox is gone")
                }
                Ok(_) => {}
            }
            tokio::time::sleep(TERMINATE_POLL_INTERVAL).await;
        }

        Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: TERMINATE.to_string(),
            reason: format!(
                "deletion of '{session_id}' was accepted but the session was still present after \
                 {}s; it may still be running",
                TERMINATE_POLL_ATTEMPTS as u64 * TERMINATE_POLL_INTERVAL.as_secs()
            ),
        }))
    }
}

/// One step of a detached job's poll loop, yielding output frames as they arrive and a terminal
/// item once the job ends.
async fn job_poll_step(
    mut state: JobPollState,
) -> Option<(Result<CommandOutput>, JobPollState)> {
    loop {
        if let Some(item) = state.pending.pop_front() {
            return Some((item, state));
        }
        if state.finished {
            return None;
        }

        if tokio::time::Instant::now() >= state.deadline_at {
            // Best-effort: the job is cancelled so its process group is killed, and the caller is
            // told the deadline was exceeded rather than left reading a stream that never ends.
            let _ = state
                .client
                .execute(
                    &state.engine,
                    &state.session_id,
                    &cancel_body(&state.job_id),
                )
                .await;
            state.pending.push_back(Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "deadlineExceeded".to_string(),
                reason: "the command's deadline elapsed before its job reported an outcome"
                    .to_string(),
            })));
            state.finished = true;
            continue;
        }

        let body = match state
            .client
            .execute(&state.engine, &state.session_id, &poll_body(&state.job_id, state.since_seq))
            .await
        {
            Ok(body) => body,
            Err(error) => {
                state.pending.push_back(Err(GcpAgentPlatformSandbox::execute_failed(
                    RUN_COMMAND,
                    error,
                )));
                state.finished = true;
                continue;
            }
        };

        let poll: JobPoll = match serde_json::from_slice(&body) {
            Ok(poll) => poll,
            Err(_) => {
                state.pending.push_back(Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp-agent-platform".to_string(),
                    binding_name: RUN_COMMAND.to_string(),
                    field: "jobPoll".to_string(),
                    response_json: truncated(&body),
                })));
                state.finished = true;
                continue;
            }
        };

        for frame in poll.frames {
            // A seq gap is truncated output, not a frame still to come, so the cursor takes the
            // highest seq seen and the loop never waits for a "missing" one; `max` rather than the
            // last frame's seq so an out-of-order frame cannot walk the cursor backwards.
            state.since_seq = state.since_seq.max(frame.seq());
            state.pending.push_back(frame.into_output());
        }

        if !poll.running {
            // The terminal outcome is the envelope's, not a frame's: a clean exit carries a code,
            // and a deadline, spawn failure or cancel carries an error object with no code.
            let terminal = match poll.error {
                Some(error) => Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: error.code,
                    reason: error.message,
                })),
                None => Ok(CommandOutput::Exit {
                    code: poll.exit_code.unwrap_or(-1),
                    truncated: poll.truncated.unwrap_or(false),
                }),
            };
            state.pending.push_back(terminal);
            state.finished = true;
            continue;
        }

        if state.pending.is_empty() {
            tokio::time::sleep(JOB_POLL_INTERVAL).await;
        }
    }
}

/// The bookkeeping a detached job's poll loop carries between steps.
struct JobPollState {
    client: Arc<dyn AgentPlatformApi>,
    engine: String,
    session_id: String,
    job_id: String,
    since_seq: Option<u64>,
    pending: VecDeque<Result<CommandOutput>>,
    finished: bool,
    deadline_at: tokio::time::Instant,
}

/// A job's output so far, and how it ended once it has. Mirrors the agent's `jobPoll` reply.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobPoll {
    running: bool,
    #[serde(default)]
    frames: Vec<WireFrame>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    truncated: Option<bool>,
    #[serde(default)]
    error: Option<JobError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobError {
    code: String,
    message: String,
}

/// A frame as the agent writes it, shared by the synchronous NDJSON body and the job frames.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "t")]
enum WireFrame {
    Stdout { seq: u64, data: String },
    Stderr { seq: u64, data: String },
    Exit {
        code: i32,
        #[serde(default)]
        truncated: bool,
    },
    Error { code: String, message: String },
}

impl WireFrame {
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Exit { .. } | Self::Error { .. })
    }

    fn seq(&self) -> Option<u64> {
        match self {
            Self::Stdout { seq, .. } | Self::Stderr { seq, .. } => Some(*seq),
            _ => None,
        }
    }

    fn into_output(self) -> Result<CommandOutput> {
        match self {
            Self::Stdout { seq, data } => Ok(CommandOutput::Stdout {
                seq,
                data: decode_frame_data(&data)?,
            }),
            Self::Stderr { seq, data } => Ok(CommandOutput::Stderr {
                seq,
                data: decode_frame_data(&data)?,
            }),
            Self::Exit { code, truncated } => Ok(CommandOutput::Exit { code, truncated }),
            // An error frame is the command's outcome, so it surfaces as an error rather than a
            // stream that simply stopped.
            Self::Error { code, message } => Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: code,
                reason: message,
            })),
        }
    }
}

fn decode_frame_data(data: &str) -> Result<Vec<u8>> {
    BASE64.decode(data).map_err(|error| {
        AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: RUN_COMMAND.to_string(),
            field: "data".to_string(),
            response_json: format!("an output frame's data is not base64: {error}"),
        })
    })
}

/// Turns the agent's buffered NDJSON body into output frames.
///
/// A body that is not frames at all is the agent's error, reported as a refusal. A body that ends
/// without a terminal frame is a transport failure, not a command that finished: the command had
/// started, so the trailing item says the outcome is unknown rather than letting a truncated
/// stream read as success.
fn parse_exec_frames(body: &[u8]) -> Result<Vec<Result<CommandOutput>>> {
    let mut frames = Vec::new();
    let mut saw_any = false;
    let mut saw_terminal = false;

    for line in body.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice::<WireFrame>(line) {
            Ok(frame) => {
                saw_any = true;
                saw_terminal |= frame.is_terminal();
                frames.push(frame.into_output());
            }
            Err(error) => {
                if !saw_any {
                    return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "agentRefused".to_string(),
                        reason: format!("run_command was refused: {}", truncated(body)),
                    }));
                }
                frames.push(Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp-agent-platform".to_string(),
                    binding_name: RUN_COMMAND.to_string(),
                    field: "frame".to_string(),
                    response_json: format!("an output frame did not parse: {error}"),
                })));
                saw_terminal = true;
                break;
            }
        }
    }

    if !saw_any {
        return Err(AlienError::new(ErrorData::SandboxCommandFailed {
            failure: "agentRefused".to_string(),
            reason: "run_command returned an empty body".to_string(),
        }));
    }
    if !saw_terminal {
        frames.push(Err(AlienError::new(ErrorData::SandboxCommandFailed {
            failure: "outcomeUnknown".to_string(),
            reason: "the command's output ended without a terminal frame, so whether it finished \
                     is unknown"
                .to_string(),
        })));
    }
    Ok(frames)
}

/// The envelope for `exec` or `jobStart`. `deadlineMs` is the field the agent reads; both ops take
/// the identical body.
fn exec_envelope(op: &str, _session_id: &str, request: &RunCommandRequest) -> serde_json::Value {
    json!({
        "v": AGENT_PROTOCOL_VERSION,
        "op": op,
        "command": request.command,
        "deadlineMs": deadline_millis(request.deadline),
        "workingDirectory": request.working_directory,
        "env": request.env,
    })
}

fn poll_body(job_id: &str, since_seq: Option<u64>) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "v": AGENT_PROTOCOL_VERSION,
        "op": "jobPoll",
        "jobId": job_id,
        "sinceSeq": since_seq,
    }))
    .unwrap_or_default()
}

fn cancel_body(job_id: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "v": AGENT_PROTOCOL_VERSION,
        "op": "jobCancel",
        "jobId": job_id,
    }))
    .unwrap_or_default()
}

/// Milliseconds, saturated: a deadline long enough to overflow `u64` ms is not one anyone meant,
/// and wrapping it would turn "effectively forever" into "immediately".
fn deadline_millis(deadline: Duration) -> u64 {
    u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX)
}

/// Reads a `writeFile`/`mkdir` reply, which succeeds with an empty body.
///
/// A non-empty body from these ops is the agent's error text, not a success shape, so it is
/// surfaced as a refusal rather than ignored.
fn confirm_empty_ok(operation: &str, body: &[u8]) -> Result<()> {
    if body.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::SandboxCommandFailed {
        failure: "agentRefused".to_string(),
        reason: format!("{operation} was refused: {}", truncated(body)),
    }))
}

/// The last path segment, if it is a usable id. Used for both minted names and listed ones.
fn session_segment(name: &str) -> Option<&str> {
    let segment = name.rsplit('/').next()?;
    is_addressable_id(segment).then_some(segment)
}

fn is_addressable_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_SESSION_ID
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Maps a container boot id to a numeric generation deterministically.
///
/// A caller may compare generations across processes, so this is an explicit FNV-1a rather than a
/// `Hash` impl — the same boot id must yield the same number in any build, and std's hashers
/// promise no cross-release stability. `| 1` keeps the result clear of `NO_GENERATION`.
fn generation_from_boot_id(boot_id: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in boot_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash | 1
}

/// The API's runtime states, in ours. An unrecognised one is an error rather than a default,
/// because every default here is a lie a caller acts on.
fn session_state(operation: &str, state: Option<&str>) -> Result<SandboxSessionState> {
    match state {
        Some("STATE_RUNNING") => Ok(SandboxSessionState::Running),
        Some("STATE_CREATING" | "STATE_PENDING" | "STATE_RESUMING") => {
            Ok(SandboxSessionState::Starting)
        }
        Some("STATE_PAUSED" | "STATE_PAUSING" | "STATE_SUSPENDED") => {
            Ok(SandboxSessionState::Suspended)
        }
        Some("STATE_STOPPED" | "STATE_FAILED" | "STATE_DELETING" | "STATE_DELETED") => {
            Ok(SandboxSessionState::Terminated)
        }
        other => Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: operation.to_string(),
            field: "state".to_string(),
            response_json: other.map_or_else(|| "absent".to_string(), |state| format!("\"{state}\"")),
        })),
    }
}

/// Turns a completed operation into its response payload, or the error it reported.
fn finish_operation(
    operation: &str,
    name: &str,
    op: Operation,
) -> Result<serde_json::Value> {
    match op.result {
        Some(OperationResult::Response { response }) => Ok(response),
        Some(OperationResult::Error { error }) => Err(AlienError::new(ErrorData::SandboxCommandFailed {
            failure: "operationFailed".to_string(),
            reason: format!(
                "{operation}: operation '{name}' failed (grpc {}): {}",
                error.code, error.message
            ),
        })),
        None => Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: operation.to_string(),
            field: "response".to_string(),
            response_json: format!("operation '{name}' reported done without a result"),
        })),
    }
}

/// Whether a client error means the sandbox is already gone.
///
/// The client wraps a 404 as `RequestFailed` and leaves the `RemoteResourceNotFound` on the
/// source chain, so the classification is read by walking that chain rather than off the outer
/// variant — a path or trace id mentioning 404 in a message never reaches this.
fn is_not_found(error: &AlienError<AgentPlatformErrorData>) -> bool {
    const NOT_FOUND: &str = "REMOTE_RESOURCE_NOT_FOUND";
    if error.code == NOT_FOUND {
        return true;
    }
    let mut node = error.source.as_deref();
    while let Some(current) = node {
        if current.code == NOT_FOUND {
            return true;
        }
        node = current.source.as_deref();
    }
    false
}

/// A body short enough to sit in an error message without carrying a whole response into it.
fn truncated(body: &[u8]) -> String {
    const LIMIT: usize = 200;
    let text = String::from_utf8_lossy(body);
    let text = text.trim();
    if text.len() <= LIMIT {
        return text.to_string();
    }
    let end = (0..=LIMIT).rev().find(|at| text.is_char_boundary(*at)).unwrap_or(0);
    format!("{}…", &text[..end])
}

#[cfg(test)]
#[path = "gcp_agent_platform_tests.rs"]
mod tests;
