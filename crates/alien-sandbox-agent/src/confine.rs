//! Opening a caller's path so it cannot leave the session root.
//!
//! Resolving a path and then opening it by name is check-then-use: every guard sits in the window
//! before the open, and the code being confined is running in the same guest and can drive both
//! sides of that window. `openat2` closes it by construction — the kernel resolves and opens in
//! one call, and refuses rather than following anything that would leave the root.
//!
//! `RESOLVE_BENEATH` rejects `..` and absolute paths; `RESOLVE_NO_SYMLINKS` rejects a symlink in
//! any component, including the final one; `RESOLVE_NO_MAGICLINKS` rejects `/proc/self/fd`-style
//! links. Nothing is left for a caller to race.
//!
//! Hard links are deliberately not addressed here: a link is a second name for an inode, so no
//! resolver can tell one from the file itself. The kernel's `protected_hardlinks` (1 by default,
//! and on the images this agent ships in) already refuses linking a file the caller cannot write,
//! which bounds that to files the caller could reach anyway.

use std::io;
use std::path::Path;

#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

/// `openat2` refuses to leave the directory it starts from.
#[cfg(target_os = "linux")]
const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
#[cfg(target_os = "linux")]
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
#[cfg(target_os = "linux")]
const RESOLVE_BENEATH: u64 = 0x08;

/// The kernel's `struct open_how`. Declared here because the layout is stable ABI and this is the
/// only place that needs it.
#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Default)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

/// Strips the leading separator so a caller's `/work/x` is read as relative to the session root.
///
/// `RESOLVE_BENEATH` refuses an absolute path outright, and a caller writing `/work/x` means the
/// session's `/work/x`, not the host's.
#[cfg(target_os = "linux")]
fn relative(requested: &str) -> &str {
    requested.trim_start_matches('/')
}

/// Opens a path beneath `root`, refusing anything that would resolve outside it.
#[cfg(target_os = "linux")]
fn open_beneath(root: &Path, requested: &str, flags: i32, mode: u32) -> io::Result<std::fs::File> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let root_path = CString::new(root.as_os_str().as_bytes())
        .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    let target = CString::new(relative(requested))
        .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;

    // SAFETY: a valid NUL-terminated path and a flags word; the returned fd is owned below.
    let root_fd = unsafe { libc::open(root_path.as_ptr(), libc::O_PATH | libc::O_DIRECTORY) };
    if root_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `root_fd` is a fresh, valid descriptor this function owns.
    let root_fd = unsafe { OwnedFd::from_raw_fd(root_fd) };

    let how = OpenHow {
        flags: (flags | libc::O_CLOEXEC) as u64,
        mode: mode as u64,
        resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS,
    };

    // SAFETY: `openat2` with a valid dirfd, a NUL-terminated relative path, and a correctly sized
    // `open_how`. The kernel performs the whole resolution; nothing here dereferences its result.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            root_fd.as_raw_fd(),
            target.as_ptr(),
            &how as *const OpenHow,
            std::mem::size_of::<OpenHow>(),
        )
    };

    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: `fd` is a fresh descriptor the kernel just returned to us.
    Ok(unsafe { std::fs::File::from_raw_fd(fd as i32) })
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
/// `O_NOFOLLOW` is redundant next to `RESOLVE_NO_SYMLINKS` and harmless. `O_NONBLOCK` keeps a
/// FIFO the command planted from blocking the open; the caller checks the type on the descriptor.
#[cfg(target_os = "linux")]
pub fn open_write(root: &Path, requested: &str) -> io::Result<std::fs::File> {
    let common = libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK;

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
/// permanent rather than retryable.
#[cfg(target_os = "linux")]
fn remove_beneath(root: &Path, requested: &str, flags: i32) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let root_path = CString::new(root.as_os_str().as_bytes())
        .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    let target = CString::new(relative(requested))
        .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;

    // SAFETY: a valid NUL-terminated path and a flags word; the fd is owned below.
    let fd = unsafe { libc::open(root_path.as_ptr(), libc::O_PATH | libc::O_DIRECTORY) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a fresh, valid descriptor this function owns.
    let root_fd = unsafe { OwnedFd::from_raw_fd(fd) };

    // SAFETY: a valid dirfd, a NUL-terminated relative path, and a flags word.
    if unsafe { libc::unlinkat(root_fd.as_raw_fd(), target.as_ptr(), flags) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Creates a directory and its parents, one confined step at a time.
///
/// Each component is created relative to the previous one and then re-opened through the same
/// confinement, so a component swapped for a symlink mid-walk fails the next step rather than
/// redirecting it.
#[cfg(target_os = "linux")]
pub fn create_dir_all(root: &Path, requested: &str) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let root_path = CString::new(root.as_os_str().as_bytes())
        .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: a valid NUL-terminated path and a flags word.
    let fd = unsafe { libc::open(root_path.as_ptr(), libc::O_PATH | libc::O_DIRECTORY) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fresh, valid descriptor.
    let mut current = unsafe { OwnedFd::from_raw_fd(fd) };

    for component in relative(requested)
        .split('/')
        .filter(|part| !part.is_empty())
    {
        // `EXDEV` rather than `EINVAL`: this is the same refusal `RESOLVE_BENEATH` reports for a
        // path that leaves the root, and callers classify the escape by errno.
        if component == "." || component == ".." {
            return Err(io::Error::from_raw_os_error(libc::EXDEV));
        }

        let name =
            CString::new(component).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;

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

        let how = OpenHow {
            flags: (libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64,
            mode: 0,
            resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS,
        };

        // SAFETY: valid dirfd, NUL-terminated name, correctly sized `open_how`.
        let next = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                current.as_raw_fd(),
                name.as_ptr(),
                &how as *const OpenHow,
                std::mem::size_of::<OpenHow>(),
            )
        };
        if next < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fresh descriptor from the kernel; the previous one is dropped by the assignment.
        current = unsafe { OwnedFd::from_raw_fd(next as i32) };
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

    /// Reports a refusal as `EXDEV`, the same errno `RESOLVE_BENEATH` returns, so callers
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
