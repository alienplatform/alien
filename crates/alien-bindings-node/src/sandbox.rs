//! Sandbox binding handle. Thin argument/error translation over the `Sandbox` trait.
//!
//! One thing here is not thin: a command's output is a stream, and every other handle in this
//! crate drains its stream before returning. Collecting frames would make the resource useless
//! for what it exists for, which is agent loops that print as they go, and a collect-only API
//! cannot be widened later without a breaking change.
//!
//! So a command returns a [`CommandStreamHandle`] whose `next()` yields one frame at a time.
//! Pull-based on purpose: nothing is produced until JavaScript asks, which is exact backpressure
//! with no callback plumbing, and it maps onto an async iterator on the TypeScript side.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::traits::{
    CommandOutput, CreateSessionRequest, RunCommandRequest, Sandbox, SandboxSession,
};
use futures::stream::BoxStream;
use futures::StreamExt;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use futures::lock::Mutex;

use crate::error::map_alien_error;

/// Environment as the Rust side wants it. A `BTreeMap` so the order a caller supplied does not
/// change what the sandbox sees.
fn into_env(env: Option<std::collections::HashMap<String, String>>) -> BTreeMap<String, String> {
    env.map(|env| env.into_iter().collect()).unwrap_or_default()
}

/// A live sandbox session.
#[napi(object)]
pub struct SandboxSessionJs {
    /// Session id, which is what every later call addresses.
    pub session_id: String,
    /// Lifecycle state: `starting`, `running`, `suspended` or `terminated`.
    pub state: String,
    /// Increments when a session is replaced, so a stale handle is detectable.
    pub generation: i64,
}

fn session_to_js(session: SandboxSession) -> SandboxSessionJs {
    SandboxSessionJs {
        session_id: session.session_id,
        state: match session.state {
            alien_bindings::traits::SandboxSessionState::Starting => "starting",
            alien_bindings::traits::SandboxSessionState::Running => "running",
            alien_bindings::traits::SandboxSessionState::Suspended => "suspended",
            alien_bindings::traits::SandboxSessionState::Terminated => "terminated",
        }
        .to_string(),
        generation: session.generation as i64,
    }
}

/// One frame of a running command's output.
///
/// Exactly one of `data` or `exitCode` is set. A frame with neither would be a frame that says
/// nothing, and the terminal frame is the one that carries the exit code.
#[napi(object)]
pub struct CommandFrameJs {
    /// `stdout`, `stderr` or `exit`
    pub kind: String,
    /// Monotonic across both streams, so a caller can reconstruct production order. Absent on
    /// the terminal frame.
    pub seq: Option<i64>,
    /// Raw bytes. Not a string: command output is not necessarily UTF-8.
    pub data: Option<Buffer>,
    /// Process exit code, on the terminal frame only.
    pub exit_code: Option<i32>,
    /// Set when output was cut short by a bound rather than by the command ending.
    pub truncated: Option<bool>,
}

fn frame_to_js(frame: CommandOutput) -> CommandFrameJs {
    match frame {
        CommandOutput::Stdout { seq, data } => CommandFrameJs {
            kind: "stdout".to_string(),
            seq: Some(seq as i64),
            data: Some(Buffer::from(data)),
            exit_code: None,
            truncated: None,
        },
        CommandOutput::Stderr { seq, data } => CommandFrameJs {
            kind: "stderr".to_string(),
            seq: Some(seq as i64),
            data: Some(Buffer::from(data)),
            exit_code: None,
            truncated: None,
        },
        CommandOutput::Exit { code, truncated } => CommandFrameJs {
            kind: "exit".to_string(),
            seq: None,
            data: None,
            exit_code: Some(code),
            truncated: Some(truncated),
        },
    }
}

/// A running command's output, pulled one frame at a time.
///
/// The stream is held in an `Option` so it can be dropped on demand: a caller that stops reading
/// half way through leaves a command running, and dropping the stream closes the transport
/// carrying its output, which is what tells the backend nobody is listening.
#[napi]
pub struct CommandStreamHandle {
    frames: Arc<Mutex<Option<BoxStream<'static, alien_bindings::error::Result<CommandOutput>>>>>,
}

#[napi]
impl CommandStreamHandle {
    /// Returns the next frame, or `null` once the command has produced its last.
    ///
    /// The terminal frame is delivered like any other, so a caller reads until `null` and finds
    /// the exit code in the frame before it.
    #[napi]
    pub async fn next(&self) -> napi::Result<Option<CommandFrameJs>> {
        let mut frames = self.frames.lock().await;
        let Some(stream) = frames.as_mut() else {
            return Ok(None);
        };

        match stream.next().await {
            Some(Ok(frame)) => Ok(Some(frame_to_js(frame))),
            Some(Err(error)) => Err(map_alien_error(error)),
            None => {
                *frames = None;
                Ok(None)
            }
        }
    }

    /// Releases the stream. Idempotent, and `next()` returns `null` afterwards.
    #[napi]
    pub async fn close(&self) {
        *self.frames.lock().await = None;
    }
}

/// Handle to a resolved sandbox binding.
#[napi]
pub struct SandboxHandle {
    inner: Arc<dyn Sandbox>,
}

impl SandboxHandle {
    pub(crate) fn new(inner: Arc<dyn Sandbox>) -> Self {
        Self { inner }
    }
}

#[napi]
impl SandboxHandle {
    /// Which operations this platform's sandbox supports.
    ///
    /// Worth calling: capabilities differ per cloud, and an unsupported one raises rather than
    /// silently doing nothing.
    /// What this binding can actually do on the current platform.
    ///
    /// The point of publishing capabilities is that a caller branches on them instead of
    /// discovering a gap through an error, so a capability with no method to call is worse than
    /// one that is absent. `preview` and `snapshot` are true on some platforms but have no method
    /// here yet, so they are not advertised until they do.
    ///
    /// Destructured rather than read field by field: a capability added to the set then fails to
    /// compile here instead of being silently dropped, which is how `egressDeny` went missing.
    #[napi]
    pub fn capabilities(&self) -> Vec<String> {
        let alien_core::SandboxCapabilities {
            files,
            reconnect,
            preview: _,
            suspend_resume,
            snapshot: _,
            domain_egress_rules,
            egress_deny,
            enforced_limits,
            process_limit,
            session_lifetime,
            supervisor_pid_namespace,
        } = self.inner.capabilities();

        [
            (files, "files"),
            (reconnect, "reconnect"),
            (suspend_resume, "suspendResume"),
            (domain_egress_rules, "domainEgressRules"),
            (egress_deny, "egressDeny"),
            (enforced_limits, "enforcedLimits"),
            (process_limit, "processLimit"),
            (session_lifetime, "sessionLifetime"),
            (supervisor_pid_namespace, "supervisorPidNamespace"),
        ]
        .into_iter()
        .filter(|(supported, _)| *supported)
        .map(|(_, name)| name.to_string())
        .collect()
    }

    /// Creates a session.
    #[napi]
    pub async fn create(
        &self,
        session_id: Option<String>,
        tenant_key: Option<String>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> napi::Result<SandboxSessionJs> {
        let sandbox = self.inner.clone();
        let session = sandbox
            .create(CreateSessionRequest {
                session_id,
                tenant_key,
                env: into_env(env),
            })
            .await
            .map_err(map_alien_error)?;
        Ok(session_to_js(session))
    }

    /// Fetches a session, or `null` if it does not exist.
    #[napi]
    pub async fn get(&self, session_id: String) -> napi::Result<Option<SandboxSessionJs>> {
        let sandbox = self.inner.clone();
        let session = sandbox.get(&session_id).await.map_err(map_alien_error)?;
        Ok(session.map(session_to_js))
    }

    /// Fetches a session, creating it if absent.
    #[napi]
    pub async fn get_or_create(
        &self,
        session_id: Option<String>,
        tenant_key: Option<String>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> napi::Result<SandboxSessionJs> {
        let sandbox = self.inner.clone();
        let session = sandbox
            .get_or_create(CreateSessionRequest {
                session_id,
                tenant_key,
                env: into_env(env),
            })
            .await
            .map_err(map_alien_error)?;
        Ok(session_to_js(session))
    }

    /// Lists this sandbox's sessions.
    #[napi]
    pub async fn list(&self) -> napi::Result<Vec<SandboxSessionJs>> {
        let sandbox = self.inner.clone();
        let sessions = sandbox.list().await.map_err(map_alien_error)?;
        Ok(sessions.into_iter().map(session_to_js).collect())
    }

    /// Runs a command, returning a stream of output frames.
    ///
    /// `deadlineMs` is required rather than defaulted: a defaulted deadline is a hang waiting
    /// for a slow day, in a process the caller shares with untrusted code. It bounds the command
    /// rather than the call: backends with no timeout of their own end the session and return
    /// once that is confirmed, others kill the process group and leave the session usable.
    #[napi]
    pub async fn run_command(
        &self,
        session_id: String,
        command: Vec<String>,
        deadline_ms: u32,
        working_directory: Option<String>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> napi::Result<CommandStreamHandle> {
        let sandbox = self.inner.clone();
        let frames = sandbox
            .run_command(
                &session_id,
                RunCommandRequest {
                    command,
                    working_directory,
                    env: into_env(env),
                    deadline: Duration::from_millis(u64::from(deadline_ms)),
                },
            )
            .await
            .map_err(map_alien_error)?;

        Ok(CommandStreamHandle {
            frames: Arc::new(Mutex::new(Some(frames))),
        })
    }

    /// Reads a file out of the sandbox.
    #[napi]
    pub async fn read_file(&self, session_id: String, path: String) -> napi::Result<Buffer> {
        let sandbox = self.inner.clone();
        let contents = sandbox
            .read_file(&session_id, &path)
            .await
            .map_err(map_alien_error)?;
        Ok(Buffer::from(contents))
    }

    /// Writes one file into the sandbox.
    ///
    /// One at a time rather than a map: napi has no natural shape for a map of buffers, and the
    /// TypeScript wrapper batches on this side of the boundary.
    #[napi]
    pub async fn write_file(
        &self,
        session_id: String,
        path: String,
        contents: Buffer,
    ) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox
            .write_files(
                &session_id,
                BTreeMap::from([(path, contents.to_vec())]),
            )
            .await
            .map_err(map_alien_error)
    }

    /// Creates a directory inside the sandbox.
    #[napi]
    pub async fn mkdir(&self, session_id: String, path: String) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox
            .mkdir(&session_id, &path)
            .await
            .map_err(map_alien_error)
    }

    /// Suspends a session, preserving its state. Requires the `suspendResume` capability.
    #[napi]
    pub async fn suspend(&self, session_id: String) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox.suspend(&session_id).await.map_err(map_alien_error)
    }

    /// Resumes a suspended session. Requires the `suspendResume` capability.
    #[napi]
    pub async fn resume(&self, session_id: String) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox.resume(&session_id).await.map_err(map_alien_error)
    }

    /// Destroys a session. Idempotent: a session already gone is the desired end state.
    #[napi]
    pub async fn terminate(&self, session_id: String) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox
            .terminate(&session_id)
            .await
            .map_err(map_alien_error)
    }
}
