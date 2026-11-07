//! Interactive commands in local sandbox sessions, on the sandbox agent's wire.
//!
//! The binding drives these exactly as it drives the in-sandbox agent: one NDJSON output stream
//! per command, and short requests for input, close and signals naming the command by the id the
//! caller chose. The difference is who supervises. Nothing Alien runs inside a local session, so
//! the manager holds the exec's attached stdin and enforces the deadline from outside, where the
//! command cannot reach the supervisor.
//!
//! Signals need the command's process group, and Docker reports only a host pid. The command is
//! started under a one-line shell that writes its own pid to stderr and then `exec`s the command,
//! so that pid is the command's. A `docker exec` process already leads its own process group and
//! session, so the pid is also the group. The line is written before the command exists, which
//! makes it the first bytes on stderr with nothing able to race it, and it is removed before any
//! output reaches the caller.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use futures::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::sandbox_manager::{InteractiveExec, LocalSandboxManager};

/// The most interactive commands one sandbox runs at once, matching the agent.
const MAX_INTERACTIVE: usize = 16;

/// How long one write may wait for the command to read before answering with a short count,
/// matching the agent.
pub const STDIN_WRITE_WAIT: Duration = Duration::from_secs(20);

/// How many frames may wait between Docker and the caller: the backpressure window.
const FRAME_CHANNEL_DEPTH: usize = 16;

/// The longest the announcement line may be. A pid is a few digits; anything longer means the
/// wrapper did not run as written, and the command is refused rather than guessed at.
const MAX_ANNOUNCEMENT: usize = 32;

/// How long to wait for the output to end after the group was killed at its deadline.
const KILL_SETTLE: Duration = Duration::from_secs(10);

/// One frame, serialized exactly as the agent writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "t")]
pub enum Frame {
    Stdout { seq: u64, data: String },
    Stderr { seq: u64, data: String },
    Exit { code: i32, truncated: bool },
    Error { code: String, message: String },
}

/// Why a control request was refused, and the status it answers with.
#[derive(Debug)]
pub enum ControlError {
    /// No command by that id is running: 404
    NotRunning,
    /// stdin is closed, or a write would leave a gap: 409
    Conflict(String),
    /// The request itself is malformed: 400
    Invalid(String),
    /// Every slot is taken: 429
    Full,
    /// Docker failed: 502
    Failed(String),
}

/// The interactive commands one local sandbox is running.
#[derive(Default)]
pub struct InteractiveRegistry {
    commands: Mutex<HashMap<String, Arc<Command>>>,
}

struct Command {
    container_id: String,
    stdin: tokio::sync::Mutex<Stdin>,
    process_group: OnceLock<i64>,
}

struct Stdin {
    pipe: Option<std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>>,
    accepted: u64,
}

/// A held id, released when dropped: after that no signal can name the command.
struct Reservation {
    registry: Arc<InteractiveRegistry>,
    id: String,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.registry.lock().remove(&self.id);
    }
}

/// What a start needs besides the id.
pub struct StartRequest {
    pub id: String,
    pub command: Vec<String>,
    pub timeout: Duration,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

impl InteractiveRegistry {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<Command>>> {
        self.commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn get(&self, id: &str) -> Result<Arc<Command>, ControlError> {
        self.lock().get(id).cloned().ok_or(ControlError::NotRunning)
    }

    /// Starts a command and returns the receiver its frames arrive on.
    ///
    /// The id is registered before this returns, so the caller's first write cannot race it.
    pub async fn start(
        self: &Arc<Self>,
        manager: Arc<LocalSandboxManager>,
        container_id: String,
        request: StartRequest,
    ) -> Result<mpsc::Receiver<Frame>, ControlError> {
        validate(&request)?;

        let command = Arc::new(Command {
            container_id: container_id.clone(),
            stdin: tokio::sync::Mutex::new(Stdin {
                pipe: None,
                accepted: 0,
            }),
            process_group: OnceLock::new(),
        });
        {
            let mut commands = self.lock();
            if commands.contains_key(&request.id) {
                return Err(ControlError::Invalid(format!(
                    "interactive command '{}' is already running",
                    request.id
                )));
            }
            if commands.len() >= MAX_INTERACTIVE {
                return Err(ControlError::Full);
            }
            commands.insert(request.id.clone(), Arc::clone(&command));
        }
        let reservation = Reservation {
            registry: Arc::clone(self),
            id: request.id.clone(),
        };

        let mut argv = vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf '%s\\n' \"$$\" >&2; exec \"$@\"".to_string(),
            "sh".to_string(),
        ];
        argv.extend(request.command.iter().cloned());

        let InteractiveExec {
            exec_id,
            input,
            output,
        } = manager
            .exec_interactive(&container_id, &argv, request.cwd.as_deref(), &request.env)
            .await
            .map_err(|error| ControlError::Failed(error.to_string()))?;
        command
            .stdin
            .try_lock()
            .expect("nothing can reach a command before it is started")
            .pipe = Some(input);

        let (frames, receiver) = mpsc::channel(FRAME_CHANNEL_DEPTH);
        tokio::spawn(pump(
            manager,
            command,
            reservation,
            exec_id,
            output,
            request.timeout,
            frames,
        ));
        Ok(receiver)
    }

    /// Writes input starting at `offset`, returning how many bytes the command has taken in all.
    pub async fn write(&self, id: &str, offset: u64, data: &[u8]) -> Result<u64, ControlError> {
        let command = self.get(id)?;
        let mut stdin = command.stdin.lock().await;
        let Stdin { pipe, accepted } = &mut *stdin;

        if offset > *accepted {
            return Err(ControlError::Conflict(format!(
                "the command has taken {accepted} bytes of input; a write at offset {offset} would leave a gap"
            )));
        }
        let already = usize::try_from(*accepted - offset).unwrap_or(usize::MAX);
        if already >= data.len() {
            return Ok(*accepted);
        }
        let Some(writer) = pipe.as_mut() else {
            return Err(ControlError::Conflict(format!(
                "interactive command '{id}' no longer reads its input"
            )));
        };

        let deadline = tokio::time::Instant::now() + STDIN_WRITE_WAIT;
        let mut rest = &data[already..];
        while !rest.is_empty() {
            match tokio::time::timeout_at(deadline, writer.write(rest)).await {
                Err(_) => break,
                Ok(Ok(0)) | Ok(Err(_)) => {
                    *pipe = None;
                    return Err(ControlError::Conflict(format!(
                        "interactive command '{id}' no longer reads its input"
                    )));
                }
                Ok(Ok(written)) => {
                    *accepted += written as u64;
                    rest = &rest[written..];
                }
            }
        }
        // Flushed so the bytes leave this process now rather than when a buffer fills.
        if writer.flush().await.is_err() {
            *pipe = None;
        }
        Ok(*accepted)
    }

    /// Closes stdin. Closing twice is not an error.
    pub async fn close_stdin(&self, id: &str) -> Result<(), ControlError> {
        let command = self.get(id)?;
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
        id: &str,
        signal: alien_core::sandbox_signal::CommandSignal,
    ) -> Result<(), ControlError> {
        let command = self.get(id)?;
        let Some(group) = command.process_group.get().copied() else {
            return Err(ControlError::Conflict(
                "the command has not reported its process group yet".to_string(),
            ));
        };
        kill_group(manager, &command.container_id, group, signal.posix_name()).await
    }

    /// How many commands are running.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
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

async fn kill_group(
    manager: &LocalSandboxManager,
    container_id: &str,
    group: i64,
    signal: &str,
) -> Result<(), ControlError> {
    let result = manager
        .exec(
            container_id,
            &[
                "sh".to_string(),
                "-c".to_string(),
                "kill -s \"$1\" -- \"-$2\"".to_string(),
                "sh".to_string(),
                signal.to_string(),
                group.to_string(),
            ],
        )
        .await
        .map_err(|error| ControlError::Failed(error.to_string()))?;
    match result.exit_code {
        0 => Ok(()),
        // `kill` found no such group: everything in it has exited.
        _ => Err(ControlError::NotRunning),
    }
}

/// Moves Docker's output to the caller until the command ends, enforcing its deadline.
async fn pump(
    manager: Arc<LocalSandboxManager>,
    command: Arc<Command>,
    reservation: Reservation,
    exec_id: String,
    mut output: std::pin::Pin<
        Box<
            dyn futures::Stream<
                    Item = std::result::Result<
                        bollard::container::LogOutput,
                        bollard::errors::Error,
                    >,
                > + Send,
        >,
    >,
    timeout: Duration,
    frames: mpsc::Sender<Frame>,
) {
    let mut seq = 0u64;
    let mut announcement: Option<Vec<u8>> = Some(Vec::new());
    let deadline = tokio::time::Instant::now() + timeout;

    enum Ending {
        Exited,
        Departed,
        TimedOut,
        Failed(String),
    }

    let ending = loop {
        let item = tokio::select! {
            biased;
            () = frames.closed() => break Ending::Departed,
            () = tokio::time::sleep_until(deadline) => break Ending::TimedOut,
            item = output.next() => item,
        };
        let (stream_is_stdout, mut bytes) = match item {
            None => break Ending::Exited,
            Some(Err(error)) => break Ending::Failed(error.to_string()),
            Some(Ok(bollard::container::LogOutput::StdOut { message })) => (true, message.to_vec()),
            Some(Ok(bollard::container::LogOutput::StdErr { message })) => {
                (false, message.to_vec())
            }
            Some(Ok(_)) => continue,
        };

        // The first stderr line is the wrapper's, naming the command's pid. Held back until whole.
        if !stream_is_stdout {
            if let Some(mut pending) = announcement.take() {
                pending.extend_from_slice(&bytes);
                match pending.iter().position(|byte| *byte == b'\n') {
                    Some(end) => {
                        let line = String::from_utf8_lossy(&pending[..end]).into_owned();
                        let Ok(group) = line.trim().parse::<i64>() else {
                            break Ending::Failed(format!(
                                "the command wrapper did not report a pid: {line:?}"
                            ));
                        };
                        let _ = command.process_group.set(group);
                        bytes = pending[end + 1..].to_vec();
                    }
                    None if pending.len() > MAX_ANNOUNCEMENT => {
                        break Ending::Failed(
                            "the command wrapper did not report a pid".to_string(),
                        );
                    }
                    None => {
                        announcement = Some(pending);
                        continue;
                    }
                }
            }
        }
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
        if frames.send(frame).await.is_err() {
            break Ending::Departed;
        }
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

    let _ = frames.send(terminal).await;
    drop(reservation);
}

/// Kills the command's group and waits for its output to end, which is Docker saying the exec's
/// process is gone. Returns whether that was confirmed.
async fn stop(
    manager: &LocalSandboxManager,
    command: &Command,
    output: &mut std::pin::Pin<
        Box<
            dyn futures::Stream<
                    Item = std::result::Result<
                        bollard::container::LogOutput,
                        bollard::errors::Error,
                    >,
                > + Send,
        >,
    >,
) -> bool {
    let Some(group) = command.process_group.get().copied() else {
        // No pid yet means the wrapper has not run; closing stdin and the stream is all there is.
        return false;
    };
    let _ = kill_group(manager, &command.container_id, group, "KILL").await;
    tokio::time::timeout(KILL_SETTLE, async {
        while output.next().await.is_some() {}
    })
    .await
    .is_ok()
}
