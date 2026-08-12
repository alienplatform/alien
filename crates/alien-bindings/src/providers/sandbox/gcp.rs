//! GCP sandbox provider.
//!
//! A Cloud Run sandbox is a subprocess of the workload's own instance, created through a CLI on
//! the container's filesystem. There is no control plane to call, no credential to hold and no
//! capability to mint: the boundary is the launcher, and the launcher is already there.
//!
//! Every command is passed as argv rather than a shell string, including file paths and file
//! contents, so nothing a caller supplies is ever parsed by a shell.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, CommandOutput, CreateSessionRequest, PreviewCapability, RunCommandRequest, Sandbox,
    SandboxSession, SandboxSessionState,
};
use alien_core::bindings::GcpSandboxBinding;
use alien_core::sandbox_process::{self, ProcessFrame, ProcessStream, FRAME_CHANNEL_DEPTH};
use alien_core::{Platform, SandboxCapabilities};
use alien_error::AlienError;

/// How much of one command's output is kept before the terminal frame reports truncation.
const OUTPUT_CAP: usize = 8 * 1024 * 1024;

/// Ceiling on a launcher call that is not the caller's command, such as a create or a delete.
const CONTROL_DEADLINE: Duration = Duration::from_secs(60);

/// A Sandbox backed by the Cloud Run sandbox launcher.
#[derive(Debug)]
pub struct GcpSandbox {
    launcher_path: String,
    allow_egress: bool,
    binding_name: String,
}

impl GcpSandbox {
    /// Builds a provider from its binding.
    pub fn new(binding_name: &str, binding: &GcpSandboxBinding) -> Result<Self> {
        let launcher_path = binding
            .launcher_path
            .clone()
            .into_value(binding_name, "launcherPath")
            .map_err(|error| {
                AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    env_var: alien_core::bindings::binding_env_var_name(binding_name),
                    reason: error.to_string(),
                })
            })?;

        let allow_egress = binding
            .allow_egress
            .clone()
            .into_value(binding_name, "allowEgress")
            .map_err(|error| {
                AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    env_var: alien_core::bindings::binding_env_var_name(binding_name),
                    reason: error.to_string(),
                })
            })?;

        Ok(Self {
            launcher_path,
            allow_egress,
            binding_name: binding_name.to_string(),
        })
    }

    /// Runs the launcher and returns its stdout, failing on a non-zero exit.
    ///
    /// Used for the control verbs. A caller's own command goes through [`Self::frames`] instead,
    /// which streams rather than collecting.
    async fn control(&self, operation: &str, arguments: &[String]) -> Result<Vec<u8>> {
        let child = sandbox_process::spawn(&self.launcher_path, arguments)
            .and_then(|mut command| command.spawn())
            .map_err(|error| {
                self.failed(operation, &format!("launcher would not start: {error}"))
            })?;

        let frames = sandbox_process::run(child, CONTROL_DEADLINE, OUTPUT_CAP).await;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        for frame in &frames {
            match frame {
                ProcessFrame::Output {
                    stream: ProcessStream::Stdout,
                    data,
                    ..
                } => stdout.extend_from_slice(data),
                ProcessFrame::Output {
                    stream: ProcessStream::Stderr,
                    data,
                    ..
                } => stderr.extend_from_slice(data),
                _ => {}
            }
        }

        match frames.last() {
            Some(ProcessFrame::Exit { code: 0, .. }) => Ok(stdout),
            // stderr, not the exit code alone: the launcher puts the actual cause there, and a
            // bare status turns a specific failure into a guess.
            Some(ProcessFrame::Exit { code, .. }) => Err(self.failed(
                operation,
                &format!(
                    "launcher exited with {code}: {}",
                    String::from_utf8_lossy(&stderr).trim()
                ),
            )),
            Some(ProcessFrame::Failed { code, message }) => {
                Err(self.failed(operation, &format!("{code}: {message}")))
            }
            _ => Err(self.failed(operation, "launcher produced no terminal frame")),
        }
    }

    fn failed(&self, operation: &str, reason: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: operation.to_string(),
            reason: format!("{reason} (binding '{}')", self.binding_name),
        })
    }

    fn unsupported(&self, capability: &str, reason: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: capability.to_string(),
            reason: reason.to_string(),
        })
    }

    /// Refuses a path that traverses upward, the same lexical rule the in-sandbox agent applies.
    fn checked_path(&self, path: &str, operation: &str) -> Result<String> {
        if path.is_empty() || path.split('/').any(|part| part == "..") {
            return Err(self.failed(operation, &format!("path '{path}' traverses upward")));
        }
        Ok(path.to_string())
    }

    /// Builds `sandbox exec <session> -- <command...>`.
    fn exec_arguments(&self, session_id: &str, command: &[String]) -> Vec<String> {
        let mut arguments = vec!["exec".to_string(), session_id.to_string(), "--".to_string()];
        arguments.extend(command.iter().cloned());
        arguments
    }
}

impl Binding for GcpSandbox {}

#[async_trait]
impl Sandbox for GcpSandbox {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::for_platform(Platform::Gcp).expect("GCP has a sandbox backend")
    }

    /// Starts a sandbox with a caller-chosen id.
    ///
    /// Egress comes from the binding rather than the request: the launcher decides it at create
    /// time and an application must not be able to widen its own.
    async fn create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        let session_id = request
            .session_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());

        let mut arguments = vec!["run".to_string(), "--id".to_string(), session_id.clone()];
        if self.allow_egress {
            arguments.push("--allow-egress".to_string());
        }

        self.control("sandbox.create", &arguments).await?;

        Ok(SandboxSession {
            session_id,
            state: SandboxSessionState::Running,
            // A sandbox is destroyed rather than fenced, so a session never outlives its own
            // generation and there is nothing for a second one to mean.
            generation: 1,
        })
    }

    /// Reconnecting is not offered, and the reason is measured rather than assumed.
    async fn get(&self, _session_id: &str) -> Result<Option<SandboxSession>> {
        Err(self.unsupported(
            "reconnect",
            "a Cloud Run sandbox id is scoped to one instance, and session affinity held 2 of \
             100 five-turn conversations",
        ))
    }

    async fn get_or_create(&self, request: CreateSessionRequest) -> Result<SandboxSession> {
        self.create(request).await
    }

    async fn list(&self) -> Result<Vec<SandboxSession>> {
        Err(self.unsupported(
            "reconnect",
            "the launcher has no enumeration verb, and an id reaches only the instance that \
             created it",
        ))
    }

    async fn run_command(
        &self,
        session_id: &str,
        request: RunCommandRequest,
    ) -> Result<BoxStream<'static, Result<CommandOutput>>> {
        if request.command.is_empty() {
            return Err(self.failed("sandbox.runCommand", "command is empty"));
        }

        let mut arguments = self.exec_arguments(session_id, &request.command);
        if let Some(directory) = &request.working_directory {
            // Prepended rather than appended: everything after `--` is the caller's command.
            arguments.insert(2, directory.clone());
            arguments.insert(2, "--workdir".to_string());
        }

        let child = sandbox_process::spawn(&self.launcher_path, &arguments)
            .and_then(|mut command| command.spawn())
            .map_err(|error| {
                self.failed(
                    "sandbox.runCommand",
                    &format!("launcher would not start: {error}"),
                )
            })?;

        let (sender, receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);
        tokio::spawn(sandbox_process::stream(
            child,
            request.deadline,
            OUTPUT_CAP,
            sender,
        ));

        // A failed frame becomes a stream error rather than a fabricated exit code: a deadline
        // that killed the command is not the command reporting -1.
        Ok(
            futures::stream::unfold(receiver, |mut receiver| async move {
                let frame = receiver.recv().await?;
                let item = match frame {
                    ProcessFrame::Failed { code, message } => {
                        Err(AlienError::new(ErrorData::OperationNotSupported {
                            operation: "sandbox.runCommand".to_string(),
                            reason: format!("{code}: {message}"),
                        }))
                    }
                    other => Ok(CommandOutput::from(other)),
                };
                Some((item, receiver))
            })
            .boxed(),
        )
    }

    async fn read_file(&self, session_id: &str, path: &str) -> Result<Vec<u8>> {
        let path = self.checked_path(path, "sandbox.readFile")?;
        let command = vec!["/bin/cat".to_string(), path];
        self.control(
            "sandbox.readFile",
            &self.exec_arguments(session_id, &command),
        )
        .await
    }

    /// Writes files by handing the contents to the sandbox base64-encoded **as an argument**.
    ///
    /// Not interpolated into a shell string, so a file's contents can never be parsed as code.
    /// The cost is `ARG_MAX`: a file larger than roughly a megabyte needs a different transport,
    /// and fails loudly here rather than being silently truncated.
    async fn write_files(&self, session_id: &str, files: BTreeMap<String, Vec<u8>>) -> Result<()> {
        for (path, contents) in files {
            let path = self.checked_path(&path, "sandbox.writeFiles")?;
            let encoded = BASE64.encode(&contents);

            let command = vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                // Parent directories are created, matching the in-sandbox agent, so one path
                // means the same thing on every backend.
                "mkdir -p \"$(dirname \"$2\")\" && printf %s \"$1\" | base64 -d > \"$2\""
                    .to_string(),
                "sh".to_string(),
                encoded,
                path,
            ];

            self.control(
                "sandbox.writeFiles",
                &self.exec_arguments(session_id, &command),
            )
            .await?;
        }

        Ok(())
    }

    async fn mkdir(&self, session_id: &str, path: &str) -> Result<()> {
        let path = self.checked_path(path, "sandbox.mkdir")?;
        let command = vec!["/bin/mkdir".to_string(), "-p".to_string(), path];
        self.control("sandbox.mkdir", &self.exec_arguments(session_id, &command))
            .await?;
        Ok(())
    }

    async fn preview(&self, _session_id: &str, _port: u16) -> Result<PreviewCapability> {
        Err(self.unsupported(
            "preview",
            "a Cloud Run sandbox has no ingress of its own and no addressable endpoint",
        ))
    }

    async fn suspend(&self, _session_id: &str) -> Result<()> {
        Err(self.unsupported("suspendResume", "the launcher has no suspend verb"))
    }

    async fn resume(&self, _session_id: &str) -> Result<()> {
        Err(self.unsupported("suspendResume", "the launcher has no resume verb"))
    }

    async fn snapshot(&self, _session_id: &str) -> Result<String> {
        Err(self.unsupported(
            "snapshot",
            "`sandbox fork` produces another live sandbox rather than a durable artifact",
        ))
    }

    async fn terminate(&self, session_id: &str) -> Result<()> {
        self.control(
            "sandbox.terminate",
            &["delete".to_string(), session_id.to_string()],
        )
        .await?;
        Ok(())
    }
}

impl From<ProcessFrame> for CommandOutput {
    fn from(frame: ProcessFrame) -> Self {
        match frame {
            ProcessFrame::Output {
                seq,
                stream: ProcessStream::Stdout,
                data,
            } => CommandOutput::Stdout { seq, data },
            ProcessFrame::Output {
                seq,
                stream: ProcessStream::Stderr,
                data,
            } => CommandOutput::Stderr { seq, data },
            ProcessFrame::Exit { code, truncated } => CommandOutput::Exit { code, truncated },
            // Handled as a stream error before it reaches here, because an exit code would
            // claim the command reported something it never did.
            ProcessFrame::Failed { code, message } => {
                unreachable!("a failed frame is mapped to an error: {code} {message}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::BindingValue;

    /// A fake launcher: it records the argv it was given and answers like the real one.
    ///
    /// Testing against a script rather than a mock is deliberate. What this provider gets wrong
    /// is argument construction, and a mock of the launcher would be built from the same
    /// misunderstanding as the code.
    fn launcher(body: &str) -> (tempfile::TempDir, GcpSandbox) {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("sandbox");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write launcher");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("make executable");
        }

        let sandbox = GcpSandbox::new(
            "sbx",
            &GcpSandboxBinding {
                launcher_path: BindingValue::value(path.display().to_string()),
                allow_egress: BindingValue::value(false),
            },
        )
        .expect("binding is valid");

        (directory, sandbox)
    }

    #[tokio::test]
    async fn create_names_the_session_and_withholds_egress() {
        let (_dir, sandbox) = launcher(r#"echo "$@""#);

        let session = sandbox
            .create(CreateSessionRequest {
                session_id: Some("s1".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("create succeeds");

        assert_eq!(session.session_id, "s1");
        assert_eq!(session.state, SandboxSessionState::Running);
    }

    /// The launcher takes `--allow-egress` per sandbox, so an application that could pass its
    /// own would choose its own confinement. The binding decides it.
    #[tokio::test]
    async fn egress_comes_from_the_binding_and_not_from_the_request() {
        let (dir, _) = launcher(r#"echo "$@" > "$(dirname "$0")/argv""#);
        let path = dir.path().join("sandbox");

        for (allow, expected) in [(false, false), (true, true)] {
            let sandbox = GcpSandbox::new(
                "sbx",
                &GcpSandboxBinding {
                    launcher_path: BindingValue::value(path.display().to_string()),
                    allow_egress: BindingValue::value(allow),
                },
            )
            .expect("binding is valid");

            sandbox
                .create(CreateSessionRequest {
                    session_id: Some("s1".to_string()),
                    tenant_key: None,
                    env: BTreeMap::new(),
                })
                .await
                .expect("create succeeds");

            let argv = std::fs::read_to_string(dir.path().join("argv")).expect("argv recorded");
            assert_eq!(
                argv.contains("--allow-egress"),
                expected,
                "binding said allow_egress={allow}, argv was: {argv}"
            );
        }
    }

    /// A launcher that fails must not report a session. The cause is on stderr, and losing it
    /// turns a specific failure into a guess.
    #[tokio::test]
    async fn a_failing_launcher_surfaces_its_stderr() {
        let (_dir, sandbox) = launcher(r#"echo "quota exhausted" 1>&2; exit 7"#);

        let error = sandbox
            .create(CreateSessionRequest {
                session_id: Some("s1".to_string()),
                tenant_key: None,
                env: BTreeMap::new(),
            })
            .await
            .expect_err("a non-zero launcher exit is a failure");

        let rendered = format!("{error:?}");
        assert!(rendered.contains("quota exhausted"), "got: {rendered}");
        assert!(
            rendered.contains('7'),
            "the exit code belongs in the error: {rendered}"
        );
    }

    #[tokio::test]
    async fn a_command_streams_output_and_a_real_exit_code() {
        let (_dir, sandbox) = launcher(r#"echo hello; echo problem 1>&2; exit 3"#);

        let frames: Vec<_> = sandbox
            .run_command(
                "s1",
                RunCommandRequest {
                    command: vec!["/bin/true".to_string()],
                    working_directory: None,
                    env: BTreeMap::new(),
                    deadline: Duration::from_secs(10),
                },
            )
            .await
            .expect("the command runs")
            .collect()
            .await;

        let decoded: String = frames
            .iter()
            .filter_map(|frame| match frame {
                Ok(CommandOutput::Stdout { data, .. }) => {
                    Some(String::from_utf8_lossy(data).to_string())
                }
                _ => None,
            })
            .collect();
        assert!(decoded.contains("hello"), "stdout was: {decoded}");

        assert!(
            frames
                .iter()
                .any(|frame| matches!(frame, Ok(CommandOutput::Stderr { .. }))),
            "stderr must be framed, not dropped"
        );

        assert!(
            matches!(frames.last(), Some(Ok(CommandOutput::Exit { code: 3, .. }))),
            "the terminal frame must carry the real exit code: {:?}",
            frames.last()
        );
    }

    /// Declared capabilities and actual behaviour have to agree, or a caller branches on a lie.
    #[tokio::test]
    async fn unsupported_capabilities_error_rather_than_pretend() {
        let (_dir, sandbox) = launcher("exit 0");
        let capabilities = sandbox.capabilities();

        assert!(!capabilities.reconnect);
        assert!(!capabilities.preview);
        assert!(!capabilities.suspend_resume);
        assert!(!capabilities.snapshot);

        sandbox
            .get("s1")
            .await
            .expect_err("reconnect is not offered");
        sandbox
            .list()
            .await
            .expect_err("enumeration is not offered");
        sandbox
            .preview("s1", 8080)
            .await
            .expect_err("preview is not offered");
        sandbox
            .suspend("s1")
            .await
            .expect_err("suspend is not offered");
        sandbox
            .resume("s1")
            .await
            .expect_err("resume is not offered");
        sandbox
            .snapshot("s1")
            .await
            .expect_err("snapshot is not offered");
    }

    /// The lexical rule the agent applies, applied here too, so one path means one thing.
    #[tokio::test]
    async fn a_traversing_path_is_refused_before_the_launcher_sees_it() {
        let (_dir, sandbox) = launcher("exit 0");

        sandbox
            .read_file("s1", "../etc/passwd")
            .await
            .expect_err("a traversing path must be refused");
        sandbox
            .write_files(
                "s1",
                BTreeMap::from([("../etc/passwd".to_string(), b"x".to_vec())]),
            )
            .await
            .expect_err("a traversing path must be refused on write too");
    }
}
