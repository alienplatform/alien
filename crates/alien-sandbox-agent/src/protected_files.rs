//! Protect bootstrap files before any image code can replace the next boot's supervisor.

use std::{
    collections::BTreeSet,
    ffi::CString,
    fs, io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    path::{Path, PathBuf},
};

/// Called while single-threaded, before reducing the supervisor's capabilities.
/// Mounts are private to the agent, so provider restore mounts remain untouched.
pub fn protect(binary: &Path, metadata: &Path) -> io::Result<()> {
    // Same-UID image root must not recover writable descriptors or the old mount
    // namespace through /proc/<agent>/{fd,root}, nor attach a debugger.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    private_mount_namespace()?;
    if unsafe {
        libc::mount(
            std::ptr::null(),
            c"/".as_ptr(),
            std::ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            std::ptr::null(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    // Mount every ancestor too: protecting only a file would still let root
    // rename /usr/local and put a replacement binary at the startup path.
    let mut ancestors = BTreeSet::<PathBuf>::new();
    for target in [binary, metadata] {
        for parent in target.ancestors().skip(1) {
            if parent != Path::new("/") {
                ancestors.insert(parent.to_owned());
            }
        }
    }
    for parent in ancestors {
        bind(&parent, false)?;
    }
    bind(binary, true)?;
    bind(metadata, true)?;
    for path in READ_ONLY_KERNEL_PATHS {
        read_only_tree(Path::new(path))
            .map_err(|error| io::Error::new(error.kind(), format!("{path}: {error}")))?;
    }
    Ok(())
}

/// Kernel settings the image could otherwise write as root without any capability: `/proc/sys`
/// files are root-writable by mode, and so is `/proc/sysrq-trigger`. Both act on the whole VM (a
/// `kernel.core_pattern` pipe helper runs as root in the VM's own namespaces). They are bound
/// read-only in this private mount namespace, as container runtimes do; reading still works. The
/// image never holds `SYS_ADMIN`, so it cannot undo the binds. Processes outside this namespace
/// (the provider's, and the mount broker, which writes its own settings) keep their view.
///
/// `/sys` too: root-writable files there act on the whole VM as well (`kernel/uevent_helper` names
/// a program the kernel runs as root on every device event; module parameters, memory settings).
/// The agent writes nothing below `/sys` after this point (the guest cgroup is set up before).
const READ_ONLY_KERNEL_PATHS: [&str; 3] = ["/proc/sys", "/proc/sysrq-trigger", "/sys"];

/// Binds `path` over itself with every mount below it, then makes each of those mounts read-only.
/// Recursive, because a user namespace refuses a plain bind that would uncover locked mounts
/// below (such as `fs/binfmt_misc`), and those must not stay writable either. The flags each
/// mount already has are kept (proc is `noexec`): a user namespace refuses a remount that clears
/// a flag its parent locked. They are read from the mount table rather than by `statvfs`, which
/// would trigger an automount point below.
fn read_only_tree(path: &Path) -> io::Result<()> {
    let target = CString::new(path.as_os_str().as_bytes())?;
    if unsafe {
        libc::mount(
            target.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REC,
            std::ptr::null(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    // This thread's namespace: `self` would name the thread-group leader's.
    let table = fs::read_to_string("/proc/thread-self/mountinfo")?;
    for (point, flags) in mounts_within(&table, path.as_os_str().as_bytes()) {
        let point = CString::new(point)?;
        if unsafe {
            libc::mount(
                std::ptr::null(),
                point.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY | flags,
                std::ptr::null(),
            )
        } != 0
        {
            let error = io::Error::last_os_error();
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "read-only remount of {}: {error}",
                    String::from_utf8_lossy(point.as_bytes())
                ),
            ));
        }
    }
    Ok(())
}

/// The mount points at or below `root`, each once, parents first, with the remount flags to keep
/// (always `nosuid` and `nodev`).
fn mounts_within(table: &str, root: &[u8]) -> Vec<(Vec<u8>, libc::c_ulong)> {
    let mut found: Vec<(Vec<u8>, libc::c_ulong)> = Vec::new();
    for line in table.lines() {
        let mut fields = line.split(' ');
        let (Some(point), Some(options)) = (fields.nth(4), fields.next()) else {
            continue;
        };
        let point = crate::read_only_mounts::unescape(point);
        let below =
            point.len() > root.len() && point.starts_with(root) && point[root.len()] == b'/';
        if point != root && !below {
            continue;
        }
        let mut flags = libc::MS_NOSUID | libc::MS_NODEV;
        for option in options.split(',') {
            flags |= match option {
                "noexec" => libc::MS_NOEXEC,
                "noatime" => libc::MS_NOATIME,
                "nodiratime" => libc::MS_NODIRATIME,
                "relatime" => libc::MS_RELATIME,
                _ => 0,
            };
        }
        // A later entry at the same point covers the earlier one: only it can be remounted.
        if let Some(entry) = found.iter_mut().find(|(seen, _)| *seen == point) {
            entry.1 = flags;
        } else {
            found.push((point, flags));
        }
    }
    found.sort_by_key(|(point, _)| point.len());
    found
}

fn bind(path: &Path, readonly: bool) -> io::Result<()> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "protected path must not be a symlink",
        ));
    }
    let path = CString::new(path.as_os_str().as_bytes())?;
    if unsafe {
        libc::mount(
            path.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if readonly
        && unsafe {
            libc::mount(
                std::ptr::null(),
                path.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND
                    | libc::MS_REMOUNT
                    | libc::MS_RDONLY
                    | libc::MS_NOSUID
                    | libc::MS_NODEV,
                std::ptr::null(),
            )
        } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn private_mount_namespace() -> io::Result<()> {
    if crate::pid_namespace::available() {
        if unsafe { libc::unshare(libc::CLONE_NEWNS) } != 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(());
    }
    // GCP starts root without SYS_ADMIN. A helper retaining the original user
    // namespace writes identity maps after this process unshares. It exits before
    // startup continues; there is no additional runtime supervisor or UID remap.
    // Map every valid UID/GID so arbitrary image workers retain their identities.
    let uid_map = CString::new(format!("/proc/{}/uid_map", std::process::id()))?;
    let gid_map = CString::new(format!("/proc/{}/gid_map", std::process::id()))?;
    let mut pipe = [-1; 2];
    if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let read = unsafe { OwnedFd::from_raw_fd(pipe[0]) };
    let write = unsafe { OwnedFd::from_raw_fd(pipe[1]) };
    let child = unsafe { libc::fork() };
    if child < 0 {
        return Err(io::Error::last_os_error());
    }
    if child == 0 {
        // Only async-signal-safe operations in this short-lived helper.
        unsafe {
            libc::close(write.as_raw_fd());
            let mut ready = 0u8;
            if libc::read(read.as_raw_fd(), (&mut ready as *mut u8).cast(), 1) != 1 || ready != 1 {
                libc::_exit(1);
            }
            for path in [&uid_map, &gid_map] {
                let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
                if fd < 0 {
                    libc::_exit(1);
                }
                let mapping = b"0 0 4294967295\n";
                let result = libc::write(fd, mapping.as_ptr().cast(), mapping.len());
                libc::close(fd);
                if result != mapping.len() as isize {
                    libc::_exit(1);
                }
            }
            libc::_exit(0);
        }
    }
    drop(read);
    let unshared = unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) };
    let error = io::Error::last_os_error();
    let ready = u8::from(unshared == 0);
    let sent = unsafe { libc::write(write.as_raw_fd(), (&ready as *const u8).cast(), 1) };
    drop(write);
    let mut status = 0;
    loop {
        if unsafe { libc::waitpid(child, &mut status, 0) } >= 0 {
            break;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    if unshared != 0 {
        return Err(error);
    }
    if sent != 1 || !libc::WIFEXITED(status) || libc::WEXITSTATUS(status) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cannot map supervisor identities into the private mount namespace",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_within_lists_each_point_below_once_with_its_flags() {
        let table = "\
25 1 0:23 / /proc rw,nosuid,nodev,noexec,relatime shared:12 - proc proc rw
37 25 0:31 / /proc/sys/fs/binfmt_misc rw,relatime shared:13 - autofs systemd-1 rw,fd=32
36 37 0:39 / /proc/sys/fs/binfmt_misc rw,nosuid,nodev,noexec,relatime shared:45 - binfmt_misc binfmt_misc rw
90 25 0:23 /sys /proc/sys rw,nosuid,nodev,noexec,relatime - proc proc rw
91 25 0:23 /sysrq-trigger /proc/sysrq-trigger rw,nosuid,nodev,noexec,relatime - proc proc rw
92 25 0:40 / /proc/sys\\040x rw,noatime - tmpfs tmpfs rw
";
        let keep = libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC | libc::MS_RELATIME;
        assert_eq!(
            mounts_within(table, b"/proc/sys"),
            vec![
                (b"/proc/sys".to_vec(), keep),
                (b"/proc/sys/fs/binfmt_misc".to_vec(), keep),
            ]
        );
        assert_eq!(
            mounts_within(table, b"/proc/sysrq-trigger"),
            vec![(b"/proc/sysrq-trigger".to_vec(), keep)]
        );
    }
    use crate::{exec::ExecIdentity, privilege};
    use std::{
        io::{Read, Seek, SeekFrom, Write},
        os::unix::process::CommandExt,
        process::Command,
    };

    #[test]
    #[ignore = "requires root, SYS_ADMIN and Python 3"]
    fn root_image_cannot_replace_bootstrap_files() {
        const CHILD: &str = "ALIEN_PROTECTION_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let root = PathBuf::from(std::env::var_os("ALIEN_PROTECTION_TEST_ROOT").unwrap());
            let binary = root.join("usr/local/bin/agent");
            let metadata = root.join("opt/alien");
            fs::create_dir_all(binary.parent().unwrap()).unwrap();
            fs::create_dir_all(&metadata).unwrap();
            fs::write(&binary, b"original binary").unwrap();
            let ca = metadata.join("ca.pem");
            let mut writable = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&ca)
                .unwrap();
            writable.write_all(b"old").unwrap();
            protect(&binary, &metadata).expect("protect before image startup");
            // Gateway delivery must still update its preopened trust bundle.
            writable.seek(SeekFrom::Start(0)).unwrap();
            writable.write_all(b"new").unwrap();
            let mut command = Command::new("python3");
            command.args([
                "-c",
                r#"
import errno, os, pathlib, sys
binary, metadata, ancestor = map(pathlib.Path, sys.argv[1:4])
assert os.getuid() == 0
for path in [binary, metadata / 'ca.pem']:
    try:
        with path.open('wb') as f: f.write(b'forged')
    except OSError as e: assert e.errno in [errno.EROFS, errno.EPERM], e
    else: raise AssertionError(f'root overwrote {path}')
for path in [binary, metadata, ancestor]:
    try: path.rename(str(path) + '.replaced')
    except OSError as e: assert e.errno in [errno.EBUSY, errno.EROFS, errno.EPERM], e
    else: raise AssertionError(f'root renamed {path}')
assert (metadata / 'ca.pem').read_bytes() == b'new'
# VM-wide kernel settings: opening for writing is refused, so nothing is ever written even when
# the protection is missing (a write to sysrq-trigger would act at once).
for path in ['/proc/sys/kernel/core_pattern', '/proc/sys/net/core/somaxconn', '/proc/sysrq-trigger']:
    try: fd = os.open(path, os.O_WRONLY)
    except OSError as e: assert e.errno == errno.EROFS, (path, e)
    else:
        os.close(fd)
        raise AssertionError(f'root opened {path} for writing')
# Mounts below /proc/sys are read-only too (binfmt_misc registers interpreters for every exec).
# Only the last entry at a mount point is reachable; earlier ones are covered.
visible = {}
for line in open('/proc/self/mountinfo'):
    fields = line.split()
    visible[fields[4]] = fields[5].split(',')
for point, options in visible.items():
    if point.startswith('/proc/sys/'):
        assert 'ro' in options, (point, options)
# Reading still works.
for path in ['kernel/osrelease', 'net/core/somaxconn', 'vm/overcommit_memory']:
    assert pathlib.Path('/proc/sys', path).read_text().strip(), path
# /sys likewise, every mount below it included.
for path in ['/sys/kernel/uevent_helper', '/sys/kernel/mm/transparent_hugepage/enabled', '/sys/kernel/profiling']:
    if not os.path.exists(path): continue
    try: fd = os.open(path, os.O_WRONLY)
    except OSError as e: assert e.errno == errno.EROFS, (path, e)
    else:
        os.close(fd)
        raise AssertionError(f'root opened {path} for writing')
for point, options in visible.items():
    if point == '/sys' or point.startswith('/sys/'):
        assert 'ro' in options, (point, options)
assert pathlib.Path('/sys/class/net/lo/mtu').read_text().strip()
# The inherited descriptor is CLOEXEC, and root cannot reopen the agent's FD.
try: os.open(f'/proc/{sys.argv[4]}/fd/{sys.argv[5]}', os.O_WRONLY)
except OSError as e: assert e.errno in [errno.EACCES, errno.EPERM], e
else: raise AssertionError('root recovered the writable bootstrap descriptor')
"#,
            ]);
            command
                .arg(&binary)
                .arg(&metadata)
                .arg(root.join("usr"))
                .arg(std::process::id().to_string())
                .arg(writable.as_raw_fd().to_string());
            unsafe {
                command.pre_exec(|| {
                    privilege::restrict_supervisor(false)?;
                    privilege::drop_image_to(ExecIdentity { uid: 0, gid: 0 })
                });
            }
            let output = command.output().expect("run root image attack");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read(&binary).unwrap(), b"original binary");
            writable.seek(SeekFrom::Start(0)).unwrap();
            let mut bytes = Vec::new();
            writable.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"new");
            // Exit releases this process's private mounts before the parent removes fixtures.
            return;
        }
        assert_eq!(unsafe { libc::geteuid() }, 0);
        let temp = tempfile::tempdir().expect("temporary filesystem");
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "protected_files::tests::root_image_cannot_replace_bootstrap_files",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("ALIEN_PROTECTION_TEST_ROOT", temp.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
