//! Dropping to the unprivileged identity a command runs as.
//!
//! One implementation, used by both spawn paths. The PID-namespace path has to drop inside the
//! namespace, and the ordinary path cannot use `Command::uid`/`gid` for it: `std` applies those
//! before `pre_exec` runs, and by then the privilege needed to drop supplementary groups is gone.

use std::io;

use crate::exec::ExecIdentity;

/// Drops to `identity` and makes the drop irreversible.
///
/// Runs in the forked child before `exec`, so everything here is async-signal-safe.
///
/// The order is load-bearing. Supplementary groups go first, because dropping the uid gives away
/// the privilege to drop them and they would otherwise survive — including group 0, which is most
/// of what refusing gid 0 is there to prevent. gid before uid for the same reason.
///
/// # Safety
///
/// Must be called only between `fork` and `exec`.
pub unsafe fn drop_to(identity: ExecIdentity) -> io::Result<()> {
    // Shedding groups needs `CAP_SETGID`, which a process that is not crossing a privilege
    // boundary does not have and does not need: if the command already runs as this identity,
    // there is no membership for it to inherit that it would not have had anyway. A real drop
    // must shed them, and a failure there is fatal rather than a partial boundary.
    let crossing = libc::geteuid() != identity.uid || libc::getegid() != identity.gid;
    if crossing && libc::setgroups(0, std::ptr::null()) != 0 {
        return Err(io::Error::last_os_error());
    }

    if libc::setgid(identity.gid) != 0 {
        return Err(io::Error::last_os_error());
    }

    if libc::setuid(identity.uid) != 0 {
        return Err(io::Error::last_os_error());
    }

    // The base image comes from the caller, so it may carry a setuid binary. Without this the
    // command runs one and returns to uid 0, undoing the drop above. Unprivileged and one-way.
    #[cfg(target_os = "linux")]
    if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
        return Err(io::Error::last_os_error());
    }

    // Refuse rather than run wide. A drop that silently failed would run untrusted code as the
    // agent, which is the whole escalation this exists to prevent.
    if libc::geteuid() != identity.uid || libc::getegid() != identity.gid {
        // No allocation between fork and exec: std transports only the raw errno to the parent,
        // so a message would be discarded, and allocating here can deadlock on the malloc lock.
        return Err(io::Error::from_raw_os_error(libc::EPERM));
    }

    Ok(())
}
