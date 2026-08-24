//! Opening a caller's path so it cannot leave the session root.
//!
//! Resolving a path to a string and then opening that string is check-then-use: every guard sits in
//! the window before the open, and the code being confined runs in the same guest and can drive both
//! sides of that window. This module never does that. It walks the path one component at a time,
//! each `openat` taken relative to the *file descriptor* of the component before it, so the inode a
//! step opens is the inode the previous step checked — there is no name left to re-resolve, and
//! nothing for a caller to swap between the check and the use.
//!
//! Two rules make the walk stay beneath the root. Every step opens with `O_NOFOLLOW`, so a symlink
//! at any component — including a `/proc/self/fd`-style magic link — fails the step rather than
//! redirecting it (an intermediate component also carries `O_DIRECTORY`, so a symlink there is
//! `ENOTDIR` even when `O_PATH` would otherwise open the link itself). And `.` and `..` are refused
//! outright, so the walk only ever descends. Together these are what `openat2`'s
//! `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS` give in one syscall — done in
//! userspace instead because the sandbox runtimes this agent ships under do not all implement
//! `openat2` (gVisor returns `ENOSYS`), and a security boundary cannot depend on a syscall the
//! platform may lack.
//!
//! Hard links are deliberately not addressed here: a link is a second name for an inode, so no
//! resolver can tell one from the file itself. The kernel's `protected_hardlinks` (1 by default,
//! and on the images this agent ships in) already refuses linking a file the caller cannot write,
//! which bounds that to files the caller could reach anyway.

use std::io;
use std::path::Path;

#[cfg(target_os = "linux")]
use std::ffi::{CStr, CString};
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;

/// Strips the leading separator so a caller's `/work/x` is read as relative to the session root.
///
/// A caller writing `/work/x` means the session's `/work/x`, not the host's; the walk below only
/// ever descends from the root, so an absolute path cannot reach outside it either way.
#[cfg(target_os = "linux")]
fn relative(requested: &str) -> &str {
    requested.trim_start_matches('/')
}

/// Opens the session root itself, to begin a walk from. The root path is the agent's own and is
/// canonicalized at startup, so following symlinks within *its* prefix is fine — confinement is
/// what happens on the descent below, not here.
#[cfg(target_os = "linux")]
fn open_root(root: &Path) -> io::Result<OwnedFd> {
    let path = CString::new(root.as_os_str().as_bytes())
        .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: a valid NUL-terminated path; the returned fd is owned below.
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a fresh, valid descriptor this function owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// A single confined step: opens `name` directly under `dir`, never following a symlink at it.
///
/// `O_NOFOLLOW` is the whole point — it is added to every step so a symlink (or magic link) planted
/// at that name fails here rather than sending the open somewhere else.
#[cfg(target_os = "linux")]
fn open_child(dir: RawFd, name: &CStr, flags: i32, mode: u32) -> io::Result<OwnedFd> {
    // SAFETY: a valid dirfd and a NUL-terminated component; `mode` is consulted only under `O_CREAT`.
    let fd = unsafe {
        libc::openat(
            dir,
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a fresh descriptor the kernel just returned.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Rejects any component that is not a plain name. `.` and `..` are refused as `EXDEV` — the errno
/// callers already read as an attempt to leave the root.
#[cfg(target_os = "linux")]
fn confined_component(part: &str) -> io::Result<CString> {
    if part == "." || part == ".." {
        return Err(io::Error::from_raw_os_error(libc::EXDEV));
    }
    CString::new(part).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))
}

/// Walks to the directory that holds `requested`'s final component, opening each parent through a
/// confined step, and returns that directory's fd together with the final component's name. A
/// symlink or `..` at any parent fails the walk rather than redirecting it.
#[cfg(target_os = "linux")]
fn descend_to_parent(root: &Path, requested: &str) -> io::Result<(OwnedFd, CString)> {
    let parts: Vec<&str> = relative(requested)
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let Some((last, parents)) = parts.split_last() else {
        // The session root itself is a directory, not a file a caller names to open or remove.
        return Err(io::Error::from_raw_os_error(libc::EISDIR));
    };

    let mut dir = open_root(root)?;
    for part in parents {
        let name = confined_component(part)?;
        dir = open_child(dir.as_raw_fd(), &name, libc::O_PATH | libc::O_DIRECTORY, 0)?;
    }
    Ok((dir, confined_component(last)?))
}

/// Opens a path beneath `root`, refusing anything that would resolve outside it.
#[cfg(target_os = "linux")]
fn open_beneath(root: &Path, requested: &str, flags: i32, mode: u32) -> io::Result<std::fs::File> {
    let (dir, leaf) = descend_to_parent(root, requested)?;
    let fd = open_child(dir.as_raw_fd(), &leaf, flags, mode)?;
    Ok(std::fs::File::from(fd))
}

/// Mode for a file this agent creates, and for a directory, below.
///
/// The command runs as a different uid than the agent — on a MicroVM the agent is root, because
/// it needs `setuid` to drop — so an owner-only mode is unreachable by the code it was uploaded
/// for. Containment is not these bits: the session root is `0700` owned by the exec uid, so
/// "other" inside it is that uid and root, and nothing else can traverse in. Writable rather than
/// read-only because uploading a project and building it is the case this API exists for.
#[cfg(target_os = "linux")]
const CREATED_FILE: u32 = 0o666;
/// See [`CREATED_FILE`]. Traversable and writable, or the command cannot use the tree it was given.
#[cfg(target_os = "linux")]
const CREATED_DIR: u32 = 0o777;

/// Opens a file for reading, beneath the session root.
///
/// `O_NONBLOCK` because the command can create anything it likes in the session root: opening a
/// FIFO without it blocks until a writer appears, and the caller's read would hang a blocking
/// thread that nothing cancels. On a regular file the flag has no effect on reads.
#[cfg(target_os = "linux")]
pub fn open_read(root: &Path, requested: &str) -> io::Result<std::fs::File> {
    open_beneath(root, requested, libc::O_RDONLY | libc::O_NONBLOCK, 0)
}

/// Creates or truncates a file for writing, beneath the session root.
///
/// `O_NONBLOCK` keeps a FIFO the command planted from blocking the open; the caller checks the type
/// on the descriptor.
#[cfg(target_os = "linux")]
pub fn open_write(root: &Path, requested: &str) -> io::Result<std::fs::File> {
    let common = libc::O_WRONLY | libc::O_NONBLOCK;

    // `O_EXCL` so "did this call create the file" is answered by the kernel rather than by a stat
    // that another process can invalidate. Only a file this agent created gets its mode set; one
    // the command already owns keeps whatever it chose.
    match open_beneath(
        root,
        requested,
        common | libc::O_CREAT | libc::O_EXCL,
        CREATED_FILE,
    ) {
        Ok(file) => {
            if let Err(error) = set_mode(&file, CREATED_FILE) {
                // The mode failure is the one worth reporting; a removal that also fails leaves
                // an entry the next write would preserve, which is the state this avoids.
                let _ = remove_beneath(root, requested, 0);
                return Err(error);
            }
            Ok(file)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            open_beneath(root, requested, common | libc::O_TRUNC, 0)
        }
        Err(error) => Err(error),
    }
}

/// Forces the mode of something just created, ignoring the inherited umask.
///
/// The mode passed to `open`/`mkdirat` is masked by the umask the entrypoint happened to inherit,
/// so a `0666` request can arrive as `0600`, unreachable to the command.
#[cfg(target_os = "linux")]
fn set_mode(file: &std::fs::File, mode: u32) -> io::Result<()> {
    // SAFETY: a valid descriptor this function borrows and a mode word.
    if unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Removes an entry beneath the session root, for the path that just created it.
///
/// Only reached when the mode could not be set: leaving the entry would make every later write
/// take the branch that preserves an existing entry's mode, so one failure here would be
/// permanent rather than retryable. Walks to the parent through the same confined steps, so it can
/// only unlink beneath the root.
#[cfg(target_os = "linux")]
fn remove_beneath(root: &Path, requested: &str, flags: i32) -> io::Result<()> {
    let (dir, leaf) = descend_to_parent(root, requested)?;
    // SAFETY: a valid dirfd, a NUL-terminated leaf name, and a flags word.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), leaf.as_ptr(), flags) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Creates a directory and its parents, one confined step at a time.
///
/// Each component is created relative to the previous one and then re-opened through a confined
/// step, so a component swapped for a symlink mid-walk fails the re-open rather than redirecting it.
#[cfg(target_os = "linux")]
pub fn create_dir_all(root: &Path, requested: &str) -> io::Result<()> {
    let mut current = open_root(root)?;

    for component in relative(requested)
        .split('/')
        .filter(|part| !part.is_empty())
    {
        let name = confined_component(component)?;

        // SAFETY: a valid dirfd and NUL-terminated component name.
        let made = unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), CREATED_DIR) };
        if made != 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error);
            }
        } else {
            // Only on the branch that created it — an existing directory is the command's own.
            // `mkdirat`'s mode is umask-masked, so it is forced here rather than trusted.
            // SAFETY: a valid dirfd and NUL-terminated component name, mode word, no flags.
            if unsafe {
                libc::fchmodat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    CREATED_DIR as libc::mode_t,
                    0,
                )
            } != 0
            {
                let error = io::Error::last_os_error();
                // SAFETY: the same valid dirfd and name; removing what this iteration created.
                unsafe { libc::unlinkat(current.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
                return Err(error);
            }
        }

        // The re-open is where a symlink is caught: `O_NOFOLLOW` with `O_DIRECTORY` makes a symlink
        // at this name `ENOTDIR`, so the walk never steps onto it even if the command raced `mkdirat`.
        current = open_child(
            current.as_raw_fd(),
            &name,
            libc::O_PATH | libc::O_DIRECTORY,
            0,
        )?;
    }

    Ok(())
}

/// Non-Linux builds resolve and then open, which is the check-then-use this module exists to
/// avoid. The agent only ships in Linux images; this exists so the crate builds and tests on a
/// development machine, and is never the path a sandbox runs.
#[cfg(not(target_os = "linux"))]
mod fallback {
    use super::*;
    use crate::paths::resolve_within_root;

    /// Reports a refusal as `EXDEV`, the same errno the Linux walk returns for `..`, so callers
    /// classify an escape the same way on both paths.
    fn resolved(root: &Path, requested: &str) -> io::Result<std::path::PathBuf> {
        resolve_within_root(root, requested).map_err(|_| io::Error::from_raw_os_error(libc::EXDEV))
    }

    pub fn open_read(root: &Path, requested: &str) -> io::Result<std::fs::File> {
        std::fs::File::open(resolved(root, requested)?)
    }

    // Not the Linux modes: `File::create` and `create_dir_all` already produce `0644`/`0755` here,
    // which is reachable across uids. This is the path developers on macOS exercise; the Linux
    // path is what ships, so a permission regression there won't show up here.
    pub fn open_write(root: &Path, requested: &str) -> io::Result<std::fs::File> {
        std::fs::File::create(resolved(root, requested)?)
    }

    pub fn create_dir_all(root: &Path, requested: &str) -> io::Result<()> {
        std::fs::create_dir_all(resolved(root, requested)?)
    }
}

#[cfg(not(target_os = "linux"))]
pub use fallback::{create_dir_all, open_read, open_write};

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmp_root() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "alien-confine-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_file_written_beneath_the_root_reads_back() {
        let root = tmp_root();
        create_dir_all(&root, "a/b").expect("nested dirs are created");
        let mut w = open_write(&root, "a/b/f").expect("a file writes beneath the root");
        w.write_all(b"hello").unwrap();
        let mut r = open_read(&root, "a/b/f").expect("the file reads back");
        let mut got = String::new();
        r.read_to_string(&mut got).unwrap();
        assert_eq!(got, "hello");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_dotdot_component_is_refused() {
        let root = tmp_root();
        let error = open_read(&root, "../escape").expect_err("`..` must not leave the root");
        assert_eq!(error.raw_os_error(), Some(libc::EXDEV), "{error}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_symlink_final_component_is_not_followed() {
        let root = tmp_root();
        // A dangling target is enough: O_NOFOLLOW fails at the link itself, before the target.
        symlink("/etc/passwd", root.join("link")).unwrap();
        let error = open_read(&root, "link").expect_err("a symlink target must not be followed");
        assert!(
            matches!(error.raw_os_error(), Some(libc::ELOOP) | Some(libc::EMLINK)),
            "{error}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_symlinked_parent_component_is_not_followed() {
        let root = tmp_root();
        symlink("/tmp", root.join("up")).unwrap();
        let error = open_read(&root, "up/passwd")
            .expect_err("a symlinked parent must not redirect the walk");
        // O_DIRECTORY on the symlink parent makes it ENOTDIR (the link itself is not a directory).
        assert!(
            matches!(
                error.raw_os_error(),
                Some(libc::ENOTDIR) | Some(libc::ELOOP)
            ),
            "{error}"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
