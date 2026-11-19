//! Finding the directory a mount goes on, without following a symbolic link.
//!
//! The mount helper runs as root, and a workload can rename or replace directories it is allowed
//! to write. A path that was a directory when the sandbox booted may be a link to `/opt/alien`
//! when a resume mounts again. So the helper never gives a path to `mount` or `umount`: it walks
//! from `/` one component at a time, opens each with `O_NOFOLLOW`, refuses a link, and checks
//! that every directory it passes through belongs to root and is not writable by anyone else (a
//! directory that is itself a declared mount is the exception: its content is the workload's).
//! The mount then goes on the opened descriptor, through `/proc/self/fd`, so the directory that
//! was checked is the directory that is mounted on.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

/// A directory opened for mounting, held by descriptor.
#[derive(Debug)]
pub struct MountPoint {
    fd: OwnedFd,
}

impl MountPoint {
    /// The path to give `mount(2)`: the opened directory, whatever its name has become since.
    pub fn target(&self) -> String {
        format!("/proc/self/fd/{}", self.fd.as_raw_fd())
    }
}

fn io_error(what: &str, path: &str) -> String {
    format!("{what} {path}: {}", std::io::Error::last_os_error())
}

fn c_name(name: &str) -> Result<std::ffi::CString, String> {
    std::ffi::CString::new(name).map_err(|_| "invalid path".to_string())
}

fn stat(fd: &OwnedFd) -> Result<libc::stat, String> {
    // SAFETY: fstat on a descriptor we own into a zeroed struct of the right type.
    unsafe {
        let mut st: libc::stat = std::mem::zeroed();
        if libc::fstat(fd.as_raw_fd(), &mut st) != 0 {
            return Err(io_error("cannot inspect", "a path component"));
        }
        Ok(st)
    }
}

fn open_at(parent: &OwnedFd, name: &str) -> Result<Option<OwnedFd>, String> {
    let name = c_name(name)?;
    // SAFETY: valid descriptor and NUL-terminated name; O_PATH|O_NOFOLLOW opens a link itself
    // instead of what it points to.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        // SAFETY: a descriptor just returned by openat that nothing else owns.
        return Ok(Some(unsafe { OwnedFd::from_raw_fd(fd) }));
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ENOENT) => Ok(None),
        _ => Err(io_error("cannot open", name.to_str().unwrap_or("?"))),
    }
}

/// Opens the directory at `path` for a mount or an unmount.
///
/// * `create`: make missing directories (root-owned, mode 0755); otherwise a missing one is an error.
/// * `expected_owner`: who must own every directory on the way (0 in production).
/// * `trusted`: declared mount paths; a directory at or under one of them is the workload's own
///   and is not held to the ownership rule (links are still refused there).
pub fn open(
    path: &str,
    create: bool,
    expected_owner: u32,
    trusted: &[String],
) -> Result<MountPoint, String> {
    walk(path, create, expected_owner, trusted, "")
}

/// [`open`], except that directories on the way to `unchecked_above` (and it) are not held to the
/// ownership rule. For tests, which cannot own `/`.
fn walk(
    path: &str,
    create: bool,
    expected_owner: u32,
    trusted: &[String],
    unchecked_above: &str,
) -> Result<MountPoint, String> {
    if !path.starts_with('/')
        || path
            .split('/')
            .skip(1)
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(format!("'{path}' is not a plain absolute path"));
    }
    if alien_core::mount_path_is_reserved(path) {
        return Err(format!("'{path}' is a system path and cannot hold a mount"));
    }
    let root = c_name("/")?;
    // SAFETY: valid NUL-terminated path.
    let raw = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(io_error("cannot open", "/"));
    }
    // SAFETY: a descriptor just returned by open that nothing else owns.
    let mut current = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut walked = String::new();
    for name in path.split('/').skip(1) {
        let here = if walked.is_empty() {
            "/"
        } else {
            walked.as_str()
        };
        let own_content = trusted
            .iter()
            .any(|t| here == t || here.starts_with(&format!("{t}/")));
        let unchecked = !unchecked_above.is_empty()
            && (here == "/"
                || unchecked_above == here
                || unchecked_above.starts_with(&format!("{here}/")));
        if !own_content && !unchecked {
            let st = stat(&current)?;
            if st.st_uid != expected_owner || st.st_mode & 0o022 != 0 {
                return Err(format!(
                    "{here} must be owned by root and not writable by others before a mount can go below it"
                ));
            }
        }
        let next = match open_at(&current, name)? {
            Some(fd) => fd,
            None if create => {
                let c = c_name(name)?;
                // SAFETY: valid descriptor and name. EEXIST is a race with another creator, fine.
                let rc = unsafe { libc::mkdirat(current.as_raw_fd(), c.as_ptr(), 0o755) };
                if rc != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                    return Err(io_error("cannot create", &format!("{here}/{name}")));
                }
                open_at(&current, name)?.ok_or_else(|| format!("{here}/{name} vanished"))?
            }
            None => return Err(format!("{here}/{name} does not exist")),
        };
        walked = format!("{walked}/{name}");
        match stat(&next)?.st_mode & libc::S_IFMT {
            libc::S_IFDIR => {}
            libc::S_IFLNK => return Err(format!("{walked} is a symbolic link")),
            _ => return Err(format!("{walked} is not a directory")),
        }
        current = next;
    }
    Ok(MountPoint { fd: current })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn me() -> u32 {
        // SAFETY: getuid cannot fail.
        unsafe { libc::getuid() }
    }

    /// A directory chain whose every parent is owned by the caller and closed to others, standing
    /// in for the root-owned parents of a real sandbox.
    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        dir
    }

    /// Opens below `dir`, ignoring the ownership of everything above it: the walk starts at `/`,
    /// which the test user does not own.
    fn open_in(
        dir: &std::path::Path,
        path: &std::path::Path,
        create: bool,
        owner: u32,
        trusted: &[String],
    ) -> Result<MountPoint, String> {
        walk(
            path.to_str().unwrap(),
            create,
            owner,
            trusted,
            dir.to_str().unwrap(),
        )
    }

    #[test]
    fn missing_directories_are_created_and_an_existing_one_is_opened() {
        let dir = tree();
        let path = dir.path().join("a/b");
        let point = open_in(dir.path(), &path, true, me(), &[]).expect("creates");
        assert!(point.target().starts_with("/proc/self/fd/"));
        assert!(path.is_dir());
        open_in(dir.path(), &path, false, me(), &[]).expect("opens what exists");
        open_in(dir.path(), &path.join("nope"), false, me(), &[])
            .expect_err("a missing directory is an error without create");
    }

    #[test]
    fn a_symbolic_link_component_is_refused_and_nothing_is_created_through_it() {
        let dir = tree();
        let elsewhere = tree();
        symlink(elsewhere.path(), dir.path().join("link")).unwrap();
        let through = dir.path().join("link/inner");
        let error =
            open_in(dir.path(), &through, true, me(), &[]).expect_err("a link in the middle");
        assert!(error.contains("symbolic link"), "{error}");
        assert!(
            !elsewhere.path().join("inner").exists(),
            "nothing was created behind the link"
        );
        let last = dir.path().join("link");
        let error =
            open_in(dir.path(), &last, true, me(), &[]).expect_err("a link as the last component");
        assert!(error.contains("symbolic link"), "{error}");
    }

    #[test]
    fn a_parent_others_can_write_is_refused_unless_it_is_a_declared_mount() {
        let dir = tree();
        let open_parent = dir.path().join("shared");
        std::fs::create_dir(&open_parent).unwrap();
        std::fs::set_permissions(&open_parent, std::fs::Permissions::from_mode(0o777)).unwrap();
        let target = open_parent.join("m");
        let error = open_in(dir.path(), &target, true, me(), &[]).expect_err("writable parent");
        assert!(error.contains("not writable by others"), "{error}");
        assert!(!open_parent.join("m").exists());
        let trusted = vec![open_parent.to_str().unwrap().to_string()];
        open_in(dir.path(), &target, true, me(), &trusted)
            .expect("the workload's own mount holds its content");
    }

    #[test]
    fn a_parent_owned_by_someone_else_is_refused() {
        let dir = tree();
        let target = dir.path().join("m");
        let error =
            open_in(dir.path(), &target.join("x"), true, me() + 1, &[]).expect_err("wrong owner");
        assert!(error.contains("owned by root"), "{error}");
    }

    #[test]
    fn reserved_and_unclean_paths_are_refused() {
        for path in [
            "/opt/alien",
            "/opt",
            "/etc/x",
            "relative",
            "/a/../b",
            "/a//b",
            "/a/./b",
        ] {
            open(path, false, 0, &[]).expect_err(path);
        }
    }
}
