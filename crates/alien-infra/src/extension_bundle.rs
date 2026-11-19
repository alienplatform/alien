//! An extension's bundle, as every loader reads it: the core zip the stack pins by digest, and
//! the controller binaries the zip's manifest pins by sha256. A loader uses nothing in a bundle
//! before its sha256 checks out, and caches the core zip by its digest.

use crate::error::{ErrorData, Result};
use crate::step_protocol::ProxyFactory;
use alien_core::{
    BinaryTarget, ControllerBinary, DeploymentState, ExtensionBundleManifest, ExtensionManifest,
    EXTENSION_BUNDLE_MANIFEST, EXTENSION_CONTROLLER_MODULE, EXTENSION_HOOKS_MODULE,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_hooks_runtime::HookModule;
/// The engine every hooks module of a process runs in. A process starts one and shares it.
pub use alien_hooks_runtime::Runtime;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;

/// The most a manifest may take once decompressed. It is metadata, never a payload.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// The most a hooks module, or any one bundle object, may take: the bundle store's cap per object.
pub const MAX_OBJECT_BYTES: u64 = 64 * 1024 * 1024;

const CORE_OBJECT: &str = "core zip";
const CORE_FILE: &str = "core.zip";

/// The sha256 a digest `sha256:<hex>` names, as 64 lowercase hex characters.
pub fn sha256_hex(digest: &str) -> Option<&str> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))).then_some(hex)
}

/// Where a manager at `manager_url` serves the bundle object `digest`, `sha256:<hex>`: its
/// `GET /v1/extensions/bundles/{digest}` route.
pub fn manager_object_url(manager_url: &str, digest: &str) -> String {
    format!(
        "{}/v1/extensions/bundles/{digest}",
        manager_url.trim_end_matches('/')
    )
}

fn refused(extension: &str, object: &str, reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ExtensionBundleRefused {
        extension: extension.to_string(),
        object: object.to_string(),
        reason: reason.into(),
    })
}

fn unavailable(extension: &str, object: &str, message: impl Into<String>) -> ErrorData {
    ErrorData::ExtensionBundleUnavailable {
        extension: extension.to_string(),
        object: object.to_string(),
        message: message.into(),
    }
}

/// Fails, naming `extension` and `object`, unless `bytes` hash to `expected`, a sha256 hex in
/// either case.
pub fn verify_sha256(extension: &str, object: &str, bytes: &[u8], expected: &str) -> Result<()> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(refused(
            extension,
            object,
            format!("its sha256 is {actual}, not the pinned {expected}"),
        ));
    }
    Ok(())
}

/// An extension's core zip, checked against the digest its stack pins.
#[derive(Debug, Clone)]
pub struct ExtensionBundle {
    extension: String,
    manifest: ExtensionBundleManifest,
    hooks: Vec<u8>,
    /// The controller module the manifest pins, checked against its sha256.
    controller_module: Option<Vec<u8>>,
}

impl ExtensionBundle {
    /// Opens `extension`'s core zip. Fails, naming the extension, unless the zip's sha256 is the
    /// digest the stack pins, the zip is this extension's bundle at this version, and its hooks
    /// module is the one its manifest pins.
    pub fn open(extension: &ExtensionManifest, zip: &[u8]) -> Result<Self> {
        let bundle = Self::open_pinned(extension, zip)?;
        bundle.check_stack(extension)?;
        Ok(bundle)
    }

    /// Opens the core zip `extension`'s digest pins, without checking that the stack describes
    /// it as the bundle does.
    fn open_pinned(extension: &ExtensionManifest, zip: &[u8]) -> Result<Self> {
        let name = extension.name.as_str();
        let pinned = sha256_hex(&extension.digest).ok_or_else(|| {
            refused(
                name,
                CORE_OBJECT,
                format!(
                    "the stack pins digest '{}', which is not sha256:<hex>",
                    extension.digest
                ),
            )
        })?;
        verify_sha256(name, CORE_OBJECT, zip, pinned)?;
        Self::read(name, zip)
    }

    /// Fails, naming the extension, unless this bundle is the one `extension` describes: its
    /// name and version, and the origins its controller module may call.
    pub fn check_stack(&self, extension: &ExtensionManifest) -> Result<()> {
        if self.manifest.name != extension.name || self.manifest.version != extension.version {
            return Err(refused(
                &extension.name,
                CORE_OBJECT,
                format!(
                    "it is the bundle of {} {}, not {} {}",
                    self.manifest.name, self.manifest.version, extension.name, extension.version
                ),
            ));
        }
        // The customer reviews the origins the stack lists; the transport allows the bundle's.
        if self.manifest.egress != extension.egress {
            return Err(refused(
                &extension.name,
                CORE_OBJECT,
                format!(
                    "its controller module may call {:?}, and the stack lists {:?}",
                    self.manifest.egress, extension.egress
                ),
            ));
        }
        Ok(())
    }

    /// Reads a core zip uploaded for extension `name`: the zip must be that extension's bundle,
    /// and its hooks module the one its manifest pins. The caller checks the zip's own digest.
    pub fn read(name: &str, zip: &[u8]) -> Result<Self> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))
            .map_err(|error| refused(name, CORE_OBJECT, format!("it is not a zip: {error}")))?;
        let manifest = read_entry(
            &mut archive,
            name,
            EXTENSION_BUNDLE_MANIFEST,
            MAX_MANIFEST_BYTES,
        )?;
        let manifest: ExtensionBundleManifest =
            serde_json::from_slice(&manifest).map_err(|error| {
                refused(
                    name,
                    CORE_OBJECT,
                    format!("its {EXTENSION_BUNDLE_MANIFEST} is not valid: {error}"),
                )
            })?;
        if manifest.name != name {
            return Err(refused(
                name,
                CORE_OBJECT,
                format!(
                    "it is the bundle of {} {}, not {name}",
                    manifest.name, manifest.version
                ),
            ));
        }
        // A controller's digest names the files it keys, so it is written one way only.
        let digests = manifest
            .controller_module_sha256
            .iter()
            .map(|sha256| (EXTENSION_CONTROLLER_MODULE.to_string(), sha256))
            .chain(manifest.controllers.iter().filter_map(|(target, binary)| {
                binary
                    .as_ref()
                    .map(|binary| (format!("controller for {target}"), &binary.sha256))
            }));
        for (object, sha256) in digests {
            let lowercase_hex = sha256.len() == 64
                && sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !lowercase_hex {
                return Err(refused(
                    name,
                    &object,
                    format!(
                        "its pinned sha256 '{}' is not 64 lowercase hex characters",
                        sha256.escape_debug()
                    ),
                ));
            }
        }
        let hooks = read_entry(&mut archive, name, EXTENSION_HOOKS_MODULE, MAX_OBJECT_BYTES)?;
        verify_sha256(
            name,
            EXTENSION_HOOKS_MODULE,
            &hooks,
            &manifest.hooks_sha256.to_ascii_lowercase(),
        )?;
        let controller_module = match &manifest.controller_module_sha256 {
            Some(sha256) => {
                let module = read_entry(
                    &mut archive,
                    name,
                    EXTENSION_CONTROLLER_MODULE,
                    MAX_OBJECT_BYTES,
                )?;
                verify_sha256(
                    name,
                    EXTENSION_CONTROLLER_MODULE,
                    &module,
                    &sha256.to_ascii_lowercase(),
                )?;
                Some(module)
            }
            None => None,
        };
        Ok(Self {
            extension: name.to_string(),
            manifest,
            hooks,
            controller_module,
        })
    }

    /// What the zip's manifest says about the extension.
    pub fn manifest(&self) -> &ExtensionBundleManifest {
        &self.manifest
    }

    /// Loads the bundle's hooks module into `runtime`.
    pub fn load_hooks(&self, runtime: &Runtime) -> Result<HookModule> {
        // A module the runtime refuses is refused again on another try.
        runtime
            .load(&self.hooks, &self.manifest.hooks_sha256)
            .map_err(|error| {
                alien_error::ContextError::context(
                    error,
                    ErrorData::ExtensionBundleRefused {
                        extension: self.extension.clone(),
                        object: EXTENSION_HOOKS_MODULE.to_string(),
                        reason: "the hooks runtime refused it".to_string(),
                    },
                )
            })
    }

    /// The transport that runs the bundle's controller module in this process for a deployment
    /// of `project`, or `None` when the extension's controllers don't run as WebAssembly.
    pub fn wasm_transport(
        &self,
        runtime: &Runtime,
        host: &Arc<crate::step_protocol::wasm::WasmHost>,
        project: &str,
    ) -> Result<Option<crate::step_protocol::wasm::WasmTransport>> {
        let (Some(module), Some(sha256)) = (
            &self.controller_module,
            &self.manifest.controller_module_sha256,
        ) else {
            return Ok(None);
        };
        // As `load_hooks`: a module the runtime refuses is refused again on another try.
        let module = runtime.load_controller(module, sha256).map_err(|error| {
            alien_error::ContextError::context(
                error,
                ErrorData::ExtensionBundleRefused {
                    extension: self.extension.clone(),
                    object: EXTENSION_CONTROLLER_MODULE.to_string(),
                    reason: "the controller runtime refused it".to_string(),
                },
            )
        })?;
        host.transport(project, &self.extension, module, &self.manifest.egress)
            .map(Some)
    }

    /// The controller binary for this machine, with its target, or `None` when the extension
    /// has no controller. Fails, naming the extension and the machine, when it has controllers
    /// but none for this machine.
    pub fn host_controller(&self) -> Result<Option<(BinaryTarget, &ControllerBinary)>> {
        if self.manifest.controllers.is_empty() {
            return Ok(None);
        }
        let host = host_target();
        match host.and_then(|target| self.manifest.controllers.get(&target)) {
            Some(Some(binary)) => Ok(host.map(|target| (target, binary))),
            _ => {
                let machine = host
                    .map(|target| target.to_string())
                    .unwrap_or_else(|| alien_core::BuildHost::current().id().to_string());
                Err(refused(
                    &self.extension,
                    &format!("controller for {machine}"),
                    format!("the bundle has no controller for {machine}, this machine"),
                ))
            }
        }
    }
}

/// This machine's controller target, or `None` on a machine no bundle has a controller for.
pub fn host_target() -> Option<BinaryTarget> {
    match alien_core::BuildHost::current() {
        alien_core::BuildHost::LinuxX64 => Some(BinaryTarget::LinuxX64),
        alien_core::BuildHost::LinuxArm64 => Some(BinaryTarget::LinuxArm64),
        alien_core::BuildHost::WindowsX64 => Some(BinaryTarget::WindowsX64),
        alien_core::BuildHost::DarwinArm64 => Some(BinaryTarget::DarwinArm64),
        _ => None,
    }
}

fn read_entry(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    extension: &str,
    entry: &str,
    limit: u64,
) -> Result<Vec<u8>> {
    let file = archive.by_name(entry).map_err(|error| {
        refused(
            extension,
            CORE_OBJECT,
            format!("it has no {entry}: {error}"),
        )
    })?;
    let mut bytes = Vec::new();
    // A declared size is not trusted: the read stops one byte past the limit.
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            refused(
                extension,
                CORE_OBJECT,
                format!("its {entry} is unreadable: {error}"),
            )
        })?;
    if bytes.len() as u64 > limit {
        return Err(refused(
            extension,
            CORE_OBJECT,
            format!("its {entry} is larger than {limit} bytes"),
        ));
    }
    Ok(bytes)
}

/// Where a bundle object is downloaded from. A bearer token goes only to its own manager: a
/// presigned URL carries its credential in its query and refuses a second one.
#[derive(Clone, Copy)]
pub struct BundleSource<'a> {
    pub client: &'a reqwest::Client,
    pub url: &'a str,
    pub bearer: Option<&'a str>,
}

/// Verified bundles on disk, one folder per extension and digest: `<name>@sha256-<hex>/`, with
/// its `core.zip`. Each download writes `core.zip.part-<id>`, renamed into place only once its
/// sha256 checks out, so a folder never holds an unverified zip under its final name.
#[derive(Debug, Clone)]
pub struct BundleCache {
    root: PathBuf,
    /// How long one download may take, from the request to the last byte.
    download_timeout: std::time::Duration,
}

/// How long a download may take by default: a bundle object is at most [`MAX_OBJECT_BYTES`], so
/// a download that takes longer than this has stopped, and a later step tries it again.
pub const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

impl BundleCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            download_timeout: DOWNLOAD_TIMEOUT,
        }
    }

    /// The same cache, with downloads that fail once they take longer than `timeout`.
    pub fn with_download_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.download_timeout = timeout;
        self
    }

    /// The folder of `extension`'s bundle.
    pub fn dir(&self, extension: &ExtensionManifest) -> Result<PathBuf> {
        let hex = sha256_hex(&extension.digest).ok_or_else(|| {
            refused(
                &extension.name,
                CORE_OBJECT,
                format!(
                    "the stack pins digest '{}', which is not sha256:<hex>",
                    extension.digest
                ),
            )
        })?;
        // The name becomes a folder name, so it must not be able to leave the cache.
        let folder_safe = extension.name.starts_with(|c: char| c.is_ascii_lowercase())
            && extension
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !folder_safe {
            return Err(refused(
                &extension.name,
                CORE_OBJECT,
                "the extension's name is not lowercase letters, digits and `-`",
            ));
        }
        Ok(self.root.join(format!("{}@sha256-{hex}", extension.name)))
    }

    /// `extension`'s bundle, if its core zip is in the cache. A file is checked against its
    /// digest the first time this process reads it, and again only once its size, modification
    /// time, inode or status-change time changes, or once this process has dropped the bundle to
    /// keep fewer. A file that fails its check is deleted, so the next download replaces it.
    pub async fn cached(&self, extension: &ExtensionManifest) -> Result<Option<ExtensionBundle>> {
        let path = self.dir(extension)?.join(CORE_FILE);
        let Some(seen) = file_version(&extension.name, CORE_OBJECT, &path).await? else {
            return Ok(None);
        };
        let kept = {
            let mut opened = opened();
            let index = opened
                .iter()
                .position(|(at, version, _)| *at == path && *version == seen);
            index.and_then(|index| opened.remove(index)).map(|used| {
                let bundle = used.2.clone();
                opened.push_back(used);
                bundle
            })
        };
        if let Some(bundle) = kept {
            // The folder is named by name and digest, and two stacks may describe one digest
            // differently.
            bundle.check_stack(extension)?;
            return Ok(Some(bundle));
        }
        let zip = tokio::fs::read(&path)
            .await
            .into_alien_error()
            .context(unavailable(
                &extension.name,
                CORE_OBJECT,
                format!("{} is unreadable", path.display()),
            ))?;
        // Only a file that isn't the pinned bundle is dropped; a stack that describes the bundle
        // differently is refused and leaves it for the stacks it matches.
        match ExtensionBundle::open_pinned(extension, &zip) {
            Ok(bundle) => {
                remember(path, seen, bundle.clone());
                bundle.check_stack(extension)?;
                Ok(Some(bundle))
            }
            Err(error) => {
                discard(&path, &error).await;
                Ok(None)
            }
        }
    }

    /// `extension`'s bundle, downloaded from `source` when the cache does not have it.
    pub async fn core(
        &self,
        extension: &ExtensionManifest,
        source: BundleSource<'_>,
    ) -> Result<ExtensionBundle> {
        if let Some(bundle) = self.cached(extension).await? {
            return Ok(bundle);
        }
        let dir = self.dir(extension)?;
        let path = dir.join(CORE_FILE);
        remove_abandoned_parts(&dir).await;
        // Each download writes its own file, so two at once never rename the other's away.
        let part = part_path(&path);
        let zip = match download(
            &extension.name,
            CORE_OBJECT,
            source,
            &part,
            self.download_timeout,
        )
        .await
        {
            Ok(zip) => zip,
            Err(error) => {
                remove_part(&part).await;
                return Err(error);
            }
        };
        let bundle = match ExtensionBundle::open(extension, &zip) {
            Ok(bundle) => bundle,
            Err(error) => {
                remove_part(&part).await;
                return Err(error);
            }
        };
        if let Err(error) = finish(&extension.name, CORE_OBJECT, &part, &path).await {
            remove_part(&part).await;
            return Err(error);
        }
        if let Some(seen) = file_version(&extension.name, CORE_OBJECT, &path).await? {
            remember(path, seen, bundle.clone());
        }
        Ok(bundle)
    }
}

/// A file's size and modification time, and on unix its inode and status-change time, which a
/// write that restores the size and modification time still moves. A file whose version changes
/// is checked again.
type FileVersion = (u64, std::time::SystemTime, u64, i64);

type Opened = (PathBuf, FileVersion, ExtensionBundle);

/// Core zips this process opened, by path, with the version of the file it read, least recently
/// used first.
fn opened() -> std::sync::MutexGuard<'static, VecDeque<Opened>> {
    static OPENED: std::sync::OnceLock<Mutex<VecDeque<Opened>>> = std::sync::OnceLock::new();
    OPENED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How many opened bundles a process keeps, and how many bytes of modules they may hold. Past
/// either, the one used longest ago is read from disk and checked again when it is next needed.
const MAX_OPENED_BUNDLES: usize = 256;
const MAX_OPENED_BYTES: usize = 512 << 20;

/// Keeps `bundle`, read from `path` at version `seen`, as the bundle used last.
fn remember(path: PathBuf, seen: FileVersion, bundle: ExtensionBundle) {
    let mut opened = opened();
    opened.retain(|(at, _, _)| *at != path);
    opened.push_back((path, seen, bundle));
    evict(&mut opened, MAX_OPENED_BUNDLES, MAX_OPENED_BYTES);
}

/// Drops the bundles used longest ago until at most `max_bundles` are left, holding at most
/// `max_bytes` of modules; the one used last always stays.
fn evict(opened: &mut VecDeque<Opened>, max_bundles: usize, max_bytes: usize) {
    let bytes = |opened: &VecDeque<Opened>| -> usize {
        opened
            .iter()
            .map(|(_, _, bundle)| {
                bundle.hooks.len() + bundle.controller_module.as_ref().map_or(0, Vec::len)
            })
            .sum()
    };
    while opened.len() > 1 && (opened.len() > max_bundles || bytes(opened) > max_bytes) {
        opened.pop_front();
    }
}

/// The version of the file at `path`, or `None` when there is no such file.
async fn file_version(extension: &str, object: &str, path: &Path) -> Result<Option<FileVersion>> {
    let unreadable = || {
        unavailable(
            extension,
            object,
            format!("{} is unreadable", path.display()),
        )
    };
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).into_alien_error().context(unreadable()),
    };
    let modified = metadata
        .modified()
        .into_alien_error()
        .context(unreadable())?;
    #[cfg(unix)]
    let (inode, changed) = {
        use std::os::unix::fs::MetadataExt;
        (
            metadata.ino(),
            metadata.ctime() * 1_000_000_000 + metadata.ctime_nsec(),
        )
    };
    #[cfg(not(unix))]
    let (inode, changed) = (0, 0);
    Ok(Some((metadata.len(), modified, inode, changed)))
}

/// Deletes a cached file that failed its check, so the next download replaces it.
async fn discard(path: &Path, error: &AlienError<ErrorData>) {
    tracing::warn!(path = %path.display(), error = %error, "Discarding a cached extension bundle object that failed its check");
    opened().retain(|(at, _, _)| at != path);
    if let Err(error) = tokio::fs::remove_file(path).await {
        tracing::warn!(path = %path.display(), error = %error, "Could not delete a cached extension bundle object that failed its check");
    }
}

/// How long a download's file may sit unchanged before it counts as abandoned: far past any
/// download's own time limit, so no file of a download still running is removed.
const ABANDONED_PART_AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Removes the files of downloads in `dir` that a stopped process left, once they are older
/// than [`ABANDONED_PART_AGE`]. Best effort: a file that can't be read or removed stays.
async fn remove_abandoned_parts(dir: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if !entry.file_name().to_string_lossy().contains(".part-") {
            continue;
        }
        let abandoned = entry
            .metadata()
            .await
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > ABANDONED_PART_AGE);
        if abandoned {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

/// A file beside `path`, of its own for one download.
fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".part-{}", uuid::Uuid::new_v4().simple()));
    path.with_file_name(name)
}

/// How many redirects a download follows.
const MAX_REDIRECTS: usize = 5;

/// Streams `source` into `part` and returns the bytes, failing, retryably, once that takes longer
/// than `timeout`: a server that accepts the request and then stops answering would otherwise
/// hold the step forever. The caller removes `part` when this fails.
async fn download(
    extension: &str,
    object: &str,
    source: BundleSource<'_>,
    part: &Path,
    timeout: std::time::Duration,
) -> Result<Vec<u8>> {
    tokio::time::timeout(timeout, download_unbounded(extension, object, source, part))
        .await
        .unwrap_or_else(|_| {
            Err(AlienError::new(unavailable(
                extension,
                object,
                format!("the download took longer than {}s", timeout.as_secs()),
            )))
        })
}

/// Sends the request for `source`, and follows a redirect the client didn't. A hop to another
/// origin, such as a presigned storage URL, goes through a plain client that carries none of the
/// manager's credentials: `source.client` may hold one as a default header.
async fn request(
    extension: &str,
    object: &str,
    source: &BundleSource<'_>,
) -> Result<reqwest::Response> {
    let failed = |error: reqwest::Error| {
        AlienError::new(unavailable(
            extension,
            object,
            // The URL may be presigned: its query is a credential, so no error carries it.
            format!("the download failed: {}", error.without_url()),
        ))
    };
    let mut url = reqwest::Url::parse(source.url).map_err(|_| {
        refused(
            extension,
            object,
            "its download URL is not a URL".to_string(),
        )
    })?;
    let origin = url.origin();
    let mut plain = None;
    for _ in 0..=MAX_REDIRECTS {
        let request = if url.origin() == origin {
            let request = source.client.get(url.clone());
            match source.bearer {
                Some(token) => request.bearer_auth(token),
                None => request,
            }
        } else {
            let client = match &plain {
                Some(client) => client,
                None => plain.insert(
                    reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .build()
                        .map_err(failed)?,
                ),
            };
            client.get(url.clone())
        };
        let response = request.send().await.map_err(failed)?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|location| location.to_str().ok())
            .and_then(|location| url.join(location).ok())
            .ok_or_else(|| {
                AlienError::new(unavailable(
                    extension,
                    object,
                    format!(
                        "the download returned HTTP {} without a usable location",
                        response.status()
                    ),
                ))
            })?;
        url = location;
    }
    Err(AlienError::new(unavailable(
        extension,
        object,
        format!("the download was redirected more than {MAX_REDIRECTS} times"),
    )))
}

async fn download_unbounded(
    extension: &str,
    object: &str,
    source: BundleSource<'_>,
    part: &Path,
) -> Result<Vec<u8>> {
    let dir = part.parent().unwrap_or(part);
    tokio::fs::create_dir_all(dir)
        .await
        .into_alien_error()
        .context(unavailable(
            extension,
            object,
            format!("{} could not be created", dir.display()),
        ))?;
    let mut response = request(extension, object, &source).await?;
    let status = response.status();
    if !status.is_success() {
        let mut error = AlienError::new(unavailable(
            extension,
            object,
            format!("the download returned HTTP {status}"),
        ));
        // Any other status answers the same request the same way again.
        error.retryable = status.is_server_error()
            || matches!(
                status,
                reqwest::StatusCode::REQUEST_TIMEOUT | reqwest::StatusCode::TOO_MANY_REQUESTS
            );
        return Err(error);
    }
    let mut file = tokio::fs::File::create(part)
        .await
        .into_alien_error()
        .context(unavailable(
            extension,
            object,
            format!("{} could not be written", part.display()),
        ))?;
    let mut bytes = Vec::new();
    loop {
        let chunk = response.chunk().await.map_err(|error| {
            AlienError::new(unavailable(
                extension,
                object,
                format!("the download was cut short: {}", error.without_url()),
            ))
        })?;
        let Some(chunk) = chunk else { break };
        if (bytes.len() + chunk.len()) as u64 > MAX_OBJECT_BYTES {
            return Err(refused(
                extension,
                object,
                format!("it is larger than {MAX_OBJECT_BYTES} bytes"),
            ));
        }
        file.write_all(&chunk)
            .await
            .into_alien_error()
            .context(unavailable(
                extension,
                object,
                format!("{} could not be written", part.display()),
            ))?;
        bytes.extend_from_slice(&chunk);
    }
    file.sync_all()
        .await
        .into_alien_error()
        .context(unavailable(
            extension,
            object,
            format!("{} could not be written", part.display()),
        ))?;
    Ok(bytes)
}

async fn remove_part(part: &Path) {
    let _ = tokio::fs::remove_file(part).await;
}

/// Renames a verified `part` into place as `path`. `rename` within one folder is atomic, so
/// `path` either does not exist or holds the whole verified object.
async fn finish(extension: &str, object: &str, part: &Path, path: &Path) -> Result<()> {
    tokio::fs::rename(part, path)
        .await
        .into_alien_error()
        .context(unavailable(
            extension,
            object,
            format!("{} could not be moved into place", path.display()),
        ))
}

/// Each extension a deployment in `state` lists in the stack an unfinished update applies, its
/// target release and its current release, once per digest, in the order they list them.
pub fn release_extensions(state: &DeploymentState) -> Vec<&ExtensionManifest> {
    let mut extensions: Vec<&ExtensionManifest> = Vec::new();
    let stacks = [
        state.unfinished_update().map(|(_, stack)| stack),
        state.target_release.as_ref().map(|release| &release.stack),
        state.current_release.as_ref().map(|release| &release.stack),
    ];
    for stack in stacks.into_iter().flatten() {
        for extension in &stack.extensions {
            if !extensions
                .iter()
                .any(|seen| seen.digest == extension.digest && seen.name == extension.name)
            {
                extensions.push(extension);
            }
        }
    }
    extensions
}

/// The provider a deployment's extensions are added to, from the host's own `provider`: its
/// local bindings and its runtime credentials, and nothing it loaded for another release.
pub fn base_provider(
    provider: Option<&Arc<dyn crate::PlatformServiceProvider>>,
) -> crate::DefaultPlatformServiceProvider {
    let Some(provider) = provider else {
        return crate::DefaultPlatformServiceProvider::default();
    };
    #[cfg(feature = "local")]
    let base = match provider.get_local_bindings_provider() {
        Some(local) => crate::DefaultPlatformServiceProvider::with_local_bindings(local),
        None => crate::DefaultPlatformServiceProvider::default(),
    };
    #[cfg(not(feature = "local"))]
    let base = crate::DefaultPlatformServiceProvider::default();
    match provider.runtime_registry_credentials() {
        Some(credentials) => base.with_runtime_registry_credentials(credentials),
        None => base,
    }
}

/// `provider` for a deployment in `state`, with the hooks module of each extension its releases
/// list, from `bundles`, the verified bundles this process holds by digest, and the endpoint of
/// each controller in `controllers`, by the same digest. Fails, naming the extension, when
/// `bundles` lacks one.
pub fn provider_for(
    provider: crate::DefaultPlatformServiceProvider,
    state: &DeploymentState,
    runtime: &Runtime,
    bundles: &HashMap<String, Arc<ExtensionBundle>>,
    controllers: &HashMap<String, ProxyFactory>,
) -> Result<crate::DefaultPlatformServiceProvider> {
    provider_with_controllers(provider, state, runtime, bundles, true, |extension, _| {
        Ok(controllers.get(&extension.digest).cloned())
    })
}

/// [`provider_for`], where each extension's proxies also reach the controller `controller`
/// gives for its bundle. `None` leaves the extension's controller unloaded, so stepping one of
/// its resources fails naming it. Without `interceptors`, no client carries the extensions'
/// interceptors.
pub fn provider_with_controllers(
    provider: crate::DefaultPlatformServiceProvider,
    state: &DeploymentState,
    runtime: &Runtime,
    bundles: &HashMap<String, Arc<ExtensionBundle>>,
    interceptors: bool,
    controller: impl Fn(&ExtensionManifest, &ExtensionBundle) -> Result<Option<ProxyFactory>>,
) -> Result<crate::DefaultPlatformServiceProvider> {
    provider.with_releases(state, |stack| {
        let mut loaded = crate::LoadedExtensions::default();
        if !interceptors {
            loaded = loaded.without_interceptors();
        }
        for extension in &stack.extensions {
            let bundle = bundles.get(&extension.digest).ok_or_else(|| {
                AlienError::new(unavailable(
                    &extension.name,
                    CORE_OBJECT,
                    format!("bundle {} is not loaded", extension.digest),
                ))
            })?;
            loaded = loaded.with_hooks(extension.name.clone(), bundle.load_hooks(runtime)?);
            if let Some(factory) = controller(extension, bundle)? {
                loaded = loaded.with_controller(extension.name.clone(), factory);
            }
        }
        Ok(Arc::new(loaded))
    })
}

#[cfg(test)]
mod tests {
    use super::testing::TestBundle;
    use super::*;
    use alien_core::ExtensionProtocols;
    use indexmap::IndexMap;

    /// The provider a release's extensions are added to keeps the host's runtime credentials, so
    /// an enrolled Operator still pulls its images once a release lists an extension.
    #[tokio::test]
    async fn the_base_provider_keeps_the_hosts_runtime_credentials() {
        use crate::PlatformServiceProvider as _;
        struct Pull;
        #[async_trait::async_trait]
        impl crate::RuntimeRegistryCredentialProvider for Pull {
            async fn registry_pull_credential(
                &self,
                deployment_id: &str,
            ) -> Result<alien_client_core::runtime_credentials::RuntimeCapability> {
                Ok(alien_client_core::runtime_credentials::RuntimeCapability {
                    token: format!("pull-{deployment_id}"),
                    expires_at: chrono::Utc::now(),
                })
            }
        }
        let host: Arc<dyn crate::PlatformServiceProvider> = Arc::new(
            crate::DefaultPlatformServiceProvider::default()
                .with_runtime_registry_credentials(Arc::new(Pull)),
        );
        let base = base_provider(Some(&host));
        let pulled = base
            .runtime_registry_pull_credential("dep_1")
            .await
            .unwrap();
        assert_eq!(
            pulled.map(|credential| credential.token).as_deref(),
            Some("pull-dep_1")
        );
        let without = base_provider(None);
        assert!(without
            .runtime_registry_pull_credential("dep_1")
            .await
            .unwrap()
            .is_none());
    }

    /// A hooks module the runtime refuses is refused again on another try, so the failure is
    /// final.
    #[test]
    fn a_hooks_module_the_runtime_refuses_is_not_retried() {
        let hooks = b"not a WebAssembly module";
        let manifest = ExtensionBundleManifest {
            name: "acme".to_string(),
            version: "1.0.0".to_string(),
            protocol: ExtensionProtocols {
                hooks: 1,
                controller: None,
            },
            hooks_sha256: format!("{:x}", Sha256::digest(hooks)),
            controllers: Default::default(),
            controller_module_sha256: None,
            egress: Vec::new(),
            resource_types: IndexMap::new(),
        };
        let zip = testing::core_zip(&manifest, hooks);
        let extension = ExtensionManifest {
            name: "acme".to_string(),
            version: "1.0.0".to_string(),
            digest: format!("sha256:{:x}", Sha256::digest(&zip)),
            egress: Vec::new(),
            resource_types: IndexMap::new(),
        };
        let bundle = ExtensionBundle::open(&extension, &zip).unwrap();
        let Err(error) = bundle.load_hooks(&Runtime::new().unwrap()) else {
            panic!("the runtime loaded a module that is not WebAssembly")
        };
        assert!(!error.retryable, "{error}");
        assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED", "{error}");
        assert_eq!(
            error.source.as_ref().map(|source| source.code.as_str()),
            Some("HOOKS_MODULE_REFUSED"),
            "{error}"
        );
    }

    /// A controller module the runtime refuses is refused again on another try, so the failure
    /// is final, as a hooks module's is.
    #[test]
    fn a_controller_module_the_runtime_refuses_is_not_retried() {
        let bundle = TestBundle::with_controller_module("acme", "1.0.0", b"\0asm", &[]);
        let opened = ExtensionBundle::open(&bundle.manifest, &bundle.zip).unwrap();
        let host = Arc::new(crate::step_protocol::wasm::WasmHost::new().unwrap());
        let Err(error) = opened.wasm_transport(&Runtime::new().unwrap(), &host, "proj") else {
            panic!("the runtime loaded a module that is not WebAssembly")
        };
        assert!(!error.retryable, "{error}");
        assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED", "{error}");
        assert_eq!(
            error.source.as_ref().map(|source| source.code.as_str()),
            Some("CONTROLLER_MODULE_REFUSED"),
            "{error}"
        );
    }

    /// A download the server refuses is refused again; one it could not serve just then, timed
    /// out or throttled may pass on another try.
    #[tokio::test]
    async fn only_a_download_the_server_could_not_serve_then_is_retried() {
        let dir = tempfile::tempdir().unwrap();
        let client = reqwest::Client::new();
        for (status, retryable) in [
            (404, false),
            (403, false),
            (400, false),
            (408, true),
            (429, true),
            (500, true),
            (503, true),
        ] {
            let server = alien_client_core::testing::capture::start(move |_| {
                alien_client_core::testing::capture::Reply::new(status, "text/plain", "no")
            })
            .await;
            let source = BundleSource {
                client: &client,
                url: &server.base_url,
                bearer: None,
            };
            let error = download(
                "acme",
                CORE_OBJECT,
                source,
                &dir.path().join("core.zip.part-test"),
                DOWNLOAD_TIMEOUT,
            )
            .await
            .unwrap_err();
            assert_eq!(error.retryable, retryable, "{status}: {error}");
        }
    }

    /// A pinned controller digest names the file it keys, so it is written one way only: 64
    /// lowercase hex characters.
    #[test]
    fn a_controller_digest_that_is_not_lowercase_hex_is_refused() {
        let hooks = testing::hooks_module("acme");
        let module = b"\0asm\x01\0\0\0";
        let digest = format!("{:x}", Sha256::digest(module));
        let zip_with = |module_sha256: String, binary_sha256: String| {
            let manifest = ExtensionBundleManifest {
                name: "acme".to_string(),
                version: "1.0.0".to_string(),
                protocol: ExtensionProtocols {
                    hooks: 1,
                    controller: Some(1),
                },
                hooks_sha256: format!("{:x}", Sha256::digest(&hooks)),
                controllers: [(
                    BinaryTarget::LinuxX64,
                    Some(alien_core::ControllerBinary {
                        sha256: binary_sha256,
                        size: 1,
                    }),
                )]
                .into_iter()
                .collect(),
                controller_module_sha256: Some(module_sha256),
                egress: Vec::new(),
                resource_types: IndexMap::new(),
            };
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default();
            for (name, bytes) in [
                (
                    EXTENSION_BUNDLE_MANIFEST,
                    serde_json::to_vec(&manifest).unwrap(),
                ),
                (EXTENSION_HOOKS_MODULE, hooks.clone()),
                (EXTENSION_CONTROLLER_MODULE, module.to_vec()),
            ] {
                zip.start_file(name, options).unwrap();
                std::io::Write::write_all(&mut zip, &bytes).unwrap();
            }
            zip.finish().unwrap().into_inner()
        };
        assert!(ExtensionBundle::read("acme", &zip_with(digest.clone(), digest.clone())).is_ok());
        for (module_sha256, binary_sha256) in [
            (digest.to_ascii_uppercase(), digest.clone()),
            (digest.clone(), digest.to_ascii_uppercase()),
            (digest.clone(), "abc".to_string()),
            (digest.clone(), format!("{digest}/../x")),
        ] {
            let error = ExtensionBundle::read("acme", &zip_with(module_sha256, binary_sha256))
                .err()
                .expect("refused");
            assert!(error.to_string().contains("lowercase hex"), "{error}");
        }
    }

    /// A stack that shows the customer other origins than the bundle's module may call is
    /// refused, so the origins the customer reviewed are the ones the transport allows.
    #[test]
    fn the_stack_lists_the_origins_the_bundle_allows() {
        let bundle = TestBundle::with_controller_module(
            "acme",
            "1.0.0",
            b"\0asm",
            &["https://api.acme.example"],
        );
        assert!(ExtensionBundle::open(&bundle.manifest, &bundle.zip).is_ok());

        for listed in [
            vec![],
            vec![
                "https://api.acme.example".to_string(),
                "https://evil.example".to_string(),
            ],
        ] {
            let mut shown = bundle.manifest.clone();
            shown.egress = listed;
            let error = ExtensionBundle::open(&shown, &bundle.zip)
                .err()
                .expect("refused");
            assert!(error.to_string().contains("may call"), "{error}");
        }
    }

    /// A process that opens more bundles than it keeps drops the ones used longest ago, and
    /// reads a dropped one from disk again when it is next needed.
    #[tokio::test]
    async fn opening_more_bundles_than_the_limit_keeps_the_ones_used_last() {
        let root = tempfile::tempdir().unwrap();
        let cache = BundleCache::new(root.path());
        let bundles: Vec<_> = (0..=MAX_OPENED_BUNDLES)
            .map(|patch| TestBundle::new("acme", &format!("1.0.{patch}"), &[]))
            .collect();
        for bundle in &bundles {
            let dir = cache.dir(&bundle.manifest).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(CORE_FILE), &bundle.zip).unwrap();
            assert!(cache.cached(&bundle.manifest).await.unwrap().is_some());
        }
        let first = cache.dir(&bundles[0].manifest).unwrap().join(CORE_FILE);
        let kept = |path: &Path| opened().iter().any(|(at, _, _)| at == path);

        assert_eq!(opened().len(), MAX_OPENED_BUNDLES);
        assert!(!kept(&first), "the bundle used longest ago is dropped");

        let second = cache.dir(&bundles[1].manifest).unwrap().join(CORE_FILE);
        let third = cache.dir(&bundles[2].manifest).unwrap().join(CORE_FILE);
        assert!(cache.cached(&bundles[1].manifest).await.unwrap().is_some());
        let reopened = cache.cached(&bundles[0].manifest).await.unwrap().unwrap();
        assert_eq!(reopened.manifest().version, "1.0.0");
        assert!(kept(&first), "a bundle read again is kept");
        assert!(kept(&second), "a bundle just used is kept");
        assert!(!kept(&third), "the bundle now used longest ago is dropped");
        assert_eq!(opened().len(), MAX_OPENED_BUNDLES);
    }

    /// The bundles used longest ago leave first, by count and by bytes, and the one just used
    /// stays even when it alone is over the bytes.
    #[test]
    fn the_least_recently_used_bundles_leave_by_count_and_bytes() {
        let bundle = TestBundle::new("acme", "1.0.0", &[]);
        let bundle = ExtensionBundle::open(&bundle.manifest, &bundle.zip).unwrap();
        let size = bundle.hooks.len();
        let queue = |paths: &[&str]| -> VecDeque<Opened> {
            paths
                .iter()
                .map(|path| {
                    let version = (0, std::time::SystemTime::UNIX_EPOCH, 0, 0);
                    (PathBuf::from(path), version, bundle.clone())
                })
                .collect()
        };
        let paths = |opened: &VecDeque<Opened>| -> Vec<PathBuf> {
            opened.iter().map(|(path, _, _)| path.clone()).collect()
        };

        let mut opened = queue(&["a", "b", "c"]);
        evict(&mut opened, 2, usize::MAX);
        assert_eq!(paths(&opened), [PathBuf::from("b"), PathBuf::from("c")]);

        let mut opened = queue(&["a", "b", "c"]);
        evict(&mut opened, 10, 2 * size);
        assert_eq!(paths(&opened), [PathBuf::from("b"), PathBuf::from("c")]);

        let mut opened = queue(&["big"]);
        evict(&mut opened, 10, 0);
        assert_eq!(paths(&opened), [PathBuf::from("big")]);
    }
}

/// Builds extension bundles for tests: a core zip with a hooks module that declares no hooks.
#[cfg(any(test, feature = "test-utils"))]
pub mod testing {
    use super::*;
    use alien_core::{ControllerBinary, ExtensionProtocols};
    use indexmap::IndexMap;
    use std::io::Write;

    /// A bundle of extension `name` at `version`, with one controller binary per entry of
    /// `controllers`.
    pub struct TestBundle {
        /// What the stack says about the extension, pinning the zip's digest.
        pub manifest: ExtensionManifest,
        /// The core zip.
        pub zip: Vec<u8>,
        /// Each controller binary, by target.
        pub controllers: IndexMap<BinaryTarget, Vec<u8>>,
    }

    impl TestBundle {
        pub fn new(name: &str, version: &str, controllers: &[(BinaryTarget, &[u8])]) -> Self {
            Self::with_hooks(name, version, "[]", controllers)
        }

        /// A bundle whose controllers run as the controller module `module`, which may call
        /// the origins in `egress`.
        pub fn with_controller_module(
            name: &str,
            version: &str,
            module: &[u8],
            egress: &[&str],
        ) -> Self {
            Self::with_binaries_and_module(name, version, &[], module, egress)
        }

        /// A bundle that carries both a controller binary per entry of `controllers`, as
        /// `alien dev` runs them, and the controller module `module`.
        pub fn with_binaries_and_module(
            name: &str,
            version: &str,
            controllers: &[(BinaryTarget, &[u8])],
            module: &[u8],
            egress: &[&str],
        ) -> Self {
            Self::with_module_and_hooks(name, version, controllers, module, egress, "[]")
        }

        /// [`Self::with_binaries_and_module`], whose hooks module declares `hooks`, a JSON list
        /// of hook entries, and answers every call with an empty output.
        pub fn with_module_and_hooks(
            name: &str,
            version: &str,
            controllers: &[(BinaryTarget, &[u8])],
            module: &[u8],
            egress: &[&str],
            hooks: &str,
        ) -> Self {
            let hooks = hooks_module_declaring(name, hooks, "");
            let bundle_manifest = ExtensionBundleManifest {
                name: name.to_string(),
                version: version.to_string(),
                protocol: ExtensionProtocols {
                    hooks: 1,
                    controller: Some(1),
                },
                hooks_sha256: format!("{:x}", Sha256::digest(&hooks)),
                controllers: controllers
                    .iter()
                    .map(|(target, bytes)| {
                        (
                            *target,
                            Some(ControllerBinary {
                                sha256: format!("{:x}", Sha256::digest(bytes)),
                                size: bytes.len() as u64,
                            }),
                        )
                    })
                    .collect(),
                resource_types: IndexMap::new(),
                controller_module_sha256: Some(format!("{:x}", Sha256::digest(module))),
                egress: egress.iter().map(|origin| origin.to_string()).collect(),
            };
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file(EXTENSION_BUNDLE_MANIFEST, options).unwrap();
            zip.write_all(&serde_json::to_vec(&bundle_manifest).unwrap())
                .unwrap();
            zip.start_file(EXTENSION_HOOKS_MODULE, options).unwrap();
            zip.write_all(&hooks).unwrap();
            zip.start_file(alien_core::EXTENSION_CONTROLLER_MODULE, options)
                .unwrap();
            zip.write_all(module).unwrap();
            let zip = zip.finish().unwrap().into_inner();
            Self {
                manifest: ExtensionManifest {
                    name: name.to_string(),
                    version: version.to_string(),
                    digest: format!("sha256:{:x}", Sha256::digest(&zip)),
                    resource_types: IndexMap::new(),
                    egress: egress.iter().map(|origin| origin.to_string()).collect(),
                },
                zip,
                controllers: controllers
                    .iter()
                    .map(|(target, bytes)| (*target, bytes.to_vec()))
                    .collect(),
            }
        }

        /// A bundle whose hooks module declares `hooks`, a JSON list of hook entries, and
        /// answers every call with an empty output.
        pub fn with_hooks(
            name: &str,
            version: &str,
            hooks: &str,
            controllers: &[(BinaryTarget, &[u8])],
        ) -> Self {
            Self::with_hooks_answering(name, version, hooks, "", controllers)
        }

        /// A bundle whose hooks module declares `hooks` and answers every call with `output`.
        pub fn with_hooks_answering(
            name: &str,
            version: &str,
            hooks: &str,
            output: &str,
            controllers: &[(BinaryTarget, &[u8])],
        ) -> Self {
            let hooks = hooks_module_declaring(name, hooks, output);
            let bundle_manifest = ExtensionBundleManifest {
                name: name.to_string(),
                version: version.to_string(),
                protocol: ExtensionProtocols {
                    hooks: 1,
                    controller: (!controllers.is_empty()).then_some(1),
                },
                hooks_sha256: format!("{:x}", Sha256::digest(&hooks)),
                controllers: controllers
                    .iter()
                    .map(|(target, bytes)| {
                        (
                            *target,
                            Some(ControllerBinary {
                                sha256: format!("{:x}", Sha256::digest(bytes)),
                                size: bytes.len() as u64,
                            }),
                        )
                    })
                    .collect(),
                resource_types: IndexMap::new(),
                controller_module_sha256: None,
                egress: Vec::new(),
            };
            let zip = core_zip(&bundle_manifest, &hooks);
            Self {
                manifest: ExtensionManifest {
                    name: name.to_string(),
                    version: version.to_string(),
                    digest: format!("sha256:{:x}", Sha256::digest(&zip)),
                    resource_types: IndexMap::new(),
                    egress: Vec::new(),
                },
                zip,
                controllers: controllers
                    .iter()
                    .map(|(target, bytes)| (*target, bytes.to_vec()))
                    .collect(),
            }
        }
    }

    /// The controller module of `tests/controller-guest`, built for wasm32-wasip1 once per
    /// test process. Its custom type is `acme/probe`: each step runs the resource's `probe`
    /// with its `arg` and reports the result as the step's progress.
    ///
    /// Test processes share the build's target directory, and each holds a lock on it from the
    /// build to the read, since a build that finds the module fresh can still replace the file
    /// another process is about to read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn controller_guest() -> &'static [u8] {
        static GUEST: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        GUEST.get_or_init(|| {
            let krate = Path::new(env!("CARGO_MANIFEST_DIR"));
            let target_dir = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| krate.join("../../target"))
                .join("controller-test-guest");
            std::fs::create_dir_all(&target_dir).expect("the target directory can be created");
            let lock = std::fs::File::create(target_dir.join("test.lock"))
                .expect("the lock file can be created");
            lock.lock().expect("the lock is taken");
            let output = std::process::Command::new(env!("CARGO"))
                .args(["build", "--release", "--locked", "--target", "wasm32-wasip1"])
                .arg("--manifest-path")
                .arg(krate.join("tests/controller-guest/Cargo.toml"))
                .arg("--target-dir")
                .arg(&target_dir)
                // Flags meant for the host build (a linker, a target) must not reach this one.
                .env_remove("RUSTFLAGS")
                .env_remove("CARGO_ENCODED_RUSTFLAGS")
                .env_remove("CARGO_BUILD_TARGET")
                .env_remove("CARGO_TARGET_DIR")
                .output()
                .expect("cargo runs");
            assert!(
                output.status.success(),
                "building the test guest for wasm32-wasip1 failed; `rustup target add wasm32-wasip1` installs the target:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            std::fs::read(target_dir.join("wasm32-wasip1/release/controller_test_guest.wasm"))
                .expect("the guest module was built")
        })
    }

    /// A core zip holding `manifest` and `hooks`.
    pub fn core_zip(manifest: &ExtensionBundleManifest, hooks: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(EXTENSION_BUNDLE_MANIFEST, options).unwrap();
        zip.write_all(&serde_json::to_vec(manifest).unwrap())
            .unwrap();
        zip.start_file(EXTENSION_HOOKS_MODULE, options).unwrap();
        zip.write_all(hooks).unwrap();
        zip.finish().unwrap().into_inner()
    }

    /// A hooks module for `extension` that declares no hooks.
    pub fn hooks_module(extension: &str) -> Vec<u8> {
        hooks_module_declaring(extension, "[]", "")
    }

    /// A hooks module whose manifest declares `hooks` and whose every call returns `output`,
    /// which sits in a page of its own after the manifest's.
    fn hooks_module_declaring(extension: &str, hooks: &str, output: &str) -> Vec<u8> {
        let manifest = format!(r#"{{"extension":"{extension}","hooks":{hooks}}}"#);
        let packed = 16u64 | ((manifest.len() as u64) << 32);
        let answer = if output.is_empty() {
            0
        } else {
            65_536u64 | ((output.len() as u64) << 32)
        };
        wat::parse_str(format!(
            r#"(module
                (memory (export "memory") 3)
                (data (i32.const 16) "{}")
                (data (i32.const 65536) "{}")
                (func (export "alien_hooks_abi_v1"))
                (func (export "alien_alloc") (param i32) (result i32) i32.const 131072)
                (func (export "alien_hooks_manifest") (result i64) i64.const {packed})
                (func (export "alien_hooks_call") (param i32 i32 i32 i32) (result i64) i64.const {answer}))"#,
            manifest.replace('"', "\\\""),
            output.replace('"', "\\\"")
        ))
        .unwrap()
    }
}
