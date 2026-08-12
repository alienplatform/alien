//! Running a command as PID 1 of its own namespace, so it cannot see or signal the agent.
//!
//! This is supervisor isolation's second lock; the first (uid drop plus path confinement)
//! already closes the escalation. This one prevents untrusted code from enumerating or signaling
//! the agent.
//!
//! **No backend grants `CAP_SYS_ADMIN` today.** Creating a PID namespace needs
//! `CAP_SYS_ADMIN`. The agent inside a Lambda MicroVM runs as uid 0 with
//! `CapEff: 00000000a80425fb` — the standard container default set, which holds `CAP_SETUID` and
//! `CAP_SETGID` (so the uid drop works) and excludes `CAP_SYS_ADMIN`. A Kubernetes sandbox pod
//! drops every capability by design. So `SandboxCapabilities::supervisor_pid_namespace` is
//! `false` everywhere, and this code is gated on a real capability read rather than deleted: it
//! turns itself on if a runtime ever grants the capability, and it turns nothing on until then.
//!
//! Two ordering traps are handled here and both are silent if you get them wrong:
//!
//! 1. **`std` drops the uid before running `pre_exec`.** So the unshare has to happen inside the
//!    closure *and* the uid drop has to move in with it, because by the time a `Command::uid()`
//!    has taken effect there is no privilege left to create a namespace with.
//! 2. **`unshare(CLONE_NEWPID)` does not move the caller.** It affects the caller's future
//!    children, so the process that unshares must fork; the fork's child is PID 1.

#[cfg(target_os = "linux")]
use std::io;

use crate::exec::ExecIdentity;

/// Whether this process can create a PID namespace for its children.
///
/// Reads the effective capability set rather than checking for uid 0. **Root is not the same as
/// `CAP_SYS_ADMIN`**: a Lambda MicroVM runs the agent as uid 0 with `CAP_SYS_ADMIN` masked out,
/// so a uid-0 check would fail every spawn with `EPERM` from inside `pre_exec`, surfacing to the
/// caller only as `spawnFailed`.
#[cfg(target_os = "linux")]
pub fn available() -> bool {
    effective_capabilities().is_some_and(|capabilities| capabilities & (1 << CAP_SYS_ADMIN) != 0)
}

/// `CAP_SYS_ADMIN` in `capability.h`.
#[cfg(target_os = "linux")]
const CAP_SYS_ADMIN: u64 = 21;

/// Reads `CapEff` from `/proc/self/status`, or `None` if it cannot be read.
///
/// `None` means no namespace: a capability we cannot confirm is one we do not claim.
#[cfg(target_os = "linux")]
fn effective_capabilities() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("CapEff:"))?;
    u64::from_str_radix(line.split_whitespace().nth(1)?, 16).ok()
}

#[cfg(not(target_os = "linux"))]
pub fn available() -> bool {
    false
}

/// Installs the namespace-and-drop sequence on a command about to be spawned.
///
/// The caller must **not** also set `Command::uid`/`gid`: this performs the drop itself, after
/// the unshare, for the reason in the module docs.
#[cfg(target_os = "linux")]
pub fn apply(command: &mut tokio::process::Command, identity: ExecIdentity) {
    unsafe {
        command.pre_exec(move || enter_namespace(identity));
    }
}

#[cfg(not(target_os = "linux"))]
pub fn apply(_command: &mut tokio::process::Command, _identity: ExecIdentity) {}

/// Runs in the forked child, before `exec`. Everything here is async-signal-safe.
///
/// Returning `Ok` continues to `exec`; the intermediate process never returns, it waits for
/// PID 1 and exits with its status so the caller sees the command's real exit code.
#[cfg(target_os = "linux")]
fn enter_namespace(identity: ExecIdentity) -> io::Result<()> {
    unsafe {
        if libc::unshare(libc::CLONE_NEWPID | libc::CLONE_NEWNS) != 0 {
            return Err(io::Error::last_os_error());
        }

        match libc::fork() {
            -1 => Err(io::Error::last_os_error()),
            0 => become_pid_one(identity),
            child => supervise(child),
        }
    }
}

/// The new namespace's PID 1: give it its own `/proc`, drop privilege, continue to `exec`.
#[cfg(target_os = "linux")]
unsafe fn become_pid_one(identity: ExecIdentity) -> io::Result<()> {
    // Die with the intermediate. A deadline kills the intermediate, and PID 1 outliving it would
    // leave a runaway with no parent — the opposite of what a deadline is for. When PID 1 goes,
    // the kernel takes the rest of the namespace with it.
    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
        return Err(io::Error::last_os_error());
    }

    // Private first, or the /proc mount below propagates back to the host namespace and the
    // agent's own view of its processes changes underneath it.
    if libc::mount(
        std::ptr::null(),
        c"/".as_ptr(),
        std::ptr::null(),
        libc::MS_REC | libc::MS_PRIVATE,
        std::ptr::null(),
    ) != 0
    {
        return Err(io::Error::last_os_error());
    }

    // Without this the command reads the host's /proc and sees every process, which is most of
    // what the namespace was for.
    if libc::mount(
        c"proc".as_ptr(),
        c"/proc".as_ptr(),
        c"proc".as_ptr(),
        0,
        std::ptr::null(),
    ) != 0
    {
        return Err(io::Error::last_os_error());
    }

    crate::privilege::drop_to(identity)?;

    Ok(())
}

/// The intermediate process: wait for PID 1 and exit with its status.
///
/// Never returns, so it never reaches `exec`. The caller's `wait()` sees this process, which is
/// why its exit status has to be the command's.
#[cfg(target_os = "linux")]
unsafe fn supervise(child: libc::pid_t) -> ! {
    let mut status: libc::c_int = 0;

    while libc::waitpid(child, &mut status, 0) < 0 {
        if *libc::__errno_location() != libc::EINTR {
            libc::_exit(127);
        }
    }

    if libc::WIFEXITED(status) {
        libc::_exit(libc::WEXITSTATUS(status));
    }

    // Signalled. 128+signal is the shell convention, and it distinguishes "killed" from an exit
    // code the command chose.
    if libc::WIFSIGNALED(status) {
        libc::_exit(128 + libc::WTERMSIG(status));
    }

    libc::_exit(127)
}
