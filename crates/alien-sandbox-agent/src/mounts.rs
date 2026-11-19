//! Mounts declared for the sandbox: a capped tmpfs and S3 Files file systems.
//!
//! The agent drops `CAP_SYS_ADMIN` before any command can run, so it cannot mount once it is
//! serving. A helper process is forked at boot, before that drop and before any thread exists,
//! and keeps the capability. It takes small typed requests over a socket pair and performs
//! exactly those mounts; it never runs anything from the image. Credentials for a mount arrive
//! over the agent's authenticated control channel, are written to a file under a directory only
//! root can open, and are never placed in the environment, logs or status.
//!
//! The image command starts only once every declared mount is in place.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use alien_core::SandboxMount;
use serde::{Deserialize, Serialize};

/// Environment variable holding the declared mounts as JSON.
pub const ENV_MOUNTS: &str = alien_core::sandbox_egress::ENV_SANDBOX_MOUNTS;

/// Directory holding the credential files. Mode 0700, owned by root.
pub const CREDENTIALS_DIR: &str = "/run/alien/mounts";

/// Credentials for one mount, as delivered by the caller.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MountCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    pub expiration: String,
}

impl std::fmt::Debug for MountCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MountCredentials(..)")
    }
}

/// Where one S3 Files mount comes from.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MountSource {
    pub file_system_id: String,
    pub access_point_id: String,
    pub region: String,
    /// Availability zone id (for example `use2-az1`) of the mount target to use. S3 Files names a
    /// mount target `<zone id>.<file system id>.s3files.<region>.on.aws`; the upstream helper reads
    /// the zone id from the instance metadata, which a sandbox does not have.
    #[serde(default)]
    pub availability_zone_id: Option<String>,
    pub credentials: MountCredentials,
}

/// Body of `PUT /v1/mounts`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MountsRequest {
    pub mounts: BTreeMap<String, MountSource>,
}

/// Body of `PUT /v1/mounts/credentials`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CredentialsRequest {
    pub path: String,
    pub credentials: MountCredentials,
}

/// What the helper is asked to do. Typed: the helper accepts nothing else.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum HelperRequest {
    Tmpfs {
        path: String,
        size_bytes: u64,
        uid: u32,
        gid: u32,
    },
    S3Files {
        path: String,
        file_system_id: String,
        access_point_id: String,
        region: String,
        availability_zone_id: Option<String>,
    },
    /// New credentials were written: make a fresh client certificate and have the proxy read it.
    Refresh {
        path: String,
    },
    Unmount {
        path: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct HelperReply {
    error: Option<String>,
}

/// Parses the declaration the agent was started with. `None` and `[]` mean no mounts.
pub fn parse_declared(value: Option<&str>) -> Result<Vec<SandboxMount>, String> {
    match value {
        None => Ok(Vec::new()),
        Some(text) => {
            serde_json::from_str(text).map_err(|_| format!("{ENV_MOUNTS} is not a list of mounts"))
        }
    }
}

/// Parses a quantity such as `8Gi`, `512Mi` or `1G` into bytes.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let bad = || format!("'{text}' is not a size such as 8Gi");
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);
    let number: u64 = digits.parse().map_err(|_| bad())?;
    let factor: u64 = match unit {
        "" => 1,
        "Ki" => 1 << 10,
        "Mi" => 1 << 20,
        "Gi" => 1 << 30,
        "Ti" => 1 << 40,
        "K" | "k" => 1_000,
        "M" => 1_000_000,
        "G" => 1_000_000_000,
        "T" => 1_000_000_000_000,
        _ => return Err(bad()),
    };
    let bytes = number.checked_mul(factor).ok_or_else(bad)?;
    if bytes == 0 {
        return Err(bad());
    }
    Ok(bytes)
}

/// Mount options for a capped tmpfs the command identity owns.
pub fn tmpfs_options(size_bytes: u64, uid: u32, gid: u32) -> String {
    format!("size={size_bytes},uid={uid},gid={gid},mode=0755")
}

/// File name of a mount's credentials: derived from the path, never the path itself.
pub fn credentials_file(dir: &Path, mount_path: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(mount_path.as_bytes());
    dir.join(format!("{}.json", hex::encode(&digest[..12])))
}

/// Writes the credentials for `mount_path` atomically with mode 0600. A renewal calls this again
/// and nothing else: the mounted file system keeps working and reads the new file.
#[cfg(unix)]
pub fn write_credentials(
    dir: &Path,
    mount_path: &str,
    credentials: &MountCredentials,
) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let target = credentials_file(dir, mount_path);
    let staging = target.with_extension("tmp");
    let _ = std::fs::remove_file(&staging);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)?;
    file.write_all(
        serde_json::to_string(credentials)
            .map_err(std::io::Error::other)?
            .as_bytes(),
    )?;
    file.sync_all()?;
    std::fs::rename(&staging, &target)
}

/// What each mount reports.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MountStatus {
    pub path: String,
    /// `mounted`, `pending` or `failed: <reason>`
    pub state: String,
}

/// Performs mounts for the agent. The production implementation talks to the helper.
pub trait MountBackend: Send + Sync {
    fn request(&self, request: &HelperRequest) -> Result<(), String>;
}

/// Why a delivery was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Unknown(String),
    Missing(String),
    NoMounts,
    RefreshFailed(String),
    NoZone(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(path) => write!(f, "'{path}' is not a declared s3files mount"),
            Self::Missing(path) => write!(f, "no source was given for the mount '{path}'"),
            Self::NoMounts => f.write_str("this sandbox declares no s3files mounts"),
            Self::NoZone(path) => write!(
                f,
                "mount '{path}': {}",
                "availabilityZoneId is missing: pass the zone id of a mount target of this file system that the sandbox's network reaches (for example use2-az1; the S3 Files mount target listing shows it as availabilityZoneId)"
            ),
            Self::RefreshFailed(reason) => write!(f, "the new credentials were stored but not applied: {reason}"),
        }
    }
}

struct Inner {
    status: BTreeMap<String, String>,
    start_deferred: bool,
}

/// The sandbox's declared mounts and what has happened to each.
pub struct Mounts {
    declared: Vec<SandboxMount>,
    uid: u32,
    gid: u32,
    credentials_dir: PathBuf,
    backend: Box<dyn MountBackend>,
    inner: Mutex<Inner>,
}

impl Mounts {
    pub fn new(
        declared: Vec<SandboxMount>,
        uid: u32,
        gid: u32,
        credentials_dir: PathBuf,
        backend: Box<dyn MountBackend>,
    ) -> Self {
        let status = declared
            .iter()
            .map(|m| (m.path().to_string(), "pending".to_string()))
            .collect();
        Self {
            declared,
            uid,
            gid,
            credentials_dir,
            backend,
            inner: Mutex::new(Inner {
                status,
                start_deferred: false,
            }),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.declared.is_empty()
    }

    pub fn status(&self) -> Vec<MountStatus> {
        let inner = self.inner.lock().expect("mount status");
        self.declared
            .iter()
            .map(|m| MountStatus {
                path: m.path().to_string(),
                state: inner.status[m.path()].clone(),
            })
            .collect()
    }

    /// True when every declared mount is mounted.
    pub fn ready(&self) -> bool {
        let inner = self.inner.lock().expect("mount status");
        inner.status.values().all(|s| s == "mounted")
    }

    /// Called where the image command would start. Returns whether it may start now; when it may
    /// not, the start is remembered and [`Mounts::take_deferred_start`] hands it back once the
    /// mounts are in place.
    pub fn may_start(&self) -> bool {
        let mut inner = self.inner.lock().expect("mount status");
        if inner.status.values().all(|s| s == "mounted") {
            return true;
        }
        inner.start_deferred = true;
        false
    }

    /// True once, when a start was deferred and every mount is now in place.
    pub fn take_deferred_start(&self) -> bool {
        let mut inner = self.inner.lock().expect("mount status");
        if inner.start_deferred && inner.status.values().all(|s| s == "mounted") {
            inner.start_deferred = false;
            return true;
        }
        false
    }

    fn s3files_paths(&self) -> impl Iterator<Item = &str> {
        self.declared.iter().filter_map(|m| match m {
            SandboxMount::S3files { path } => Some(path.as_str()),
            _ => None,
        })
    }

    /// Checks a delivery names exactly the declared s3files mounts.
    pub fn check_sources(&self, given: &BTreeMap<String, MountSource>) -> Result<(), Refusal> {
        if self.s3files_paths().next().is_none() {
            return Err(Refusal::NoMounts);
        }
        for path in given.keys() {
            if !self.s3files_paths().any(|p| p == path) {
                return Err(Refusal::Unknown(path.clone()));
            }
        }
        for path in self.s3files_paths() {
            match given.get(path) {
                None => return Err(Refusal::Missing(path.to_string())),
                Some(source) if source.availability_zone_id.is_none() => {
                    return Err(Refusal::NoZone(path.to_string()))
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn set(&self, path: &str, state: String) {
        self.inner
            .lock()
            .expect("mount status")
            .status
            .insert(path.to_string(), state);
    }

    fn is_mounted(&self, path: &str) -> bool {
        self.inner.lock().expect("mount status").status.get(path) == Some(&"mounted".to_string())
    }

    /// Mounts every declared mount that can be mounted now, in declaration order. A tmpfs under
    /// an s3files mount waits for it. Failures are recorded as the mount's status.
    pub fn advance(&self, sources: &BTreeMap<String, MountSource>) {
        for mount in &self.declared {
            let path = mount.path();
            if self.is_mounted(path) {
                continue;
            }
            // Wait for any mount this one sits under.
            let blocked = self.declared.iter().any(|other| {
                other.path() != path
                    && Path::new(path).starts_with(other.path())
                    && !self.is_mounted(other.path())
            });
            if blocked {
                continue;
            }
            let outcome = match mount {
                SandboxMount::Tmpfs { size, .. } => parse_size(size).and_then(|size_bytes| {
                    self.backend.request(&HelperRequest::Tmpfs {
                        path: path.to_string(),
                        size_bytes,
                        uid: self.uid,
                        gid: self.gid,
                    })
                }),
                SandboxMount::S3files { .. } => match sources.get(path) {
                    None => continue,
                    Some(source) => {
                        write_credentials(&self.credentials_dir, path, &source.credentials)
                            .map_err(|_| "cannot store the credentials".to_string())
                            .and_then(|()| {
                                self.backend.request(&HelperRequest::S3Files {
                                    path: path.to_string(),
                                    file_system_id: source.file_system_id.clone(),
                                    access_point_id: source.access_point_id.clone(),
                                    region: source.region.clone(),
                                    availability_zone_id: source.availability_zone_id.clone(),
                                })
                            })
                    }
                },
            };
            match outcome {
                Ok(()) => self.set(path, "mounted".into()),
                Err(reason) => self.set(path, format!("failed: {reason}")),
            }
        }
    }

    /// Replaces a mount's credentials without remounting.
    pub fn renew(&self, path: &str, credentials: &MountCredentials) -> Result<(), Refusal> {
        if !self.s3files_paths().any(|p| p == path) {
            return Err(Refusal::Unknown(path.to_string()));
        }
        write_credentials(&self.credentials_dir, path, credentials)
            .map_err(|_| Refusal::Unknown(path.to_string()))?;
        // Only a mounted file system has a certificate to remake; before that the mount reads
        // the new file itself.
        if self.is_mounted(path) {
            self.backend
                .request(&HelperRequest::Refresh {
                    path: path.to_string(),
                })
                .map_err(Refusal::RefreshFailed)?;
        }
        Ok(())
    }

    /// Unmounts in reverse declaration order, before a suspend. Status goes back to `pending`.
    pub fn unmount_all(&self) {
        for mount in self.declared.iter().rev() {
            let path = mount.path();
            if !self.is_mounted(path) {
                continue;
            }
            match self.backend.request(&HelperRequest::Unmount {
                path: path.to_string(),
            }) {
                Ok(()) => self.set(path, "pending".into()),
                Err(reason) => self.set(path, format!("failed: {reason}")),
            }
        }
    }
}

static GLOBAL: OnceLock<Mounts> = OnceLock::new();

/// Installs the process's mounts. Called once at boot.
pub fn install(mounts: Mounts) {
    let _ = GLOBAL.set(mounts);
}

/// The process's mounts, when any are declared.
pub fn global() -> Option<&'static Mounts> {
    GLOBAL.get().filter(|m| !m.is_empty())
}

/// Talks to the forked helper.
pub struct HelperClient {
    stream: Mutex<(
        std::os::unix::net::UnixStream,
        BufReader<std::os::unix::net::UnixStream>,
    )>,
}

impl MountBackend for HelperClient {
    fn request(&self, request: &HelperRequest) -> Result<(), String> {
        let mut guard = self.stream.lock().map_err(|_| "mount helper failed")?;
        // Every step in the helper has its own deadline, so this one only catches a helper that
        // is stuck.
        let _ = guard.0.set_read_timeout(Some(std::time::Duration::from_secs(40)));
        let mut line = serde_json::to_string(request).map_err(|_| "bad request")?;
        line.push('\n');
        guard
            .0
            .write_all(line.as_bytes())
            .map_err(|_| "the mount helper is gone")?;
        let mut reply = String::new();
        guard.1.read_line(&mut reply).map_err(|e| {
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) {
                "the mount helper did not answer in 40 s"
            } else {
                "the mount helper is gone"
            }
        })?;
        let reply: HelperReply =
            serde_json::from_str(&reply).map_err(|_| "the mount helper is gone")?;
        match reply.error {
            None => Ok(()),
            Some(reason) => Err(reason),
        }
    }
}

/// Forks the helper. Call before the agent drops capabilities and before it starts threads.
/// In the parent returns the client; the child never returns.
#[cfg(target_os = "linux")]
pub fn fork_helper(prepare_key: bool) -> std::io::Result<HelperClient> {
    use std::os::unix::net::UnixStream;
    let (parent, child) = UnixStream::pair()?;
    // SAFETY: the agent has no other threads yet; the child only uses its own memory and exits.
    match unsafe { libc::fork() } {
        -1 => Err(std::io::Error::last_os_error()),
        0 => {
            drop(parent);
            // SAFETY: plain prctl calls; the helper dies with the agent, and no one without
            // CAP_SYS_PTRACE can read its memory or descriptors through /proc.
            unsafe {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
            }
            helper_loop(child, prepare_key);
            std::process::exit(0);
        }
        _ => {
            drop(child);
            let reader = BufReader::new(parent.try_clone()?);
            Ok(HelperClient {
                stream: Mutex::new((parent, reader)),
            })
        }
    }
}

#[cfg(target_os = "linux")]
fn helper_loop(stream: std::os::unix::net::UnixStream, prepare_key: bool) {
    let mut writer = stream.try_clone().expect("socket clone");
    let reader = BufReader::new(stream);
    let mut helper = crate::s3files_mount::Helper::new(CREDENTIALS_DIR.into());
    // The client key is made once, while the agent is still starting, and reused by every mount
    // and renewal (upstream shares one key between mounts too). It stays in this process.
    if prepare_key {
        helper.prepare_key();
    }
    let mut in_agent_namespace = false;
    for line in reader.lines() {
        let Ok(line) = line else { return };
        let error = match serde_json::from_str::<HelperRequest>(&line) {
            // The agent moves into a private mount namespace after this process was forked, and
            // what the guest sees is what that namespace holds. Mount there, not here.
            Ok(request) if !in_agent_namespace => match join_agent_mount_namespace() {
                Ok(()) => {
                    in_agent_namespace = true;
                    perform(&mut helper, &request).err()
                }
                Err(reason) => Some(reason),
            },
            Ok(request) => perform(&mut helper, &request).err(),
            Err(_) => Some("unreadable request".to_string()),
        };
        let reply = serde_json::to_string(&HelperReply { error }).expect("reply");
        if writeln!(writer, "{reply}").is_err() {
            return;
        }
    }
}

/// `setns` into the mount namespace of the agent, this process's parent. Needs CAP_SYS_ADMIN and a
/// single thread, both true of the helper.
#[cfg(target_os = "linux")]
fn join_agent_mount_namespace() -> Result<(), String> {
    // SAFETY: getppid has no arguments and cannot fail.
    let parent = unsafe { libc::getppid() };
    let path = c_string(&format!("/proc/{parent}/ns/mnt"))?;
    // SAFETY: valid NUL-terminated path; the descriptor is closed below.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(format!(
            "cannot open the agent's mount namespace: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: fd is a namespace descriptor just opened.
    let rc = unsafe { libc::setns(fd, libc::CLONE_NEWNS) };
    let error = std::io::Error::last_os_error();
    // SAFETY: fd is open and owned here.
    unsafe { libc::close(fd) };
    if rc != 0 {
        return Err(format!("cannot join the agent's mount namespace: {error}"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn perform(
    helper: &mut crate::s3files_mount::Helper,
    request: &HelperRequest,
) -> Result<(), String> {
    match request {
        HelperRequest::Tmpfs {
            path,
            size_bytes,
            uid,
            gid,
        } => mount_tmpfs(path, *size_bytes, *uid, *gid),
        HelperRequest::S3Files {
            path,
            file_system_id,
            access_point_id,
            region,
            availability_zone_id,
        } => helper.mount(
            path,
            file_system_id,
            access_point_id,
            region,
            availability_zone_id.as_deref(),
        ),
        HelperRequest::Refresh { path } => helper.refresh(path),
        HelperRequest::Unmount { path } => {
            helper.stop_proxy(path);
            unmount(path)
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn c_string(text: &str) -> Result<std::ffi::CString, String> {
    std::ffi::CString::new(text).map_err(|_| "invalid path".to_string())
}

#[cfg(target_os = "linux")]
fn mount_tmpfs(path: &str, size_bytes: u64, uid: u32, gid: u32) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("cannot create {path}: {e}"))?;
    let target = c_string(path)?;
    let source = c_string("tmpfs")?;
    let fstype = c_string("tmpfs")?;
    let options = c_string(&tmpfs_options(size_bytes, uid, gid))?;
    // SAFETY: all pointers are valid NUL-terminated strings that outlive the call.
    let rc = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fstype.as_ptr(),
            libc::MS_NOSUID | libc::MS_NODEV,
            options.as_ptr().cast(),
        )
    };
    if rc != 0 {
        return Err(format!("mount failed: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn unmount(path: &str) -> Result<(), String> {
    let target = c_string(path)?;
    // SAFETY: valid NUL-terminated path.
    if unsafe { libc::umount2(target.as_ptr(), 0) } != 0 {
        return Err(format!(
            "unmount failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<HelperRequest>>,
        fail_s3: bool,
    }

    impl MountBackend for Arc<Fake> {
        fn request(&self, request: &HelperRequest) -> Result<(), String> {
            let copy: HelperRequest =
                serde_json::from_str(&serde_json::to_string(request).unwrap()).unwrap();
            let fail = self.fail_s3 && matches!(request, HelperRequest::S3Files { .. });
            self.calls.lock().unwrap().push(copy);
            if fail {
                Err("no route to the mount target".into())
            } else {
                Ok(())
            }
        }
    }

    fn credentials(tag: &str) -> MountCredentials {
        MountCredentials {
            access_key_id: format!("AK{tag}"),
            secret_access_key: format!("SK{tag}"),
            session_token: format!("ST{tag}"),
            expiration: "2030-01-01T00:00:00Z".into(),
        }
    }

    fn source(tag: &str) -> MountSource {
        MountSource {
            file_system_id: "fs-1".into(),
            access_point_id: "fsap-1".into(),
            region: "us-east-1".into(),
            availability_zone_id: Some("use1-az1".into()),
            credentials: credentials(tag),
        }
    }

    fn workspace_and_scratch() -> Vec<SandboxMount> {
        parse_declared(Some(
            r#"[{"type":"s3files","path":"/workspace"},{"type":"tmpfs","path":"/workspace/tmp","size":"8Gi"}]"#,
        ))
        .unwrap()
    }

    fn mounts(fake: &Arc<Fake>, dir: &Path) -> Mounts {
        Mounts::new(
            workspace_and_scratch(),
            1000,
            1000,
            dir.to_path_buf(),
            Box::new(Arc::clone(fake)),
        )
    }

    #[test]
    fn sizes_parse_as_quantities() {
        assert_eq!(parse_size("8Gi").unwrap(), 8 << 30);
        assert_eq!(parse_size("512Mi").unwrap(), 512 << 20);
        assert_eq!(parse_size("1G").unwrap(), 1_000_000_000);
        for bad in ["", "Gi", "0", "8Xi", "-1", "99999999999999999999Ti"] {
            parse_size(bad).expect_err(bad);
        }
    }

    #[test]
    fn tmpfs_options_cap_the_size_and_name_the_owner() {
        assert_eq!(
            tmpfs_options(8 << 30, 1000, 2000),
            "size=8589934592,uid=1000,gid=2000,mode=0755"
        );
    }

    #[test]
    fn the_image_waits_for_every_mount_then_starts_once() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let mounts = mounts(&fake, dir.path());
        assert!(!mounts.may_start(), "nothing is mounted yet");
        assert!(!mounts.take_deferred_start());
        let sources = BTreeMap::from([("/workspace".to_string(), source("1"))]);
        mounts.check_sources(&sources).unwrap();
        mounts.advance(&sources);
        assert!(mounts.ready());
        assert!(mounts.take_deferred_start());
        assert!(!mounts.take_deferred_start(), "only once");
        // The tmpfs under /workspace is mounted after the file system it sits on.
        let calls = fake.calls.lock().unwrap();
        assert!(matches!(calls[0], HelperRequest::S3Files { .. }));
        assert!(
            matches!(calls[1], HelperRequest::Tmpfs { size_bytes, .. } if size_bytes == 8 << 30)
        );
    }

    #[test]
    fn a_tmpfs_under_a_failed_file_system_stays_pending() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake {
            fail_s3: true,
            ..Default::default()
        });
        let mounts = mounts(&fake, dir.path());
        mounts.advance(&BTreeMap::from([("/workspace".to_string(), source("1"))]));
        let status = mounts.status();
        assert_eq!(status[0].state, "failed: no route to the mount target");
        assert_eq!(status[1].state, "pending");
        assert!(!mounts.may_start());
        assert!(!mounts.take_deferred_start());
    }

    #[test]
    fn a_delivery_must_name_exactly_the_declared_s3files_mounts() {
        let dir = tempfile::tempdir().unwrap();
        let mounts = mounts(&Arc::new(Fake::default()), dir.path());
        assert_eq!(
            mounts.check_sources(&BTreeMap::new()),
            Err(Refusal::Missing("/workspace".into()))
        );
        let extra = BTreeMap::from([
            ("/workspace".to_string(), source("1")),
            ("/other".to_string(), source("2")),
        ]);
        assert_eq!(
            mounts.check_sources(&extra),
            Err(Refusal::Unknown("/other".into()))
        );
        let mut no_zone = source("1");
        no_zone.availability_zone_id = None;
        let message = mounts
            .check_sources(&BTreeMap::from([("/workspace".to_string(), no_zone)]))
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("availabilityZoneId") && message.contains("use2-az1"),
            "{message}"
        );
        // A tmpfs path takes no source.
        let tmpfs = BTreeMap::from([("/workspace/tmp".to_string(), source("1"))]);
        assert!(matches!(
            mounts.check_sources(&tmpfs),
            Err(Refusal::Unknown(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn renewal_rewrites_only_the_credentials_file_with_owner_only_access() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let mounts = mounts(&fake, dir.path());
        let sources = BTreeMap::from([("/workspace".to_string(), source("1"))]);
        mounts.advance(&sources);
        let calls_before = fake.calls.lock().unwrap().len();
        mounts.renew("/workspace", &credentials("2")).unwrap();
        // The only thing asked of the helper is a fresh certificate; nothing is mounted again.
        assert_eq!(
            fake.calls.lock().unwrap()[calls_before..],
            [HelperRequest::Refresh {
                path: "/workspace".into()
            }]
        );

        let file = credentials_file(dir.path(), "/workspace");
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("SK2") && !written.contains("SK1"));
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(mounts.renew("/nowhere", &credentials("3")).is_err());
    }

    #[test]
    fn nothing_that_reports_status_carries_the_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let mounts = mounts(&fake, dir.path());
        mounts.advance(&BTreeMap::from([(
            "/workspace".to_string(),
            source("secret"),
        )]));
        let shown = format!(
            "{:?} {}",
            source("secret"),
            serde_json::to_string(&mounts.status()).unwrap()
        );
        assert!(!shown.contains("secret"), "{shown}");
    }

    #[test]
    fn unmount_runs_innermost_first_and_returns_to_pending() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let mounts = mounts(&fake, dir.path());
        mounts.advance(&BTreeMap::from([("/workspace".to_string(), source("1"))]));
        fake.calls.lock().unwrap().clear();
        mounts.unmount_all();
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            *calls,
            vec![
                HelperRequest::Unmount {
                    path: "/workspace/tmp".into()
                },
                HelperRequest::Unmount {
                    path: "/workspace".into()
                },
            ]
        );
        assert!(!mounts.ready());
    }

    /// Runs the real helper: forked before the agent unshares its mount namespace, it must mount
    /// inside the namespace of its parent, where commands will look. The scenario runs in a forked
    /// child because a mount namespace belongs to one thread and the test harness has many.
    /// Run with `unshare --user --map-root-user --mount -- <test binary> --ignored`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs mount privileges, e.g. unshare --user --map-root-user --mount"]
    fn the_helper_mounts_a_capped_tmpfs_where_the_parent_sees_it() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("scratch");
        // SAFETY: the child only runs the scenario below and exits without returning.
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            let outcome = std::panic::catch_unwind(|| capped_tmpfs_scenario(&target));
            if let Err(panic) = &outcome {
                let text = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("panic");
                eprintln!("SCENARIO FAILED: {text}");
            }
            // SAFETY: leave the forked copy of the test harness without running its destructors.
            unsafe { libc::_exit(if outcome.is_ok() { 0 } else { 1 }) };
        }
        let mut status = 0;
        // SAFETY: waiting for the child just forked.
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
        assert!(
            libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
            "scenario failed"
        );
    }

    #[cfg(target_os = "linux")]
    fn capped_tmpfs_scenario(target: &Path) {
        use std::os::unix::fs::MetadataExt;
        let helper = fork_helper(false).expect("forks the helper");
        // Only after the fork, as the agent does: a private mount namespace of its own.
        assert_eq!(unsafe { libc::unshare(libc::CLONE_NEWNS) }, 0);
        assert_eq!(
            unsafe {
                libc::mount(
                    std::ptr::null(),
                    c"/".as_ptr(),
                    std::ptr::null(),
                    libc::MS_REC | libc::MS_PRIVATE,
                    std::ptr::null(),
                )
            },
            0
        );
        helper
            .request(&HelperRequest::Tmpfs {
                path: target.to_str().unwrap().into(),
                size_bytes: 4 << 20,
                // Only root is mapped in the test's user namespace.
                uid: 0,
                gid: 0,
            })
            .expect("mounts");
        let stat = std::fs::metadata(target).unwrap();
        assert_eq!((stat.uid(), stat.gid()), (0, 0));
        let table = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
        let line = table
            .lines()
            .find(|l| l.contains(target.to_str().unwrap()))
            .expect("the mount is in this process's namespace");
        assert!(line.contains("nosuid") && line.contains("nodev"), "{line}");
        assert!(line.contains("size=4096k"), "{line}");
        // The cap is enforced: a write past it fails with "no space left".
        let error = std::fs::write(target.join("fill"), vec![0u8; 8 << 20])
            .expect_err("a write past the cap must fail");
        assert_eq!(error.raw_os_error(), Some(libc::ENOSPC), "{error}");
        helper
            .request(&HelperRequest::Unmount {
                path: target.to_str().unwrap().into(),
            })
            .expect("unmounts");
        assert!(
            !target.join("fill").exists(),
            "the unmount removed the mount"
        );
    }

    #[test]
    fn the_declaration_variable_must_be_a_list_of_mounts() {
        assert!(parse_declared(None).unwrap().is_empty());
        parse_declared(Some("{}")).expect_err("not a list");
        parse_declared(Some(r#"[{"type":"nfs","path":"/x"}]"#)).expect_err("unknown type");
    }
}
