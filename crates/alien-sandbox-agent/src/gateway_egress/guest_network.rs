//! Tag sockets by process ancestry, independently of the image's chosen UIDs.
//!
//! AWS restore processes stay outside this cgroup. The agent and all its children
//! enter it before worker threads start; only trusted agent sockets override the
//! inherited mark. Guest root has neither NET_ADMIN nor NET_RAW to change marks,
//! nor SYS_ADMIN to make the original cgroup mount writable again. The inherited
//! seccomp filter denies BPF in every egress mode, including detach through a new
//! user namespace. IPv6 filtering is a separate gateway-only policy.

use std::{
    fs::{self, File},
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};

pub(super) const GUEST_MARK: u32 = 0xa11f;
const CGROUP: &str = "/sys/fs/cgroup/alien-guest";

/// Runs once during privileged bootstrap, before capabilities are reduced.
/// A missing cgroup/BPF facility is fatal; UID exemptions cannot contain a root image.
pub(super) fn install() -> io::Result<()> {
    fs::create_dir(CGROUP)?;
    let cgroup = File::open(CGROUP)?;
    let program = load_socket_marker()?;
    // Linux UAPI: BPF_PROG_ATTACH and BPF_CGROUP_INET_SOCK_CREATE. A non-override
    // attachment also applies to every descendant cgroup the guest might create.
    #[repr(C)]
    struct Attach {
        target_fd: u32,
        program_fd: u32,
        attach_type: u32,
        flags: u32,
        replace_fd: u32,
    }
    let attach = Attach {
        target_fd: cgroup.as_raw_fd() as u32,
        program_fd: program.as_raw_fd() as u32,
        attach_type: 2,
        flags: 0,
        replace_fd: 0,
    };
    // SAFETY: attach is the initialized UAPI layout, and both descriptors are live.
    if unsafe { libc::syscall(libc::SYS_bpf, 8, &attach, size_of::<Attach>()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    fs::write(
        format!("{CGROUP}/cgroup.procs"),
        std::process::id().to_string(),
    )?;
    // Read-only on this mount, not just file modes: guest root retains DAC_OVERRIDE
    // and could otherwise leave via cgroup.procs. Keep the filesystem writable
    // through provider mounts so AWS can freeze processes during suspend.
    // There are no retained writable descriptors the guest could recover via /proc.
    if unsafe {
        libc::mount(
            std::ptr::null(),
            c"/sys/fs/cgroup".as_ptr(),
            std::ptr::null(),
            libc::MS_REMOUNT
                | libc::MS_BIND
                | libc::MS_RDONLY
                | libc::MS_NOSUID
                | libc::MS_NODEV
                | libc::MS_NOEXEC,
            std::ptr::null(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    crate::privilege::block_command_bpf();
    Ok(())
}

fn load_socket_marker() -> io::Result<OwnedFd> {
    #[repr(C)]
    struct Instruction {
        opcode: u8,
        registers: u8,
        offset: i16,
        immediate: i32,
    }
    // Linux UAPI bpf_sock.mark is at byte 16. Four fixed instructions, no maps,
    // helpers, compiler, userspace daemon or external BPF artifact required.
    let instructions = [
        Instruction {
            opcode: 0xb7,
            registers: 2,
            offset: 0,
            immediate: GUEST_MARK as i32,
        }, // r2 = mark
        Instruction {
            opcode: 0x63,
            registers: 0x21,
            offset: 16,
            immediate: 0,
        }, // *(u32 *)(r1 + 16) = r2
        Instruction {
            opcode: 0xb7,
            registers: 0,
            offset: 0,
            immediate: 1,
        }, // allow socket creation
        Instruction {
            opcode: 0x95,
            registers: 0,
            offset: 0,
            immediate: 0,
        }, // exit
    ];
    #[repr(C)]
    struct Load {
        program_type: u32,
        instruction_count: u32,
        instructions: u64,
        license: u64,
        log_level: u32,
        log_size: u32,
        log_buffer: u64,
        kernel_version: u32,
        flags: u32,
    }
    let load = Load {
        program_type: 9, // BPF_PROG_TYPE_CGROUP_SOCK
        instruction_count: instructions.len() as u32,
        instructions: instructions.as_ptr() as u64,
        license: c"GPL".as_ptr() as u64,
        log_level: 0,
        log_size: 0,
        log_buffer: 0,
        kernel_version: 0,
        flags: 0,
    };
    // SAFETY: load and its pointed-to buffers remain live for BPF_PROG_LOAD.
    let fd = unsafe { libc::syscall(libc::SYS_bpf, 5, &load, size_of::<Load>()) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful BPF_PROG_LOAD returns a new owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as _) })
}
