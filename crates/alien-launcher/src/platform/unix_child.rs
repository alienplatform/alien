//! Unix `ChildSupervisor` (Linux + macOS): spawn the operator in its own
//! process group, guarantee die-with-parent, stop gracefully with escalation.
//!
//! Die-with-parent is normative (see the trait docs). On Linux the kernel
//! delivers SIGTERM to the child when the launcher dies
//! (`PR_SET_PDEATHSIG`, installed in `pre_exec` between fork and exec). macOS
//! has no equivalent, so there the fast path is the operator-side parent
//! watch (it polls its parent pid and exits when reparented); the operator's
//! `InstanceLock` is the correctness backstop on both — a respawned
//! launcher's operator cannot run until an orphan exits.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use command_group::{CommandGroup, GroupChild};

use crate::core::traits::{
    ChildSupervisor, ExitStatus, OperatorHandle, UpdateEnv,
};
use crate::error::{ErrorData, Result};
use alien_core::self_update::{ENV_HEALTH_ADDR, ENV_LAUNCHER_VERSION, ENV_SELF_UPDATE};
use alien_error::{AlienError, Context, IntoAlienError};

/// How often `stop` re-polls the child while waiting out the grace period.
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Default)]
pub struct UnixChildSupervisor {
    /// Live children by pid (the group leader's pid IS the pgid).
    children: HashMap<u32, GroupChild>,
}

impl UnixChildSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    fn child_mut(&mut self, handle: &OperatorHandle) -> Result<&mut GroupChild> {
        self.children.get_mut(&handle.pid).ok_or_else(|| {
            AlienError::new(ErrorData::Other {
                message: format!("unknown child pid {}", handle.pid),
            })
        })
    }
}

impl ChildSupervisor for UnixChildSupervisor {
    fn spawn(&mut self, binary: &Path, env: &UpdateEnv) -> Result<OperatorHandle> {
        let mut command = std::process::Command::new(binary);
        command
            .env(ENV_SELF_UPDATE, "1")
            .env(ENV_LAUNCHER_VERSION, &env.launcher_version)
            .env(ENV_HEALTH_ADDR, env.health_addr.to_string());

        // Linux: ask the kernel to SIGTERM the child if we die. Runs between
        // fork and exec; prctl is async-signal-safe. The tiny race (launcher
        // dies before the prctl runs) is covered by the InstanceLock backstop.
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: only async-signal-safe calls (prctl, getppid) run in
            // the pre-exec child context.
            unsafe {
                command.pre_exec(|| {
                    nix::sys::prctl::set_pdeathsig(nix::sys::signal::Signal::SIGTERM)
                        .map_err(std::io::Error::from)?;
                    // If the launcher died between fork and the prctl above,
                    // the death signal will never fire — detect the reparent
                    // and bail before exec.
                    if nix::unistd::getppid().as_raw() == 1 {
                        return Err(std::io::Error::other(
                            "launcher died before the operator exec'd",
                        ));
                    }
                    Ok(())
                });
            }
        }

        let child = command
            .group_spawn()
            .into_alien_error()
            .context(ErrorData::SpawnFailed {
                binary_path: binary.display().to_string(),
                message: "failed to spawn the operator in its own process group".to_string(),
            })?;
        let pid = child.id();
        self.children.insert(pid, child);
        Ok(OperatorHandle { pid })
    }

    fn stop(&mut self, handle: &OperatorHandle, grace: Duration) -> Result<()> {
        // Graceful first: SIGTERM the whole group (negative pgid; the group
        // leader's pid is the pgid). ESRCH means it already exited — success.
        let pgid = nix::unistd::Pid::from_raw(handle.pid as i32);
        match nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGTERM) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(e) => {
                return Err(AlienError::new(ErrorData::Other {
                    message: format!("failed to SIGTERM process group {}: {e}", handle.pid),
                }));
            }
        }

        // Wait out the grace period, then escalate to a group SIGKILL.
        let deadline = Instant::now() + grace;
        loop {
            if self.try_wait(handle)?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(STOP_POLL_INTERVAL.min(deadline - Instant::now()));
        }

        let child = self.child_mut(handle)?;
        child.kill().into_alien_error().context(ErrorData::Other {
            message: format!("failed to SIGKILL process group {}", handle.pid),
        })?;
        // Reap the killed child so it does not linger as a zombie.
        child.wait().into_alien_error().context(ErrorData::Other {
            message: format!("failed to reap process group {}", handle.pid),
        })?;
        Ok(())
    }

    fn try_wait(&mut self, handle: &OperatorHandle) -> Result<Option<ExitStatus>> {
        let child = self.child_mut(handle)?;
        let status = child
            .try_wait()
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("failed to poll child {}", handle.pid),
            })?;
        Ok(status.map(map_exit_status))
    }
}

fn map_exit_status(status: std::process::ExitStatus) -> ExitStatus {
    match status.code() {
        Some(code) => ExitStatus::Code(code),
        None => {
            // No code on Unix means signal-terminated.
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if status.signal().is_some() {
                    return ExitStatus::Signal;
                }
            }
            ExitStatus::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::testing::test_update_env;

    fn spawn_sh(supervisor: &mut UnixChildSupervisor, script: &str) -> OperatorHandle {
        // `sh -c` needs the binary to be `sh` with args; the trait spawns a
        // bare binary, so write the script to a temp file and exec it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("op.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Leak the tempdir so the script survives until the child execs.
        let _ = Box::leak(Box::new(dir));
        supervisor
            .spawn(&path, &test_update_env())
            .expect("spawn should succeed")
    }

    /// Exit codes pass through `try_wait` — including the handoff code 10.
    #[test]
    fn exit_codes_pass_through() {
        let mut supervisor = UnixChildSupervisor::new();
        let handle = spawn_sh(&mut supervisor, "exit 10");

        let mut status = None;
        for _ in 0..100 {
            if let Some(s) = supervisor.try_wait(&handle).unwrap() {
                status = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(status, Some(ExitStatus::Code(10)));
    }

    /// The spawn env carries the handoff contract variables.
    #[test]
    fn spawn_env_carries_the_contract() {
        let mut supervisor = UnixChildSupervisor::new();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("env-dump");
        let handle = spawn_sh(
            &mut supervisor,
            &format!(
                "printf '%s|%s|%s' \"$ALIEN_SELF_UPDATE\" \"$ALIEN_LAUNCHER_VERSION\" \"$ALIEN_HEALTH_ADDR\" > {}",
                out.display()
            ),
        );
        for _ in 0..100 {
            if supervisor.try_wait(&handle).unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let dump = std::fs::read_to_string(&out).expect("child should have written the env dump");
        assert_eq!(dump, "1|0.1.0-test|127.0.0.1:7799");
    }

    /// A stubborn child (ignores TERM) is force-killed after the grace and
    /// reported as signal-terminated; a cooperative child exits 0 in time.
    #[test]
    fn stop_escalates_after_grace() {
        let mut supervisor = UnixChildSupervisor::new();
        // Ignores SIGTERM and sleeps far beyond the test.
        let stubborn = spawn_sh(&mut supervisor, "trap '' TERM; sleep 30");
        std::thread::sleep(Duration::from_millis(100)); // let the trap install
        let started = Instant::now();
        supervisor
            .stop(&stubborn, Duration::from_millis(300))
            .expect("stop should succeed");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "stop must not wait for the 30s sleep"
        );

        // Cooperative child: exits 0 on TERM, well within the grace.
        let cooperative = spawn_sh(&mut supervisor, "trap 'exit 0' TERM; sleep 30");
        std::thread::sleep(Duration::from_millis(100));
        supervisor
            .stop(&cooperative, Duration::from_secs(5))
            .expect("stop should succeed");
    }

    // NOTE: die-with-parent (the operator must die when the launcher dies) is
    // proven end-to-end by the os-service E2E, which SIGKILLs a real
    // `alien-launcher` process and asserts its operator child exits. There is
    // no sound *unit-level* test for it: the parent that dies must itself be a
    // process that called `UnixChildSupervisor::spawn`, and forking one inside
    // this multi-threaded test binary is unsafe (the forked child copies only
    // the calling thread, so `Command::spawn`'s allocations/locks can deadlock
    // post-fork). The production mechanism here — `PR_SET_PDEATHSIG` in
    // `pre_exec`, with `command-group`'s plain `process_group(0)` spawn keeping
    // the operator a DIRECT child of the launcher — is what makes that E2E pass.
}
