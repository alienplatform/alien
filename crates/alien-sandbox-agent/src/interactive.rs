//! Interactive commands: a running command a caller can write to, close and signal.
//!
//! The output travels the way `/v1/exec` sends it, as one NDJSON stream that ends in exactly one
//! terminal frame and kills the command if the caller stops reading. Everything flowing the other
//! way is a separate short request naming the command by an id the caller chose, so it works
//! through any proxy that can stream a response, including ones that cannot stream a request.
//!
//! Three rules carry the weight:
//!
//! - **The id is registered before the stream answers.** The caller learns the command started
//!   from the response headers, and its first write may arrive straight after. A registration
//!   made later would refuse that write as naming nothing.
//! - **A write is acknowledged by offset, never by "ok".** A write that waits on a child that is
//!   not reading answers with how far it got rather than holding the request open past a proxy's
//!   timeout, and a retry of the same bytes is applied once. A gap is refused: bytes a caller
//!   thinks it sent that never arrived would otherwise be silently skipped.
//! - **A barrier retires every write sent before it.** A caller whose write was cancelled cannot
//!   know whether that request is still on its way. The barrier waits out any write in progress,
//!   reports how much input the command has taken, and moves the input to a new generation; a
//!   write from an older generation that arrives later is refused rather than applied.
//! - **A signal reaches the process group, and only while the command runs.** The group is
//!   forgotten once the command has exited and before it is reaped, under the lock every signal
//!   holds while it is sent. An exited child keeps its pid until it is reaped, so a signal sent
//!   under that lock can only reach this command, and none is sent after it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use alien_core::sandbox_signal::CommandSignal;
use alien_error::AlienError;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;

use crate::error::{ErrorData, Result};

/// The most interactive commands one session runs at once. The same ceiling as jobs, for the
/// same reason: each holds a process, two pipes and a stream.
const MAX_INTERACTIVE: usize = 16;

/// How long one write may wait for the command to read before answering with a short count.
///
/// Below every proxy's request timeout on the backends that use this, so a command that has
/// stopped reading costs the caller a retry rather than a failed request.
pub const STDIN_WRITE_WAIT: Duration = Duration::from_secs(20);

/// The longest id accepted. Ids are the caller's, so this only bounds what a hostile one costs.
const MAX_ID_LEN: usize = 128;

/// The interactive commands one session is running.
pub struct InteractiveRegistry {
    commands: Mutex<HashMap<String, Arc<Interactive>>>,
    capacity: usize,
}

/// One running interactive command.
pub struct Interactive {
    /// Serializes writers, so offsets apply in order and a retry cannot interleave with the write
    /// it repeats.
    stdin: tokio::sync::Mutex<Stdin>,
    /// The command's process group: set after spawn and before the stream answers, and cleared
    /// to 0 once the command has exited, before it is reaped. Held while a signal is sent, so the
    /// clearing waits for a signal in flight and the reap waits for the clearing.
    process_group: Mutex<i32>,
}

struct Stdin {
    /// `None` once closed, by the caller or by the command no longer reading
    pipe: Option<ChildStdin>,
    /// Bytes the command's pipe has taken so far
    accepted: u64,
    /// Moved on by each barrier. Only writes carrying the current one are applied.
    generation: u64,
}

/// A slot held for one command. Dropping it frees the id, which is what makes a signal after the
/// command ended name nothing.
pub struct Reservation {
    registry: Arc<InteractiveRegistry>,
    id: String,
    command: Arc<Interactive>,
}

impl Reservation {
    /// Hands the command's stdin and process group to the slot. Awaits the stdin lock rather than
    /// trying it: a request naming the id early holds it only briefly, and skipping it would close
    /// the command's input before anyone wrote to it.
    pub async fn attach(&self, stdin: Option<ChildStdin>, process_group: i32) {
        self.command.stdin.lock().await.pipe = stdin;
        *lock(&self.command.process_group) = process_group;
    }

    /// Forgets the process group. Called once the command has exited and before it is reaped;
    /// waits for a signal being sent to finish first.
    pub fn exited(&self) {
        *lock(&self.command.process_group) = 0;
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.registry.lock().remove(&self.id);
    }
}

impl InteractiveRegistry {
    pub fn new() -> Self {
        Self::with_capacity(MAX_INTERACTIVE)
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            commands: Mutex::new(HashMap::new()),
            capacity,
        }
    }

    /// Reserves `id` before anything is spawned, so a full session or a reused id is an error
    /// response rather than a command that starts and is then refused.
    pub fn reserve(self: &Arc<Self>, id: &str) -> Result<Reservation> {
        if id.is_empty()
            || id.len() > MAX_ID_LEN
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(AlienError::new(ErrorData::RequestInvalid {
                reason: format!(
                    "an interactive command id is 1 to {MAX_ID_LEN} characters of [A-Za-z0-9_-]"
                ),
            }));
        }

        let command = Arc::new(Interactive {
            stdin: tokio::sync::Mutex::new(Stdin {
                pipe: None,
                accepted: 0,
                generation: 0,
            }),
            process_group: Mutex::new(0),
        });

        let mut commands = self.lock();
        if commands.contains_key(id) {
            return Err(AlienError::new(ErrorData::RequestInvalid {
                reason: format!("interactive command '{id}' is already running"),
            }));
        }
        if commands.len() >= self.capacity {
            return Err(AlienError::new(ErrorData::InteractiveLimitReached {
                limit: self.capacity,
            }));
        }
        commands.insert(id.to_string(), Arc::clone(&command));

        Ok(Reservation {
            registry: Arc::clone(self),
            id: id.to_string(),
            command,
        })
    }

    /// Writes `data`, which starts at byte `offset` of the command's input, and returns how many
    /// bytes of input the command has taken in total.
    ///
    /// Bytes before what the command already took are acknowledged without being written again,
    /// so a retry is safe. A return short of `offset + data.len()` means the command stopped
    /// reading for `wait`; the caller resends from the returned count. A write from a generation
    /// a barrier has retired is refused.
    pub async fn write(
        &self,
        id: &str,
        generation: u64,
        offset: u64,
        data: &[u8],
        wait: Duration,
    ) -> Result<u64> {
        let command = self.get(id)?;
        let mut stdin = command.stdin.lock().await;
        let Stdin {
            pipe,
            accepted,
            generation: current,
        } = &mut *stdin;

        if generation != *current {
            return Err(AlienError::new(ErrorData::StdinStaleWrite {
                generation,
                current: *current,
            }));
        }

        if offset > *accepted {
            return Err(AlienError::new(ErrorData::StdinOffsetMismatch {
                accepted: *accepted,
                offset,
            }));
        }
        let already = usize::try_from(*accepted - offset).unwrap_or(usize::MAX);
        if already >= data.len() {
            return Ok(*accepted);
        }
        let Some(writer) = pipe.as_mut() else {
            return Err(AlienError::new(ErrorData::StdinClosed {
                id: id.to_string(),
            }));
        };

        let deadline = tokio::time::Instant::now() + wait;
        let mut rest = &data[already..];
        while !rest.is_empty() {
            // One `write` at a time, never `write_all`: a pipe write that has not completed has
            // written nothing, so cancelling at the deadline loses no byte the count does not
            // already include.
            match tokio::time::timeout_at(deadline, writer.write(rest)).await {
                Err(_) => break,
                Ok(Ok(0)) => {
                    *pipe = None;
                    return Err(AlienError::new(ErrorData::StdinClosed {
                        id: id.to_string(),
                    }));
                }
                Ok(Ok(written)) => {
                    *accepted += written as u64;
                    rest = &rest[written..];
                }
                Ok(Err(error)) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                    *pipe = None;
                    return Err(AlienError::new(ErrorData::StdinClosed {
                        id: id.to_string(),
                    }));
                }
                Ok(Err(error)) => {
                    return Err(AlienError::new(ErrorData::OperationFailed {
                        operation: "write command stdin".to_string(),
                        reason: error.to_string(),
                    }));
                }
            }
        }

        Ok(*accepted)
    }

    /// Retires every write sent so far: waits for one in progress, then moves the input to
    /// generation `target` unless it is already there or past it. Returns how many bytes the
    /// command has taken and the current generation.
    ///
    /// The caller names the generation rather than asking for the next one, so a barrier sent
    /// twice, because its first answer was lost, moves the input once.
    pub async fn barrier(&self, id: &str, target: u64) -> Result<(u64, u64)> {
        let command = self.get(id)?;
        let mut stdin = command.stdin.lock().await;
        stdin.generation = stdin.generation.max(target);
        Ok((stdin.accepted, stdin.generation))
    }

    /// Closes the command's stdin, so a program reading to end of input finishes. Closing twice
    /// is not an error: the second close asks for what is already true.
    pub async fn close_stdin(&self, id: &str) -> Result<()> {
        let command = self.get(id)?;
        let mut stdin = command.stdin.lock().await;
        if let Some(mut pipe) = stdin.pipe.take() {
            // A flush that fails means the command already stopped reading, which is what the
            // close was for.
            let _ = pipe.shutdown().await;
        }
        Ok(())
    }

    /// Sends `signal` to the command's process group.
    pub fn signal(&self, id: &str, signal: CommandSignal) -> Result<()> {
        let command = self.get(id)?;
        // Held until the signal is sent. The group is cleared under this lock once the command has
        // exited and before it is reaped, so a group read here is still this command's: a 0 is a
        // command that has ended even if its terminal frame has not reached the caller yet.
        let held = lock(&command.process_group);
        let group = *held;
        if group <= 0 {
            return Err(AlienError::new(ErrorData::CommandNotRunning {
                id: id.to_string(),
            }));
        }

        #[cfg(unix)]
        {
            let number = match signal {
                CommandSignal::Interrupt => libc::SIGINT,
                CommandSignal::Terminate => libc::SIGTERM,
                CommandSignal::Kill => libc::SIGKILL,
                CommandSignal::Hangup => libc::SIGHUP,
                CommandSignal::Quit => libc::SIGQUIT,
                CommandSignal::User1 => libc::SIGUSR1,
                CommandSignal::User2 => libc::SIGUSR2,
            };
            // SAFETY: `kill(2)` with a negative pid signals a process group. The group is the
            // command's own, established at spawn, and `held` keeps it from being cleared, and so
            // the command from being reaped, until this returns.
            if unsafe { libc::kill(-group, number) } != 0 {
                let error = std::io::Error::last_os_error();
                // The group is already gone: everything in it exited between the lookup and the
                // signal, and the terminal frame is on its way.
                if error.raw_os_error() == Some(libc::ESRCH) {
                    return Err(AlienError::new(ErrorData::CommandNotRunning {
                        id: id.to_string(),
                    }));
                }
                return Err(AlienError::new(ErrorData::OperationFailed {
                    operation: "signal command".to_string(),
                    reason: error.to_string(),
                }));
            }
        }
        #[cfg(not(unix))]
        let _ = (group, signal);

        drop(held);
        Ok(())
    }

    /// How many commands are running.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get(&self, id: &str) -> Result<Arc<Interactive>> {
        self.lock()
            .get(id)
            .cloned()
            .ok_or_else(|| AlienError::new(ErrorData::CommandNotRunning { id: id.to_string() }))
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<Interactive>>> {
        // A panic while holding this lock leaves at worst a stale id, which refuses one write.
        self.commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Locks a command's process group. A panic while holding it leaves a number that is either the
/// command's or 0, both of which are still true.
fn lock(group: &Mutex<i32>) -> MutexGuard<'_, i32> {
    group
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Default for InteractiveRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading_nothing() -> tokio::process::Child {
        tokio::process::Command::new("/bin/sleep")
            .arg("30")
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawns")
    }

    /// Backpressure, end to end at the pipe: a command that is not reading takes what its pipe
    /// buffer holds and no more, and the write answers with that count once its wait is up rather
    /// than buffering the rest in the agent or holding the request open.
    #[tokio::test]
    async fn a_write_to_a_command_that_is_not_reading_answers_short() {
        let registry = Arc::new(InteractiveRegistry::new());
        let mut child = reading_nothing();
        let reservation = registry.reserve("slow").expect("reserves");
        reservation
            .attach(child.stdin.take(), child.id().unwrap() as i32)
            .await;

        let data = vec![0u8; 4 << 20];
        let started = std::time::Instant::now();
        let accepted = registry
            .write("slow", 0, 0, &data, Duration::from_millis(300))
            .await
            .expect("a short write is not an error");

        assert!(accepted > 0, "the pipe buffer takes something");
        assert!(
            accepted < data.len() as u64,
            "a command that is not reading cannot have taken all {} bytes, took {accepted}",
            data.len()
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the write answers at its wait"
        );

        // The count is exact: resending from it applies nothing twice.
        let again = registry
            .write(
                "slow",
                0,
                accepted,
                &data[accepted as usize..],
                Duration::from_millis(100),
            )
            .await
            .expect("resend");
        assert_eq!(
            again, accepted,
            "still not reading, so nothing more was taken"
        );
    }

    #[tokio::test]
    async fn an_id_is_reserved_once_and_released_with_its_reservation() {
        let registry = Arc::new(InteractiveRegistry::with_capacity(1));
        let reservation = registry.reserve("a").expect("reserves");
        assert!(registry.reserve("a").is_err(), "a running id is not reused");
        assert!(
            registry.reserve("b").is_err(),
            "a full session refuses another command"
        );
        drop(reservation);
        assert!(
            registry.reserve("a").is_ok(),
            "a released id can be used again"
        );
    }

    #[test]
    fn an_id_outside_the_alphabet_is_refused() {
        let registry = Arc::new(InteractiveRegistry::new());
        for id in ["", "a/b", "a b", &"x".repeat(MAX_ID_LEN + 1)] {
            assert!(registry.reserve(id).is_err(), "{id:?}");
        }
    }
}
