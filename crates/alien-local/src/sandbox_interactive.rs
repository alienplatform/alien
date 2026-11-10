//! Interactive commands in local sandbox sessions, on the sandbox agent's wire.
//!
//! The binding drives these exactly as it drives the in-sandbox agent: one NDJSON output stream
//! per command, and short requests for input, close and signals naming the command by the id the
//! caller chose. The difference is who supervises. Nothing Alien runs inside a local session, so
//! the manager holds the exec's attached stdin and enforces the deadline from outside, where the
//! command cannot reach the supervisor.
//!
//! Signals go through a shell kept running in each container, started before the container's
//! first command and using only builtins, so a command that takes every process slot in the
//! container cannot keep it from being stopped. It runs as root, which in a session container
//! holds `CAP_KILL` and nothing else, so it can signal the session's user while the command,
//! running as that user with no capability at all, cannot signal it. A command that still cannot
//! be stopped from inside, as when a child it moved to a session of its own holds its output
//! open, has its whole session killed from the host, which runs nothing in the container.
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
use tracing::warn;

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

/// The signal shell kept running in each container. Each line it reads is `SIGNAL GROUP START`
/// (the start time may be empty), and it answers each with one line: 0 when the group was
/// signalled, 3 when the group no longer exists or what has its number is not the command (its
/// leader's start time differs), 2 when the signal failed. It uses only builtins, so it starts no
/// process: a command that has taken every process slot in the container cannot keep it from
/// killing that command. It runs as root, holding only `CAP_KILL`, so the command cannot kill it
/// and it can signal the session's user. `kill -SIG -N` is the one spelling dash and busybox
/// both take.
const SIGNAL_SHELL: &str = "while read -r g n t; do r=0; \
     if [ -n \"$t\" ] && read -r s 2>/dev/null < \"/proc/$n/stat\"; then \
     set -- ${s##*) }; [ \"${20}\" = \"$t\" ] || r=3; fi; \
     if [ \"$r\" = 0 ] && ! kill -\"$g\" -\"$n\" 2>/dev/null; then \
     if kill -0 -\"$n\" 2>/dev/null; then r=2; else r=3; fi; fi; \
     echo \"$r\"; done";

/// How long the signal shell may take to answer one request before it is replaced.
const SIGNAL_ANSWER_WAIT: Duration = Duration::from_secs(5);

/// How long the start waits for the wrapper to announce the pid.
const ANNOUNCEMENT_WAIT: Duration = Duration::from_secs(10);

/// How long the kill of a command's group may take to run, and then to end its output, before
/// its session is killed from the host instead.
const KILL_SETTLE: Duration = Duration::from_secs(5);

/// How long to wait before asking Docker again to kill a session it refused to kill.
const KILL_RETRY: Duration = Duration::from_secs(5);

/// What the terminal frame says when a command was ended by killing its whole session.
const SESSION_STOPPED: &str = "it could not be stopped inside its sandbox session, so the whole \
     session was stopped: every command in it has ended, and the session will not run again";

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
    signal_shells: Arc<SignalShells>,
}

/// The signal shell of each container, by container id. One is started before a container's
/// first command, and again whenever the last one has ended, so a running command always has
/// one that was started while a process slot was free.
#[derive(Default)]
struct SignalShells {
    by_container: tokio::sync::Mutex<HashMap<String, Arc<SignalShell>>>,
}

/// One container's signal shell. `None` once it has ended or stopped answering in step.
struct SignalShell {
    io: tokio::sync::Mutex<Option<SignalShellIo>>,
}

struct SignalShellIo {
    input: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
    output: Output,
    /// stdout read past the last answer
    stdout: Vec<u8>,
}

impl SignalShells {
    /// The container's signal shell, starting one if it has none that is still running.
    async fn get(
        &self,
        manager: &LocalSandboxManager,
        container_id: &str,
    ) -> Result<Arc<SignalShell>, ControlError> {
        let mut shells = self.by_container.lock().await;
        // Forget shells that have ended, so removed containers do not pile up. One in use is
        // kept: its request ends within `SIGNAL_ANSWER_WAIT` either way.
        shells.retain(|_, shell| !matches!(shell.io.try_lock(), Ok(io) if io.is_none()));
        if let Some(shell) = shells.get(container_id) {
            return Ok(Arc::clone(shell));
        }
        let InteractiveExec { input, output, .. } = manager
            .exec_signal_shell(
                container_id,
                &["sh".to_string(), "-c".to_string(), SIGNAL_SHELL.to_string()],
            )
            .await
            .map_err(|error| {
                ControlError::Failed(format!("starting the container's signal shell: {error}"))
            })?;
        let shell = Arc::new(SignalShell {
            io: tokio::sync::Mutex::new(Some(SignalShellIo {
                input,
                output,
                stdout: Vec::new(),
            })),
        });
        shells.insert(container_id.to_string(), Arc::clone(&shell));
        Ok(shell)
    }
}

impl SignalShell {
    /// Sends `signal` to `process_group` unless its leader's start time is not `started`.
    async fn signal(
        &self,
        signal: &str,
        process_group: i64,
        started: &str,
    ) -> Result<(), ControlError> {
        let mut io = self.io.lock().await;
        // Taken out while the request is in flight, and put back only once its answer is read. A
        // call cancelled between the two drops the shell, which ends it, so its late answer is
        // never read as the next request's.
        let Some(mut shell) = io.take() else {
            return Err(ControlError::Failed(
                "the container's signal shell has ended".to_string(),
            ));
        };
        let request = format!("{signal} {process_group} {started}\n");
        let answer = match tokio::time::timeout(SIGNAL_ANSWER_WAIT, shell.ask(&request)).await {
            Ok(Ok(answer)) => answer,
            // Dropping its input ends the shell; the next request starts another.
            Ok(Err(reason)) => return Err(ControlError::Failed(reason)),
            Err(_) => {
                return Err(ControlError::Failed(format!(
                    "the container's signal shell did not answer within {}s",
                    SIGNAL_ANSWER_WAIT.as_secs()
                )))
            }
        };
        *io = Some(shell);
        match answer.as_str() {
            "0" => Ok(()),
            "3" => Err(ControlError::NotRunning),
            other => Err(ControlError::Failed(format!(
                "signalling the command's process group answered {other:?}"
            ))),
        }
    }
}

impl SignalShellIo {
    /// Writes one request and reads the line that answers it.
    async fn ask(&mut self, request: &str) -> Result<String, String> {
        self.input
            .write_all(request.as_bytes())
            .await
            .map_err(|error| format!("writing to the container's signal shell: {error}"))?;
        self.input
            .flush()
            .await
            .map_err(|error| format!("writing to the container's signal shell: {error}"))?;
        loop {
            if let Some(end) = self.stdout.iter().position(|byte| *byte == b'\n') {
                let line = String::from_utf8_lossy(&self.stdout[..end]).into_owned();
                self.stdout.drain(..=end);
                return Ok(line);
            }
            match self.output.next().await {
                None => return Err("the container's signal shell has ended".to_string()),
                Some(Err(error)) => {
                    return Err(format!("reading the container's signal shell: {error}"))
                }
                Some(Ok(bollard::container::LogOutput::StdOut { message })) => {
                    self.stdout.extend_from_slice(&message);
                }
                Some(Ok(_)) => {}
            }
        }
    }
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
    signal_shells: Arc<SignalShells>,
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

        // The command may take every process slot in the container, after which nothing new can
        // start there to stop it. Its signal shell is running before it is.
        self.signal_shells.get(&manager, &container_id).await?;

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

        let mut heard = Heard::default();
        let announced =
            tokio::time::timeout(ANNOUNCEMENT_WAIT, announcement(&mut output, &mut heard)).await;
        let ((process_group, started), pending) = match announced {
            Ok(Ok(announced)) => announced,
            Ok(Err(reason)) => {
                // Docker has started the exec, so the command may be running even though its
                // wrapper did not say so as written. The id stays held until its output ends.
                drop(input);
                tokio::spawn(outlast(output, reservation));
                return Err(ControlError::Failed(reason));
            }
            Err(_) => {
                // The announcement may still come. Until the command is known to be gone, it is
                // stopped as soon as its group is named, and its id stays held.
                drop(input);
                tokio::spawn(abandon(
                    manager,
                    container_id,
                    Arc::clone(&self.signal_shells),
                    output,
                    heard,
                    reservation,
                ));
                return Err(ControlError::Failed(format!(
                    "the command wrapper did not report a pid within {}s",
                    ANNOUNCEMENT_WAIT.as_secs()
                )));
            }
        };

        let command = Arc::new(Command {
            container_id,
            signal_shells: Arc::clone(&self.signal_shells),
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

    /// Signals the command's process group through the container's signal shell, which runs in
    /// the same container as the same user, so it can reach nothing the command could not reach
    /// itself.
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
/// What has arrived ahead of the announcement, kept across a timed-out wait so the reading can
/// go on where it stopped.
#[derive(Default)]
struct Heard {
    stderr: Vec<u8>,
    early: Early,
}

/// Reads the wrapper's line off the front of stderr, returning the pid, the start time and
/// whatever output arrived around it. The caller bounds the wait; a wait cut short leaves what
/// was read in `heard`.
async fn announcement(
    output: &mut Output,
    heard: &mut Heard,
) -> Result<((i64, String), Early), String> {
    loop {
        match output.next().await {
            None => return Err("the command wrapper ended before reporting a pid".to_string()),
            Some(Err(error)) => return Err(format!("reading the command wrapper: {error}")),
            Some(Ok(bollard::container::LogOutput::StdOut { message })) => {
                heard.early.push((true, message.to_vec()));
            }
            Some(Ok(bollard::container::LogOutput::StdErr { message })) => {
                heard.stderr.extend_from_slice(&message);
                if let Some(end) = heard.stderr.iter().position(|byte| *byte == b'\n') {
                    let line = String::from_utf8_lossy(&heard.stderr[..end]).into_owned();
                    let announced = parse_announcement(&line).ok_or_else(|| {
                        format!("the command wrapper did not report a pid: {line:?}")
                    })?;
                    let rest = heard.stderr[end + 1..].to_vec();
                    let mut early = std::mem::take(&mut heard.early);
                    if !rest.is_empty() {
                        early.push((false, rest));
                    }
                    return Ok((announced, early));
                }
                if heard.stderr.len() > MAX_ANNOUNCEMENT {
                    return Err("the command wrapper did not report a pid".to_string());
                }
            }
            Some(Ok(_)) => {}
        }
    }
}

/// Holds the id of a command whose start was answered as failed until its output ends, which is
/// Docker saying the exec is gone. Nothing names its group, so nothing can stop it sooner.
async fn outlast(mut output: Output, reservation: Reservation) {
    while output.next().await.is_some() {}
    drop(reservation);
}

/// A start that timed out waiting for the announcement: the command may be running anyway. If
/// the announcement still comes, its group is stopped; the id is released only once the output
/// has ended.
async fn abandon(
    manager: Arc<LocalSandboxManager>,
    container_id: String,
    signal_shells: Arc<SignalShells>,
    mut output: Output,
    mut heard: Heard,
    reservation: Reservation,
) {
    let (process_group, started) = match announcement(&mut output, &mut heard).await {
        Ok((announced, _)) => announced,
        Err(_) => return outlast(output, reservation).await,
    };
    let command = Command {
        container_id,
        signal_shells,
        stdin: tokio::sync::Mutex::new(Stdin {
            pipe: None,
            accepted: 0,
            generation: 0,
        }),
        process_group,
        started,
    };
    stop(&manager, &command, &mut output).await;
    drop(reservation);
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
    command
        .signal_shells
        .get(manager, &command.container_id)
        .await?
        .signal(signal, command.process_group, &command.started)
        .await
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
        Ending::Exited => Some(match manager.exec_exit_code(&exec_id).await {
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
        }),
        Ending::Departed => {
            stop(&manager, &command, &mut output).await;
            None
        }
        Ending::TimedOut => {
            let exceeded = format!("exceeded its {}ms timeout", timeout.as_millis());
            Some(Frame::Error {
                code: "timeoutExceeded".to_string(),
                message: match stop(&manager, &command, &mut output).await {
                    Stopped::Command => exceeded,
                    Stopped::Session => format!("{exceeded}; {SESSION_STOPPED}"),
                },
            })
        }
        Ending::Failed(reason) => Some(Frame::Error {
            code: "outputReadFailed".to_string(),
            message: match stop(&manager, &command, &mut output).await {
                Stopped::Command => reason,
                Stopped::Session => format!("{reason}; {SESSION_STOPPED}"),
            },
        }),
    };

    // The command is gone: release the id before the terminal frame, which a slow reader can
    // hold off indefinitely.
    drop(reservation);
    if let Some(terminal) = terminal {
        let _ = frames.send(terminal).await;
    }
}

/// How a command was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stopped {
    /// Its group was killed from inside the container, and its output ended
    Command,
    /// It could not be stopped from inside, so its whole session was killed from the host
    Session,
}

/// Stops the command, and returns only once nothing of it can still run.
///
/// First its group is killed from inside the container, by the signal shell. If that kill
/// cannot run, or the output is still open `KILL_SETTLE` after it (a child moved to a session of
/// its own can hold it), the session's container is killed from the host. That runs nothing
/// inside the container and ends every process there, so a command cannot defeat it. A kill
/// Docker refuses is asked again every `KILL_RETRY`, with the command's id still held.
///
/// The output is drained while the kill runs, not after: an unread exec holds Docker's copy of
/// its output, and until that copy moves, the container's other execs (the kill among them) and
/// its exit can stall behind it.
async fn stop(manager: &LocalSandboxManager, command: &Command, output: &mut Output) -> Stopped {
    let drain = async { while output.next().await.is_some() {} };
    tokio::pin!(drain);
    let kill = tokio::time::timeout(KILL_SETTLE, kill_group(manager, command, "KILL"));
    tokio::pin!(kill);
    // The kill always runs to its answer: output that ends (a dropped attach connection, say)
    // does not show that the command ended.
    let mut drained = false;
    let killed = loop {
        tokio::select! {
            killed = &mut kill => break killed,
            () = &mut drain, if !drained => drained = true,
        }
    };
    // Gone already counts: the group no longer exists, though its output may still be open.
    let signalled = matches!(killed, Ok(Ok(())) | Ok(Err(ControlError::NotRunning)));
    if signalled && (drained || tokio::time::timeout(KILL_SETTLE, &mut drain).await.is_ok()) {
        return Stopped::Command;
    }

    warn!(
        container_id = %command.container_id,
        process_group = command.process_group,
        "An interactive command could not be stopped inside its session; killing the session"
    );
    while let Err(error) = manager.kill_session(&command.container_id).await {
        warn!(
            container_id = %command.container_id,
            error = %error,
            "Docker refused to kill the session of an interactive command that would not stop; \
             asking again"
        );
        tokio::time::sleep(KILL_RETRY).await;
    }
    // Nothing in the container runs now. Its output ends as Docker notices; past the wait it is
    // dropped rather than waited on.
    if !drained {
        let _ = tokio::time::timeout(KILL_SETTLE, &mut drain).await;
    }
    Stopped::Session
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

    /// The signal shell as the manager runs it, reading requests on stdin and answering on
    /// stdout. `nproc`, when set, caps the processes its user may run, so it can start none.
    struct SignalShellProcess {
        child: std::process::Child,
        answers: std::io::BufReader<std::process::ChildStdout>,
    }

    impl SignalShellProcess {
        fn start(nproc: Option<libc::rlim_t>) -> Self {
            let mut command = sh(SIGNAL_SHELL, &[]);
            command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped());
            if let Some(limit) = nproc {
                limit_processes(&mut command, limit);
            }
            let mut child = command.spawn().expect("spawns");
            let answers = std::io::BufReader::new(child.stdout.take().expect("piped"));
            Self { child, answers }
        }

        fn ask(&mut self, signal: &str, group: &str, started: &str) -> String {
            use std::io::{BufRead, Write};

            let stdin = self.child.stdin.as_mut().expect("piped");
            writeln!(stdin, "{signal} {group} {started}").expect("writes");
            stdin.flush().expect("flushes");
            let mut line = String::new();
            self.answers.read_line(&mut line).expect("reads an answer");
            line.trim_end().to_string()
        }
    }

    impl Drop for SignalShellProcess {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Caps the processes the child's user may run at `limit`, from inside the child before it
    /// runs the program, so the cap binds it and nothing else.
    fn limit_processes(command: &mut std::process::Command, limit: libc::rlim_t) {
        use std::os::unix::process::CommandExt;

        // SAFETY: setrlimit is async-signal-safe and touches only the forked child.
        unsafe {
            command.pre_exec(move || {
                let cap = libc::rlimit {
                    rlim_cur: limit,
                    rlim_max: limit,
                };
                if libc::setrlimit(libc::RLIMIT_NPROC, &cap) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }

    /// The check that keeps a signal off a process that took a finished command's number: a
    /// leader with the right pid but another start time is left alone.
    #[test]
    fn a_signal_reaches_the_group_only_while_its_leader_is_the_command() {
        let (mut child, pid, started) = announced_leader();
        let group = pid.to_string();
        let mut shell = SignalShellProcess::start(None);

        let other = (started.parse::<u64>().expect("a number") + 1).to_string();
        assert_eq!(
            shell.ask("TERM", &group, &other),
            "3",
            "another start time is another process"
        );
        assert!(
            child.try_wait().expect("waits").is_none(),
            "and it was not signalled"
        );

        assert_eq!(
            shell.ask("TERM", &group, &started),
            "0",
            "the command's own start time"
        );
        let ended = child.wait().expect("waits");
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&ended),
            Some(libc::SIGTERM)
        );

        assert_eq!(
            shell.ask("TERM", &group, &started),
            "3",
            "a group that is gone is reported gone"
        );
    }

    /// A command can take every process slot in its container. The signal shell was started
    /// before it, and must still kill it without starting a process of its own.
    #[test]
    fn the_signal_shell_kills_a_command_while_no_process_can_start() {
        // A cap of one process for the user is below what the user already runs, so every fork
        // under it fails. Root is exempt, which would make this test prove nothing: the probe
        // below fails it instead.
        let mut probe = sh("( : ) && echo forked", &[]);
        probe.stdout(std::process::Stdio::piped());
        limit_processes(&mut probe, 1);
        let probed = probe.output().expect("runs");
        assert!(
            probed.stdout.is_empty(),
            "a shell under the cap must not be able to fork (is the test running as root?)"
        );

        let (mut child, pid, started) = announced_leader();
        let group = pid.to_string();
        let mut shell = SignalShellProcess::start(Some(1));

        assert_eq!(shell.ask("KILL", &group, &started), "0", "killed");
        let ended = child.wait().expect("waits");
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&ended),
            Some(libc::SIGKILL)
        );
        assert_eq!(
            shell.ask("KILL", &group, &started),
            "3",
            "and then reported gone"
        );
        assert_eq!(
            shell.ask("KILL", &group, ""),
            "3",
            "with no start time to check, too"
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
