//! Windows child supervisor — mirrors `unix_child.rs`. The operator is spawned
//! inside a Job Object (`command-group`, `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`:
//! die-with-parent for free — when the launcher dies the job closes and the
//! operator is terminated). Graceful `stop` sends `CTRL_BREAK` to the child's
//! process group, then escalates to Job termination after the grace window.
//!
//! Constructed by `main.rs`'s Windows `run_supervisor` (aliased
//! `ActiveChildSupervisor`) and driven through `core::run`.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use alien_core::self_update::{ENV_HEALTH_ADDR, ENV_LAUNCHER_VERSION, ENV_SELF_UPDATE};
use alien_error::{AlienError, Context, IntoAlienError};
use command_group::{CommandGroup, GroupChild};
use tracing::warn;
use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};

use crate::core::traits::{ChildSupervisor, ExitStatus, OperatorHandle, UpdateEnv};
use crate::error::{ErrorData, Result};

/// Poll cadence while waiting out a graceful stop's grace window.
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// `CREATE_NEW_PROCESS_GROUP` — the child leads its own console process group so
/// `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid)` targets only it, not the
/// launcher. (Die-with-parent is separate: command-group's kill-on-close Job.)
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// `STILL_ACTIVE` — `GetExitCodeProcess` reports this while the process runs.
#[cfg(test)]
const STILL_ACTIVE: u32 = 259;

#[derive(Default)]
pub struct WindowsChildSupervisor {
    /// Live children by pid. The pid is also the console process-group id (the
    /// child is a `CREATE_NEW_PROCESS_GROUP` leader), which is what
    /// `GenerateConsoleCtrlEvent` targets on a graceful stop.
    children: HashMap<u32, GroupChild>,
}

impl WindowsChildSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    fn child_mut(&mut self, handle: &OperatorHandle) -> Result<&mut GroupChild> {
        self.children.get_mut(&handle.pid).ok_or_else(|| {
            AlienError::new(ErrorData::Other {
                message: format!("no live child for pid {}", handle.pid),
            })
        })
    }
}

impl ChildSupervisor for WindowsChildSupervisor {
    fn spawn(&mut self, binary: &Path, env: &UpdateEnv) -> Result<OperatorHandle> {
        use std::os::windows::process::CommandExt;

        let mut command = std::process::Command::new(binary);
        command
            .env(ENV_SELF_UPDATE, "1")
            .env(ENV_LAUNCHER_VERSION, &env.launcher_version)
            .env(ENV_HEALTH_ADDR, env.health_addr.to_string())
            // Own console group so a later CTRL_BREAK targets only this child.
            .creation_flags(CREATE_NEW_PROCESS_GROUP);

        // `group_spawn` puts the child in a Job Object with kill-on-close; on
        // Windows >= 8 nested Jobs work, so this succeeds even when the launcher
        // itself runs inside a Job (some CI) — command-group sets breakaway as
        // needed.
        let child = command
            .group_spawn()
            .into_alien_error()
            .context(ErrorData::SpawnFailed {
                binary_path: binary.display().to_string(),
                message: "failed to spawn the operator in its own Job Object".to_string(),
            })?;
        let pid = child.id();
        self.children.insert(pid, child);
        Ok(OperatorHandle { pid })
    }

    fn stop(&mut self, handle: &OperatorHandle, grace: Duration) -> Result<()> {
        // Graceful: CTRL_BREAK to the child's process group (pid == pgid).
        // Best-effort — a child that has already exited, ignores Ctrl events, or
        // has no console just doesn't stop here; the grace-poll + Job-terminate
        // escalation below is the hard guarantee.
        // SAFETY: FFI call taking a plain pid; touches no shared state.
        let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, handle.pid) };
        if sent == 0 {
            warn!(
                pid = handle.pid,
                "GenerateConsoleCtrlEvent(CTRL_BREAK) failed; escalating to Job termination"
            );
        }

        let deadline = Instant::now() + grace;
        loop {
            if self.try_wait(handle)?.is_some() {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            std::thread::sleep(STOP_POLL_INTERVAL.min(deadline - now));
        }

        // Escalate: terminate the whole Job Object, then reap.
        let child = self.child_mut(handle)?;
        child.kill().into_alien_error().context(ErrorData::Other {
            message: format!("failed to terminate the Job Object for pid {}", handle.pid),
        })?;
        child.wait().into_alien_error().context(ErrorData::Other {
            message: format!("failed to reap the Job Object for pid {}", handle.pid),
        })?;
        Ok(())
    }

    fn try_wait(&mut self, handle: &OperatorHandle) -> Result<Option<ExitStatus>> {
        let child = self.child_mut(handle)?;
        let status = child.try_wait().into_alien_error().context(ErrorData::Other {
            message: format!("failed to poll child {}", handle.pid),
        })?;
        Ok(status.map(map_exit_status))
    }
}

/// Map a process exit to our `ExitStatus`. On Windows `code()` is always `Some`
/// (no signals), so this never yields `Signal`.
fn map_exit_status(status: std::process::ExitStatus) -> ExitStatus {
    match status.code() {
        Some(code) => ExitStatus::Code(code),
        None => ExitStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;

    /// Spawn `cmd /c <cmdline>` in a kill-on-close Job + its own process group,
    /// and register it with the supervisor so `stop`/`try_wait` address it by
    /// pid. (`ChildSupervisor::spawn` takes no args, so the tests build the
    /// child directly — same as `unix_child`'s `spawn_sh`.)
    fn spawn_cmd(sup: &mut WindowsChildSupervisor, cmdline: &str) -> OperatorHandle {
        let child = std::process::Command::new("cmd")
            .args(["/c", cmdline])
            .creation_flags(CREATE_NEW_PROCESS_GROUP)
            .group_spawn()
            .expect("group_spawn cmd");
        let pid = child.id();
        sup.children.insert(pid, child);
        OperatorHandle { pid }
    }

    /// Is `pid` still running? (`OpenProcess` + `GetExitCodeProcess` == STILL_ACTIVE.)
    fn process_alive(pid: u32) -> bool {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: standard Win32 query dance; the handle is closed on every path.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code: u32 = 0;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok != 0 && code == STILL_ACTIVE
        }
    }

    #[test]
    fn stop_terminates_a_long_running_child_within_grace() {
        let mut sup = WindowsChildSupervisor::new();
        // `/nobreak` ignores Ctrl events, so this exercises the Job-terminate
        // escalation, not the graceful path.
        let handle = spawn_cmd(&mut sup, "timeout /t 30 /nobreak >nul");
        let started = Instant::now();
        sup.stop(&handle, Duration::from_secs(2)).expect("stop");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "stop must not wait for the 30s timeout"
        );
        assert!(
            sup.try_wait(&handle).expect("try_wait").is_some(),
            "child should be gone after stop"
        );
    }

    #[test]
    fn try_wait_reports_the_exit_code() {
        let mut sup = WindowsChildSupervisor::new();
        let handle = spawn_cmd(&mut sup, "exit 10");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = sup.try_wait(&handle).expect("try_wait") {
                assert_eq!(status, ExitStatus::Code(10));
                return;
            }
            assert!(Instant::now() < deadline, "child never exited");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn dropping_the_child_kills_it_via_job_close() {
        // Spawn a long child, capture its pid, then drop the GroupChild — closing
        // the Job handle fires JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE (the launcher's
        // die-with-parent). The child must be gone shortly after.
        let child = std::process::Command::new("cmd")
            .args(["/c", "timeout /t 30 /nobreak >nul"])
            .creation_flags(CREATE_NEW_PROCESS_GROUP)
            .group_spawn()
            .expect("group_spawn");
        let pid = child.id();
        drop(child);

        let deadline = Instant::now() + Duration::from_secs(2);
        while process_alive(pid) {
            assert!(
                Instant::now() < deadline,
                "child survived the Job handle closing"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
