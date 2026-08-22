//! Turning a child process into sandbox output frames.
//!
//! Two backends need this and neither can be the other's dependency: the in-sandbox agent runs a
//! command in its own guest, and the GCP binding runs one through a launcher CLI on the Cloud
//! Run container. The framing rules are the same on both sides and subtle enough that a second
//! implementation would drift, so they live here once.
//!
//! The rules, all of which cost something to learn:
//!
//! - **One sequence across both streams.** Two counters cannot express production order.
//! - **The two streams are drained as independent futures, joined at the whole-stream level.**
//!   Joining per read stalls a command that writes only to stdout until it exits, which makes
//!   streaming dead while every test whose command exits immediately still passes.
//! - **Exactly one terminal frame, always last.**
//! - **A read that fails is not a clean exit.** Reporting an exit code over output the caller
//!   never received describes a command that did not happen.
//! - **The deadline is enforced, not advisory.** Kill first, then report. The command leads its
//!   own process group and the group is killed, so what it forked goes with it — except a child
//!   that calls `setsid`, which needs a cgroup or a PID namespace to contain.
//! - **Backpressure is the send.** A bounded channel means a chatty command blocks rather than
//!   growing a buffer, and a dropped receiver kills the process instead of leaving it running.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// Largest chunk read before a frame is emitted.
///
/// Output with no newline in it would otherwise be buffered whole: `output_cap` is enforced only
/// after a read returns, so it bounds what is kept, never what is allocated.
const MAX_FRAME_BYTES: u64 = 64 * 1024;

/// Port the agent listens on inside a sandbox.
///
/// Defined once because two independent copies are a runtime-only failure: the image build places
/// the agent on one port and the client dials the other, and nothing catches it until a session
/// hangs. AWS scopes its endpoint token to an explicit port set, so this cannot be discovered.
pub const AGENT_PORT: u16 = 8971;

/// How many frames may sit between the process and the caller.
///
/// Small on purpose: this is the backpressure window, and a large one would just be a buffer
/// that hides a caller who has stopped reading.
pub const FRAME_CHANNEL_DEPTH: usize = 16;

/// Which stream a chunk of output came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStream {
    Stdout,
    Stderr,
}

/// One frame of a running process's output, before a backend maps it onto its own wire type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessFrame {
    /// Bytes read from one of the two streams
    Output {
        /// Monotonic across both streams
        seq: u64,
        stream: ProcessStream,
        /// Raw bytes; output is not necessarily UTF-8
        data: Vec<u8>,
    },
    /// The process finished. Terminal.
    Exit {
        /// Exit code, or -1 when the process was signalled and reported none
        code: i32,
        /// Set when output was cut short by `output_cap` rather than by the process ending
        truncated: bool,
    },
    /// The process did not finish. Also terminal.
    Failed { code: &'static str, message: String },
}

/// The only variable [`spawn_sandboxed`] gives a command for free. Without it the program name
/// resolves against nothing, so `python` in an image that has one would stop working.
const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Spawns one of Alien's own helper processes, inheriting the environment it runs in.
///
/// For a caller's code use [`spawn_sandboxed`] instead — the difference is the whole security
/// property, which is why these are two functions and not a flag.
///
/// stdin is null rather than inherited: a sandboxed command that blocks on a terminal read would
/// hold its deadline open with nothing to answer it.
pub fn spawn(program: &str, arguments: &[String]) -> std::io::Result<Command> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Its own process group, so a deadline can reach everything the command forked. Without it
    // `kill` reaches the direct child only, and `sh -c 'sleep 3000 &'` outlives its own deadline.
    #[cfg(unix)]
    command.process_group(0);

    Ok(command)
}

/// Kills the command and everything it forked.
///
/// The child leads its own group, so the negated pid names that group and nothing else. Falls back
/// to the child alone if it has already been reaped and has no pid to name.
async fn kill_all(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: `kill(2)` with a negative pid signals a process group. The group is this
        // child's own, established at spawn, so nothing else can be in it.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }

    let _ = child.kill().await;
}

/// Spawns untrusted code with a fresh environment: `PATH`, plus `environment` and nothing else.
///
/// The ambient environment is not inherited. The agent's own environment names its port, its
/// session root and its session id, so passing it down hands untrusted code a map to the API that
/// is running it — along with whatever else the runtime happened to set.
pub fn spawn_sandboxed(
    program: &str,
    arguments: &[String],
    environment: &BTreeMap<String, String>,
) -> std::io::Result<Command> {
    let mut command = spawn(program, arguments)?;
    command
        .env_clear()
        .env("PATH", DEFAULT_PATH)
        .envs(environment);
    Ok(command)
}

/// Streams a spawned child's output, sending frames as they are produced.
///
/// `output_cap` bounds how many bytes of each stream are kept. Beyond it the process still runs
/// to completion, because killing a command over a chatty log would be a surprising failure, but
/// the extra output is dropped and the terminal frame says so.
pub async fn stream(
    mut child: Child,
    deadline: Duration,
    output_cap: usize,
    frames: mpsc::Sender<ProcessFrame>,
) {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let seq = AtomicU64::new(0);
    let truncated = AtomicBool::new(false);
    let read_error: Mutex<Option<String>> = Mutex::new(None);

    let pump = async {
        let (stdout_connected, stderr_connected) = tokio::join!(
            drain(
                stdout,
                ProcessStream::Stdout,
                output_cap,
                &seq,
                &truncated,
                &read_error,
                &frames
            ),
            drain(
                stderr,
                ProcessStream::Stderr,
                output_cap,
                &seq,
                &truncated,
                &read_error,
                &frames
            ),
        );
        stdout_connected && stderr_connected
    };

    let mut connected = true;
    let outcome = tokio::time::timeout(deadline, async {
        // Racing the channel against the work, not only draining it: `pump` learns the caller
        // left by failing to send, so a command that prints nothing would run to its deadline
        // after everyone stopped listening. `closed()` resolves as soon as the receiver drops,
        // whether or not there was ever a frame to deliver.
        tokio::select! {
            biased;
            () = frames.closed() => {
                connected = false;
                Ok(std::process::ExitStatus::default())
            }
            result = async {
                connected = pump.await;
                child.wait().await
            } => result,
        }
    })
    .await;

    // The caller went away, so there is nobody to report to and no reason to keep running.
    if !connected {
        kill_all(&mut child).await;
        return;
    }

    let truncated = truncated.load(Ordering::Relaxed);
    let read_error = read_error.into_inner().expect("no panic holds this lock");

    let terminal = match outcome {
        Ok(Ok(_)) if read_error.is_some() => ProcessFrame::Failed {
            code: "outputReadFailed",
            message: read_error.expect("checked"),
        },
        Ok(Ok(status)) => ProcessFrame::Exit {
            code: status.code().unwrap_or(-1),
            truncated,
        },
        Ok(Err(error)) => ProcessFrame::Failed {
            code: "waitFailed",
            message: error.to_string(),
        },
        Err(_) => {
            kill_all(&mut child).await;
            ProcessFrame::Failed {
                code: "deadlineExceeded",
                message: format!("exceeded its {}ms deadline", deadline.as_millis()),
            }
        }
    };

    let _ = frames.send(terminal).await;
}

/// Runs a child to completion and collects every frame.
///
/// Safe on unbounded output: [`stream`] blocks on a full channel rather than buffering, and
/// `output_cap` still applies to what is kept.
pub async fn run(child: Child, deadline: Duration, output_cap: usize) -> Vec<ProcessFrame> {
    let (sender, mut receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);

    let produce = stream(child, deadline, output_cap, sender);
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

/// Reads one stream to EOF, framing each line. Returns false if the caller went away.
///
/// A truncated line still consumes a sequence number, so a gap tells the caller output was
/// dropped rather than hiding it.
async fn drain<R>(
    stream: Option<R>,
    which: ProcessStream,
    output_cap: usize,
    seq: &AtomicU64,
    truncated: &AtomicBool,
    read_error: &Mutex<Option<String>>,
    frames: &mpsc::Sender<ProcessFrame>,
) -> bool
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(stream) = stream else {
        return true;
    };

    let mut reader = BufReader::new(stream);
    let mut kept = 0usize;

    loop {
        let mut line = Vec::new();
        // Bounded per read: `output_cap` is checked only after a read returns, so an unbounded
        // `read_until` on output that never contains a newline grows this buffer until the
        // process is killed. A longer line is split, which the sequence numbers already express.
        match (&mut reader)
            .take(MAX_FRAME_BYTES)
            .read_until(b'\n', &mut line)
            .await
        {
            Ok(0) => return true,
            Err(error) => {
                read_error
                    .lock()
                    .expect("no panic holds this lock")
                    .get_or_insert_with(|| error.to_string());
                return true;
            }
            Ok(read) => {
                let number = seq.fetch_add(1, Ordering::Relaxed);

                if kept + read > output_cap {
                    truncated.store(true, Ordering::Relaxed);
                    continue;
                }

                kept += read;
                if frames
                    .send(ProcessFrame::Output {
                        seq: number,
                        stream: which,
                        data: line,
                    })
                    .await
                    .is_err()
                {
                    return false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(command: &[&str]) -> Child {
        let arguments: Vec<String> = command[1..].iter().map(|s| s.to_string()).collect();
        spawn(command[0], &arguments)
            .expect("command builds")
            .spawn()
            .expect("command spawns")
    }

    /// A caller that stops reading has to stop the command, not merely stop hearing from it.
    /// Learning that from a failed send only works for a command that prints: a silent one would
    /// hold a sandbox until its deadline after everyone had gone. The deadline here is long
    /// enough that reaching it would fail the test rather than pass it slowly.
    #[tokio::test]
    async fn a_silent_command_is_killed_when_the_caller_stops_listening() {
        let (sender, receiver) = mpsc::channel(4);
        let running = tokio::spawn(stream(
            child(&["/bin/sh", "-c", "sleep 60"]),
            Duration::from_secs(60),
            1024,
            sender,
        ));

        drop(receiver);

        tokio::time::timeout(Duration::from_secs(10), running)
            .await
            .expect("dropping the receiver stops the command rather than waiting out its deadline")
            .expect("the task does not panic");
    }

    fn terminal(frames: &[ProcessFrame]) -> &ProcessFrame {
        frames.last().expect("there is always a terminal frame")
    }

    fn stdout_of(frames: &[ProcessFrame]) -> String {
        let mut collected = Vec::new();
        for frame in frames {
            if let ProcessFrame::Output {
                stream: ProcessStream::Stdout,
                data,
                ..
            } = frame
            {
                collected.extend_from_slice(data);
            }
        }
        String::from_utf8_lossy(&collected).into_owned()
    }

    /// A deadline that reaches only the direct child is not a deadline: the command backgrounds a
    /// process and returns, and that process keeps the session's CPU and files after the caller
    /// was told the command was killed.
    #[tokio::test]
    #[cfg(unix)]
    async fn a_deadline_kills_what_the_command_forked() {
        let marker = std::env::temp_dir().join(format!("alien-forked-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);

        // The grandchild outlives the deadline and keeps writing. stdout is closed so it cannot
        // hold the pipe open — this test is about the process surviving, not about the stream.
        let script = format!(
            "(while true; do echo x >> {} ; sleep 0.05; done) >/dev/null 2>&1 &\nsleep 30",
            marker.display()
        );
        let command = spawn("/bin/sh", &["-c".to_string(), script])
            .expect("command builds")
            .spawn()
            .expect("command spawns");

        let frames = run(command, Duration::from_millis(400), 1 << 20).await;
        assert!(
            matches!(
                terminal(&frames),
                ProcessFrame::Failed {
                    code: "deadlineExceeded",
                    ..
                }
            ),
            "the command must hit its deadline: {frames:?}"
        );

        // Let anything still running write again, then compare: a survivor keeps growing the file.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let after_kill = std::fs::metadata(&marker).map(|m| m.len());
        tokio::time::sleep(Duration::from_millis(300)).await;
        let later = std::fs::metadata(&marker).map(|m| m.len());
        let _ = std::fs::remove_file(&marker);

        // Asserted, not defaulted: if the grandchild never ran, both reads would be "missing" and
        // a comparison of two absent files would pass while proving nothing.
        let after_kill = after_kill.expect("the forked process must have written before the kill");
        let later = later.expect("the marker must still exist");
        assert!(
            after_kill > 0,
            "the forked process wrote nothing, so this test proves nothing"
        );
        assert_eq!(
            after_kill, later,
            "a process the command forked outlived the deadline and is still writing"
        );
    }

    /// The security property [`spawn_sandboxed`] exists for, asserted rather than assumed. The
    /// control arm matters as much as the leak arm: a command that saw no variables because the
    /// shell never ran would pass a one-sided version of this test.
    #[tokio::test]
    async fn a_sandboxed_command_does_not_inherit_the_ambient_environment() {
        std::env::set_var("ALIEN_SANDBOX_LEAK_PROBE", "leaked");

        let environment = BTreeMap::from([("PASSED_IN".to_string(), "yes".to_string())]);
        let command = spawn_sandboxed(
            "/bin/sh",
            &[
                "-c".to_string(),
                "echo \"ambient=${ALIEN_SANDBOX_LEAK_PROBE:-absent} passed=${PASSED_IN:-absent}\""
                    .to_string(),
            ],
            &environment,
        )
        .expect("command builds")
        .spawn()
        .expect("command spawns");

        let frames = run(command, Duration::from_secs(10), 1 << 20).await;

        assert!(
            matches!(terminal(&frames), ProcessFrame::Exit { code: 0, .. }),
            "the probe must actually run: {frames:?}"
        );
        assert_eq!(
            stdout_of(&frames).trim(),
            "ambient=absent passed=yes",
            "the ambient environment must not cross into a caller's command"
        );
    }

    #[tokio::test]
    async fn output_is_followed_by_exactly_one_terminal_frame() {
        let frames = run(
            child(&["/bin/echo", "hello"]),
            Duration::from_secs(10),
            1 << 20,
        )
        .await;

        assert!(matches!(
            terminal(&frames),
            ProcessFrame::Exit { code: 0, .. }
        ));
        assert_eq!(
            frames
                .iter()
                .filter(|frame| matches!(
                    frame,
                    ProcessFrame::Exit { .. } | ProcessFrame::Failed { .. }
                ))
                .count(),
            1
        );
    }

    /// The ordering property the single counter exists for: a caller can interleave the two
    /// streams back into the order the process produced them.
    #[tokio::test]
    async fn both_streams_share_one_monotonic_sequence() {
        let frames = run(
            child(&["/bin/sh", "-c", "echo out; echo err 1>&2; echo out2"]),
            Duration::from_secs(10),
            1 << 20,
        )
        .await;

        let sequence: Vec<u64> = frames
            .iter()
            .filter_map(|frame| match frame {
                ProcessFrame::Output { seq, .. } => Some(*seq),
                _ => None,
            })
            .collect();

        let mut sorted = sequence.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), sequence.len(), "no sequence number is reused");
        assert!(
            frames.iter().any(|frame| matches!(
                frame,
                ProcessFrame::Output {
                    stream: ProcessStream::Stderr,
                    ..
                }
            )),
            "stderr must be framed, not dropped"
        );
    }

    /// A command that never ends must still end. The kill happens before the report, so a
    /// timeout never leaves a runaway behind.
    #[tokio::test]
    async fn a_deadline_ends_a_command_that_would_not() {
        let frames = run(
            child(&["/bin/sh", "-c", "sleep 30"]),
            Duration::from_millis(200),
            1 << 20,
        )
        .await;

        assert!(matches!(
            terminal(&frames),
            ProcessFrame::Failed {
                code: "deadlineExceeded",
                ..
            }
        ));
    }

    /// Output past the cap is dropped and declared, rather than the command being killed over a
    /// chatty log.
    #[tokio::test]
    async fn output_past_the_cap_is_truncated_and_the_terminal_frame_says_so() {
        let frames = run(
            child(&[
                "/bin/sh",
                "-c",
                "for i in 1 2 3 4 5 6 7 8 9 10; do echo aaaaaaaaaa; done",
            ]),
            Duration::from_secs(10),
            8,
        )
        .await;

        assert!(matches!(
            terminal(&frames),
            ProcessFrame::Exit {
                code: 0,
                truncated: true
            }
        ));
    }

    /// A caller that stops reading must stop the process, not leak it.
    /// A caller that stops reading must stop the process, not leak it.
    ///
    /// The bounded channel is what applies the backpressure, and a dropped receiver is a caller
    /// that went away — the process it was waiting on has nobody left to report to.
    #[tokio::test]
    async fn a_departed_caller_kills_the_process() {
        let (sender, receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);
        let produce = tokio::spawn(stream(
            child(&["/bin/sh", "-c", "while true; do echo aaaaaaaa; done"]),
            Duration::from_secs(30),
            1 << 30,
            sender,
        ));

        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(
            !produce.is_finished(),
            "an endless command must still be running, not drained into memory"
        );

        drop(receiver);

        tokio::time::timeout(Duration::from_secs(10), produce)
            .await
            .expect("a command whose caller left must be killed, not left running")
            .expect("the producing task must not panic");
    }

    /// Frames arrive while the process is still running. This is the property the whole-stream
    /// join exists for, and the one a command that exits immediately cannot prove.
    #[tokio::test]
    async fn frames_arrive_before_the_process_exits() {
        let (sender, mut receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);
        let produce = tokio::spawn(stream(
            child(&["/bin/sh", "-c", "echo first; sleep 5; echo second"]),
            Duration::from_secs(30),
            1 << 20,
            sender,
        ));

        let first = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("the first frame must arrive long before the process exits")
            .expect("a frame");

        assert!(matches!(first, ProcessFrame::Output { .. }));

        produce.abort();
    }
}
