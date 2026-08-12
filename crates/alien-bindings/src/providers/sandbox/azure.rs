//! Azure sandbox provider.
//!
//! The one backend with no Alien agent inside the sandbox: the ADC data plane implements exec,
//! files and lifecycle natively, so this provider is a translation layer rather than a transport
//! for a protocol. Verified against a stock `ubuntu` catalog disk containing no Alien code.

use std::collections::BTreeMap;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};

use crate::error::{ErrorData, Result};
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
    /// Disk image every session is created from.
    disk: String,
    /// Session ceilings, in the data plane's own units.
    cpu: String,
    memory: String,
}

impl AzureSandbox {
    /// Builds a provider bound to one sandbox group.
    pub fn new(
        client: std::sync::Arc<dyn SandboxDataPlaneApi>,
        sandbox_group: String,
        disk: String,
        cpu: String,
        memory: String,
    ) -> Self {
        Self {
            client,
            sandbox_group,
            disk,
            cpu,
            memory,
        }
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
            .create_sandbox(&self.sandbox_group, &self.disk, &self.cpu, &self.memory)
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
        // `workingDirectory` and nothing else, so there is no server-side timeout to ask for and
        // the only lever that stops an overrun is ending the session. The call returns once that
        // is confirmed, which is after the deadline — reporting containment before it held would
        // be the claim this whole path exists to make good on.
        let result = match tokio::time::timeout(
            request.deadline,
            self.client.execute_shell_command(
                &self.sandbox_group,
                session_id,
                &request.command.join(" "),
                request.working_directory.clone(),
            ),
        )
        .await
        {
            Ok(inner) => inner.map_err(|error| Self::failed("sandbox.runCommand", error))?,
            Err(_) => {
                self.terminate(session_id).await?;
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "deadlineExceeded".to_string(),
                    reason: format!(
                        "the command exceeded its {}s deadline and the session was terminated",
                        request.deadline.as_secs()
                    ),
                }));
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
        if !result.stderr.is_empty() {
            frames.push(Ok(CommandOutput::Stderr {
                seq: frames.len() as u64,
                data: result.stderr.into_bytes(),
            }));
        }

        frames.push(Ok(CommandOutput::Exit {
            // A missing exit code is not success. Azure did not report one, so the command's
            // outcome is unknown, and -1 says that rather than claiming zero.
            code: result.exit_code.unwrap_or(-1),
            truncated: false,
        }));

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
        match self
            .client
            .delete_sandbox(&self.sandbox_group, session_id)
            .await
        {
            Ok(_) => {}
            // An already-gone session is the desired end state. Every other failure leaves the
            // session running, and reporting success there tells the caller untrusted code has
            // stopped when it has not.
            Err(error) if is_not_found(&error) => return Ok(()),
            Err(error) => return Err(Self::failed("sandbox.terminate", error)),
        }

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

/// How long termination waits for Azure to actually remove a session.
///
/// Azure accepts a delete and completes it asynchronously, so "gone" is only observable by
/// polling. Bounded rather than open-ended: a caller waiting forever is its own outage, and an
/// unconfirmed deletion is reported as unconfirmed rather than silently treated as done.
const TERMINATE_POLL_ATTEMPTS: u32 = 15;
const TERMINATE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

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
    use alien_azure_clients::azure::sandbox_data_plane::MockSandboxDataPlaneApi;

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

    /// A data plane whose exec never returns, so the only thing that can end the call is the
    /// deadline. Hand-written rather than mocked because mockall resolves an async expectation
    /// immediately, which is the one thing this test needs not to happen.
    #[derive(Debug)]
    struct HangingExec {
        deleted: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait]
    impl SandboxDataPlaneApi for HangingExec {
        async fn create_sandbox(
            &self,
            _group: &str,
            _disk: &str,
            _cpu: &str,
            _memory: &str,
        ) -> alien_client_core::Result<alien_azure_clients::azure::sandbox_data_plane::Sandbox>
        {
            unreachable!("the deadline path never creates")
        }

        async fn get_sandbox(
            &self,
            _group: &str,
            _sandbox_id: &str,
        ) -> alien_client_core::Result<alien_azure_clients::azure::sandbox_data_plane::Sandbox>
        {
            Err(http_error(404, "SandboxNotFound"))
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
            _command: &str,
            _working_directory: Option<String>,
        ) -> alien_client_core::Result<alien_azure_clients::azure::sandbox_data_plane::ExecResult>
        {
            std::future::pending().await
        }
    }

    /// The deadline bounds untrusted code, not the caller's patience. The data plane takes no
    /// timeout, so reporting `deadlineExceeded` while the command kept running would be the
    /// containment claim this resource exists to make, unbacked. Time is paused, so the deadline
    /// arrives instantly.
    #[tokio::test(start_paused = true)]
    async fn a_command_past_its_deadline_takes_the_session_with_it() {
        let deleted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sandbox = AzureSandbox::new(
            std::sync::Arc::new(HangingExec {
                deleted: deleted.clone(),
            }),
            "grp".to_string(),
            "ubuntu".to_string(),
            "1000m".to_string(),
            "2048Mi".to_string(),
        );

        let error = sandbox
            .run_command(
                "s1",
                RunCommandRequest {
                    command: vec!["sleep".to_string(), "forever".to_string()],
                    working_directory: None,
                    env: BTreeMap::new(),
                    deadline: std::time::Duration::from_secs(30),
                },
            )
            .await
            .err()
            .expect("a command that outran its deadline has not succeeded");

        assert!(
            error.to_string().contains("deadlineExceeded"),
            "the caller has to be able to tell this apart from a command that failed: {error}"
        );
        assert!(
            deleted.load(std::sync::atomic::Ordering::SeqCst),
            "the session must actually be deleted, not merely reported as terminated"
        );
    }
}
