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

/// Opens a file for reading, beneath the session root.
#[cfg(target_os = "linux")]
pub fn open_read(root: &Path, requested: &str) -> io::Result<std::fs::File> {
    open_beneath(root, requested, libc::O_RDONLY, 0)
}

/// Creates or truncates a file for writing, beneath the session root.
///
/// `O_NOFOLLOW` is redundant next to `RESOLVE_NO_SYMLINKS` and harmless; the mode applies only
/// when the file is created, so an existing file keeps its own.
#[cfg(target_os = "linux")]
pub fn open_write(root: &Path, requested: &str) -> io::Result<std::fs::File> {
    open_beneath(
        root,
        requested,
        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW,
        0o600,
    )
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

    for component in relative(requested).split('/').filter(|part| !part.is_empty()) {
        // `EXDEV` rather than `EINVAL`: this is the same refusal `RESOLVE_BENEATH` reports for a
        // path that leaves the root, and callers classify the escape by errno.
        if component == "." || component == ".." {
            return Err(io::Error::from_raw_os_error(libc::EXDEV));
        }

        let name = CString::new(component).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;

        // SAFETY: a valid dirfd and NUL-terminated component name.
        let made = unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o700) };
        if made != 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
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
        resolve_within_root(root, requested)
            .map_err(|_| io::Error::from_raw_os_error(libc::EXDEV))
    }

    pub fn open_read(root: &Path, requested: &str) -> io::Result<std::fs::File> {
        std::fs::File::open(resolved(root, requested)?)
    }

    pub fn open_write(root: &Path, requested: &str) -> io::Result<std::fs::File> {
        std::fs::File::create(resolved(root, requested)?)
    }

    pub fn create_dir_all(root: &Path, requested: &str) -> io::Result<()> {
        std::fs::create_dir_all(resolved(root, requested)?)
    }
}

#[cfg(not(target_os = "linux"))]
pub use fallback::{create_dir_all, open_read, open_write};
