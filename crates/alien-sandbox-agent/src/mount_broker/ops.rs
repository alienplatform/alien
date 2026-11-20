//! What a broker helper does once it is in the image's mount namespace. Runs in a forked child of
//! the single-threaded broker, so ordinary allocation is fine.

use std::{
    collections::BTreeSet,
    ffi::CString,
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    time::Instant,
};

use super::request::{Errno, Plan, Refusal, MOUNT_ATTR_NODEV, MOUNT_ATTR_NOSUID};

const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;
const FSOPEN_CLOEXEC: u32 = 0x1;
const FSMOUNT_CLOEXEC: u32 = 0x1;
const FSCONFIG_SET_FLAG: u32 = 0;
const FSCONFIG_SET_STRING: u32 = 1;
const FSCONFIG_CMD_CREATE: u32 = 6;
const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x4;
const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x40;
const OPEN_TREE_CLONE: u32 = 1;
const OPEN_TREE_CLOEXEC: u32 = libc::O_CLOEXEC as u32;

#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

#[repr(C)]
struct MountAttr {
    attr_set: u64,
    attr_clr: u64,
    propagation: u64,
    userns_fd: u64,
}

/// What a helper reports back.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Outcome {
    pub result: Result<Option<u64>, (String, String)>,
    /// The broker-made mounts still present in the namespace.
    pub alive: Vec<u64>,
}

fn os_error(what: &str) -> Refusal {
    let error = io::Error::last_os_error();
    (
        Errno::from_raw(error.raw_os_error().unwrap_or(libc::EIO)),
        format!("{what}: {error}"),
    )
}

fn c(text: &str) -> Result<CString, Refusal> {
    CString::new(text).map_err(|_| (Errno::Einval, "a NUL byte".to_string()))
}

fn owned(fd: libc::c_long, what: &str) -> Result<OwnedFd, Refusal> {
    if fd < 0 {
        return Err(os_error(what));
    }
    // SAFETY: a descriptor the kernel just returned that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

/// Opens a directory without following any symbolic or magic link on the way.
pub fn open_dir(dirfd: RawFd, path: &str, beneath: bool) -> Result<OwnedFd, Refusal> {
    let name = c(path)?;
    let how = OpenHow {
        flags: (libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64,
        mode: 0,
        resolve: RESOLVE_NO_SYMLINKS
            | RESOLVE_NO_MAGICLINKS
            | if beneath { RESOLVE_BENEATH } else { 0 },
    };
    // SAFETY: valid path and a correctly sized open_how.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            dirfd,
            name.as_ptr(),
            &how as *const OpenHow,
            size_of::<OpenHow>(),
        )
    };
    owned(fd, &format!("cannot open {path} without following links"))
}

/// The mount ID of the mount `fd` is on, from `/proc/self/fdinfo`.
pub fn mount_id(fd: &OwnedFd) -> Result<u64, Refusal> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", fd.as_raw_fd()))
        .map_err(|e| (Errno::Eio, format!("cannot read the mount ID: {e}")))?;
    info.lines()
        .find_map(|line| line.strip_prefix("mnt_id:"))
        .and_then(|id| id.trim().parse().ok())
        .ok_or_else(|| (Errno::Eio, "no mount ID".to_string()))
}

/// Every mount ID in this mount namespace.
pub fn mount_ids() -> BTreeSet<u64> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split(' ').next()?.parse().ok())
        .collect()
}

/// The kernel's own messages for a failed `fsconfig`, read from the context.
fn context_messages(fs: &OwnedFd) -> String {
    let mut all = Vec::new();
    let mut buffer = [0u8; 512];
    for _ in 0..8 {
        // SAFETY: reading into a buffer we own.
        let n = unsafe { libc::read(fs.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if n <= 0 {
            break;
        }
        all.push(String::from_utf8_lossy(&buffer[..n as usize]).into_owned());
    }
    all.join("; ")
}

/// A detached mount made with `fsopen`/`fsconfig`/`fsmount`.
fn new_mount(
    fstype: &str,
    source: Option<&str>,
    params: &[(String, Option<String>)],
    attributes: u64,
) -> Result<OwnedFd, Refusal> {
    let fstype_c = c(fstype)?;
    // SAFETY: valid NUL-terminated name.
    let fs = owned(
        unsafe { libc::syscall(libc::SYS_fsopen, fstype_c.as_ptr(), FSOPEN_CLOEXEC) },
        &format!("cannot open a {fstype} context"),
    )?;
    let config = |cmd: u32, key: Option<&str>, value: Option<&str>| -> Result<(), Refusal> {
        let key = key.map(c).transpose()?;
        let value = value.map(c).transpose()?;
        // SAFETY: valid descriptor and NUL-terminated strings (or null).
        let rc = unsafe {
            libc::syscall(
                libc::SYS_fsconfig,
                fs.as_raw_fd(),
                cmd,
                key.as_ref().map_or(std::ptr::null(), |k| k.as_ptr()),
                value.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                0,
            )
        };
        if rc != 0 {
            let (errno, reason) = os_error(&format!(
                "{fstype} {}",
                key.as_ref()
                    .map_or("create".into(), |k| k.to_string_lossy().into_owned())
            ));
            let kernel = context_messages(&fs);
            return Err((
                errno,
                if kernel.is_empty() {
                    reason
                } else {
                    format!("{reason} ({kernel})")
                },
            ));
        }
        Ok(())
    };
    if let Some(source) = source {
        config(FSCONFIG_SET_STRING, Some("source"), Some(source))?;
    }
    for (key, value) in params {
        match value {
            Some(value) => config(FSCONFIG_SET_STRING, Some(key), Some(value))?,
            None => config(FSCONFIG_SET_FLAG, Some(key), None)?,
        }
    }
    config(FSCONFIG_CMD_CREATE, None, None)?;
    // SAFETY: valid descriptor.
    owned(
        unsafe {
            libc::syscall(
                libc::SYS_fsmount,
                fs.as_raw_fd(),
                FSMOUNT_CLOEXEC,
                attributes | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV,
            )
        },
        &format!("cannot make the {fstype} mount"),
    )
}

/// Attaches `mount` on the directory opened as `target`, then checks that `path` now resolves to
/// that very mount. If it does not (the directory was renamed between the open and the attach),
/// the mount is detached again and the request fails.
fn attach(mount: &OwnedFd, target: &OwnedFd, path: &str) -> Result<u64, Refusal> {
    let empty = c("")?;
    // SAFETY: valid descriptors and empty paths with the EMPTY_PATH flags.
    if unsafe {
        libc::syscall(
            libc::SYS_move_mount,
            mount.as_raw_fd(),
            empty.as_ptr(),
            target.as_raw_fd(),
            empty.as_ptr(),
            MOVE_MOUNT_F_EMPTY_PATH | MOVE_MOUNT_T_EMPTY_PATH,
        )
    } != 0
    {
        return Err(os_error(&format!("cannot attach the mount at {path}")));
    }
    let made = mount_id(mount)?;
    let landed = open_dir(libc::AT_FDCWD, path, false).and_then(|fd| mount_id(&fd));
    // A rename of the target that passed its mount-point check before the attach may still be
    // running: the check and the move are not serialized against `move_mount`, so the first check
    // can pass and the directory move a moment later. The rename holds the parent directory's
    // lock until it is done; wait for that lock, then check again. A rename that starts after
    // the attach fails with EBUSY.
    let landed = landed.and_then(|first| {
        wait_for_renames_in_parent(target)?;
        let again = open_dir(libc::AT_FDCWD, path, false).and_then(|fd| mount_id(&fd))?;
        Ok(if again == first { first } else { 0 })
    });
    if landed.as_ref().ok() != Some(&made) {
        let detach = c(&format!("/proc/self/fd/{}", mount.as_raw_fd()))?;
        // SAFETY: valid path naming the mount we just attached.
        unsafe { libc::umount2(detach.as_ptr(), libc::MNT_DETACH) };
        return Err((
            Errno::Ebusy,
            format!("{path} changed while it was being mounted; the mount was removed"),
        ));
    }
    Ok(made)
}

/// Waits until no rename is running in the directory that holds `target`. Looking up a name the
/// kernel has not cached takes the directory's lock (shared), which a rename holds exclusively
/// from before its mount-point check until the entry has moved. The name is random, so the image
/// cannot have created it; if it exists anyway, another is tried.
fn wait_for_renames_in_parent(target: &OwnedFd) -> Result<(), Refusal> {
    let parent = open_dir(target.as_raw_fd(), "..", false)?;
    for _ in 0..8 {
        let mut bytes = [0u8; 16];
        // SAFETY: writes at most `bytes.len()` bytes into `bytes`.
        let read = unsafe { libc::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), 0) };
        if read != bytes.len() as isize {
            return Err(os_error("cannot read random bytes"));
        }
        let name: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let name = c(&format!(".alien-mount-{name}"))?;
        let how = OpenHow {
            flags: (libc::O_PATH | libc::O_CLOEXEC) as u64,
            mode: 0,
            resolve: RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS | RESOLVE_BENEATH,
        };
        // SAFETY: valid path and a correctly sized open_how.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                parent.as_raw_fd(),
                name.as_ptr(),
                &how as *const OpenHow,
                size_of::<OpenHow>(),
            )
        };
        if fd >= 0 {
            // SAFETY: a descriptor the kernel just returned that nothing else owns.
            drop(unsafe { OwnedFd::from_raw_fd(fd as RawFd) });
            continue;
        }
        return match io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => Ok(()),
            _ => Err(os_error("cannot wait for renames next to the target")),
        };
    }
    Err((
        Errno::Ebusy,
        "cannot wait for renames next to the target".to_string(),
    ))
}

/// `mount_setattr` flag for a read-only mount.
const MOUNT_ATTR_RDONLY: u64 = 0x1;
const MOUNT_ATTR_NOEXEC: u64 = 0x8;

const RESOLVE_IN_ROOT: u64 = 0x10;
/// `FS_IOC_GETFLAGS` / `FS_IOC_SETFLAGS` (`_IOR('f', 1, long)` / `_IOW('f', 2, long)`).
const FS_IOC_GETFLAGS: libc::c_ulong = 0x8008_6601;
const FS_IOC_SETFLAGS: libc::c_ulong = 0x4008_6602;
const FS_IMMUTABLE_FL: libc::c_int = 0x10;

/// What a path the kernel reads on its own must be when the image does not provide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemPath {
    /// Created as an empty `0755` directory.
    Directory,
    /// Created as an empty `0644` file.
    File,
}

/// Opens `path` under `root` as the image would see it with `root` as `/`: symbolic links are
/// followed but never leave `root`, magic links are refused.
fn open_in_root(root: &OwnedFd, path: &str, flags: libc::c_int) -> io::Result<OwnedFd> {
    let name = CString::new(path.trim_start_matches('/'))
        .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    let name = if name.as_bytes().is_empty() {
        c".".to_owned()
    } else {
        name
    };
    let how = OpenHow {
        flags: (flags | libc::O_CLOEXEC) as u64,
        mode: 0,
        resolve: RESOLVE_IN_ROOT | RESOLVE_NO_MAGICLINKS,
    };
    // SAFETY: valid path and a correctly sized open_how.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            root.as_raw_fd(),
            name.as_ptr(),
            &how as *const OpenHow,
            size_of::<OpenHow>(),
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a descriptor the kernel just returned that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

fn fstat(fd: &OwnedFd) -> io::Result<libc::stat> {
    // SAFETY: a zeroed stat for fstat to fill.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: valid descriptor and buffer.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat)
}

/// Creates `path` under `root` (and its missing parents, as directories) when nothing is there.
/// A symbolic link that leads nowhere is not followed into a new file: that fails.
fn create_in_root(root: &OwnedFd, path: &str, kind: SystemPath) -> Result<(), String> {
    let trimmed = path.trim_matches('/');
    let (parent, name) = match trimmed.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => ("", trimmed),
    };
    let parent_fd = match open_in_root(root, parent, libc::O_PATH | libc::O_DIRECTORY) {
        Ok(fd) => fd,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) && !parent.is_empty() => {
            create_in_root(root, parent, SystemPath::Directory)?;
            open_in_root(root, parent, libc::O_PATH | libc::O_DIRECTORY)
                .map_err(|error| format!("cannot open /{parent}: {error}"))?
        }
        Err(error) => return Err(format!("cannot open /{parent}: {error}")),
    };
    let name_c = CString::new(name).map_err(|_| "a NUL byte".to_string())?;
    // SAFETY: a zeroed stat for fstatat to fill; valid descriptor and name.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::fstatat(
            parent_fd.as_raw_fd(),
            name_c.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Err(format!(
            "/{trimmed} is a symbolic link to nothing in the image; it is left alone and cannot be protected"
        ));
    }
    // O_PATH descriptors cannot create: reopen the parent for that.
    let parent_dir = open_in_root(root, parent, libc::O_RDONLY | libc::O_DIRECTORY)
        .map_err(|error| format!("cannot open /{parent}: {error}"))?;
    let created = match kind {
        // SAFETY: valid descriptor and name.
        SystemPath::Directory => unsafe {
            libc::mkdirat(parent_dir.as_raw_fd(), name_c.as_ptr(), 0o755) == 0
                && libc::fchmodat(parent_dir.as_raw_fd(), name_c.as_ptr(), 0o755, 0) == 0
        },
        SystemPath::File => {
            // SAFETY: valid descriptor and name; the new file is closed at once.
            let fd = unsafe {
                libc::openat(
                    parent_dir.as_raw_fd(),
                    name_c.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o644,
                )
            };
            // SAFETY: as above.
            fd >= 0 && unsafe { libc::fchmod(fd, 0o644) == 0 && libc::close(fd) == 0 }
        }
    };
    if !created {
        return Err(format!(
            "cannot create /{trimmed}: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Makes what `path` names under `root` read-only in this mount namespace: a read-only bind of
/// it onto itself. The image holds no `SYS_ADMIN`, so it can neither remount nor unmount it, and
/// renaming or removing a mount point gives `EBUSY`.
fn bind_read_only(object: &OwnedFd, path: &str) -> Result<(), String> {
    let empty = c"";
    // SAFETY: valid descriptor, empty path with AT_EMPTY_PATH.
    let tree = unsafe {
        libc::syscall(
            libc::SYS_open_tree,
            object.as_raw_fd(),
            empty.as_ptr(),
            OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC | libc::AT_EMPTY_PATH as u32,
        )
    };
    if tree < 0 {
        return Err(format!(
            "cannot copy {path}: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: a descriptor the kernel just returned that nothing else owns.
    let tree = unsafe { OwnedFd::from_raw_fd(tree as RawFd) };
    let attr = MountAttr {
        attr_set: MOUNT_ATTR_RDONLY,
        attr_clr: 0,
        propagation: 0,
        userns_fd: 0,
    };
    // SAFETY: valid descriptor and a correctly sized mount_attr.
    if unsafe {
        libc::syscall(
            libc::SYS_mount_setattr,
            tree.as_raw_fd(),
            empty.as_ptr(),
            libc::AT_EMPTY_PATH,
            &attr as *const MountAttr,
            size_of::<MountAttr>(),
        )
    } != 0
    {
        return Err(format!(
            "cannot make {path} read-only: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: valid descriptors and empty paths with the EMPTY_PATH flags.
    if unsafe {
        libc::syscall(
            libc::SYS_move_mount,
            tree.as_raw_fd(),
            empty.as_ptr(),
            object.as_raw_fd(),
            empty.as_ptr(),
            MOVE_MOUNT_F_EMPTY_PATH | MOVE_MOUNT_T_EMPTY_PATH,
        )
    } != 0
    {
        return Err(format!(
            "cannot attach the read-only copy of {path}: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Makes `path` (resolved under `root` as the kernel resolves it, symbolic links followed but never
/// out of `root`) unrunnable: `/dev/null`, read-only and `noexec`, is bound over it. Called in the
/// mount namespace the kernel starts its own helpers in, where the kernel's `exec` of the path then
/// fails before any loader or library is read, whatever the image wrote. A path that is not there
/// is reported and left alone: the kernel's call fails the same way. Returns the report line.
pub fn block_kernel_helper(root: &str, path: &str) -> Result<String, String> {
    let open_dir = |name: &str| -> Result<OwnedFd, String> {
        let name_c = CString::new(name).map_err(|_| "a NUL byte".to_string())?;
        // SAFETY: a valid path; the descriptor is owned below.
        let fd = unsafe { libc::open(name_c.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(format!(
                "cannot open {name}: {}",
                io::Error::last_os_error()
            ));
        }
        // SAFETY: a descriptor the kernel just returned that nothing else owns.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    };
    let root_fd = open_dir(root)?;
    let object = match open_in_root(&root_fd, path, libc::O_PATH) {
        Ok(fd) => fd,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
            return Ok(format!("{path}: absent"));
        }
        Err(error) => return Err(format!("cannot open {path}: {error}")),
    };
    let null = open_dir("/dev/null")?;
    let null_stat = fstat(&null).map_err(|error| format!("cannot inspect /dev/null: {error}"))?;
    let is_null = |stat: &libc::stat| {
        stat.st_mode & libc::S_IFMT == libc::S_IFCHR && stat.st_rdev == null_stat.st_rdev
    };
    let before = fstat(&object).map_err(|error| format!("cannot inspect {path}: {error}"))?;
    if before.st_mode & libc::S_IFMT == libc::S_IFDIR {
        return Err(format!("{path} is a directory"));
    }
    if !is_null(&before) {
        let empty = c"";
        // SAFETY: valid descriptor, empty path with AT_EMPTY_PATH.
        let tree = unsafe {
            libc::syscall(
                libc::SYS_open_tree,
                null.as_raw_fd(),
                empty.as_ptr(),
                OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC | libc::AT_EMPTY_PATH as u32,
            )
        };
        if tree < 0 {
            return Err(format!(
                "cannot copy /dev/null: {}",
                io::Error::last_os_error()
            ));
        }
        // SAFETY: a descriptor the kernel just returned that nothing else owns.
        let tree = unsafe { OwnedFd::from_raw_fd(tree as RawFd) };
        let attr = MountAttr {
            attr_set: MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOEXEC | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV,
            attr_clr: 0,
            propagation: 0,
            userns_fd: 0,
        };
        // SAFETY: valid descriptor and a correctly sized mount_attr.
        if unsafe {
            libc::syscall(
                libc::SYS_mount_setattr,
                tree.as_raw_fd(),
                empty.as_ptr(),
                libc::AT_EMPTY_PATH,
                &attr as *const MountAttr,
                size_of::<MountAttr>(),
            )
        } != 0
        {
            return Err(format!(
                "cannot make the /dev/null copy read-only and noexec: {}",
                io::Error::last_os_error()
            ));
        }
        // SAFETY: valid descriptors and empty paths with the EMPTY_PATH flags.
        if unsafe {
            libc::syscall(
                libc::SYS_move_mount,
                tree.as_raw_fd(),
                empty.as_ptr(),
                object.as_raw_fd(),
                empty.as_ptr(),
                MOVE_MOUNT_F_EMPTY_PATH | MOVE_MOUNT_T_EMPTY_PATH,
            )
        } != 0
        {
            return Err(format!(
                "cannot bind /dev/null over {path}: {}",
                io::Error::last_os_error()
            ));
        }
    }
    // What the kernel now finds at the path.
    let check = open_in_root(&root_fd, path, libc::O_PATH)
        .map_err(|error| format!("cannot open {path} after blocking it: {error}"))?;
    let after = fstat(&check).map_err(|error| format!("cannot inspect {path}: {error}"))?;
    if !is_null(&after) {
        return Err(format!("{path} is not /dev/null after blocking it"));
    }
    let target = std::fs::read_link(format!("/proc/self/fd/{}", object.as_raw_fd()))
        .map(|link| link.display().to_string())
        .unwrap_or_default();
    let shown = match target.strip_prefix(root.trim_end_matches('/')) {
        Some(inside) if inside != path && !inside.is_empty() => format!("{path} -> {inside}"),
        _ => path.to_string(),
    };
    Ok(format!("{shown}: /dev/null"))
}

/// The paths the kernel reads when it runs a helper on its own (`/sbin/request-key`, its
/// configuration, `/sbin/modprobe`), made read-only in the image's mount namespace before the
/// image starts. `root` is `/` except in tests.
///
/// Each path is resolved as the image would resolve it, with symbolic links followed but never out
/// of `root` (`/sbin -> usr/sbin` protects `/usr/sbin`). A path the image does not have is created
/// first (an empty directory or file), so the image cannot create it later; a symbolic link that
/// leads nowhere fails, by name. Then each is checked to be on a read-only mount. Returns what
/// was protected, as `path` or `path -> target`.
pub fn protect_system_paths(
    root: &str,
    paths: &[(&str, SystemPath)],
) -> Result<Vec<String>, String> {
    let root_fd = {
        let name = CString::new(root).map_err(|_| "a NUL byte".to_string())?;
        // SAFETY: a valid path; the descriptor is owned below.
        let fd = unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(format!(
                "cannot open {root}: {}",
                io::Error::last_os_error()
            ));
        }
        // SAFETY: a descriptor the kernel just returned that nothing else owns.
        unsafe { OwnedFd::from_raw_fd(fd) }
    };
    let mut done: Vec<(libc::dev_t, libc::ino_t)> = Vec::new();
    let mut protected = Vec::new();
    for &(path, kind) in paths {
        let object = match open_in_root(&root_fd, path, libc::O_PATH) {
            Ok(fd) => fd,
            Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
                create_in_root(&root_fd, path, kind)?;
                open_in_root(&root_fd, path, libc::O_PATH)
                    .map_err(|error| format!("cannot open {path} after creating it: {error}"))?
            }
            Err(error) => return Err(format!("cannot open {path}: {error}")),
        };
        let stat = fstat(&object).map_err(|error| format!("cannot inspect {path}: {error}"))?;
        let target = std::fs::read_link(format!("/proc/self/fd/{}", object.as_raw_fd()))
            .map(|link| link.display().to_string())
            .unwrap_or_default();
        let shown = match target.strip_prefix(root.trim_end_matches('/')) {
            Some(inside) if inside != path && !inside.is_empty() => format!("{path} -> {inside}"),
            _ => path.to_string(),
        };
        if !done.contains(&(stat.st_dev, stat.st_ino)) {
            bind_read_only(&object, path)?;
            done.push((stat.st_dev, stat.st_ino));
        }
        // What the image now finds at the path is on a read-only mount.
        let check = open_in_root(&root_fd, path, libc::O_PATH)
            .map_err(|error| format!("cannot open {path} after protecting it: {error}"))?;
        // SAFETY: a zeroed statvfs for fstatvfs to fill; valid descriptor.
        let mut fs: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstatvfs(check.as_raw_fd(), &mut fs) } != 0
            || fs.f_flag & libc::ST_RDONLY == 0
        {
            return Err(format!("{shown} is not read-only after protecting it"));
        }
        protected.push(shown);
    }
    Ok(protected)
}

/// Creates each declared mount path that the image does not have (`0755` directories owned by
/// root, missing parents too), resolved under `root` as the image would resolve it. Runs before
/// the immutable attribute is set, which would otherwise keep a path at the top level from ever
/// being created. A path that exists, as anything, is left alone; one that cannot be created is
/// reported and only its mounts fail later. Returns one line per created or skipped path.
pub fn create_declared_paths(root: &str, paths: &[&str]) -> Vec<String> {
    let root_fd = match CString::new(root).ok().map(|name| {
        // SAFETY: a valid path.
        unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        }
    }) {
        // SAFETY: a descriptor the kernel just returned that nothing else owns.
        Some(fd) if fd >= 0 => unsafe { OwnedFd::from_raw_fd(fd) },
        _ => return vec![format!("declared paths: skipped, cannot open {root}")],
    };
    let mut lines = Vec::new();
    for path in paths {
        match open_in_root(&root_fd, path, libc::O_PATH) {
            Ok(_) => {}
            Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
                match create_in_root(&root_fd, path, SystemPath::Directory) {
                    Ok(()) => lines.push(format!("created: {path}")),
                    Err(reason) => lines.push(format!("not created: {path} ({reason})")),
                }
            }
            Err(error) => lines.push(format!("not created: {path} ({error})")),
        }
    }
    lines
}

/// Sets the immutable attribute on each directory (resolved under `root` as above), so no entry in
/// it can be created, removed or renamed by anyone without `CAP_LINUX_IMMUTABLE` (the image never
/// holds it). Without it the image could replace the `/sbin` link or move `/usr` aside and put its
/// own `sbin` where the kernel looks for its helpers, outside every bind. So a directory that
/// cannot take the attribute (a filesystem without it, or a broker without the capability) is an
/// error naming it. Returns one line per directory.
pub fn make_immutable(root: &str, directories: &[&str]) -> Result<Vec<String>, String> {
    let root_fd = CString::new(root)
        .ok()
        .map(|name| {
            // SAFETY: a valid path.
            unsafe {
                libc::open(
                    name.as_ptr(),
                    libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
                )
            }
        })
        .filter(|fd| *fd >= 0)
        // SAFETY: a descriptor the kernel just returned that nothing else owns.
        .map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
        .ok_or_else(|| {
            format!(
                "immutable: cannot open {root}: {}",
                io::Error::last_os_error()
            )
        })?;
    directories
        .iter()
        .map(|directory| {
            open_in_root(&root_fd, directory, libc::O_RDONLY | libc::O_DIRECTORY)
                .and_then(|fd| {
                    let mut flags: libc::c_int = 0;
                    // SAFETY: valid descriptor and an int for the flags.
                    if unsafe { libc::ioctl(fd.as_raw_fd(), FS_IOC_GETFLAGS as _, &mut flags) } != 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    flags |= FS_IMMUTABLE_FL;
                    // SAFETY: as above.
                    if unsafe { libc::ioctl(fd.as_raw_fd(), FS_IOC_SETFLAGS as _, &flags) } != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                })
                .map(|()| format!("immutable: {directory} set"))
                .map_err(|error| {
                    let hint = match error.raw_os_error() {
                        Some(libc::EPERM) => ", the broker needs CAP_LINUX_IMMUTABLE",
                        Some(libc::ENOTTY | libc::EOPNOTSUPP | libc::EINVAL) => {
                            ", the filesystem has no immutable attribute"
                        }
                        _ => "",
                    };
                    format!(
                        "immutable: cannot set it on {directory} ({error}{hint}); without it the image could replace the path the kernel runs its helpers from"
                    )
                })
        })
        .collect()
}

/// Asks the kernel for the filesystem types the broker mounts, so their modules load while the
/// kernel can still run its module loader.
pub fn load_filesystems() {
    let known = std::fs::read_to_string("/proc/filesystems").unwrap_or_default();
    for fstype in [c"nfs4", c"tmpfs"] {
        // Built in or already loaded: nothing to ask for.
        if filesystem_known(&known, fstype.to_str().unwrap_or_default()) {
            continue;
        }
        // SAFETY: a NUL-terminated name; the context descriptor is closed at once.
        let fd = unsafe { libc::syscall(libc::SYS_fsopen, fstype.as_ptr(), FSOPEN_CLOEXEC) };
        if fd >= 0 {
            // SAFETY: the descriptor fsopen just returned.
            unsafe { libc::close(fd as RawFd) };
        }
    }
}

/// Whether `/proc/filesystems` (`[nodev]\t<name>` per line) lists `name`.
fn filesystem_known(filesystems: &str, name: &str) -> bool {
    filesystems
        .lines()
        .any(|line| line.rsplit('\t').next() == Some(name))
}

/// Least time left on the request deadline for a new mount to still be attached. The broker kills
/// a helper at the deadline; a mount attached just before that kill would land without the broker
/// ever learning its ID.
pub const ATTACH_MARGIN: std::time::Duration = std::time::Duration::from_secs(2);

/// Refuses to attach a mount whose request is about to reach its deadline. The detached mount is
/// released when its descriptor closes.
fn in_time(attach_by: Option<Instant>) -> Result<(), Refusal> {
    match attach_by {
        Some(deadline) if Instant::now() + ATTACH_MARGIN > deadline => Err((
            Errno::Etimedout,
            "the server answered too close to the request deadline; nothing was mounted".into(),
        )),
        _ => Ok(()),
    }
}

/// How many mounts the broker stacks on one path at most.
pub const MOST_STACKED: usize = 4;

/// Refuses another mount on `target` once the broker's mounts there reach [`MOST_STACKED`].
fn room_on(target: &str, made: &BTreeSet<u64>) -> Result<(), Refusal> {
    let stacked = mounts()
        .iter()
        .filter(|(id, point, _)| point == target && made.contains(id))
        .count();
    if stacked >= MOST_STACKED {
        return Err((
            Errno::Ebusy,
            format!("{target} already has {stacked} mounts; unmount one first"),
        ));
    }
    Ok(())
}

/// Carries out a checked plan. `made` is every mount ID the broker made; `attach_by` is the
/// request deadline.
pub fn run(
    plan: &Plan,
    made: &BTreeSet<u64>,
    attach_by: Option<Instant>,
) -> Result<Option<u64>, Refusal> {
    match plan {
        Plan::Nfs { target, .. } | Plan::Tmpfs { target, .. } | Plan::Bind { target, .. } => {
            room_on(target, made)?
        }
        _ => {}
    }
    match plan {
        Plan::Nfs {
            target,
            source,
            params,
            attributes,
            ..
        } => {
            let at = open_dir(libc::AT_FDCWD, target, false)?;
            let mount = new_mount("nfs4", Some(source), params, *attributes)?;
            in_time(attach_by)?;
            attach(&mount, &at, target).map(Some)
        }
        Plan::Tmpfs {
            target,
            params,
            attributes,
        } => {
            let at = open_dir(libc::AT_FDCWD, target, false)?;
            let mount = new_mount("tmpfs", Some(BROKER_SOURCE), params, *attributes)?;
            in_time(attach_by)?;
            attach(&mount, &at, target).map(Some)
        }
        Plan::Bind { from, target } => {
            let source = open_dir(libc::AT_FDCWD, from, false)?;
            if !made.contains(&mount_id(&source)?) {
                return Err((
                    Errno::Eperm,
                    format!("{from} is not on a mount the broker made"),
                ));
            }
            let at = open_dir(libc::AT_FDCWD, target, false)?;
            let empty = c("")?;
            // SAFETY: valid descriptor, empty path with AT_EMPTY_PATH.
            let tree = owned(
                unsafe {
                    libc::syscall(
                        libc::SYS_open_tree,
                        source.as_raw_fd(),
                        empty.as_ptr(),
                        OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC | libc::AT_EMPTY_PATH as u32,
                    )
                },
                &format!("cannot copy {from}"),
            )?;
            let attr = MountAttr {
                attr_set: MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV,
                attr_clr: 0,
                propagation: 0,
                userns_fd: 0,
            };
            // SAFETY: valid descriptor and a correctly sized mount_attr.
            if unsafe {
                libc::syscall(
                    libc::SYS_mount_setattr,
                    tree.as_raw_fd(),
                    empty.as_ptr(),
                    libc::AT_EMPTY_PATH,
                    &attr as *const MountAttr,
                    size_of::<MountAttr>(),
                )
            } != 0
            {
                return Err(os_error("cannot set nosuid,nodev on the bind"));
            }
            in_time(attach_by)?;
            attach(&tree, &at, target).map(Some)
        }
        Plan::Umount { target, flags } => {
            let (parent, name) = target
                .rsplit_once('/')
                .map(|(parent, name)| (if parent.is_empty() { "/" } else { parent }, name))
                .ok_or_else(|| (Errno::Einval, format!("'{target}' has no parent")))?;
            let parent_fd = open_dir(libc::AT_FDCWD, parent, false)?;
            let id = {
                let child = open_dir(parent_fd.as_raw_fd(), name, true)?;
                mount_id(&child)?
            };
            if !made.contains(&id) {
                return Err((
                    Errno::Eperm,
                    format!("the mount at {target} was not made by the broker"),
                ));
            }
            // The parent stays open, so the name is looked up in the directory that was checked.
            let path = c(&format!("/proc/self/fd/{}/{name}", parent_fd.as_raw_fd()))?;
            // SAFETY: valid path; UMOUNT_NOFOLLOW refuses a link as the last component.
            if unsafe { libc::umount2(path.as_ptr(), flags | libc::UMOUNT_NOFOLLOW) } != 0 {
                return Err(os_error(&format!("cannot unmount {target}")));
            }
            Ok(Some(id))
        }
    }
}

/// How long a request waits for the image's proxy to listen on its port. A mount helper may
/// start the proxy and ask for the mount at once, before the proxy listens.
pub const LISTENER_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// [`port_held_by`], polled until it holds or `until`, then its last answer.
pub fn wait_for_port(
    port: u16,
    procs: &[std::path::PathBuf],
    until: Instant,
) -> Result<(), Refusal> {
    loop {
        match port_held_by(port, procs) {
            Ok(()) => return Ok(()),
            Err(refusal) if Instant::now() >= until => return Err(refusal),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

/// Whether a TCP listener on loopback `port` belongs to a process in the image's cgroups, whose
/// member lists are `procs` (`cgroup.procs` files; one that does not exist yet is skipped).
pub fn port_held_by(port: u16, procs: &[std::path::PathBuf]) -> Result<(), Refusal> {
    let refused = |why: String| Err((Errno::Eperm, why));
    let table = std::fs::read_to_string("/proc/net/tcp")
        .map_err(|e| (Errno::Eio, format!("cannot read the socket table: {e}")))?;
    let inodes: BTreeSet<String> = table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (address, port_hex) = fields.get(1)?.split_once(':')?;
            let listening = *fields.get(3)? == "0A";
            // 0100007F is 127.0.0.1 and 00000000 is 0.0.0.0, in the kernel's byte order.
            let loopback = matches!(address, "0100007F" | "00000000");
            let inode = fields.get(9)?;
            (listening && loopback && u16::from_str_radix(port_hex, 16).ok()? == port)
                .then(|| format!("socket:[{inode}]"))
        })
        .collect();
    if inodes.is_empty() {
        return refused(format!("nothing listens on 127.0.0.1:{port}"));
    }
    let mut members = String::new();
    for file in procs {
        match std::fs::read_to_string(file) {
            Ok(text) => members.push_str(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err((Errno::Eio, format!("cannot read the image cgroup: {e}"))),
        }
    }
    for pid in members.lines().filter(|l| !l.is_empty()) {
        let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Ok(link) = std::fs::read_link(entry.path()) {
                if inodes.contains(link.to_string_lossy().as_ref()) {
                    return Ok(());
                }
            }
        }
    }
    refused(format!(
        "the listener on 127.0.0.1:{port} is not the image's own proxy"
    ))
}

/// Every mount in this namespace as (mount ID, mount point, filesystem type), from
/// `/proc/self/mountinfo` (whose octal escapes are decoded).
/// The source name of every tmpfs the broker makes (and so of binds from one): how a mount the
/// broker made but never recorded is recognised.
pub const BROKER_SOURCE: &str = "alien-mount";

/// Each mount in this namespace: (ID, mount point, filesystem type).
fn mounts() -> Vec<(u64, String, String)> {
    mounts_with_source()
        .into_iter()
        .map(|(id, point, fstype, _)| (id, point, fstype))
        .collect()
}

fn mounts_with_source() -> Vec<(u64, String, String, String)> {
    parse_mountinfo(&std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default())
}

fn parse_mountinfo(text: &str) -> Vec<(u64, String, String, String)> {
    let decode = |field: &str| {
        let bytes = field.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 3 < bytes.len() {
                if let Ok(value) = u8::from_str_radix(&field[i + 1..i + 4], 8) {
                    out.push(value);
                    i += 4;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    text.lines()
        .filter_map(|line| {
            let (head, tail) = line.split_once(" - ")?;
            let mut fields = head.split(' ');
            let id = fields.next()?.parse().ok()?;
            let point = decode(fields.nth(3)?);
            let mut tail = tail.split(' ');
            let fstype = tail.next()?.to_string();
            let source = decode(tail.next()?);
            Some((id, point, fstype, source))
        })
        .collect()
}

/// Detaches every mount only the broker could have made (NFS, which the image cannot mount, or a
/// tmpfs or bind with [`BROKER_SOURCE`]) that is not in `made`: what a helper killed between
/// attaching and reporting leaves, wherever the image moved it. Every helper runs this first.
pub fn detach_orphans(made: &BTreeSet<u64>) -> usize {
    let mut detached = 0;
    for _ in 0..64 {
        let found: Vec<(u64, String)> = mounts_with_source()
            .into_iter()
            .filter(|(id, _, fstype, source)| {
                !made.contains(id) && (fstype.starts_with("nfs") || source == BROKER_SOURCE)
            })
            .map(|(id, point, _, _)| (id, point))
            .collect();
        let before = detached;
        for (id, point) in found {
            let one = Plan::Umount {
                target: point,
                flags: libc::MNT_DETACH,
            };
            if run(&one, &BTreeSet::from([id]), None).is_ok() {
                detached += 1;
            }
        }
        if detached == before {
            break;
        }
    }
    detached
}

/// Detaches (lazily, `MNT_DETACH`) every NFS mount in this mount namespace, wherever it is,
/// checking by mount ID that the path still shows that mount. Only the broker makes NFS mounts
/// here (the image cannot), so this also takes a mount whose helper was killed before it reported,
/// even one the image moved off its declared path. Repeats until a pass finds none, so mounts
/// stacked on one path all go. Returns how many were detached.
pub fn detach_nfs() -> usize {
    let mut detached = 0;
    for _ in 0..64 {
        let found: Vec<(u64, String)> = mounts()
            .into_iter()
            .filter(|(_, _, fstype)| fstype.starts_with("nfs"))
            .map(|(id, point, _)| (id, point))
            .collect();
        let before = detached;
        for (id, point) in found {
            let one = Plan::Umount {
                target: point,
                flags: libc::MNT_DETACH,
            };
            // Only the top mount on a path matches its ID; the ones below go in later passes.
            if run(&one, &BTreeSet::from([id]), None).is_ok() {
                detached += 1;
            }
        }
        if detached == before {
            break;
        }
    }
    detached
}

#[cfg(test)]
mod tests {
    #[test]
    fn filesystems_already_known_are_not_loaded_again() {
        let listed = "nodev\tsysfs\nnodev\ttmpfs\n\text4\nnodev\tnfs\nnodev\tnfs4\n";
        assert!(super::filesystem_known(listed, "nfs4"));
        assert!(super::filesystem_known(listed, "tmpfs"));
        assert!(!super::filesystem_known("nodev\tnfs\n", "nfs4"));
        assert!(!super::filesystem_known("", "tmpfs"));
    }

    use super::*;

    #[test]
    fn missing_declared_paths_are_created_inside_the_root_and_others_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("existing")).unwrap();
        std::fs::write(root.path().join("existing/keep"), "x").unwrap();
        // An absolute link resolves inside the root, as the image would see it.
        std::fs::create_dir(root.path().join("var")).unwrap();
        std::os::unix::fs::symlink("/var", root.path().join("data")).unwrap();
        // A link to nothing is not followed into a new directory.
        std::os::unix::fs::symlink(outside.path().join("gone"), root.path().join("dangling"))
            .unwrap();
        let lines = create_declared_paths(
            root.path().to_str().unwrap(),
            &[
                "/existing",
                "/var/lib/app/scratch",
                "/data/cache",
                "/dangling",
            ],
        );
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert_eq!(lines[0], "created: /var/lib/app/scratch");
        assert_eq!(lines[1], "created: /data/cache");
        assert!(lines[2].starts_with("not created: /dangling"), "{lines:?}");
        assert!(root.path().join("existing/keep").exists());
        for created in ["var/lib", "var/lib/app", "var/lib/app/scratch", "var/cache"] {
            let mode = std::fs::metadata(root.path().join(created))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o7777, 0o755, "{created}");
        }
        assert!(!outside.path().join("gone").exists());
    }

    #[test]
    fn no_attach_in_the_last_moments_of_a_request() {
        let now = Instant::now();
        assert!(in_time(None).is_ok());
        assert!(in_time(Some(now + std::time::Duration::from_secs(10))).is_ok());
        assert_eq!(
            in_time(Some(now + std::time::Duration::from_secs(1))).map_err(|(errno, _)| errno),
            Err(Errno::Etimedout)
        );
    }

    #[test]
    fn mountinfo_gives_id_point_type_and_source() {
        let text = "29 1 0:26 / / rw,relatime shared:1 - ext4 /dev/vda rw\n\
                    41 29 0:40 / /mnt/a\\040b rw,nosuid,nodev - nfs4 127.0.0.1:/ rw,vers=4.2\n\
                    42 41 0:41 / /scratch rw master:3 - tmpfs tmpfs rw,size=1024k\n\
                    43 29 0:42 / /odd\\134 rw - tmpfs tmpfs rw\n";
        assert_eq!(
            parse_mountinfo(text),
            vec![
                (29, "/".into(), "ext4".into(), "/dev/vda".into()),
                (41, "/mnt/a b".into(), "nfs4".into(), "127.0.0.1:/".into()),
                (42, "/scratch".into(), "tmpfs".into(), "tmpfs".into()),
                (43, "/odd\\".into(), "tmpfs".into(), "tmpfs".into()),
            ]
        );
    }
}
