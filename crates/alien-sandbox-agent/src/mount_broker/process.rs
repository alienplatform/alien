//! The broker process, the agent's handle on it, and the join step the image spawn path uses.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd},
        unix::net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

use super::{
    ops,
    request::{self, Errno, Plan, Reply},
};
use crate::image_permissions::Declared;

/// The image's cgroup, below the guest cgroup.
pub const IMAGE_CGROUP: &str = "/sys/fs/cgroup/alien-guest/image";
/// Its name as `/proc/<pid>/cgroup` shows it.
pub const IMAGE_CGROUP_NAME: &str = "/alien-guest/image";

/// The image's lifecycle steps run in this cgroup below the image's, so a step that leaves its
/// process group is still found and killed at its deadline.
pub const STEPS_CGROUP: &str = "steps";
/// The socket mark the image cgroup's marker sets. The guest firewall treats it as guest traffic
/// apart from the declared connections; the agent's peer check refuses any non-zero mark.
pub const IMAGE_MARK: u32 = 0xa120;

/// How long a mount request may take before its helper is killed.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

/// How long the pause detach may take. With the lifecycle step's own deadline and the broker
/// finishing the request it is reading, it fits the provider's 60 s hook.
pub const DETACH_DEADLINE: Duration = Duration::from_secs(12);

/// How long a caller has to send its whole request line.
const REQUEST_READ: Duration = Duration::from_secs(2);

/// Added to the broker's own bounds before the agent stops waiting for an answer.
const ANSWER_SLACK: Duration = Duration::from_secs(10);

/// Capabilities the broker keeps: mount (SYS_ADMIN), enter a mount namespace (SYS_CHROOT with
/// SYS_ADMIN), read image processes' descriptors (SYS_PTRACE, DAC_READ_SEARCH), write the cgroup
/// (DAC_OVERRIDE) and kill a helper that missed its deadline (KILL).
const BROKER_CAPABILITIES: u32 = (1 << 21) | (1 << 18) | (1 << 19) | (1 << 2) | (1 << 1) | (1 << 5);

/// Held until the image is about to start, to seal the kernel's helpers (SETPCAP and SYS_MODULE
/// write `usermodehelper/bset` and `inheritable`).
const SEAL_CAPABILITIES: u32 = (1 << 8) | (1 << 16);

/// Kept until the seal when the agent has it: sets the immutable attribute on `/` and `/usr`.
const IMMUTABLE_CAPABILITY: u32 = 1 << 9;

/// Where the kernel finds its helpers (`/sbin/request-key`, `/sbin/modprobe`); read-only in the
/// image's mount namespace. The `/dev/null` bound over [`KERNEL_HELPER`] in the kernel's own
/// namespace covers the file the path names there: the image must not be able to unlink or rename
/// that file in its own view and put a new one at the path. `/usr/sbin` first: `/sbin` is often a
/// link to it.
const SYSTEM_PATHS: &[(&str, ops::SystemPath)] = &[
    ("/usr/sbin", ops::SystemPath::Directory),
    ("/sbin", ops::SystemPath::Directory),
];

/// Directories whose entries the image must not create, remove or rename: the image could
/// otherwise move `/usr` aside or replace the `/sbin` link and put its own `sbin` in their place,
/// outside the read-only binds.
const IMMUTABLE_DIRECTORIES: &[&str] = &["/", "/usr"];

/// The kernel settings for the programs it runs on its own (`request-key`, `modprobe`). The
/// image's NFS server can make the kernel call `request-key` (an ID mapping or a DNS referral), and
/// the image can write the files that choose what that runs. Before the image starts, the broker
/// empties the capability sets every such program gets and stops module loading by program (the
/// filesystems the broker mounts are loaded first).
#[derive(Debug, Clone)]
pub struct HelperSettings {
    pub bset: PathBuf,
    pub inheritable: PathBuf,
    pub modprobe: PathBuf,
}

impl Default for HelperSettings {
    fn default() -> Self {
        Self {
            bset: "/proc/sys/kernel/usermodehelper/bset".into(),
            inheritable: "/proc/sys/kernel/usermodehelper/inheritable".into(),
            modprobe: "/proc/sys/kernel/modprobe".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    /// The image cgroup directory in the broker's own (writable) view.
    pub cgroup_dir: PathBuf,
    /// The image cgroup as `/proc/<pid>/cgroup` names it.
    pub cgroup_name: String,
    pub deadline: Duration,
    /// Host-wide kernel settings; tests point them elsewhere.
    pub helpers: HelperSettings,
    /// The image's `/` for [`SYSTEM_PATHS`] and [`IMMUTABLE_DIRECTORIES`]; tests use a stand-in.
    pub system_root: PathBuf,
    /// The mount namespace the kernel starts its own helpers (`request-key`) in: the one of
    /// `kthreadd`, which [`HelperNamespace::comm`] names. Tests use their own.
    pub helper_namespace: HelperNamespace,
}

/// Where [`KERNEL_HELPER`] is blocked: a `/proc/<pid>/ns/mnt` path and, when set, the `comm` file
/// that must say `kthreadd` (the PID is checked to be the kernel's helper parent, not reused).
#[derive(Debug, Clone)]
pub struct HelperNamespace {
    pub namespace: PathBuf,
    pub comm: Option<PathBuf>,
}

impl Default for HelperNamespace {
    fn default() -> Self {
        Self {
            namespace: "/proc/2/ns/mnt".into(),
            comm: Some("/proc/2/comm".into()),
        }
    }
}

/// The program the kernel runs for a key request (an NFS ID mapping or DNS referral the image's
/// server can cause), as root, outside the image's cgroup and filter, from the root filesystem the
/// image shares. Blocked before the image starts so that nothing the image writes (a loader
/// preload, a library, a configuration) can run there; NFS with `sec=sys`, numeric IDs and an IP
/// address does not need it (an ID mapping that cannot run falls back to `nobody`).
const KERNEL_HELPER: &str = "/sbin/request-key";

impl Default for Config {
    fn default() -> Self {
        Self {
            cgroup_dir: IMAGE_CGROUP.into(),
            cgroup_name: IMAGE_CGROUP_NAME.into(),
            deadline: REQUEST_DEADLINE,
            helpers: HelperSettings::default(),
            system_root: "/".into(),
            helper_namespace: HelperNamespace::default(),
        }
    }
}

/// The agent's end of the broker.
#[derive(Debug)]
pub struct Broker {
    control: OwnedFd,
    join: OwnedFd,
    pid: i32,
    /// How long the agent waits for any broker answer: the broker may first finish one request
    /// (its deadline) and one request read.
    answer_within: libc::timeval,
}

/// Bounds blocking receives on `socket` by `timeout`.
///
/// Async-signal-safe: used in `pre_exec`.
fn receive_timeout(socket: RawFd, timeout: &libc::timeval) -> io::Result<()> {
    // SAFETY: a valid descriptor and a correctly sized timeval.
    check(unsafe {
        libc::setsockopt(
            socket,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (timeout as *const libc::timeval).cast(),
            size_of::<libc::timeval>() as libc::socklen_t,
        )
    })
}

static BROKER: OnceLock<Broker> = OnceLock::new();

/// Whether a broker serves this sandbox.
pub fn active() -> bool {
    BROKER.get().is_some()
}

/// The running broker's process ID.
pub fn pid() -> Option<i32> {
    BROKER.get().map(|b| b.pid)
}

fn check(rc: libc::c_int) -> io::Result<()> {
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn seqpacket_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: fds has room for two descriptors.
    check(unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    })?;
    // SAFETY: two descriptors the kernel just returned.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Forks the broker. Must be called while the agent is single-threaded and still holds its full
/// grant, before it joins the guest cgroup, and at most once.
pub fn start(declared: &Declared, config: Config) -> io::Result<()> {
    let threads = std::fs::read_to_string("/proc/self/status")?
        .lines()
        .find_map(|l| l.strip_prefix("Threads:").map(|n| n.trim().to_string()));
    if threads.as_deref() != Some("1") {
        return Err(io::Error::other(
            "the mount broker must be forked while the agent is single-threaded",
        ));
    }
    let (control_agent, control_broker) = seqpacket_pair()?;
    let (join_agent, join_broker) = seqpacket_pair()?;
    let parent = std::process::id() as i32;
    // SAFETY: single-threaded (checked above), so the child may allocate.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        drop((control_agent, join_agent));
        let code = match broker_main(declared, &config, &control_broker, join_broker, parent) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("alien-mount-broker: {error}");
                // The agent waits for the broker's start: tell it why there is none.
                let mut answer = b"e".to_vec();
                answer.extend(error.to_string().bytes().take(500));
                // SAFETY: writing from a buffer we own, at most its length.
                unsafe {
                    libc::send(
                        control_broker.as_raw_fd(),
                        answer.as_ptr().cast(),
                        answer.len(),
                        libc::MSG_NOSIGNAL,
                    )
                };
                1
            }
        };
        // SAFETY: leaving the forked child without running the parent's destructors.
        unsafe { libc::_exit(code) };
    }
    drop((control_broker, join_broker));
    let answer_within = libc::timeval {
        tv_sec: (config.deadline + REQUEST_READ + ANSWER_SLACK).as_secs() as libc::time_t,
        tv_usec: 0,
    };
    receive_timeout(control_agent.as_raw_fd(), &answer_within)?;
    if let Err(error) = wait_ack(control_agent.as_raw_fd(), "to start") {
        // SAFETY: reaping our own child, which ends right after its answer.
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
        return Err(error);
    }
    BROKER
        .set(Broker {
            control: control_agent,
            join: join_agent,
            pid,
            answer_within,
        })
        .map_err(|_| io::Error::other("the mount broker is started only once"))
}

/// Sends `tag` with descriptors over a seqpacket socket.
fn send_with_fds(socket: RawFd, tag: u8, fds: &[RawFd]) -> io::Result<()> {
    #[repr(C, align(8))]
    struct Space([u8; 64]);
    let mut space = Space([0; 64]);
    let mut byte = [tag];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: plain C structures, zeroed then filled in; the control buffer is large and aligned
    // enough for up to four descriptors.
    unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        if !fds.is_empty() {
            let bytes = (fds.len() * size_of::<RawFd>()) as u32;
            message.msg_control = space.0.as_mut_ptr().cast();
            message.msg_controllen = libc::CMSG_SPACE(bytes) as _;
            let header = libc::CMSG_FIRSTHDR(&message);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(bytes) as _;
            std::ptr::copy_nonoverlapping(
                fds.as_ptr(),
                libc::CMSG_DATA(header).cast::<RawFd>(),
                fds.len(),
            );
        }
        if libc::sendmsg(socket, &message, libc::MSG_NOSIGNAL) != 1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// What arrived: the tag, the sender's (pid, uid) when credentials were passed, and descriptors.
struct Received {
    tag: u8,
    credentials: Option<(i32, u32)>,
    fds: Vec<OwnedFd>,
}

fn receive(socket: RawFd) -> io::Result<Option<Received>> {
    #[repr(C, align(8))]
    struct Space([u8; 128]);
    let mut space = Space([0; 128]);
    let mut byte = [0u8];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    // SAFETY: as in `send_with_fds`; every header is bounds-checked by the CMSG macros.
    unsafe {
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = space.0.as_mut_ptr().cast();
        message.msg_controllen = space.0.len() as _;
        let n = libc::recvmsg(socket, &mut message, libc::MSG_CMSG_CLOEXEC);
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        if n == 0 {
            return Ok(None);
        }
        let mut received = Received {
            tag: byte[0],
            credentials: None,
            fds: Vec::new(),
        };
        let mut header = libc::CMSG_FIRSTHDR(&message);
        while !header.is_null() {
            let data = libc::CMSG_DATA(header);
            let len = (*header).cmsg_len as usize - (data as usize - header as usize);
            if (*header).cmsg_level == libc::SOL_SOCKET {
                if (*header).cmsg_type == libc::SCM_RIGHTS {
                    for i in 0..len / size_of::<RawFd>() {
                        let fd = std::ptr::read_unaligned(data.cast::<RawFd>().add(i));
                        received.fds.push(OwnedFd::from_raw_fd(fd));
                    }
                } else if (*header).cmsg_type == libc::SCM_CREDENTIALS
                    && len >= size_of::<libc::ucred>()
                {
                    let credentials = std::ptr::read_unaligned(data.cast::<libc::ucred>());
                    received.credentials = Some((credentials.pid, credentials.uid));
                }
            }
            header = libc::CMSG_NXTHDR(&message, header);
        }
        Ok(Some(received))
    }
}

/// Discards answers that arrived after an earlier wait gave up, so they cannot be taken for the
/// answer to the next message.
fn drain_late_acks(socket: RawFd) {
    let mut ack = [0u8];
    // SAFETY: reading one byte into a buffer we own, without blocking.
    while unsafe { libc::recv(socket, ack.as_mut_ptr().cast(), 1, libc::MSG_DONTWAIT) } == 1 {}
}

fn wait_ack(socket: RawFd, what: &str) -> io::Result<String> {
    let mut ack = [0u8; 2048];
    // SAFETY: reading into a buffer we own, at most its length.
    let n = unsafe { libc::recv(socket, ack.as_mut_ptr().cast(), ack.len(), 0) };
    if n < 0 {
        let error = io::Error::last_os_error();
        return Err(io::Error::new(
            error.kind(),
            format!("the mount broker did not answer {what}: {error}"),
        ));
    }
    ack_result(&ack[..n as usize], what)
}

/// `k` followed by what the broker reports (may be empty), or `e` followed by its reason.
fn ack_result(ack: &[u8], what: &str) -> io::Result<String> {
    match ack.split_first() {
        Some((b'k', report)) => Ok(String::from_utf8_lossy(report).into_owned()),
        Some((b'e', reason)) if !reason.is_empty() => Err(io::Error::other(format!(
            "the mount broker refused {what}: {}",
            String::from_utf8_lossy(reason)
        ))),
        _ => Err(io::Error::other(format!("the mount broker refused {what}"))),
    }
}

/// Creates the broker's socket at `path` in the agent's current mount namespace (the one the
/// image runs in) and hands it, with that namespace, to the broker. Called before the image
/// starts. Returns the broker's report of the seal.
pub fn serve_socket(path: &Path) -> io::Result<String> {
    let broker = BROKER
        .get()
        .ok_or_else(|| io::Error::other("no mount broker"))?;
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("the broker socket needs a directory"))?;
    // Not through a symbolic link the image's files may hold (`/run -> ...`): every directory on
    // the way is a real one.
    for ancestor in directory
        .ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .skip(1)
    {
        match std::fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "{} is not a directory (a symbolic link?)",
                    ancestor.display()
                )))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir(ancestor)?;
            }
            Err(error) => return Err(error),
        }
    }
    let meta = std::fs::symlink_metadata(directory)?;
    {
        use std::os::unix::fs::MetadataExt;
        if !meta.is_dir() || meta.uid() != 0 {
            return Err(io::Error::other(format!(
                "{} must be a root-owned directory",
                directory.display()
            )));
        }
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    // Root-only from the moment it exists: a umask, not a chmod after bind.
    // SAFETY: umask cannot fail.
    let previous = unsafe { libc::umask(0o177) };
    let listener = UnixListener::bind(path);
    // SAFETY: as above.
    unsafe { libc::umask(previous) };
    let listener = listener?;
    let namespace = std::fs::File::open("/proc/self/ns/mnt")?;
    let _serial = CONTROL
        .lock()
        .map_err(|_| io::Error::other("broker control poisoned"))?;
    drain_late_acks(broker.control.as_raw_fd());
    send_with_fds(
        broker.control.as_raw_fd(),
        b'S',
        &[listener.as_raw_fd(), namespace.as_raw_fd()],
    )?;
    wait_ack(broker.control.as_raw_fd(), "its socket")
}

/// Empties the capability sets of the programs the kernel runs on its own and stops module
/// loading by program. A kernel without module support has no `modprobe` setting: nothing to
/// stop there. Every other failure names its step and keeps the image from starting.
fn seal_helper_settings(helpers: &HelperSettings) -> Result<(), String> {
    let write = |step: &str, path: &Path, value: &str| {
        // Never creates the file: a kernel setting either exists or the step fails.
        std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .and_then(|mut file| file.write_all(value.as_bytes()))
            .map_err(|error| format!("step {step} ({}): {error}", path.display()))
    };
    write("usermodehelper bset", &helpers.bset, "0\t0\n")?;
    write("usermodehelper inheritable", &helpers.inheritable, "0\t0\n")?;
    match std::fs::symlink_metadata(&helpers.modprobe) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            eprintln!(
                "mount broker: {} does not exist (no module loading in this kernel); nothing to seal there",
                helpers.modprobe.display()
            );
            Ok(())
        }
        _ => write("modprobe", &helpers.modprobe, "\n"),
    }
}

/// The helper settings as the kernel now reads them back, for the boot report.
fn helper_settings_report(helpers: &HelperSettings) -> String {
    let read = |path: &Path| match std::fs::read_to_string(path) {
        Ok(value) => format!("{:?}", value.trim_end()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => "absent".to_string(),
        Err(error) => format!("unreadable ({error})"),
    };
    format!(
        "usermodehelper bset {}; usermodehelper inheritable {}; modprobe {}",
        read(&helpers.bset),
        read(&helpers.inheritable),
        read(&helpers.modprobe)
    )
}

static CONTROL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Asks the broker to detach every NFS mount in the image's mount namespace (blocking; bounded by
/// [`DETACH_DEADLINE`] and the request the broker may be reading).
pub fn detach_nfs_mounts() -> io::Result<()> {
    let broker = BROKER
        .get()
        .ok_or_else(|| io::Error::other("no mount broker"))?;
    let _ = broker;
    // The broker may first finish reading one request and end the one in flight.
    control_message(
        b'D',
        "to detach the NFS mounts",
        DETACH_DEADLINE + REQUEST_READ * 2 + Duration::from_secs(1),
    )
}

/// Binds `/dev/null` over [`KERNEL_HELPER`] in the kernel's helper namespace, from a forked child
/// (`setns` needs a single-threaded caller, and the broker keeps its own namespace).
fn block_kernel_helper(target: &HelperNamespace, root: &str) -> Result<String, String> {
    if let Some(comm) = &target.comm {
        let name = std::fs::read_to_string(comm)
            .map_err(|error| format!("cannot read {}: {error}", comm.display()))?;
        if name.trim() != "kthreadd" {
            return Err(format!(
                "{} is {:?}, not kthreadd: the kernel's helper namespace is not known",
                comm.display(),
                name.trim()
            ));
        }
    }
    let namespace = std::fs::File::open(&target.namespace)
        .map_err(|error| format!("cannot open {}: {error}", target.namespace.display()))?;
    let mut pipe = [0; 2];
    // SAFETY: a two-element buffer for pipe2.
    if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(format!("pipe: {}", io::Error::last_os_error()));
    }
    // SAFETY: pipe2 returned two descriptors we now own.
    let (read_end, write_end) =
        unsafe { (OwnedFd::from_raw_fd(pipe[0]), OwnedFd::from_raw_fd(pipe[1])) };
    // SAFETY: the broker is single-threaded, so the child may allocate.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(format!("fork: {}", io::Error::last_os_error()));
    }
    if pid == 0 {
        drop(read_end);
        // SAFETY: a mount namespace descriptor; the child is single-threaded.
        let result = if unsafe { libc::setns(namespace.as_raw_fd(), libc::CLONE_NEWNS) } != 0 {
            Err(format!(
                "cannot enter {}: {}",
                target.namespace.display(),
                io::Error::last_os_error()
            ))
        } else {
            ops::block_kernel_helper(root, KERNEL_HELPER)
        };
        let (code, text) = match result {
            Ok(text) => (0, text),
            Err(text) => (1, text),
        };
        // SAFETY: writing from a buffer we own to our pipe.
        unsafe {
            libc::write(
                write_end.as_raw_fd(),
                text.as_ptr().cast(),
                text.len().min(600),
            )
        };
        // SAFETY: leaving the forked child.
        unsafe { libc::_exit(code) };
    }
    drop(write_end);
    let mut status = 0;
    // SAFETY: waiting for our own child (local filesystem calls only).
    if unsafe { libc::waitpid(pid, &mut status, 0) } < 0 {
        return Err(format!("wait: {}", io::Error::last_os_error()));
    }
    let mut text = String::new();
    let _ = std::fs::File::from(read_end).read_to_string(&mut text);
    if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0 {
        Ok(text)
    } else if text.is_empty() {
        Err("the helper failed".into())
    } else {
        Err(text)
    }
}

/// Sends a control message without descriptors and waits for its answer within `within`.
fn control_message(tag: u8, what: &str, within: Duration) -> io::Result<()> {
    let broker = BROKER
        .get()
        .ok_or_else(|| io::Error::other("no mount broker"))?;
    let _serial = CONTROL
        .lock()
        .map_err(|_| io::Error::other("broker control poisoned"))?;
    let control = broker.control.as_raw_fd();
    drain_late_acks(control);
    let timeout = libc::timeval {
        tv_sec: within.as_secs() as libc::time_t,
        tv_usec: 0,
    };
    receive_timeout(control, &timeout)?;
    let result = send_with_fds(control, tag, &[])
        .and_then(|()| wait_ack(control, what))
        .map(drop);
    receive_timeout(control, &broker.answer_within)?;
    result
}

/// A pause starts: the broker refuses new mounts until [`resumed`].
pub fn pausing() -> io::Result<()> {
    control_message(b'Q', "a pause", Duration::from_secs(5))
}

/// The sandbox resumed: mounts are allowed again.
pub fn resumed() -> io::Result<()> {
    control_message(b'R', "a resume", Duration::from_secs(5))
}

/// Kills every process of the image's lifecycle steps.
pub fn kill_steps() -> io::Result<()> {
    control_message(b'K', "to kill the steps", Duration::from_secs(5))
}

/// After a step: what it left running joins the image's own processes.
pub fn end_steps() -> io::Result<()> {
    control_message(b'E', "to end the step", Duration::from_secs(5))
}

/// Moves the calling process into the image cgroup, through the broker.
///
/// # Safety
/// Async-signal-safe: meant for `pre_exec`. No allocation.
pub unsafe fn join_image_cgroup() -> io::Result<()> {
    join_cgroup(b'J')
}

/// Moves the calling process into the image's steps cgroup, through the broker.
///
/// # Safety
/// As [`join_image_cgroup`].
pub unsafe fn join_steps_cgroup() -> io::Result<()> {
    join_cgroup(b'j')
}

/// # Safety
/// As [`join_image_cgroup`].
unsafe fn join_cgroup(tag: u8) -> io::Result<()> {
    let Some(broker) = BROKER.get() else {
        return Err(io::Error::from_raw_os_error(libc::ENOTCONN));
    };
    let mut fds = [0; 2];
    if libc::socketpair(
        libc::AF_UNIX,
        libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
        0,
        fds.as_mut_ptr(),
    ) != 0
    {
        return Err(io::Error::last_os_error());
    }
    let sent = receive_timeout(fds[0], &broker.answer_within)
        .and_then(|()| send_with_fds(broker.join.as_raw_fd(), tag, &[fds[1]]));
    libc::close(fds[1]);
    let result = sent.and_then(|()| {
        let mut ack = [0u8];
        let n = libc::recv(fds[0], ack.as_mut_ptr().cast(), 1, 0);
        if n == 1 && ack[0] == b'k' {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(libc::EPERM))
        }
    });
    libc::close(fds[0]);
    result
}

/// Installs a filter that lets the broker create only `AF_UNIX` sockets.
fn deny_network() -> io::Result<()> {
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
    let mut filter = vec![
        stmt(0x20, 4),
        jump(ARCH, 1, 0),
        stmt(0x06, 0x80000000),
        stmt(0x20, 0),
    ];
    // x32 numbers come under AUDIT_ARCH_X86_64 with bit 30 set: kill that ABI.
    #[cfg(target_arch = "x86_64")]
    filter.extend([
        libc::sock_filter {
            code: 0x35,
            jt: 0,
            jf: 1,
            k: 0x40000000,
        },
        stmt(0x06, 0x80000000),
    ]);
    filter.extend([
        jump(libc::SYS_socket as u32, 0, 3),
        stmt(0x20, 16),
        jump(libc::AF_UNIX as u32, 1, 0),
        stmt(0x06, 0x00050000 | libc::EPERM as u32),
        stmt(0x06, 0x7fff0000),
    ]);
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };
    // SAFETY: a valid classic BPF program; no_new_privs is already set.
    check(unsafe { libc::prctl(libc::PR_SET_SECCOMP, 2, &program, 0, 0) })
}

fn close_other_fds(keep: &[RawFd]) {
    let fds: Vec<RawFd> = std::fs::read_dir("/proc/self/fd")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    for fd in fds {
        if fd > 2 && !keep.contains(&fd) {
            // SAFETY: closing a descriptor this process inherited and does not use.
            unsafe { libc::close(fd) };
        }
    }
}

struct Inflight {
    connection: UnixStream,
    helper: i32,
    pipe: std::fs::File,
    deadline: Instant,
    plan: Plan,
}

fn broker_main(
    declared: &Declared,
    config: &Config,
    control: &OwnedFd,
    join: OwnedFd,
    parent: i32,
) -> io::Result<()> {
    // What the agent was started with, for every error that depends on it.
    let boot_capabilities = crate::privilege::capability_sets();
    let permitted = crate::privilege::permitted_capabilities()?;
    let missing = BROKER_CAPABILITIES & !permitted;
    if missing != 0 {
        return Err(io::Error::other(format!(
            "the broker needs {} and the agent was started without it ({boot_capabilities})",
            crate::privilege::capability_names(missing)
        )));
    }
    // SAFETY: plain prctl/unshare/mount calls on this process.
    unsafe {
        check(libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL))?;
        if libc::getppid() != parent {
            return Ok(());
        }
        check(libc::prctl(libc::PR_SET_NAME, c"alien-mount-brk".as_ptr()))?;
        // Image root must not reach the broker's descriptors or root through /proc.
        check(libc::prctl(libc::PR_SET_DUMPABLE, 0))?;
        // A mount namespace of its own: the agent later makes the cgroup mount read-only in its
        // namespace, and the broker keeps a writable copy to move image processes into the
        // image cgroup.
        check(libc::unshare(libc::CLONE_NEWNS))?;
        check(libc::mount(
            std::ptr::null(),
            c"/".as_ptr(),
            std::ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            std::ptr::null(),
        ))?;
    }
    close_other_fds(&[control.as_raw_fd(), join.as_raw_fd()]);
    // The seal's capabilities only where the agent has them: without them the seal fails, by
    // name, when the image is about to start.
    crate::privilege::restrict_capabilities(
        BROKER_CAPABILITIES | ((SEAL_CAPABILITIES | IMMUTABLE_CAPABILITY) & permitted),
    )?;
    deny_network()?;
    let one: libc::c_int = 1;
    // SAFETY: a valid descriptor and option.
    check(unsafe {
        libc::setsockopt(
            join.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PASSCRED,
            (&one as *const libc::c_int).cast(),
            size_of::<libc::c_int>() as _,
        )
    })?;
    // Started: the agent waits for this before it relies on the broker.
    // SAFETY: writing one byte we own.
    if unsafe {
        libc::send(
            control.as_raw_fd(),
            b"k".as_ptr().cast(),
            1,
            libc::MSG_NOSIGNAL,
        )
    } != 1
    {
        return Err(io::Error::last_os_error());
    }
    Loop {
        declared,
        config,
        listener: None,
        namespace: None,
        made: BTreeMap::new(),
        inflight: None,
        zombies: Vec::new(),
        sealed: None,
        paused: false,
        boot_capabilities,
    }
    .run(control, &join)
}

struct Loop<'a> {
    declared: &'a Declared,
    config: &'a Config,
    listener: Option<UnixListener>,
    namespace: Option<OwnedFd>,
    /// Every mount the broker made that still exists, with its kind (`nfs`, `tmpfs`, `bind`).
    made: BTreeMap<u64, &'static str>,
    inflight: Option<Inflight>,
    zombies: Vec<i32>,
    /// The kernel's helpers are sealed (once, before the image first starts), with what was done.
    sealed: Option<String>,
    /// From the start of a pause until the resume: no new mounts.
    paused: bool,
    /// The capability sets the broker started with (`/proc/self/status`), for seal errors.
    boot_capabilities: String,
}

impl Loop<'_> {
    fn run(&mut self, control: &OwnedFd, join: &OwnedFd) -> io::Result<()> {
        loop {
            let mut polls = vec![
                libc::pollfd {
                    fd: control.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: join.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let mut third = None;
            if let Some(inflight) = &self.inflight {
                third = Some("pipe");
                polls.push(libc::pollfd {
                    fd: inflight.pipe.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                });
            } else if let Some(listener) = &self.listener {
                third = Some("listener");
                polls.push(libc::pollfd {
                    fd: listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                });
            }
            let timeout = match &self.inflight {
                Some(inflight) => inflight
                    .deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(1000) as i32,
                None => 1000,
            };
            // SAFETY: polls is a valid array of pollfd.
            let n = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as _, timeout) };
            if n < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if polls[0].revents & libc::POLLIN != 0 {
                match receive(control.as_raw_fd())? {
                    None => return Ok(()),
                    Some(message) => self.control(control, message),
                }
            } else if polls[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                // The agent is gone.
                return Ok(());
            }
            if polls[1].revents & libc::POLLIN != 0 {
                if let Some(message) = receive(join.as_raw_fd())? {
                    self.join(message);
                }
            }
            if let Some(revents) = polls.get(2).map(|p| p.revents) {
                if revents != 0 && third == Some("listener") {
                    self.accept();
                } else if revents != 0 && third == Some("pipe") {
                    self.finish();
                }
            }
            if self
                .inflight
                .as_ref()
                .is_some_and(|inflight| Instant::now() >= inflight.deadline)
            {
                self.expire();
            }
            self.zombies.retain(|pid| {
                // SAFETY: reaping our own child without blocking.
                unsafe { libc::waitpid(*pid, std::ptr::null_mut(), libc::WNOHANG) == 0 }
            });
        }
    }

    fn control(&mut self, control: &OwnedFd, mut message: Received) {
        if message.tag == b'D' {
            self.detach_nfs();
        }
        match message.tag {
            b'Q' => self.paused = true,
            b'R' => self.paused = false,
            b'K' => self.kill_steps(),
            b'E' => self.end_steps(),
            _ => {}
        }
        let mut ok = matches!(message.tag, b'D' | b'Q' | b'R' | b'K' | b'E')
            || (message.tag == b'S' && message.fds.len() == 2);
        let mut reason = String::new();
        if message.tag == b'S' && ok {
            match self.seal(message.fds[1].as_raw_fd()) {
                // On success the ack carries the seal's report for the agent's boot report.
                Ok(report) => reason = report,
                Err(error) => {
                    reason = format!("cannot seal the kernel's helpers: {error}");
                    eprintln!("mount broker: {reason}");
                    ok = false;
                }
            }
        }
        if message.tag == b'S' && ok {
            let namespace = message.fds.pop().expect("two descriptors");
            let listener = message.fds.pop().expect("two descriptors");
            // SAFETY: the agent sent a listening unix socket.
            self.listener = Some(unsafe { UnixListener::from_raw_fd(listener.into_raw_fd()) });
            self.namespace = Some(namespace);
        }
        let mut ack = vec![if ok { b'k' } else { b'e' }];
        ack.extend(reason.bytes().take(2000));
        // SAFETY: writing from a buffer we own, at most its length.
        unsafe {
            libc::send(
                control.as_raw_fd(),
                ack.as_ptr().cast(),
                ack.len(),
                libc::MSG_NOSIGNAL,
            )
        };
    }

    /// Before the image first starts: loads the filesystems the broker mounts, makes what the
    /// kernel reads to run a helper ([`SYSTEM_PATHS`]) read-only in the image's mount namespace,
    /// sets the immutable attribute on [`IMMUTABLE_DIRECTORIES`] (best effort, reported), empties
    /// the capability sets of every program the kernel runs on its own and stops module loading
    /// by program, then drops the capabilities that took. Any other failure keeps the image from
    /// starting.
    fn seal(&mut self, namespace: RawFd) -> Result<String, String> {
        if let Some(report) = &self.sealed {
            return Ok(report.clone());
        }
        // The kernel lets a process write `usermodehelper/*` only with both SETPCAP and SYS_MODULE.
        let held = crate::privilege::permitted_capabilities()
            .map_err(|error| format!("step usermodehelper bset: capget: {error}"))?;
        if held & SEAL_CAPABILITIES != SEAL_CAPABILITIES {
            return Err(format!(
                "step usermodehelper bset: writing it needs CAP_SETPCAP and CAP_SYS_MODULE, and the agent was started without {} ({})",
                crate::privilege::capability_names(SEAL_CAPABILITIES & !held),
                self.boot_capabilities
            ));
        }
        let blocked = block_kernel_helper(
            &self.config.helper_namespace,
            &self.config.system_root.to_string_lossy(),
        )
        .map_err(|reason| format!("step request-key: {reason}"))?;
        ops::load_filesystems();
        let mut pipe = [0; 2];
        // SAFETY: a two-element buffer for pipe2.
        if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(format!(
                "step read-only system paths: pipe: {}",
                io::Error::last_os_error()
            ));
        }
        // SAFETY: pipe2 returned two descriptors we now own.
        let (read_end, write_end) =
            unsafe { (OwnedFd::from_raw_fd(pipe[0]), OwnedFd::from_raw_fd(pipe[1])) };
        // SAFETY: the broker is single-threaded, so the child may allocate.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(format!(
                "step read-only system paths: fork: {}",
                io::Error::last_os_error()
            ));
        }
        if pid == 0 {
            drop(read_end);
            // SAFETY: a mount namespace descriptor; the child is single-threaded.
            let failure = if unsafe { libc::setns(namespace, libc::CLONE_NEWNS) } != 0 {
                Some(format!(
                    "cannot enter the image's mount namespace: {}",
                    io::Error::last_os_error()
                ))
            } else {
                let root = self.config.system_root.to_string_lossy();
                match ops::protect_system_paths(&root, SYSTEM_PATHS) {
                    // Created what was missing first: nothing new is needed at the top level now.
                    Ok(protected) => {
                        let mut report = format!("read-only: {}", protected.join(", "));
                        // Before the immutable attribute, which would keep them from appearing.
                        let declared: Vec<&str> = self
                            .declared
                            .permissions
                            .mounts
                            .iter()
                            .map(|mount| mount.path.as_str())
                            .collect();
                        for line in ops::create_declared_paths(&root, &declared) {
                            report.push_str("; ");
                            report.push_str(&line);
                        }
                        for line in ops::make_immutable(&root, IMMUTABLE_DIRECTORIES) {
                            report.push_str("; ");
                            report.push_str(&line);
                        }
                        // SAFETY: writing from a buffer we own to our pipe.
                        unsafe {
                            libc::write(
                                write_end.as_raw_fd(),
                                report.as_ptr().cast(),
                                report.len().min(1000),
                            )
                        };
                        None
                    }
                    Err(reason) => Some(reason),
                }
            };
            let code = match failure {
                None => 0,
                Some(reason) => {
                    // SAFETY: writing from a buffer we own to our pipe.
                    unsafe {
                        libc::write(
                            write_end.as_raw_fd(),
                            reason.as_ptr().cast(),
                            reason.len().min(400),
                        )
                    };
                    1
                }
            };
            // SAFETY: leaving the forked child.
            unsafe { libc::_exit(code) };
        }
        drop(write_end);
        let mut status = 0;
        // SAFETY: waiting for our own child (local filesystem calls only).
        if unsafe { libc::waitpid(pid, &mut status, 0) } < 0 {
            return Err(format!(
                "step read-only system paths: wait: {}",
                io::Error::last_os_error()
            ));
        }
        let mut report = String::new();
        let _ = std::fs::File::from(read_end).read_to_string(&mut report);
        if !libc::WIFEXITED(status) || libc::WEXITSTATUS(status) != 0 {
            return Err(format!(
                "step read-only system paths: {}",
                if report.is_empty() {
                    "the helper failed"
                } else {
                    &report
                }
            ));
        }
        let report = format!("request-key: {blocked}; {report}");
        eprintln!("mount broker: sealed: {report}");
        seal_helper_settings(&self.config.helpers)?;
        crate::privilege::restrict_capabilities(BROKER_CAPABILITIES).map_err(|error| {
            format!("step drop SETPCAP, SYS_MODULE and LINUX_IMMUTABLE: {error}")
        })?;
        let report = format!("{report}; {}", helper_settings_report(&self.config.helpers));
        self.sealed = Some(report.clone());
        Ok(report)
    }

    /// Kills every process in the steps cgroup (`cgroup.kill`, or one by one where the kernel
    /// has none), bounded.
    fn kill_steps(&mut self) {
        let steps = self.config.cgroup_dir.join(STEPS_CGROUP);
        if std::fs::write(steps.join("cgroup.kill"), "1").is_ok() {
            return;
        }
        let until = Instant::now() + REQUEST_READ;
        while Instant::now() < until {
            let Ok(procs) = std::fs::read_to_string(steps.join("cgroup.procs")) else {
                return;
            };
            let pids: Vec<i32> = procs.lines().filter_map(|l| l.parse().ok()).collect();
            if pids.is_empty() {
                return;
            }
            for pid in pids {
                // SAFETY: signalling a process in the steps cgroup.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// After a step: what it left running (a proxy it started, say) joins the image's own
    /// processes, so a later step's deadline does not kill it.
    fn end_steps(&mut self) {
        let steps = self.config.cgroup_dir.join(STEPS_CGROUP);
        let Ok(procs) = std::fs::read_to_string(steps.join("cgroup.procs")) else {
            return;
        };
        for pid in procs.lines().filter(|l| !l.is_empty()) {
            let _ = std::fs::write(self.config.cgroup_dir.join("cgroup.procs"), pid);
        }
    }

    /// Moves the sender into the image cgroup and answers on the descriptor it sent.
    fn join(&mut self, message: Received) {
        let Some(reply) = message.fds.into_iter().next() else {
            return;
        };
        let directory = match message.tag {
            b'J' => Some(self.config.cgroup_dir.clone()),
            b'j' => {
                let steps = self.config.cgroup_dir.join(STEPS_CGROUP);
                match std::fs::create_dir(&steps) {
                    Ok(()) => Some(steps),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Some(steps),
                    Err(_) => None,
                }
            }
            _ => None,
        };
        let moved = match (message.credentials, directory) {
            (Some((pid, 0)), Some(directory)) if pid > 0 => {
                std::fs::write(directory.join("cgroup.procs"), pid.to_string()).is_ok()
            }
            _ => false,
        };
        let ack = [if moved { b'k' } else { b'e' }];
        // SAFETY: writing one byte from a buffer we own.
        unsafe {
            libc::send(
                reply.as_raw_fd(),
                ack.as_ptr().cast(),
                1,
                libc::MSG_NOSIGNAL,
            )
        };
    }

    /// Only a root process in the image cgroup may ask.
    fn peer_allowed(&self, connection: &UnixStream) -> Result<(), String> {
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: a valid descriptor and a correctly sized buffer.
        check(unsafe {
            libc::getsockopt(
                connection.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut len,
            )
        })
        .map_err(|e| format!("cannot identify the caller: {e}"))?;
        if credentials.uid != 0 {
            return Err("only the image's root may ask the broker".into());
        }
        let cgroup = std::fs::read_to_string(format!("/proc/{}/cgroup", credentials.pid))
            .map_err(|e| format!("cannot read the caller's cgroup: {e}"))?;
        let steps = format!("{}/{STEPS_CGROUP}", self.config.cgroup_name);
        if !cgroup.lines().any(|line| {
            line.strip_prefix("0::")
                .is_some_and(|name| name == self.config.cgroup_name || name == steps)
        }) {
            return Err("only the image's own processes may ask the broker".into());
        }
        Ok(())
    }

    fn accept(&mut self) {
        let Some(listener) = &self.listener else {
            return;
        };
        let Ok((mut connection, _)) = listener.accept() else {
            return;
        };
        let _ = connection.set_write_timeout(Some(REQUEST_READ));
        // One deadline for the whole request, not per read: a caller that trickles bytes must
        // not hold the broker's only thread.
        let read_by = Instant::now() + REQUEST_READ;
        let mut text = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let left = read_by.saturating_duration_since(Instant::now());
            if left.is_zero() || connection.set_read_timeout(Some(left)).is_err() {
                break;
            }
            match connection.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    text.extend_from_slice(&buffer[..n]);
                    if text.contains(&b'\n') || text.len() > 64 * 1024 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        // Checked after the request is read, so a refused caller gets its answer rather than a
        // reset connection.
        if let Err(reason) = self.peer_allowed(&connection) {
            respond(&mut connection, Reply::refused(Errno::Eperm, reason));
            return;
        }
        let line = text.split(|b| *b == b'\n').next().unwrap_or_default();
        let parsed: request::Request = match serde_json::from_slice(line) {
            Ok(parsed) => parsed,
            Err(error) => {
                respond(
                    &mut connection,
                    Reply::refused(Errno::Einval, format!("not a broker request: {error}")),
                );
                return;
            }
        };
        let plan = match request::plan(self.declared, &parsed) {
            Ok(plan) => plan,
            Err((errno, reason)) => {
                respond(&mut connection, Reply::refused(errno, reason));
                return;
            }
        };
        if self.paused && !matches!(plan, Plan::Umount { .. }) {
            respond(
                &mut connection,
                Reply::refused(
                    Errno::Ebusy,
                    "the sandbox is pausing; mounts are refused until it resumes",
                ),
            );
            return;
        }
        let Some(namespace) = &self.namespace else {
            respond(
                &mut connection,
                Reply::refused(Errno::Enoent, "the image is not running"),
            );
            return;
        };
        let deadline = Instant::now() + self.config.deadline;
        match self.fork_helper(&plan, namespace.as_raw_fd(), deadline) {
            Ok((helper, pipe)) => {
                self.inflight = Some(Inflight {
                    connection,
                    helper,
                    pipe,
                    deadline,
                    plan,
                });
            }
            Err(error) => respond(
                &mut connection,
                Reply::refused(Errno::Eio, format!("cannot start the helper: {error}")),
            ),
        }
    }

    fn fork_helper(
        &self,
        plan: &Plan,
        namespace: RawFd,
        deadline: Instant,
    ) -> io::Result<(i32, std::fs::File)> {
        let mut fds = [0; 2];
        // SAFETY: fds has room for two descriptors.
        check(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) })?;
        // SAFETY: two descriptors the kernel just returned.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        // SAFETY: the broker is single-threaded, so the child may allocate.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(io::Error::last_os_error());
        }
        if pid == 0 {
            drop(read);
            let made: BTreeSet<u64> = self.made.keys().copied().collect();
            let outcome = helper(plan, namespace, &made, &self.config.cgroup_dir, deadline);
            let mut write = std::fs::File::from(write);
            let _ = write.write_all(&serde_json::to_vec(&outcome).unwrap_or_default());
            // SAFETY: leaving the forked helper.
            unsafe { libc::_exit(0) };
        }
        drop(write);
        Ok((pid, std::fs::File::from(read)))
    }

    /// Detaches every NFS mount in the image's mount namespace, wherever it is, in a helper bound
    /// by the detach deadline.
    /// Used when the image's own pause step fails: an NFS connection does not survive the
    /// snapshot. A request still in flight is ended first, so its mount cannot land afterwards.
    fn detach_nfs(&mut self) {
        if let Some(helper) = self.inflight.as_ref().map(|inflight| inflight.helper) {
            self.expire();
            // A killed helper is gone once it leaves the kernel; wait for that, bounded, so a
            // mount it was attaching is in place (and detached below) or never made.
            let gone_by = Instant::now() + REQUEST_READ;
            while Instant::now() < gone_by {
                // SAFETY: reaping our own helper without blocking.
                if unsafe { libc::waitpid(helper, std::ptr::null_mut(), libc::WNOHANG) } != 0 {
                    self.zombies.retain(|pid| *pid != helper);
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let Some(namespace) = &self.namespace else {
            return;
        };
        let namespace = namespace.as_raw_fd();
        // The detach reports which recorded mounts are still there: a mount ID is reused once
        // its mount is gone, and a stale record would hide a new mount from the orphan sweep.
        let mut fds = [0; 2];
        // SAFETY: fds has room for two descriptors.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return;
        }
        // SAFETY: two descriptors the kernel just returned.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        // SAFETY: the broker is single-threaded, so the child may allocate.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            drop(read);
            // SAFETY: a mount namespace descriptor; the helper is single-threaded.
            let code = if unsafe { libc::setns(namespace, libc::CLONE_NEWNS) } == 0 {
                ops::detach_nfs();
                let present: Vec<u64> = ops::mount_ids().into_iter().collect();
                let _ = std::fs::File::from(write)
                    .write_all(&serde_json::to_vec(&present).unwrap_or_default());
                0
            } else {
                1
            };
            // SAFETY: leaving the forked helper.
            unsafe { libc::_exit(code) };
        }
        drop(write);
        if pid < 0 {
            return;
        }
        let deadline = Instant::now() + DETACH_DEADLINE.min(self.config.deadline);
        loop {
            // SAFETY: reaping our own helper without blocking.
            if unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) } != 0 {
                break;
            }
            if Instant::now() >= deadline {
                // SAFETY: signalling our own helper.
                unsafe { libc::kill(pid, libc::SIGKILL) };
                self.zombies.push(pid);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut text = Vec::new();
        let _ = std::fs::File::from(read).read_to_end(&mut text);
        if let Ok(present) = serde_json::from_slice::<BTreeSet<u64>>(&text) {
            self.made.retain(|id, _| present.contains(id));
        }
    }

    fn finish(&mut self) {
        let Some(mut inflight) = self.inflight.take() else {
            return;
        };
        let mut text = Vec::new();
        let _ = inflight.pipe.read_to_end(&mut text);
        // SAFETY: reaping our own helper, which has closed its end and so is exiting.
        unsafe { libc::waitpid(inflight.helper, std::ptr::null_mut(), 0) };
        let reply = match serde_json::from_slice::<ops::Outcome>(&text) {
            Ok(outcome) => {
                let alive: BTreeSet<u64> = outcome.alive.into_iter().collect();
                self.made.retain(|id, _| alive.contains(id));
                match outcome.result {
                    Ok(Some(id)) => {
                        match inflight.plan {
                            Plan::Umount { .. } => {
                                self.made.remove(&id);
                            }
                            Plan::Nfs { .. } => {
                                self.made.insert(id, "nfs");
                            }
                            Plan::Tmpfs { .. } => {
                                self.made.insert(id, "tmpfs");
                            }
                            Plan::Bind { .. } => {
                                self.made.insert(id, "bind");
                            }
                        }
                        Reply::ok(Some(id))
                    }
                    Ok(None) => Reply::ok(None),
                    Err((errno, reason)) => Reply {
                        ok: false,
                        errno: Some(errno),
                        reason: Some(reason),
                        mount_id: None,
                    },
                }
            }
            Err(_) => Reply::refused(Errno::Eio, "the helper failed"),
        };
        respond(&mut inflight.connection, reply);
    }

    fn expire(&mut self) {
        let Some(mut inflight) = self.inflight.take() else {
            return;
        };
        // SAFETY: signalling our own helper.
        unsafe { libc::kill(inflight.helper, libc::SIGKILL) };
        self.zombies.push(inflight.helper);
        respond(
            &mut inflight.connection,
            Reply::refused(
                Errno::Etimedout,
                format!(
                    "the request did not finish within {}s",
                    self.config.deadline.as_secs()
                ),
            ),
        );
    }
}

fn respond(connection: &mut UnixStream, reply: Reply) {
    let mut text = serde_json::to_vec(&reply).unwrap_or_default();
    text.push(b'\n');
    let _ = connection.write_all(&text);
}

/// Runs in the forked helper: checks that need the broker's view, then joins the image's mount
/// namespace and carries out the plan.
fn helper(
    plan: &Plan,
    namespace: RawFd,
    made: &BTreeSet<u64>,
    cgroup_dir: &Path,
    deadline: Instant,
) -> ops::Outcome {
    // SAFETY: a mount namespace descriptor; the helper is single-threaded.
    if unsafe { libc::setns(namespace, libc::CLONE_NEWNS) } != 0 {
        let error = io::Error::last_os_error();
        return ops::Outcome {
            result: Err((
                Errno::Eio.name().to_string(),
                format!("cannot enter the image's mount namespace: {error}"),
            )),
            // Nothing is known about the image's mounts from here: keep the record as it is.
            alive: made.iter().copied().collect(),
        };
    }
    // A helper killed after attaching but before reporting left a mount the broker never
    // recorded: it goes before anything else.
    ops::detach_orphans(made);
    let result = (|| {
        if let Plan::Nfs { port, .. } = plan {
            // The image's view of the (read-only) cgroup mount shows the same members.
            // Wait for a late proxy, leaving the mount its time before the deadline.
            let until = (Instant::now() + ops::LISTENER_WAIT).min(
                deadline
                    .checked_sub(ops::ATTACH_MARGIN * 2)
                    .unwrap_or(deadline),
            );
            // The image's own processes, and a step's (an `onResume` that remounts starts its proxy
            // there).
            ops::wait_for_port(
                *port,
                &[
                    cgroup_dir.join("cgroup.procs"),
                    cgroup_dir.join(STEPS_CGROUP).join("cgroup.procs"),
                ],
                until,
            )?;
        }
        ops::run(plan, made, Some(deadline))
    })();
    // Listed in the image's mount namespace, so a mount the image's view no longer has is
    // forgotten (a lazily detached parent takes its children with it).
    let present = ops::mount_ids();
    ops::Outcome {
        result: result.map_err(|(errno, reason)| (errno.name().to_string(), reason)),
        alive: made
            .iter()
            .copied()
            .filter(|id| present.contains(id))
            .collect(),
    }
}

/// Sends one request to the broker socket at `path` and returns its reply.
pub fn call(path: &Path, request: &request::Request, timeout: Duration) -> io::Result<Reply> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut text = serde_json::to_vec(request).map_err(io::Error::other)?;
    text.push(b'\n');
    stream.write_all(&text)?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer)?;
    serde_json::from_slice(answer.split(|b| *b == b'\n').next().unwrap_or_default())
        .map_err(io::Error::other)
}

/// Links the shims to the agent binary in [`super::shim::DIRECTORY`]. Runs as root before the
/// agent's files are protected and before any image code exists.
pub fn link_shims(agent: &Path) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let directory = Path::new(super::shim::DIRECTORY);
    // Not through a symbolic link the image's files may hold (`/run -> ...`): every directory on
    // the way is a real one.
    for ancestor in directory
        .ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .skip(1)
    {
        match std::fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "{} is not a directory (a symbolic link?)",
                    ancestor.display()
                )))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir(ancestor)?;
            }
            Err(error) => return Err(error),
        }
    }
    let meta = std::fs::symlink_metadata(directory)?;
    if !meta.is_dir() || meta.uid() != 0 {
        return Err(io::Error::other(format!(
            "{} must be a root-owned directory",
            directory.display()
        )));
    }
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755))?;
    for name in super::shim::NAMES {
        let link = directory.join(name);
        match std::fs::remove_file(&link) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::os::unix::fs::symlink(agent, &link)?;
    }
    Ok(())
}

static IMAGE_LEADER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Records the image command's process, whose environment lifecycle steps start from.
pub fn set_image_leader(pid: u32) {
    IMAGE_LEADER.store(pid, std::sync::atomic::Ordering::Relaxed);
}

pub fn image_leader() -> Option<u32> {
    match IMAGE_LEADER.load(std::sync::atomic::Ordering::Relaxed) {
        0 => None,
        pid => Some(pid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(dir: &Path) -> HelperSettings {
        HelperSettings {
            bset: dir.join("bset"),
            inheritable: dir.join("inheritable"),
            modprobe: dir.join("modprobe"),
        }
    }

    #[test]
    fn a_kernel_without_modprobe_seals_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let helpers = settings(dir.path());
        std::fs::write(&helpers.bset, "4294967295\t511\n").unwrap();
        std::fs::write(&helpers.inheritable, "4294967295\t511\n").unwrap();
        seal_helper_settings(&helpers).expect("no modprobe setting: nothing to seal there");
        assert_eq!(std::fs::read_to_string(&helpers.bset).unwrap(), "0\t0\n");
        assert_eq!(
            std::fs::read_to_string(&helpers.inheritable).unwrap(),
            "0\t0\n"
        );
        assert!(!helpers.modprobe.exists());
    }

    #[test]
    fn modprobe_is_emptied_when_it_exists() {
        let dir = tempfile::tempdir().unwrap();
        let helpers = settings(dir.path());
        for path in [&helpers.bset, &helpers.inheritable] {
            std::fs::write(path, "x\n").unwrap();
        }
        std::fs::write(&helpers.modprobe, "/sbin/modprobe\n").unwrap();
        seal_helper_settings(&helpers).unwrap();
        assert_eq!(std::fs::read_to_string(&helpers.modprobe).unwrap(), "\n");
    }

    #[test]
    fn a_failing_step_is_named_and_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let helpers = settings(dir.path());
        std::fs::write(&helpers.bset, "x\n").unwrap();
        std::fs::write(&helpers.modprobe, "/sbin/modprobe\n").unwrap();
        // inheritable is missing: a real failure of a step that exists.
        let error = seal_helper_settings(&helpers).unwrap_err();
        assert!(error.contains("step usermodehelper inheritable"), "{error}");
        // modprobe is a directory: writing it fails, and the step says so.
        std::fs::write(&helpers.inheritable, "x\n").unwrap();
        std::fs::remove_file(&helpers.modprobe).unwrap();
        std::fs::create_dir(&helpers.modprobe).unwrap();
        let error = seal_helper_settings(&helpers).unwrap_err();
        assert!(error.contains("step modprobe"), "{error}");
        std::fs::write(&helpers.bset, "0\t0\n").unwrap();
        std::fs::remove_dir(&helpers.modprobe).unwrap();
        assert_eq!(
            helper_settings_report(&helpers),
            "usermodehelper bset \"0\\t0\"; usermodehelper inheritable \"0\\t0\"; modprobe absent"
        );
    }

    #[test]
    fn a_refusal_carries_the_broker_reason() {
        assert_eq!(ack_result(b"k", "its socket").unwrap(), "");
        assert_eq!(
            ack_result(b"kread-only: /usr/sbin", "its socket").unwrap(),
            "read-only: /usr/sbin"
        );
        let error = ack_result(b"ecannot seal: step modprobe: EROFS", "its socket").unwrap_err();
        assert_eq!(
            error.to_string(),
            "the mount broker refused its socket: cannot seal: step modprobe: EROFS"
        );
        let error = ack_result(b"e", "its socket").unwrap_err();
        assert_eq!(error.to_string(), "the mount broker refused its socket");
        assert!(ack_result(b"", "its socket").is_err());
    }
}
