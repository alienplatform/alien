//! Running a command inside the sandbox, framed as the protocol specifies.
//!
//! Two rules carry most of the weight, and both exist because the code being run is hostile:
//! a command **always** ends — by exit, by deadline, or by cancellation — and output is framed
//! with one sequence across both streams so a caller can reconstruct production order.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use std::collections::BTreeMap;
use std::time::Duration;

use alien_core::sandbox_process;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::{ErrorData, Result};
use alien_error::AlienError;

/// The uid and gid a command runs as.
///
/// Not optional, and not defaulted to the agent's own: a command running as the agent can read
/// and write the agent's binary and state, which is the escalation the split exists to prevent.
/// This type is what makes that boundary unavoidable at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecIdentity {
    pub uid: u32,
    pub gid: u32,
}

pub use alien_core::sandbox_process::FRAME_CHANNEL_DEPTH;

/// A command to run.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecRequest {
    /// Command and arguments. Never a shell string — a shell would re-parse hostile input.
    pub command: Vec<String>,
    /// Wall-clock ceiling in milliseconds. Required.
    pub deadline_ms: u64,
    /// Working directory, resolved against the session root before use.
    #[serde(default)]
    pub working_directory: Option<String>,
    /// Environment for the command. The agent's own environment is never inherited, so this is
    /// everything the command gets beyond `PATH`.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// One frame of output, serialized as a line of NDJSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "t")]
pub enum Frame {
    /// Bytes from stdout
    Stdout {
        /// Monotonic across both streams
        seq: u64,
        /// Base64, because output is arbitrary bytes rather than UTF-8
        data: String,
    },
    /// Bytes from stderr
    Stderr {
        /// Monotonic across both streams
        seq: u64,
        /// Base64
        data: String,
    },
    /// The command finished. Exactly one terminal frame, always last.
    Exit {
        /// Process exit code
        code: i32,
        /// Set when output was cut short by a bound rather than by the command ending
        truncated: bool,
    },
    /// The command did not finish. Also terminal.
    Error {
        /// Machine-readable cause, e.g. `deadlineExceeded`
        code: String,
        /// Human-readable detail
        message: String,
    },
}

impl Frame {
    fn stdout(seq: u64, data: String) -> Self {
        Self::Stdout { seq, data }
    }

    fn stderr(seq: u64, data: String) -> Self {
        Self::Stderr { seq, data }
    }
}

impl ExecRequest {
    /// Rejects a request the agent must not act on.
    ///
    /// A zero deadline is refused rather than defaulted: a defaulted deadline is a hang waiting
    /// for a slow day, and this process shares a machine with the workload that asked for it.
    pub fn validate(&self) -> Result<()> {
        if self.command.is_empty() {
            return Err(invalid("command is empty"));
        }

        if self.deadline_ms == 0 {
            return Err(invalid("a command must carry a non-zero deadline"));
        }

        // JSON can carry a NUL, and std panics rather than erroring when building an
        // environment from a string with an interior NUL. `=` in a key is the same shape of
        // problem: it would split into a variable the caller did not name.
        for (key, value) in &self.env {
            if key.contains('\0') || value.contains('\0') {
                return Err(invalid("an environment entry contains a NUL byte"));
            }
            if key.is_empty() || key.contains('=') {
                return Err(invalid(
                    "an environment name must be non-empty and contain no '='",
                ));
            }
        }

        Ok(())
    }
}

/// Runs a command, sending its frames as they are produced.
///
/// `output_cap` bounds how many bytes of each stream are kept. Beyond it the command still runs
/// to completion — killing it on a chatty log would be a surprising failure — but the extra
/// output is dropped and the terminal frame is marked `truncated`.
///
/// **Backpressure is the send.** `frames` is bounded, so a command that writes faster than the
/// caller reads blocks this task rather than growing a buffer. A dropped receiver means the
/// caller went away, and the command is killed rather than left running for nobody.
pub async fn stream(
    request: &ExecRequest,
    working_directory: Option<&std::path::Path>,
    identity: ExecIdentity,
    output_cap: usize,
    frames: mpsc::Sender<Frame>,
) {
    if let Err(error) = request.validate() {
        let _ = frames
            .send(Frame::Error {
                code: "requestInvalid".to_string(),
                message: error.to_string(),
            })
            .await;
        return;
    }

    let Ok(mut command) = sandbox_process::spawn_sandboxed(
        &request.command[0],
        &request.command[1..].to_vec(),
        &request.env,
    ) else {
        let _ = frames
            .send(Frame::Error {
                code: "spawnFailed".to_string(),
                message: "command could not be prepared".to_string(),
            })
            .await;
        return;
    };

    // Where a PID namespace is available the drop moves inside it, because `std` applies
    // `Command::uid` before `pre_exec` runs and an unprivileged process cannot unshare.
    if crate::pid_namespace::available() {
        crate::pid_namespace::apply(&mut command, identity);
    } else {
        // Not `Command::uid`/`gid`: `std` applies those before `pre_exec`, which is too late to
        // drop supplementary groups and too late to refuse a drop that did not take. This is the
        // path every backend actually uses — no runtime grants the capability the other needs.
        #[cfg(unix)]
        unsafe {
            command.pre_exec(move || crate::privilege::drop_to(identity));
        }

        #[cfg(not(unix))]
        compile_error!("the sandbox agent runs untrusted code and requires a unix privilege drop");
    }

    // Not `Command::current_dir`: `std` applies it before `pre_exec`, so the chdir would happen
    // while still privileged and could enter a directory the command itself cannot. Every relative
    // path would then fail with nothing to report. Registered after the privilege closure above
    // because `std` runs them in order, which puts it under the identity the command runs as.
    if let Some(directory) = working_directory {
        use std::os::unix::ffi::OsStrExt;

        // Allocated here rather than in the closure: between `fork` and `exec` nothing may
        // allocate, and the whole path has to be a plain pointer by then.
        let Ok(directory) = std::ffi::CString::new(directory.as_os_str().as_bytes()) else {
            let _ = frames
                .send(Frame::Error {
                    code: "requestInvalid".to_string(),
                    message: "the working directory contains a NUL byte".to_string(),
                })
                .await;
            return;
        };

        unsafe {
            command.pre_exec(move || {
                // SAFETY: a NUL-terminated path owned by the closure; `chdir` is
                // async-signal-safe.
                if libc::chdir(directory.as_ptr()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = frames
                .send(Frame::Error {
                    code: "spawnFailed".to_string(),
                    message: error.to_string(),
                })
                .await;
            return;
        }
    };

    // The framing itself is shared with the GCP binding, which runs commands through a launcher
    // CLI rather than in-guest. Only the wire type differs, and it differs here.
    let (raw, mut incoming) = mpsc::channel(FRAME_CHANNEL_DEPTH);
    let produce = sandbox_process::stream(
        child,
        Duration::from_millis(request.deadline_ms),
        output_cap,
        raw,
    );

    // `incoming` is moved in so it is dropped the moment forwarding stops. Holding it would
    // leave the producer writing into a channel nobody reads, which is exactly the runaway a
    // departed caller is supposed to end.
    // The caller leaving is watched directly, not only noticed on the next failed send: a
    // command that prints nothing never produces a frame to fail on, so without `closed()` it
    // would run to its deadline for a receiver that is already gone.
    let forward = async move {
        loop {
            tokio::select! {
                frame = incoming.recv() => match frame {
                    Some(frame) => {
                        if frames.send(Frame::from(frame)).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
                () = frames.closed() => break,
            }
        }
    };

    tokio::join!(produce, forward);
}

impl From<sandbox_process::ProcessFrame> for Frame {
    fn from(frame: sandbox_process::ProcessFrame) -> Self {
        use sandbox_process::{ProcessFrame, ProcessStream};

        match frame {
            ProcessFrame::Output {
                seq,
                stream: ProcessStream::Stdout,
                data,
            } => Frame::stdout(seq, encode(&data)),
            ProcessFrame::Output {
                seq,
                stream: ProcessStream::Stderr,
                data,
            } => Frame::stderr(seq, encode(&data)),
            ProcessFrame::Exit { code, truncated } => Frame::Exit { code, truncated },
            ProcessFrame::Failed { code, message } => Frame::Error {
                code: code.to_string(),
                message,
            },
        }
    }
}

/// Runs a command and collects every frame it produced.
///
/// The bounded channel is what makes this safe to use on unbounded output: [`stream`] blocks on
/// a full channel rather than buffering, and the cap still applies to what is kept.
pub async fn run(
    request: &ExecRequest,
    working_directory: Option<&std::path::Path>,
    identity: ExecIdentity,
    output_cap: usize,
) -> Vec<Frame> {
    let (sender, mut receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);

    let produce = stream(request, working_directory, identity, output_cap, sender);
    let consume = async {
        let mut frames = Vec::new();
        while let Some(frame) = receiver.recv().await {
            frames.push(frame);
        }
        frames
    };

    let (_, frames) = tokio::join!(produce, consume);
    frames
}

fn encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

fn invalid(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::RequestInvalid {
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(command: &[&str], deadline_ms: u64) -> ExecRequest {
        ExecRequest {
            command: command.iter().map(|s| s.to_string()).collect(),
            deadline_ms,
            working_directory: None,
            env: BTreeMap::new(),
        }
    }

    /// The uid the test process already has. Setting a uid to its own is permitted unprivileged,
    /// so this exercises the real drop path without needing root.
    fn same_identity() -> ExecIdentity {
        #[cfg(unix)]
        unsafe {
            ExecIdentity {
                uid: libc::getuid(),
                gid: libc::getgid(),
            }
        }
        #[cfg(not(unix))]
        ExecIdentity { uid: 0, gid: 0 }
    }

    fn terminal(frames: &[Frame]) -> &Frame {
        frames.last().expect("there is always a terminal frame")
    }

    #[tokio::test]
    async fn a_command_produces_output_then_exactly_one_terminal_frame() {
        let frames = run(
            &request(&["/bin/echo", "hello"], 10_000),
            None,
            same_identity(),
            1 << 20,
        )
        .await;

        assert!(matches!(terminal(&frames), Frame::Exit { code: 0, .. }));
        let terminals = frames
            .iter()
            .filter(|f| matches!(f, Frame::Exit { .. } | Frame::Error { .. }))
            .count();
        assert_eq!(terminals, 1, "exactly one terminal frame: {frames:?}");
    }

    #[tokio::test]
    async fn a_nonzero_exit_is_reported_as_its_real_code() {
        let frames = run(
            &request(&["/bin/sh", "-c", "exit 3"], 10_000),
            None,
            same_identity(),
            1 << 20,
        )
        .await;
        assert!(matches!(terminal(&frames), Frame::Exit { code: 3, .. }));
    }

    /// The deadline is enforced, not advisory — this is the rule that stops hostile code
    /// occupying a session forever.
    #[tokio::test]
    async fn a_command_that_overruns_is_killed_and_reported() {
        let frames = run(
            &request(&["/bin/sleep", "30"], 300),
            None,
            same_identity(),
            1 << 20,
        )
        .await;

        match terminal(&frames) {
            Frame::Error { code, .. } => assert_eq!(code, "deadlineExceeded"),
            other => panic!("expected a deadline error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_request_without_a_deadline_is_refused_before_spawning() {
        let frames = run(
            &request(&["/bin/echo", "hi"], 0),
            None,
            same_identity(),
            1 << 20,
        )
        .await;

        match terminal(&frames) {
            Frame::Error { code, .. } => assert_eq!(code, "requestInvalid"),
            other => panic!("a zero deadline must be refused, got {other:?}"),
        }
        assert_eq!(frames.len(), 1, "nothing should have been spawned");
    }

    #[tokio::test]
    async fn an_empty_command_is_refused() {
        let frames = run(&request(&[], 10_000), None, same_identity(), 1 << 20).await;
        assert!(matches!(terminal(&frames), Frame::Error { .. }));
    }

    /// Output is base64 because a command's output is arbitrary bytes. Treating it as UTF-8
    /// would corrupt binary output and hand a parse failure to hostile input.
    #[tokio::test]
    async fn output_is_base64_so_arbitrary_bytes_survive() {
        let frames = run(
            &request(&["/bin/echo", "hello"], 10_000),
            None,
            same_identity(),
            1 << 20,
        )
        .await;

        let Frame::Stdout { data, .. } = &frames[0] else {
            panic!("expected stdout first, got {:?}", frames[0]);
        };

        let decoded = STANDARD.decode(data).expect("valid base64");
        assert_eq!(String::from_utf8_lossy(&decoded).trim(), "hello");
    }

    /// A chatty command is truncated rather than killed — killing on volume would be a
    /// surprising failure — but the caller is told, so it never mistakes a cut for the end.
    #[tokio::test]
    async fn excess_output_is_truncated_and_flagged_rather_than_silently_cut() {
        let frames = run(
            &request(
                &[
                    "/bin/sh",
                    "-c",
                    "for i in $(seq 1 200); do echo aaaaaaaaaaaaaaaa; done",
                ],
                20_000,
            ),
            None,
            same_identity(),
            64,
        )
        .await;

        match terminal(&frames) {
            Frame::Exit { truncated, .. } => assert!(truncated, "truncation must be reported"),
            other => panic!("expected an exit frame, got {other:?}"),
        }
    }

    /// A command writing continuously must block on the channel rather than grow a buffer.
    #[tokio::test]
    async fn a_continuously_writing_command_blocks_instead_of_buffering() {
        let (sender, receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);
        let request = request(
            &["/bin/sh", "-c", "while true; do echo aaaaaaaa; done"],
            30_000,
        );

        let producer = tokio::spawn(async move {
            stream(&request, None, same_identity(), 1 << 30, sender).await;
        });

        tokio::time::sleep(Duration::from_millis(250)).await;

        assert!(
            !producer.is_finished(),
            "an endless command must still be running, not drained into memory"
        );
        assert_eq!(
            receiver.capacity(),
            0,
            "the channel must be full: that is what makes the producer wait"
        );

        // Dropping the receiver is the caller going away, which must end the command.
        drop(receiver);
        tokio::time::timeout(Duration::from_secs(10), producer)
            .await
            .expect("a command whose caller left must be killed, not left running")
            .expect("the producer task must not panic");
    }

    /// A command that prints nothing gives forwarding no send to fail on, so the caller leaving
    /// has to be noticed on its own; otherwise a departed caller's silent command runs to its
    /// deadline.
    #[tokio::test]
    async fn a_silent_command_whose_caller_left_is_killed() {
        let (sender, receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);
        let request = request(&["/bin/sh", "-c", "sleep 30"], 60_000);

        let producer = tokio::spawn(async move {
            stream(&request, None, same_identity(), 1 << 30, sender).await;
        });

        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(
            !producer.is_finished(),
            "a sleeping command must still be running"
        );

        drop(receiver);
        tokio::time::timeout(Duration::from_secs(10), producer)
            .await
            .expect("a silent command whose caller left must be killed, not run to its deadline")
            .expect("the producer task must not panic");
    }

    /// One sequence across both streams, so a caller can reconstruct production order. Two
    /// independent counters could not express it.
    #[tokio::test]
    async fn the_sequence_is_monotonic_across_both_streams() {
        let frames = run(
            &request(&["/bin/sh", "-c", "echo out; echo err 1>&2"], 10_000),
            None,
            same_identity(),
            1 << 20,
        )
        .await;

        let sequences: Vec<u64> = frames
            .iter()
            .filter_map(|f| match f {
                Frame::Stdout { seq, .. } | Frame::Stderr { seq, .. } => Some(*seq),
                _ => None,
            })
            .collect();

        let mut sorted = sequences.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), sequences.len(), "sequences must not repeat");
    }

    /// The drop is applied, not merely configured. `id -u` reports what the process actually
    /// runs as, which is the only thing that answers the question.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_runs_as_the_configured_uid() {
        let identity = same_identity();
        let frames = run(
            &request(&["/usr/bin/id", "-u"], 10_000),
            None,
            identity,
            1 << 20,
        )
        .await;

        assert_eq!(
            reported_uid(&frames),
            identity.uid,
            "the command must run as the configured uid, not the agent's"
        );
    }

    /// Reads the uid a `/usr/bin/id -u` run reported on its first stdout frame.
    #[cfg(unix)]
    fn reported_uid(frames: &[Frame]) -> u32 {
        let Frame::Stdout { data, .. } = &frames[0] else {
            panic!("expected stdout, got {:?}", frames[0]);
        };
        String::from_utf8(STANDARD.decode(data).expect("base64"))
            .expect("utf8")
            .trim()
            .parse()
            .expect("a uid")
    }

    /// The security-critical half. If the uid cannot be dropped, the command must **not** run —
    /// falling back to the agent's uid would silently hand untrusted code the agent's privileges,
    /// which is exactly the escalation the boundary exists to prevent.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_is_refused_when_the_uid_cannot_be_dropped() {
        let nobody = ExecIdentity {
            uid: 65534,
            gid: 65534,
        };
        let frames = run(
            &request(&["/usr/bin/id", "-u"], 10_000),
            None,
            nobody,
            1 << 20,
        )
        .await;

        // Root can drop, so the refusal cannot be provoked there. Assert the other half of the
        // same invariant instead — the command never runs as the agent — so this asserts under
        // either runner rather than passing vacuously in a root CI container.
        if unsafe { libc::getuid() } == 0 {
            assert_eq!(
                reported_uid(&frames),
                nobody.uid,
                "a successful drop must land on the requested uid, not the agent's"
            );
            return;
        }

        match terminal(&frames) {
            Frame::Error { code, .. } => assert_eq!(code, "spawnFailed"),
            other => panic!("a failed uid drop must refuse the command, got {other:?}"),
        }
        assert_eq!(frames.len(), 1, "nothing may have run: {frames:?}");
    }
}
