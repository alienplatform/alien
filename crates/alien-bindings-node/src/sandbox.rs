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
    CommandOutput, CreateSandboxRequest, JobPoll, RunCommandRequest, Sandbox, SandboxInstance,
};
use futures::channel::oneshot;
use futures::future::{select, Either, FutureExt, Shared};
use futures::lock::Mutex;
use futures::stream::BoxStream;
use futures::StreamExt;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

use crate::error::map_alien_error;

/// Environment as the Rust side wants it. A `BTreeMap` so the order a caller supplied does not
/// change what the sandbox sees.
fn into_env(env: Option<std::collections::HashMap<String, String>>) -> BTreeMap<String, String> {
    env.map(|env| env.into_iter().collect()).unwrap_or_default()
}

/// A live sandbox.
#[napi(object)]
pub struct SandboxInstanceJs {
    /// Sandbox id, which is what every later call addresses.
    pub sandbox_id: String,
    /// Lifecycle state: `starting`, `running`, `paused` or `terminated`.
    pub state: String,
    /// Increments when a sandbox is replaced, so a stale handle is detectable.
    pub generation: i64,
}

/// A sandbox from `getOrCreate`, and which of the two things happened.
#[napi(object)]
pub struct ResolvedSandboxJs {
    /// The sandbox, whether it was made here or found.
    pub sandbox: SandboxInstanceJs,
    /// Whether this call is what created it, so a caller knows if its first-run setup has run.
    pub created: bool,
}

fn sandbox_to_js(sandbox: SandboxInstance) -> SandboxInstanceJs {
    SandboxInstanceJs {
        sandbox_id: sandbox.sandbox_id,
        state: match sandbox.state {
            alien_bindings::traits::SandboxState::Starting => "starting",
            alien_bindings::traits::SandboxState::Running => "running",
            alien_bindings::traits::SandboxState::Paused => "paused",
            alien_bindings::traits::SandboxState::Terminated => "terminated",
        }
        .to_string(),
        generation: sandbox.generation as i64,
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

/// A job's output so far, and how it ended once it has.
#[napi(object)]
pub struct JobPollJs {
    /// Whether the command is still running
    pub running: bool,
    /// Output produced after the polled sequence. The ending is `exit` or `error`, never a frame.
    pub frames: Vec<CommandFrameJs>,
    /// How the command exited, once it has
    pub exit: Option<JobExitJs>,
    /// Why the command ended without exiting
    pub error: Option<JobErrorJs>,
}

/// How a job's command exited.
#[napi(object)]
pub struct JobExitJs {
    /// Process exit code
    pub code: i32,
    /// Set when output was cut short by a bound rather than by the command finishing
    pub truncated: bool,
}

/// Why a job ended without its command exiting.
#[napi(object)]
pub struct JobErrorJs {
    /// Machine-readable cause, e.g. `timeoutExceeded`
    pub code: String,
    /// Human-readable detail
    pub message: String,
}

fn poll_to_js(poll: JobPoll) -> JobPollJs {
    JobPollJs {
        running: poll.running,
        frames: poll.frames.into_iter().map(frame_to_js).collect(),
        exit: poll.exit.map(|exit| JobExitJs {
            code: exit.code,
            truncated: exit.truncated,
        }),
        error: poll.error.map(|error| JobErrorJs {
            code: error.code,
            message: error.message,
        }),
    }
}

/// A running command's output, pulled one frame at a time.
///
/// The stream is held in an `Option` so it can be dropped on demand: a caller that stops reading
/// half way through leaves a command running, and dropping the stream is what tells the backend to
/// kill it. `close()` is that signal, and it has to land even while a `next()` is parked on a
/// command that prints nothing — that `next()` holds the lock, so `close()` cannot wait for it; it
/// fires `closed` and the parked `next()` drops the stream itself.
#[napi]
pub struct CommandStreamHandle {
    frames: Arc<Mutex<Option<BoxStream<'static, alien_bindings::error::Result<CommandOutput>>>>>,
    closed: Shared<oneshot::Receiver<()>>,
    close: Arc<std::sync::Mutex<Option<oneshot::Sender<()>>>>,
}

impl CommandStreamHandle {
    fn new(frames: BoxStream<'static, alien_bindings::error::Result<CommandOutput>>) -> Self {
        let (close, closed) = oneshot::channel();
        Self {
            frames: Arc::new(Mutex::new(Some(frames))),
            closed: closed.shared(),
            close: Arc::new(std::sync::Mutex::new(Some(close))),
        }
    }
}

#[napi]
impl CommandStreamHandle {
    /// Returns the next frame, or `null` once the command has produced its last.
    ///
    /// The terminal frame is delivered like any other, so a caller reads until `null` and finds
    /// the exit code in the frame before it. A `close()` while this waits ends it with `null`.
    #[napi]
    pub async fn next(&self) -> napi::Result<Option<CommandFrameJs>> {
        let mut frames = self.frames.lock().await;
        let Some(stream) = frames.as_mut() else {
            return Ok(None);
        };

        let next = select(stream.next(), self.closed.clone()).await;

        // Whatever won the select, a close that has already fired means the caller has cancelled:
        // the stream is released, not whatever the stream was about to say. Checked once, here,
        // rather than in the arm that happened to win — a frame and an error race the signal the
        // same way, and delivering either would keep the command alive until its deadline.
        if self.closed.clone().now_or_never().is_some() {
            *frames = None;
            return Ok(None);
        }

        match next {
            Either::Left((Some(Ok(frame)), _)) => Ok(Some(frame_to_js(frame))),
            Either::Left((Some(Err(error)), _)) => Err(map_alien_error(error)),
            Either::Left((None, _)) | Either::Right(_) => {
                *frames = None;
                Ok(None)
            }
        }
    }

    /// Cancels the command by releasing its stream. Idempotent, and `next()` returns `null`
    /// afterwards — including a `next()` already waiting when this is called.
    #[napi]
    pub async fn close(&self) {
        if let Some(close) = self
            .close
            .lock()
            .expect("close sender lock poisoned")
            .take()
        {
            let _ = close.send(());
        }
        // Acquiring after the signal is what makes this terminate: a parked `next()` wakes on
        // `closed` and releases, and one arriving later finds the stream already gone. Returning
        // without the stream released would leave the command running to its deadline.
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
            jobs,
            preview: _,
            pause_resume,
            snapshot: _,
            domain_egress_rules,
            egress_deny,
            enforced_limits,
            process_limit,
            sandbox_lifetime,
            supervisor_pid_namespace,
            supervisor_isolation,
        } = self.inner.capabilities();

        [
            (files, "files"),
            (reconnect, "reconnect"),
            (jobs, "jobs"),
            (pause_resume, "pauseResume"),
            (domain_egress_rules, "domainEgressRules"),
            (egress_deny, "egressDeny"),
            (enforced_limits, "enforcedLimits"),
            (process_limit, "processLimit"),
            (sandbox_lifetime, "sandboxLifetime"),
            (supervisor_pid_namespace, "supervisorPidNamespace"),
            (supervisor_isolation, "supervisorIsolation"),
        ]
        .into_iter()
        .filter(|(supported, _)| *supported)
        .map(|(_, name)| name.to_string())
        .collect()
    }

    /// Creates a sandbox.
    ///
    /// `timeoutMs` is the wall-clock lifetime the platform terminates the sandbox at. Only a
    /// backend reporting `sandboxLifetime` takes one; the rest raise rather than run on past it.
    #[napi]
    pub async fn create(
        &self,
        sandbox_id: Option<String>,
        tenant_key: Option<String>,
        env: Option<std::collections::HashMap<String, String>>,
        timeout_ms: Option<u32>,
    ) -> napi::Result<SandboxInstanceJs> {
        let sandbox = self.inner.clone();
        let created = sandbox
            .create(CreateSandboxRequest {
                sandbox_id,
                tenant_key,
                env: into_env(env),
                timeout_ms: timeout_ms.map(u64::from),
            })
            .await
            .map_err(map_alien_error)?;
        Ok(sandbox_to_js(created))
    }

    /// Fetches a sandbox, or `null` if it does not exist.
    #[napi]
    pub async fn get(&self, sandbox_id: String) -> napi::Result<Option<SandboxInstanceJs>> {
        let sandbox = self.inner.clone();
        let found = sandbox.get(&sandbox_id).await.map_err(map_alien_error)?;
        Ok(found.map(sandbox_to_js))
    }

    /// Fetches a sandbox, creating it if absent, and reports which it did.
    ///
    /// `timeoutMs` bounds a sandbox this call creates; one that is found keeps the lifetime it
    /// was created with, and `created` is how a caller tells the two apart.
    #[napi]
    pub async fn get_or_create(
        &self,
        sandbox_id: Option<String>,
        tenant_key: Option<String>,
        env: Option<std::collections::HashMap<String, String>>,
        timeout_ms: Option<u32>,
    ) -> napi::Result<ResolvedSandboxJs> {
        let sandbox = self.inner.clone();
        let resolved = sandbox
            .get_or_create(CreateSandboxRequest {
                sandbox_id,
                tenant_key,
                env: into_env(env),
                timeout_ms: timeout_ms.map(u64::from),
            })
            .await
            .map_err(map_alien_error)?;
        Ok(ResolvedSandboxJs {
            sandbox: sandbox_to_js(resolved.sandbox),
            created: resolved.created,
        })
    }

    /// Lists this binding's sandboxes.
    #[napi]
    pub async fn list(&self) -> napi::Result<Vec<SandboxInstanceJs>> {
        let sandbox = self.inner.clone();
        let instances = sandbox.list().await.map_err(map_alien_error)?;
        Ok(instances.into_iter().map(sandbox_to_js).collect())
    }

    /// Runs a command, returning a stream of output frames.
    ///
    /// `timeoutMs` is required rather than defaulted: a defaulted timeout is a hang waiting
    /// for a slow day, in a process the caller shares with untrusted code. It bounds this command
    /// rather than the call, and is unrelated to `create`'s own `timeoutMs`, which is the
    /// sandbox's wall-clock lifetime: backends with no timeout of their own end the sandbox and
    /// return once that is confirmed, others kill the process group and leave the sandbox usable.
    #[napi]
    pub async fn run_command(
        &self,
        sandbox_id: String,
        command: String,
        args: Vec<String>,
        timeout_ms: u32,
        cwd: Option<String>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> napi::Result<CommandStreamHandle> {
        let sandbox = self.inner.clone();
        let frames = sandbox
            .run_command(
                &sandbox_id,
                RunCommandRequest {
                    command,
                    args,
                    cwd,
                    env: into_env(env),
                    timeout: Duration::from_millis(u64::from(timeout_ms)),
                },
            )
            .await
            .map_err(map_alien_error)?;

        Ok(CommandStreamHandle::new(frames))
    }

    /// Starts a command as a job, returning the id later polls and cancels address.
    ///
    /// Use this rather than `runCommand` for a command that outlives one call: nothing has to
    /// hold the stream open, and a caller that goes away can reach the job again by its id.
    #[napi]
    pub async fn start_job(
        &self,
        sandbox_id: String,
        command: String,
        args: Vec<String>,
        timeout_ms: u32,
        cwd: Option<String>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> napi::Result<String> {
        let sandbox = self.inner.clone();
        let started = sandbox
            .start_job(
                &sandbox_id,
                RunCommandRequest {
                    command,
                    args,
                    cwd,
                    env: into_env(env),
                    timeout: Duration::from_millis(u64::from(timeout_ms)),
                },
            )
            .await
            .map_err(map_alien_error)?;

        Ok(started.job_id)
    }

    /// Reads a job's output after `sinceSeq`, and its ending once it has one.
    ///
    /// Omit `sinceSeq` to read from the first frame; afterwards pass the highest `seq` seen, which
    /// is what makes a repeated poll return only what is new.
    #[napi]
    pub async fn poll_job(
        &self,
        sandbox_id: String,
        job_id: String,
        since_seq: Option<i64>,
    ) -> napi::Result<JobPollJs> {
        let since_seq = match since_seq {
            Some(seq) => Some(u64::try_from(seq).map_err(|_| {
                map_alien_error(alien_error::AlienError::new(
                    alien_bindings::error::ErrorData::InvalidInput {
                        operation_context: "sandbox.pollJob".to_string(),
                        details: "sinceSeq is a frame sequence and cannot be negative".to_string(),
                        field_name: Some("sinceSeq".to_string()),
                    },
                ))
            })?),
            None => None,
        };

        let sandbox = self.inner.clone();
        let poll = sandbox
            .poll_job(&sandbox_id, &job_id, since_seq)
            .await
            .map_err(map_alien_error)?;

        Ok(poll_to_js(poll))
    }

    /// Cancels a job, stopping its command.
    #[napi]
    pub async fn cancel_job(&self, sandbox_id: String, job_id: String) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox
            .cancel_job(&sandbox_id, &job_id)
            .await
            .map_err(map_alien_error)
    }

    /// Reads a file out of the sandbox.
    #[napi]
    pub async fn read_file(&self, sandbox_id: String, path: String) -> napi::Result<Buffer> {
        let sandbox = self.inner.clone();
        let contents = sandbox
            .read_file(&sandbox_id, &path)
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
        sandbox_id: String,
        path: String,
        contents: Buffer,
    ) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox
            .write_files(&sandbox_id, BTreeMap::from([(path, contents.to_vec())]))
            .await
            .map_err(map_alien_error)
    }

    /// Pauses a sandbox, preserving its state. Requires the `pauseResume` capability.
    #[napi]
    pub async fn pause(&self, sandbox_id: String) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox.pause(&sandbox_id).await.map_err(map_alien_error)
    }

    /// Resumes a paused sandbox. Requires the `pauseResume` capability.
    #[napi]
    pub async fn resume(&self, sandbox_id: String) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox.resume(&sandbox_id).await.map_err(map_alien_error)
    }

    /// Destroys a sandbox. Idempotent: a sandbox already gone is the desired end state.
    #[napi]
    pub async fn terminate(&self, sandbox_id: String) -> napi::Result<()> {
        let sandbox = self.inner.clone();
        sandbox
            .terminate(&sandbox_id)
            .await
            .map_err(map_alien_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_bindings::traits::SandboxState;

    /// The wire word for every lifecycle state, pinned against the union in
    /// `packages/bindings/src/types.ts`. The mapping is a hand-written literal, so a variant
    /// whose name changes in `alien-bindings` still compiles here while emitting a word no
    /// TypeScript caller can narrow — a failure neither `cargo` nor `tsc` would report.
    #[test]
    fn every_state_reaches_javascript_as_the_word_typescript_declares() {
        for (state, word) in [
            (SandboxState::Starting, "starting"),
            (SandboxState::Running, "running"),
            (SandboxState::Paused, "paused"),
            (SandboxState::Terminated, "terminated"),
        ] {
            let js = sandbox_to_js(SandboxInstance {
                sandbox_id: "s1".to_string(),
                state,
                generation: 1,
            });
            assert_eq!(js.state, word, "{state:?}");
        }
    }

    /// A command that never prints parks `next()` on the lock; `close()` must still end it,
    /// or the caller that gave up cannot cancel a silent command.
    #[test]
    fn close_ends_a_next_parked_on_a_silent_command() {
        let handle = CommandStreamHandle::new(futures::stream::pending().boxed());
        let (frame, ()) =
            futures::executor::block_on(futures::future::join(handle.next(), async {
                handle.close().await;
            }));
        assert!(
            frame.expect("close is not an error").is_none(),
            "a closed stream reports its end, not a frame"
        );
        assert!(
            futures::executor::block_on(handle.frames.lock()).is_none(),
            "the stream must be dropped, that is what reaches the backend"
        );
    }

    /// A frame that becomes ready in the same instant as the close signal wins the select. The
    /// caller has already cancelled, so the frame is not the answer — the stream is, and it must
    /// be dropped, or the command it feeds runs on until its deadline with nobody left to stop it.
    ///
    /// The race is only reachable while `next()` holds the frames lock, so `close()` cannot drop
    /// the stream itself; that is arranged by parking `next()` on a stream that yields its frame
    /// only after the close has been sent.
    #[test]
    fn close_racing_a_ready_frame_still_releases_the_stream() {
        let (release, released) = oneshot::channel::<()>();
        // The frame arrives only once `released` fires — after close() has been called while
        // next() is parked inside the select holding the lock.
        let gated = futures::stream::once(async move {
            let _ = released.await;
            Ok(CommandOutput::Stdout {
                seq: 0,
                data: b"late".to_vec(),
            })
        })
        .chain(futures::stream::pending());
        let handle = Arc::new(CommandStreamHandle::new(gated.boxed()));

        futures::executor::block_on(async {
            let reader = {
                let handle = Arc::clone(&handle);
                async move { handle.next().await }
            };
            let closer = {
                let handle = Arc::clone(&handle);
                async move {
                    // Let next() take the lock and park; then close, then release the frame so
                    // both the frame and the close signal are ready in the same poll.
                    futures::future::ready(()).await;
                    handle.close().await;
                    let _ = release.send(());
                }
            };
            let (frame, ()) = futures::future::join(reader, closer).await;
            assert!(
                frame.expect("close is not an error").is_none(),
                "a cancelled command's frame is not delivered: the stream is released instead"
            );
            assert!(
                handle.frames.lock().await.is_none(),
                "the stream must be dropped, that is what reaches the backend"
            );
        });
    }

    /// The same race with an error instead of a frame: a stream failure that lands in the same
    /// instant as the close must not be delivered either — the caller has cancelled, and what it
    /// needs is the stream released, not the stream's last word.
    #[test]
    fn close_racing_a_ready_error_still_releases_the_stream() {
        let (release, released) = oneshot::channel::<()>();
        let gated = futures::stream::once(async move {
            let _ = released.await;
            Err(alien_error::AlienError::new(
                alien_bindings::error::ErrorData::SandboxUnreachable {
                    operation: "sandbox.runCommand".to_string(),
                    reason: "late".to_string(),
                },
            ))
        })
        .chain(futures::stream::pending());
        let handle = Arc::new(CommandStreamHandle::new(gated.boxed()));

        futures::executor::block_on(async {
            let reader = {
                let handle = Arc::clone(&handle);
                async move { handle.next().await }
            };
            let closer = {
                let handle = Arc::clone(&handle);
                async move {
                    futures::future::ready(()).await;
                    handle.close().await;
                    let _ = release.send(());
                }
            };
            let (result, ()) = futures::future::join(reader, closer).await;
            assert!(
                result
                    .expect("a cancelled read is the end, not an error")
                    .is_none(),
                "a cancelled command's stream error is not delivered: the stream is released"
            );
            assert!(
                handle.frames.lock().await.is_none(),
                "the stream must be dropped, that is what reaches the backend"
            );
        });
    }

    /// A reader that already passed its own close check holds the lock with a frame in hand and
    /// will not release the stream, so `close()` returning at that moment would report a command
    /// cancelled that still runs to its deadline. Returning has to mean the stream is gone.
    #[test]
    fn close_does_not_return_while_a_reader_holds_the_stream() {
        let handle = CommandStreamHandle::new(futures::stream::pending().boxed());
        futures::executor::block_on(async {
            let held = handle.frames.lock().await;
            assert!(
                handle.close().now_or_never().is_none(),
                "close reported the command cancelled while its stream was still held"
            );
            drop(held);

            handle.close().await;
            assert!(
                handle.frames.lock().await.is_none(),
                "close returned without releasing the stream"
            );
        });
    }

    /// Closing before or after the end is a no-op; a caller need not track which came first.
    #[test]
    fn close_is_idempotent_and_next_stays_null() {
        let handle = CommandStreamHandle::new(futures::stream::empty().boxed());
        futures::executor::block_on(async {
            handle.close().await;
            handle.close().await;
            assert!(handle.next().await.expect("closed").is_none());
            assert!(handle.next().await.expect("closed").is_none());
        });
    }
}
