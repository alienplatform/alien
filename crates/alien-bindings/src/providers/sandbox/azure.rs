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
use alien_azure_clients::azure::sandbox_data_plane::SandboxDataPlaneApi;
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{Platform, SandboxCapabilities};
use alien_error::AlienError;

/// A Sandbox backed by the Azure ADC data plane.
#[derive(Debug)]
pub struct AzureSandbox {
    client: std::sync::Arc<dyn SandboxDataPlaneApi>,
    sandbox_group: String,
    /// Catalog disk image every session is created from, from the declaration.
    disk_image: String,
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
        cpu: String,
        memory: String,
    ) -> Self {
        Self {
            client,
            sandbox_group,
            disk_image,
            cpu,
            memory,
        }
    }

    /// The catalog image sessions are created from. Exists so a test can prove the declaration
    /// reached the provider — the failure it guards is silent, so nothing else would show it.
    pub(crate) fn disk_image(&self) -> &str {
        &self.disk_image
    }

    fn unsupported(&self, capability: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: capability.to_string(),
            reason: "not supported on azure".to_string(),
        })
    }

    fn failed(operation: &str, error: impl std::fmt::Display) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: operation.to_string(),
            reason: format!("the Azure sandbox data plane refused the call: {error}"),
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
        let sandbox = self
            .client
            .create_sandbox(&self.sandbox_group, &self.disk_image, &self.cpu, &self.memory)
            .await
            .map_err(|error| Self::failed("sandbox.create", error))?;

        // The caller's requested id is not authoritative: Azure allocates the id, and returning
        // the requested one would hand back a handle that addresses nothing.
        let _ = request.session_id;

        Ok(SandboxSession {
            session_id: sandbox.id,
            state: SandboxSessionState::Running,
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
                state: match sandbox.status.as_deref() {
                    Some("Stopped") => SandboxSessionState::Suspended,
                    _ => SandboxSessionState::Running,
                },
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
            if let Some(existing) = self.get(id).await? {
                return Ok(existing);
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

    async fn read_file(&self, _session_id: &str, _path: &str) -> Result<Vec<u8>> {
        Err(self.unsupported("readFile"))
    }

    async fn write_files(
        &self,
        _session_id: &str,
        _files: BTreeMap<String, Vec<u8>>,
    ) -> Result<()> {
        Err(self.unsupported("writeFiles"))
    }

    async fn mkdir(&self, _session_id: &str, _path: &str) -> Result<()> {
        Err(self.unsupported("mkdir"))
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
            Ok(inner) => inner.map_err(|error| Self::failed("sandbox.runCommand", error)),
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
            .withf(|_, disk_image, _, _| disk_image == "my-toolchain")
            .times(1)
            .returning(|_, _, _, _| {
                Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
                    id: "s1".to_string(),
                    status: Some("Running".to_string()),
                })
            });

        let sandbox = AzureSandbox::new(
            std::sync::Arc::new(client),
            "grp".to_string(),
            "my-toolchain".to_string(),
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
                status: Some("Running".to_string()),
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
        async fn create_sandbox(
            &self,
            _group: &str,
            _disk: &str,
            _cpu: &str,
            _memory: &str,
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
                status: Some("Running".to_string()),
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
}
