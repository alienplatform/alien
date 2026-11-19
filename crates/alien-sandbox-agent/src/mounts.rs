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
    /// IP address of a mount target. When given, the agent makes no DNS lookup and connects to
    /// it; the server certificate is still checked against the file system's name.
    #[serde(default)]
    pub mount_target_ip: Option<String>,
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
        #[serde(default)]
        mount_target_ip: Option<String>,
    },
    /// New credentials were written: make a fresh client certificate and have the proxy read it.
    Refresh {
        path: String,
    },
    Unmount {
        path: String,
    },
}

impl HelperRequest {
    /// Names the step and its path for the log; never a credential or an identifier of a file system.
    fn describe(&self) -> String {
        let (op, path) = match self {
            Self::Tmpfs { path, .. } => ("tmpfs", path),
            Self::S3Files { path, .. } => ("s3files", path),
            Self::Refresh { path } => ("refresh", path),
            Self::Unmount { path } => ("unmount", path),
        };
        format!("{op} {path}")
    }
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
                "neither availabilityZoneId nor mountTargetIp is given: pass the zone id of a mount target of this file system (for example use2-az1), or the IP address of one the sandbox's network reaches"
            ),
            Self::RefreshFailed(reason) => write!(f, "the new credentials were stored but not applied: {reason}"),
        }
    }
}

struct Inner {
    status: BTreeMap<String, String>,
    start_deferred: bool,
    /// The sources last delivered, kept so a resume can mount again without the binding.
    sources: BTreeMap<String, MountSource>,
    /// True between a suspend and the resume that follows; nothing is mounted then.
    suspended: bool,
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
                sources: BTreeMap::new(),
                suspended: false,
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
                Some(source)
                    if source.availability_zone_id.is_none() && source.mount_target_ip.is_none() =>
                {
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
        let now = self.status();
        crate::report::update(|s| {
            s.mounts = now
                .into_iter()
                .map(|m| crate::report::MountState {
                    path: m.path,
                    state: m.state,
                })
                .collect();
        });
    }

    fn is_mounted(&self, path: &str) -> bool {
        self.inner.lock().expect("mount status").status.get(path) == Some(&"mounted".to_string())
    }

    /// Mounts every declared mount that can be mounted now, in declaration order. A tmpfs under
    /// an s3files mount waits for it. Failures are recorded as the mount's status.
    pub fn advance(&self, sources: &BTreeMap<String, MountSource>) {
        if !sources.is_empty() {
            self.inner
                .lock()
                .expect("mount status")
                .sources
                .extend(sources.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        for mount in &self.declared {
            let path = mount.path();
            if self.is_mounted(path) {
                continue;
            }
            // Wait for any mount this one sits under.
            let parent = self.declared.iter().find(|other| {
                other.path() != path
                    && Path::new(path).starts_with(other.path())
                    && !self.is_mounted(other.path())
            });
            if let Some(parent) = parent {
                // Say why this one is not mounted when the mount it sits on failed.
                let parent_state = self
                    .inner
                    .lock()
                    .expect("mount status")
                    .status
                    .get(parent.path())
                    .cloned()
                    .unwrap_or_default();
                if parent_state.starts_with("failed") {
                    self.set(
                        path,
                        format!("failed: the mount it sits on ({}) failed", parent.path()),
                    );
                }
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
                                    mount_target_ip: source.mount_target_ip.clone(),
                                })
                            })
                    }
                },
            };
            match outcome {
                Ok(()) => self.set(path, "mounted".into()),
                Err(reason) => {
                    crate::report::log(&format!("alien-sandbox-agent: mounts: {path} failed: {reason}"));
                    self.set(path, format!("failed: {reason}"))
                }
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
        // A resume mounts from the remembered source; it must carry the newest credentials.
        if let Some(source) = self
            .inner
            .lock()
            .expect("mount status")
            .sources
            .get_mut(path)
        {
            source.credentials = credentials.clone();
        }
        // Only a mounted file system has a certificate to remake; before that the mount reads
        // the new file itself.
        if self.is_mounted(path) {
            self.backend
                .request(&HelperRequest::Refresh {
                    path: path.to_string(),
                })
                .map_err(Refusal::RefreshFailed)?;
            return Ok(());
        }
        // A mount that failed (for instance on credentials that had expired) or is still pending
        // is tried again with the new credentials, unless a suspend is holding everything down.
        let (retry, source) = {
            let inner = self.inner.lock().expect("mount status");
            let state = inner
                .status
                .get(path)
                .map(String::as_str)
                .unwrap_or("pending");
            (
                !inner.suspended && (state == "pending" || state.starts_with("failed")),
                inner.sources.get(path).cloned(),
            )
        };
        if let (true, Some(source)) = (retry, source) {
            self.advance(&BTreeMap::from([(path.to_string(), source)]));
        }
        Ok(())
    }

    /// True when the mount is a file system from outside, or sits under one. A plain tmpfs keeps
    /// its memory across a suspend; these do not survive it: the connection to the file system
    /// is gone when the VM is restored.
    fn is_remote_or_under_remote(&self, mount: &SandboxMount) -> bool {
        matches!(mount, SandboxMount::S3files { .. })
            || self.declared.iter().any(|other| {
                matches!(other, SandboxMount::S3files { .. })
                    && other.path() != mount.path()
                    && Path::new(mount.path()).starts_with(other.path())
            })
    }

    /// Before a suspend: unmounts the s3files mounts (and anything mounted under them), innermost
    /// first, and stops their proxies. Status goes back to `pending`. A tmpfs outside them stays
    /// as it is. Credentials files stay, so renewals arriving while suspended still apply.
    pub fn suspend(&self) {
        self.inner.lock().expect("mount status").suspended = true;
        for mount in self.declared.iter().rev() {
            let path = mount.path();
            if !self.is_remote_or_under_remote(mount) || !self.is_mounted(path) {
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

    /// After a resume: mounts again whatever [`Mounts::suspend`] took away (or that failed), from
    /// the sources delivered before. Nothing is asked of the binding.
    pub fn resume(&self) {
        self.inner.lock().expect("mount status").suspended = false;
        let sources = self.inner.lock().expect("mount status").sources.clone();
        // A mount that failed on the way down is still there; forget it so it is tried afresh.
        for mount in &self.declared {
            if self.is_remote_or_under_remote(mount) && !self.is_mounted(mount.path()) {
                self.set(mount.path(), "pending".into());
            }
        }
        self.advance(&sources);
    }

    /// Unmounts every mount in reverse declaration order. Status goes back to `pending`.
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
    /// Carries the agent's mount namespace to the helper, once. The helper cannot open it through
    /// `/proc`: the agent is not dumpable, so only a process holding `CAP_SYS_PTRACE` may.
    #[cfg(target_os = "linux")]
    namespace: Mutex<Option<std::os::unix::net::UnixStream>>,
}

impl MountBackend for HelperClient {
    fn request(&self, request: &HelperRequest) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        self.hand_over_namespace()?;
        let mut guard = self.stream.lock().map_err(|_| "mount helper failed")?;
        // Every step in the helper has its own deadline, so this one only catches a helper that
        // is stuck.
        let _ = guard
            .0
            .set_read_timeout(Some(std::time::Duration::from_secs(40)));
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
        let (step, result) = (
            request.describe(),
            reply.error.clone().map_or("ok".to_string(), |r| format!("failed: {r}")),
        );
        crate::report::log(&format!("alien-sandbox-agent: mounts: {step}: {result}"));
        crate::report::update(|s| s.steps.push(crate::report::Step { step, result }));
        match reply.error {
            None => Ok(()),
            Some(reason) => Err(reason),
        }
    }
}

#[cfg(target_os = "linux")]
impl HelperClient {
    /// Sends the helper a descriptor of this process's mount namespace, once, before its first
    /// request. Called from the first request, which comes after the agent made its private
    /// namespace, so the helper mounts where commands will look.
    fn hand_over_namespace(&self) -> Result<(), String> {
        let mut slot = self.namespace.lock().map_err(|_| "mount helper failed")?;
        let Some(channel) = slot.as_ref() else {
            return Ok(());
        };
        let namespace = std::fs::File::open("/proc/self/ns/mnt")
            .map_err(|e| format!("cannot open the agent's mount namespace: {e}"))?;
        send_descriptor(channel, std::os::fd::AsRawFd::as_raw_fd(&namespace))
            .map_err(|e| format!("cannot hand the mount namespace to the helper: {e}"))?;
        crate::report::log("alien-sandbox-agent: mounts: handed the mount namespace to the helper");
        *slot = None;
        Ok(())
    }
}

/// Sends one descriptor over a Unix socket (`SCM_RIGHTS`), with a single byte of payload.
#[cfg(target_os = "linux")]
fn send_descriptor(socket: &std::os::unix::net::UnixStream, fd: i32) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut byte = 0u8;
    let mut iov = libc::iovec {
        iov_base: (&mut byte as *mut u8).cast(),
        iov_len: 1,
    };
    // SAFETY: CMSG_SPACE only computes a size.
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32) } as usize;
    let mut control = vec![0u8; space];
    // SAFETY: zeroed msghdr is valid; the pointers set below outlive the call.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = space as _;
    // SAFETY: the control buffer holds room for one header and one descriptor.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as u32) as _;
        std::ptr::copy_nonoverlapping(
            (&fd as *const i32).cast::<u8>(),
            libc::CMSG_DATA(header),
            std::mem::size_of::<i32>(),
        );
    }
    // SAFETY: a fully initialised message on an open socket.
    if unsafe { libc::sendmsg(socket.as_raw_fd(), &message, 0) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Receives one descriptor sent by [`send_descriptor`].
#[cfg(target_os = "linux")]
fn receive_descriptor(
    socket: &std::os::unix::net::UnixStream,
) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let mut byte = 0u8;
    let mut iov = libc::iovec {
        iov_base: (&mut byte as *mut u8).cast(),
        iov_len: 1,
    };
    // SAFETY: CMSG_SPACE only computes a size.
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32) } as usize;
    let mut control = vec![0u8; space];
    // SAFETY: zeroed msghdr is valid; the pointers set below outlive the call.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = space as _;
    // SAFETY: a valid message with room for one descriptor.
    if unsafe { libc::recvmsg(socket.as_raw_fd(), &mut message, libc::MSG_CMSG_CLOEXEC) } <= 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the kernel filled the control buffer; a descriptor it installed is ours to own.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null()
            || (*header).cmsg_level != libc::SOL_SOCKET
            || (*header).cmsg_type != libc::SCM_RIGHTS
        {
            return Err(std::io::Error::other("no descriptor came with the message"));
        }
        let mut fd = 0i32;
        std::ptr::copy_nonoverlapping(
            libc::CMSG_DATA(header),
            (&mut fd as *mut i32).cast::<u8>(),
            std::mem::size_of::<i32>(),
        );
        Ok(std::os::fd::OwnedFd::from_raw_fd(fd))
    }
}

/// Forks the helper. Call before the agent drops capabilities and before it starts threads.
/// In the parent returns the client; the child never returns.
#[cfg(target_os = "linux")]
pub fn fork_helper(prepare_key: bool) -> std::io::Result<HelperClient> {
    use std::os::unix::net::UnixStream;
    let (parent, child) = UnixStream::pair()?;
    let (namespace_parent, namespace_child) = UnixStream::pair()?;
    // SAFETY: the agent has no other threads yet; the child only uses its own memory and exits.
    match unsafe { libc::fork() } {
        -1 => Err(std::io::Error::last_os_error()),
        0 => {
            drop(parent);
            drop(namespace_parent);
            // SAFETY: plain prctl calls; the helper dies with the agent, and no one without
            // CAP_SYS_PTRACE can read its memory or descriptors through /proc.
            unsafe {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
            }
            helper_loop(child, namespace_child, prepare_key);
            std::process::exit(0);
        }
        pid => {
            drop(child);
            drop(namespace_child);
            let capabilities = std::fs::read_to_string(format!("/proc/{pid}/status"))
                .ok()
                .and_then(|status| {
                    status
                        .lines()
                        .find(|l| l.starts_with("CapEff:"))
                        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
                });
            // The helper must sit outside the guest cgroup, or its sockets carry the guest mark.
            let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
                .map(|text| text.trim().to_string())
                .unwrap_or_else(|_| "cgroup unknown".to_string());
            crate::report::log(&format!(
                "alien-sandbox-agent: mounts: helper forked (pid {pid}, {}, cgroup {cgroup})",
                capabilities.as_deref().unwrap_or("capabilities unknown")
            ));
            crate::report::update(|s| {
                s.helper_started = true;
                s.helper_cap_eff = capabilities;
            });
            let reader = BufReader::new(parent.try_clone()?);
            Ok(HelperClient {
                stream: Mutex::new((parent, reader)),
                namespace: Mutex::new(Some(namespace_parent)),
            })
        }
    }
}

#[cfg(target_os = "linux")]
fn helper_loop(
    stream: std::os::unix::net::UnixStream,
    namespace: std::os::unix::net::UnixStream,
    prepare_key: bool,
) {
    let mut writer = stream.try_clone().expect("socket clone");
    let reader = BufReader::new(stream);
    let mut helper = crate::s3files_mount::Helper::new(CREDENTIALS_DIR.into());
    // The client key is made once, while the agent is still starting, and reused by every mount
    // and renewal (upstream shares one key between mounts too). It stays in this process.
    if prepare_key {
        helper.prepare_key();
    }
    // Joined once, on the first request; a failure is the answer to every request after it.
    let mut joined: Option<Result<(), String>> = None;
    eprintln!(
        "alien-sandbox-agent: mounts: helper started ({})",
        capabilities_line()
    );
    for line in reader.lines() {
        let Ok(line) = line else { return };
        let error = match serde_json::from_str::<HelperRequest>(&line) {
            Ok(request) => {
                // The agent moves into a private mount namespace after this process was forked,
                // and what commands see is what that namespace holds. Mount there, not here.
                let ready = joined
                    .get_or_insert_with(|| join_agent_mount_namespace(&namespace))
                    .clone();
                let what = request.describe();
                let outcome = ready.and_then(|()| perform(&mut helper, &request));
                match &outcome {
                    Ok(()) => eprintln!("alien-sandbox-agent: mounts: {what}: ok"),
                    Err(reason) => {
                        eprintln!("alien-sandbox-agent: mounts: {what}: failed: {reason}")
                    }
                }
                outcome.err()
            }
            Err(_) => Some("unreadable request".to_string()),
        };
        let reply = serde_json::to_string(&HelperReply { error }).expect("reply");
        if writeln!(writer, "{reply}").is_err() {
            return;
        }
    }
}

/// `setns` into the mount namespace the agent sent as a descriptor. Needs CAP_SYS_ADMIN and a
/// single thread, both true of the helper. The helper does not open `/proc/<agent>/ns/mnt` itself:
/// the agent is not dumpable, so that open needs `CAP_SYS_PTRACE`, which a MicroVM does not give.
#[cfg(target_os = "linux")]
fn join_agent_mount_namespace(channel: &std::os::unix::net::UnixStream) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    let fd = receive_descriptor(channel)
        .map_err(|e| format!("did not receive the agent's mount namespace: {e}"))?;
    // SAFETY: fd is a namespace descriptor received from the agent.
    let rc = unsafe { libc::setns(fd.as_raw_fd(), libc::CLONE_NEWNS) };
    if rc != 0 {
        return Err(format!(
            "cannot join the agent's mount namespace: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// The helper's effective capabilities, for the log line that explains a failed step.
#[cfg(target_os = "linux")]
fn capabilities_line() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|l| l.starts_with("CapEff:"))
                .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        })
        .unwrap_or_else(|| "capabilities unknown".to_string())
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
            mount_target_ip,
        } => helper.mount(
            path,
            file_system_id,
            access_point_id,
            region,
            availability_zone_id.as_deref(),
            mount_target_ip.as_deref(),
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
        let error = std::io::Error::last_os_error();
        // A process still holding a file open must not keep the VM from suspending: detach, and
        // the mount goes once the last file closes.
        // SAFETY: as above.
        if error.raw_os_error() == Some(libc::EBUSY)
            && unsafe { libc::umount2(target.as_ptr(), libc::MNT_DETACH) } == 0
        {
            return Ok(());
        }
        return Err(format!("unmount failed: {error}"));
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
            mount_target_ip: None,
            credentials: credentials(tag),
        }
    }

    fn workspace_and_scratch() -> Vec<SandboxMount> {
        parse_declared(Some(
            r#"[{"type":"s3files","path":"/workspace"},{"type":"tmpfs","path":"/workspace/tmp","size":"8Gi"}]"#,
        ))
        .unwrap()
    }

    fn mounts_with(fake: &Arc<Fake>, dir: &Path) -> Mounts {
        mounts(fake, dir)
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
    fn a_tmpfs_under_a_failed_file_system_reports_the_parent() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake {
            fail_s3: true,
            ..Default::default()
        });
        let mounts = mounts(&fake, dir.path());
        mounts.advance(&BTreeMap::from([("/workspace".to_string(), source("1"))]));
        let status = mounts.status();
        assert_eq!(status[0].state, "failed: no route to the mount target");
        assert_eq!(
            status[1].state,
            "failed: the mount it sits on (/workspace) failed"
        );
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
        no_zone.mount_target_ip = None;
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

    fn with_standalone_tmpfs(fake: &Arc<Fake>, dir: &Path) -> Mounts {
        let mut declared = workspace_and_scratch();
        declared.push(SandboxMount::Tmpfs {
            path: "/cache".into(),
            size: "1Gi".into(),
        });
        Mounts::new(
            declared,
            1000,
            1000,
            dir.to_path_buf(),
            Box::new(Arc::clone(fake)),
        )
    }

    #[test]
    fn a_suspend_unmounts_the_file_system_and_what_sits_on_it_but_not_a_plain_tmpfs() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let mounts = with_standalone_tmpfs(&fake, dir.path());
        mounts.advance(&BTreeMap::from([("/workspace".to_string(), source("1"))]));
        assert!(mounts.ready());
        fake.calls.lock().unwrap().clear();
        mounts.suspend();
        assert_eq!(
            *fake.calls.lock().unwrap(),
            vec![
                HelperRequest::Unmount {
                    path: "/workspace/tmp".into()
                },
                HelperRequest::Unmount {
                    path: "/workspace".into()
                },
            ]
        );
        let states: Vec<_> = mounts
            .status()
            .into_iter()
            .map(|m| (m.path, m.state))
            .collect();
        assert!(
            states.contains(&("/cache".into(), "mounted".into())),
            "{states:?}"
        );
        assert!(
            states.contains(&("/workspace".into(), "pending".into())),
            "{states:?}"
        );
    }

    #[test]
    fn a_resume_mounts_again_from_the_remembered_sources_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let mounts = with_standalone_tmpfs(&fake, dir.path());
        mounts.advance(&BTreeMap::from([("/workspace".to_string(), source("1"))]));
        mounts.suspend();
        fake.calls.lock().unwrap().clear();
        mounts.resume();
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert!(matches!(&calls[0], HelperRequest::S3Files { path, .. } if path == "/workspace"));
        assert!(matches!(&calls[1], HelperRequest::Tmpfs { path, .. } if path == "/workspace/tmp"));
        assert!(mounts.ready());
    }

    #[test]
    fn credentials_renewed_while_suspended_are_the_ones_a_resume_mounts_with() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let mounts = mounts(&fake, dir.path());
        mounts.advance(&BTreeMap::from([("/workspace".to_string(), source("1"))]));
        mounts.suspend();
        fake.calls.lock().unwrap().clear();
        mounts.renew("/workspace", &credentials("2")).unwrap();
        // Suspended: only the file changes; there is no certificate to remake.
        assert!(fake.calls.lock().unwrap().is_empty());
        mounts.resume();
        let written = std::fs::read_to_string(credentials_file(dir.path(), "/workspace")).unwrap();
        assert!(
            written.contains("SK2") && !written.contains("SK1"),
            "stale credentials written"
        );
        assert!(mounts.ready());
    }

    #[test]
    fn a_renewal_retries_a_mount_that_failed_or_is_pending() {
        let dir = tempfile::tempdir().unwrap();
        // A failed state, with the source that was delivered.
        let retry = Arc::new(Fake::default());
        let m = mounts_with(&retry, dir.path());
        m.set("/workspace", "failed: no route to the mount target".into());
        m.inner
            .lock()
            .unwrap()
            .sources
            .insert("/workspace".into(), source("1"));
        m.renew("/workspace", &credentials("2")).unwrap();
        assert_eq!(m.status()[0].state, "mounted");
        let calls = retry.calls.lock().unwrap();
        assert!(calls
            .iter()
            .any(|c| matches!(c, HelperRequest::S3Files { .. })));
        drop(calls);
        let written = std::fs::read_to_string(credentials_file(dir.path(), "/workspace")).unwrap();
        assert!(written.contains("SK2"));
        // Pending with a delivered source (e.g. delivery interrupted) is retried too.
        let pend = Arc::new(Fake::default());
        let p = mounts_with(&pend, dir.path());
        p.inner
            .lock()
            .unwrap()
            .sources
            .insert("/workspace".into(), source("1"));
        p.renew("/workspace", &credentials("3")).unwrap();
        assert_eq!(p.status()[0].state, "mounted");
        // Never delivered: nothing to mount from, so a renewal only writes the file.
        let none = Arc::new(Fake::default());
        let n = mounts_with(&none, dir.path());
        n.renew("/workspace", &credentials("4")).unwrap();
        assert!(none.calls.lock().unwrap().is_empty());
        // Suspended: held down until the resume.
        let sus = Arc::new(Fake::default());
        let s = mounts_with(&sus, dir.path());
        s.advance(&BTreeMap::from([("/workspace".to_string(), source("1"))]));
        s.suspend();
        sus.calls.lock().unwrap().clear();
        s.renew("/workspace", &credentials("5")).unwrap();
        assert!(sus.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_remount_shows_in_the_status() {
        let dir = tempfile::tempdir().unwrap();
        let ok = Arc::new(Fake::default());
        let mounts_ok = mounts(&ok, dir.path());
        mounts_ok.advance(&BTreeMap::from([("/workspace".to_string(), source("1"))]));
        // A backend that fails s3files from the start: resume leaves a failed status, not a panic.
        let bad = Arc::new(Fake {
            fail_s3: true,
            ..Default::default()
        });
        let mounts_bad = mounts(&bad, dir.path());
        mounts_bad.advance(&BTreeMap::from([("/workspace".to_string(), source("1"))]));
        mounts_bad.resume();
        assert_eq!(
            mounts_bad.status()[0].state,
            "failed: no route to the mount target"
        );
        assert!(!mounts_bad.ready());
    }

    #[test]
    fn a_suspend_with_nothing_mounted_asks_the_helper_for_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let mounts = mounts(&fake, dir.path());
        mounts.suspend();
        mounts.resume();
        assert!(
            fake.calls.lock().unwrap().is_empty(),
            "no source was ever delivered"
        );
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

    /// The helper is not dumpable-visible to its sibling processes: the agent sets dumpable to 0,
    /// and opening `/proc/<agent>/ns/mnt` then needs CAP_SYS_PTRACE. The namespace must therefore
    /// reach the helper as a descriptor from the agent, never by a path the helper opens itself.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_namespace_reaches_the_helper_without_opening_the_agents_proc_entry() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;
        // The split this reproduces: a non-dumpable agent and a helper without CAP_SYS_PTRACE.
        // Both run in forked children because dumpable is a property of the whole process.
        // SAFETY: each child only runs the scenario and leaves with _exit.
        let agent = unsafe { libc::fork() };
        assert!(agent >= 0);
        if agent == 0 {
            let outcome = std::panic::catch_unwind(|| {
                assert_eq!(unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) }, 0);
                let (agent_end, helper_end) = UnixStream::pair().unwrap();
                let agent_pid = unsafe { libc::getpid() };
                let helper = unsafe { libc::fork() };
                assert!(helper >= 0);
                if helper == 0 {
                    // The old way: open the agent's entry by path.
                    let path = std::ffi::CString::new(format!("/proc/{agent_pid}/ns/mnt")).unwrap();
                    let by_path = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY) };
                    let by_path_ok = by_path >= 0;
                    // The new way: the agent hands the descriptor over.
                    let received = receive_descriptor(&helper_end).is_ok();
                    // Report: bit 0 = by path worked, bit 1 = received.
                    unsafe { libc::_exit(i32::from(by_path_ok) | (i32::from(received) << 1)) }
                }
                let namespace = std::fs::File::open("/proc/self/ns/mnt").unwrap();
                send_descriptor(&agent_end, namespace.as_raw_fd()).unwrap();
                let mut status = 0;
                assert_eq!(unsafe { libc::waitpid(helper, &mut status, 0) }, helper);
                assert!(libc::WIFEXITED(status));
                let report = libc::WEXITSTATUS(status);
                assert_eq!(report & 2, 2, "the descriptor must arrive");
                // Without CAP_SYS_PTRACE the path is closed. (Under root in a container it can be
                // open, so only the descriptor route is asserted when the path happens to work.)
                eprintln!("open by path worked: {}", report & 1 == 1);
                if unsafe { libc::geteuid() } != 0 {
                    assert_eq!(
                        report & 1,
                        0,
                        "a non-dumpable agent's entry is closed to it"
                    );
                }
            });
            // SAFETY: leave the forked harness copy.
            unsafe { libc::_exit(i32::from(outcome.is_err())) };
        }
        let mut status = 0;
        // SAFETY: waiting for the child just forked.
        assert_eq!(unsafe { libc::waitpid(agent, &mut status, 0) }, agent);
        assert!(libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0);
    }

    #[test]
    fn the_declaration_variable_must_be_a_list_of_mounts() {
        assert!(parse_declared(None).unwrap().is_empty());
        parse_declared(Some("{}")).expect_err("not a list");
        parse_declared(Some(r#"[{"type":"nfs","path":"/x"}]"#)).expect_err("unknown type");
    }
}
