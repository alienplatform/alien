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
    CreateSandbox, EgressHostRule, EgressPolicy, EgressRule, EgressRuleAction, EgressRuleMatch,
    SandboxDataPlaneApi,
};
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{Platform, SandboxCapabilities, SandboxEgress};
use alien_error::{AlienError, ContextError};

/// A Sandbox backed by the Azure ADC data plane.
#[derive(Debug)]
pub struct AzureSandbox {
    client: std::sync::Arc<dyn SandboxDataPlaneApi>,
    sandbox_group: String,
    /// Catalog disk image every session is created from, from the declaration.
    disk_image: String,
    /// Outbound policy every session is created with, from the declaration.
    egress: SandboxEgress,
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
        cpu: String,
        memory: String,
    ) -> Self {
        Self {
            client,
            sandbox_group,
            disk_image,
            egress,
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

        if operation == RUN_COMMAND {
            return error.context(ErrorData::SandboxCommandFailed {
                failure: "outcomeUnknown".to_string(),
                reason: format!(
                    "{operation} did not complete against the Azure sandbox data plane, so \
                     whether the command ran is unknown"
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
                },
            )
            .await
            .map_err(|error| Self::failed("sandbox.create", error))?;

        // The caller's requested id is not authoritative: Azure allocates the id, and returning
        // the requested one would hand back a handle that addresses nothing.
        let _ = request.session_id;

        // A restriction that did not take effect is worse than one that was never asked for: the
        // caller believes the sandbox is contained. The response says what the sandbox is running
        // under, so this is checked rather than assumed, and a sandbox that came up without the
        // policy is deleted rather than handed back.
        if let Some(asked) = &asked {
            if !policy_holds(asked, sandbox.egress_policy.as_ref()) {
                // Deleting is safe to do unconditionally here: Azure allocates the id, so the
                // one in this response was minted by this call and belongs to no other caller.
                // The delete's own failure is carried rather than returned: it would replace the
                // finding that matters — that the sandbox is not contained — with a delete error.
                let deleted = match self.accept_delete(&sandbox.id).await {
                    Ok(()) => "it was deleted".to_string(),
                    Err(error) => format!("deleting it also failed: {error}"),
                };
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: "sandbox".to_string(),
                    env_var: "ALIEN_BINDING_SANDBOX".to_string(),
                    reason: format!(
                        "the sandbox was created asking for {} but came up with {}, so {deleted} \
                         rather than handed back",
                        describe(Some(asked)),
                        describe(sandbox.egress_policy.as_ref())
                    ),
                }));
            }
        }

        Ok(SandboxSession {
            session_id: sandbox.id,
            state: session_state("sandbox.create", sandbox.state.as_deref())?,
            generation: 1,
        })
    }

    async fn get(&self, session_id: &str) -> Result<Option<SandboxSession>> {
        match self
            .client
            .get_sandbox(&self.sandbox_group, session_id)
            .await
        {
            Ok(sandbox) => Ok(Some(SandboxSession {
                session_id: sandbox.id,
                state: session_state("sandbox.get", sandbox.state.as_deref())?,
                generation: 1,
            })),
            // A 404 is "gone", which is a valid answer. Anything else is a real failure and must
            // not be flattened into None, or a throttle would read as an expired session.
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(Self::failed("sandbox.get", error)),
        }
    }

    async fn get_or_create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        if let Some(id) = request.session_id.as_deref() {
            // A session on its way out is not one to reconnect to: the id will not run again, and
            // handing it back trades an error now for a command that never lands.
            match self.get(id).await? {
                Some(existing) if existing.state != SandboxSessionState::Terminated => {
                    return Ok(existing)
                }
                _ => {}
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
        if request.deadline.is_zero() {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox.runCommand".to_string(),
                reason: "a command must carry a non-zero deadline".to_string(),
            }));
        }

        // The deadline bounds the untrusted code, not the caller's patience. Read out of the
        // preview SDK rather than assumed: `executeShellCommand` sends `command` and an optional
        // `workingDirectory` and nothing else, so there is no server-side timeout to ask for. The
        // deadline is enforced inside the session instead — the wrapper kills the command at it,
        // so the session survives and the call lands right after, the same shape the
        // agent-supervised backends give. The client-side guard is the backstop for a data plane
        // that never answers at all; there the only lever left is ending the session, and that
        // call returns once the session is confirmed gone rather than claim containment early.
        let shell = bounded_shell(&request.command, request.deadline);

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

    async fn read_file(&self, session_id: &str, path: &str) -> Result<Vec<u8>> {
        checked_path("sandbox.readFile", path)?;

        self.client
            .read_file(&self.sandbox_group, session_id, path)
            .await
            .map_err(|error| Self::failed("sandbox.readFile", error))
    }

    async fn write_files(&self, session_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        // One request per path, stopping at the first failure: the same partial application every
        // other backend performs, so a caller sees one contract rather than five.
        for (path, contents) in files {
            checked_path("sandbox.writeFiles", &path)?;

            self.client
                .write_file(&self.sandbox_group, session_id, &path, contents)
                .await
                .map_err(|error| Self::failed("sandbox.writeFiles", error))?;
        }

        Ok(())
    }

    async fn mkdir(&self, session_id: &str, path: &str) -> Result<()> {
        checked_path("sandbox.mkdir", path)?;

        self.client
            .mkdir(&self.sandbox_group, session_id, path)
            .await
            .map_err(|error| Self::failed("sandbox.mkdir", error))
    }

    async fn preview(&self, _session_id: &str, _port: u16) -> Result<PreviewCapability> {
        Err(self.unsupported("preview"))
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
        self.accept_delete(session_id).await?;

        // The delete is accepted, not completed: the client's own contract is "returns before it
        // is gone; confirm by polling to 404". Returning here would report containment while the
        // code is still running, which is the whole point of terminate.
        for _ in 0..TERMINATE_POLL_ATTEMPTS {
            if self.get(session_id).await?.is_none() {
                return Ok(());
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
    /// Runs one shell string under the client-side guard.
    ///
    /// The guard is the deadline plus the grace the in-session `timeout` needs to report back.
    /// When it fires the session itself did not end the command, so the session is ended, and
    /// the call returns once that is confirmed — the same rule the agent-supervised backends
    /// follow, where the agent waits for its kill before reporting: `deadlineExceeded` means the
    /// command has stopped, never that a stop was requested. This is the one path where untrusted
    /// code is known to be running past its deadline, so it is bounded rather than early: the
    /// deadline, the grace, and the delete's confirmation window, and it is reached only by a
    /// session that could not run `timeout` — every other overrun is ended in place, at the
    /// deadline.
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
fn bounded_shell(command: &[String], deadline: std::time::Duration) -> String {
    let escape = |value: &str| value.replace('\'', "'\\''");
    let arguments = command
        .iter()
        .map(|argument| format!(" '{}'", escape(argument)))
        .collect::<String>();
    format!(
        "sh -c '{}' sh{arguments}",
        escape(&DeadlineReport::bounded_program(deadline))
    )
}

/// Refuses a caller's path before it reaches the data plane.
///
/// Whether the server bounds a path to a root is undocumented and unmeasured, so this is the only
/// confinement there is, and it is a client-side rule rather than a guarantee. Relative only:
/// Azure exposes no session root to rewrite an absolute path against, so accepting one would hand
/// the caller the sandbox's whole filesystem instead of its own directory.
fn checked_path(operation: &str, path: &str) -> Result<()> {
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
    if path.is_empty() {
        return refused("is empty");
    }
    if path.starts_with('/') {
        return refused("must be relative to the sandbox's own directory");
    }
    if path.contains('\0') {
        return refused("contains a null byte");
    }
    if path.split('/').any(|part| part == ".." || part.is_empty()) {
        return refused("must not traverse");
    }

    Ok(())
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

    let allowed = |host: &str| {
        asked
            .host_rules
            .iter()
            .any(|rule| rule.action == ALLOW && rule.pattern == host)
    };

    effective.default_action == asked.default_action
        && effective.traffic_inspection.as_deref() == Some(FULL_INSPECTION)
        && asked
            .host_rules
            .iter()
            .all(|rule| effective.host_rules.contains(rule))
        && effective
            .host_rules
            .iter()
            .all(|rule| rule.action != ALLOW || allowed(&rule.pattern))
        // An advanced rule is refused outright rather than matched host by host: this client
        // never sends one, so an `Allow` here came from somewhere else, and `Transform` and
        // `Rewrite` reach a host by rewriting the request rather than by naming it.
        && effective.rules.iter().all(|rule| {
            rule.action
                .as_ref()
                .is_some_and(|action| action.action_type == DENY)
        })
}

/// The effective policy, short enough to read in an error.
fn describe(effective: Option<&EgressPolicy>) -> String {
    match effective {
        None => "no policy at all".to_string(),
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
        Some("Stopping" | "Stopped" | "Suspended") => Ok(SandboxSessionState::Suspended),
        Some("Deleting") => Ok(SandboxSessionState::Terminated),
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

/// The one operation a repeat could run twice.
const RUN_COMMAND: &str = "sandbox.runCommand";

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

    fn sandbox_with(client: MockSandboxDataPlaneApi) -> AzureSandbox {
        AzureSandbox::new(
            std::sync::Arc::new(client),
            "grp".to_string(),
            "ubuntu".to_string(),
            SandboxEgress::Allow,
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

        let sandbox = AzureSandbox::new(
            std::sync::Arc::new(client),
            "grp".to_string(),
            "my-toolchain".to_string(),
            SandboxEgress::Allow,
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
    /// path — used to read as "the session is gone", which starts a second sandbox while the
    /// first keeps running and reports a live session as terminated.
    #[test]
    fn only_the_status_decides_whether_a_session_is_gone() {
        assert!(is_not_found(&http_error(404, "SandboxNotFound")));

        // The shape the client actually produces: a 404 is returned as
        // `http_error.context(RemoteResourceNotFound)`, so the outer variant is the classified
        // one. Matching only `HttpResponseError` made every real 404 read as a live session.
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
            std::time::Duration::from_millis(1500),
        );
        assert!(wrapped.contains("sleep 1.500"), "{wrapped}");
        assert!(
            wrapped.ends_with("' sh 'echo' 'it'\\''s' '&&' 'sleep 5'"),
            "{wrapped}"
        );
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

        for path in ["../etc/shadow", "/etc/shadow", "", "work/", "a//b", "a/../../b"] {
            let error = sandbox
                .read_file("s1", path)
                .await
                .expect_err("'{path}' must be refused");
            assert_eq!(error.code, "INVALID_INPUT", "{path}: {error}");

            sandbox
                .write_files("s1", BTreeMap::from([(path.to_string(), vec![1u8])]))
                .await
                .expect_err("'{path}' must be refused on write too");
            sandbox
                .mkdir("s1", path)
                .await
                .expect_err("'{path}' must be refused on mkdir too");
        }

        // The same shapes, accepted: a rule that refuses everything would pass the loop above.
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_read_file()
            .times(2)
            .returning(|_, _, _| Ok(Vec::new()));
        let sandbox = sandbox_with(client);
        for path in ["app.py", "src/app.py"] {
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

    /// Writing stops at the first failure rather than pressing on, which is what makes a partial
    /// write observable to the caller instead of a success with a hole in it.
    #[tokio::test]
    async fn a_failed_write_stops_the_ones_behind_it() {
        let mut client = MockSandboxDataPlaneApi::new();
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
            ("Stopping", SandboxSessionState::Suspended),
            ("Stopped", SandboxSessionState::Suspended),
            ("Suspended", SandboxSessionState::Suspended),
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

        let session = sandbox_with(client)
            .create(CreateSessionRequest {
                session_id: None,
                tenant_key: None,
                env: BTreeMap::from([("TOKEN".to_string(), "t".to_string())]),
            })
            .await
            .expect("the create should succeed");

        assert_eq!(
            session.state,
            SandboxSessionState::Starting,
            "a sandbox still being created is not one a command can reach"
        );
    }

    fn running(id: &str, egress: Option<EgressPolicy>) -> alien_azure_clients::azure::sandbox_data_plane::Sandbox {
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
                rules: Vec::new(),
                host_rules: Vec::new(),
                traffic_inspection: Some("Partial".to_string()),
            }),
            // Inspected, and open.
            Some(EgressPolicy {
                default_action: "Allow".to_string(),
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
            client
                .expect_delete_sandbox()
                .withf(|_, id| id == "s1")
                .times(1)
                .returning(|_, _| Ok(()));

            let error = sandbox_denying(client, SandboxEgress::Deny)
                .create(CreateSessionRequest::default())
                .await
                .expect_err("a sandbox without its policy must not be handed back");

            assert_eq!(error.code, "BINDING_CONFIG_INVALID", "{error}");
        }
    }

    /// A host the declaration named that the sandbox is not running is the same failure as a
    /// missing policy: the caller believes traffic to it is allowed and it is not, or worse, the
    /// list came back holding something else.
    #[tokio::test]
    async fn a_missing_host_rule_fails_the_create() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_create_sandbox().times(1).returning(|_, _| {
            Ok(running(
                "s1",
                Some(EgressPolicy {
                    default_action: "Deny".to_string(),
                    rules: Vec::new(),
                    host_rules: vec![EgressHostRule {
                        pattern: "elsewhere.example.com".to_string(),
                        action: "Allow".to_string(),
                    }],
                    traffic_inspection: Some("Full".to_string()),
                }),
            ))
        });
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

        assert_eq!(error.code, "BINDING_CONFIG_INVALID", "{error}");
    }

    /// A session that is going away is not one to reconnect to.
    ///
    /// `get_or_create` hands back whatever `get` finds, and the id of a deleting sandbox will not
    /// run again — so the caller would receive a handle whose every command lands on nothing.
    #[tokio::test]
    async fn a_terminated_session_is_replaced_rather_than_reconnected_to() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_get_sandbox()
            .times(1)
            .returning(|_, id| Ok(running(id, None)).map(|mut sandbox: alien_azure_clients::azure::sandbox_data_plane::Sandbox| {
                sandbox.state = Some("Deleting".to_string());
                sandbox
            }));
        client
            .expect_create_sandbox()
            .times(1)
            .returning(|_, _| Ok(running("fresh", None)));

        let session = sandbox_with(client)
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
                host_rules: vec![declared.clone()],
                rules: vec![EgressRule {
                    r#match: Some(EgressRuleMatch {
                        host: "*".to_string(),
                    }),
                    action: Some(EgressRuleAction {
                        action_type: "Allow".to_string(),
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
            client.expect_delete_sandbox().times(1).returning(|_, _| Ok(()));

            let error = sandbox_denying(client, asked_for())
                .create(CreateSessionRequest::default())
                .await
                .expect_err("a permission nobody asked for must fail the create");

            assert_eq!(error.code, "BINDING_CONFIG_INVALID", "{error}");
        }

        // The same policy without the extra permission creates normally, so the rule above is
        // refusing the addition rather than refusing everything.
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_create_sandbox().times(1).returning(move |_, _| {
            Ok(running(
                "s1",
                Some(EgressPolicy {
                    default_action: "Deny".to_string(),
                    host_rules: vec![EgressHostRule {
                        pattern: "api.example.com".to_string(),
                        action: "Allow".to_string(),
                    }],
                    rules: Vec::new(),
                    traffic_inspection: Some("Full".to_string()),
                }),
            ))
        });
        sandbox_denying(client, asked_for())
            .create(CreateSessionRequest::default())
            .await
            .expect("the policy that was asked for should create");
    }
}
