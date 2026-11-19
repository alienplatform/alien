//! What a broker helper does once it is in the image's mount namespace. Runs in a forked child of
//! the single-threaded broker, so ordinary allocation is fine.

use std::{
    collections::BTreeSet,
    ffi::CString,
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
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

/// Carries out a checked plan. `made` is every mount ID the broker made.
pub fn run(plan: &Plan, made: &BTreeSet<u64>) -> Result<Option<u64>, Refusal> {
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
            attach(&mount, &at, target).map(Some)
        }
        Plan::Tmpfs {
            target,
            params,
            attributes,
        } => {
            let at = open_dir(libc::AT_FDCWD, target, false)?;
            let mount = new_mount("tmpfs", Some("tmpfs"), params, *attributes)?;
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

/// Whether a TCP listener on loopback `port` belongs to a process in the image cgroup, whose
/// member list is `procs` (a `cgroup.procs` file).
pub fn port_held_by(port: u16, procs: &str) -> Result<(), Refusal> {
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
    let members = std::fs::read_to_string(procs)
        .map_err(|e| (Errno::Eio, format!("cannot read the image cgroup: {e}")))?;
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

/// Where each mount in this namespace is: mount ID to mount point, from `/proc/self/mountinfo`
/// (whose octal escapes are decoded).
fn mount_points() -> std::collections::BTreeMap<u64, String> {
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
    std::fs::read_to_string("/proc/self/mountinfo")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let id = fields.next()?.parse().ok()?;
            Some((id, decode(fields.nth(3)?)))
        })
        .collect()
}

/// Detaches each of `ids` (lazily, `MNT_DETACH`), checking by mount ID that the path still
/// shows that mount. Returns how many were detached.
pub fn detach(ids: &[u64]) -> usize {
    let points = mount_points();
    let mut detached = 0;
    for id in ids {
        let Some(point) = points.get(id) else {
            continue;
        };
        let one = Plan::Umount {
            target: point.clone(),
            flags: libc::MNT_DETACH,
        };
        if run(&one, &BTreeSet::from([*id])).is_ok() {
            detached += 1;
        }
    }
    detached
}
