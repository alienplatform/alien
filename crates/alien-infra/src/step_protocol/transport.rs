//! How a step reaches an extension's controller: a WebAssembly module in this process (`wasm`),
//! or a local process over stdin and stdout. Each returns the parsed reply, or a
//! [`TransportFailure`] that says whether the executor retries.

use super::protocol::{
    codes, LoadOp, LoadRequest, LoadResponse, LoadResult, StepRequest, StepResponse,
    PROTOCOL_VERSION,
};
use alien_client_core::interceptor::{Cloud, InterceptorChain};
use alien_core::Platform;
use async_trait::async_trait;
use serde_json::Value;
use std::fmt::Debug;
use std::time::Duration;

/// How long connecting to a server a controller module calls, TLS included, may take. The
/// step's own timeout bounds the rest.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a step got no usable reply.
#[derive(Debug, Clone, PartialEq)]
pub struct TransportFailure {
    /// One of [`codes`].
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl TransportFailure {
    pub(super) fn unreachable(message: impl Into<String>) -> Self {
        Self {
            code: codes::UNREACHABLE,
            message: message.into(),
            retryable: true,
        }
    }

    pub(super) fn protocol_error(message: impl Into<String>) -> Self {
        Self {
            code: codes::PROTOCOL_ERROR,
            message: message.into(),
            retryable: false,
        }
    }
}

/// A deployment's interceptors for a cloud's calls, as
/// [`crate::PlatformServiceProvider::interceptors`] gives them: the second argument is the
/// tenant an Azure call is made in.
pub type Interceptors =
    std::sync::Arc<dyn Fn(Cloud, Option<&str>) -> InterceptorChain + Send + Sync>;

/// Sends one step to an extension's controller and waits for its reply.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Transport: Send + Sync + Debug {
    /// Fails when the controller can't be reached at all, so a step that would go nowhere asks
    /// for no credentials.
    async fn ready(&self) -> std::result::Result<(), TransportFailure> {
        Ok(())
    }

    /// This transport with `interceptors`, the stepping deployment's, on every cloud call it
    /// makes for the controller. `None` for a transport that makes none, because its controller
    /// calls its cloud itself.
    fn intercepted(&self, _interceptors: Interceptors) -> Option<std::sync::Arc<dyn Transport>> {
        None
    }

    /// Gives up after `timeout`.
    async fn step(
        &self,
        request: &StepRequest,
        timeout: Duration,
    ) -> std::result::Result<StepResponse, TransportFailure>;
}

/// Reads the reply's version before its schema, so a newer controller's reply is refused for
/// its version and not for a field this side doesn't know.
pub(super) fn parse_response(
    body: &str,
    request: &StepRequest,
) -> std::result::Result<StepResponse, TransportFailure> {
    let raw: Value = serde_json::from_str(body)
        .map_err(|e| TransportFailure::protocol_error(format!("the reply is not JSON: {e}")))?;
    check_reply_version(&raw)?;
    let response: StepResponse = serde_json::from_value(raw).map_err(|e| {
        TransportFailure::protocol_error(format!("the reply is not a step reply: {e}"))
    })?;
    if response.step_id != request.step_id {
        return Err(TransportFailure::protocol_error(format!(
            "the reply is for step {} but step {} was sent",
            response.step_id, request.step_id
        )));
    }
    Ok(response)
}

pub(super) fn check_reply_version(raw: &Value) -> std::result::Result<(), TransportFailure> {
    let protocol = raw
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| TransportFailure::protocol_error("the reply has no protocol version"))?;
    if protocol > u64::from(PROTOCOL_VERSION) {
        return Err(TransportFailure {
            code: codes::PROTOCOL_UNSUPPORTED,
            message: format!(
                "the controller answered with step protocol {protocol} but this deployment speaks up to {PROTOCOL_VERSION}; upgrade Alien or pin an older controller"
            ),
            retryable: false,
        });
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub use stdio::StdioTransport;

#[cfg(not(target_arch = "wasm32"))]
mod stdio {
    use super::*;
    use alien_error::{AlienError, Context, IntoAlienError};
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::{Child, ChildStdin, ChildStdout, Command};
    use tokio::sync::Mutex;

    struct ChildIo {
        child: Child,
        /// The child's pid, which is its process group's id. Kept from the spawn: `Child::id`
        /// is gone once the leader is reaped, while what it started may still run.
        group: Option<u32>,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        /// Set while a request is out and its reply not yet read. A call that finds it set
        /// follows one that was dropped midway, so a reply it never read, or half a request,
        /// may still be in the pipe: the process is replaced.
        mid_exchange: bool,
    }

    impl ChildIo {
        /// Stops the controller and anything it started: it runs in its own process group.
        async fn kill(mut self) {
            self.kill_group();
            // Once the leader is reaped its id may be reused, so `Drop` doesn't signal it again.
            self.group = None;
            let _ = self.child.kill().await;
        }

        /// Signals the whole group, whether or not its leader is still alive.
        fn kill_group(&self) {
            #[cfg(unix)]
            if let Some(group) = self.group {
                // SAFETY: signals the process group this transport created for the child.
                unsafe {
                    libc::killpg(group as libc::pid_t, libc::SIGKILL);
                }
            }
        }
    }

    impl Drop for ChildIo {
        fn drop(&mut self) {
            // `kill_on_drop` reaches the child alone.
            self.kill_group();
        }
    }

    /// A controller process that reads one request per line on stdin and writes one reply per
    /// line on stdout.
    ///
    /// One request is in flight at a time: the executor steps several resources at once, and
    /// the others wait for the lock. The process starts on first use, and again after it exits
    /// or misses a deadline, since a late reply would answer the next request.
    ///
    /// The process gets only the system variables a program needs to start and the host's proxy
    /// and certificate settings from this process's environment, and the transport's own
    /// variables, so nothing else this process holds, such as its credentials, reaches it.
    pub struct StdioTransport {
        program: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
        child: Mutex<Option<ChildIo>>,
        pid: std::sync::Mutex<Option<u32>>,
        /// The verified copy `program` names, for a transport made with [`Self::verified`].
        _copy: Option<PrivateCopy>,
    }

    /// The variables of this process's environment a controller process needs to start.
    #[cfg(not(windows))]
    const SYSTEM_ENV: &[&str] = &["PATH", "HOME", "TMPDIR"];
    #[cfg(windows)]
    const SYSTEM_ENV: &[&str] = &[
        "PATH",
        "SystemRoot",
        "windir",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "PATHEXT",
        "COMSPEC",
    ];

    /// The host's proxy and certificate settings, which a controller process also gets: they are
    /// the host's own network setup, not credentials.
    const NETWORK_ENV: [&str; 8] = [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ];

    /// The start of the name of every private copy folder this process makes. A copy folder
    /// named otherwise was left by a process that ended without removing it.
    fn run_prefix() -> &'static str {
        static PREFIX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        PREFIX.get_or_init(|| format!(".run-{}-", uuid::Uuid::new_v4().simple()))
    }

    /// A copy of a controller binary in a folder only this process can open, removed with the
    /// transport that runs it.
    struct PrivateCopy {
        dir: PathBuf,
    }

    impl Drop for PrivateCopy {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.dir) {
                tracing::warn!(dir = %self.dir.display(), %error, "Could not remove a controller binary's private copy");
            }
        }
    }

    impl Debug for StdioTransport {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("StdioTransport")
                .field("program", &self.program)
                .field("args", &self.args)
                .finish_non_exhaustive()
        }
    }

    impl StdioTransport {
        /// The process runs `program`, with `env` beside the variables it inherits.
        pub fn new(
            program: impl Into<String>,
            args: Vec<String>,
            env: Vec<(String, String)>,
        ) -> Self {
            Self {
                program: program.into(),
                args,
                env,
                child: Mutex::new(None),
                pid: std::sync::Mutex::new(None),
                _copy: None,
            }
        }

        /// The process for extension `extension`'s controller `binary`, which must have the
        /// sha256 `sha256`. The bytes it checks are copied into a new folder under `copies` that
        /// only this process can open, and every start of the process runs that copy, so a file
        /// replaced at `binary` after the check never runs.
        pub async fn verified(
            extension: &str,
            binary: &Path,
            sha256: &str,
            copies: &Path,
            env: Vec<(String, String)>,
        ) -> crate::error::Result<Self> {
            let object = "controller";
            let unavailable =
                |message: String| crate::error::ErrorData::ExtensionBundleUnavailable {
                    extension: extension.to_string(),
                    object: object.to_string(),
                    message,
                };
            let bytes = tokio::fs::read(binary)
                .await
                .into_alien_error()
                .context(unavailable(format!("{} is unreadable", binary.display())))?;
            crate::extension_bundle::verify_sha256(extension, object, &bytes, sha256)?;
            let name = binary.file_name().ok_or_else(|| {
                AlienError::new(unavailable(format!("{} names no file", binary.display())))
            })?;
            let dir = copies.join(format!("{}{}", run_prefix(), uuid::Uuid::new_v4().simple()));
            let copy = PrivateCopy { dir: dir.clone() };
            let path = dir.join(name);
            write_private(copies, &dir, &path, &bytes)
                .await
                .into_alien_error()
                .context(unavailable(format!(
                    "its private copy in {} could not be written",
                    dir.display()
                )))?;
            Ok(Self {
                program: path.display().to_string(),
                args: Vec::new(),
                env,
                child: Mutex::new(None),
                pid: std::sync::Mutex::new(None),
                _copy: Some(copy),
            })
        }

        /// The running process's id, if one is running.
        pub fn child_pid(&self) -> Option<u32> {
            *self.pid.lock().unwrap_or_else(|e| e.into_inner())
        }

        /// Asks the controller to load `state` for `resource_type` on `platform`, without
        /// stepping and without a cloud call, and returns the view it computes from it, or
        /// why it can't read it.
        pub async fn load(
            &self,
            resource_type: &str,
            platform: Platform,
            state: Value,
            timeout: Duration,
        ) -> std::result::Result<LoadResult, TransportFailure> {
            let request = LoadRequest {
                protocol: PROTOCOL_VERSION,
                op: LoadOp::Load,
                resource_type: resource_type.to_string(),
                platform,
                state,
            };
            self.exchange(
                || &request,
                timeout,
                |line| {
                    let raw: Value = serde_json::from_str(line).map_err(|e| {
                        TransportFailure::protocol_error(format!("the reply is not JSON: {e}"))
                    })?;
                    check_reply_version(&raw)?;
                    let response: LoadResponse = serde_json::from_value(raw).map_err(|e| {
                        TransportFailure::protocol_error(format!(
                            "the reply is not a load reply: {e}"
                        ))
                    })?;
                    Ok(response.result)
                },
            )
            .await
        }

        async fn spawn(&self) -> std::result::Result<ChildIo, TransportFailure> {
            let mut command = Command::new(&self.program);
            command
                .args(&self.args)
                .env_clear()
                .envs(
                    SYSTEM_ENV
                        .iter()
                        .chain(NETWORK_ENV.iter())
                        .filter_map(|name| std::env::var_os(name).map(|value| (*name, value))),
                )
                .envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .kill_on_drop(true);
            #[cfg(unix)]
            command.process_group(0);
            let mut child = command.spawn().map_err(|e| {
                TransportFailure::unreachable(format!("cannot start {}: {e}", self.program))
            })?;
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                return Err(TransportFailure::unreachable(format!(
                    "{} started without piped stdin and stdout",
                    self.program
                )));
            };
            *self.pid.lock().unwrap_or_else(|e| e.into_inner()) = child.id();
            Ok(ChildIo {
                group: child.id(),
                child,
                stdin,
                stdout: BufReader::new(stdout),
                mid_exchange: false,
            })
        }

        /// Writes one line, reads one line back and parses it with `parse`. A process that
        /// exits, can't be written to, misses `timeout`, or answers with a line `parse`
        /// refuses is killed, and the next call starts a new one: its next line could answer
        /// the wrong request.
        ///
        /// Sends the line `message` builds once the process is free, so a request stamped with
        /// the time, as a step's deadline is, is stamped after any wait for another step.
        async fn exchange<T, M: serde::Serialize>(
            &self,
            message: impl FnOnce() -> M,
            timeout: Duration,
            parse: impl FnOnce(&str) -> std::result::Result<T, TransportFailure>,
        ) -> std::result::Result<T, TransportFailure> {
            let mut slot = self.child.lock().await;
            let mut line = serde_json::to_string(&message()).map_err(|e| {
                TransportFailure::protocol_error(format!("cannot serialize the request: {e}"))
            })?;
            line.push('\n');

            if slot.as_ref().is_some_and(|io| io.mid_exchange) {
                if let Some(io) = slot.take() {
                    io.kill().await;
                }
            }
            if slot.is_none() {
                *slot = Some(self.spawn().await?);
            }
            let Some(io) = slot.as_mut() else {
                return Err(TransportFailure::unreachable(
                    "the controller process is gone",
                ));
            };
            io.mid_exchange = true;
            // One limit covers the write too: a process that stops reading its stdin would
            // otherwise hold the step, and the lock, once the request fills the pipe.
            let exchange = async {
                io.stdin.write_all(line.as_bytes()).await.map_err(|e| {
                    TransportFailure::unreachable(format!(
                        "cannot write to the controller process: {e}"
                    ))
                })?;
                let mut reply = String::new();
                match io.stdout.read_line(&mut reply).await {
                    Err(e) => Err(TransportFailure::unreachable(format!(
                        "cannot read from the controller process: {e}"
                    ))),
                    Ok(0) => Err(TransportFailure::unreachable(
                        "the controller process exited before answering",
                    )),
                    Ok(_) => Ok(reply.trim_end().to_string()),
                }
            };
            let result = match tokio::time::timeout(timeout, exchange).await {
                Ok(result) => result.and_then(|reply| parse(&reply)),
                Err(_) => Err(TransportFailure::unreachable(format!(
                    "the controller process did not answer within {timeout:?}"
                ))),
            };
            match &result {
                Ok(_) => {
                    if let Some(io) = slot.as_mut() {
                        io.mid_exchange = false;
                    }
                }
                Err(_) => {
                    if let Some(io) = slot.take() {
                        io.kill().await;
                    }
                    *self.pid.lock().unwrap_or_else(|e| e.into_inner()) = None;
                }
            }
            result
        }
    }

    /// Writes `bytes` to `path` in a new folder `dir` under `copies` that only this user can
    /// open, as a file only this user can read and run.
    async fn write_private(
        copies: &Path,
        dir: &Path,
        path: &Path,
        bytes: &[u8],
    ) -> std::io::Result<()> {
        tokio::fs::create_dir_all(copies).await?;
        let mut folder = tokio::fs::DirBuilder::new();
        #[cfg(unix)]
        folder.mode(0o700);
        folder.create(dir).await?;
        tokio::fs::write(path, bytes).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500)).await?;
        }
        Ok(())
    }

    #[async_trait]
    impl Transport for StdioTransport {
        async fn step(
            &self,
            request: &StepRequest,
            timeout: Duration,
        ) -> std::result::Result<StepResponse, TransportFailure> {
            // The controller gets its deadline counted from when it can start on the step, and
            // the host waits `HOST_GRACE` past it for the reply.
            let Some(deadline) = timeout
                .checked_sub(crate::step_protocol::HOST_GRACE)
                .filter(|deadline| !deadline.is_zero())
            else {
                return Err(TransportFailure::protocol_error(format!(
                    "a step's timeout of {timeout:?} leaves the controller no time before the host's grace of {:?}",
                    crate::step_protocol::HOST_GRACE
                )));
            };
            let stamped = || StepRequest {
                deadline: (chrono::Utc::now()
                    + chrono::Duration::from_std(deadline).unwrap_or(chrono::Duration::MAX))
                .to_rfc3339(),
                ..request.clone()
            };
            self.exchange(stamped, timeout, |line| parse_response(line, request))
                .await
        }
    }
}
