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
    /// Wall-clock ceiling on this command, in milliseconds. Required.
    pub timeout_ms: u64,
    /// Working directory, resolved against the sandbox root before use.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Environment for the command. It wins over the `PATH`, `HOME` and `USER` the agent sets, but
    /// not over the uid/gid/session-root variables or the proxy and CA variables the agent sets.
    /// The agent's own environment is not passed on, except under native-file authorization
    /// (Azure), where its variables, minus `ALIEN_SANDBOX_*` and credential endpoints, fill in the
    /// names this leaves out, `HOME` and `USER` included.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Bytes of encoded reply each output stream may take, if the caller's transport limits one
    /// reply (the GCP `:execute` proxy). Each kept frame is charged what it will cost on the
    /// wire: its JSON, base64-encoded once more by the transport, so short lines cost their
    /// framing and not only their data. Output past it is dropped and the result is marked
    /// `truncated`, as with the agent's own cap. Older agents ignore the field.
    #[serde(default)]
    pub reply_budget: Option<usize>,
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
        /// Machine-readable cause, e.g. `timeoutExceeded`
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
    /// A zero timeout is refused rather than defaulted: a defaulted timeout is a hang waiting
    /// for a slow day, and this process shares a machine with the workload that asked for it.
    pub fn validate(&self) -> Result<()> {
        if self.command.is_empty() {
            return Err(invalid("command is empty"));
        }

        if self.timeout_ms == 0 {
            return Err(invalid("a command must carry a non-zero timeout"));
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
    let (child, _registration) = match spawn(request, working_directory, identity, false) {
        Ok(spawned) => spawned,
        Err(frame) => {
            let _ = frames.send(frame).await;
            return;
        }
    };

    pump(
        child,
        Duration::from_millis(request.timeout_ms),
        output_cap,
        sandbox_process::Framing::Lines,
        request.reply_budget,
        frames,
        None::<fn()>,
    )
    .await;
}

/// Streams a spawned child's frames to `frames` until the terminal one, killing the child if the
/// receiver goes away. `exited`, when given, runs once the child has exited and before it is
/// reaped, ahead of the terminal frame.
pub(crate) async fn pump(
    child: tokio::process::Child,
    timeout: Duration,
    output_cap: usize,
    framing: sandbox_process::Framing,
    reply_budget: Option<usize>,
    frames: mpsc::Sender<Frame>,
    exited: Option<impl FnOnce() + Send>,
) {
    // The framing itself is shared with the GCP binding, which runs commands through a launcher
    // CLI rather than in-guest. Only the wire type differs, and it differs here.
    let (raw, mut incoming) = mpsc::channel(FRAME_CHANNEL_DEPTH);
    let produce = sandbox_process::stream_framed(child, timeout, output_cap, framing, raw, exited);

    // `incoming` is moved in so it is dropped the moment forwarding stops. Holding it would
    // leave the producer writing into a channel nobody reads, which is exactly the runaway a
    // departed caller is supposed to end.
    // The caller leaving is watched directly, not only noticed on the next failed send: a
    // command that prints nothing never produces a frame to fail on, so without `closed()` it
    // would run to its deadline for a receiver that is already gone.
    let forward = async move {
        let mut budget = ReplyBudget::new(reply_budget);
        loop {
            tokio::select! {
                frame = incoming.recv() => match frame {
                    Some(frame) => {
                        let Some(frame) = budget.admit(Frame::from(frame)) else {
                            continue;
                        };
                        if frames.send(frame).await.is_err() {
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

/// What a frame costs in a reply: its JSON and the newline or comma that separates it, base64-
/// encoded as the transport will encode it.
pub fn frame_wire_cost(frame: &Frame) -> usize {
    let json = serde_json::to_vec(frame).map_or(usize::MAX / 4, |bytes| bytes.len() + 1);
    json.div_ceil(3).saturating_mul(4)
}

/// What one stream of a command's output may still add to a reply.
///
/// A frame is charged its JSON plus the newline or comma that separates it, base64-encoded as the
/// transport will encode it. Both streams get the same allowance, so one cannot crowd out the
/// other, and the terminal frame is never charged: it is small and must always arrive.
struct ReplyBudget {
    remaining: Option<[usize; 2]>,
    /// Per stream: once a frame is dropped, every later one is, so what is kept is a clean prefix
    /// and not a prefix with a smaller frame's data after a gap.
    dropped: [bool; 2],
}

impl ReplyBudget {
    fn new(per_stream: Option<usize>) -> Self {
        Self {
            remaining: per_stream.map(|bytes| [bytes, bytes]),
            dropped: [false; 2],
        }
    }

    /// The frame to forward, or `None` when it does not fit. An exit frame carries whether
    /// anything was dropped.
    fn admit(&mut self, frame: Frame) -> Option<Frame> {
        let Some(remaining) = self.remaining.as_mut() else {
            return Some(frame);
        };
        let stream = match &frame {
            Frame::Stdout { .. } => 0,
            Frame::Stderr { .. } => 1,
            Frame::Exit { code, truncated } => {
                return Some(Frame::Exit {
                    code: *code,
                    truncated: *truncated || self.dropped.iter().any(|d| *d),
                });
            }
            Frame::Error { .. } => return Some(frame),
        };
        let cost = frame_wire_cost(&frame);
        if self.dropped[stream] || cost > remaining[stream] {
            self.dropped[stream] = true;
            return None;
        }
        remaining[stream] -= cost;
        Some(frame)
    }
}

/// Validates a request and spawns its command under `identity`, or returns the frame that says
/// why it did not run.
///
/// `piped_stdin` gives the command a pipe for stdin instead of null. Only an interactive command
/// asks for one: anything else would hold its deadline open on a read nobody answers.
pub(crate) fn spawn(
    request: &ExecRequest,
    working_directory: Option<&std::path::Path>,
    identity: ExecIdentity,
    piped_stdin: bool,
) -> std::result::Result<(tokio::process::Child, crate::children::Registration), Frame> {
    request.validate().map_err(|error| Frame::Error {
        code: "requestInvalid".to_string(),
        message: error.to_string(),
    })?;

    let spawn_failed = |message: String| Frame::Error {
        code: "spawnFailed".to_string(),
        message,
    };
    let passwd = match std::fs::read("/etc/passwd") {
        // Lossy: an invalid line elsewhere in the file must not cost this uid its entry.
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        // No account file: the command gets what `docker exec` gives a uid without an entry.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(spawn_failed(format!("cannot read /etc/passwd: {error}"))),
    };
    // Startup already parsed this file and it is root-owned, so this fails only if it changed or
    // could not be read since. The frame names only what failed; the log keeps the parse error.
    let image = crate::image_permissions::image_config_env().map_err(|error| {
        tracing::warn!(%error, "the image config could not be read for a command");
        spawn_failed("the image config could not be read".to_string())
    })?;
    let mut environment = account_environment(identity.uid, &passwd, &image);
    environment.extend(request.env.clone());
    let mut command =
        sandbox_process::spawn_sandboxed(&request.command[0], &request.command[1..], &environment)
            .map_err(|_| Frame::Error {
                code: "spawnFailed".to_string(),
                message: "command could not be prepared".to_string(),
            })?;
    if piped_stdin {
        command.stdin(std::process::Stdio::piped());
    }

    command
        .env("ALIEN_SANDBOX_EXEC_UID", identity.uid.to_string())
        .env("ALIEN_SANDBOX_EXEC_GID", identity.gid.to_string());
    if let Some(root) = std::env::var_os("ALIEN_SANDBOX_ROOT") {
        command.env("ALIEN_SANDBOX_ROOT", root);
    }

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
        let directory =
            std::ffi::CString::new(directory.as_os_str().as_bytes()).map_err(|_| Frame::Error {
                code: "requestInvalid".to_string(),
                message: "the working directory contains a NUL byte".to_string(),
            })?;

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

    crate::children::spawn(&mut command).map_err(|error| Frame::Error {
        code: "spawnFailed".to_string(),
        message: error.to_string(),
    })
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

/// `HOME` and `USER` for a command run as `uid`: the image's own value where its config sets a
/// usable (non-empty, NUL-free) one, else the account's passwd entry. `HOME` is `/` when the entry
/// has no usable home or there is no entry, and there is no `USER` without a usable name.
fn account_environment(
    uid: u32,
    passwd: &str,
    image: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let account = alien_core::sandbox_identity::passwd_account(passwd, uid);
    let usable = |value: &&str| alien_core::sandbox_identity::usable_account_value(value);
    let mut environment = BTreeMap::from([(
        "HOME".to_string(),
        account
            .map(|(_, home)| home)
            .filter(usable)
            .unwrap_or("/")
            .to_string(),
    )]);
    if let Some(name) = account.map(|(name, _)| name).filter(usable) {
        environment.insert("USER".to_string(), name.to_string());
    }
    for name in ["HOME", "USER"] {
        if let Some(value) = image.get(name).map(String::as_str).filter(usable) {
            environment.insert(name.to_string(), value.to_string());
        }
    }
    environment
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/sh\n\
                          sandbox:x:60000:60000::/sandbox:/sbin/nologin\n\
                          homeless:x:60001:60001:::/sbin/nologin\n";

    fn account(uid: u32, passwd: &str, image: &[(&str, &str)]) -> BTreeMap<String, String> {
        let image = image
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        account_environment(uid, passwd, &image)
    }

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn a_command_gets_its_accounts_home_and_name() {
        assert_eq!(
            account(60000, PASSWD, &[]),
            vars(&[("HOME", "/sandbox"), ("USER", "sandbox")])
        );
    }

    #[test]
    fn the_images_own_home_and_user_win_over_the_account() {
        assert_eq!(
            account(
                60000,
                PASSWD,
                &[("HOME", "/image-home"), ("PATH", "/image/bin")]
            ),
            vars(&[("HOME", "/image-home"), ("USER", "sandbox")]),
            "only HOME and USER come from the image, and its PATH stays the agent's"
        );
        assert_eq!(
            account(60000, PASSWD, &[("USER", "builder")]),
            vars(&[("HOME", "/sandbox"), ("USER", "builder")])
        );
        assert_eq!(
            account(60000, PASSWD, &[("HOME", "")]),
            vars(&[("HOME", "/sandbox"), ("USER", "sandbox")]),
            "an empty HOME in the image counts as unset"
        );
    }

    #[test]
    fn a_uid_with_no_entry_gets_root_as_home_and_no_user() {
        assert_eq!(account(4242, PASSWD, &[]), vars(&[("HOME", "/")]));
        assert_eq!(
            account(60000, "", &[]),
            vars(&[("HOME", "/")]),
            "an image with no passwd file"
        );
    }

    #[test]
    fn an_entry_with_an_empty_home_gets_root_as_home() {
        assert_eq!(
            account(60001, PASSWD, &[]),
            vars(&[("HOME", "/"), ("USER", "homeless")])
        );
    }

    #[test]
    fn a_value_with_a_nul_is_left_out() {
        assert_eq!(
            account(60000, PASSWD, &[("HOME", "/bad\0home")]),
            vars(&[("HOME", "/sandbox"), ("USER", "sandbox")]),
            "an image HOME with a NUL falls back to the account's"
        );
        assert_eq!(
            account(60000, "sandbox:x:60000:60000::/bad\0home:/bin/sh\n", &[]),
            vars(&[("HOME", "/"), ("USER", "sandbox")])
        );
    }

    #[test]
    fn an_unusable_image_user_or_account_name_is_left_out() {
        assert_eq!(
            account(60000, PASSWD, &[("USER", "")]),
            vars(&[("HOME", "/sandbox"), ("USER", "sandbox")]),
            "an empty USER in the image counts as unset"
        );
        assert_eq!(
            account(60000, PASSWD, &[("USER", "a\0b")]),
            vars(&[("HOME", "/sandbox"), ("USER", "sandbox")])
        );
        assert_eq!(
            account(60002, ":x:60002:60002::/h:/bin/sh\n", &[]),
            vars(&[("HOME", "/h")]),
            "an entry with no name gives no USER"
        );
        assert_eq!(
            account(60000, "a\0b:x:60000:60000::/h:/bin/sh\n", &[]),
            vars(&[("HOME", "/h")])
        );
        assert_eq!(
            account(
                60000,
                "first:x:60000:60000::/first:/bin/sh\nsecond:x:60000:60000::/second:/bin/sh\n",
                &[]
            ),
            vars(&[("HOME", "/first"), ("USER", "first")]),
            "the first entry for a uid wins"
        );
    }

    fn request(command: &[&str], timeout_ms: u64) -> ExecRequest {
        ExecRequest {
            command: command.iter().map(|s| s.to_string()).collect(),
            timeout_ms,
            cwd: None,
            env: BTreeMap::new(),
            reply_budget: None,
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

    /// Once a frame does not fit, nothing later on that stream is kept, however small, so the kept
    /// output is a clean prefix; the other stream keeps its own allowance.
    #[test]
    fn a_stream_stops_at_its_first_dropped_frame() {
        let big = Frame::Stdout {
            seq: 0,
            data: "A".repeat(400),
        };
        let small = |seq| Frame::Stdout {
            seq,
            data: "A".to_string(),
        };
        let mut budget = ReplyBudget::new(Some(frame_wire_cost(&small(1)) * 2));
        assert!(budget.admit(big).is_none());
        assert!(
            budget.admit(small(1)).is_none(),
            "a gap must not be filled in"
        );
        assert!(budget
            .admit(Frame::Stderr {
                seq: 2,
                data: "A".to_string()
            })
            .is_some());
        assert!(matches!(
            budget.admit(Frame::Exit {
                code: 0,
                truncated: false
            }),
            Some(Frame::Exit {
                truncated: true,
                ..
            })
        ));
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

    /// The timeout is enforced, not advisory — this is the rule that stops hostile code
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
            Frame::Error { code, .. } => assert_eq!(code, "timeoutExceeded"),
            other => panic!("expected a timeout error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_request_without_a_timeout_is_refused_before_spawning() {
        let frames = run(
            &request(&["/bin/echo", "hi"], 0),
            None,
            same_identity(),
            1 << 20,
        )
        .await;

        match terminal(&frames) {
            Frame::Error { code, .. } => assert_eq!(code, "requestInvalid"),
            other => panic!("a zero timeout must be refused, got {other:?}"),
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
