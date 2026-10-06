//! Dropping to the unprivileged identity a command runs as.
//!
//! One implementation, used by both spawn paths. The PID-namespace path has to drop inside the
//! namespace, and the ordinary path cannot use `Command::uid`/`gid` for it: `std` applies those
//! before `pre_exec` runs, and by then the privilege needed to drop supplementary groups is gone.

use std::{
    fs,
    io::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::exec::ExecIdentity;

static BLOCK_IPV6: AtomicBool = AtomicBool::new(false);

/// Set once before any image command or exec request can run. AWS transport may need IPv6,
/// so retain it for the agent while denying it irreversibly to every command.
pub fn block_command_ipv6() {
    BLOCK_IPV6.store(true, Ordering::Relaxed);
}

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

    #[cfg(target_os = "linux")]
    {
        if identity.uid == 0 || identity.gid == 0 {
            return Err(io::Error::from_raw_os_error(libc::EPERM));
        }
        // Clear ambient and inherited capabilities even if the supervisor received ALL.
        clear_ambient_capabilities()?;
        if libc::geteuid() == 0 {
            for capability in 0..64 {
                let present = libc::prctl(libc::PR_CAPBSET_READ, capability, 0, 0, 0);
                if present < 0 {
                    if io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
                        break;
                    }
                    return Err(io::Error::last_os_error());
                }
                if present == 1 && libc::prctl(libc::PR_CAPBSET_DROP, capability, 0, 0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
        }
    }

    if libc::setgid(identity.gid) != 0 {
        return Err(io::Error::last_os_error());
    }

    if libc::setuid(identity.uid) != 0 {
        return Err(io::Error::last_os_error());
    }

    #[cfg(target_os = "linux")]
    {
        #[repr(C)]
        struct Header {
            version: u32,
            pid: i32,
        }
        #[repr(C)]
        struct Data {
            effective: u32,
            permitted: u32,
            inheritable: u32,
        }
        let header = Header {
            version: 0x20080522,
            pid: 0,
        };
        let data = [
            Data {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
            Data {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
        ];
        if libc::syscall(libc::SYS_capset, &header, data.as_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
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

    #[cfg(target_os = "linux")]
    if BLOCK_IPV6.load(Ordering::Relaxed) {
        deny_ipv6_sockets()?;
    }
    Ok(())
}

/// A kernel or sandbox runtime without ambient capabilities (Linux < 4.3, older gVisor) rejects
/// PR_CAP_AMBIENT with EINVAL. Accepting it is safe only because every caller follows with a
/// capset that zeroes the inheritable set, which empties the ambient set as well; a caller that
/// does not must not use this.
#[cfg(target_os = "linux")]
unsafe fn clear_ambient_capabilities() -> io::Result<()> {
    if libc::prctl(
        libc::PR_CAP_AMBIENT,
        libc::PR_CAP_AMBIENT_CLEAR_ALL as libc::c_ulong,
        0 as libc::c_ulong,
        0 as libc::c_ulong,
        0 as libc::c_ulong,
    ) == 0
    {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EINVAL) {
        Ok(())
    } else {
        Err(error)
    }
}

/// Shrinks AWS's ALL capability grant before the supervisor starts image code or serves exec.
/// Retains only network administration, identity dropping, and access to command-owned files.
#[cfg(target_os = "linux")]
pub fn restrict_supervisor() -> io::Result<()> {
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    // CHOWN, DAC_OVERRIDE, FOWNER (file projection), KILL (timeouts/cancellation),
    // SETGID, SETUID, SETPCAP (irreversible child drop), NET_ADMIN.
    let keep: u32 =
        (1 << 0) | (1 << 1) | (1 << 3) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 12);
    unsafe {
        for capability in 0..64 {
            let present = libc::prctl(libc::PR_CAPBSET_READ, capability, 0, 0, 0);
            if present < 0 {
                if io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
                    break;
                }
                return Err(io::Error::last_os_error());
            }
            if present == 1
                && (capability >= 32 || keep & (1 << capability) == 0)
                && libc::prctl(libc::PR_CAPBSET_DROP, capability, 0, 0, 0) != 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        clear_ambient_capabilities()?;
        let header = Header {
            version: 0x20080522,
            pid: 0,
        };
        let data = [
            Data {
                effective: keep,
                permitted: keep,
                inheritable: 0,
            },
            Data {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
        ];
        if libc::syscall(libc::SYS_capset, &header, data.as_ptr()) != 0
            || libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Kernel-enforced even if the guest later acquires a new IPv6 route. Blocking io_uring prevents
/// IORING_OP_SOCKET from creating a socket without passing through the socket syscall filter.
#[cfg(target_os = "linux")]
unsafe fn deny_ipv6_sockets() -> io::Result<()> {
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xc000003e;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc00000b7;
    // BPF LD W ABS / JMP JEQ K / RET K. Offsets are from Linux's struct seccomp_data.
    let stmt = |code, k| libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    };
    let jump = |k, jt, jf| libc::sock_filter {
        code: 0x15,
        jt,
        jf,
        k,
    };
    let mut filter = [
        stmt(0x20, 4),
        jump(ARCH, 1, 0),
        stmt(0x06, 0x80000000), // kill foreign ABI
        stmt(0x20, 0),
        jump(libc::SYS_io_uring_setup as u32, 7, 0),
        jump(libc::SYS_io_uring_enter as u32, 6, 0),
        jump(libc::SYS_io_uring_register as u32, 5, 0),
        // x32 uses a different syscall number under AUDIT_ARCH_X86_64. Kill that ABI too.
        libc::sock_filter {
            code: 0x45,
            jt: 0,
            jf: 1,
            k: 0x40000000,
        },
        stmt(0x06, 0x80000000),
        jump(libc::SYS_socket as u32, 0, 4),
        stmt(0x20, 16),
        jump(libc::AF_INET6 as u32, 0, 2),
        stmt(0x06, 0x00050000 | libc::EPERM as u32),
        stmt(0x06, 0x7fff0000),
        stmt(0x06, 0x7fff0000),
    ];
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };
    if libc::prctl(libc::PR_SET_SECCOMP, 2, &program, 0, 0) != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn unprivileged_identity() -> ExecIdentity {
        unsafe {
            let uid = if libc::geteuid() == 0 {
                60000
            } else {
                libc::geteuid()
            };
            let gid = if libc::getegid() == 0 {
                60000
            } else {
                libc::getegid()
            };
            ExecIdentity { uid, gid }
        }
    }

    #[test]
    fn child_identity_and_ipv6_filter_are_irreversible() {
        // Only async-signal-safe syscalls in the child: no allocator or test framework after fork.
        unsafe {
            let ExecIdentity { uid, gid } = unprivileged_identity();
            let pid = libc::fork();
            assert!(pid >= 0, "fork must succeed");
            if pid == 0 {
                block_command_ipv6();
                if drop_to(ExecIdentity { uid, gid }).is_err() {
                    libc::_exit(1);
                }
                if libc::geteuid() != uid || libc::getegid() != gid {
                    libc::_exit(2);
                }
                let socket = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
                if socket < 0 {
                    libc::_exit(3);
                }
                libc::close(socket);
                if libc::socket(libc::AF_INET6, libc::SOCK_STREAM, 0) != -1
                    || *libc::__errno_location() != libc::EPERM
                {
                    libc::_exit(4);
                }
                if libc::syscall(libc::SYS_io_uring_setup, 1, std::ptr::null::<u8>()) != -1
                    || *libc::__errno_location() != libc::EPERM
                {
                    libc::_exit(5);
                }
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 0, 0, 0, 0) != -1 {
                    libc::_exit(6);
                }
                if libc::setuid(0) != -1 {
                    libc::_exit(7);
                }
                libc::_exit(0);
            }
            let mut status = 0;
            assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
            assert!(libc::WIFEXITED(status), "child was killed: {status}");
            assert_eq!(libc::WEXITSTATUS(status), 0, "identity/filter check failed");
        }
    }

    #[test]
    fn drop_succeeds_where_the_kernel_has_no_ambient_capabilities() {
        #[cfg(target_arch = "x86_64")]
        const ARCH: u32 = 0xc000003e;
        #[cfg(target_arch = "aarch64")]
        const ARCH: u32 = 0xc00000b7;
        let stmt = |code, k| libc::sock_filter {
            code,
            jt: 0,
            jf: 0,
            k,
        };
        let jump = |k, jt, jf| libc::sock_filter {
            code: 0x15,
            jt,
            jf,
            k,
        };
        // Every PR_CAP_AMBIENT prctl fails with EINVAL, as on a kernel without ambient
        // capabilities; everything else is allowed.
        let mut filter = [
            stmt(0x20, 4),
            jump(ARCH, 0, 5),
            stmt(0x20, 0),
            jump(libc::SYS_prctl as u32, 0, 3),
            stmt(0x20, 16),
            jump(libc::PR_CAP_AMBIENT as u32, 0, 1),
            stmt(0x06, 0x00050000 | libc::EINVAL as u32),
            stmt(0x06, 0x7fff0000),
        ];
        let program = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_mut_ptr(),
        };
        unsafe {
            let identity = unprivileged_identity();
            let pid = libc::fork();
            assert!(pid >= 0, "fork must succeed");
            if pid == 0 {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                    || libc::prctl(libc::PR_SET_SECCOMP, 2, &program, 0, 0) != 0
                {
                    libc::_exit(10);
                }
                if libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_IS_SET, 0, 0, 0) != -1
                    || *libc::__errno_location() != libc::EINVAL
                {
                    libc::_exit(11);
                }
                if drop_to(identity).is_err() {
                    libc::_exit(1);
                }
                if libc::geteuid() != identity.uid || libc::getegid() != identity.gid {
                    libc::_exit(2);
                }
                libc::_exit(0);
            }
            let mut status = 0;
            assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
            assert!(libc::WIFEXITED(status), "child was killed: {status}");
            assert_eq!(
                libc::WEXITSTATUS(status),
                0,
                "the drop must succeed without ambient capabilities"
            );
        }
    }

    /// Root only: raising an ambient capability needs one in the permitted set.
    #[test]
    fn an_ambient_capability_is_emptied_even_when_clear_all_is_rejected() {
        if unsafe { libc::geteuid() } != 0 {
            return;
        }
        #[cfg(target_arch = "x86_64")]
        const ARCH: u32 = 0xc000003e;
        #[cfg(target_arch = "aarch64")]
        const ARCH: u32 = 0xc00000b7;
        const CAP_KILL: u32 = 5;
        #[repr(C)]
        struct Header {
            version: u32,
            pid: i32,
        }
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Data {
            effective: u32,
            permitted: u32,
            inheritable: u32,
        }
        let stmt = |code, k| libc::sock_filter {
            code,
            jt: 0,
            jf: 0,
            k,
        };
        let jump = |k, jt, jf| libc::sock_filter {
            code: 0x15,
            jt,
            jf,
            k,
        };
        // Rejects only PR_CAP_AMBIENT_CLEAR_ALL, so PR_CAP_AMBIENT_IS_SET still answers.
        let mut filter = [
            stmt(0x20, 4),
            jump(ARCH, 0, 7),
            stmt(0x20, 0),
            jump(libc::SYS_prctl as u32, 0, 5),
            stmt(0x20, 16),
            jump(libc::PR_CAP_AMBIENT as u32, 0, 3),
            stmt(0x20, 24),
            jump(libc::PR_CAP_AMBIENT_CLEAR_ALL as u32, 0, 1),
            stmt(0x06, 0x00050000 | libc::EINVAL as u32),
            stmt(0x06, 0x7fff0000),
        ];
        let program = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_mut_ptr(),
        };
        let identity = ExecIdentity {
            uid: 60000,
            gid: 60000,
        };
        unsafe {
            let pid = libc::fork();
            assert!(pid >= 0, "fork must succeed");
            if pid == 0 {
                // Become the identity first, keeping the permitted set: the kernel empties the
                // ambient set on any root-to-user switch, so a drop that changed the uid would
                // hide whether the capset does.
                if libc::prctl(libc::PR_SET_KEEPCAPS, 1, 0, 0, 0) != 0
                    || libc::setgid(identity.gid) != 0
                    || libc::setuid(identity.uid) != 0
                {
                    libc::_exit(10);
                }
                let header = Header {
                    version: 0x20080522,
                    pid: 0,
                };
                let mut data = [Data {
                    effective: 0,
                    permitted: 0,
                    inheritable: 0,
                }; 2];
                if libc::syscall(libc::SYS_capget, &header, data.as_mut_ptr()) != 0 {
                    libc::_exit(11);
                }
                data[0].inheritable |= 1 << CAP_KILL;
                if libc::syscall(libc::SYS_capset, &header, data.as_ptr()) != 0 {
                    libc::_exit(12);
                }
                if libc::prctl(
                    libc::PR_CAP_AMBIENT,
                    libc::PR_CAP_AMBIENT_RAISE,
                    CAP_KILL,
                    0,
                    0,
                ) != 0
                    || libc::prctl(
                        libc::PR_CAP_AMBIENT,
                        libc::PR_CAP_AMBIENT_IS_SET,
                        CAP_KILL,
                        0,
                        0,
                    ) != 1
                {
                    libc::_exit(13);
                }
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                    || libc::prctl(libc::PR_SET_SECCOMP, 2, &program, 0, 0) != 0
                {
                    libc::_exit(14);
                }
                if libc::prctl(
                    libc::PR_CAP_AMBIENT,
                    libc::PR_CAP_AMBIENT_CLEAR_ALL,
                    0,
                    0,
                    0,
                ) != -1
                    || *libc::__errno_location() != libc::EINVAL
                    || libc::prctl(
                        libc::PR_CAP_AMBIENT,
                        libc::PR_CAP_AMBIENT_IS_SET,
                        CAP_KILL,
                        0,
                        0,
                    ) != 1
                {
                    libc::_exit(15);
                }
                if drop_to(identity).is_err() {
                    libc::_exit(1);
                }
                if libc::prctl(
                    libc::PR_CAP_AMBIENT,
                    libc::PR_CAP_AMBIENT_IS_SET,
                    CAP_KILL,
                    0,
                    0,
                ) != 0
                {
                    libc::_exit(2);
                }
                libc::_exit(0);
            }
            let mut status = 0;
            assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
            assert!(libc::WIFEXITED(status), "child was killed: {status}");
            assert_eq!(
                libc::WEXITSTATUS(status),
                0,
                "the ambient set must be empty after the drop"
            );
        }
    }
}

/// Creates passwd/group entries for a declaration-owned numeric identity when the base image
/// has none. The kernel itself needs no entry, but image commands often call getpwuid/getgrgid.
/// This runs at startup, never in a forked child.
pub fn prepare_identity(identity: ExecIdentity, root: &std::path::Path) -> io::Result<()> {
    for (path, id, entry) in [
        (
            "/etc/passwd",
            identity.uid,
            format!(
                "sandbox-{}:x:{}:{}::{}:/sbin/nologin\n",
                identity.uid,
                identity.uid,
                identity.gid,
                root.display()
            ),
        ),
        (
            "/etc/group",
            identity.gid,
            format!("sandbox-{}:x:{}:\n", identity.gid, identity.gid),
        ),
    ] {
        let contents = fs::read_to_string(path)?;
        if !contents.lines().any(|line| {
            line.split(':')
                .nth(2)
                .and_then(|value| value.parse::<u32>().ok())
                == Some(id)
        }) {
            let mut file = fs::OpenOptions::new().append(true).open(path)?;
            if !contents.ends_with('\n') {
                file.write_all(b"\n")?;
            }
            file.write_all(entry.as_bytes())?;
        }
    }
    Ok(())
}
