//! GCP Agent Platform sandbox provider.
//!
//! Sandboxes are `sandboxEnvironments` created under a durable reasoning engine and reached from
//! outside the guest through the `:execute` proxy, which forwards one request to the agent's
//! `POST /` envelope and returns its body verbatim. So every command, file operation and health
//! check is one envelope over that proxy, and the lifecycle verbs are long-running operations
//! polled to completion.

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
    Binding, CommandOutput, CreateSandboxRequest, JobError, JobExit, JobPoll, JobStart,
    PreviewCapability, ResolvedSandbox, RunCommandRequest, Sandbox, SandboxInstance, SandboxState,
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
const MAX_SYNCHRONOUS_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest sandbox id this provider will place in a proxy URL. A bound on what is handed back to a
/// caller, not on what the API mints — the names seen are far shorter.
const MAX_SANDBOX_ID: usize = 63;

/// How long a created sandbox has to reach `STATE_RUNNING`, and how often that is checked.
const SANDBOX_READY_ATTEMPTS: u32 = 150;
const SANDBOX_READY_INTERVAL: Duration = Duration::from_secs(2);

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
const JOB_START: &str = "sandbox.jobStart";
const JOB_POLL: &str = "sandbox.jobPoll";
const JOB_CANCEL: &str = "sandbox.jobCancel";
const TERMINATE: &str = "sandbox.terminate";

/// The generation of a sandbox whose live container identity was not established: a state with no
/// reachable agent, or a bulk `list` that does not probe each sandbox. Never a value
/// `generation_from_boot_id` returns, so a real identity is always distinguishable from an
/// unprobed one.
const NO_GENERATION: u64 = 0;

/// A single health probe is bounded to this, because the client sets no per-request timeout and an
/// agent that accepts the connection but never answers would otherwise hang `get()` and `create()`
/// forever. Set above the proxy's ~30s synchronous window (see `MAX_SYNCHRONOUS_TIMEOUT`) rather
/// than tight to the round trip: too tight reports a healthy sandbox unreachable, and
/// `get_or_create` then provisions a fresh sandbox and loses the caller's filesystem — the failure
/// this task exists to prevent — where too loose only delays an already-broken sandbox.
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
    /// Template every sandbox is cut from, as a resource name the create body carries unchanged.
    template: String,
    /// Sandbox lifetime in seconds, from the declaration; absent takes the service default.
    max_lifetime_seconds: Option<u32>,
}

impl GcpAgentPlatformSandbox {
    /// The `ttl` a create is sent with: what the caller asked for, never above what the
    /// declaration allows.
    fn lifetime_seconds(&self, timeout_ms: Option<u64>, operation: &str) -> Result<Option<u32>> {
        match timeout_ms {
            Some(timeout_ms) => {
                super::requested_lifetime_seconds(timeout_ms, self.max_lifetime_seconds, operation)
                    .map(Some)
            }
            None => Ok(self.max_lifetime_seconds),
        }
    }

    /// Builds a provider bound to one engine and template.
    ///
    /// The engine is normalised to its last path segment because the client builds the full
    /// resource path itself; passing the whole name would double it and address nothing.
    pub fn new(
        client: Arc<dyn AgentPlatformApi>,
        engine: String,
        template: String,
        max_lifetime_seconds: Option<u32>,
    ) -> Self {
        let engine = engine.rsplit('/').next().unwrap_or(&engine).to_string();
        Self {
            client,
            engine,
            template,
            max_lifetime_seconds,
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

    /// A sandbox id that stays a single path segment.
    ///
    /// The id is interpolated into the proxy URL, so one carrying `/`, `..`, `?` or `#` would
    /// address a different sandbox — a resource the same engine grant can reach. The API mints
    /// these; this bounds the ones a caller hands back.
    fn checked_sandbox_id(operation: &str, sandbox_id: &str) -> Result<()> {
        if is_addressable_id(sandbox_id) {
            return Ok(());
        }
        Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: operation.to_string(),
            details: format!(
                "sandbox id '{sandbox_id}' must be a single segment of letters, digits, '-' and \
                 '_', at most {MAX_SANDBOX_ID} characters"
            ),
            field_name: Some("sandboxId".to_string()),
        }))
    }

    /// Reads a sandbox, or `None` when it is gone, without judging it.
    async fn read_sandbox(
        &self,
        operation: &str,
        sandbox_id: &str,
    ) -> Result<Option<SandboxEnvironment>> {
        match self.client.get_sandbox(&self.engine, sandbox_id).await {
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
            current =
                self.client
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
    /// not-found is reported as a gone sandbox so a caller does not read it as a live one.
    async fn execute_op(
        &self,
        sandbox_id: &str,
        operation: &str,
        envelope: serde_json::Value,
    ) -> Result<Vec<u8>> {
        let body = serde_json::to_vec(&envelope).map_err(|error| {
            AlienError::new(ErrorData::SerializationFailed {
                message: format!("could not encode the {operation} envelope: {error}"),
            })
        })?;

        self.client
            .execute(&self.engine, sandbox_id, &body)
            .await
            .map_err(|error| Self::execute_failed(operation, error))
    }

    fn execute_failed(
        operation: &str,
        error: AlienError<AgentPlatformErrorData>,
    ) -> AlienError<ErrorData> {
        if is_not_found(&error) {
            return error.context(ErrorData::SandboxCommandFailed {
                failure: "sandboxGone".to_string(),
                reason: format!("{operation}: the sandbox does not exist"),
            });
        }
        // The client does not tell a delivered-but-failed call apart from an undelivered one, so
        // a `:execute` carrying a command leaves its outcome unestablished. The cause stays on the
        // chain rather than in `reason`, keeping a redacted request body out of an externally
        // visible message.
        if operation == RUN_COMMAND || operation == JOB_START {
            return error.context(ErrorData::SandboxOutcomeUnknown {
                operation: operation.to_string(),
                reason: "the sandbox did not complete the call".to_string(),
            });
        }
        error.context(ErrorData::SandboxCommandFailed {
            failure: "executeFailed".to_string(),
            reason: format!("{operation} could not be completed against the sandbox"),
        })
    }

    /// Confirms the agent answers and speaks the protocol, and returns the sandbox's generation.
    ///
    /// A sandbox can report `STATE_RUNNING` while every `:execute` fails, so a state read is not a
    /// health check; the agent has to answer for the sandbox to be usable. The reply carries the
    /// container boot id, from which the generation is derived so a caller can detect a container
    /// that was replaced under a stable sandbox name.
    async fn probe_agent(&self, operation: &str, sandbox_id: &str) -> Result<u64> {
        // Mapped to unreachable whatever the failure — a refused delivery, a probe that outran its
        // budget, an unparseable body, a protocol mismatch — because a health probe is idempotent
        // and the caller acts on the same thing each way: the agent cannot be reached, so
        // `get_or_create` provisions a fresh one rather than destroying a sandbox it did not create.
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
                sandbox_id,
                &serde_json::to_vec(&json!({ "v": AGENT_PROTOCOL_VERSION, "op": "health" }))
                    .unwrap_or_default(),
            ),
        )
        .await
        .map_err(|_| {
            unreachable(format!(
                "the sandbox's agent did not answer a health probe within {}s",
                AGENT_PROBE_BUDGET.as_secs()
            ))
        })?
        .map_err(|error| {
            error.context(ErrorData::SandboxUnreachable {
                operation: operation.to_string(),
                reason: "the sandbox's agent did not answer a health probe".to_string(),
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
                "the sandbox's agent answered a health probe with a body this provider cannot \
                 read: {}",
                truncated(&body)
            ))
        })?;

        if health.protocol_version != AGENT_PROTOCOL_VERSION {
            return Err(unreachable(format!(
                "the sandbox's agent speaks protocol {} where this provider speaks {}",
                health.protocol_version, AGENT_PROTOCOL_VERSION
            )));
        }
        // An agent that answers without a boot id cannot be told apart from a replaced container,
        // so the sandbox is refused rather than reconnected to a possibly-blank one.
        if health.boot_id.is_empty() {
            return Err(unreachable(
                "the sandbox's agent reported no container boot id, so its identity cannot be \
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
    async fn discard(
        &self,
        sandbox_id: &str,
        reason: AlienError<ErrorData>,
    ) -> AlienError<ErrorData> {
        let Err(error) = self.client.delete_sandbox(&self.engine, sandbox_id).await else {
            return reason;
        };
        warn!(
            sandbox = %sandbox_id,
            %error,
            "could not delete a sandbox that was never handed to its caller"
        );
        reason.context(ErrorData::SandboxCommandFailed {
            failure: "sandboxLeftBehind".to_string(),
            reason: format!(
                "sandbox '{sandbox_id}' was not handed to its caller and could not be deleted, so \
                 it is still running"
            ),
        })
    }

    /// Waits for a created sandbox to reach `STATE_RUNNING`, confirms its agent answers, and returns
    /// the sandbox's generation.
    ///
    /// The running record is judged, not the create accept: a sandbox still coming up need not be
    /// addressable yet, and reading that as a failure would delete every one that answered early.
    async fn settle(&self, sandbox_id: &str) -> Result<u64> {
        for _ in 0..SANDBOX_READY_ATTEMPTS {
            let Some(sandbox) = self.read_sandbox(CREATE, sandbox_id).await? else {
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "sandboxGone".to_string(),
                    reason: format!("sandbox '{sandbox_id}' disappeared while it was coming up"),
                }));
            };
            match sandbox_state(CREATE, sandbox.state.as_deref())? {
                SandboxState::Running => {
                    return self.probe_agent(CREATE, sandbox_id).await;
                }
                SandboxState::Terminated => {
                    return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "sandboxTerminated".to_string(),
                        reason: format!(
                            "sandbox '{sandbox_id}' reached a terminal state while starting"
                        ),
                    }));
                }
                // Waited on rather than woken: a fresh sandbox has no idle-suspend policy to pause
                // it before its first command — the binding carries no such field — so a suspended
                // reading here is a transient step on the way up, not a resting state to resume.
                SandboxState::Starting | SandboxState::Paused => {}
            }
            tokio::time::sleep(SANDBOX_READY_INTERVAL).await;
        }
        Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: CREATE.to_string(),
            reason: format!(
                "sandbox '{sandbox_id}' was not running after {}s",
                SANDBOX_READY_ATTEMPTS as u64 * SANDBOX_READY_INTERVAL.as_secs()
            ),
        }))
    }

    /// Runs a command inside the proxy's synchronous window, streaming the buffered NDJSON body.
    async fn run_synchronous(
        &self,
        sandbox_id: &str,
        request: &RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        let envelope = exec_envelope("exec", sandbox_id, request);
        let body = self.execute_op(sandbox_id, RUN_COMMAND, envelope).await?;
        let frames = parse_exec_frames(&body)?;
        Ok(Box::pin(stream::iter(frames)))
    }

    /// Runs a command as a detached job whose output is polled for until it ends.
    async fn run_detached(
        &self,
        sandbox_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        let timeout = request.timeout;
        let started = self.start_job(sandbox_id, request).await?;

        let state = JobPollState {
            client: self.client.clone(),
            engine: self.engine.clone(),
            sandbox_id: sandbox_id.to_string(),
            job_id: started.job_id,
            since_seq: None,
            pending: VecDeque::new(),
            finished: false,
            deadline_at: tokio::time::Instant::now() + timeout + JOB_POLL_GRACE,
        };

        Ok(Box::pin(stream::unfold(state, job_poll_step)))
    }

    /// Refuses a command the agent would refuse anyway, before a call is spent on it.
    fn checked_command(operation: &str, request: &RunCommandRequest) -> Result<()> {
        if request.command.is_empty() {
            return Err(AlienError::new(ErrorData::InvalidInput {
                operation_context: operation.to_string(),
                details: "a command must name a program to run".to_string(),
                field_name: Some("command".to_string()),
            }));
        }
        // Refused rather than defaulted, and refused where it floors to zero milliseconds too: the
        // agent rejects a `timeoutMs` of 0, and a defaulted timeout is a hang waiting for a slow
        // day in a sandbox running code the caller does not control.
        if timeout_millis(request.timeout) == 0 {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "invalidRequest".to_string(),
                reason: "a command must carry a timeout of at least one millisecond".to_string(),
            }));
        }
        Ok(())
    }
}

impl Binding for GcpAgentPlatformSandbox {}

#[async_trait]
impl Sandbox for GcpAgentPlatformSandbox {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// The platform's row, unnarrowed. `sandboxLifetime` stays true even with no declared ttl:
    /// the API always sets `expireTime` on output, so an undeclared sandbox still carries a
    /// deadline the platform enforces.
    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::gcp_agent_platform()
    }

    async fn create(&self, request: CreateSandboxRequest) -> Result<SandboxInstance> {
        // A sandbox takes no environment of its own: `SandboxCreateRequest` has no env field,
        // so silently dropping one would run the caller's code without the variables it asked for.
        // They travel per command through `run_command` instead.
        // `OperationNotSupported`, not `InvalidInput`: the value is fine, the backend has nowhere
        // to put it. AWS answers the identical condition the same way, and a portable caller
        // branching on the code must not get two answers for one situation.
        if !request.env.is_empty() {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: CREATE.to_string(),
                reason: "Agent Platform sandboxes take no sandbox-level env; set env per command \
                         instead"
                    .to_string(),
            }));
        }

        // Same reason as `env` above: nowhere to carry a tenant key, so accepting one would
        // silently merge tenants into one sandbox.
        if request.tenant_key.is_some() {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: CREATE.to_string(),
                reason: "Agent Platform sandboxes take no tenantKey; create one sandbox per \
                         tenant instead"
                    .to_string(),
            }));
        }

        let ttl = self
            .lifetime_seconds(request.timeout_ms, CREATE)?
            .map(|seconds| format!("{seconds}s"));

        let started = self
            .client
            .create_sandbox(
                &self.engine,
                SandboxCreateRequest {
                    display_name: request.sandbox_id.clone(),
                    sandbox_environment_template: Some(self.template.clone()),
                    sandbox_environment_snapshot: None,
                    ttl,
                },
            )
            .await
            .context(ErrorData::SandboxUnreachable {
                operation: CREATE.to_string(),
                reason: "the Agent Platform API refused a sandbox create".to_string(),
            })?;

        let created: SandboxEnvironment = serde_json::from_value(
            self.await_operation(CREATE, started).await?,
        )
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
        let Some(sandbox_id) = created.name.as_deref().and_then(sandbox_segment) else {
            return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: CREATE.to_string(),
                field: "name".to_string(),
                response_json: format!("{:?}", created.name),
            }));
        };
        let sandbox_id = sandbox_id.to_string();

        // Past here a sandbox exists the caller has no id for, so every failure deletes it.
        match self.settle(&sandbox_id).await {
            Ok(generation) => Ok(SandboxInstance {
                sandbox_id,
                state: SandboxState::Running,
                generation,
            }),
            Err(error) => Err(self.discard(&sandbox_id, error).await),
        }
    }

    async fn get(&self, sandbox_id: &str) -> Result<Option<SandboxInstance>> {
        Self::checked_sandbox_id(GET, sandbox_id)?;
        let Some(sandbox) = self.read_sandbox(GET, sandbox_id).await? else {
            return Ok(None);
        };

        let state = sandbox_state(GET, sandbox.state.as_deref())?;
        // Only a running sandbox carries a reachable agent, and a state read is not health: a
        // running record whose agent does not answer is not reported as usable. A non-running
        // sandbox has no live container to identify, so it carries no generation.
        let generation = if state == SandboxState::Running {
            self.probe_agent(GET, sandbox_id).await?
        } else {
            NO_GENERATION
        };

        Ok(Some(SandboxInstance {
            sandbox_id: sandbox_id.to_string(),
            state,
            generation,
        }))
    }

    async fn get_or_create(&self, request: CreateSandboxRequest) -> Result<ResolvedSandbox> {
        if let Some(id) = request.sandbox_id.as_deref() {
            // A running, reachable sandbox is handed back; anything else is served by a fresh
            // sandbox rather than by destroying one this call did not create, which may be
            // another revision's.
            match self.get(id).await {
                Ok(Some(sandbox)) if sandbox.state == SandboxState::Running => {
                    return Ok(ResolvedSandbox::found(sandbox))
                }
                // The ordinary resting state for a reconnect: a suspended sandbox is woken and
                // confirmed, and handed back if it comes up healthy. A wake this call made that
                // cannot be confirmed is put back to sleep before a fresh sandbox is provisioned —
                // the paused one may be another revision's, and a second live sandbox beside it is
                // a leak the caller never receives an id for.
                Ok(Some(sandbox)) if sandbox.state == SandboxState::Paused => {
                    if self.resume(id).await.is_ok() {
                        match self.get(id).await {
                            Ok(Some(woken)) if woken.state == SandboxState::Running => {
                                return Ok(ResolvedSandbox::found(woken))
                            }
                            _ => {
                                // The wake could not be undone: leaving it live beside a fresh
                                // sandbox is a leak the caller gets no id for. Fail so the woken
                                // sandbox stays identifiable rather than provisioning a second one.
                                if let Err(error) = self.pause(id).await {
                                    return Err(error.context(ErrorData::SandboxCommandFailed {
                                        failure: "resumeRollbackFailed".to_string(),
                                        reason: format!(
                                            "{GET_OR_CREATE}: woke sandbox '{id}' but could not \
                                             confirm it healthy or put it back to sleep"
                                        ),
                                    }));
                                }
                            }
                        }
                    }
                }
                // Still coming up, or already being woken by someone else. Waited for rather than
                // replaced: the sandbox keeps starting either way, and a second one beside it is
                // the leak the arm above exists to avoid. A slow data plane is answered with the
                // failure, never by provisioning more of it.
                Ok(Some(sandbox)) if sandbox.state == SandboxState::Starting => {
                    let generation = self.settle(id).await?;
                    return Ok(ResolvedSandbox::found(SandboxInstance {
                        sandbox_id: id.to_string(),
                        state: SandboxState::Running,
                        generation,
                    }));
                }
                Ok(_) => {}
                Err(error) if error.code == "SANDBOX_UNREACHABLE" => {}
                Err(error) => {
                    return Err(error.context(ErrorData::SandboxCommandFailed {
                        failure: "getOrCreateFailed".to_string(),
                        reason: format!("{GET_OR_CREATE}: reaching sandbox '{id}' failed"),
                    }))
                }
            }
        }

        self.create(request).await.map(ResolvedSandbox::created)
    }

    async fn list(&self) -> Result<Vec<SandboxInstance>> {
        let sandboxes = self.client.list_sandboxes(&self.engine).await.context(
            ErrorData::SandboxUnreachable {
                operation: "sandbox.list".to_string(),
                reason: "the Agent Platform API did not answer a sandbox list".to_string(),
            },
        )?;

        // A sandbox this provider cannot fully read — an unaddressable name or an unrecognised
        // state — is left out rather than surfaced as a handle to nothing or failing the whole
        // enumeration; one odd sandbox must not hide every other from an orphan sweep. Both halves
        // are skipped for the same reason, so leniency is consistent across the record.
        Ok(sandboxes
            .into_iter()
            .filter_map(|sandbox| {
                let sandbox_id = sandbox.name.as_deref().and_then(sandbox_segment)?;
                let state = sandbox_state("sandbox.list", sandbox.state.as_deref()).ok()?;
                // A bulk list does not probe each agent, so it reports no generation; a caller that
                // needs one reads the single sandbox through `get`.
                Some(SandboxInstance {
                    sandbox_id: sandbox_id.to_string(),
                    state,
                    generation: NO_GENERATION,
                })
            })
            .collect())
    }

    async fn run_command(
        &self,
        sandbox_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        Self::checked_sandbox_id(RUN_COMMAND, sandbox_id)?;
        Self::checked_command(RUN_COMMAND, &request)?;

        // The synchronous window is the proxy's, not the command's: a command that outlives one
        // `:execute` is detached as a job so a later poll can still reach its output.
        if request.timeout <= MAX_SYNCHRONOUS_TIMEOUT {
            self.run_synchronous(sandbox_id, &request).await
        } else {
            self.run_detached(sandbox_id, request).await
        }
    }

    async fn start_job(&self, sandbox_id: &str, request: RunCommandRequest) -> Result<JobStart> {
        Self::checked_sandbox_id(JOB_START, sandbox_id)?;
        Self::checked_command(JOB_START, &request)?;

        let envelope = exec_envelope("jobStart", sandbox_id, &request);
        let body = self.execute_op(sandbox_id, JOB_START, envelope).await?;

        // The `:execute` succeeded, so the job was accepted and is running; only its id could not
        // be read. Nothing can poll or cancel it after this, and the command's own deadline is
        // what bounds it — so the outcome is unestablished rather than a reply that failed to read.
        let started: JobStartReply = serde_json::from_slice(&body).map_err(|_| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp-agent-platform".to_string(),
                binding_name: JOB_START.to_string(),
                field: "jobId".to_string(),
                response_json: truncated(&body),
            })
            .context(ErrorData::SandboxOutcomeUnknown {
                operation: JOB_START.to_string(),
                reason: "the job started and its id could not be read, so it cannot be polled"
                    .to_string(),
            })
        })?;

        Ok(JobStart {
            job_id: started.job_id,
        })
    }

    async fn poll_job(
        &self,
        sandbox_id: &str,
        job_id: &str,
        since_seq: Option<u64>,
    ) -> Result<JobPoll> {
        Self::checked_sandbox_id(JOB_POLL, sandbox_id)?;
        let reply = poll_once(
            self.client.as_ref(),
            &self.engine,
            sandbox_id,
            job_id,
            since_seq,
        )
        .await?;

        Ok(JobPoll {
            running: reply.running,
            frames: reply
                .frames
                .into_iter()
                .map(WireFrame::into_output)
                .collect::<Result<Vec<_>>>()?,
            exit: reply.exit_code.map(|code| JobExit {
                code,
                truncated: reply.truncated.unwrap_or(false),
            }),
            error: reply.error.map(|error| JobError {
                code: error.code,
                message: error.message,
            }),
        })
    }

    async fn cancel_job(&self, sandbox_id: &str, job_id: &str) -> Result<()> {
        Self::checked_sandbox_id(JOB_CANCEL, sandbox_id)?;
        let body = self
            .client
            .execute(&self.engine, sandbox_id, &cancel_body(job_id))
            .await
            .map_err(|error| unanswered_job(JOB_CANCEL, error))?;

        if !cancel_confirmed(&body) {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "agentRefused".to_string(),
                reason: format!("{JOB_CANCEL}: {}", truncated(&body)),
            }));
        }

        Ok(())
    }

    async fn read_file(&self, sandbox_id: &str, path: &str) -> Result<Vec<u8>> {
        Self::checked_sandbox_id("sandbox.readFile", sandbox_id)?;
        let body = self
            .execute_op(
                sandbox_id,
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

    async fn write_files(&self, sandbox_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        Self::checked_sandbox_id("sandbox.writeFiles", sandbox_id)?;
        // One request per path, stopping at the first failure — the partial application every
        // backend performs, so a caller sees one contract rather than several. The agent's field
        // is `contentsBase64`; `contents` is dropped silently.
        for (path, contents) in files {
            let body = self
                .execute_op(
                    sandbox_id,
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

    async fn preview(&self, _sandbox_id: &str, _port: u16) -> Result<PreviewCapability> {
        Err(self.unsupported(
            "preview",
            "Agent Platform mints no port-scoped ingress capability; the only ingress is :execute",
        ))
    }

    async fn pause(&self, sandbox_id: &str) -> Result<()> {
        Self::checked_sandbox_id("sandbox.pause", sandbox_id)?;
        let started = self.client.pause(&self.engine, sandbox_id).await.context(
            ErrorData::SandboxCommandFailed {
                failure: "pauseFailed".to_string(),
                reason: format!("sandbox.pause: sandbox '{sandbox_id}' could not be paused"),
            },
        )?;
        self.await_operation("sandbox.pause", started).await?;
        Ok(())
    }

    async fn resume(&self, sandbox_id: &str) -> Result<()> {
        Self::checked_sandbox_id("sandbox.resume", sandbox_id)?;
        let started = self.client.resume(&self.engine, sandbox_id).await.context(
            ErrorData::SandboxCommandFailed {
                failure: "resumeFailed".to_string(),
                reason: format!("sandbox.resume: sandbox '{sandbox_id}' could not be resumed"),
            },
        )?;
        self.await_operation("sandbox.resume", started).await?;
        Ok(())
    }

    async fn snapshot(&self, sandbox_id: &str) -> Result<String> {
        Self::checked_sandbox_id("sandbox.snapshot", sandbox_id)?;
        // A generated display name, because the API takes one and the caller does not supply it.
        // The trait has no restore verb, so the returned name is not yet consumable through it —
        // restore is `create` from a snapshot, which this backend can do but the trait cannot ask.
        let display_name = format!("snap-{}", uuid::Uuid::new_v4().simple());
        let started = self
            .client
            .snapshot(&self.engine, sandbox_id, &display_name)
            .await
            .context(ErrorData::SandboxCommandFailed {
                failure: "snapshotFailed".to_string(),
                reason: format!("sandbox.snapshot: sandbox '{sandbox_id}' could not be captured"),
            })?;

        let snapshot: SandboxSnapshot =
            serde_json::from_value(self.await_operation("sandbox.snapshot", started).await?)
                .map_err(|error| {
                    AlienError::new(ErrorData::UnexpectedResponseFormat {
                        provider: "gcp-agent-platform".to_string(),
                        binding_name: "sandbox.snapshot".to_string(),
                        field: "response".to_string(),
                        response_json: format!(
                            "the snapshot operation resolved to a non-snapshot: {error}"
                        ),
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

    async fn terminate(&self, sandbox_id: &str) -> Result<()> {
        Self::checked_sandbox_id(TERMINATE, sandbox_id)?;
        // Accepted, not completed: the client returns before the sandbox is gone. Returning here
        // would report containment while the code may still run, which is the whole point of
        // terminate — so the delete is confirmed by polling to not-found.
        // A sandbox that is already gone is the state terminate exists to reach, so not-found is
        // success. Narrowed to exactly that: mapping any failure to `Ok` would report containment
        // for a sandbox another deployment owns and this one was refused.
        if let Err(error) = self.client.delete_sandbox(&self.engine, sandbox_id).await {
            if !is_not_found(&error) {
                return Err(error.context(ErrorData::SandboxUnreachable {
                    operation: TERMINATE.to_string(),
                    reason: format!("the delete of sandbox '{sandbox_id}' was not accepted"),
                }));
            }
            return Ok(());
        }

        for _ in 0..TERMINATE_POLL_ATTEMPTS {
            match self.client.get_sandbox(&self.engine, sandbox_id).await {
                Err(error) if is_not_found(&error) => return Ok(()),
                // A read that fails is not a sandbox that is gone, and one throttled response must
                // not end the poll: the attempt budget decides.
                Err(error) => {
                    warn!(sandbox = %sandbox_id, %error, "could not confirm a sandbox is gone")
                }
                Ok(_) => {}
            }
            tokio::time::sleep(TERMINATE_POLL_INTERVAL).await;
        }

        Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: TERMINATE.to_string(),
            reason: format!(
                "deletion of '{sandbox_id}' was accepted but the sandbox was still present after \
                 {}s; it may still be running",
                TERMINATE_POLL_ATTEMPTS as u64 * TERMINATE_POLL_INTERVAL.as_secs()
            ),
        }))
    }
}

/// One step of a detached job's poll loop, yielding output frames as they arrive and a terminal
/// item once the job ends.
async fn job_poll_step(mut state: JobPollState) -> Option<(Result<CommandOutput>, JobPollState)> {
    loop {
        if let Some(item) = state.pending.pop_front() {
            return Some((item, state));
        }
        if state.finished {
            return None;
        }

        if tokio::time::Instant::now() >= state.deadline_at {
            // The deadline is this client's decision, so it only names an outcome once the cancel
            // that makes it true has landed. A cancel that fails leaves the job running, and the
            // caller has to be told that rather than that the command was stopped.
            let cancelled = state
                .client
                .execute(
                    &state.engine,
                    &state.sandbox_id,
                    &cancel_body(&state.job_id),
                )
                .await;
            let confirmed = cancelled.as_ref().is_ok_and(|body| cancel_confirmed(body));
            state.pending.push_back(Err(match cancelled {
                Ok(_) if confirmed => AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "timeoutExceeded".to_string(),
                    reason: "the command's deadline elapsed before its job reported an outcome"
                        .to_string(),
                }),
                Ok(_) => AlienError::new(ErrorData::SandboxOutcomeUnknown {
                    operation: RUN_COMMAND.to_string(),
                    reason: "the command's deadline elapsed and its job did not confirm the cancel"
                        .to_string(),
                }),
                Err(error) => error.context(ErrorData::SandboxOutcomeUnknown {
                    operation: RUN_COMMAND.to_string(),
                    reason: "the command's deadline elapsed and its job could not be cancelled"
                        .to_string(),
                }),
            }));
            state.finished = true;
            continue;
        }

        let poll = match poll_once(
            state.client.as_ref(),
            &state.engine,
            &state.sandbox_id,
            &state.job_id,
            state.since_seq,
        )
        .await
        {
            Ok(poll) => poll,
            // A standalone poll is repeatable, but this one watches a running command, and giving
            // up on it leaves that command's outcome unestablished.
            Err(error) => {
                state
                    .pending
                    .push_back(Err(error.context(ErrorData::SandboxOutcomeUnknown {
                        operation: RUN_COMMAND.to_string(),
                        reason: "the job is no longer watched".to_string(),
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
            let output = frame.into_output();
            // As in the synchronous path: a frame that will not convert ends the poll rather than
            // being queued ahead of a terminal result that would contradict it.
            let failed = output.is_err();
            state.pending.push_back(output);
            if failed {
                state.finished = true;
                break;
            }
        }

        if !poll.running {
            // The terminal outcome is the envelope's, not a frame's: a clean exit carries a code,
            // and a deadline, spawn failure or cancel carries an error object with no code.
            let terminal = match poll.error {
                Some(error) => Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: error.code,
                    reason: error.message,
                })),
                // A job that finished without an exit code never established its outcome; an
                // invented code is indistinguishable from one the command really exited with.
                None => match poll.exit_code {
                    Some(code) => Ok(CommandOutput::Exit {
                        code,
                        truncated: poll.truncated.unwrap_or(false),
                    }),
                    None => Err(AlienError::new(ErrorData::SandboxOutcomeUnknown {
                        operation: RUN_COMMAND.to_string(),
                        reason: "the job finished without reporting an exit code".to_string(),
                    })),
                },
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

/// One `jobPoll` against a sandbox, classified as a standalone poll: nothing about the job
/// changes, so a call that fails is worth repeating. `run_command`'s loop re-contexts it.
async fn poll_once(
    client: &dyn AgentPlatformApi,
    engine: &str,
    sandbox_id: &str,
    job_id: &str,
    since_seq: Option<u64>,
) -> Result<JobPollReply> {
    let body = client
        .execute(engine, sandbox_id, &poll_body(job_id, since_seq))
        .await
        .map_err(|error| unanswered_job(JOB_POLL, error))?;

    serde_json::from_slice(&body).map_err(|_| {
        AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: JOB_POLL.to_string(),
            field: "jobPoll".to_string(),
            response_json: truncated(&body),
        })
    })
}

/// A poll or cancel that did not complete. Both leave the job exactly as it was, so unlike a
/// command they carry the retry signal; a sandbox that is gone is an answer rather than a failure.
fn unanswered_job(
    operation: &str,
    error: AlienError<AgentPlatformErrorData>,
) -> AlienError<ErrorData> {
    if is_not_found(&error) {
        return error.context(ErrorData::SandboxCommandFailed {
            failure: "sandboxGone".to_string(),
            reason: format!("{operation}: the sandbox does not exist"),
        });
    }
    error.context(ErrorData::SandboxUnreachable {
        operation: operation.to_string(),
        reason: "the sandbox did not complete the call".to_string(),
    })
}

/// Whether a `jobCancel` reply is the cancel landing.
///
/// A reply arriving is not the cancel succeeding: the agent answers `{}` when it cancelled the job
/// and its own error text when it did not — `JobNotFound`, say — and both come back through a
/// successful `:execute`.
fn cancel_confirmed(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body).is_ok_and(|value| value.is_object())
}

/// The id a started job answers to, as the agent's `jobStart` returns it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobStartReply {
    job_id: String,
}

/// The bookkeeping a detached job's poll loop carries between steps.
struct JobPollState {
    client: Arc<dyn AgentPlatformApi>,
    engine: String,
    sandbox_id: String,
    job_id: String,
    since_seq: Option<u64>,
    pending: VecDeque<Result<CommandOutput>>,
    finished: bool,
    deadline_at: tokio::time::Instant,
}

/// A job's output so far, and how it ended once it has. Mirrors the agent's `jobPoll` reply.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobPollReply {
    running: bool,
    #[serde(default)]
    frames: Vec<WireFrame>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    truncated: Option<bool>,
    #[serde(default)]
    error: Option<JobErrorReply>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobErrorReply {
    code: String,
    message: String,
}

/// A frame as the agent writes it, shared by the synchronous NDJSON body and the job frames.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "t")]
enum WireFrame {
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
            Self::Error { code, message } => {
                Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: code,
                    reason: message,
                }))
            }
        }
    }
}

/// A frame that arrived is proof the command ran, so a payload that will not decode leaves the
/// outcome unestablished rather than merely malformed.
fn decode_frame_data(data: &str) -> Result<Vec<u8>> {
    BASE64.decode(data).map_err(|error| {
        AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: RUN_COMMAND.to_string(),
            field: "data".to_string(),
            response_json: format!("an output frame's data is not base64: {error}"),
        })
        .context(ErrorData::SandboxOutcomeUnknown {
            operation: RUN_COMMAND.to_string(),
            reason: "an output frame did not decode".to_string(),
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
                let output = frame.into_output();
                // A frame that will not convert ends the body: letting a later exit follow would
                // answer the question this item just reported as unanswerable. `saw_terminal`
                // stops the trailing item below from saying the same thing twice.
                let failed = output.is_err();
                frames.push(output);
                if failed {
                    saw_terminal = true;
                    break;
                }
            }
            Err(error) => {
                if !saw_any {
                    return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "agentRefused".to_string(),
                        reason: format!("run_command was refused: {}", truncated(body)),
                    }));
                }
                // Frames already arrived, so the command ran and this leaves its end unknown.
                // `saw_terminal` stops the trailing item below from saying the same thing twice.
                frames.push(Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp-agent-platform".to_string(),
                    binding_name: RUN_COMMAND.to_string(),
                    field: "frame".to_string(),
                    response_json: format!("an output frame did not parse: {error}"),
                })
                .context(ErrorData::SandboxOutcomeUnknown {
                    operation: RUN_COMMAND.to_string(),
                    reason: "an output frame did not parse".to_string(),
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
        frames.push(Err(AlienError::new(ErrorData::SandboxOutcomeUnknown {
            operation: RUN_COMMAND.to_string(),
            reason: "the command's output ended without a terminal frame".to_string(),
        })));
    }
    Ok(frames)
}

/// The envelope for `exec` or `jobStart`. `timeoutMs` is the field the agent reads; both ops take
/// the identical body.
fn exec_envelope(op: &str, _sandbox_id: &str, request: &RunCommandRequest) -> serde_json::Value {
    json!({
        "v": AGENT_PROTOCOL_VERSION,
        "op": op,
        "command": request.argv(),
        "timeoutMs": timeout_millis(request.timeout),
        "cwd": request.cwd,
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

/// Milliseconds, saturated: a timeout long enough to overflow `u64` ms is not one anyone meant,
/// and wrapping it would turn "effectively forever" into "immediately".
fn timeout_millis(timeout: Duration) -> u64 {
    u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)
}

/// Reads a `writeFile` reply, which succeeds with an empty body.
///
/// A non-empty body from the op is the agent's error text, not a success shape, so it is
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
fn sandbox_segment(name: &str) -> Option<&str> {
    let segment = name.rsplit('/').next()?;
    is_addressable_id(segment).then_some(segment)
}

fn is_addressable_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_SANDBOX_ID
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
fn sandbox_state(operation: &str, state: Option<&str>) -> Result<SandboxState> {
    match state {
        Some("STATE_RUNNING") => Ok(SandboxState::Running),
        Some("STATE_CREATING" | "STATE_PENDING" | "STATE_RESUMING") => Ok(SandboxState::Starting),
        Some("STATE_PAUSED" | "STATE_PAUSING" | "STATE_SUSPENDED") => Ok(SandboxState::Paused),
        Some("STATE_STOPPED" | "STATE_FAILED" | "STATE_DELETING" | "STATE_DELETED") => {
            Ok(SandboxState::Terminated)
        }
        other => Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "gcp-agent-platform".to_string(),
            binding_name: operation.to_string(),
            field: "state".to_string(),
            response_json: other
                .map_or_else(|| "absent".to_string(), |state| format!("\"{state}\"")),
        })),
    }
}

/// Turns a completed operation into its response payload, or the error it reported.
fn finish_operation(operation: &str, name: &str, op: Operation) -> Result<serde_json::Value> {
    match op.result {
        Some(OperationResult::Response { response }) => Ok(response),
        Some(OperationResult::Error { error }) => {
            Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "operationFailed".to_string(),
                reason: format!(
                    "{operation}: operation '{name}' failed (grpc {}): {}",
                    error.code, error.message
                ),
            }))
        }
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
    let end = (0..=LIMIT)
        .rev()
        .find(|at| text.is_char_boundary(*at))
        .unwrap_or(0);
    format!("{}…", &text[..end])
}

#[cfg(test)]
#[path = "gcp_agent_platform_tests.rs"]
mod tests;
