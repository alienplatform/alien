//! What the agent creates has to be reachable by the command it runs.
//!
//! Every other test in this crate runs the agent and the command as the same user, because a test
//! process cannot become a second one. That is not the configuration that ships: inside a MicroVM
//! the agent is root — it needs `setuid` to drop — and the command runs as an unprivileged uid it
//! does not share. The whole class of bug these cover is invisible when the two coincide.
//!
//! Needs to run as root, so it is ignored by default and never runs in CI:
//!
//! ```text
//! docker run --rm --platform linux/arm64 --cap-add NET_ADMIN -v "$PWD:/work" \
//!   -e CARGO_TARGET_DIR=/tmp/target -w /work rust:1-bookworm \
//!   cargo test -p alien-sandbox-agent --test privileged -- --ignored
//! ```
//!
//! NET_ADMIN because `restrict_supervisor` keeps it, and a capset cannot keep what the process
//! does not hold.
#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use alien_sandbox_agent::exec::{stream, ExecIdentity, ExecRequest, Frame};
use alien_sandbox_agent::files;
use alien_sandbox_agent::privilege::{drop_to, restrict_supervisor};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use tempfile::TempDir;
use tokio::sync::mpsc;

/// The uid the shipped image runs commands as.
const EXEC_UID: u32 = alien_core::sandbox_image::AWS_MICROVM.exec_uid;

fn exec_identity() -> ExecIdentity {
    ExecIdentity {
        uid: EXEC_UID,
        gid: EXEC_UID,
    }
}

/// A session root shaped like the one the image builds: owned by the exec uid, and `0700` so
/// nothing but that uid and root can traverse in. Containment is here, not on the contents.
fn session_root() -> (TempDir, PathBuf) {
    // SAFETY: a getter with no arguments.
    let euid = unsafe { libc::geteuid() };
    assert_eq!(
        euid, 0,
        "this suite has to run as root to drop to a different uid; run it in a container"
    );

    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical root");

    let path = std::ffi::CString::new(root.as_os_str().as_encoded_bytes()).expect("no NUL");
    // SAFETY: a valid NUL-terminated path, and uid/gid/mode words.
    unsafe {
        assert_eq!(
            libc::chown(path.as_ptr(), EXEC_UID, EXEC_UID),
            0,
            "the session root belongs to the exec uid"
        );
        assert_eq!(libc::chmod(path.as_ptr(), 0o700), 0, "and only to it");
    }

    (dir, root)
}

/// Runs `command` as the exec identity and returns its frames.
///
/// `working_directory` is a host path, not one inside a chroot — the server resolves a caller's
/// relative paths against it the same way.
async fn run_as_exec_uid(command: &[&str], working_directory: Option<&Path>) -> Vec<Frame> {
    let request = ExecRequest {
        command: command.iter().map(|part| part.to_string()).collect(),
        timeout_ms: 30_000,
        cwd: None,
        env: BTreeMap::new(),
    };

    let (sender, mut receiver) = mpsc::channel(64);
    let directory = working_directory.map(Path::to_path_buf);
    tokio::spawn(async move {
        stream(
            &request,
            directory.as_deref(),
            exec_identity(),
            1 << 20,
            sender,
        )
        .await;
    });

    let mut frames = Vec::new();
    while let Some(frame) = receiver.recv().await {
        frames.push(frame);
    }
    frames
}

fn stdout_of(frames: &[Frame]) -> String {
    frames
        .iter()
        .filter_map(|frame| match frame {
            Frame::Stdout { data, .. } => Some(BASE64.decode(data).expect("base64")),
            _ => None,
        })
        .flatten()
        .collect::<Vec<u8>>()
        .into_iter()
        .map(|byte| byte as char)
        .collect()
}

/// The exit code, or a panic naming why the command never produced one — a spawn that failed is
/// the interesting case here and must not read as a missing frame.
fn exit_code_of(frames: &[Frame]) -> i32 {
    for frame in frames {
        match frame {
            Frame::Exit { code, .. } => return *code,
            Frame::Error { code, message } => panic!("the command did not run: {code}: {message}"),
            _ => {}
        }
    }
    panic!("a command always reports a terminal frame, got {frames:?}")
}

/// The flow the resource exists for: upload a script, then run it. The agent writes as root and
/// the command reads as 60000, so an owner-only mode makes the upload unreadable to the only code
/// that was ever going to read it.
#[tokio::test]
#[ignore = "requires running as root, e.g. inside a container"]
async fn a_file_the_agent_wrote_is_readable_by_the_command() {
    let (_dir, root) = session_root();

    files::write(&root, "/work/main.py", b"print(1)")
        .await
        .expect("the agent writes it");

    let frames = run_as_exec_uid(&["/bin/cat", "work/main.py"], Some(&root)).await;

    assert_eq!(
        exit_code_of(&frames),
        0,
        "the command must be able to read what was uploaded for it: {}",
        stdout_of(&frames)
    );
    assert_eq!(stdout_of(&frames).trim(), "print(1)");
}

/// A project is uploaded as a tree, so the command has to be able to work inside it — not merely
/// read it. Writing beside the source is what a build or an install does first.
#[tokio::test]
#[ignore = "requires running as root, e.g. inside a container"]
async fn the_command_can_write_beside_what_was_uploaded() {
    let (_dir, root) = session_root();

    files::write(&root, "/project/main.py", b"print(1)")
        .await
        .expect("the agent writes it");

    let frames = run_as_exec_uid(
        &[
            "/bin/sh",
            "-c",
            "echo built > project/out.txt && cat project/out.txt",
        ],
        Some(&root),
    )
    .await;

    assert_eq!(
        exit_code_of(&frames),
        0,
        "a directory the agent created must be writable by the command: {}",
        stdout_of(&frames)
    );
    assert_eq!(stdout_of(&frames).trim(), "built");
}

/// The chdir has to happen after the privilege drop. Performed while still root it succeeds into a
/// directory the command cannot enter, and every relative path then fails with nothing reported.
#[tokio::test]
#[ignore = "requires running as root, e.g. inside a container"]
async fn a_working_directory_the_agent_created_is_usable() {
    let (_dir, root) = session_root();

    files::write(&root, "/work/data.txt", b"payload")
        .await
        .expect("the agent creates the directory and writes into it");

    let frames = run_as_exec_uid(&["/bin/cat", "data.txt"], Some(&root.join("work"))).await;

    assert_eq!(
        exit_code_of(&frames),
        0,
        "a relative path must resolve in the working directory the caller asked for: {}",
        stdout_of(&frames)
    );
    assert_eq!(stdout_of(&frames).trim(), "payload");
}

/// The discriminating case for *when* the chdir happens. A directory the exec identity cannot
/// enter must refuse the spawn; performed while still root the chdir succeeds, the command starts
/// somewhere it cannot read, and the caller gets an ordinary non-zero exit with no cause.
///
/// Built directly here rather than through the agent, since everything the agent creates is
/// reachable by design — the point under test is the ordering, not what produced the directory.
#[tokio::test]
#[ignore = "requires running as root, e.g. inside a container"]
async fn a_working_directory_the_command_cannot_enter_refuses_the_spawn() {
    let (_dir, root) = session_root();

    let closed = root.join("closed");
    std::fs::create_dir(&closed).expect("root creates it");
    std::fs::write(closed.join("data.txt"), b"payload").expect("with something inside");
    let path = std::ffi::CString::new(closed.as_os_str().as_encoded_bytes()).expect("no NUL");
    // SAFETY: a valid NUL-terminated path and a mode word. Root-owned and owner-only, so the exec
    // identity cannot traverse it.
    unsafe {
        assert_eq!(
            libc::chmod(path.as_ptr(), 0o700),
            0,
            "closed to the exec uid"
        );
    }

    let frames = run_as_exec_uid(&["/bin/cat", "data.txt"], Some(&closed)).await;

    let refused = frames
        .iter()
        .any(|frame| matches!(frame, Frame::Error { .. }));
    assert!(
        refused,
        "entering a directory the command cannot use must fail the spawn, not hand back an \
         opaque command failure: {frames:?}"
    );
}

/// In Docker's default capability set, and the one `restrict_supervisor` keeps.
const CAP_KILL: u32 = 5;

#[repr(C)]
struct CapHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

/// Puts `cap` in the ambient set. The kernel wants it in the inheritable set first, beside the
/// permitted one. Returns 0, or an exit code naming the step that failed.
unsafe fn raise_ambient(cap: u32) -> i32 {
    let header = CapHeader {
        version: 0x20080522,
        pid: 0,
    };
    let mut data = [CapData {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    if libc::syscall(libc::SYS_capget, &header, data.as_mut_ptr()) != 0 {
        return 11;
    }
    data[0].inheritable |= 1 << cap;
    if libc::syscall(libc::SYS_capset, &header, data.as_ptr()) != 0 {
        return 12;
    }
    if libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_RAISE, cap, 0, 0) != 0
        || ambient_is_set(cap) != 1
    {
        return 13;
    }
    0
}

unsafe fn ambient_is_set(cap: u32) -> libc::c_int {
    libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_IS_SET, cap, 0, 0)
}

/// Makes PR_CAP_AMBIENT_CLEAR_ALL fail with EINVAL, as on a kernel without ambient capabilities,
/// while PR_CAP_AMBIENT_IS_SET keeps answering. Returns 0, or an exit code naming the failure.
unsafe fn reject_clear_all() -> i32 {
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
    // BPF LD W ABS / JMP JEQ K / RET K. Offsets are from Linux's struct seccomp_data: arch, nr,
    // args[0], args[1].
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
    if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
        || libc::prctl(libc::PR_SET_SECCOMP, 2, &program, 0, 0) != 0
    {
        return 14;
    }
    if libc::prctl(
        libc::PR_CAP_AMBIENT,
        libc::PR_CAP_AMBIENT_CLEAR_ALL,
        0,
        0,
        0,
    ) != -1
        || *libc::__errno_location() != libc::EINVAL
    {
        return 15;
    }
    0
}

fn child_exit_code(pid: libc::pid_t) -> i32 {
    let mut status = 0;
    // SAFETY: waits on a pid this process forked.
    unsafe {
        assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
    }
    assert!(libc::WIFEXITED(status), "child was killed: {status}");
    libc::WEXITSTATUS(status)
}

/// Where the kernel rejects clearing the ambient set, the capset that zeroes the inheritable set
/// is what empties it. The child becomes the exec identity before raising the capability, keeping
/// its permitted set: the kernel empties the ambient set on any root-to-user switch, so a drop
/// that changed the uid would hide whether the capset does.
#[test]
#[ignore = "requires running as root, e.g. inside a container"]
fn the_drop_empties_the_ambient_set_even_when_clearing_it_is_rejected() {
    let identity = exec_identity();
    // SAFETY: fork, then only async-signal-safe syscalls in the child, which never returns.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork must succeed");
    if pid == 0 {
        unsafe {
            if libc::prctl(libc::PR_SET_KEEPCAPS, 1, 0, 0, 0) != 0
                || libc::setgid(identity.gid) != 0
                || libc::setuid(identity.uid) != 0
            {
                libc::_exit(10);
            }
            let code = raise_ambient(CAP_KILL);
            if code != 0 {
                libc::_exit(code);
            }
            let code = reject_clear_all();
            if code != 0 {
                libc::_exit(code);
            }
            if ambient_is_set(CAP_KILL) != 1 {
                libc::_exit(16);
            }
            if drop_to(identity).is_err() {
                libc::_exit(1);
            }
            if ambient_is_set(CAP_KILL) != 0 {
                libc::_exit(2);
            }
            libc::_exit(0);
        }
    }
    assert_eq!(
        child_exit_code(pid),
        0,
        "the ambient set must be empty after the drop"
    );
}

/// `restrict_supervisor` keeps CAP_KILL, so only its capset zeroing the inheritable set can take
/// an ambient CAP_KILL away. It must, even where clearing the ambient set is rejected.
#[test]
#[ignore = "requires running as root, e.g. inside a container"]
fn restricting_the_supervisor_empties_the_ambient_set_even_when_clearing_it_is_rejected() {
    // SAFETY: fork, then only async-signal-safe syscalls in the child, which never returns.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork must succeed");
    if pid == 0 {
        unsafe {
            let code = raise_ambient(CAP_KILL);
            if code != 0 {
                libc::_exit(code);
            }
            let code = reject_clear_all();
            if code != 0 {
                libc::_exit(code);
            }
            if ambient_is_set(CAP_KILL) != 1 {
                libc::_exit(16);
            }
            if restrict_supervisor().is_err() {
                libc::_exit(1);
            }
            if ambient_is_set(CAP_KILL) != 0 {
                libc::_exit(2);
            }
            libc::_exit(0);
        }
    }
    assert_eq!(
        child_exit_code(pid),
        0,
        "the ambient set must be empty after the supervisor is restricted"
    );
}
