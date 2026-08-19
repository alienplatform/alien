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
//! docker run --rm --platform linux/arm64 -v "$PWD:/work" -e CARGO_TARGET_DIR=/tmp/target \
//!   -w /work rust:1-bookworm \
//!   cargo test -p alien-sandbox-agent --test privileged -- --ignored
//! ```
#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use alien_sandbox_agent::exec::{stream, ExecIdentity, ExecRequest, Frame};
use alien_sandbox_agent::files;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use tempfile::TempDir;
use tokio::sync::mpsc;

/// The uid the shipped image runs commands as, from `alien_build::sandbox_bundle`.
const EXEC_UID: u32 = 60000;

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
        deadline_ms: 30_000,
        working_directory: None,
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

    files::mkdir(&root, "/work")
        .await
        .expect("the agent creates it");
    files::write(&root, "/work/data.txt", b"payload")
        .await
        .expect("the agent writes into it");

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
