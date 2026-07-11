//! The spawned user-application process, with a per-OS die-with-parent backstop.
//!
//! The operator must never let the workload outlive it: an operator self-update
//! handoff (or any operator exit/crash) otherwise orphans the app, and a second
//! copy then starts under the swapped-in operator. The backstop differs per OS,
//! and this wrapper hides that behind one API so `runtime.rs` handles a single
//! child type:
//!
//! - **Linux:** `PR_SET_PDEATHSIG=SIGKILL` on the child (set on the `Command`
//!   before spawn) kills it the instant the operator dies, plus `kill_on_drop`.
//! - **Windows:** the child runs in its own Job Object with
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (via `command-group`, exactly as the
//!   launcher supervises the operator). When the operator exits or crashes, its
//!   Job handle closes and the OS terminates the app. `kill_on_drop`
//!   (TerminateProcess on handle drop) is only a partial cover — it does nothing
//!   on a hard crash — so the Job Object is the robust mechanism. This is a
//!   nested Job (the app's Job inside the launcher's operator Job); fine on
//!   Windows >= 8.
//! - **macOS:** no pdeathsig; relies on `kill_on_drop` + the graceful teardown.

use tokio::process::{ChildStderr, ChildStdout, Command};

use alien_error::AlienError;

use crate::error::{ErrorData, Result};

/// A spawned user-application process. On Windows it is wrapped in a kill-on-close
/// Job Object; elsewhere it is a plain child (the caller sets the Linux pdeathsig
/// and `kill_on_drop` backstops on the `Command`).
pub struct AppChild {
    #[cfg(windows)]
    inner: command_group::AsyncGroupChild,
    #[cfg(not(windows))]
    inner: tokio::process::Child,
}

impl AppChild {
    /// Spawn `cmd`. On Windows the child is placed in its own kill-on-close Job
    /// Object; elsewhere it is spawned directly.
    pub fn spawn(cmd: &mut Command) -> Result<Self> {
        #[cfg(windows)]
        let inner = {
            use command_group::AsyncCommandGroup;
            cmd.group_spawn().map_err(spawn_error)?
        };
        #[cfg(not(windows))]
        let inner = cmd.spawn().map_err(spawn_error)?;
        Ok(Self { inner })
    }

    /// The OS process id, while the child is still known to the OS.
    pub fn id(&self) -> Option<u32> {
        self.inner.id()
    }

    /// Take the piped stdout stream (once).
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child_mut().stdout.take()
    }

    /// Take the piped stderr stream (once).
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child_mut().stderr.take()
    }

    /// Wait for the child to exit.
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.inner.wait().await
    }

    /// Kill the child. On Windows this terminates the whole Job Object.
    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.inner.kill().await
    }

    /// Mutable access to the underlying tokio child, for its stdio streams.
    fn child_mut(&mut self) -> &mut tokio::process::Child {
        #[cfg(windows)]
        {
            self.inner.inner()
        }
        #[cfg(not(windows))]
        {
            &mut self.inner
        }
    }
}

/// Map a spawn failure to the runtime's `ProcessFailed` error.
fn spawn_error(e: std::io::Error) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ProcessFailed {
        exit_code: None,
        message: format!("Failed to start application: {}", e),
    })
}
