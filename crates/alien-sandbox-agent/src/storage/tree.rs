//! The storage directory as the agent reads and writes it: a walk that never follows a symlink,
//! a copy of changed files into agent-only staging, and a restore that writes as root.
//!
//! Every path is resolved one component at a time with `openat(…, O_NOFOLLOW)` from a directory
//! descriptor the agent opened itself, so a command that swaps a directory for a symlink between
//! two steps can make a step fail, never make the agent read or write outside the directory.

use std::collections::HashMap;
use std::ffi::{CStr, CString, OsStr};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use alien_error::AlienError;
use alien_sandbox_storage::{
    Entry, ErrorData as StorageError, FileSource, Manifest, Result as StorageResult, TreeEntry,
    TreeEntryKind, TreeSink,
};

fn tree_io(operation: &str, path: &str, error: io::Error) -> AlienError<StorageError> {
    AlienError::new(StorageError::TreeIo {
        operation: operation.to_string(),
        path: path.to_string(),
        reason: error.to_string(),
    })
}

fn cstring(name: &str) -> io::Result<CString> {
    CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in a name"))
}

fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// Opens a directory without following a symlink at any step.
pub fn open_root(path: &Path) -> io::Result<OwnedFd> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in the path"))?;
    // SAFETY: path is NUL-terminated; the returned fd is owned below.
    let fd = check(unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    // SAFETY: fd was just opened and is owned by nobody else.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn openat(dir: RawFd, name: &str, flags: libc::c_int, mode: libc::mode_t) -> io::Result<OwnedFd> {
    let name = cstring(name)?;
    // SAFETY: dir is a live descriptor; name is NUL-terminated.
    let fd = check(unsafe {
        libc::openat(
            dir,
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            libc::c_uint::from(mode),
        )
    })?;
    // SAFETY: fd was just opened and is owned by nobody else.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn open_dir_at(dir: RawFd, name: &str) -> io::Result<OwnedFd> {
    openat(dir, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)
}

fn fstatat(dir: RawFd, name: &str) -> io::Result<libc::stat> {
    let name = cstring(name)?;
    // SAFETY: zeroed stat is a valid out-parameter.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: dir is live, name NUL-terminated, st a valid pointer.
    check(unsafe { libc::fstatat(dir, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) })?;
    Ok(st)
}

fn fstat(fd: RawFd) -> io::Result<libc::stat> {
    // SAFETY: zeroed stat is a valid out-parameter.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fd is live, st a valid pointer.
    check(unsafe { libc::fstat(fd, &mut st) })?;
    Ok(st)
}

fn readlinkat(dir: RawFd, name: &str) -> io::Result<String> {
    let name = cstring(name)?;
    let mut buffer = vec![0u8; libc::PATH_MAX as usize];
    // SAFETY: buffer is writable for its length.
    let len =
        unsafe { libc::readlinkat(dir, name.as_ptr(), buffer.as_mut_ptr().cast(), buffer.len()) };
    if len < 0 {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(len as usize);
    String::from_utf8(buffer)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "symlink target is not UTF-8"))
}

/// The names in a directory, without `.` and `..`.
fn list_dir(dir: RawFd) -> io::Result<Vec<String>> {
    // fdopendir takes ownership of its descriptor, so it gets a duplicate.
    // SAFETY: dir is live; dup returns a new descriptor or -1.
    let dup = check(unsafe { libc::fcntl(dir, libc::F_DUPFD_CLOEXEC, 0) })?;
    // SAFETY: dup is a live directory descriptor that the stream now owns.
    let stream = unsafe { libc::fdopendir(dup) };
    if stream.is_null() {
        let error = io::Error::last_os_error();
        // SAFETY: fdopendir failed, so dup is still ours to close.
        unsafe { libc::close(dup) };
        return Err(error);
    }
    // SAFETY: a fresh stream starts at offset 0 only after a rewind on a duplicated fd.
    unsafe { libc::rewinddir(stream) };
    let mut names = Vec::new();
    loop {
        // SAFETY: stream is a live DIR*.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            break;
        }
        // SAFETY: d_name is NUL-terminated inside the entry readdir returned.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        let bytes = name.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        match std::str::from_utf8(bytes) {
            Ok(name) => names.push(name.to_string()),
            // A name the manifest cannot carry is skipped, not committed under another name.
            Err(_) => tracing::warn!(
                name = %String::from_utf8_lossy(bytes),
                "persistent storage skips a name that is not UTF-8"
            ),
        }
    }
    // SAFETY: stream is live and closed exactly once.
    unsafe { libc::closedir(stream) };
    names.sort();
    Ok(names)
}

fn mtime_ns(st: &libc::stat) -> i64 {
    st.st_mtime.saturating_mul(1_000_000_000) + st.st_mtime_nsec
}

fn ctime_ns(st: &libc::stat) -> i64 {
    st.st_ctime.saturating_mul(1_000_000_000) + st.st_ctime_nsec
}

/// Whether `exclude` keeps `path` local. A pattern names a file or a directory relative to the
/// storage directory; a trailing `/` is allowed and means the same directory.
pub fn excluded(path: &str, exclude: &[String]) -> bool {
    exclude.iter().any(|pattern| {
        let pattern = pattern.trim_end_matches('/');
        !pattern.is_empty()
            && (path == pattern
                || path
                    .strip_prefix(pattern)
                    .is_some_and(|rest| rest.starts_with('/')))
    })
}

/// Walks the storage directory. Directories, regular files, symlinks and hard links are
/// recorded; other kinds (sockets, FIFOs, devices) and other file systems mounted inside are
/// skipped. Nothing is followed.
pub fn walk(root: &OwnedFd, exclude: &[String]) -> io::Result<Vec<TreeEntry>> {
    let device = fstat(root.as_raw_fd())?.st_dev;
    let mut entries = Vec::new();
    let mut first_name: HashMap<(u64, u64), String> = HashMap::new();
    let mut stack: Vec<(String, OwnedFd)> = vec![(String::new(), dup(root)?)];
    while let Some((prefix, dir)) = stack.pop() {
        for name in list_dir(dir.as_raw_fd())? {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if excluded(&path, exclude) {
                continue;
            }
            // Gone between the listing and the stat: the next walk sees the new state.
            let st = match fstatat(dir.as_raw_fd(), &name) {
                Ok(st) => st,
                Err(error) if error.raw_os_error() == Some(libc::ENOENT) => continue,
                Err(error) => return Err(error),
            };
            let kind = match st.st_mode & libc::S_IFMT {
                libc::S_IFDIR => {
                    if st.st_dev != device {
                        continue;
                    }
                    match open_dir_at(dir.as_raw_fd(), &name) {
                        Ok(child) => {
                            let opened = fstat(child.as_raw_fd())?;
                            // Swapped after the stat: skip; the next walk sees the new state.
                            if opened.st_ino != st.st_ino || opened.st_dev != st.st_dev {
                                continue;
                            }
                            stack.push((path.clone(), child));
                        }
                        Err(error)
                            if matches!(
                                error.raw_os_error(),
                                Some(libc::ENOENT | libc::ELOOP | libc::ENOTDIR)
                            ) =>
                        {
                            continue
                        }
                        Err(error) => return Err(error),
                    }
                    TreeEntryKind::Dir
                }
                libc::S_IFLNK => match readlinkat(dir.as_raw_fd(), &name) {
                    Ok(target) => TreeEntryKind::Symlink { target },
                    Err(error) if error.raw_os_error() == Some(libc::ENOENT) => continue,
                    Err(error) => return Err(error),
                },
                libc::S_IFREG => {
                    let inode = (st.st_dev as u64, st.st_ino as u64);
                    match (st.st_nlink > 1).then(|| first_name.get(&inode)).flatten() {
                        Some(target) => TreeEntryKind::HardLink {
                            target: target.clone(),
                        },
                        None => {
                            if st.st_nlink > 1 {
                                first_name.insert(inode, path.clone());
                            }
                            TreeEntryKind::File {
                                size: st.st_size as u64,
                                change_token: ctime_ns(&st),
                            }
                        }
                    }
                }
                _ => continue,
            };
            entries.push(TreeEntry {
                path,
                kind,
                mode: st.st_mode & 0o7777,
                uid: st.st_uid,
                gid: st.st_gid,
                mtime_ns: mtime_ns(&st),
            });
        }
    }
    Ok(entries)
}

fn dup(fd: &OwnedFd) -> io::Result<OwnedFd> {
    fd.try_clone()
}

/// Opens the directory holding `path` (relative to `root`) one component at a time.
fn open_parent(root: &OwnedFd, path: &str) -> io::Result<(OwnedFd, String)> {
    let mut components: Vec<&str> = path.split('/').collect();
    let name = components
        .pop()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty path"))?;
    let mut dir = dup(root)?;
    for component in components {
        dir = open_dir_at(dir.as_raw_fd(), component)?;
    }
    Ok((dir, name.to_string()))
}

/// Opens a regular file under `root` for reading, refusing a symlink at any step.
pub fn open_file(root: &OwnedFd, path: &str) -> io::Result<File> {
    let (dir, name) = open_parent(root, path)?;
    let fd = openat(dir.as_raw_fd(), &name, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
    let st = fstat(fd.as_raw_fd())?;
    if st.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(File::from(fd))
}

/// The files a commit has to read: regular files whose size, mtime or change token differ
/// from the manifest HEAD names.
pub fn changed_files<'a>(walk: &'a [TreeEntry], head: Option<&Manifest>) -> Vec<&'a TreeEntry> {
    walk.iter()
        .filter(|item| {
            let TreeEntryKind::File { size, change_token } = item.kind else {
                return false;
            };
            !head.and_then(|m| m.entry(&item.path)).is_some_and(|old| {
                matches!(
                    old.kind,
                    alien_sandbox_storage::EntryKind::File {
                        size: old_size,
                        change_token: old_token,
                        ..
                    } if old_size == size && old_token == change_token && old.mtime_ns == item.mtime_ns
                )
            })
        })
        .collect()
}

/// Copies files from the storage directory into staging, as flat numbered files, so a commit
/// reads a consistent copy after the guest is thawed. Returns the staged bytes.
pub struct Staging {
    dir: OwnedFd,
    files: HashMap<String, String>,
}

impl Staging {
    /// Empties `dir` (owned by the agent, mode 0700) and stages into it.
    pub fn new(dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
        for entry in std::fs::read_dir(dir)? {
            std::fs::remove_file(entry?.path())?;
        }
        Ok(Self {
            dir: open_root(dir)?,
            files: HashMap::new(),
        })
    }

    /// Copies one file. A file that vanished is left out; the commit will not see it either
    /// once the walk is redone, and this walk's entry for it is dropped by the caller.
    pub fn copy(&mut self, root: &OwnedFd, path: &str) -> io::Result<Option<u64>> {
        let mut source = match open_file(root, path) {
            Ok(file) => file,
            Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(None),
            Err(error) => return Err(error),
        };
        let staged = self.files.len().to_string();
        let mut target = File::from(openat(
            self.dir.as_raw_fd(),
            &staged,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?);
        let copied = io::copy(&mut source, &mut target)?;
        self.files.insert(path.to_string(), staged);
        Ok(Some(copied))
    }

    /// Whether `path` was staged.
    pub fn contains(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }
}

impl FileSource for Staging {
    fn open(&mut self, path: &str) -> StorageResult<Box<dyn Read + Send + '_>> {
        let staged = self.files.get(path).ok_or_else(|| {
            tree_io(
                "read",
                path,
                io::Error::new(io::ErrorKind::NotFound, "not staged"),
            )
        })?;
        let file = openat(self.dir.as_raw_fd(), staged, libc::O_RDONLY, 0)
            .map_err(|e| tree_io("read", path, e))?;
        Ok(Box::new(File::from(file)))
    }
}

/// Writes a restored tree under the storage directory as root, with owners, modes and mtimes,
/// replacing whatever an entry's name holds and never following a symlink.
pub struct DirSink {
    root: OwnedFd,
    file: Option<File>,
    /// When false (tests as an unprivileged user), owners are not applied.
    chown: bool,
}

impl DirSink {
    /// A sink writing under `root`.
    pub fn new(root: OwnedFd, chown: bool) -> Self {
        Self {
            root,
            file: None,
            chown,
        }
    }

    fn remove(&self, dir: RawFd, name: &str) -> io::Result<()> {
        let c = cstring(name)?;
        // SAFETY: dir is live and c NUL-terminated.
        if unsafe { libc::unlinkat(dir, c.as_ptr(), 0) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ENOENT) => Ok(()),
            Some(libc::EISDIR) | Some(libc::EPERM) => {
                remove_tree(dir, name)?;
                Ok(())
            }
            _ => Err(error),
        }
    }

    fn owner_mode_time(&self, fd: RawFd, entry: &Entry) -> io::Result<()> {
        if self.chown {
            // SAFETY: fd is live.
            check(unsafe { libc::fchown(fd, entry.uid, entry.gid) })?;
        }
        // SAFETY: fd is live.
        check(unsafe { libc::fchmod(fd, entry.mode & 0o7777) })?;
        let times = mtime_spec(entry.mtime_ns);
        // SAFETY: times points at two timespecs.
        check(unsafe { libc::futimens(fd, times.as_ptr()) })?;
        Ok(())
    }
}

fn mtime_spec(mtime_ns: i64) -> [libc::timespec; 2] {
    let mtime = libc::timespec {
        tv_sec: mtime_ns.div_euclid(1_000_000_000) as libc::time_t,
        tv_nsec: mtime_ns.rem_euclid(1_000_000_000) as _,
    };
    [mtime, mtime]
}

/// Removes a directory tree below `dir`, without following symlinks.
fn remove_tree(dir: RawFd, name: &str) -> io::Result<()> {
    let child = open_dir_at(dir, name)?;
    for entry in list_dir(child.as_raw_fd())? {
        let c = cstring(&entry)?;
        // SAFETY: child is live and c NUL-terminated.
        if unsafe { libc::unlinkat(child.as_raw_fd(), c.as_ptr(), 0) } != 0 {
            remove_tree(child.as_raw_fd(), &entry)?;
        }
    }
    let c = cstring(name)?;
    // SAFETY: dir is live and c NUL-terminated.
    check(unsafe { libc::unlinkat(dir, c.as_ptr(), libc::AT_REMOVEDIR) })?;
    Ok(())
}

impl TreeSink for DirSink {
    fn dir(&mut self, entry: &Entry) -> StorageResult<()> {
        let io = |e| tree_io("create directory", &entry.path, e);
        let (parent, name) = open_parent(&self.root, &entry.path).map_err(io)?;
        let c = cstring(&name).map_err(io)?;
        // SAFETY: parent is live and c NUL-terminated.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), c.as_ptr(), 0o700) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EEXIST) {
                return Err(io(error));
            }
            // A non-directory under the name is replaced.
            if open_dir_at(parent.as_raw_fd(), &name).is_err() {
                self.remove(parent.as_raw_fd(), &name).map_err(io)?;
                // SAFETY: as above.
                check(unsafe { libc::mkdirat(parent.as_raw_fd(), c.as_ptr(), 0o700) })
                    .map_err(io)?;
            }
        }
        Ok(())
    }

    fn begin_file(&mut self, entry: &Entry) -> StorageResult<()> {
        let io = |e| tree_io("create file", &entry.path, e);
        let (parent, name) = open_parent(&self.root, &entry.path).map_err(io)?;
        self.remove(parent.as_raw_fd(), &name).map_err(io)?;
        let fd = openat(
            parent.as_raw_fd(),
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
        .map_err(io)?;
        self.file = Some(File::from(fd));
        Ok(())
    }

    fn append(&mut self, bytes: &[u8]) -> StorageResult<()> {
        let file = self.file.as_mut().ok_or_else(|| {
            tree_io(
                "write",
                "",
                io::Error::new(io::ErrorKind::Other, "no file begun"),
            )
        })?;
        file.write_all(bytes).map_err(|e| tree_io("write", "", e))
    }

    fn end_file(&mut self, entry: &Entry) -> StorageResult<()> {
        let file = self.file.take().ok_or_else(|| {
            tree_io(
                "finish",
                &entry.path,
                io::Error::new(io::ErrorKind::Other, "no file begun"),
            )
        })?;
        self.owner_mode_time(file.as_raw_fd(), entry)
            .map_err(|e| tree_io("finish", &entry.path, e))
    }

    fn symlink(&mut self, entry: &Entry, target: &str) -> StorageResult<()> {
        let io = |e| tree_io("create symlink", &entry.path, e);
        let (parent, name) = open_parent(&self.root, &entry.path).map_err(io)?;
        self.remove(parent.as_raw_fd(), &name).map_err(io)?;
        let c_target = cstring(target).map_err(io)?;
        let c_name = cstring(&name).map_err(io)?;
        // SAFETY: both strings are NUL-terminated; parent is live.
        check(unsafe { libc::symlinkat(c_target.as_ptr(), parent.as_raw_fd(), c_name.as_ptr()) })
            .map_err(io)?;
        if self.chown {
            // SAFETY: as above; AT_SYMLINK_NOFOLLOW changes the link itself.
            check(unsafe {
                libc::fchownat(
                    parent.as_raw_fd(),
                    c_name.as_ptr(),
                    entry.uid,
                    entry.gid,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            })
            .map_err(io)?;
        }
        let times = mtime_spec(entry.mtime_ns);
        // SAFETY: as above.
        check(unsafe {
            libc::utimensat(
                parent.as_raw_fd(),
                c_name.as_ptr(),
                times.as_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        })
        .map_err(io)?;
        Ok(())
    }

    fn hard_link(&mut self, entry: &Entry, target: &str) -> StorageResult<()> {
        let io = |e| tree_io("create hard link", &entry.path, e);
        let (from_dir, from_name) = open_parent(&self.root, target).map_err(io)?;
        let (parent, name) = open_parent(&self.root, &entry.path).map_err(io)?;
        self.remove(parent.as_raw_fd(), &name).map_err(io)?;
        let c_from = cstring(&from_name).map_err(io)?;
        let c_name = cstring(&name).map_err(io)?;
        // SAFETY: both directories are live; flags 0 never follows a symlink at the source.
        check(unsafe {
            libc::linkat(
                from_dir.as_raw_fd(),
                c_from.as_ptr(),
                parent.as_raw_fd(),
                c_name.as_ptr(),
                0,
            )
        })
        .map_err(io)?;
        Ok(())
    }

    fn finish(&mut self, dirs: &[&Entry]) -> StorageResult<()> {
        for entry in dirs {
            let io = |e| tree_io("finish directory", &entry.path, e);
            let (parent, name) = open_parent(&self.root, &entry.path).map_err(io)?;
            let dir = open_dir_at(parent.as_raw_fd(), &name).map_err(io)?;
            self.owner_mode_time(dir.as_raw_fd(), entry).map_err(io)?;
        }
        Ok(())
    }
}

/// Removes everything under `root` that the restored manifest does not name and that is not
/// excluded, so the directory holds exactly the key's tree before the image command starts.
pub fn prune(root: &OwnedFd, manifest: Option<&Manifest>, exclude: &[String]) -> io::Result<()> {
    let keep: std::collections::HashSet<&str> = manifest
        .map(|m| m.entries.iter().map(|e| e.path.as_str()).collect())
        .unwrap_or_default();
    let mut stack: Vec<(String, OwnedFd)> = vec![(String::new(), dup(root)?)];
    while let Some((prefix, dir)) = stack.pop() {
        for name in list_dir(dir.as_raw_fd())? {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if excluded(&path, exclude) {
                continue;
            }
            if !keep.contains(path.as_str()) {
                let c = cstring(&name)?;
                // SAFETY: dir is live and c NUL-terminated.
                if unsafe { libc::unlinkat(dir.as_raw_fd(), c.as_ptr(), 0) } != 0 {
                    remove_tree(dir.as_raw_fd(), &name)?;
                }
                continue;
            }
            if let Ok(child) = open_dir_at(dir.as_raw_fd(), &name) {
                stack.push((path, child));
            }
        }
    }
    Ok(())
}

/// For logs: a path as UTF-8.
pub fn display(path: &OsStr) -> String {
    String::from_utf8_lossy(path.as_bytes()).into_owned()
}
