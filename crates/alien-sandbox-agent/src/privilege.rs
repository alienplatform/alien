//! Dropping to the unprivileged identity a command runs as.
//!
//! One implementation, used by both spawn paths. The PID-namespace path has to drop inside the
//! namespace, and the ordinary path cannot use `Command::uid`/`gid` for it: `std` applies those
//! before `pre_exec` runs, and by then the privilege needed to drop supplementary groups is gone.

use std::{
    io,
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
        if libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        ) != 0
        {
            return Err(io::Error::last_os_error());
        }
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
    // CHOWN, DAC_OVERRIDE, SETGID, SETUID, SETPCAP, NET_ADMIN.
    let keep: u32 = (1 << 0) | (1 << 1) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 12);
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
        if libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        ) != 0
        {
            return Err(io::Error::last_os_error());
        }
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
