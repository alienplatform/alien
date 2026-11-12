//! How a step reaches an extension's controller: a local process over stdin and stdout, or the
//! developer's server over HTTPS. Both return the parsed reply, or a [`TransportFailure`] that
//! says whether the executor retries.

use super::protocol::{
    codes, LoadOp, LoadRequest, LoadResponse, LoadResult, ResponseBody, StepRequest, StepResponse,
    PROTOCOL_VERSION,
};
use super::token::{self, Claims};
use crate::error::Result;
use alien_core::Platform;
use async_trait::async_trait;
use ed25519_compact::SecretKey;
use serde_json::Value;
use std::fmt::Debug;
use std::time::Duration;

/// How long connecting to a controller's server, TLS included, may take. The step's own timeout
/// bounds the rest.
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
    fn unreachable(message: impl Into<String>) -> Self {
        Self {
            code: codes::UNREACHABLE,
            message: message.into(),
            retryable: true,
        }
    }

    fn protocol_error(message: impl Into<String>) -> Self {
        Self {
            code: codes::PROTOCOL_ERROR,
            message: message.into(),
            retryable: false,
        }
    }
}

/// Sends one step to an extension's controller and waits for its reply.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Transport: Send + Sync + Debug {
    /// Fails when the controller can't be reached at all, so a step that would go nowhere asks
    /// for no credentials.
    async fn ready(&self) -> std::result::Result<(), TransportFailure> {
        Ok(())
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
fn parse_response(
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

fn check_reply_version(raw: &Value) -> std::result::Result<(), TransportFailure> {
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
pub use stdio::{remove_stale_private_copies, StdioTransport};

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

    /// Removes the private copy folders under each folder in `root` that no running transport of
    /// this process made: those a process left when it ended without removing them. Only one
    /// process may run controllers from `root`, or this removes the copies another runs.
    pub async fn remove_stale_private_copies(root: &Path) {
        let Ok(mut folders) = tokio::fs::read_dir(root).await else {
            return;
        };
        while let Ok(Some(folder)) = folders.next_entry().await {
            let Ok(mut entries) = tokio::fs::read_dir(folder.path()).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".run-") && !name.starts_with(run_prefix()) {
                    let path = entry.path();
                    if let Err(error) = tokio::fs::remove_dir_all(&path).await {
                        tracing::warn!(dir = %path.display(), %error, "Could not remove a stale private copy of a controller binary");
                    }
                }
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

/// Who the host signs step tokens as, with which key.
pub struct TokenSigner {
    /// The `iss` claim: the URL of the Alien manager that signs.
    pub issuer: String,
    /// The `project` claim: the project of the deployment the transport steps.
    pub project: String,
    pub secret_key: SecretKey,
}

impl Debug for TokenSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSigner")
            .field("issuer", &self.issuer)
            .field("project", &self.project)
            .finish_non_exhaustive()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use http::HttpTransport;

#[cfg(not(target_arch = "wasm32"))]
mod http {
    use super::*;

    /// The largest reply a controller's server may send. Its state and outputs are saved with the
    /// deployment, so the limit matches what a hook may return.
    const MAX_REPLY_BYTES: usize = 16 << 20;

    /// How much of a body that isn't a reply an error message quotes.
    const QUOTED_BODY_BYTES: usize = 512;

    /// `POST {controller_url}/v1/step`, with a token signed for each request.
    ///
    /// Each step resolves the server's host again and connects only to the public addresses it
    /// resolves to, so a URL that later resolves into a private network reaches nothing there.
    /// A transport made with [`Self::to_own_workload`] has no such rule.
    #[derive(Debug)]
    pub struct HttpTransport {
        controller_url: String,
        step_url: String,
        signer: TokenSigner,
        /// The host started the server itself, at an address it chose.
        own_workload: bool,
        /// Admits a server on this machine, and these roots for its certificate.
        #[cfg(any(test, feature = "test-utils"))]
        local: Option<Vec<reqwest::Certificate>>,
    }

    impl HttpTransport {
        pub fn new(controller_url: impl Into<String>, signer: TokenSigner) -> Self {
            let controller_url = controller_url.into();
            let step_url = format!("{}/v1/step", controller_url.trim_end_matches('/'));
            Self {
                controller_url,
                step_url,
                signer,
                own_workload: false,
                #[cfg(any(test, feature = "test-utils"))]
                local: None,
            }
        }

        /// Reaches a server the host itself started at `controller_url`, an address the host
        /// chose, such as the Service in front of a pod it created: over plain HTTP, at a
        /// private address. Never for a URL a release names, which [`Self::new`] holds to
        /// public addresses.
        pub fn to_own_workload(controller_url: impl Into<String>, signer: TokenSigner) -> Self {
            Self {
                own_workload: true,
                ..Self::new(controller_url, signer)
            }
        }

        /// Reaches a server a test runs on this machine, over `http` or over `https` with a
        /// certificate one of `roots` signed.
        #[cfg(any(test, feature = "test-utils"))]
        pub fn allowing_local(mut self, roots: Vec<reqwest::Certificate>) -> Self {
            self.local = Some(roots);
            self
        }

        async fn client(&self) -> std::result::Result<reqwest::Client, TransportFailure> {
            let url = reqwest::Url::parse(&self.step_url).map_err(|e| {
                TransportFailure::unreachable(format!("{} is not a URL: {e}", self.step_url))
            })?;
            #[allow(unused_mut)]
            let mut builder = reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT);
            if self.own_workload {
                return builder
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|e| {
                        TransportFailure::protocol_error(format!("cannot build the client: {e}"))
                    });
            }
            #[allow(unused_mut)]
            let mut allow_local = false;
            #[cfg(any(test, feature = "test-utils"))]
            if let Some(roots) = &self.local {
                allow_local = true;
                for root in roots {
                    builder = builder.add_root_certificate(root.clone());
                }
            }
            crate::public_host::pinned_client(&url, builder, allow_local)
                .await
                .map_err(|refusal| {
                    let mut failure = TransportFailure::unreachable(format!(
                        "cannot reach {}: {refusal}",
                        self.step_url
                    ));
                    // A URL that breaks a rule breaks it on every try.
                    failure.retryable =
                        matches!(refusal, crate::public_host::Refusal::Unreachable(_));
                    failure
                })
        }

        fn token(&self, request: &StepRequest, body: &[u8]) -> Result<String> {
            let now = chrono::Utc::now().timestamp();
            let extension = token::extension_of(&request.resource_type);
            token::mint(
                &Claims {
                    iss: self.signer.issuer.clone(),
                    aud: self.controller_url.clone(),
                    project: self.signer.project.clone(),
                    deployment_id: request.deployment_id.clone(),
                    extension: extension.to_string(),
                    resource_id: request.resource_id.clone(),
                    step_id: request.step_id.clone(),
                    body_sha256: token::body_sha256(body),
                    iat: now,
                    exp: now + token::TOKEN_LIFETIME.as_secs() as i64,
                    jti: uuid::Uuid::new_v4().to_string(),
                },
                &self.signer.secret_key,
            )
        }

        /// The reply's body, refused once it passes [`MAX_REPLY_BYTES`].
        async fn body(
            &self,
            mut response: reqwest::Response,
        ) -> std::result::Result<Vec<u8>, TransportFailure> {
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| {
                TransportFailure::unreachable(format!(
                    "cannot read the reply from {}: {e}",
                    self.step_url
                ))
            })? {
                if body.len() + chunk.len() > MAX_REPLY_BYTES {
                    return Err(TransportFailure::protocol_error(format!(
                        "{} answered with more than {} MiB",
                        self.step_url,
                        MAX_REPLY_BYTES >> 20
                    )));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(body)
        }
    }

    /// The start of `body`, for an error message.
    fn quoted(body: &[u8]) -> String {
        let text = String::from_utf8_lossy(&body[..body.len().min(QUOTED_BODY_BYTES)]);
        if body.len() > QUOTED_BODY_BYTES {
            format!("{text}...")
        } else {
            text.into_owned()
        }
    }

    #[async_trait]
    impl Transport for HttpTransport {
        /// The step itself resolves the host again, so an address that changes in between is
        /// checked too.
        async fn ready(&self) -> std::result::Result<(), TransportFailure> {
            self.client().await.map(|_| ())
        }

        async fn step(
            &self,
            request: &StepRequest,
            timeout: Duration,
        ) -> std::result::Result<StepResponse, TransportFailure> {
            let protocol_error = |message: String| TransportFailure {
                code: codes::PROTOCOL_ERROR,
                message,
                retryable: false,
            };
            let body = serde_json::to_vec(request)
                .map_err(|e| protocol_error(format!("cannot serialize the step: {e}")))?;
            let token = self
                .token(request, &body)
                .map_err(|e| protocol_error(e.message))?;
            let sent = self
                .client()
                .await?
                .post(&self.step_url)
                .bearer_auth(token)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .timeout(timeout)
                .send()
                .await
                .map_err(|e| {
                    TransportFailure::unreachable(format!("cannot reach {}: {e}", self.step_url))
                })?;
            let status = sent.status();
            let body = self.body(sent).await?;
            let reply = || String::from_utf8_lossy(&body);
            match status.as_u16() {
                200 => parse_response(&reply(), request),
                400 => match parse_response(&reply(), request)? {
                    response @ StepResponse {
                        body: ResponseBody::Refused { .. },
                        ..
                    } => Ok(response),
                    _ => Err(TransportFailure::protocol_error(format!(
                        "{} answered 400 with a reply that is not `refused`",
                        self.step_url
                    ))),
                },
                401 | 403 => Err(TransportFailure {
                    code: codes::AUTH_REJECTED,
                    message: format!(
                        "{} refused the step's token ({status}): {}",
                        self.step_url,
                        quoted(&body)
                    ),
                    retryable: false,
                }),
                500..=599 => Err(TransportFailure::unreachable(format!(
                    "{} answered {status}: {}",
                    self.step_url,
                    quoted(&body)
                ))),
                _ => Err(TransportFailure::protocol_error(format!(
                    "{} answered {status}, which the step protocol doesn't use: {}",
                    self.step_url,
                    quoted(&body)
                ))),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use ed25519_compact::{KeyPair, Seed};

        fn request() -> StepRequest {
            serde_json::from_value(serde_json::json!({
                "protocol": 1, "stepId": "s1", "deploymentId": "dep_1",
                "resourceId": "zone", "resourceType": "acme/dns-zone", "platform": "test",
                "resourcePrefix": "p", "initialSetupAuthority": "importedHandoff",
                "lifecycle": "live", "resource": { "type": "acme/dns-zone", "id": "zone" },
                "previousResource": null, "dependencies": {},
                "deploymentConfig": {}, "setupRun": false, "setupRunId": null, "state": null,
                "transitions": [], "credentials": null, "deadline": "2099-01-01T00:00:00Z"
            }))
            .unwrap()
        }

        /// A server on loopback that answers every step with `status` and `body`.
        async fn answering(status: u16, body: Vec<u8>) -> HttpTransport {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let app = axum::Router::new().route(
                "/v1/step",
                axum::routing::post(move || {
                    let body = body.clone();
                    async move { (axum::http::StatusCode::from_u16(status).unwrap(), body) }
                }),
            );
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            HttpTransport::new(
                url,
                TokenSigner {
                    issuer: "https://manager.example.com".to_string(),
                    project: "prj_1".to_string(),
                    secret_key: KeyPair::from_seed(Seed::new([1; 32])).sk,
                },
            )
            .allowing_local(Vec::new())
        }

        /// A server at a private address is refused for a URL a release names, and reached when
        /// the host started it itself.
        #[tokio::test]
        async fn only_the_host_s_own_workload_is_reached_at_a_private_address() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let app = axum::Router::new().route(
                "/v1/step",
                axum::routing::post(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
            );
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let signer = || TokenSigner {
                issuer: "https://manager.example.com".to_string(),
                project: "prj_1".to_string(),
                secret_key: KeyPair::from_seed(Seed::new([1; 32])).sk,
            };

            let named = HttpTransport::new(url.clone(), signer());
            let refused = named.ready().await.unwrap_err();
            assert!(!refused.retryable, "{refused:?}");
            assert!(refused.message.contains("not https"), "{}", refused.message);

            let own = HttpTransport::to_own_workload(url, signer());
            own.ready().await.unwrap();
            let answered = own
                .step(&request(), Duration::from_secs(10))
                .await
                .unwrap_err();
            assert!(
                answered.message.contains("answered 503"),
                "{}",
                answered.message
            );
        }

        /// A reply over the limit fails the step without being read whole, and an error quotes
        /// only the start of the body the server sent.
        #[tokio::test]
        async fn a_reply_is_read_only_up_to_its_limit() {
            let oversized = answering(200, vec![b' '; MAX_REPLY_BYTES + 1]).await;
            let failure = oversized
                .step(&request(), Duration::from_secs(10))
                .await
                .unwrap_err();
            assert_eq!(failure.code, codes::PROTOCOL_ERROR);
            assert!(!failure.retryable);
            assert!(
                failure.message.contains("more than 16 MiB"),
                "{}",
                failure.message
            );

            let failing = answering(503, vec![b'x'; 10_000]).await;
            let failure = failing
                .step(&request(), Duration::from_secs(10))
                .await
                .unwrap_err();
            assert_eq!(failure.code, codes::UNREACHABLE);
            assert!(failure.message.len() < 1_000, "{}", failure.message);
        }
    }
}
