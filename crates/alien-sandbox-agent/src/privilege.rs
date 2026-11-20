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

static BLOCK_BPF: AtomicBool = AtomicBool::new(false);
static BLOCK_IPV6: AtomicBool = AtomicBool::new(false);
static IMAGE_MOUNT_FILTER: AtomicBool = AtomicBool::new(false);

/// Set once, before the image starts, when a mount broker serves it: the image and its lifecycle
/// steps then get [`deny_mount_bypass`] on top of the other filters.
pub fn filter_image_mounts() {
    IMAGE_MOUNT_FILTER.store(true, Ordering::Relaxed);
}

/// Set once before any image command or exec request can run. AWS transport may need IPv6,
/// so retain it for the agent while denying it irreversibly to every command.
pub fn block_command_ipv6() {
    BLOCK_IPV6.store(true, Ordering::Relaxed);
}

/// Prevent image descendants from changing the AWS cgroup socket marker, even
/// when unrestricted egress does not need the IPv6 filter.
pub fn block_command_bpf() {
    BLOCK_BPF.store(true, Ordering::Relaxed);
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
    drop_unprivileged_to(identity, false)
}

/// Docker's default capabilities without NET_RAW. Root image processes can manage
/// their workers, but cannot administer networking, mount filesystems or set socket
/// marks (which recent kernels also permit with NET_RAW).
#[cfg(target_os = "linux")]
pub const IMAGE_ROOT_CAPABILITIES: u32 = alien_core::sandbox_image::IMAGE_ROOT_CAPABILITY_MASK;

/// Enter the image USER with a bounded root grant or no capabilities for non-root.
///
/// # Safety
/// Must be called only between `fork` and `exec`.
#[cfg(target_os = "linux")]
pub unsafe fn drop_image_to(identity: ExecIdentity) -> io::Result<()> {
    if identity.uid != 0 {
        return drop_unprivileged_to(identity, true);
    }
    if libc::geteuid() != 0 {
        return Err(io::Error::from_raw_os_error(libc::EPERM));
    }
    if libc::setgroups(0, std::ptr::null()) != 0 || libc::setgid(identity.gid) != 0 {
        return Err(io::Error::last_os_error());
    }
    restrict_capabilities(IMAGE_ROOT_CAPABILITIES)?;
    if BLOCK_IPV6.load(Ordering::Relaxed) || BLOCK_BPF.load(Ordering::Relaxed) {
        deny_network_bypass(BLOCK_IPV6.load(Ordering::Relaxed))?;
    }
    if IMAGE_MOUNT_FILTER.load(Ordering::Relaxed) {
        deny_mount_bypass()?;
    }
    Ok(())
}

unsafe fn drop_unprivileged_to(identity: ExecIdentity, allow_root_group: bool) -> io::Result<()> {
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
        if identity.uid == 0 || (identity.gid == 0 && !allow_root_group) {
            return Err(io::Error::from_raw_os_error(libc::EPERM));
        }
        // Clear ambient capabilities; where the kernel rejects that, the capset after setuid
        // zeroes inheritable, which empties the ambient set too.
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
    if BLOCK_IPV6.load(Ordering::Relaxed) || BLOCK_BPF.load(Ordering::Relaxed) {
        deny_network_bypass(BLOCK_IPV6.load(Ordering::Relaxed))?;
    }
    Ok(())
}

/// Empties the ambient capability set. EINVAL is tolerated because every caller follows with a
/// capset that zeroes the inheritable set, and that capset empties the ambient set whether or not
/// this call did; a caller that does not must not use this. A kernel without ambient support
/// (Linux < 4.3, or the gVisor that GCP Agent Platform runs) answers every `PR_CAP_AMBIENT` with
/// EINVAL.
#[cfg(target_os = "linux")]
fn clear_ambient_capabilities() -> io::Result<()> {
    let cleared = unsafe {
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        )
    } == 0;
    if cleared {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EINVAL) {
        Ok(())
    } else {
        Err(error)
    }
}

/// Retains the image's bounded root grant so children can inherit it after bootstrap.
/// Only an agent installing guest network rules also retains NET_ADMIN; the image
/// spawn path always drops it. A missing required capability fails startup.
#[cfg(target_os = "linux")]
pub fn restrict_supervisor(network_admin: bool) -> io::Result<()> {
    let keep = IMAGE_ROOT_CAPABILITIES | if network_admin { 1 << 12 } else { 0 };
    restrict_capabilities(keep)
}

/// This thread's permitted capabilities (the low 32; nothing the agent keeps is above them).
#[cfg(target_os = "linux")]
pub(crate) fn permitted_capabilities() -> io::Result<u32> {
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    let mut header = Header {
        version: 0x20080522,
        pid: 0,
    };
    let mut data = [Data::default(); 2];
    // SAFETY: a version 3 header and two data structures, as capget expects.
    if unsafe { libc::syscall(libc::SYS_capget, &mut header, data.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(data[0].permitted)
}

/// The names of the capabilities in `mask` (bits 0 to 31).
pub(crate) fn capability_names(mask: u32) -> String {
    const NAMES: [&str; 32] = [
        "CHOWN",
        "DAC_OVERRIDE",
        "DAC_READ_SEARCH",
        "FOWNER",
        "FSETID",
        "KILL",
        "SETGID",
        "SETUID",
        "SETPCAP",
        "LINUX_IMMUTABLE",
        "NET_BIND_SERVICE",
        "NET_BROADCAST",
        "NET_ADMIN",
        "NET_RAW",
        "IPC_LOCK",
        "IPC_OWNER",
        "SYS_MODULE",
        "SYS_RAWIO",
        "SYS_CHROOT",
        "SYS_PTRACE",
        "SYS_PACCT",
        "SYS_ADMIN",
        "SYS_BOOT",
        "SYS_NICE",
        "SYS_RESOURCE",
        "SYS_TIME",
        "SYS_TTY_CONFIG",
        "MKNOD",
        "LEASE",
        "AUDIT_WRITE",
        "AUDIT_CONTROL",
        "SETFCAP",
    ];
    (0..32)
        .filter(|bit| mask & (1 << bit) != 0)
        .map(|bit| format!("CAP_{}", NAMES[bit]))
        .collect::<Vec<_>>()
        .join(", ")
}

/// This process's capability sets as `/proc/self/status` shows them, for error messages.
pub(crate) fn capability_sets() -> String {
    std::fs::read_to_string("/proc/self/status")
        .map(|status| {
            status
                .lines()
                .filter(|line| line.starts_with("Cap"))
                .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|error| format!("unreadable: {error}"))
}

#[cfg(target_os = "linux")]
pub(crate) fn restrict_capabilities(keep: u32) -> io::Result<()> {
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

/// Always deny BPF when cgroup socket attribution is installed: a new user/cgroup
/// namespace could otherwise expose a writable mount and detach the marker.
/// Gateway mode also denies IPv6 sockets and io_uring (whose socket operation
/// bypasses the socket syscall filter), even if the guest acquires a new route.
#[cfg(target_os = "linux")]
unsafe fn deny_network_bypass(block_ipv6: bool) -> io::Result<()> {
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
        jump(libc::SYS_bpf as u32, 8, 0),
        jump(
            libc::SYS_io_uring_setup as u32,
            if block_ipv6 { 7 } else { 8 },
            0,
        ),
        jump(
            libc::SYS_io_uring_enter as u32,
            if block_ipv6 { 6 } else { 7 },
            0,
        ),
        jump(
            libc::SYS_io_uring_register as u32,
            if block_ipv6 { 5 } else { 6 },
            0,
        ),
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
        jump(libc::AF_INET6 as u32, if block_ipv6 { 0 } else { 1 }, 2),
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

/// The image filter when a mount broker serves the image. The image never holds `SYS_ADMIN`;
/// this closes the ways around that:
///
/// * no new user or mount namespace (`unshare`/`clone` with `CLONE_NEWUSER` or `CLONE_NEWNS`
///   fail with EPERM), so the image cannot become root of a namespace that may mount;
/// * no `setns`, so it cannot enter another process's namespaces;
/// * the new mount API, `pivot_root` and `clone3` return ENOSYS (`clone3` hides its flags in
///   memory a filter cannot read; C libraries fall back to `clone`, whose flags it can);
/// * no keyrings (`request_key`, `keyctl`, `add_key`), which reach kernel upcalls and the
///   NFS client's identity mapping;
/// * no `perf_event_open`: what it exposes depends on `perf_event_paranoid`, which the guest does
///   not guarantee.
///
/// # Safety
/// Must be called only between `fork` and `exec`, after `no_new_privs`.
#[cfg(target_os = "linux")]
pub unsafe fn deny_mount_bypass() -> io::Result<()> {
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xc000003e;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc00000b7;
    const NAMESPACES: u32 = (libc::CLONE_NEWUSER | libc::CLONE_NEWNS) as u32;
    const EPERM: u32 = 0x00050000 | libc::EPERM as u32;
    const ENOSYS: u32 = 0x00050000 | libc::ENOSYS as u32;
    const ALLOW: u32 = 0x7fff0000;
    let stmt = |code, k| libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    };
    let jump = |k: libc::c_long, jt, jf| libc::sock_filter {
        code: 0x15,
        jt,
        jf,
        k: k as u32,
    };
    let enosys = [
        libc::SYS_clone3,
        libc::SYS_fsopen,
        libc::SYS_fsconfig,
        libc::SYS_fsmount,
        libc::SYS_fspick,
        libc::SYS_move_mount,
        libc::SYS_open_tree,
        libc::SYS_mount_setattr,
        libc::SYS_pivot_root,
    ];
    let eperm = [
        libc::SYS_setns,
        libc::SYS_request_key,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_perf_event_open,
    ];
    // No allocation between fork and exec: a fixed array, filled by index.
    const CHECKS: usize = 9 + 5 + 2;
    const LEN: usize = 6 + CHECKS + 7;
    let mut filter = [stmt(0x06, ALLOW); LEN];
    filter[0] = stmt(0x20, 4);
    filter[1] = jump(ARCH as libc::c_long, 1, 0);
    filter[2] = stmt(0x06, 0x80000000); // kill a foreign ABI
    filter[3] = stmt(0x20, 0);
    // x32 numbers under AUDIT_ARCH_X86_64: kill.
    filter[4] = libc::sock_filter {
        code: 0x45,
        jt: 0,
        jf: 1,
        k: 0x40000000,
    };
    filter[5] = stmt(0x06, 0x80000000);
    // Returns after the checks: ALLOW, ENOSYS, EPERM, then the flags test.
    let allow = 6 + CHECKS;
    let to = |at: usize, target: usize| (target - at - 1) as u8;
    for (i, nr) in enosys.iter().enumerate() {
        filter[6 + i] = jump(*nr, to(6 + i, allow + 1), 0);
    }
    for (i, nr) in eperm.iter().enumerate() {
        let at = 6 + enosys.len() + i;
        filter[at] = jump(*nr, to(at, allow + 2), 0);
    }
    let at = 6 + enosys.len() + eperm.len();
    filter[at] = jump(libc::SYS_unshare, to(at, allow + 3), 0);
    filter[at + 1] = jump(libc::SYS_clone, to(at + 1, allow + 3), 0);
    filter[allow] = stmt(0x06, ALLOW);
    filter[allow + 1] = stmt(0x06, ENOSYS);
    filter[allow + 2] = stmt(0x06, EPERM);
    // Flags: the low word of argument 0 (unshare's and clone's flags on both architectures).
    filter[allow + 3] = stmt(0x20, 16);
    filter[allow + 4] = libc::sock_filter {
        code: 0x45,
        jt: 0,
        jf: 1,
        k: NAMESPACES,
    };
    filter[allow + 5] = stmt(0x06, EPERM);
    filter[allow + 6] = stmt(0x06, ALLOW);
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
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    #[test]
    #[ignore = "requires root with the Docker default capability set and setpriv"]
    fn root_image_capabilities_survive_exec_without_network_administration() {
        supervisor_can_start_bounded_root(false);
    }

    #[test]
    #[ignore = "requires root with the Docker default capability set, NET_ADMIN and setpriv"]
    fn root_image_does_not_inherit_supervisor_network_administration() {
        supervisor_can_start_bounded_root(true);
    }

    fn supervisor_can_start_bounded_root(network_admin: bool) {
        assert_eq!(unsafe { libc::geteuid() }, 0, "run this test as root");
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "cat /proc/self/status; setpriv --reuid 2000 --regid 2000 --clear-groups id -u",
        ]);
        unsafe {
            command.pre_exec(move || {
                restrict_supervisor(network_admin)?;
                drop_image_to(ExecIdentity { uid: 0, gid: 0 })?;
                let socket = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
                if socket < 0 {
                    return Err(io::Error::last_os_error());
                }
                let mark: u32 = 0xa11e;
                let result = libc::setsockopt(
                    socket,
                    libc::SOL_SOCKET,
                    libc::SO_MARK,
                    (&mark as *const u32).cast(),
                    size_of::<u32>() as _,
                );
                let error = io::Error::last_os_error();
                libc::close(socket);
                if result == 0 || error.raw_os_error() != Some(libc::EPERM) {
                    return Err(io::Error::from_raw_os_error(libc::EINVAL));
                }
                Ok(())
            });
        }
        let output = command
            .output()
            .expect("bounded root can exec, but cannot set SO_MARK");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("process status is UTF-8");
        for set in ["CapPrm", "CapEff", "CapBnd"] {
            assert!(
                stdout.contains(&format!("{set}:\t00000000a80405fb")),
                "{stdout}"
            );
        }
        for set in ["CapInh", "CapAmb"] {
            assert!(
                stdout.contains(&format!("{set}:\t0000000000000000")),
                "{stdout}"
            );
        }
        assert!(stdout.contains("Uid:\t0\t0\t0\t0"), "{stdout}");
        assert!(
            stdout.ends_with("2000\n"),
            "root must be able to start a worker: {stdout}"
        );
    }

    #[test]
    #[ignore = "requires an isolated writable cgroup2 mount and privileged bootstrap"]
    fn cgroup_attribution_filters_bpf_without_disabling_ipv6() {
        let temp = tempfile::tempdir().unwrap();
        let existing =
            std::ffi::CString::new(temp.path().join("block").as_os_str().as_encoded_bytes())
                .unwrap();
        let fresh = std::ffi::CString::new(
            temp.path()
                .join("fresh-block")
                .as_os_str()
                .as_encoded_bytes(),
        )
        .unwrap();
        assert_eq!(
            unsafe {
                libc::mknod(
                    existing.as_ptr(),
                    libc::S_IFBLK | 0o600,
                    libc::makedev(1, 0),
                )
            },
            0
        );
        crate::gateway_egress::isolate_control_callers().unwrap();
        for uid in [0, 60000] {
            let mut child = Command::new("/bin/true");
            let existing = existing.clone();
            let fresh = fresh.clone();
            unsafe {
                child.pre_exec(move || {
                    restrict_supervisor(true)?;
                    drop_image_to(ExecIdentity { uid, gid: uid })?;
                    // Privileged test containers begin without a seccomp filter. Every
                    // guest path must install one even when IPv6 remains unrestricted.
                    if libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0) != 2 {
                        return Err(io::Error::from_raw_os_error(libc::EINVAL));
                    }
                    if libc::syscall(libc::SYS_bpf, -1, std::ptr::null::<u8>(), 0) != -1
                        || *libc::__errno_location() != libc::EPERM
                    {
                        return Err(io::Error::from_raw_os_error(libc::EINVAL));
                    }
                    if uid == 0 {
                        if libc::open(existing.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) != -1
                            || *libc::__errno_location() != libc::EPERM
                            || libc::mknod(
                                fresh.as_ptr(),
                                libc::S_IFBLK | 0o600,
                                libc::makedev(1, 0),
                            ) != -1
                            || *libc::__errno_location() != libc::EPERM
                        {
                            return Err(io::Error::from_raw_os_error(libc::EINVAL));
                        }
                    }
                    let socket = libc::socket(libc::AF_INET6, libc::SOCK_STREAM, 0);
                    if socket < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    libc::close(socket);
                    Ok(())
                });
            }
            assert!(child
                .status()
                .expect("BPF filter with working IPv6")
                .success());
        }
    }
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
