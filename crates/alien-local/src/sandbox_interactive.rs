//! Interactive commands in local sandbox sessions, on the sandbox agent's wire.
//!
//! The binding drives these exactly as it drives the in-sandbox agent: one NDJSON output stream
//! per command, and short requests for input, close and signals naming the command by the id the
//! caller chose. The difference is who supervises. Nothing Alien runs inside a local session, so
//! the manager holds the exec's attached stdin and enforces the deadline from outside, where the
//! command cannot reach the supervisor.
//!
//! Signals need the command's process group, and Docker reports only a host pid. The command is
//! started under a one-line shell that writes its own pid and start time to stderr and then
//! `exec`s the command, so both are the command's. A `docker exec` process already leads its own
//! process group and session, so the pid is also the group. The line is written before the command
//! exists, which makes it the first bytes on stderr with nothing able to race it. The start waits
//! for it, so a command is never running without a group the manager can signal, and it is removed
//! before any output reaches the caller.
//!
//! Docker reaps the command inside the container, out of the manager's sight, so the pid can be
//! free again before the manager learns the command ended. A signal therefore checks the start
//! time first: a leader with the pid but another start time is a different process, and is left
//! alone. A leader that is gone while its group lives on cannot have been replaced, because the
//! kernel does not hand out a number a process group still uses. What remains is the instant
//! between the check and the kill, in which the command would have to end, be reaped and have its
//! number handed to a new group leader.
//!
//! Two things differ from the agent and are stated rather than hidden: an acknowledged write has
//! reached Docker's attach socket rather than provably the program's pipe, and output is not
//! capped, because the caller is the developer's own machine.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::sandbox_manager::{InteractiveExec, LocalSandboxManager};

/// The most interactive commands one session runs at once, matching the agent.
const MAX_INTERACTIVE: usize = 16;

/// How long one write may wait for the command to read before answering with a short count,
/// matching the agent.
pub const STDIN_WRITE_WAIT: Duration = Duration::from_secs(20);

/// How many frames may wait between Docker and the caller: the backpressure window.
const FRAME_CHANNEL_DEPTH: usize = 16;

/// The longest the announcement line may be. A pid and a start time are a few digits each;
/// anything longer means the wrapper did not run as written, and the command is refused rather
/// than guessed at.
const MAX_ANNOUNCEMENT: usize = 64;

/// Announces the shell's pid and start time (the 22nd field of its `stat`, counted past the
/// command name, which may hold spaces) on stderr, then becomes the command, which keeps both.
const WRAPPER: &str = "f() { printf '%s %s\\n' \"$$\" \"${20}\" >&2; }; \
     read -r s 2>/dev/null < /proc/$$/stat || s=; f ${s##*) }; exec \"$@\"";

/// Signals a process group, unless its leader has been replaced. `$1` is the signal, `$2` the
/// group and `$3` the leader's start time. Exit 3 means the group no longer exists, or what has
/// its number is not the command. `kill -SIG -N` is the one spelling dash and busybox both take.
const KILL_GROUP: &str = "if [ -n \"$3\" ] && read -r s 2>/dev/null < \"/proc/$2/stat\"; then \
     f() { [ \"${21}\" = \"$1\" ]; }; f \"$3\" ${s##*) } || exit 3; fi; \
     kill -\"$1\" -\"$2\" 2>/dev/null && exit 0; kill -0 -\"$2\" 2>/dev/null && exit 2; exit 3";

/// How long the start waits for the wrapper to announce the pid.
const ANNOUNCEMENT_WAIT: Duration = Duration::from_secs(10);

/// How long to wait for the output to end after the group was killed.
const KILL_SETTLE: Duration = Duration::from_secs(10);

type Output = std::pin::Pin<
    Box<
        dyn futures::Stream<
                Item = std::result::Result<bollard::container::LogOutput, bollard::errors::Error>,
            > + Send,
    >,
>;

/// One frame, serialized exactly as the agent writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "t")]
pub enum Frame {
    Stdout { seq: u64, data: String },
    Stderr { seq: u64, data: String },
    Exit { code: i32, truncated: bool },
    Error { code: String, message: String },
}

/// Why a request was refused. Each carries the code the agent would put first in its body, which
/// is what the binding reads.
#[derive(Debug)]
pub enum ControlError {
    /// No command by that id is running in this session: 404 `COMMAND_NOT_RUNNING`
    NotRunning,
    /// stdin is closed: 409 `STDIN_CLOSED`
    StdinClosed,
    /// A write would leave a gap: 409 `STDIN_OFFSET_MISMATCH`
    OffsetMismatch { accepted: u64, offset: u64 },
    /// The request itself is malformed: 400 `REQUEST_INVALID`
    Invalid(String),
    /// A write from before the caller's last barrier: 409 `STDIN_STALE_WRITE`
    StaleWrite { generation: u64, current: u64 },
    /// Every slot in the session is taken: 429 `INTERACTIVE_LIMIT_REACHED`
    Full,
    /// Docker failed: 502
    Failed(String),
}

impl ControlError {
    /// The status and body the route answers with.
    pub fn response(self) -> (u16, String) {
        match self {
            Self::NotRunning => (
                404,
                "COMMAND_NOT_RUNNING: no such interactive command is running in this session"
                    .to_string(),
            ),
            Self::StdinClosed => (
                409,
                "STDIN_CLOSED: the command no longer reads its input".to_string(),
            ),
            Self::OffsetMismatch { accepted, offset } => (
                409,
                format!(
                    "STDIN_OFFSET_MISMATCH: the command has taken {accepted} bytes of input; a \
                     write at offset {offset} would leave a gap"
                ),
            ),
            Self::StaleWrite {
                generation,
                current,
            } => (
                409,
                format!(
                    "STDIN_STALE_WRITE: a write from input generation {generation} arrived after \
                     the input moved to generation {current}"
                ),
            ),
            Self::Invalid(reason) => (400, format!("REQUEST_INVALID: {reason}")),
            Self::Full => (
                429,
                format!(
                    "INTERACTIVE_LIMIT_REACHED: the session is running its maximum of \
                     {MAX_INTERACTIVE} interactive commands"
                ),
            ),
            Self::Failed(reason) => (502, reason),
        }
    }
}

/// The interactive commands one local sandbox is running, keyed by session and id.
#[derive(Default)]
pub struct InteractiveRegistry {
    commands: Mutex<HashMap<(String, String), Slot>>,
}

/// An id in use. It is held from the moment a start is accepted, before anything awaits, so
/// concurrent starts cannot together pass the limit or share an id.
enum Slot {
    /// Started but not yet answered: nothing can name it yet
    Starting,
    Running(Arc<Command>),
}

struct Command {
    container_id: String,
    stdin: tokio::sync::Mutex<Stdin>,
    /// Set before the start answers; never changes. Docker reaps the exec inside the container,
    /// and the id is released only once the exec's output has ended.
    process_group: i64,
    /// The leader's start time in clock ticks since boot, as the wrapper read it. Empty if
    /// `/proc` could not be read, in which case signals go unchecked.
    started: String,
}

struct Stdin {
    pipe: Option<std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>>,
    accepted: u64,
    /// Moved on by each barrier. Only writes carrying the current one are applied.
    generation: u64,
}

/// A held id, released when dropped: after that no request can name the command.
struct Reservation {
    registry: Arc<InteractiveRegistry>,
    key: (String, String),
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.registry.lock().remove(&self.key);
    }
}

/// What a start needs besides the session.
pub struct StartRequest {
    pub id: String,
    pub command: Vec<String>,
    pub timeout: Duration,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

impl InteractiveRegistry {
    fn lock(&self) -> MutexGuard<'_, HashMap<(String, String), Slot>> {
        self.commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn get(&self, session: &str, id: &str) -> Result<Arc<Command>, ControlError> {
        match self.lock().get(&(session.to_string(), id.to_string())) {
            Some(Slot::Running(command)) => Ok(Arc::clone(command)),
            Some(Slot::Starting) | None => Err(ControlError::NotRunning),
        }
    }

    /// Starts a command and returns the receiver its frames arrive on.
    ///
    /// Returns only once the command is running and its process group is known, so the caller's
    /// first write or signal cannot race it.
    pub async fn start(
        self: &Arc<Self>,
        manager: Arc<LocalSandboxManager>,
        session: &str,
        container_id: String,
        request: StartRequest,
    ) -> Result<mpsc::Receiver<Frame>, ControlError> {
        validate(&request)?;
        let key = (session.to_string(), request.id.clone());
        // Held from here: every early return below releases it by dropping.
        let reservation = {
            let mut commands = self.lock();
            if commands.contains_key(&key) {
                return Err(ControlError::Invalid(format!(
                    "interactive command '{}' is already running",
                    request.id
                )));
            }
            if commands
                .keys()
                .filter(|(other, _)| other == session)
                .count()
                >= MAX_INTERACTIVE
            {
                return Err(ControlError::Full);
            }
            commands.insert(key.clone(), Slot::Starting);
            Reservation {
                registry: Arc::clone(self),
                key,
            }
        };

        let mut argv = vec![
            "sh".to_string(),
            "-c".to_string(),
            WRAPPER.to_string(),
            "sh".to_string(),
        ];
        argv.extend(request.command.iter().cloned());

        let InteractiveExec {
            exec_id,
            input,
            mut output,
        } = manager
            .exec_interactive(&container_id, &argv, request.cwd.as_deref(), &request.env)
            .await
            .map_err(|error| ControlError::Failed(error.to_string()))?;
        let deadline = tokio::time::Instant::now() + request.timeout;

        let ((process_group, started), pending) = match announcement(&mut output).await {
            Ok(announced) => announced,
            Err(reason) => {
                // Without the pid there is nothing to signal, so end the input and the attach and
                // report the start as failed; a wrapper that never ran has started nothing.
                drop(input);
                drop(output);
                return Err(ControlError::Failed(reason));
            }
        };

        let command = Arc::new(Command {
            container_id,
            stdin: tokio::sync::Mutex::new(Stdin {
                pipe: Some(input),
                accepted: 0,
                generation: 0,
            }),
            process_group,
            started,
        });
        self.lock()
            .insert(reservation.key.clone(), Slot::Running(Arc::clone(&command)));

        let (frames, receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);
        tokio::spawn(pump(
            manager,
            command,
            reservation,
            exec_id,
            output,
            pending,
            deadline,
            request.timeout,
            frames,
        ));
        Ok(receiver)
    }

    /// Writes input starting at `offset`, returning how many bytes the command has taken in all.
    pub async fn write(
        &self,
        session: &str,
        id: &str,
        generation: u64,
        offset: u64,
        data: &[u8],
    ) -> Result<u64, ControlError> {
        let command = self.get(session, id)?;
        let mut stdin = command.stdin.lock().await;
        let Stdin {
            pipe,
            accepted,
            generation: current,
        } = &mut *stdin;

        if generation != *current {
            return Err(ControlError::StaleWrite {
                generation,
                current: *current,
            });
        }

        if offset > *accepted {
            return Err(ControlError::OffsetMismatch {
                accepted: *accepted,
                offset,
            });
        }
        let already = usize::try_from(*accepted - offset).unwrap_or(usize::MAX);
        if already >= data.len() {
            return Ok(*accepted);
        }
        let Some(writer) = pipe.as_mut() else {
            return Err(ControlError::StdinClosed);
        };

        let deadline = tokio::time::Instant::now() + STDIN_WRITE_WAIT;
        let mut rest = &data[already..];
        while !rest.is_empty() {
            match tokio::time::timeout_at(deadline, writer.write(rest)).await {
                Err(_) => break,
                Ok(Ok(0)) | Ok(Err(_)) => {
                    *pipe = None;
                    return Err(ControlError::StdinClosed);
                }
                Ok(Ok(written)) => {
                    *accepted += written as u64;
                    rest = &rest[written..];
                }
            }
        }
        // Flushed so the bytes leave this process now rather than when a buffer fills, and under
        // the same wait, so a stalled socket cannot hold the request open.
        match tokio::time::timeout_at(deadline, writer.flush()).await {
            Ok(Ok(())) | Err(_) => {}
            Ok(Err(_)) => *pipe = None,
        }
        Ok(*accepted)
    }

    /// Retires every write sent so far: waits for one in progress, then moves the input to
    /// generation `target` unless it is already there or past it, so a repeated barrier moves it
    /// once. Returns how many bytes the command has taken and the current generation.
    pub async fn barrier(
        &self,
        session: &str,
        id: &str,
        target: u64,
    ) -> Result<(u64, u64), ControlError> {
        let command = self.get(session, id)?;
        let mut stdin = command.stdin.lock().await;
        stdin.generation = stdin.generation.max(target);
        Ok((stdin.accepted, stdin.generation))
    }

    /// Closes stdin. Closing twice is not an error.
    pub async fn close_stdin(&self, session: &str, id: &str) -> Result<(), ControlError> {
        let command = self.get(session, id)?;
        let mut stdin = command.stdin.lock().await;
        if let Some(mut pipe) = stdin.pipe.take() {
            // Docker passes a half-close on to the command as end of input.
            let _ = pipe.shutdown().await;
        }
        Ok(())
    }

    /// Signals the command's process group, from a second exec in the same container and as the
    /// same user, so it can reach nothing the command could not reach itself.
    pub async fn signal(
        &self,
        manager: &LocalSandboxManager,
        session: &str,
        id: &str,
        signal: alien_core::sandbox_signal::CommandSignal,
    ) -> Result<(), ControlError> {
        let command = self.get(session, id)?;
        kill_group(manager, &command, signal.posix_name()).await
    }
}

fn validate(request: &StartRequest) -> Result<(), ControlError> {
    let id = &request.id;
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(ControlError::Invalid(
            "an interactive command id is 1 to 128 characters of [A-Za-z0-9_-]".to_string(),
        ));
    }
    if request.command.is_empty() {
        return Err(ControlError::Invalid("command is empty".to_string()));
    }
    if request.timeout.is_zero() {
        return Err(ControlError::Invalid(
            "a command must carry a non-zero timeout".to_string(),
        ));
    }
    for (name, value) in &request.env {
        if name.is_empty() || name.contains('=') || name.contains('\0') || value.contains('\0') {
            return Err(ControlError::Invalid(format!(
                "environment variable '{name}' cannot be passed"
            )));
        }
    }
    Ok(())
}

/// Output read before the pid line, in the order Docker delivered it.
type Early = Vec<(bool, Vec<u8>)>;

/// Reads the wrapper's line off the front of stderr, returning the pid, the start time and
/// whatever output arrived around it. The line is the first thing on stderr, but Docker does not
/// order stdout against stderr, so the program's first stdout can arrive before it and is kept,
/// not refused.
async fn announcement(output: &mut Output) -> Result<((i64, String), Early), String> {
    let read = async {
        let mut stderr = Vec::new();
        let mut early: Early = Vec::new();
        loop {
            match output.next().await {
                None => return Err("the command wrapper ended before reporting a pid".to_string()),
                Some(Err(error)) => return Err(format!("reading the command wrapper: {error}")),
                Some(Ok(bollard::container::LogOutput::StdOut { message })) => {
                    early.push((true, message.to_vec()));
                }
                Some(Ok(bollard::container::LogOutput::StdErr { message })) => {
                    stderr.extend_from_slice(&message);
                    if let Some(end) = stderr.iter().position(|byte| *byte == b'\n') {
                        let line = String::from_utf8_lossy(&stderr[..end]).into_owned();
                        let announced = parse_announcement(&line).ok_or_else(|| {
                            format!("the command wrapper did not report a pid: {line:?}")
                        })?;
                        let rest = stderr[end + 1..].to_vec();
                        if !rest.is_empty() {
                            early.push((false, rest));
                        }
                        return Ok((announced, early));
                    }
                    if stderr.len() > MAX_ANNOUNCEMENT {
                        return Err("the command wrapper did not report a pid".to_string());
                    }
                }
                Some(Ok(_)) => {}
            }
        }
    };
    tokio::time::timeout(ANNOUNCEMENT_WAIT, read)
        .await
        .map_err(|_| {
            format!(
                "the command wrapper did not report a pid within {}s",
                ANNOUNCEMENT_WAIT.as_secs()
            )
        })?
}

/// `PID START`, where the start time is empty when `/proc` could not be read. A start time that
/// is not a number is refused, since the signal check compares it as written.
fn parse_announcement(line: &str) -> Option<(i64, String)> {
    let mut fields = line.split(' ');
    let pid = fields.next()?.parse::<i64>().ok().filter(|pid| *pid > 1)?;
    let started = fields.next().unwrap_or("");
    if fields.next().is_some() || !started.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((pid, started.to_string()))
}

async fn kill_group(
    manager: &LocalSandboxManager,
    command: &Command,
    signal: &str,
) -> Result<(), ControlError> {
    // Exit 3 means the group no longer exists or is no longer the command's; any other failure
    // is the signal failing, which is not the same answer.
    let result = manager
        .exec(
            &command.container_id,
            &[
                "sh".to_string(),
                "-c".to_string(),
                KILL_GROUP.to_string(),
                "sh".to_string(),
                signal.to_string(),
                command.process_group.to_string(),
                command.started.clone(),
            ],
        )
        .await
        .map_err(|error| ControlError::Failed(error.to_string()))?;
    match result.exit_code {
        0 => Ok(()),
        3 => Err(ControlError::NotRunning),
        code => Err(ControlError::Failed(format!(
            "signalling the command's process group exited with {code}"
        ))),
    }
}

/// Moves Docker's output to the caller until the command ends, enforcing its deadline whether or
/// not the caller is reading.
#[allow(clippy::too_many_arguments)] // One task's whole state, handed over once.
async fn pump(
    manager: Arc<LocalSandboxManager>,
    command: Arc<Command>,
    reservation: Reservation,
    exec_id: String,
    mut output: Output,
    early: Early,
    deadline: tokio::time::Instant,
    timeout: Duration,
    frames: mpsc::Sender<Frame>,
) {
    let mut seq = 0u64;

    enum Ending {
        Exited,
        Departed,
        TimedOut,
        Failed(String),
    }

    // Sends one frame, giving up at the deadline or when the caller leaves: a caller that keeps
    // the stream open without reading must not hold the command past its timeout.
    async fn deliver(
        frames: &mpsc::Sender<Frame>,
        frame: Frame,
        deadline: tokio::time::Instant,
    ) -> Option<Ending> {
        tokio::select! {
            biased;
            () = tokio::time::sleep_until(deadline) => Some(Ending::TimedOut),
            sent = frames.send(frame) => sent.err().map(|_| Ending::Departed),
        }
    }

    let mut ending = None;
    for (is_stdout, bytes) in early {
        let data = BASE64.encode(&bytes);
        let frame = if is_stdout {
            Frame::Stdout { seq, data }
        } else {
            Frame::Stderr { seq, data }
        };
        seq += 1;
        ending = deliver(&frames, frame, deadline).await;
        if ending.is_some() {
            break;
        }
    }

    let ending = match ending {
        Some(ending) => ending,
        None => loop {
            let item = tokio::select! {
                biased;
                () = frames.closed() => break Ending::Departed,
                () = tokio::time::sleep_until(deadline) => break Ending::TimedOut,
                item = output.next() => item,
            };
            let (stream_is_stdout, bytes) = match item {
                None => break Ending::Exited,
                Some(Err(error)) => break Ending::Failed(error.to_string()),
                Some(Ok(bollard::container::LogOutput::StdOut { message })) => {
                    (true, message.to_vec())
                }
                Some(Ok(bollard::container::LogOutput::StdErr { message })) => {
                    (false, message.to_vec())
                }
                Some(Ok(_)) => continue,
            };
            if bytes.is_empty() {
                continue;
            }
            let data = BASE64.encode(&bytes);
            let frame = if stream_is_stdout {
                Frame::Stdout { seq, data }
            } else {
                Frame::Stderr { seq, data }
            };
            seq += 1;
            if let Some(ending) = deliver(&frames, frame, deadline).await {
                break ending;
            }
        },
    };

    let terminal = match ending {
        Ending::Exited => match manager.exec_exit_code(&exec_id).await {
            Ok(Some(code)) => Frame::Exit {
                code: i32::try_from(code).unwrap_or(-1),
                truncated: false,
            },
            Ok(None) => Frame::Error {
                code: "waitFailed".to_string(),
                message: "the command's output ended without an exit code".to_string(),
            },
            Err(error) => Frame::Error {
                code: "waitFailed".to_string(),
                message: error.to_string(),
            },
        },
        Ending::Departed => {
            stop(&manager, &command, &mut output).await;
            return;
        }
        Ending::TimedOut => {
            if stop(&manager, &command, &mut output).await {
                Frame::Error {
                    code: "timeoutExceeded".to_string(),
                    message: format!("exceeded its {}ms timeout", timeout.as_millis()),
                }
            } else {
                // Reported only once the command has verifiably stopped; it has not.
                Frame::Error {
                    code: "killFailed".to_string(),
                    message: format!(
                        "exceeded its {}ms timeout and could not be confirmed stopped",
                        timeout.as_millis()
                    ),
                }
            }
        }
        Ending::Failed(message) => {
            stop(&manager, &command, &mut output).await;
            Frame::Error {
                code: "outputReadFailed".to_string(),
                message,
            }
        }
    };

    // The output has ended, so Docker has reaped the exec: release the id before the terminal
    // frame, which a slow reader can hold off indefinitely.
    drop(reservation);
    let _ = frames.send(terminal).await;
}

/// Kills the command's group and waits for its output to end, which is Docker saying the exec's
/// process is gone. Returns whether that was confirmed.
///
/// The output is drained while the kill runs, not after: an unread exec holds Docker's copy of
/// its output, and until that copy moves the container's other execs (the kill among them) and
/// its exit can stall behind it.
async fn stop(manager: &LocalSandboxManager, command: &Command, output: &mut Output) -> bool {
    let kill = kill_group(manager, command, "KILL");
    let drain = async { while output.next().await.is_some() {} };
    tokio::time::timeout(KILL_SETTLE, async {
        let _ = tokio::join!(kill, drain);
    })
    .await
    .is_ok()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    //! The wrapper and the signal script are plain `sh` over `/proc`, so they run here as they
    //! run in a container.

    use super::*;

    fn sh(script: &str, args: &[&str]) -> std::process::Command {
        let mut command = std::process::Command::new("/bin/sh");
        command.arg("-c").arg(script).arg("sh").args(args);
        command
    }

    /// The pid and start time a group leader started under `WRAPPER` announces.
    fn announced_leader() -> (std::process::Child, i64, String) {
        use std::io::BufRead;
        use std::os::unix::process::CommandExt;

        let mut child = sh(WRAPPER, &["sleep", "30"])
            .stderr(std::process::Stdio::piped())
            .process_group(0)
            .spawn()
            .expect("spawns");
        let mut line = String::new();
        std::io::BufReader::new(child.stderr.take().expect("piped"))
            .read_line(&mut line)
            .expect("reads the announcement");
        let (pid, started) = parse_announcement(line.trim_end()).expect("a pid and a start time");
        (child, pid, started)
    }

    fn start_time(pid: i64) -> String {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("stat");
        let (_, rest) = stat.rsplit_once(") ").expect("a command name");
        rest.split(' ').nth(19).expect("field 22").to_string()
    }

    #[test]
    fn the_wrapper_announces_the_commands_own_pid_and_start_time() {
        let (mut child, pid, started) = announced_leader();
        assert_eq!(pid, i64::from(child.id()), "the shell became the command");
        assert_eq!(started, start_time(pid), "exec keeps the start time");
        let _ = child.kill();
        let _ = child.wait();
    }

    /// The check that keeps a signal off a process that took a finished command's number: a
    /// leader with the right pid but another start time is left alone.
    #[test]
    fn a_signal_reaches_the_group_only_while_its_leader_is_the_command() {
        let (mut child, pid, started) = announced_leader();
        let group = pid.to_string();

        let other = (started.parse::<u64>().expect("a number") + 1).to_string();
        let status = sh(KILL_GROUP, &["TERM", &group, &other])
            .status()
            .expect("runs");
        assert_eq!(
            status.code(),
            Some(3),
            "another start time is another process"
        );
        assert!(
            child.try_wait().expect("waits").is_none(),
            "and it was not signalled"
        );

        let status = sh(KILL_GROUP, &["TERM", &group, &started])
            .status()
            .expect("runs");
        assert_eq!(status.code(), Some(0), "the command's own start time");
        let ended = child.wait().expect("waits");
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&ended),
            Some(libc::SIGTERM)
        );

        let status = sh(KILL_GROUP, &["TERM", &group, &started])
            .status()
            .expect("runs");
        assert_eq!(
            status.code(),
            Some(3),
            "a group that is gone is reported gone"
        );
    }

    #[test]
    fn an_announcement_is_a_pid_and_an_optional_start_time() {
        assert_eq!(
            parse_announcement("42 1234"),
            Some((42, "1234".to_string()))
        );
        assert_eq!(parse_announcement("42 "), Some((42, String::new())));
        assert_eq!(parse_announcement("42"), Some((42, String::new())));
        for line in ["", "x 1", "42 1 2", "42 1x", "1 5", "-3 5"] {
            assert_eq!(parse_announcement(line), None, "{line:?}");
        }
    }
}
