//! An extension's bundle, as every loader reads it: the core zip the stack pins by digest, and
//! the controller binaries the zip's manifest pins by sha256. A loader uses nothing in a bundle
//! before its sha256 checks out, and caches each object by its digest.

use crate::error::{ErrorData, Result};
use crate::step_protocol::ProxyFactory;
use alien_core::{
    BinaryTarget, ControllerBinary, DeploymentState, ExtensionBundleManifest, ExtensionManifest,
    EXTENSION_BUNDLE_MANIFEST, EXTENSION_HOOKS_MODULE,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_hooks_runtime::HookModule;
/// The engine every hooks module of a process runs in. A process starts one and shares it.
pub use alien_hooks_runtime::Runtime;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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

/// The object name a controller binary goes by in errors: `controller for <target>`.
fn controller_object(target: BinaryTarget) -> String {
    format!("controller for {target}")
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
}

impl ExtensionBundle {
    /// Opens `extension`'s core zip. Fails, naming the extension, unless the zip's sha256 is the
    /// digest the stack pins, the zip is this extension's bundle at this version, and its hooks
    /// module is the one its manifest pins.
    pub fn open(extension: &ExtensionManifest, zip: &[u8]) -> Result<Self> {
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
        let bundle = Self::read(name, zip)?;
        if bundle.manifest.version != extension.version {
            return Err(refused(
                name,
                CORE_OBJECT,
                format!(
                    "it is the bundle of {} {}, not {} {}",
                    bundle.manifest.name,
                    bundle.manifest.version,
                    extension.name,
                    extension.version
                ),
            ));
        }
        Ok(bundle)
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
        let hooks = read_entry(&mut archive, name, EXTENSION_HOOKS_MODULE, MAX_OBJECT_BYTES)?;
        verify_sha256(
            name,
            EXTENSION_HOOKS_MODULE,
            &hooks,
            &manifest.hooks_sha256.to_ascii_lowercase(),
        )?;
        Ok(Self {
            extension: name.to_string(),
            manifest,
            hooks,
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
                let retryable = error.retryable;
                let mut wrapped = Err::<(), _>(error)
                    .context(unavailable(
                        &self.extension,
                        EXTENSION_HOOKS_MODULE,
                        "the hooks runtime refused it",
                    ))
                    .unwrap_err();
                wrapped.retryable = retryable;
                wrapped
            })
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
/// `core.zip` and one `controller-<target>` per controller this machine runs. A file is written
/// as `<file>.part` and renamed into place only once its sha256 checks out, so a folder never
/// holds an unverified object under its final name.
#[derive(Debug, Clone)]
pub struct BundleCache {
    root: PathBuf,
}

impl BundleCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
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
    /// time, inode or status-change time changes. A file that fails its check is deleted, so the
    /// next download replaces it.
    pub async fn cached(&self, extension: &ExtensionManifest) -> Result<Option<ExtensionBundle>> {
        let path = self.dir(extension)?.join(CORE_FILE);
        let Some(seen) = file_version(&extension.name, CORE_OBJECT, &path).await? else {
            return Ok(None);
        };
        if let Some(bundle) = opened().get(&path).filter(|(at, _)| *at == seen) {
            return Ok(Some(bundle.1.clone()));
        }
        let zip = tokio::fs::read(&path)
            .await
            .into_alien_error()
            .context(unavailable(
                &extension.name,
                CORE_OBJECT,
                format!("{} is unreadable", path.display()),
            ))?;
        match ExtensionBundle::open(extension, &zip) {
            Ok(bundle) => {
                opened().insert(path, (seen, bundle.clone()));
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
        let zip = download(&extension.name, CORE_OBJECT, source, &path).await?;
        let bundle = match ExtensionBundle::open(extension, &zip) {
            Ok(bundle) => bundle,
            Err(error) => {
                remove_part(&path).await;
                return Err(error);
            }
        };
        finish(&extension.name, CORE_OBJECT, &path, false).await?;
        if let Some(seen) = file_version(&extension.name, CORE_OBJECT, &path).await? {
            opened().insert(path, (seen, bundle.clone()));
        }
        Ok(bundle)
    }

    /// The path of `bundle`'s controller for this machine, downloaded from `source` when the
    /// cache does not have it, or `None` when the extension has no controller. Fails, naming the
    /// extension and the machine, when the bundle has none for this machine, or when the cache
    /// lacks it and there is no `source`.
    pub async fn controller(
        &self,
        extension: &ExtensionManifest,
        bundle: &ExtensionBundle,
        source: Option<BundleSource<'_>>,
    ) -> Result<Option<PathBuf>> {
        let Some((target, binary)) = bundle.host_controller()? else {
            return Ok(None);
        };
        let object = controller_object(target);
        let expected = binary.sha256.to_ascii_lowercase();
        let path = self.dir(extension)?.join(format!("controller-{target}"));
        if let Some(seen) = file_version(&extension.name, &object, &path).await? {
            if verified().get(&path) == Some(&(seen, expected.clone())) {
                return Ok(Some(path));
            }
            #[cfg(test)]
            hashed_from_disk(&path);
            let bytes = tokio::fs::read(&path)
                .await
                .into_alien_error()
                .context(unavailable(
                    &extension.name,
                    &object,
                    format!("{} is unreadable", path.display()),
                ))?;
            match verify_sha256(&extension.name, &object, &bytes, &expected) {
                Ok(()) => {
                    verified().insert(path.clone(), (seen, expected));
                    return Ok(Some(path));
                }
                Err(error) => discard(&path, &error).await,
            }
        }
        let source = source.ok_or_else(|| {
            AlienError::new(unavailable(
                &extension.name,
                &object,
                "it is not downloaded, and no download was offered for it",
            ))
        })?;
        let bytes = download(&extension.name, &object, source, &path).await?;
        if let Err(error) = verify_sha256(&extension.name, &object, &bytes, &expected) {
            remove_part(&path).await;
            return Err(error);
        }
        finish(&extension.name, &object, &path, true).await?;
        if let Some(seen) = file_version(&extension.name, &object, &path).await? {
            verified().insert(path.clone(), (seen, expected));
        }
        Ok(Some(path))
    }
}

/// A file's size and modification time, and on unix its inode and status-change time, which a
/// write that restores the size and modification time still moves. A file whose version changes
/// is checked again.
type FileVersion = (u64, std::time::SystemTime, u64, i64);

/// Core zips this process opened, by path, with the version of the file it read.
fn opened() -> std::sync::MutexGuard<'static, HashMap<PathBuf, (FileVersion, ExtensionBundle)>> {
    static OPENED: std::sync::OnceLock<Mutex<HashMap<PathBuf, (FileVersion, ExtensionBundle)>>> =
        std::sync::OnceLock::new();
    OPENED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Controller binaries this process checked, by path, with the file's version and the sha256
/// it matched.
fn verified() -> std::sync::MutexGuard<'static, HashMap<PathBuf, (FileVersion, String)>> {
    static VERIFIED: std::sync::OnceLock<Mutex<HashMap<PathBuf, (FileVersion, String)>>> =
        std::sync::OnceLock::new();
    VERIFIED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    opened().remove(path);
    verified().remove(path);
    if let Err(error) = tokio::fs::remove_file(path).await {
        tracing::warn!(path = %path.display(), error = %error, "Could not delete a cached extension bundle object that failed its check");
    }
}

fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

/// Streams `source` into `<path>.part` and returns the bytes. A download cut short leaves only
/// the `.part` file, which the next download replaces.
async fn download(
    extension: &str,
    object: &str,
    source: BundleSource<'_>,
    path: &Path,
) -> Result<Vec<u8>> {
    let dir = path.parent().unwrap_or(path);
    tokio::fs::create_dir_all(dir)
        .await
        .into_alien_error()
        .context(unavailable(
            extension,
            object,
            format!("{} could not be created", dir.display()),
        ))?;
    let mut request = source.client.get(source.url);
    if let Some(token) = source.bearer {
        request = request.bearer_auth(token);
    }
    // The URL may be presigned: its query is a credential, so no error carries it.
    let mut response = request.send().await.map_err(|error| {
        AlienError::new(unavailable(
            extension,
            object,
            format!("the download failed: {}", error.without_url()),
        ))
    })?;
    if !response.status().is_success() {
        return Err(AlienError::new(unavailable(
            extension,
            object,
            format!("the download returned HTTP {}", response.status()),
        )));
    }
    let part = part_path(path);
    let mut file = tokio::fs::File::create(&part)
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

async fn remove_part(path: &Path) {
    let _ = tokio::fs::remove_file(part_path(path)).await;
}

/// Renames a verified `<path>.part` into place. `rename` within one folder is atomic, so `path`
/// either does not exist or holds the whole verified object.
async fn finish(extension: &str, object: &str, path: &Path, executable: bool) -> Result<()> {
    let part = part_path(path);
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755))
            .await
            .into_alien_error()
            .context(unavailable(
                extension,
                object,
                format!("{} could not be made executable", part.display()),
            ))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    tokio::fs::rename(&part, path)
        .await
        .into_alien_error()
        .context(unavailable(
            extension,
            object,
            format!("{} could not be moved into place", path.display()),
        ))
}

/// Each extension the current and target releases of a deployment in `state` list, once per
/// digest, in the order the releases list them.
pub fn release_extensions(state: &DeploymentState) -> Vec<&ExtensionManifest> {
    let mut extensions: Vec<&ExtensionManifest> = Vec::new();
    for release in [&state.target_release, &state.current_release]
        .into_iter()
        .flatten()
    {
        for extension in &release.stack.extensions {
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
    provider_with_controllers(provider, state, runtime, bundles, |extension, _| {
        Ok(controllers.get(&extension.digest).cloned())
    })
}

/// [`provider_for`], where each extension's proxies also reach the controller `controller`
/// gives for its bundle. `None` leaves the extension's controller unloaded, so stepping one of
/// its resources fails naming it.
pub fn provider_with_controllers(
    provider: crate::DefaultPlatformServiceProvider,
    state: &DeploymentState,
    runtime: &Runtime,
    bundles: &HashMap<String, Arc<ExtensionBundle>>,
    controller: impl Fn(&ExtensionManifest, &ExtensionBundle) -> Result<Option<ProxyFactory>>,
) -> Result<crate::DefaultPlatformServiceProvider> {
    provider.with_releases(state, |stack| {
        let mut loaded = crate::LoadedExtensions::default();
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

/// How many times this process hashed each cached controller from disk.
#[cfg(test)]
fn hashed() -> std::sync::MutexGuard<'static, HashMap<PathBuf, usize>> {
    static HASHED: std::sync::OnceLock<Mutex<HashMap<PathBuf, usize>>> = std::sync::OnceLock::new();
    HASHED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
fn hashed_from_disk(path: &Path) {
    *hashed().entry(path.to_path_buf()).or_default() += 1;
}

#[cfg(test)]
mod tests {
    use super::testing::TestBundle;
    use super::*;
    use alien_core::ExtensionProtocols;
    use indexmap::IndexMap;

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
            controller_url: None,
            hooks_sha256: format!("{:x}", Sha256::digest(hooks)),
            controllers: Default::default(),
            resource_types: IndexMap::new(),
        };
        let zip = testing::core_zip(&manifest, hooks);
        let extension = ExtensionManifest {
            name: "acme".to_string(),
            version: "1.0.0".to_string(),
            digest: format!("sha256:{:x}", Sha256::digest(&zip)),
            resource_types: IndexMap::new(),
        };
        let bundle = ExtensionBundle::open(&extension, &zip).unwrap();
        let Err(error) = bundle.load_hooks(&Runtime::new().unwrap()) else {
            panic!("the runtime loaded a module that is not WebAssembly")
        };
        assert!(!error.retryable, "{error}");
    }

    /// A cached controller is hashed once per process, and again once the file changes.
    #[tokio::test]
    async fn a_cached_controller_is_hashed_once_until_it_changes() {
        let target = host_target().expect("tests run on a machine with a controller target");
        let bundle = TestBundle::new("acme", "1.0.0", &[(target, b"controller")]);
        let opened = ExtensionBundle::open(&bundle.manifest, &bundle.zip).unwrap();
        let root = tempfile::tempdir().unwrap();
        let cache = BundleCache::new(root.path());
        let path = cache
            .dir(&bundle.manifest)
            .unwrap()
            .join(format!("controller-{target}"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"controller").unwrap();

        for _ in 0..3 {
            cache
                .controller(&bundle.manifest, &opened, None)
                .await
                .unwrap();
        }
        assert_eq!(hashed().get(&path), Some(&1));

        std::fs::write(&path, b"controllex").unwrap();
        let refused = cache.controller(&bundle.manifest, &opened, None).await;
        assert_eq!(hashed().get(&path), Some(&2));
        assert!(refused.is_err(), "a changed file is checked again");
        assert!(!path.exists(), "and deleted when it fails");
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

        /// A bundle whose hooks module declares `hooks`, a JSON list of hook entries, and
        /// answers every call with an empty output.
        pub fn with_hooks(
            name: &str,
            version: &str,
            hooks: &str,
            controllers: &[(BinaryTarget, &[u8])],
        ) -> Self {
            Self::build(name, version, hooks, controllers, None)
        }

        /// A bundle whose controllers run on the server at `controller_url`, as a push
        /// deployment's manager reaches them.
        pub fn serving(name: &str, version: &str, controller_url: &str) -> Self {
            Self::build(name, version, "[]", &[], Some(controller_url.to_string()))
        }

        fn build(
            name: &str,
            version: &str,
            hooks: &str,
            controllers: &[(BinaryTarget, &[u8])],
            controller_url: Option<String>,
        ) -> Self {
            let hooks = hooks_module_declaring(name, hooks);
            let bundle_manifest = ExtensionBundleManifest {
                name: name.to_string(),
                version: version.to_string(),
                protocol: ExtensionProtocols {
                    hooks: 1,
                    controller: (!controllers.is_empty() || controller_url.is_some()).then_some(1),
                },
                controller_url,
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
            };
            let zip = core_zip(&bundle_manifest, &hooks);
            Self {
                manifest: ExtensionManifest {
                    name: name.to_string(),
                    version: version.to_string(),
                    digest: format!("sha256:{:x}", Sha256::digest(&zip)),
                    resource_types: IndexMap::new(),
                },
                zip,
                controllers: controllers
                    .iter()
                    .map(|(target, bytes)| (*target, bytes.to_vec()))
                    .collect(),
            }
        }
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
        hooks_module_declaring(extension, "[]")
    }

    fn hooks_module_declaring(extension: &str, hooks: &str) -> Vec<u8> {
        let manifest = format!(r#"{{"extension":"{extension}","hooks":{hooks}}}"#);
        let packed = 16u64 | ((manifest.len() as u64) << 32);
        wat::parse_str(format!(
            r#"(module
                (memory (export "memory") 1)
                (data (i32.const 16) "{}")
                (func (export "alien_hooks_abi_v1"))
                (func (export "alien_alloc") (param i32) (result i32) i32.const 1024)
                (func (export "alien_hooks_manifest") (result i64) i64.const {packed})
                (func (export "alien_hooks_call") (param i32 i32 i32 i32) (result i64) i64.const 0))"#,
            manifest.replace('"', "\\\"")
        ))
        .unwrap()
    }
}
