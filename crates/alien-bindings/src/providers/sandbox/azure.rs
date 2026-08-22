//! Azure sandbox provider.
//!
//! The one backend with no Alien agent inside the sandbox: the ADC data plane implements exec,
//! files and lifecycle natively, so this provider is a translation layer rather than a transport
//! for a protocol. Verified against a stock `ubuntu` catalog disk containing no Alien code.

use std::collections::BTreeMap;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};

use crate::error::{ErrorData, Result};
use crate::providers::sandbox::{guard_for, Bounded, DeadlineReport};
use crate::traits::{
    Binding, CommandOutput, CreateSessionRequest, PreviewCapability, RunCommandRequest, Sandbox,
    SandboxSession, SandboxSessionState,
};
use alien_azure_clients::azure::sandbox_data_plane::{
    CreateSandbox, EgressHostRule, EgressPolicy, SandboxDataPlaneApi,
};
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{Platform, SandboxCapabilities, SandboxEgress};
use alien_error::{AlienError, ContextError};
use tracing::warn;

/// A Sandbox backed by the Azure ADC data plane.
#[derive(Debug)]
pub struct AzureSandbox {
    client: std::sync::Arc<dyn SandboxDataPlaneApi>,
    sandbox_group: String,
    /// Catalog disk image every session is created from, from the declaration.
    disk_image: String,
    /// Outbound policy every session is created with, from the declaration.
    egress: SandboxEgress,
    /// Idle seconds after which a session suspends itself, if the declaration asked for one.
    idle_suspend_seconds: Option<u32>,
    /// Session ceilings, in the data plane's own units.
    cpu: String,
    memory: String,
}

impl AzureSandbox {
    /// Builds a provider bound to one sandbox group.
    pub fn new(
        client: std::sync::Arc<dyn SandboxDataPlaneApi>,
        sandbox_group: String,
        disk_image: String,
        egress: SandboxEgress,
        idle_suspend_seconds: Option<u32>,
        cpu: String,
        memory: String,
    ) -> Self {
        Self {
            client,
            sandbox_group,
            disk_image,
            egress,
            idle_suspend_seconds,
            cpu,
            memory,
        }
    }

    /// The catalog image sessions are created from. Exists so a test can prove the declaration
    /// reached the provider — the failure it guards is silent, so nothing else would show it.
    #[cfg(test)]
    pub(crate) fn disk_image(&self) -> &str {
        &self.disk_image
    }

    /// A session id that stays one path segment.
    ///
    /// The id is interpolated into the data-plane URL, and `Url::parse` resolves `..` — so an id
    /// carrying one addresses a different sandbox group, which a stack-scoped management identity
    /// can reach. Azure mints ids itself; this bounds the ones a caller hands back.
    fn checked_session_id(operation: &str, session_id: &str) -> Result<()> {
        let usable = !session_id.is_empty()
            && session_id.len() <= MAX_SESSION_ID
            && session_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

        if usable {
            return Ok(());
        }

        Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: operation.to_string(),
            details: format!(
                "session id '{session_id}' must hold only letters, digits, '-' and '_', at most \
                 {MAX_SESSION_ID} characters"
            ),
            field_name: Some("sessionId".to_string()),
        }))
    }

    fn unsupported(&self, capability: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: capability.to_string(),
            reason: "not supported on azure".to_string(),
        })
    }

    /// Sorts a data-plane failure into the two buckets every other backend uses.
    ///
    /// A refusal is a request the data plane understood and rejected, so repeating it repeats the
    /// refusal. Anything else left the outcome unknown: for the idempotent file operations that is
    /// worth another attempt, but `run_command` may already have started the command and must not
    /// carry the retry signal. The cause stays on the source chain rather than in `reason`, which
    /// is what keeps a raw response body out of an externally visible message.
    fn failed(operation: &str, error: AlienError<ClientErrorData>) -> AlienError<ErrorData> {
        if is_refusal(&error) {
            return error.context(ErrorData::SandboxCommandFailed {
                failure: "dataPlaneRefused".to_string(),
                reason: format!("{operation} was refused; the cause carries which side refused"),
            });
        }

        if operation == RUN_COMMAND || operation == CREATE {
            return error.context(ErrorData::SandboxCommandFailed {
                failure: "outcomeUnknown".to_string(),
                reason: format!(
                    "{operation} did not complete against the Azure sandbox data plane, so \
                     whether it took effect is unknown"
                ),
            });
        }

        error.context(ErrorData::SandboxUnreachable {
            operation: operation.to_string(),
            reason: "the Azure sandbox data plane did not complete the call".to_string(),
        })
    }
}

impl Binding for AzureSandbox {}

#[async_trait]
impl Sandbox for AzureSandbox {
    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::for_platform(Platform::Azure).expect("Azure has a sandbox backend")
    }

    async fn create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        let asked = egress_policy(&self.egress);
        let sandbox = self
            .client
            .create_sandbox(
                &self.sandbox_group,
                CreateSandbox {
                    disk_image: self.disk_image.clone(),
                    cpu: self.cpu.clone(),
                    memory: self.memory.clone(),
                    environment: request.env,
                    egress: asked.clone(),
                    idle_suspend_seconds: self.idle_suspend_seconds,
                },
            )
            .await
            .map_err(|error| Self::failed(CREATE, error))?;

        // The caller's requested id is not authoritative: Azure allocates the id, and returning
        // the requested one would hand back a handle that addresses nothing. Checked because
        // every later verb addresses the sandbox by it, and one this client cannot send is one
        // nothing can reach or reap.
        let _ = request.session_id;
        if Self::checked_session_id(CREATE, &sandbox.id).is_err() {
            let unreadable = AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "azure".to_string(),
                binding_name: CREATE.to_string(),
                field: "id".to_string(),
                response_json: format!("{:?}", sandbox.id),
            });

            // Reaped unless the id is itself what makes the delete unsafe: a path separator or an
            // escape would send that delete into another group. Everything else this check
            // refuses — an over-long id, an unusual character — is still safe to address once,
            // and refusing to reap it leaves a running sandbox no id-holder can find.
            // An allowlist, because the hazard is anything the URL parser reads differently:
            // `abc?x` starts a query string, so the delete would land on the sandbox named `abc`.
            let addressable = !sandbox.id.is_empty()
                && sandbox
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

            return Err(if !addressable {
                warn!(
                    session = %sandbox.id,
                    "the data plane minted an id this client will not send; the sandbox is \
                     running and cannot be deleted through this binding"
                );
                unreadable
            } else {
                self.discard(&sandbox.id, unreadable).await
            });
        }

        // Everything past this point owns a sandbox the caller has no id for, so every failure
        // deletes it. Azure allocates the id, so the one in this response was minted by this call.
        match self.settle(&sandbox).await {
            Ok(session) => Ok(session),
            Err(error) => Err(self.discard(&sandbox.id, error).await),
        }
    }

    async fn get(&self, session_id: &str) -> Result<Option<SandboxSession>> {
        Self::checked_session_id("sandbox.get", session_id)?;
        // A 404 is "gone", which is a valid answer. Anything else is a real failure and must not
        // be flattened into None, or a throttle would read as an expired session.
        let Some(sandbox) = self.read_session("sandbox.get", session_id).await? else {
            return Ok(None);
        };

        let state = session_state("sandbox.get", sandbox.state.as_deref())?;

        // This is the path a reconnect takes: a session outlives the declaration it was created
        // under, so a caller holding its id would otherwise be handed whatever containment it was
        // built with. Only the two ends of the lifecycle carry no policy, and that is not a
        // mismatch.
        self.judge_if_judgeable(&sandbox)?;

        Ok(Some(SandboxSession {
            session_id: sandbox.id,
            state,
            generation: 1,
        }))
    }

    async fn get_or_create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        if let Some(id) = request.session_id.as_deref() {
            // `create` returns a session that can take work, and reaching one someone else
            // started has to mean the same thing — so the same gate every other verb uses: bring
            // it up, judge it there, and refuse it if it does not match.
            match self.reconnect(id).await {
                Ok(session) => return Ok(session),
                // The two ways an id can fail to serve — gone, or running a policy the
                // declaration no longer matches — mean the same thing to a caller asking for a
                // session, and are answered the same way: a fresh one. The gate has already
                // discarded whatever it refused, so nothing is left running.
                //
                // Narrow on purpose: a readiness timeout says the data plane is slow, and
                // answering that by creating a second sandbox makes it slower.
                Err(error)
                    if error.code == "SANDBOX_NOT_AS_DECLARED"
                        || matches!(
                            &error.error,
                            Some(ErrorData::SandboxCommandFailed { failure, .. })
                                if failure == "sessionGone" || failure == "sessionTerminated"
                        ) => {}
                Err(error) => return Err(error),
            }
        }

        self.create(request).await
    }

    async fn list(&self) -> Result<Vec<SandboxSession>> {
        Err(self.unsupported("list"))
    }

    async fn run_command(
        &self,
        session_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        Self::checked_session_id(RUN_COMMAND, session_id)?;
        if request.deadline.is_zero() {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox.runCommand".to_string(),
                reason: "a command must carry a non-zero deadline".to_string(),
            }));
        }

        // The only verb that starts untrusted code, so it is the one that re-reads the policy: a
        // session id outlives a declaration change, and nothing else stands between an id a
        // caller kept and the egress it was built with. One extra read against a data plane the
        // command itself is about to cross.
        self.judged_session(RUN_COMMAND, session_id).await?;

        // The deadline bounds the untrusted code, not the caller's patience. Read out of the
        // preview SDK rather than assumed: `executeShellCommand` sends `command` and an optional
        // `workingDirectory` and nothing else, so there is no server-side timeout to ask for. The
        // deadline is enforced inside the session instead — the wrapper kills the command at it,
        // so the session survives and the call lands right after, the same shape the
        // agent-supervised backends give. The client-side guard is the backstop for a data plane
        // that never answers at all; there the only lever left is ending the session, and that
        // call returns once the session is confirmed gone rather than claim containment early.
        // The data plane's exec takes a command and a working directory and nothing else, so a
        // per-command variable has to travel as a shell assignment in front of it. Names are
        // checked first: an unchecked one is a second command, not a variable.
        for name in request.env.keys() {
            checked_env_name(RUN_COMMAND, name)?;
        }
        let shell = bounded_shell(&request.command, &request.env, request.deadline);

        let result = self.execute_within(session_id, &shell, &request).await?;
        // The session's own report, removed from what the caller sees.
        let (deadline_exceeded, stderr) =
            match DeadlineReport::read(result.exit_code, &result.stderr) {
                Bounded::Ran { killed, stderr } => (killed, stderr),
                Bounded::NotRun { reason } => {
                    return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "commandNotBounded".to_string(),
                        reason,
                    }))
                }
            };

        // The data plane returns a completed result, not a stream, so the frames are
        // reconstructed in order. Streaming is unverified on Azure, and pretending otherwise
        // here would be inventing a guarantee.
        let mut frames: Vec<Result<CommandOutput>> = Vec::new();
        if !result.stdout.is_empty() {
            frames.push(Ok(CommandOutput::Stdout {
                seq: 0,
                data: result.stdout.into_bytes(),
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
                // A missing exit code is not success. Azure did not report one, so the command's
                // outcome is unknown, and -1 says that rather than claiming zero.
                code: result.exit_code.unwrap_or(-1),
                truncated: false,
            }));
        }

        Ok(Box::pin(stream::iter(frames)))
    }

    /// Ungated on purpose, as is `mkdir`: reading existing content and creating an empty
    /// directory add nothing to a sandbox, so neither can turn a stale session into a way to run
    /// something under egress the declaration has since removed.
    async fn read_file(&self, session_id: &str, path: &str) -> Result<Vec<u8>> {
        Self::checked_session_id("sandbox.readFile", session_id)?;
        let path = &checked_path("sandbox.readFile", path)?;

        self.client
            .read_file(&self.sandbox_group, session_id, path)
            .await
            .map_err(|error| Self::failed("sandbox.readFile", error))
    }

    async fn write_files(&self, session_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        Self::checked_session_id("sandbox.writeFiles", session_id)?;
        // Checked before anything is written, and before anything is read: partial application is
        // the contract for a data plane that refuses midway, not for a path this process could
        // have rejected without a round trip.
        let files = files
            .into_iter()
            .map(|(path, contents)| Ok((checked_path("sandbox.writeFiles", &path)?, contents)))
            .collect::<Result<Vec<_>>>()?;

        // The one file operation that moves the caller's own content in. A write-then-run against
        // an id kept across a tightened declaration would land the payload in a sandbox with the
        // egress the declaration just removed, and the refusal would arrive a beat later.
        self.judged_session("sandbox.writeFiles", session_id).await?;

        // One request per path, stopping at the first failure: the same partial application every
        // other backend performs, so a caller sees one contract rather than five.
        for (path, contents) in files {
            self.client
                .write_file(&self.sandbox_group, session_id, &path, contents)
                .await
                .map_err(|error| Self::failed("sandbox.writeFiles", error))?;
        }

        Ok(())
    }

    async fn mkdir(&self, session_id: &str, path: &str) -> Result<()> {
        Self::checked_session_id("sandbox.mkdir", session_id)?;
        let path = &checked_path("sandbox.mkdir", path)?;

        self.client
            .mkdir(&self.sandbox_group, session_id, path)
            .await
            .map_err(|error| Self::failed("sandbox.mkdir", error))
    }

    async fn preview(&self, _session_id: &str, _port: u16) -> Result<PreviewCapability> {
        Err(self.unsupported("preview"))
    }

    async fn suspend(&self, session_id: &str) -> Result<()> {
        Self::checked_session_id("sandbox.suspend", session_id)?;
        // Accepted, not completed — the same contract the AWS backend follows. `get` reports
        // `Suspended` from the moment the stop is under way, so it answers "cannot take work",
        // not "has stopped"; only `terminate` confirms a session is actually gone.
        self.client
            .stop_sandbox(&self.sandbox_group, session_id)
            .await
            .map_err(|error| Self::failed("sandbox.suspend", error))
    }

    async fn resume(&self, session_id: &str) -> Result<()> {
        Self::checked_session_id("sandbox.resume", session_id)?;
        const OPERATION: &str = "sandbox.resume";

        let Some(found) = self.read_session(OPERATION, session_id).await? else {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "sessionGone".to_string(),
                reason: format!("{OPERATION}: session '{session_id}' does not exist"),
            }));
        };

        // Refused from the record already in hand where that record answers it, so a session
        // whose stored policy is plainly wrong is never put back on the network for a boot.
        self.judge_if_judgeable(&found)?;

        // Judged again after the wake: the stopped record is not the one the work runs under, and
        // a policy set on the group can change while a session sleeps.
        let mut resumed_here = false;
        let woken = self
            .await_running(OPERATION, session_id, &mut resumed_here)
            .await;

        let refusal = match woken {
            Err(error) => error,
            Ok(running) => match self.policy_must_hold(&running) {
                Ok(()) => return Ok(()),
                Err(error) => error,
            },
        };
        Err(self.put_back(session_id, resumed_here, refusal).await)
    }

    async fn snapshot(&self, _session_id: &str) -> Result<String> {
        Err(self.unsupported("snapshot"))
    }

    async fn terminate(&self, session_id: &str) -> Result<()> {
        Self::checked_session_id("sandbox.terminate", session_id)?;
        self.accept_delete(session_id).await?;

        // The delete is accepted, not completed: the client's own contract is "returns before it
        // is gone; confirm by polling to 404". Returning here would report containment while the
        // code is still running, which is the whole point of terminate.
        // The client rather than `get`: teardown needs the 404 and nothing else, and reading a
        // state it cannot parse would abort the poll for a session that is already going away —
        // replacing a `deadlineExceeded` finding with a deserialization error on the one path
        // where untrusted code is known to be running past its deadline.
        for _ in 0..TERMINATE_POLL_ATTEMPTS {
            // A read that fails is not a session that is gone, and it is not a reason to stop
            // looking either: the attempt budget decides, so one throttled response cannot end
            // the poll that turns an accepted delete into a confirmed one.
            if let Err(error) = self
                .client
                .get_sandbox(&self.sandbox_group, session_id)
                .await
            {
                if is_not_found(&error) {
                    return Ok(());
                }
                warn!(session = %session_id, %error, "could not confirm a sandbox is gone");
            }
            tokio::time::sleep(TERMINATE_POLL_INTERVAL).await;
        }

        Err(AlienError::new(ErrorData::SandboxUnreachable {
            operation: "sandbox.terminate".to_string(),
            reason: format!(
                "deletion of '{session_id}' was accepted but the session was still present after {}s; it may still be running",
                TERMINATE_POLL_ATTEMPTS * TERMINATE_POLL_INTERVAL.as_secs() as u32
            ),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl AzureSandbox {
    /// Brings a session the caller named back into service, or says why it cannot be.
    ///
    /// The one path that repairs rather than refusing: `get_or_create` asked for a usable
    /// session, so a session that cannot serve is discarded and replaced rather than returned as
    /// an error the caller has no way to act on.
    async fn reconnect(&self, session_id: &str) -> Result<SandboxSession> {
        let gone = || {
            AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "sessionGone".to_string(),
                reason: format!("{GET_OR_CREATE}: session '{session_id}' cannot take work"),
            })
        };

        let found = match self.read_session(GET_OR_CREATE, session_id).await? {
            // A failed sandbox is not going away on its own, and the caller asked for a session
            // rather than for this one, so it is reaped rather than left beside its replacement.
            Some(sandbox) if sandbox.state.as_deref() == Some("Failed") => {
                return Err(self.discard(session_id, gone()).await)
            }
            Some(sandbox) if sandbox.state.as_deref() != Some("Deleting") => sandbox,
            _ => return Err(gone()),
        };

        // Judged asleep first: waking one that already fails puts its workload back on the network
        // for a boot. Refused rather than deleted, here and after the wake: the policy mismatch
        // may belong to another revision, mid-command in the shared group.
        self.judge_if_judgeable(&found)?;

        // Judged again once it is up: only the woken record covers a session that was still coming
        // up, or a policy set on the group while it slept.
        let mut resumed_here = false;
        let running = match self
            .await_running(GET_OR_CREATE, session_id, &mut resumed_here)
            .await
        {
            Ok(running) => running,
            Err(error) => return Err(self.put_back(session_id, resumed_here, error).await),
        };
        if let Err(error) = self.policy_must_hold(&running) {
            return Err(self.put_back(session_id, resumed_here, error).await);
        }

        Ok(SandboxSession {
            session_id: running.id,
            state: SandboxSessionState::Running,
            generation: 1,
        })
    }

    /// Reads a session that is fit to be used, refusing one that is not.
    ///
    /// Refuses rather than repairs: a session this binding did not create and the caller did not
    /// ask to replace is not this call's to destroy. Two revisions of a stack share a sandbox
    /// group, so a tightened one reaping a session the other is mid-command on would be an
    /// outage caused by a read.
    ///
    /// Requires the session to be running, because that is the only state carrying a policy
    /// worth judging — and waking one to write into it would undo the idle suspend the
    /// declaration asked for.
    async fn judged_session(&self, operation: &str, session_id: &str) -> Result<()> {
        let refuse = |failure: &str, why: &str| {
            Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: failure.to_string(),
                reason: format!("{operation}: session '{session_id}' {why}"),
            }))
        };

        let Some(sandbox) = self.read_session(operation, session_id).await? else {
            return refuse("sessionGone", "does not exist");
        };

        match sandbox.state.as_deref() {
            Some("Running") => {}
            Some("Creating" | "Resuming") => {
                return refuse("sessionNotReady", "is still starting; wait for it to run")
            }
            Some("Deleting") => return refuse("sessionGone", "is being deleted"),
            Some("Failed") => return refuse("sessionGone", "has failed"),
            Some("Stopping") => return refuse("sessionSuspended", "is stopping; wait for it"),
            Some("Stopped" | "Suspended" | "Idle") => {
                return refuse("sessionSuspended", "is suspended; resume it first")
            }
            // Unreadable rather than suspended, which would send a caller to `resume` for an
            // answer it cannot give. The refusal below is reached only if the two state lists
            // drift apart, and refusing is the safe side of that.
            other => {
                session_state(operation, other)?;
                return refuse("sessionNotReady", "is in a state this client cannot read");
            }
        }

        self.policy_must_hold(&sandbox)
    }

    /// Reads a session, or `None` when it is gone, without judging its policy.
    async fn read_session(
        &self,
        operation: &str,
        session_id: &str,
    ) -> Result<Option<alien_azure_clients::azure::sandbox_data_plane::Sandbox>> {
        match self
            .client
            .get_sandbox(&self.sandbox_group, session_id)
            .await
        {
            Ok(sandbox) => Ok(Some(sandbox)),
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(Self::failed(operation, error)),
        }
    }

    /// Wakes a session without judging it, for the wait that has nothing to judge yet.
    async fn resume_unchecked(&self, session_id: &str) -> Result<()> {
        self.client
            .resume_sandbox(&self.sandbox_group, session_id)
            .await
            .map_err(|error| Self::failed("sandbox.resume", error))
    }

    /// Refuses a sandbox that is not running the policy the declaration asked for.
    ///
    /// The effective policy can change under a live session — a group-scoped policy is set
    /// somewhere this binding never writes — so every path that hands one back checks, not just
    /// the one that created it.
    fn policy_must_hold(
        &self,
        sandbox: &alien_azure_clients::azure::sandbox_data_plane::Sandbox,
    ) -> Result<()> {
        let Some(asked) = egress_policy(&self.egress) else {
            return Ok(());
        };
        if policy_holds(&asked, sandbox.egress_policy.as_ref()) {
            return Ok(());
        }

        Err(AlienError::new(ErrorData::SandboxNotAsDeclared {
            session_id: sandbox.id.clone(),
            restriction: "egress policy".to_string(),
            reason: format!(
                "it is running {} where the declaration asks for {}",
                describe(sandbox.egress_policy.as_ref()),
                describe(Some(&asked))
            ),
        }))
    }

    /// Turns a freshly created sandbox into a session, or says why it is not one.
    ///
    /// Every check that can fail after the sandbox exists lives here, so `create` has one place
    /// to delete from rather than a delete beside each `?`.
    async fn settle(
        &self,
        sandbox: &alien_azure_clients::azure::sandbox_data_plane::Sandbox,
    ) -> Result<SandboxSession> {
        // The running sandbox is what gets judged, not the accept: a create response sent while
        // the sandbox is still coming up need not carry the policy yet, and reading its absence
        // as "the restriction did not take" would delete every sandbox that answered early.
        let mut resumed_here = false;
        let running = self
            .await_running(CREATE, &sandbox.id, &mut resumed_here)
            .await?;

        // A restriction that did not take effect is worse than one that was never asked for: the
        // caller believes the sandbox is contained.
        self.policy_must_hold(&running)?;

        Ok(SandboxSession {
            session_id: running.id,
            state: SandboxSessionState::Running,
            generation: 1,
        })
    }

    /// Waits for a session to be able to take work.
    ///
    /// The operation is the caller's, not this function's: a reconnect that waits is still a
    /// reconnect, and reporting it as a create would mark a repeatable read unrepeatable.
    ///
    /// A suspended session is resumed rather than waited on — on the create path an idle policy
    /// can stop a sandbox before its first command, and on the reconnect path a stopped sandbox
    /// is the ordinary resting state. Nothing else brings one up, so waiting alone would spend
    /// the whole deadline and then delete it.
    async fn await_running(
        &self,
        operation: &str,
        session_id: &str,
        resumed_here: &mut bool,
    ) -> Result<alien_azure_clients::azure::sandbox_data_plane::Sandbox> {
        let deadline = std::time::Instant::now() + SESSION_READY_TIMEOUT;
        let mut refusal: Option<String> = None;

        loop {
            let Some(sandbox) = self.read_session(operation, session_id).await? else {
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "sessionGone".to_string(),
                    reason: format!("{operation}: session '{session_id}' disappeared while it was being waited for"),
                }));
            };

            // The raw state, because the four the trait publishes cannot separate a sandbox on
            // its way up from one on its way down, and this loop needs that difference.
            match sandbox.state.as_deref() {
                Some("Running") => return Ok(sandbox),
                Some("Creating" | "Resuming") => {}
                // Still going down. Resume is refused in this state — the SDK's own resumable
                // set excludes it — so the wait is for `Stopped`, not for the call to work.
                Some("Stopping") => {}
                // Re-issued on every poll, because the attempt most likely to be refused is the
                // first one: remembering only that an attempt was made would spend the whole
                // budget watching a sandbox nothing is bringing up.
                Some("Stopped" | "Suspended" | "Idle") => {
                    match self.resume_unchecked(session_id).await {
                        Ok(()) => {
                            refusal = None;
                            *resumed_here = true;
                        }
                        Err(error) => {
                            let failure = match &error.error {
                                Some(ErrorData::SandboxCommandFailed { failure, .. }) => {
                                    failure.clone()
                                }
                                _ => error.code.clone(),
                            };
                            // A refusal is the one answer that proves the session did not wake.
                            // Anything else — a 5xx, a timeout, a dropped connection — leaves the
                            // outcome unknown, and an unknown wake is one this call owns.
                            if failure != "dataPlaneRefused" {
                                *resumed_here = true;
                            }
                            warn!(session = %session_id, %error, "resume was refused; still waiting");
                            refusal = Some(failure);
                        }
                    }
                }
                // A terminated session never becomes runnable, and folding it into the timeout
                // would report it a minute late as a slow boot.
                other => {
                    let state = session_state(operation, other)?;
                    return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                        failure: "sessionTerminated".to_string(),
                        reason: format!(
                            "session '{session_id}' reached {state:?} and will not run again"
                        ),
                    }));
                }
            }

            if std::time::Instant::now() >= deadline {
                // The last refusal, because "not running after 120s" sends a reader looking for a
                // slow data plane when the answer is that every resume was rejected.
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "sessionNotReady".to_string(),
                    reason: match refusal {
                        Some(code) => format!(
                            "session '{session_id}' was still not running after {}s; the last \
                             resume was refused with {code}",
                            SESSION_READY_TIMEOUT.as_secs()
                        ),
                        None => format!(
                            "session '{session_id}' was still not running after {}s",
                            SESSION_READY_TIMEOUT.as_secs()
                        ),
                    },
                }));
            }
            tokio::time::sleep(SESSION_READY_INTERVAL).await;
        }
    }

    /// Whether a record carries a policy this client can hold it to.
    ///
    /// A running session always reports its effective policy, so an absent one there is a
    /// mismatch. Off that state the data plane's behaviour is unverified, and reading absence as
    /// a mismatch would refuse every idle-suspended session; the read taken after the wake is
    /// authoritative either way.
    fn judgeable(sandbox: &alien_azure_clients::azure::sandbox_data_plane::Sandbox) -> bool {
        match sandbox.state.as_deref() {
            Some("Running") => true,
            Some("Stopping" | "Stopped" | "Suspended" | "Idle") => sandbox.egress_policy.is_some(),
            // The two ends of the lifecycle and anything unread: one has no policy yet, the other
            // has dropped it, and a state this client cannot name is refused before it gets here.
            _ => false,
        }
    }

    fn judge_if_judgeable(
        &self,
        sandbox: &alien_azure_clients::azure::sandbox_data_plane::Sandbox,
    ) -> Result<()> {
        if Self::judgeable(sandbox) {
            self.policy_must_hold(sandbox)?;
        }
        Ok(())
    }

    /// Re-suspends a session this call woke, keeping the reason it is being refused.
    ///
    /// Only a session this call woke: another revision of the same stack shares the sandbox
    /// group, and stopping one that was already up ends a command that revision is mid-way
    /// through. A stop that fails is named rather than logged — a sandbox this call put back on
    /// the network under a policy the declaration does not allow is not "nothing happened".
    async fn put_back(
        &self,
        session_id: &str,
        resumed_here: bool,
        reason: AlienError<ErrorData>,
    ) -> AlienError<ErrorData> {
        if !resumed_here {
            return reason;
        }
        let Err(failed) = self.client.stop_sandbox(&self.sandbox_group, session_id).await else {
            return reason;
        };
        // A session that is already gone is the state this was trying to reach, and reporting it
        // as left awake sends an operator looking for a sandbox that does not exist.
        if is_not_found(&failed) {
            return reason;
        }

        warn!(session = %session_id, error = %failed, "could not re-suspend a session this call woke");
        reason.context(ErrorData::SandboxCommandFailed {
            failure: "sandboxLeftAwake".to_string(),
            reason: format!(
                "session '{session_id}' was woken by this call, could not be handed back, and \
                 could not be put to sleep again"
            ),
        })
    }

    /// Deletes a sandbox the caller will never receive, keeping the reason it is being discarded.
    ///
    /// The delete's own failure must not replace that reason — it is the finding that matters —
    /// but it must not vanish either: the session id is in the error, and a failed delete leaves
    /// a sandbox only that id can find.
    async fn discard(
        &self,
        session_id: &str,
        reason: AlienError<ErrorData>,
    ) -> AlienError<ErrorData> {
        let Err(error) = self.accept_delete(session_id).await else {
            return reason;
        };

        warn!(
            session = %session_id,
            %error,
            "could not delete a sandbox that was never handed to its caller"
        );
        // Names the leak rather than the reason for it: a timeout and a policy mismatch both
        // reach here, and reporting either as the other sends the reader somewhere false. The
        // original reason stays on the chain. The clause is fixed text, because the delete's own
        // error is the cloud client's and this variant is externally visible.
        reason.context(ErrorData::SandboxCommandFailed {
            failure: "sandboxLeftBehind".to_string(),
            reason: format!(
                "session '{session_id}' was not handed to its caller and could not be deleted, \
                 so it is still running"
            ),
        })
    }

    /// Runs one shell string under the client-side guard, which is the deadline plus the grace
    /// the in-session `timeout` needs to report back. See `run_command` for why the deadline is
    /// enforced inside the session.
    ///
    /// Reached only by a session that could not run `timeout`, so it is the one path where
    /// untrusted code is known to be overrunning: the session is ended and the call returns once
    /// that is confirmed, because `deadlineExceeded` has to mean the command stopped rather than
    /// that a stop was asked for.
    async fn execute_within(
        &self,
        session_id: &str,
        command: &str,
        request: &RunCommandRequest,
    ) -> Result<alien_azure_clients::azure::sandbox_data_plane::ExecResult> {
        match tokio::time::timeout(
            guard_for(request.deadline)?,
            self.client.execute_shell_command(
                &self.sandbox_group,
                session_id,
                command,
                request.working_directory.clone(),
            ),
        )
        .await
        {
            Ok(inner) => inner.map_err(|error| Self::failed(RUN_COMMAND, error)),
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

    /// Asks Azure to delete the session and returns once the request is accepted.
    ///
    /// An already-gone session is the desired end state. Every other failure leaves the session
    /// running, and reporting success there tells the caller untrusted code has stopped when it
    /// has not.
    async fn accept_delete(&self, session_id: &str) -> Result<()> {
        match self
            .client
            .delete_sandbox(&self.sandbox_group, session_id)
            .await
        {
            Ok(_) => Ok(()),
            Err(error) if is_not_found(&error) => Ok(()),
            Err(error) => Err(Self::failed("sandbox.terminate", error)),
        }
    }
}

/// How long termination waits for Azure to actually remove a session.
///
/// Azure accepts a delete and completes it asynchronously, so "gone" is only observable by
/// polling. Bounded rather than open-ended: a caller waiting forever is its own outage, and an
/// unconfirmed deletion is reported as unconfirmed rather than silently treated as done.
const TERMINATE_POLL_ATTEMPTS: u32 = 15;
const TERMINATE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// The command, bounded inside the session.
///
/// The data plane takes one shell string, so the command is passed to `sh` as arguments rather
/// than pasted into the program text: `"$@"` cannot re-parse what it holds, so an argument
/// carrying a space or an operator stays one argument.
fn bounded_shell(
    command: &[String],
    env: &BTreeMap<String, String>,
    deadline: std::time::Duration,
) -> String {
    let escape = |value: &str| value.replace('\'', "'\\''");

    // Through `env`, so the variables reach the caller's command and not the wrapper that bounds
    // it: an assignment in front of the wrapper would put a caller-chosen `PATH` on the shell
    // that resolves `setsid`, `sleep` and `kill`, and the deadline is only as real as those.
    let mut argv = Vec::with_capacity(command.len() + env.len() + 2);
    if !env.is_empty() {
        argv.push("env".to_string());
        argv.extend(env.iter().map(|(name, value)| format!("{name}={value}")));
        argv.push("--".to_string());
    }
    argv.extend(command.iter().cloned());

    let arguments = argv
        .iter()
        .map(|argument| format!(" '{}'", escape(argument)))
        .collect::<String>();
    format!(
        "sh -c '{}' sh{arguments}",
        escape(&DeadlineReport::bounded_program(deadline))
    )
}

/// Refuses a variable name the shell would read as anything other than a name.
///
/// The name is not quotable — it sits left of the `=` — so a name carrying a space or a `;` is a
/// second command rather than a variable, and quoting the value alone would not stop it.
fn checked_env_name(operation: &str, name: &str) -> Result<()> {
    let usable = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if usable {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::InvalidInput {
        operation_context: operation.to_string(),
        details: format!(
            "environment variable name '{name}' is not a shell name: letters, digits and \
             underscores only, and not starting with a digit"
        ),
        field_name: Some("env".to_string()),
    }))
}

/// Refuses a caller's path before it reaches the data plane, and returns what to send.
///
/// This refuses traversal syntax; it establishes no root. Whether the data plane bounds a path is
/// undocumented and unmeasured, so no rule here can promise confinement — what it promises is
/// that a path cannot name a parent. A leading slash is trimmed rather than refused because it
/// means "under the session's own root" on every other backend, and refusing it would make the
/// one shape portable code writes the one shape this backend rejects.
fn checked_path(operation: &str, path: &str) -> Result<String> {
    let refused = |details: &str| {
        Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: operation.to_string(),
            details: format!("path '{path}' {details}"),
            field_name: Some("path".to_string()),
        }))
    };

    // Checked before anything is trimmed, which would make "a/b/" and the file "a/b" the same
    // request.
    if path.ends_with('/') {
        return refused("must not end in '/'");
    }
    // A leading slash means "under the session's own root" on every other backend, so it means
    // that here too: the alternative is that the one path shape portable code writes is the one
    // shape the newest `files` backend refuses.
    let relative = path.trim_start_matches('/');
    if relative.is_empty() {
        return refused("is empty");
    }
    if relative.contains('\0') {
        return refused("contains a null byte");
    }
    if relative.split('/').any(|part| part == ".." || part.is_empty()) {
        return refused("must not traverse");
    }

    Ok(relative.to_string())
}

/// The policy a declared mode is created with.
///
/// `Full` inspection is what makes a `Deny` default mean no outbound access: under `Partial`,
/// `Legacy` and `None`, non-HTTP traffic is allowed through whatever the default action says, so
/// the sandbox would carry a `deny` label and a live network. `allow` sends no policy at all —
/// the data plane's default is already open, and `Full` there would block the non-HTTP traffic
/// `allow` promises.
fn egress_policy(egress: &SandboxEgress) -> Option<EgressPolicy> {
    let bounded = |host_rules| {
        Some(EgressPolicy {
            default_action: DENY.to_string(),
            unmodelled: Default::default(),
            rules: Vec::new(),
            host_rules,
            traffic_inspection: Some(FULL_INSPECTION.to_string()),
        })
    };

    match egress {
        SandboxEgress::Allow => None,
        // Written as a rule as well as a default, because Microsoft documents `Partial`
        // inspection as evaluating only traffic a rule matches and never states that `Full`
        // differs. A policy holding no rule at all is the one shape where "deny" could mean
        // nothing, and this is one rule to be out of it.
        SandboxEgress::Deny => bounded(vec![EgressHostRule {
            pattern: EVERY_HOST.to_string(),
            action: DENY.to_string(),
        }]),
        SandboxEgress::AllowDomains { domains } => bounded(
            domains
                .iter()
                .map(|domain| EgressHostRule {
                    pattern: domain.clone(),
                    action: ALLOW.to_string(),
                })
                .collect(),
        ),
    }
}

/// Whether the sandbox is running the policy it was created with.
///
/// Not equality — the data plane may return the policy normalised, and failing every create over a
/// reordered list would push whoever hits it into removing the check. Not a subset either, which
/// is the same mistake pointing outward: a permission the sandbox holds and the declaration never
/// asked for is exactly what this is looking for. So both directions, on the two things that can
/// permit traffic: nothing may allow a host the declaration did not name, in either list.
///
/// A group-scoped policy can add an entry nobody sent here, which is why the rules list is read at
/// all — it is never written.
fn policy_holds(asked: &EgressPolicy, effective: Option<&EgressPolicy>) -> bool {
    let Some(effective) = effective else {
        return false;
    };

    let asked_for = |host: &str| {
        asked
            .host_rules
            .iter()
            .any(|rule| rule.action.eq_ignore_ascii_case(ALLOW) && rule.pattern == host)
    };

    effective.default_action.eq_ignore_ascii_case(&asked.default_action)
        && effective
            .traffic_inspection
            .as_deref()
            .is_some_and(|mode| mode.eq_ignore_ascii_case(FULL_INSPECTION))
        && asked.host_rules.iter().all(|asked_rule| {
            effective.host_rules.iter().any(|rule| {
                rule.pattern == asked_rule.pattern
                    && rule.action.eq_ignore_ascii_case(&asked_rule.action)
            })
        })
        // A whitelist, not a blacklist: an action this client does not recognise is one it cannot
        // weigh, and `Transform` and `Rewrite` reach a host by rewriting the request rather than
        // by naming it. Only a plain deny, or an allow the declaration asked for, passes.
        && effective.host_rules.iter().all(|rule| {
            rule.action.eq_ignore_ascii_case(DENY)
                || (rule.action.eq_ignore_ascii_case(ALLOW) && asked_for(&rule.pattern))
        })
        // This client never writes `rules`, so anything here came from elsewhere — a group-scoped
        // policy, or an API that moved — and only an outright deny is readable as harmless.
        && effective.rules.iter().all(|rule| {
            rule.action
                .as_ref()
                .is_some_and(|action| action.action_type.eq_ignore_ascii_case(DENY))
        })
        // A field this client cannot read is a permission it cannot rule out.
        && effective.unmodelled.is_empty()
}

/// The effective policy, short enough to read in an error.
fn describe(effective: Option<&EgressPolicy>) -> String {
    match effective {
        None => "no policy at all".to_string(),
        Some(policy) if !policy.unmodelled.is_empty() => format!(
            "a policy carrying {}, which this client cannot weigh",
            policy
                .unmodelled
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(policy) => format!(
            "default action '{}' under {} inspection, {} host rules and {} match rules",
            policy.default_action,
            policy.traffic_inspection.as_deref().unwrap_or("unstated"),
            policy.host_rules.len(),
            policy.rules.len()
        ),
    }
}

/// The data plane's own lifecycle vocabulary, in ours.
///
/// An unrecognised state is an error rather than a default, because every default here is a lie
/// a caller acts on: `Running` sends commands to a sandbox that cannot answer them, and anything
/// else hides one that can.
fn session_state(operation: &str, state: Option<&str>) -> Result<SandboxSessionState> {
    match state {
        Some("Running") => Ok(SandboxSessionState::Running),
        Some("Creating" | "Resuming") => Ok(SandboxSessionState::Starting),
        // `Idle` is where the SDK contradicts itself: it declares `Idle` as a reason a sandbox
        // stopped, and then waits for a *state* of `Idle` after a stop. Accepted as suspended
        // either way — the alternative is that the state auto-suspend produces is the one state
        // this refuses to read.
        // A sandbox on its way down is not one to send work to, and the four states the trait
        // publishes have no word for "stopping" — so it reads as unusable. Anything that has to
        // tell "going down" from "already down" reads the raw state instead.
        Some("Stopping" | "Stopped" | "Suspended" | "Idle") => Ok(SandboxSessionState::Suspended),
        Some("Deleting" | "Failed") => Ok(SandboxSessionState::Terminated),
        other => Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
            provider: "azure".to_string(),
            binding_name: operation.to_string(),
            field: "state".to_string(),
            response_json: other
                .map_or_else(|| "absent".to_string(), |state| format!("\"{state}\"")),
        })),
    }
}

/// The data plane's own words for the two actions and the one inspection mode that blocks
/// non-HTTP traffic.
const DENY: &str = "Deny";
const ALLOW: &str = "Allow";
const FULL_INSPECTION: &str = "Full";

/// The host pattern that matches everything, so `deny` is a rule rather than only a default.
const EVERY_HOST: &str = "*";

/// Longest session id this client will put in a data-plane URL.
///
/// A bound on what a caller hands back rather than on what Azure mints: the ids seen in practice
/// are far shorter, and the point is that an id reaching the URL is one this client chose to send.
const MAX_SESSION_ID: usize = 63;

/// The two operations a repeat could perform twice.
///
/// `create` is a PUT to a collection with a server-minted id, so a second attempt makes a second
/// sandbox — and with no enumeration verb, the first one has no id-holder and nothing to reap it.
const RUN_COMMAND: &str = "sandbox.runCommand";
const CREATE: &str = "sandbox.create";
const GET_OR_CREATE: &str = "sandbox.getOrCreate";

/// How long a session has to become able to take work, and how often that is checked.
const SESSION_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const SESSION_READY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Whether the data plane understood the request and rejected it.
///
/// Reads the classified variant the client attaches rather than the status on its source: the
/// wrapper is what survives `create_azure_http_error_with_context`, and it already carries the
/// 4xx-versus-everything-else split this needs.
fn is_refusal(error: &AlienError<ClientErrorData>) -> bool {
    // `RemoteResourceConflict` is deliberately absent: the client also uses it for the 400s Azure
    // marks as propagation delays, and calling those refusals would tell a caller never to retry
    // the one failure Azure says to retry.
    matches!(
        &error.error,
        Some(
            ClientErrorData::RemoteResourceNotFound { .. }
                | ClientErrorData::RemoteAccessDenied { .. }
                | ClientErrorData::InvalidInput { .. }
        )
    ) || matches!(
        &error.error,
        Some(ClientErrorData::HttpResponseError { http_status, .. }) if (400..500).contains(http_status)
    )
}

/// Whether an Azure data-plane failure means the session is already gone.
///
/// Reads the status the client carries rather than the rendered message: `AlienError`'s `Display`
/// walks the whole source chain and the data plane puts the response body in it, so a path or a
/// trace id containing "404" would otherwise turn a throttle into "gone".
fn is_not_found(error: &AlienError<ClientErrorData>) -> bool {
    // Both variants, because the client wraps: `create_azure_http_error_with_context` builds the
    // `HttpResponseError` carrying the status and then returns
    // `http_error.context(RemoteResourceNotFound)` for a 404, so the outer variant is the
    // classified one and the status only survives on the source.
    matches!(
        &error.error,
        Some(ClientErrorData::RemoteResourceNotFound { .. })
    ) || matches!(
        &error.error,
        Some(ClientErrorData::HttpResponseError { http_status, .. }) if *http_status == 404
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_azure_clients::azure::sandbox_data_plane::ExecResult;
    use alien_azure_clients::azure::sandbox_data_plane::MockSandboxDataPlaneApi;
    use alien_azure_clients::azure::sandbox_data_plane::{
        EgressRule, EgressRuleAction, EgressRuleMatch,
    };
    use futures::StreamExt;

    fn http_error(status: u16, body: &str) -> AlienError<ClientErrorData> {
        AlienError::new(ClientErrorData::HttpResponseError {
            message: "Azure ADC sandbox.get failed".to_string(),
            url: "https://example.invalid/sandboxes/s1".to_string(),
            http_status: status,
            http_request_text: None,
            http_response_text: Some(body.to_string()),
        })
    }

    /// Answers the readiness read every create makes, with the policy the sandbox came up under.
    fn settles_running(client: &mut MockSandboxDataPlaneApi, egress: Option<EgressPolicy>) {
        client
            .expect_get_sandbox()
            .returning(move |_, id| Ok(running(id, egress.clone())));
    }

    fn sandbox_with(client: MockSandboxDataPlaneApi) -> AzureSandbox {
        AzureSandbox::new(
            std::sync::Arc::new(client),
            "grp".to_string(),
            "ubuntu".to_string(),
            SandboxEgress::Allow,
            None,
            "1000m".to_string(),
            "2048Mi".to_string(),
        )
    }

    /// The declared image has to reach the create call, not a default chosen here.
    ///
    /// Asserted on the argument the client receives, because the failure this pins is silent:
    /// a sandbox started from the wrong image returns a healthy session and only diverges once
    /// the caller's code is missing from it.
    #[tokio::test]
    async fn the_declared_image_reaches_the_create_call() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_create_sandbox()
            .withf(|_, request| request.disk_image == "my-toolchain")
            .times(1)
            .returning(|_, _| {
                Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
                    id: "s1".to_string(),
                    egress_policy: None,
                    state: Some("Running".to_string()),
                })
            });
        settles_running(&mut client, None);

        let sandbox = AzureSandbox::new(
            std::sync::Arc::new(client),
            "grp".to_string(),
            "my-toolchain".to_string(),
            SandboxEgress::Allow,
            None,
            "1000m".to_string(),
            "2048Mi".to_string(),
        );

        sandbox
            .create(CreateSessionRequest::default())
            .await
            .expect("create succeeds");
    }

    /// Azure accepts a delete and completes it later, so returning on the accepted call would
    /// report that untrusted code had stopped while it was still running. Time is paused, so the
    /// poll runs to its bound instantly.
    #[tokio::test(start_paused = true)]
    async fn a_termination_that_never_completes_is_reported_as_unconfirmed() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_delete_sandbox().returning(|_, _| Ok(()));
        client.expect_get_sandbox().returning(|_, id| {
            Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
                id: id.to_string(),
                egress_policy: None,
                state: Some("Running".to_string()),
            })
        });

        let error = sandbox_with(client)
            .terminate("s1")
            .await
            .expect_err("a session still present after the poll is not contained");
        assert!(
            error.to_string().contains("may still be running"),
            "says what is not known: {error}"
        );
    }

    /// The same path when Azure does finish: the session becomes absent and terminate succeeds.
    #[tokio::test(start_paused = true)]
    async fn a_termination_is_confirmed_once_the_session_is_gone() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_delete_sandbox().returning(|_, _| Ok(()));
        client
            .expect_get_sandbox()
            .returning(|_, _| Err(http_error(404, "SandboxNotFound")));

        sandbox_with(client)
            .terminate("s1")
            .await
            .expect("an absent session is a confirmed termination");
    }

    /// The discriminating case. A throttle whose body mentions 404 — a trace id, an inner code, a
    /// path — must not read as "the session is gone": that starts a second sandbox while the
    /// first keeps running, reporting a live session as terminated.
    #[test]
    fn only_the_status_decides_whether_a_session_is_gone() {
        assert!(is_not_found(&http_error(404, "SandboxNotFound")));

        // The shape the client actually produces: a 404 is returned as
        // `http_error.context(RemoteResourceNotFound)`, so the outer variant is the classified
        // one. Matching only `HttpResponseError` would read every real 404 as a live session.
        assert!(
            is_not_found(&AlienError::new(ClientErrorData::RemoteResourceNotFound {
                resource_type: "Sandbox".to_string(),
                resource_name: "s1".to_string(),
            })),
            "a wrapped 404 is how the client reports an absent session"
        );

        assert!(
            !is_not_found(&http_error(429, "throttled; see trace 404abc")),
            "a throttle is not a missing session"
        );
        assert!(
            !is_not_found(&http_error(403, "denied on /sandboxes/404/read")),
            "a path containing 404 is not a missing session"
        );
        assert!(
            !is_not_found(&http_error(500, "internal error 404")),
            "a server failure is not a missing session"
        );
    }

    /// A hand-written data plane, because mockall resolves an async expectation immediately and
    /// these tests need exec to hang, or to answer differently per call.
    #[derive(Debug)]
    struct ScriptedExec {
        deleted: std::sync::Arc<std::sync::atomic::AtomicBool>,
        commands: std::sync::Mutex<Vec<String>>,
        /// One result per exec call, in order; an empty queue hangs.
        results: std::sync::Mutex<
            std::collections::VecDeque<alien_azure_clients::azure::sandbox_data_plane::ExecResult>,
        >,
    }

    impl ScriptedExec {
        fn new(
            results: Vec<alien_azure_clients::azure::sandbox_data_plane::ExecResult>,
        ) -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                deleted: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                commands: std::sync::Mutex::new(Vec::new()),
                results: std::sync::Mutex::new(results.into_iter().collect()),
            })
        }

        fn exec_result(
            exit_code: i32,
            stdout: &str,
            stderr: &str,
        ) -> alien_azure_clients::azure::sandbox_data_plane::ExecResult {
            alien_azure_clients::azure::sandbox_data_plane::ExecResult {
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
                exit_code: Some(exit_code),
            }
        }
    }

    #[async_trait]
    impl SandboxDataPlaneApi for ScriptedExec {
        async fn stop_sandbox(
            &self,
            _group: &str,
            _sandbox_id: &str,
        ) -> alien_client_core::Result<()> {
            unreachable!("the command paths never suspend")
        }

        async fn resume_sandbox(
            &self,
            _group: &str,
            _sandbox_id: &str,
        ) -> alien_client_core::Result<()> {
            unreachable!("the command paths never resume")
        }

        async fn read_file(
            &self,
            _group: &str,
            _sandbox_id: &str,
            _path: &str,
        ) -> alien_client_core::Result<Vec<u8>> {
            unreachable!("the command paths never read files")
        }

        async fn write_file(
            &self,
            _group: &str,
            _sandbox_id: &str,
            _path: &str,
            _contents: Vec<u8>,
        ) -> alien_client_core::Result<()> {
            unreachable!("the command paths never write files")
        }

        async fn mkdir(
            &self,
            _group: &str,
            _sandbox_id: &str,
            _path: &str,
        ) -> alien_client_core::Result<()> {
            unreachable!("the command paths never create directories")
        }

        async fn create_sandbox(
            &self,
            _group: &str,
            _request: CreateSandbox,
        ) -> alien_client_core::Result<alien_azure_clients::azure::sandbox_data_plane::Sandbox>
        {
            unreachable!("the command paths never create")
        }

        async fn get_sandbox(
            &self,
            _group: &str,
            sandbox_id: &str,
        ) -> alien_client_core::Result<alien_azure_clients::azure::sandbox_data_plane::Sandbox>
        {
            if self.deleted.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(http_error(404, "SandboxNotFound"));
            }
            Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
                id: sandbox_id.to_string(),
                egress_policy: None,
                state: Some("Running".to_string()),
            })
        }

        async fn delete_sandbox(
            &self,
            _group: &str,
            _sandbox_id: &str,
        ) -> alien_client_core::Result<()> {
            self.deleted
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        async fn execute_shell_command(
            &self,
            _group: &str,
            _sandbox_id: &str,
            command: &str,
            _working_directory: Option<String>,
        ) -> alien_client_core::Result<alien_azure_clients::azure::sandbox_data_plane::ExecResult>
        {
            self.commands
                .lock()
                .expect("commands lock")
                .push(command.to_string());
            let next = self.results.lock().expect("results lock").pop_front();
            match next {
                // A scripted `DEADLINE_PLACEHOLDER` stands for "the wrapper fired": the double
                // answers with the marker the provider itself put in the program, which is the
                // only way a test can produce one — the nonce is made per command.
                Some(result) => Ok(ExecResult {
                    stderr: as_session_stderr(&result.stderr),
                    ..result
                }),
                None => std::future::pending().await,
            }
        }
    }

    /// Stands in for the wrapper's kill in a scripted result.
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

    fn provider(client: std::sync::Arc<ScriptedExec>) -> AzureSandbox {
        AzureSandbox::new(
            client,
            "grp".to_string(),
            "ubuntu".to_string(),
            SandboxEgress::Allow,
            None,
            "1000m".to_string(),
            "2048Mi".to_string(),
        )
    }

    fn command(deadline_secs: u64) -> RunCommandRequest {
        RunCommandRequest {
            command: vec!["sleep".to_string(), "forever".to_string()],
            working_directory: None,
            env: BTreeMap::new(),
            deadline: std::time::Duration::from_secs(deadline_secs),
        }
    }

    /// The deadline is enforced inside the session: the command is wrapped in `timeout`, and
    /// when it fires the output is kept, the stream ends in `deadlineExceeded`, and the session
    /// is not touched — the caller can keep using it, as on the agent-supervised backends.
    ///
    /// The wrapper reports its own kill, so the fake answers with that report rather than the
    /// test leaning on timing.
    #[tokio::test]
    async fn a_command_past_its_deadline_is_killed_in_place_and_the_session_survives() {
        let client = ScriptedExec::new(vec![ScriptedExec::exec_result(
            137,
            "partial\n",
            DEADLINE_PLACEHOLDER,
        )]);
        let sandbox = provider(client.clone());

        let frames: Vec<Result<CommandOutput>> = sandbox
            .run_command("s1", command(30))
            .await
            .expect("the call itself succeeds; the deadline is reported in the stream")
            .collect()
            .await;

        assert!(
            matches!(&frames[0], Ok(CommandOutput::Stdout { data, .. }) if data == b"partial\n"),
            "output produced before the deadline is kept: {frames:?}"
        );
        let terminal = frames
            .last()
            .expect("frames")
            .as_ref()
            .expect_err("the stream must end in the deadline error, not an exit frame");
        assert!(
            terminal.to_string().contains("deadlineExceeded"),
            "the caller has to be able to tell this apart from a command that failed: {terminal}"
        );
        assert!(
            !client.deleted.load(std::sync::atomic::Ordering::SeqCst),
            "the session survives an in-session kill"
        );
        let sent = client.commands.lock().expect("commands lock").clone();
        assert_eq!(sent.len(), 1, "one command: {sent:?}");
        assert!(
            sent[0].starts_with("sh -c '") && sent[0].ends_with("' sh 'sleep' 'forever'"),
            "the command is passed as arguments, not pasted into the program: {}",
            sent[0]
        );
        assert!(sent[0].contains("sleep 30"), "{}", sent[0]);
    }

    /// 124 is an ordinary exit status. Without the wrapper's report the command exited on its
    /// own, and saying otherwise would tell the caller its command was killed.
    #[tokio::test]
    async fn a_command_exiting_124_of_its_own_accord_is_an_exit_not_a_deadline() {
        let client = ScriptedExec::new(vec![ScriptedExec::exec_result(124, "done\n", "")]);
        let sandbox = provider(client.clone());

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

    /// When the session cannot end the command — exec never returns — the guard ends the
    /// session, and reports the deadline only once the session is confirmed gone: on this path
    /// untrusted code is known to be running past its deadline. Time is paused, so the guard and
    /// the confirmation polls arrive instantly.
    #[tokio::test(start_paused = true)]
    async fn a_command_the_session_cannot_end_takes_the_session_with_it() {
        let client = ScriptedExec::new(Vec::new());
        let sandbox = provider(client.clone());

        let error = sandbox
            .run_command("s1", command(30))
            .await
            .err()
            .expect("a command that outran its deadline has not succeeded");

        assert!(
            error.to_string().contains("deadlineExceeded"),
            "the caller has to be able to tell this apart from a command that failed: {error}"
        );
        assert!(
            client.deleted.load(std::sync::atomic::Ordering::SeqCst),
            "the session must actually be deleted, not merely reported as terminated"
        );
    }

    /// Every argument survives quoting as itself: an operator or a space inside one is data,
    /// because the shell receives it as an argument rather than as program text.
    #[test]
    fn the_bounded_shell_passes_arguments_untouched() {
        let wrapped = bounded_shell(
            &[
                "echo".to_string(),
                "it's".to_string(),
                "&&".to_string(),
                "sleep 5".to_string(),
            ],
            &BTreeMap::new(),
            std::time::Duration::from_millis(1500),
        );
        assert!(wrapped.contains("sleep 1.500"), "{wrapped}");
        assert!(
            wrapped.ends_with("' sh 'echo' 'it'\\''s' '&&' 'sleep 5'"),
            "{wrapped}"
        );
    }

    /// A per-command variable reaches the command, and its value stays data.
    ///
    /// The exec endpoint takes no environment, so the assignment travels in the shell string —
    /// which is exactly where an unquoted value would stop being a value.
    #[test]
    fn the_bounded_shell_carries_variables_as_data() {
        let wrapped = bounded_shell(
            &["printenv".to_string(), "TOKEN".to_string()],
            &BTreeMap::from([("TOKEN".to_string(), "a'; rm -rf /".to_string())]),
            std::time::Duration::from_millis(1500),
        );

        assert!(
            wrapped.ends_with("' sh 'env' 'TOKEN=a'\\''; rm -rf /' '--' 'printenv' 'TOKEN'"),
            "the value has to survive as one argument to env: {wrapped}"
        );
    }

    /// A caller's `PATH` reaches the command and not the wrapper that bounds it.
    ///
    /// The wrapper resolves `setsid`, `od`, `sleep` and `kill` through `PATH`. A caller able to
    /// set it on the wrapper's own shell could hand it no-ops, and the deadline that keeps
    /// untrusted code bounded would never fire.
    #[test]
    fn a_caller_cannot_repoint_the_wrappers_own_path() {
        let wrapped = bounded_shell(
            &["sleep".to_string(), "forever".to_string()],
            &BTreeMap::from([("PATH".to_string(), "/tmp/attacker".to_string())]),
            std::time::Duration::from_millis(1500),
        );

        let (wrapper, argv) = wrapped
            .split_once("' sh ")
            .expect("the wrapper's program ends where its arguments begin");
        assert!(
            !wrapper.contains("PATH"),
            "the wrapper has to resolve its own tools: {wrapper}"
        );
        assert_eq!(
            argv, "'env' 'PATH=/tmp/attacker' '--' 'sleep' 'forever'",
            "the variable belongs to the command, not to the shell that bounds it"
        );
    }

    /// A name the shell would read as a second command never reaches the shell string.
    #[test]
    fn a_variable_name_that_is_not_a_name_is_refused() {
        for name in ["", "A B", "A;rm", "1A", "A=B", "A-B"] {
            let error = checked_env_name("sandbox.runCommand", name)
                .expect_err("a name the shell would not read as a name must be refused");
            assert_eq!(error.code, "INVALID_INPUT", "name '{name}': {error}");
        }
        for name in ["A", "_a", "TOKEN_1"] {
            checked_env_name("sandbox.runCommand", name)
                .unwrap_or_else(|error| panic!("name '{name}' is a shell name: {error}"));
        }
    }

    /// A path that could leave the caller's own directory is refused before anything is sent.
    ///
    /// Asserted on the client never being called, not on the error: the data plane's own path
    /// handling is undocumented, so a request that leaves this process is already outside what
    /// this backend can promise.
    #[tokio::test]
    async fn a_path_that_could_escape_never_reaches_the_data_plane() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_read_file().never();
        client.expect_write_file().never();
        client.expect_mkdir().never();
        let sandbox = sandbox_with(client);

        for path in [
            "../etc/shadow",
            "",
            "/",
            "work/",
            "a//b",
            "a/../../b",
            "/../escape",
        ] {
            let error = sandbox
                .read_file("s1", path)
                .await
                .expect_err(&format!("'{path}' must be refused"));
            assert_eq!(error.code, "INVALID_INPUT", "{path}: {error}");

            sandbox
                .write_files("s1", BTreeMap::from([(path.to_string(), vec![1u8])]))
                .await
                .expect_err(&format!("'{path}' must be refused on write too"));
            sandbox
                .mkdir("s1", path)
                .await
                .expect_err(&format!("'{path}' must be refused on mkdir too"));
        }

        // The same shapes, accepted: a rule that refuses everything would pass the loop above.
        // An absolute path is one of them — it means "under the session's own root" on every
        // other backend, and arrives at the data plane with the leading slash trimmed.
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_read_file()
            .withf(|_, _, path| !path.starts_with('/'))
            .times(3)
            .returning(|_, _, _| Ok(Vec::new()));
        let sandbox = sandbox_with(client);
        for path in ["app.py", "src/app.py", "/work/app.py"] {
            sandbox
                .read_file("s1", path)
                .await
                .unwrap_or_else(|error| panic!("'{path}' is a normal path: {error}"));
        }
    }

    /// The group, the session and the path each reach the call they belong to, and the bytes come
    /// back unchanged.
    #[tokio::test]
    async fn a_read_carries_the_session_and_path_to_the_data_plane() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_read_file()
            .withf(|group, session_id, path| {
                group == "grp" && session_id == "s1" && path == "src/app.py"
            })
            .times(1)
            .returning(|_, _, _| Ok(b"print(1)\n".to_vec()));

        let contents = sandbox_with(client)
            .read_file("s1", "src/app.py")
            .await
            .expect("the read should succeed");

        assert_eq!(contents, b"print(1)\n");
    }

    /// One bad path fails the batch before anything is written.
    ///
    /// Partial application is the contract for a data plane that refuses midway — not for a path
    /// this process could have refused before the first request.
    #[tokio::test]
    async fn a_batch_with_an_unusable_path_writes_nothing() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_write_file().never();

        let error = sandbox_with(client)
            .write_files(
                "s1",
                BTreeMap::from([
                    ("a.txt".to_string(), vec![1u8]),
                    ("b/../../escape".to_string(), vec![2u8]),
                ]),
            )
            .await
            .expect_err("a path that could escape must fail the batch");

        assert_eq!(error.code, "INVALID_INPUT", "{error}");
    }

    /// Writing stops at the first failure rather than pressing on, which is what makes a partial
    /// write observable to the caller instead of a success with a hole in it.
    #[tokio::test]
    async fn a_failed_write_stops_the_ones_behind_it() {
        let mut client = MockSandboxDataPlaneApi::new();
        settles_running(&mut client, None);
        client
            .expect_write_file()
            .times(1)
            .returning(|_, _, path, _| {
                assert_eq!(path, "a.txt", "the first path in order is the one attempted");
                Err(AlienError::new(ClientErrorData::RemoteAccessDenied {
                    resource_type: "sandbox".to_string(),
                    resource_name: "s1".to_string(),
                }))
            });

        let error = sandbox_with(client)
            .write_files(
                "s1",
                BTreeMap::from([
                    ("a.txt".to_string(), vec![1u8]),
                    ("b.txt".to_string(), vec![2u8]),
                ]),
            )
            .await
            .expect_err("a refused write must fail the call");

        assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "{error}");
    }

    /// The two buckets a caller retries on, and the one it must not.
    ///
    /// A refusal repeated is refused again, and a file operation whose outcome is unknown is safe
    /// to repeat — but a command may already be running, and a retry there runs it twice.
    #[tokio::test]
    async fn only_the_operations_that_are_safe_to_repeat_are_marked_retryable() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_read_file().times(1).returning(|_, _, _| {
            Err(AlienError::new(ClientErrorData::RemoteResourceNotFound {
                resource_type: "file".to_string(),
                resource_name: "missing.txt".to_string(),
            }))
        });
        let refused = sandbox_with(client)
            .read_file("s1", "missing.txt")
            .await
            .expect_err("a missing file is an error");
        assert_eq!(refused.code, "SANDBOX_COMMAND_FAILED", "{refused}");
        assert!(!refused.retryable, "repeating a refusal repeats it: {refused}");

        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_read_file().times(1).returning(|_, _, _| {
            Err(AlienError::new(ClientErrorData::RemoteServiceUnavailable {
                message: "the data plane is unavailable".to_string(),
            }))
        });
        let unreachable = sandbox_with(client)
            .read_file("s1", "app.py")
            .await
            .expect_err("an unavailable data plane is an error");
        assert_eq!(unreachable.code, "SANDBOX_UNREACHABLE", "{unreachable}");
        assert!(unreachable.retryable, "a read is safe to repeat: {unreachable}");

        let mut client = MockSandboxDataPlaneApi::new();
        settles_running(&mut client, None);
        client
            .expect_execute_shell_command()
            .times(1)
            .returning(|_, _, _, _| {
                Err(AlienError::new(ClientErrorData::RemoteServiceUnavailable {
                    message: "the data plane is unavailable".to_string(),
                }))
            });
        let command = match sandbox_with(client).run_command("s1", command(5)).await {
            Ok(_) => panic!("an unavailable data plane is an error"),
            Err(error) => error,
        };
        assert_eq!(command.code, "SANDBOX_COMMAND_FAILED", "{command}");
        assert!(
            !command.retryable,
            "the command may already be running, so a retry would run it twice: {command}"
        );
    }

    /// A session's state is the data plane's, not a default.
    ///
    /// The four states that are not `Running` each mean a command sent now does not run, so
    /// reporting `Running` for any of them tells a caller to use a session that cannot answer.
    #[tokio::test]
    async fn a_session_reports_the_state_the_data_plane_gave_it() {
        for (reported, expected) in [
            ("Running", SandboxSessionState::Running),
            ("Creating", SandboxSessionState::Starting),
            ("Resuming", SandboxSessionState::Starting),
            // On its way down, and the four states the trait publishes have no word for it.
            ("Stopping", SandboxSessionState::Suspended),
            ("Stopped", SandboxSessionState::Suspended),
            ("Suspended", SandboxSessionState::Suspended),
            ("Idle", SandboxSessionState::Suspended),
            ("Deleting", SandboxSessionState::Terminated),
        ] {
            let mut client = MockSandboxDataPlaneApi::new();
            let state = reported.to_string();
            client.expect_get_sandbox().times(1).returning(move |_, _| {
                Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
                    id: "s1".to_string(),
                    egress_policy: None,
                    state: Some(state.clone()),
                })
            });

            let session = sandbox_with(client)
                .get("s1")
                .await
                .unwrap_or_else(|error| panic!("{reported}: {error}"))
                .unwrap_or_else(|| panic!("{reported}: the session exists"));

            assert_eq!(session.state, expected, "state {reported}");
        }
    }

    /// A state this client does not know is a preview API that moved, and guessing which of the
    /// four it maps to is how a caller ends up talking to a sandbox that is going away.
    #[tokio::test]
    async fn an_unknown_state_is_an_error_rather_than_a_guess() {
        for reported in [Some("Hibernated"), None] {
            let mut client = MockSandboxDataPlaneApi::new();
            let state = reported.map(str::to_string);
            client.expect_get_sandbox().times(1).returning(move |_, _| {
                Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
                    id: "s1".to_string(),
                    egress_policy: None,
                    state: state.clone(),
                })
            });

            let error = sandbox_with(client)
                .get("s1")
                .await
                .expect_err("an unreadable state must not become a session");

            assert_eq!(error.code, "UNEXPECTED_RESPONSE_FORMAT", "{error}");
        }
    }

    /// The variables the caller declared have to reach the create body: a sandbox inherits none
    /// of them, and the data plane accepts a create that omits them.
    #[tokio::test]
    async fn the_declared_variables_reach_the_create_call() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_create_sandbox()
            .withf(|_, request| request.environment.get("TOKEN").map(String::as_str) == Some("t"))
            .times(1)
            .returning(|_, _| {
                Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
                    id: "s1".to_string(),
                    egress_policy: None,
                    state: Some("Creating".to_string()),
                })
            });

        // Created as `Creating`, so the create waits: the trait owes the caller a session that
        // can already take work, and returning one that cannot pushes the readiness poll into
        // every caller.
        client
            .expect_get_sandbox()
            .times(1)
            .returning(|_, _| Ok(running("s1", None)));

        let session = sandbox_with(client)
            .create(CreateSessionRequest {
                session_id: None,
                tenant_key: None,
                env: BTreeMap::from([("TOKEN".to_string(), "t".to_string())]),
            })
            .await
            .expect("the create should succeed");

        assert_eq!(session.state, SandboxSessionState::Running);
    }

    fn running(
        id: &str,
        egress: Option<EgressPolicy>,
    ) -> alien_azure_clients::azure::sandbox_data_plane::Sandbox {
        alien_azure_clients::azure::sandbox_data_plane::Sandbox {
            id: id.to_string(),
            egress_policy: egress,
            state: Some("Running".to_string()),
        }
    }

    fn sandbox_denying(client: MockSandboxDataPlaneApi, egress: SandboxEgress) -> AzureSandbox {
        AzureSandbox::new(
            std::sync::Arc::new(client),
            "grp".to_string(),
            "ubuntu".to_string(),
            egress,
            None,
            "1000m".to_string(),
            "2048Mi".to_string(),
        )
    }

    /// What each declared mode is created with.
    ///
    /// The inspection mode is the half that is easy to leave out and impossible to notice: under
    /// anything but `Full` a `Deny` default still lets every non-HTTP protocol out, so a sandbox
    /// would carry the label and none of the containment. `allow` must send no policy, because
    /// `Full` would block the traffic `allow` promises.
    #[tokio::test]
    async fn each_declared_mode_is_created_with_the_policy_that_realises_it() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, request| {
                let policy = request.egress.expect("deny must send a policy");
                assert_eq!(policy.default_action, "Deny");
                assert_eq!(
                    policy.traffic_inspection.as_deref(),
                    Some("Full"),
                    "only Full inspection blocks non-HTTP traffic"
                );
                assert_eq!(
                    policy.host_rules,
                    vec![EgressHostRule {
                        pattern: "*".to_string(),
                        action: "Deny".to_string(),
                    }],
                    "deny is written as a rule too, so it does not rest on how the proxy treats a \
                     policy with no rules"
                );
                Ok(running("s1", Some(policy)))
            });
        settles_running(
            &mut client,
            Some(EgressPolicy {
                default_action: "Deny".to_string(),
                host_rules: vec![EgressHostRule {
                    pattern: "*".to_string(),
                    action: "Deny".to_string(),
                }],
                rules: Vec::new(),
                unmodelled: Default::default(),
                traffic_inspection: Some("Full".to_string()),
            }),
        );
        sandbox_denying(client, SandboxEgress::Deny)
            .create(CreateSessionRequest::default())
            .await
            .expect("deny should create");

        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, request| {
                let policy = request.egress.expect("allowDomains must send a policy");
                assert_eq!(policy.default_action, "Deny", "anything unlisted is denied");
                assert_eq!(policy.traffic_inspection.as_deref(), Some("Full"));
                assert_eq!(
                    policy.host_rules,
                    vec![EgressHostRule {
                        pattern: "api.example.com".to_string(),
                        action: "Allow".to_string(),
                    }]
                );
                Ok(running("s1", Some(policy)))
            });
        settles_running(
            &mut client,
            Some(EgressPolicy {
                default_action: "Deny".to_string(),
                host_rules: vec![EgressHostRule {
                    pattern: "api.example.com".to_string(),
                    action: "Allow".to_string(),
                }],
                rules: Vec::new(),
                unmodelled: Default::default(),
                traffic_inspection: Some("Full".to_string()),
            }),
        );
        sandbox_denying(
            client,
            SandboxEgress::AllowDomains {
                domains: vec!["api.example.com".to_string()],
            },
        )
        .create(CreateSessionRequest::default())
        .await
        .expect("allowDomains should create");

        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, request| {
                assert!(
                    request.egress.is_none(),
                    "an open sandbox sends no policy: Full inspection would block non-HTTP traffic"
                );
                Ok(running("s1", None))
            });
        settles_running(&mut client, None);
        sandbox_denying(client, SandboxEgress::Allow)
            .create(CreateSessionRequest::default())
            .await
            .expect("allow should create");
    }

    /// A restriction that did not take effect is the failure this whole path exists to prevent,
    /// so the sandbox is deleted rather than returned with a `deny` label and a live network.
    #[tokio::test]
    async fn a_sandbox_that_came_up_without_its_policy_is_deleted_rather_than_handed_back() {
        for came_up_with in [
            None,
            // The default action alone: every non-HTTP protocol still leaves.
            Some(EgressPolicy {
                default_action: "Deny".to_string(),
                unmodelled: Default::default(),
                rules: Vec::new(),
                host_rules: Vec::new(),
                traffic_inspection: Some("Partial".to_string()),
            }),
            // Inspected, and open.
            Some(EgressPolicy {
                default_action: "Allow".to_string(),
                unmodelled: Default::default(),
                rules: Vec::new(),
                host_rules: Vec::new(),
                traffic_inspection: Some("Full".to_string()),
            }),
        ] {
            let mut client = MockSandboxDataPlaneApi::new();
            let effective = came_up_with.clone();
            client
                .expect_create_sandbox()
                .times(1)
                .returning(move |_, _| Ok(running("s1", effective.clone())));
            settles_running(&mut client, came_up_with.clone());
            client
                .expect_delete_sandbox()
                .withf(|_, id| id == "s1")
                .times(1)
                .returning(|_, _| Ok(()));

            let error = sandbox_denying(client, SandboxEgress::Deny)
                .create(CreateSessionRequest::default())
                .await
                .expect_err("a sandbox without its policy must not be handed back");

            assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
        }
    }

    /// A host the declaration named that the sandbox is not running is the same failure as a
    /// missing policy: the caller believes traffic to it is allowed and it is not, or worse, the
    /// list came back holding something else.
    #[tokio::test]
    async fn a_missing_host_rule_fails_the_create() {
        let mut client = MockSandboxDataPlaneApi::new();
        let elsewhere = EgressPolicy {
            default_action: "Deny".to_string(),
            unmodelled: Default::default(),
            rules: Vec::new(),
            host_rules: vec![EgressHostRule {
                pattern: "elsewhere.example.com".to_string(),
                action: "Allow".to_string(),
            }],
            traffic_inspection: Some("Full".to_string()),
        };
        let echoed = elsewhere.clone();
        client
            .expect_create_sandbox()
            .times(1)
            .returning(move |_, _| Ok(running("s1", Some(echoed.clone()))));
        settles_running(&mut client, Some(elsewhere));
        client.expect_delete_sandbox().times(1).returning(|_, _| Ok(()));

        let error = sandbox_denying(
            client,
            SandboxEgress::AllowDomains {
                domains: vec!["api.example.com".to_string()],
            },
        )
        .create(CreateSessionRequest::default())
        .await
        .expect_err("a host the declaration named must be in the effective policy");

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A session that is going away is not one to reconnect to.
    ///
    /// `get_or_create` hands back whatever `get` finds, and the id of a deleting sandbox will not
    /// run again — so the caller would receive a handle whose every command lands on nothing.
    #[tokio::test]
    async fn a_terminated_session_is_replaced_rather_than_reconnected_to() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().times(1).returning(|_, id| {
            Ok(running(id, None)).map(
                |mut sandbox: alien_azure_clients::azure::sandbox_data_plane::Sandbox| {
                    sandbox.state = Some("Deleting".to_string());
                    sandbox
                },
            )
        });
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, request| Ok(running("fresh", request.egress)));
        settles_running(
            &mut client,
            Some(EgressPolicy {
                default_action: "Deny".to_string(),
                host_rules: vec![EgressHostRule {
                    pattern: "*".to_string(),
                    action: "Deny".to_string(),
                }],
                rules: Vec::new(),
                unmodelled: Default::default(),
                traffic_inspection: Some("Full".to_string()),
            }),
        );

        // Declared `deny`, because a terminated session carries no policy — judging it before
        // reading the state reported a disappearing sandbox as an uncontained one.
        let session = sandbox_denying(client, SandboxEgress::Deny)
            .get_or_create(CreateSessionRequest {
                session_id: Some("going-away".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("a new session should be created");

        assert_eq!(session.session_id, "fresh");
    }

    /// A permission the declaration never asked for fails the create as surely as a missing one.
    ///
    /// The check looks outward as well as inward: an `Allow` the sandbox holds and the caller did
    /// not name is the whole failure this path exists to catch, and a group-scoped policy is a
    /// documented way for one to appear.
    #[tokio::test]
    async fn a_permission_nobody_asked_for_fails_the_create() {
        let asked_for = || SandboxEgress::AllowDomains {
            domains: vec!["api.example.com".to_string()],
        };
        let declared = EgressHostRule {
            pattern: "api.example.com".to_string(),
            action: "Allow".to_string(),
        };

        for came_up_with in [
            // A second host, allowed.
            EgressPolicy {
                default_action: "Deny".to_string(),
                unmodelled: Default::default(),
                host_rules: vec![
                    declared.clone(),
                    EgressHostRule {
                        pattern: "exfil.example.com".to_string(),
                        action: "Allow".to_string(),
                    },
                ],
                rules: Vec::new(),
                traffic_inspection: Some("Full".to_string()),
            },
            // Everything, through the list this client never writes.
            EgressPolicy {
                default_action: "Deny".to_string(),
                unmodelled: Default::default(),
                host_rules: vec![declared.clone()],
                rules: vec![EgressRule {
                    name: None,
                    r#match: Some(EgressRuleMatch {
                        host: "*".to_string(),
                        path: None,
                        methods: None,
                    }),
                    action: Some(EgressRuleAction {
                        action_type: "Allow".to_string(),
                        host: None,
                        path: None,
                        scheme: None,
                        headers: None,
                    }),
                }],
                traffic_inspection: Some("Full".to_string()),
            },
        ] {
            let mut client = MockSandboxDataPlaneApi::new();
            let effective = came_up_with.clone();
            client
                .expect_create_sandbox()
                .times(1)
                .returning(move |_, _| Ok(running("s1", Some(effective.clone()))));
            settles_running(&mut client, Some(came_up_with.clone()));
            client.expect_delete_sandbox().times(1).returning(|_, _| Ok(()));

            let error = sandbox_denying(client, asked_for())
                .create(CreateSessionRequest::default())
                .await
                .expect_err("a permission nobody asked for must fail the create");

            assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
        }

        // The same policy without the extra permission creates normally, so the rule above is
        // refusing the addition rather than refusing everything.
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_create_sandbox().times(1).returning(move |_, _| {
            Ok(running(
                "s1",
                Some(EgressPolicy {
                    default_action: "Deny".to_string(),
                    unmodelled: Default::default(),
                    host_rules: vec![EgressHostRule {
                        pattern: "api.example.com".to_string(),
                        action: "Allow".to_string(),
                    }],
                    rules: Vec::new(),
                    traffic_inspection: Some("Full".to_string()),
                }),
            ))
        });
        settles_running(
            &mut client,
            Some(EgressPolicy {
                default_action: "Deny".to_string(),
                unmodelled: Default::default(),
                host_rules: vec![EgressHostRule {
                    pattern: "api.example.com".to_string(),
                    action: "Allow".to_string(),
                }],
                rules: Vec::new(),
                traffic_inspection: Some("Full".to_string()),
            }),
        );
        sandbox_denying(client, asked_for())
            .create(CreateSessionRequest::default())
            .await
            .expect("the policy that was asked for should create");
    }

    /// Suspend and resume are one call each, and each has to reach the verb it names.
    ///
    /// Returning on acceptance rather than on the state change is the same contract AWS follows,
    /// so a caller that needs the session stopped polls `get` — the alternative is a call that
    /// blocks for a resume Microsoft describes as sub-second and a stop that is not.
    #[tokio::test]
    async fn suspend_and_resume_reach_their_own_verbs() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_stop_sandbox()
            .withf(|group, id| group == "grp" && id == "s1")
            .times(1)
            .returning(|_, _| Ok(()));
        client.expect_resume_sandbox().never();
        sandbox_with(client)
            .suspend("s1")
            .await
            .expect("suspend should be accepted");

        // Found asleep, so the verb is actually sent — a mock that answers `Running` on the
        // first read would let this pass with `resume_sandbox` never called at all.
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            if reads < 3 {
                sandbox.state = Some("Stopped".to_string());
            }
            Ok(sandbox)
        });
        client
            .expect_resume_sandbox()
            .withf(|group, id| group == "grp" && id == "s1")
            .times(1)
            .returning(|_, _| Ok(()));
        client.expect_stop_sandbox().never();
        sandbox_with(client)
            .resume("s1")
            .await
            .expect("resume should reach a running session");
    }

    /// A declared idle-suspend policy has to reach the create body.
    ///
    /// The data plane takes it at create and nowhere else, and accepts a body without it — so a
    /// declaration that stops at the binding leaves the sandbox on whatever the service defaults
    /// to, with nothing anywhere saying the number was ignored.
    #[tokio::test]
    async fn a_declared_idle_suspend_reaches_the_create_call() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_create_sandbox()
            .withf(|_, request| request.idle_suspend_seconds == Some(900))
            .times(1)
            .returning(|_, _| Ok(running("s1", None)));
        settles_running(&mut client, None);

        AzureSandbox::new(
            std::sync::Arc::new(client),
            "grp".to_string(),
            "ubuntu".to_string(),
            SandboxEgress::Allow,
            Some(900),
            "1000m".to_string(),
            "2048Mi".to_string(),
        )
        .create(CreateSessionRequest::default())
        .await
        .expect("the create should succeed");
    }

    /// Reconnect is the path a stale policy survives on.
    ///
    /// Azure has no session ceiling and an idle sandbox only suspends, so one created under an
    /// older declaration outlives the change. Checking only at create hands the caller a session
    /// whose containment is whatever it was built with, under the label it has now.
    #[tokio::test]
    async fn a_reconnect_to_a_session_built_under_another_policy_is_refused() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().times(1).returning(|_, id| {
            // What an `allow` declaration built, before it was changed to `deny`.
            Ok(running(id, None))
        });

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .get("built-under-allow")
            .await
            .expect_err("a session without the declared policy must not be handed back");

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A create whose response cannot be read owns a sandbox the caller has no id for.
    ///
    /// Azure allocates the id and has no enumeration verb, so an abandoned sandbox has no
    /// id-holder and nothing to reap it — it runs until someone finds it by hand.
    #[tokio::test]
    async fn a_create_that_cannot_be_read_deletes_what_it_made() {
        let mut client = MockSandboxDataPlaneApi::new();
        let unreadable = || {
            Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
                id: "orphan".to_string(),
                egress_policy: None,
                state: Some("Hibernated".to_string()),
            })
        };
        client.expect_create_sandbox().times(1).returning(move |_, _| unreadable());
        client.expect_get_sandbox().returning(move |_, _| unreadable());
        client
            .expect_delete_sandbox()
            .withf(|_, id| id == "orphan")
            .times(1)
            .returning(|_, _| Ok(()));

        let error = sandbox_with(client)
            .create(CreateSessionRequest::default())
            .await
            .expect_err("an unreadable state must fail the create");

        assert_eq!(error.code, "UNEXPECTED_RESPONSE_FORMAT", "{error}");
    }

    /// The three shapes a permitting policy can arrive in that a looser check would pass.
    #[tokio::test]
    async fn a_policy_this_client_cannot_read_whole_fails_the_create() {
        let declared = || SandboxEgress::Deny;
        let catch_all = EgressHostRule {
            pattern: "*".to_string(),
            action: "Deny".to_string(),
        };

        for came_up_with in [
            // A host rule carrying an action this client cannot weigh: `Transform` reaches a host
            // by rewriting the request rather than by naming it.
            EgressPolicy {
                default_action: "Deny".to_string(),
                host_rules: vec![
                    catch_all.clone(),
                    EgressHostRule {
                        pattern: "api.example.com".to_string(),
                        action: "Transform".to_string(),
                    },
                ],
                rules: Vec::new(),
                unmodelled: Default::default(),
                traffic_inspection: Some("Full".to_string()),
            },
            // A field this client does not model at all.
            EgressPolicy {
                default_action: "Deny".to_string(),
                host_rules: vec![catch_all.clone()],
                rules: Vec::new(),
                unmodelled: BTreeMap::from([(
                    "bypassList".to_string(),
                    serde_json::json!(["exfil.example.com"]),
                )]),
                traffic_inspection: Some("Full".to_string()),
            },
        ] {
            let mut client = MockSandboxDataPlaneApi::new();
            let effective = came_up_with.clone();
            client
                .expect_create_sandbox()
                .times(1)
                .returning(move |_, _| Ok(running("s1", Some(effective.clone()))));
            settles_running(&mut client, Some(came_up_with.clone()));
            client.expect_delete_sandbox().times(1).returning(|_, _| Ok(()));

            let error = sandbox_denying(client, declared())
                .create(CreateSessionRequest::default())
                .await
                .expect_err("a policy this client cannot read whole must fail the create");

            assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
        }

        // Case is the data plane's to choose: the same policy, normalised, still creates.
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_create_sandbox().times(1).returning(|_, _| {
            Ok(running(
                "s1",
                Some(EgressPolicy {
                    default_action: "deny".to_string(),
                    host_rules: vec![EgressHostRule {
                        pattern: "*".to_string(),
                        action: "deny".to_string(),
                    }],
                    rules: Vec::new(),
                    unmodelled: Default::default(),
                    traffic_inspection: Some("full".to_string()),
                }),
            ))
        });
        settles_running(
            &mut client,
            Some(EgressPolicy {
                default_action: "deny".to_string(),
                host_rules: vec![EgressHostRule {
                    pattern: "*".to_string(),
                    action: "deny".to_string(),
                }],
                rules: Vec::new(),
                unmodelled: Default::default(),
                traffic_inspection: Some("full".to_string()),
            }),
        );
        sandbox_denying(client, declared())
            .create(CreateSessionRequest::default())
            .await
            .expect("a normalised echo of the same policy is the same policy");
    }

    /// A session the declaration no longer matches is replaced, not a permanent error.
    ///
    /// `get_or_create` owes the caller a usable session, and a stale-policy sandbox is as
    /// unusable as a terminated one. The old sandbox is left running: another revision of the
    /// same stack may share this group, and the replacement is what this caller asked for.
    #[tokio::test]
    async fn a_stale_policy_session_is_replaced_rather_than_refused_forever() {
        let mut client = MockSandboxDataPlaneApi::new();
        // The stale session is running under no policy at all; the replacement carries the one
        // the declaration asks for.
        client.expect_get_sandbox().returning(move |_, id| {
            if id == "built-under-allow" {
                return Ok(running(id, None));
            }
            Ok(running(
                id,
                Some(EgressPolicy {
                    default_action: "Deny".to_string(),
                    host_rules: vec![EgressHostRule {
                        pattern: "*".to_string(),
                        action: "Deny".to_string(),
                    }],
                    rules: Vec::new(),
                    unmodelled: Default::default(),
                    traffic_inspection: Some("Full".to_string()),
                }),
            ))
        });
        client.expect_delete_sandbox().never();
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, request| Ok(running("fresh", request.egress)));

        let session = sandbox_denying(client, SandboxEgress::Deny)
            .get_or_create(CreateSessionRequest {
                session_id: Some("built-under-allow".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("a stale session is replaced");

        assert_eq!(session.session_id, "fresh");
    }

    /// A session id is one path segment, because it is interpolated into the data-plane URL and
    /// `..` in a URL resolves — reaching a sandbox group this binding was never scoped to.
    #[tokio::test]
    async fn a_traversing_session_id_never_reaches_the_data_plane() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().never();
        client.expect_delete_sandbox().never();
        client.expect_execute_shell_command().never();
        let sandbox = sandbox_with(client);

        for id in ["../../other-group/sandboxes/theirs", "a/b", "", "has space"] {
            assert_eq!(
                sandbox
                    .get(id)
                    .await
                    .expect_err(&format!("'{id}' must be refused"))
                    .code,
                "INVALID_INPUT"
            );
            sandbox
                .terminate(id)
                .await
                .expect_err(&format!("'{id}' must be refused on every verb"));
        }
    }

    /// A stale session cannot run code, which is the one verb where it matters most.
    ///
    /// An id outlives a declaration change and the SDK hands `runCommand` an arbitrary string, so
    /// without this the containment check is one a caller can walk around by keeping an id.
    #[tokio::test]
    async fn a_stale_policy_session_cannot_run_a_command() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_get_sandbox()
            .times(1)
            .returning(|_, id| Ok(running(id, None)));
        // Refused, not reaped: this call did not create the session and was not asked to replace
        // it, and two revisions of a stack share a sandbox group.
        client.expect_delete_sandbox().never();
        client.expect_execute_shell_command().never();

        let error = match sandbox_denying(client, SandboxEgress::Deny)
            .run_command("built-under-allow", command(5))
            .await
        {
            Ok(_) => panic!("a session without the declared policy must not run code"),
            Err(error) => error,
        };

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A policy that changed while a session was suspended is caught on the way back.
    ///
    /// The effective policy can be set on the group, somewhere this binding never writes, so the
    /// read that finds a stopped sandbox is not the read that decides whether it is contained —
    /// the one taken after it comes up is.
    #[tokio::test]
    async fn a_policy_that_changed_during_suspension_is_caught_on_reconnect() {
        let declared = EgressPolicy {
            default_action: "Deny".to_string(),
            host_rules: vec![EgressHostRule {
                pattern: "*".to_string(),
                action: "Deny".to_string(),
            }],
            rules: Vec::new(),
            unmodelled: Default::default(),
            traffic_inspection: Some("Full".to_string()),
        };

        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        let stopped = declared.clone();
        client.expect_get_sandbox().returning(move |_, id| {
            // The replacement is compliant; only the session that was asleep woke up wider.
            if id != "was-suspended" {
                return Ok(running(id, Some(stopped.clone())));
            }
            reads += 1;
            Ok(match reads {
                // Suspended and compliant for the reconnect's read and the wait's first poll, so
                // the reconnect proceeds and the wait is what wakes it.
                1 | 2 => {
                    let mut sandbox = running(id, Some(stopped.clone()));
                    sandbox.state = Some("Stopped".to_string());
                    sandbox
                }
                // Awake, and the group gained a host nobody here asked for.
                _ => running(
                    id,
                    Some(EgressPolicy {
                        host_rules: vec![
                            EgressHostRule {
                                pattern: "*".to_string(),
                                action: "Deny".to_string(),
                            },
                            EgressHostRule {
                                pattern: "exfil.example.com".to_string(),
                                action: "Allow".to_string(),
                            },
                        ],
                        ..stopped.clone()
                    }),
                ),
            })
        });
        // Woken here, so this call owes the put-back: it is returned to the state it was found
        // in rather than destroyed, because another revision may hold the same id.
        client.expect_resume_sandbox().returning(|_, _| Ok(()));
        client
            .expect_stop_sandbox()
            .withf(|_, id| id == "was-suspended")
            .times(1)
            .returning(|_, _| Ok(()));
        client.expect_delete_sandbox().never();
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, request| Ok(running("fresh", request.egress)));

        let session = sandbox_denying(client, SandboxEgress::Deny)
            .get_or_create(CreateSessionRequest {
                session_id: Some("was-suspended".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("a caller asking for a session gets a usable one");

        // Answered the same way as a terminated id: the one that woke up wider is discarded and
        // replaced, rather than returned as an error the caller cannot act on.
        assert_eq!(session.session_id, "fresh");
    }

    /// A sandbox left behind must not publish the cloud's own response text.
    ///
    /// `discard` wraps the reason so the leak is named, and the wrapper inherits visibility: the
    /// error it wraps is the cloud client's, which carries the request and response of the call
    /// that failed, and the flag `into_external` reads is the outermost one.
    #[tokio::test]
    async fn a_sandbox_left_behind_does_not_publish_the_response_body() {
        const SECRET: &str = "tenant-only-detail";

        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, _| Ok(running("s1", None)));
        // The readiness read and the delete both fail, which is one failure in practice: a
        // missing data-plane role refuses every verb.
        client
            .expect_get_sandbox()
            .returning(|_, _| Err(http_error(403, SECRET)));
        client
            .expect_delete_sandbox()
            .returning(|_, _| Err(http_error(403, SECRET)));

        let error = sandbox_with(client)
            .create(CreateSessionRequest::default())
            .await
            .expect_err("a create that cannot be confirmed must fail");

        assert_eq!(error.code, "SANDBOX_COMMAND_FAILED", "{error}");
        assert!(
            error.internal,
            "the wrapper must inherit the cloud error's visibility: {error}"
        );
    }

    /// Waking a session puts what it was running back on the network, so it is gated like
    /// `run_command`: a caller holding an id from an older declaration must not be able to
    /// resume its way around the check.
    #[tokio::test]
    async fn a_stale_policy_session_cannot_be_resumed() {
        let mut client = MockSandboxDataPlaneApi::new();
        // Found asleep, so this call is what wakes it — and therefore what must put it back.
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            // Asleep for the resume's own read and the wait's first poll, so the wait is what
            // wakes it — and therefore what owes the put-back.
            if reads <= 2 {
                sandbox.state = Some("Stopped".to_string());
            }
            Ok(sandbox)
        });
        client.expect_resume_sandbox().returning(|_, _| Ok(()));
        // Refused, not reaped: the caller asked to wake a session, not to lose it. Put back,
        // because this call is what woke it.
        client.expect_delete_sandbox().never();
        client
            .expect_stop_sandbox()
            .times(1)
            .returning(|_, _| Ok(()));

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .resume("built-under-allow")
            .await
            .expect_err("a session without the declared policy must not be woken");

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A resume that finds the session already awake refuses without touching it.
    ///
    /// Two revisions of a stack share a sandbox group, so stopping a session this call did not
    /// wake ends whatever command the other revision is running. Refusing is this call's to do;
    /// suspending someone else's work is not.
    #[tokio::test]
    async fn a_session_this_call_did_not_wake_is_left_running() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_get_sandbox()
            .returning(|_, id| Ok(running(id, None)));
        client.expect_resume_sandbox().never();
        client.expect_stop_sandbox().never();
        client.expect_delete_sandbox().never();

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .resume("someone-elses-session")
            .await
            .expect_err("a session without the declared policy must not be handed back");

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A session that came up on its own is not this call's to suspend.
    ///
    /// A read taken before the wait sees `Creating` and calls that asleep, but nothing here woke
    /// it — another revision created it a moment earlier. Stopping it on a policy mismatch ends
    /// that revision's session; only refusing is this call's to do.
    #[tokio::test]
    async fn a_session_that_came_up_on_its_own_is_not_suspended() {
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            if reads <= 2 {
                sandbox.state = Some("Creating".to_string());
            }
            Ok(sandbox)
        });
        client.expect_resume_sandbox().never();
        client.expect_stop_sandbox().never();
        client.expect_delete_sandbox().never();

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .resume("created-by-another-revision")
            .await
            .expect_err("a session without the declared policy must not be handed back");

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A suspended session that reports no policy reads as suspended, not as a mismatch.
    ///
    /// Whether the data plane reports `egressPolicy` off `Running` is unverified; judging it
    /// here would turn every idle-suspended session into a containment failure.
    #[tokio::test]
    async fn a_suspended_session_reporting_no_policy_is_not_a_mismatch() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().times(1).returning(|_, id| {
            let mut sandbox = running(id, None);
            sandbox.state = Some("Stopped".to_string());
            Ok(sandbox)
        });

        let session = sandbox_denying(client, SandboxEgress::Deny)
            .get("asleep")
            .await
            .expect("a sleeping session must still be readable")
            .expect("the session exists");

        assert_eq!(session.state, SandboxSessionState::Suspended);
    }

    /// A sleeping session whose own record is plainly wrong is refused before anything wakes it.
    ///
    /// Waking it to reach the same verdict puts its workload back on the network for the length of
    /// a boot, which is the window this check exists to close.
    #[tokio::test]
    async fn a_sleeping_session_with_a_wrong_policy_is_never_woken() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().times(1).returning(|_, id| {
            let mut sandbox = running(
                id,
                Some(EgressPolicy {
                    default_action: "Allow".to_string(),
                    host_rules: Vec::new(),
                    rules: Vec::new(),
                    unmodelled: Default::default(),
                    traffic_inspection: Some("Full".to_string()),
                }),
            );
            sandbox.state = Some("Stopped".to_string());
            Ok(sandbox)
        });
        client.expect_resume_sandbox().never();
        client.expect_stop_sandbox().never();
        client.expect_delete_sandbox().never();

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .resume("built-under-allow")
            .await
            .expect_err("a stored policy that already fails must not be woken");

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A wait that woke a session and then failed still puts it back.
    ///
    /// The wait can fail after issuing the resume, and a session left awake by a call that
    /// returned an error is exactly the one nothing else will come back for.
    #[tokio::test]
    async fn a_session_woken_by_a_wait_that_then_failed_is_put_back() {
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            // Asleep for the resume's read and the wait's first poll, then unreadable.
            sandbox.state = Some(if reads <= 2 { "Stopped" } else { "Hibernated" }.to_string());
            Ok(sandbox)
        });
        client
            .expect_resume_sandbox()
            .times(1)
            .returning(|_, _| Ok(()));
        client
            .expect_stop_sandbox()
            .times(1)
            .returning(|_, _| Ok(()));

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .resume("wakes-then-breaks")
            .await
            .expect_err("a wait that cannot finish must not report a resumed session");

        assert_eq!(error.code, "UNEXPECTED_RESPONSE_FORMAT", "{error}");
    }

    /// A reconnect that woke a session and then could not use it puts back what it woke.
    ///
    /// The refusal travels either way; what must not survive it is a live sandbox this call put
    /// on the network and then walked away from. Returned to sleep rather than deleted, because
    /// the id may be another revision's.
    #[tokio::test]
    async fn a_session_woken_by_a_failed_reconnect_is_put_back() {
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            sandbox.state = Some(if reads <= 2 { "Stopped" } else { "Hibernated" }.to_string());
            Ok(sandbox)
        });
        client
            .expect_resume_sandbox()
            .times(1)
            .returning(|_, _| Ok(()));
        client
            .expect_stop_sandbox()
            .withf(|_, id| id == "woken-then-unreadable")
            .times(1)
            .returning(|_, _| Ok(()));
        client.expect_delete_sandbox().never();

        let error = sandbox_with(client)
            .get_or_create(CreateSessionRequest {
                session_id: Some("woken-then-unreadable".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect_err("a state this client cannot read is not a session");

        assert_eq!(error.code, "UNEXPECTED_RESPONSE_FORMAT", "{error}");
    }

    /// The variables a command declares reach the command.
    ///
    /// Every other backend honours `RunCommandRequest.env`; dropping it here would answer a
    /// documented field with nothing, and the failure would surface inside the sandbox.
    #[tokio::test]
    async fn a_declared_variable_reaches_the_command() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_get_sandbox()
            .returning(|_, id| Ok(running(id, None)));
        client
            .expect_execute_shell_command()
            .times(1)
            .withf(|_, _, shell, _| shell.ends_with("' sh 'env' 'TOKEN=t' '--' 'sleep' 'forever'"))
            .returning(|_, _, _, _| {
                Ok(alien_azure_clients::azure::sandbox_data_plane::ExecResult {
                    exit_code: Some(0),
                    stdout: String::new(),
                    // The wrapper announces its nonce before starting the command.
                    stderr: "beef\n".to_string(),
                })
            });

        let mut request = command(5);
        request.env = BTreeMap::from([("TOKEN".to_string(), "t".to_string())]);

        let frames: Vec<Result<CommandOutput>> = sandbox_with(client)
            .run_command("s1", request)
            .await
            .expect("a command declaring a variable must run")
            .collect()
            .await;

        assert!(
            matches!(frames.last(), Some(Ok(CommandOutput::Exit { code, .. })) if *code == 0),
            "the command has to reach its exit: {frames:?}"
        );
    }

    /// A variable name that is not a name never reaches the shell string.
    ///
    /// The name sits left of the `=`, where quoting cannot reach it, so an unchecked one is a
    /// second command running inside the sandbox rather than a variable in it.
    #[tokio::test]
    async fn a_command_carrying_an_unusable_variable_name_runs_nothing() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_get_sandbox()
            .returning(|_, id| Ok(running(id, None)));
        client.expect_execute_shell_command().never();

        let mut request = command(5);
        request.env = BTreeMap::from([("X; curl evil".to_string(), "1".to_string())]);

        let error = match sandbox_with(client).run_command("s1", request).await {
            Ok(_) => panic!("a name the shell would run must not reach the shell"),
            Err(error) => error,
        };

        assert_eq!(error.code, "INVALID_INPUT", "{error}");
    }

    /// A resume whose outcome is unknown is one this call owns.
    ///
    /// A 5xx or a dropped connection does not mean the POST failed to land: the session can wake
    /// anyway. Treating that as "did not wake" leaves a sandbox this call put back on the network
    /// under a policy the declaration forbids, with nothing coming back for it.
    #[tokio::test]
    async fn a_resume_that_may_have_landed_is_owned() {
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            if reads <= 2 {
                sandbox.state = Some("Stopped".to_string());
            }
            Ok(sandbox)
        });
        // The answer never arrived; the data plane may still have taken it.
        client
            .expect_resume_sandbox()
            .returning(|_, _| Err(http_error(503, "GatewayTimeout")));
        client
            .expect_stop_sandbox()
            .times(1)
            .returning(|_, _| Ok(()));

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .resume("woke-or-did-not")
            .await
            .expect_err("a session that came up uncontained is not a resumed session");

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A resume the data plane refused is not one this call woke.
    ///
    /// The other side of the same rule: a 4xx is an answer, so the session stayed asleep and
    /// whatever woke it afterwards was someone else. Stopping it would end their work.
    #[tokio::test]
    async fn a_refused_resume_leaves_someone_elses_session_alone() {
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            if reads <= 2 {
                sandbox.state = Some("Stopped".to_string());
            }
            Ok(sandbox)
        });
        // Refused, so this call did not wake it — another revision did, between the polls.
        client
            .expect_resume_sandbox()
            .returning(|_, _| Err(http_error(409, "SandboxNotStopped")));
        client.expect_stop_sandbox().never();
        client.expect_delete_sandbox().never();

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .resume("someone-elses-session")
            .await
            .expect_err("a session without the declared policy must not be handed back");

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A session that vanished while it was being put back is not "left awake".
    ///
    /// The put-back exists to name a sandbox this call left running. One the data plane says is
    /// gone has reached that state by another route, and reporting it sends an operator looking
    /// for something that does not exist.
    #[tokio::test]
    async fn a_session_that_vanished_is_not_reported_as_left_awake() {
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            if reads <= 2 {
                sandbox.state = Some("Stopped".to_string());
            }
            Ok(sandbox)
        });
        client.expect_resume_sandbox().returning(|_, _| Ok(()));
        client
            .expect_stop_sandbox()
            .times(1)
            .returning(|_, _| Err(http_error(404, "SandboxNotFound")));

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .resume("gone-by-then")
            .await
            .expect_err("the refusal still travels");

        assert!(
            !error.to_string().contains("sandboxLeftAwake"),
            "a sandbox the data plane says is gone was not left awake: {error}"
        );
        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A session being deleted is still running, so it must not take new work.
    ///
    /// `get` skips the policy check for one — a sandbox on its way out carries no policy to
    /// judge — so a gate that only asks "does it exist" would run untrusted code on a live
    /// sandbox under whatever egress it was built with. Azure accepts a delete rather than
    /// completing it, which is why `terminate` polls to a 404 instead of trusting the accept.
    #[tokio::test]
    async fn a_session_being_deleted_takes_no_new_work() {
        for outcome in ["Deleting", "gone"] {
            let mut client = MockSandboxDataPlaneApi::new();
            let deleting = outcome == "Deleting";
            client.expect_get_sandbox().returning(move |_, id| {
                if deleting {
                    let mut sandbox = running(id, None);
                    sandbox.state = Some("Deleting".to_string());
                    Ok(sandbox)
                } else {
                    Err(http_error(404, "SandboxNotFound"))
                }
            });
            client.expect_execute_shell_command().never();
            client.expect_resume_sandbox().never();
            let sandbox = sandbox_denying(client, SandboxEgress::Deny);

            let ran = match sandbox.run_command("on-its-way-out", command(5)).await {
                Ok(_) => panic!("{outcome}: a session that cannot take work must not run code"),
                Err(error) => error,
            };
            assert_eq!(ran.code, "SANDBOX_COMMAND_FAILED", "{outcome}: {ran}");

            let woken = sandbox
                .resume("on-its-way-out")
                .await
                .expect_err("a session that cannot take work must not be resumed");
            assert_eq!(woken.code, "SANDBOX_COMMAND_FAILED", "{outcome}: {woken}");
        }
    }

    /// A create whose id this client will not send is reaped unless the id is why.
    ///
    /// An over-long or oddly-spelled id is still one path segment, so the sandbox can be deleted
    /// once and must be — nothing else can find it. An id carrying a separator or an escape is
    /// the one case where the delete itself would travel somewhere else.
    #[tokio::test]
    async fn an_unaddressable_minted_id_is_reaped_unless_the_id_is_the_hazard() {
        let minted = |id: &'static str| {
            let mut client = MockSandboxDataPlaneApi::new();
            client
                .expect_create_sandbox()
                .times(1)
                .returning(move |_, _| Ok(running(id, None)));
            client
        };

        // Safe to address once: reaped.
        let mut client = minted("x".repeat(80).leak());
        client.expect_delete_sandbox().times(1).returning(|_, _| Ok(()));
        let error = sandbox_with(client)
            .create(CreateSessionRequest::default())
            .await
            .expect_err("an id this client will not send must fail the create");
        assert_eq!(error.code, "UNEXPECTED_RESPONSE_FORMAT", "{error}");

        // The id is the hazard: the delete would travel into another group, so it is not sent.
        let mut client = minted("../../other-group/sandboxes/theirs");
        client.expect_delete_sandbox().never();
        let error = sandbox_with(client)
            .create(CreateSessionRequest::default())
            .await
            .expect_err("a traversing id must fail the create");
        assert_eq!(error.code, "UNEXPECTED_RESPONSE_FORMAT", "{error}");
    }

    /// A sandbox that is still coming up has no policy yet, and that is not a mismatch.
    ///
    /// `policy_holds` reads an absent policy as a failure, so judging a `Creating` session would
    /// report a booting sandbox as an uncontained one — and `get_or_create` acts on that by
    /// deleting it and creating another.
    #[tokio::test]
    async fn a_session_that_is_still_coming_up_is_not_a_policy_mismatch() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().times(1).returning(|_, id| {
            let mut sandbox = running(id, None);
            sandbox.state = Some("Creating".to_string());
            Ok(sandbox)
        });
        client.expect_delete_sandbox().never();

        let session = sandbox_denying(client, SandboxEgress::Deny)
            .get("still-booting")
            .await
            .expect("a booting session is not a contained-ness failure")
            .expect("the session exists");

        assert_eq!(session.state, SandboxSessionState::Starting);
    }

    /// Writing into a stale session is refused before the bytes land.
    ///
    /// `write_files` is the one file operation that moves the caller's own content in, so a
    /// write-then-run against an id kept across a tightened declaration would put the payload
    /// inside a sandbox with the egress the declaration just removed.
    #[tokio::test]
    async fn a_stale_policy_session_takes_no_written_files() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_get_sandbox()
            .times(1)
            .returning(|_, id| Ok(running(id, None)));
        client.expect_delete_sandbox().never();
        client.expect_write_file().never();

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .write_files(
                "built-under-allow",
                BTreeMap::from([("app.py".to_string(), vec![1u8])]),
            )
            .await
            .expect_err("a session without the declared policy must take no content");

        assert_eq!(error.code, "SANDBOX_NOT_AS_DECLARED", "{error}");
    }

    /// A resume the data plane refuses once is retried, not abandoned for the whole wait.
    ///
    /// The first attempt is the one most likely to be refused — a resume racing a sandbox that is
    /// still stopping answers 409 — so remembering only that an attempt was made would spend the
    /// budget watching a session nothing is bringing up.
    #[tokio::test]
    async fn a_refused_resume_is_tried_again() {
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            // Stopping, then stopped, then up — the shape a suspend-then-resume race produces.
            sandbox.state = Some(
                match reads {
                    1 => "Stopping",
                    2 | 3 => "Stopped",
                    _ => "Running",
                }
                .to_string(),
            );
            Ok(sandbox)
        });

        let mut attempts = 0;
        client.expect_resume_sandbox().times(2).returning(move |_, _| {
            attempts += 1;
            if attempts == 1 {
                // The 409 a sandbox still stopping answers.
                Err(http_error(409, "SandboxNotStopped"))
            } else {
                Ok(())
            }
        });

        sandbox_with(client)
            .resume("racing-the-idle-policy")
            .await
            .expect("a refused first resume must not doom the wait");
    }

    /// A session that is not running takes no work and no content, and is not woken to take it.
    ///
    /// Waking one to write into it would undo the idle suspend the declaration asked for, and a
    /// stopped sandbox's policy record is not the one the work would run under.
    #[tokio::test]
    async fn a_suspended_session_is_refused_rather_than_woken() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().returning(|_, id| {
            let mut sandbox = running(id, None);
            sandbox.state = Some("Stopped".to_string());
            Ok(sandbox)
        });
        client.expect_resume_sandbox().never();
        client.expect_write_file().never();
        client.expect_execute_shell_command().never();
        let sandbox = sandbox_denying(client, SandboxEgress::Deny);

        let wrote = sandbox
            .write_files(
                "asleep",
                BTreeMap::from([("app.py".to_string(), vec![1u8])]),
            )
            .await
            .expect_err("a suspended session takes no content");
        assert_eq!(wrote.code, "SANDBOX_COMMAND_FAILED", "{wrote}");

        let ran = match sandbox.run_command("asleep", command(5)).await {
            Ok(_) => panic!("a suspended session runs no code"),
            Err(error) => error,
        };
        assert_eq!(ran.code, "SANDBOX_COMMAND_FAILED", "{ran}");
    }

    /// A stopped session that no longer matches is refused before anything wakes it.
    ///
    /// The stopped record carries the policy it stopped under, so it is judgeable — and waking a
    /// sandbox to find out would put its workload back on the network for the length of a boot
    /// before this call could refuse it.
    #[tokio::test]
    async fn a_stopped_session_is_judged_before_it_is_woken() {
        let mut client = MockSandboxDataPlaneApi::new();
        let declared = EgressPolicy {
            default_action: "Deny".to_string(),
            host_rules: vec![EgressHostRule {
                pattern: "*".to_string(),
                action: "Deny".to_string(),
            }],
            rules: Vec::new(),
            unmodelled: Default::default(),
            traffic_inspection: Some("Full".to_string()),
        };
        client.expect_get_sandbox().returning(move |_, id| {
            if id == "fresh" {
                return Ok(running(id, Some(declared.clone())));
            }
            // Asleep, and the record it stopped under is present and open.
            let mut sandbox = running(
                id,
                Some(EgressPolicy {
                    default_action: "Allow".to_string(),
                    host_rules: Vec::new(),
                    rules: Vec::new(),
                    unmodelled: Default::default(),
                    traffic_inspection: Some("Full".to_string()),
                }),
            );
            sandbox.state = Some("Stopped".to_string());
            Ok(sandbox)
        });
        client.expect_resume_sandbox().never();
        // Nothing woke it and nothing owns it here, so it is left exactly as found.
        client.expect_delete_sandbox().never();
        client.expect_stop_sandbox().never();
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, request| Ok(running("fresh", request.egress)));

        let session = sandbox_denying(client, SandboxEgress::Deny)
            .get_or_create(CreateSessionRequest {
                session_id: Some("asleep-under-allow".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("a caller asking for a session gets a usable one");

        assert_eq!(session.session_id, "fresh");
    }

    /// A session the data plane reports as `Failed` is replaced, not carried forever.
    ///
    /// It is a documented terminal state, and one this client did not know: an unmapped state
    /// becomes an unexpected-response error, which nothing heals, so the id would be permanently
    /// unusable through `get_or_create`.
    #[tokio::test]
    async fn a_failed_session_is_replaced() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().returning(|_, id| {
            if id == "fresh" {
                return Ok(running(id, None));
            }
            let mut sandbox = running(id, None);
            sandbox.state = Some("Failed".to_string());
            Ok(sandbox)
        });
        // A failed sandbox is not going away on its own, so it is reaped rather than left beside
        // its replacement.
        client
            .expect_delete_sandbox()
            .withf(|_, id| id == "broken")
            .times(1)
            .returning(|_, _| Ok(()));
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, _| Ok(running("fresh", None)));

        let session = sandbox_with(client)
            .get_or_create(CreateSessionRequest {
                session_id: Some("broken".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("a failed session is replaced rather than returned");

        assert_eq!(session.session_id, "fresh");
    }

    /// `Failed` is a state the data plane reports and this client has to know.
    ///
    /// An unmapped state becomes an unexpected-response error, and nothing heals that — so the id
    /// of a failed sandbox would be permanently unusable rather than replaced.
    #[tokio::test]
    async fn a_failed_session_reads_as_terminated() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().times(1).returning(|_, id| {
            let mut sandbox = running(id, None);
            sandbox.state = Some("Failed".to_string());
            Ok(sandbox)
        });

        let session = sandbox_denying(client, SandboxEgress::Deny)
            .get("broken")
            .await
            .expect("a failed session is a state, not an unreadable response")
            .expect("the session exists");

        assert_eq!(session.state, SandboxSessionState::Terminated);
    }

    /// A session that dies while it is being waited for is replaced, like one already dead.
    ///
    /// The same condition one read earlier heals as `sessionGone`; answering it differently
    /// depending on which read observed it is the inconsistency this path exists to avoid.
    #[tokio::test]
    async fn a_session_that_dies_during_the_wait_is_replaced() {
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            if id == "fresh" {
                return Ok(running(id, None));
            }
            reads += 1;
            let mut sandbox = running(id, None);
            // Asleep when it is found, being deleted by the time the wait looks.
            sandbox.state = Some(if reads == 1 { "Stopped" } else { "Deleting" }.to_string());
            Ok(sandbox)
        });
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, _| Ok(running("fresh", None)));

        let session = sandbox_with(client)
            .get_or_create(CreateSessionRequest {
                session_id: Some("dying".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("a session that died mid-wait is replaced");

        assert_eq!(session.session_id, "fresh");
    }

    /// A sleeping session that still matches is reconnected, not replaced.
    ///
    /// The discriminating case for judging a stopped record: if the data plane does report the
    /// policy for a suspended sandbox, a compliant one has to survive the reconnect — otherwise
    /// every idle-suspended session would be silently churned on each attach.
    #[tokio::test]
    async fn a_sleeping_session_that_still_matches_is_kept() {
        let declared = EgressPolicy {
            default_action: "Deny".to_string(),
            host_rules: vec![EgressHostRule {
                pattern: "*".to_string(),
                action: "Deny".to_string(),
            }],
            rules: Vec::new(),
            unmodelled: Default::default(),
            traffic_inspection: Some("Full".to_string()),
        };

        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        let carried = declared.clone();
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, Some(carried.clone()));
            // Asleep for the first two reads — the reconnect's own, and the wait's first poll —
            // so the resume is actually issued.
            if reads <= 2 {
                sandbox.state = Some("Stopped".to_string());
            }
            Ok(sandbox)
        });
        client
            .expect_resume_sandbox()
            .times(1)
            .returning(|_, _| Ok(()));
        client.expect_delete_sandbox().never();
        client.expect_create_sandbox().never();

        let session = sandbox_denying(client, SandboxEgress::Deny)
            .get_or_create(CreateSessionRequest {
                session_id: Some("asleep-and-fine".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("a compliant sleeping session is woken and returned");

        assert_eq!(session.session_id, "asleep-and-fine");
    }

    /// A sleeping session with no policy on its record is woken before it is judged.
    ///
    /// Whether the data plane reports `egressPolicy` for a sandbox that is not running is
    /// unverified. If it does not, judging the sleeping record would delete every compliant
    /// idle-suspended session on every reconnect, so the absence is left for the post-wake read.
    #[tokio::test]
    async fn a_sleeping_session_with_no_policy_is_woken_before_it_is_judged() {
        let declared = EgressPolicy {
            default_action: "Deny".to_string(),
            host_rules: vec![EgressHostRule {
                pattern: "*".to_string(),
                action: "Deny".to_string(),
            }],
            rules: Vec::new(),
            unmodelled: Default::default(),
            traffic_inspection: Some("Full".to_string()),
        };

        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        let carried = declared.clone();
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            if reads <= 2 {
                let mut asleep = running(id, None);
                asleep.state = Some("Stopped".to_string());
                return Ok(asleep);
            }
            Ok(running(id, Some(carried.clone())))
        });
        client
            .expect_resume_sandbox()
            .times(1)
            .returning(|_, _| Ok(()));
        client.expect_delete_sandbox().never();
        client.expect_create_sandbox().never();

        let session = sandbox_denying(client, SandboxEgress::Deny)
            .get_or_create(CreateSessionRequest {
                session_id: Some("asleep-without-a-record".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("an absent policy on a sleeping record is unknown, not a mismatch");

        assert_eq!(session.session_id, "asleep-without-a-record");
    }

    /// A session woken to be judged, found uncontained, and left awake says so.
    ///
    /// The refusal alone would read as "nothing happened", when what happened is a sandbox this
    /// call put back on the network under a policy the declaration does not allow.
    #[tokio::test]
    async fn a_session_that_cannot_be_put_back_is_reported_as_left_awake() {
        let mut client = MockSandboxDataPlaneApi::new();
        let mut reads = 0;
        client.expect_get_sandbox().returning(move |_, id| {
            reads += 1;
            let mut sandbox = running(id, None);
            // Asleep for the resume's own read and the wait's first poll, so the wait is what
            // wakes it — and therefore what owes the put-back.
            if reads <= 2 {
                sandbox.state = Some("Stopped".to_string());
            }
            Ok(sandbox)
        });
        client.expect_resume_sandbox().returning(|_, _| Ok(()));
        client
            .expect_stop_sandbox()
            .times(1)
            .returning(|_, _| Err(http_error(500, "SuspendFailed")));

        let error = sandbox_denying(client, SandboxEgress::Deny)
            .resume("built-under-allow")
            .await
            .expect_err("a session that woke up uncontained must not be reported as resumed");

        assert!(
            error.to_string().contains("sandboxLeftAwake"),
            "a sandbox left awake has to be named, not folded into the refusal: {error}"
        );
    }

    /// A state this client cannot read takes no work, and is not called suspended.
    ///
    /// Reporting it as suspended sends the caller to `resume`, which answers the same thing —
    /// a loop that ends in a timeout instead of the unreadable state that caused it.
    #[tokio::test]
    async fn an_unreadable_state_takes_no_work_and_is_not_called_suspended() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_get_sandbox().times(1).returning(|_, _| {
            Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
                id: "s1".to_string(),
                egress_policy: None,
                state: Some("Hibernated".to_string()),
            })
        });
        client.expect_execute_shell_command().never();
        client.expect_resume_sandbox().never();

        let error = match sandbox_with(client).run_command("s1", command(5)).await {
            Ok(_) => panic!("an unreadable state must not take work"),
            Err(error) => error,
        };

        assert_eq!(error.code, "UNEXPECTED_RESPONSE_FORMAT", "{error}");
    }
}
