//! What `alien release` does with the extensions a stack lists.
//!
//! alien.ts is evaluated first. The workspace of each extension it lists is built, and alien.ts
//! is evaluated again for the digests the builds wrote. When alien.ts doesn't evaluate, as in a
//! fresh checkout, each extension workspace with no `generated/` module yet is built before it
//! is evaluated again. A stack without extensions builds nothing.
//!
//! Every bundle the built stack pins is then found by that digest and checked: its manifest, its
//! protocol versions, its hooks module, its controller module and binaries, and, where its
//! workspace's importer fixtures are at hand, the importers of the hooks module it ships against
//! the controller module it ships. The release then has a list of objects to upload. None of
//! this runs cargo for `--prebuilt`.
//!
//! Each extension's bundle is found from the folder its generated module sits in, the
//! workspace's `generated/`: `alien extensions build` writes the bundle to
//! `../target/alien-extension/` from there, in a workspace or in the npm package that ships it.
//! The folder is the one the module recorded when alien.ts imported it. A release that doesn't
//! evaluate alien.ts, `--prebuilt`, looks for the modules under the project instead.

use std::path::{Path, PathBuf};

use alien_core::{BinaryTarget, ExtensionBundleManifest, ExtensionManifest, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::build::{build, check_name, HOOKS_PROTOCOL};
use super::read_workspace;
use super::test::check_shipped_imports;
use crate::config::LoadedConfiguration;
use crate::error::{ErrorData, Result};

/// How deep under the project directory an extension's workspace may sit.
const SEARCH_DEPTH: usize = 4;

/// A bundle on disk: `<name>-core.zip` and the controller binaries beside it.
#[derive(Debug)]
pub(crate) struct FoundBundle {
    /// The `target/alien-extension/` directory.
    pub dir: PathBuf,
    pub zip: Vec<u8>,
    pub manifest: ExtensionBundleManifest,
    /// The extension's Cargo workspace, when the bundle sits beside one.
    pub workspace_root: Option<PathBuf>,
}

impl FoundBundle {
    fn digest(&self) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(&self.zip)))
    }
}

/// One object the release uploads for an extension, stored by its sha256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Upload {
    pub extension: String,
    /// `core` for the zip, or a controller target.
    pub object: String,
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
}

/// Where `alien extensions build` writes the bundle, from the folder of the module it
/// generates: `<root>/target/alien-extension/` from `<root>/generated/`.
pub(crate) fn bundle_dir(module_dir: &Path) -> PathBuf {
    module_dir
        .parent()
        .unwrap_or(module_dir)
        .join("target")
        .join("alien-extension")
}

/// The extension workspaces under `project_dir`: each directory whose Cargo.toml has the
/// `[workspace.metadata.alien-extension]` table, at most `SEARCH_DEPTH` levels down. The walk
/// follows no symlink and skips hidden directories, `target` and `node_modules`, whose packages
/// ship built bundles. Each workspace appears once, by its canonical path.
pub(crate) fn extension_workspaces(project_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in project_dirs(project_dir) {
        if !super::is_extension_workspace(&dir)? {
            continue;
        }
        let canonical = dir.canonicalize().unwrap_or(dir);
        if !found.contains(&canonical) {
            found.push(canonical);
        }
    }
    Ok(found)
}

/// The stack alien.ts describes, and the extension modules it imports, for a release, with the
/// extensions it lists built first.
///
/// `evaluate` runs alien.ts. A stack that lists no extension is returned as is, and nothing
/// builds, whatever the project holds. For one that does, the workspace of each extension it
/// lists is built where alien.ts imports it from, and alien.ts is evaluated again for the digests
/// the builds wrote; an extension with no workspace, from an installed package, ships the bundle
/// it came with. A listed extension that doesn't build fails the release. Each controller builds
/// for `targets`, or for every target this machine can build when it is empty.
///
/// When alien.ts doesn't evaluate, as in a fresh checkout, each extension workspace under the
/// project with no `generated/` module yet is built first, since alien.ts may import that module;
/// one outside the project is not, so alien.ts that imports its module still fails until
/// `alien extensions build` runs there. A build that fails there is a warning, reported with the
/// error if alien.ts still doesn't evaluate. Once it evaluates, the extensions it lists build as
/// above.
pub(crate) async fn evaluate_building_extensions<F, Fut>(
    project_dir: &Path,
    targets: &[BinaryTarget],
    evaluate: F,
) -> Result<LoadedConfiguration>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<LoadedConfiguration>>,
{
    let (configuration, built) = match evaluate().await {
        Ok(configuration) => {
            return build_listed(configuration, project_dir, targets, &[], &evaluate).await
        }
        Err(error) => {
            let roots: Vec<PathBuf> = match extension_workspaces(project_dir) {
                Ok(roots) => roots,
                Err(scan) => {
                    return Err(error).context(ErrorData::ReleaseFailed {
                        message: format!(
                            "the project's extension workspaces could not be listed, so none were built first: {scan}"
                        ),
                    });
                }
            }
            .into_iter()
            .filter(|root| !root.join("generated").join("index.ts").is_file())
            .collect();
            if roots.is_empty() {
                return Err(error);
            }
            let mut built = Vec::new();
            let mut warnings = Vec::new();
            for root in roots {
                match build_workspace(&root, targets).await {
                    Ok(()) => built.push(root),
                    Err(failure) => {
                        warn!("{failure}");
                        warnings.push(failure.to_string());
                    }
                }
            }
            match evaluate().await {
                Ok(configuration) => (configuration, built),
                Err(error) if warnings.is_empty() => return Err(error),
                Err(error) => {
                    return Err(error).context(ErrorData::ReleaseFailed {
                        message: format!(
                            "alien.ts did not evaluate after the project's new extension workspaces were built, and these builds failed:\n{}",
                            warnings.join("\n")
                        ),
                    })
                }
            }
        }
    };
    build_listed(configuration, project_dir, targets, &built, &evaluate).await
}

/// Builds the workspace of each extension `stack` lists, except those in `built`, and evaluates
/// alien.ts again when any built.
async fn build_listed<F, Fut>(
    configuration: LoadedConfiguration,
    project_dir: &Path,
    targets: &[BinaryTarget],
    built: &[PathBuf],
    evaluate: &F,
) -> Result<LoadedConfiguration>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<LoadedConfiguration>>,
{
    if configuration.stack.extensions.is_empty() {
        return Ok(configuration);
    }
    let roots: Vec<PathBuf> = listed_workspaces(&configuration, project_dir)?
        .into_iter()
        .filter(|root| !built.contains(root))
        .collect();
    if roots.is_empty() {
        return Ok(configuration);
    }
    for root in &roots {
        build_workspace(root, targets).await?;
    }
    evaluate().await
}

async fn build_workspace(root: &Path, targets: &[BinaryTarget]) -> Result<()> {
    info!("Building the extension in {}", root.display());
    let failed = || ErrorData::ReleaseFailed {
        message: format!("the extension in {} did not build", root.display()),
    };
    let workspace = read_workspace(root).context(failed())?;
    build(&workspace, targets).await.context(failed())?;
    Ok(())
}

/// The workspace of each extension the configuration lists: the extension workspace whose
/// `generated/` module, the one alien.ts imports, names it. A module in an installed package,
/// whose path runs through `node_modules` below the project, ships its bundle and is never built;
/// a workspace a package manager links into `node_modules` resolves to its own folder and is built.
fn listed_workspaces(
    configuration: &LoadedConfiguration,
    project_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let stack = &configuration.stack;
    let project = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());
    let mut roots: Vec<PathBuf> = Vec::new();
    for module_dir in imported_module_dirs(configuration, project_dir)? {
        // Below the project root, so a project inside a folder named node_modules still builds.
        if module_dir
            .strip_prefix(&project)
            .unwrap_or(&module_dir)
            .components()
            .any(|component| component.as_os_str() == "node_modules")
        {
            continue;
        }
        let Some(root) = module_dir.parent() else {
            continue;
        };
        if !super::is_extension_workspace(root)? {
            continue;
        }
        let name = generated_module_name(&module_dir.join("index.ts"))
            .map_err(|message| AlienError::new(ErrorData::ReleaseFailed { message }))?;
        let listed = stack
            .extensions
            .iter()
            .any(|extension| extension.name == name);
        if listed && !roots.iter().any(|known| known == root) {
            roots.push(root.to_path_buf());
        }
    }
    Ok(roots)
}

/// The extension name in a module `alien extensions build` generated, which opens with
/// `const extension: ExtensionManifest = <manifest JSON>`.
fn generated_module_name(index_ts: &Path) -> std::result::Result<String, String> {
    let unreadable = |reason: &str| {
        format!(
            "{} is not a module `alien extensions build` generated: {reason}. Run `alien extensions build` in its workspace",
            index_ts.display()
        )
    };
    let source = std::fs::read_to_string(index_ts)
        .map_err(|error| unreadable(&format!("it can't be read ({error})")))?;
    let (_, manifest) = source
        .split_once("const extension: ExtensionManifest = ")
        .ok_or_else(|| unreadable("it declares no `extension` manifest"))?;
    let manifest = serde_json::Deserializer::from_str(manifest)
        .into_iter::<serde_json::Value>()
        .next()
        .and_then(|manifest| manifest.ok())
        .ok_or_else(|| unreadable("its manifest is not JSON"))?;
    manifest["name"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| unreadable("its manifest names no extension"))
}

/// The folder of the generated module the configuration imported for each extension it lists.
/// A listed extension with no recorded module, from an alien.json or a module an older CLI
/// generated, is looked for with [`local_module_dirs`].
pub(crate) fn imported_module_dirs(
    configuration: &LoadedConfiguration,
    project_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut unrecorded = false;
    for extension in &configuration.stack.extensions {
        let mut recorded = Vec::new();
        for module in configuration
            .extension_modules
            .iter()
            .filter(|module| module.name == extension.name)
        {
            let file = url::Url::parse(&module.url)
                .ok()
                .filter(|url| url.scheme() == "file")
                .and_then(|url| url.to_file_path().ok())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ReleaseFailed {
                        message: format!(
                            "{}: its generated module was loaded from {}, which is not a file",
                            extension.name, module.url
                        ),
                    })
                })?;
            if let Some(dir) = file.parent() {
                recorded.push(dir.to_path_buf());
            }
        }
        unrecorded |= recorded.is_empty();
        for dir in recorded {
            let canonical = dir.canonicalize().unwrap_or(dir);
            if !dirs.contains(&canonical) {
                dirs.push(canonical);
            }
        }
    }
    if unrecorded {
        for dir in local_module_dirs(project_dir) {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    Ok(dirs)
}

/// The folder of each generated extension module under `project_dir`, in the project and its
/// subdirectories and in each package in `node_modules`. A package in `node_modules` may be a
/// symlink, as package managers install them.
pub(crate) fn local_module_dirs(project_dir: &Path) -> Vec<PathBuf> {
    let mut roots = project_dirs(project_dir);
    for package in scanned_children(&project_dir.join("node_modules")) {
        if package
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('@'))
        {
            roots.extend(scanned_children(&package));
        } else {
            roots.push(package);
        }
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in roots.into_iter().map(|root| root.join("generated")) {
        if !dir.join("index.ts").is_file() {
            continue;
        }
        let canonical = dir.canonicalize().unwrap_or(dir);
        if !dirs.contains(&canonical) {
            dirs.push(canonical);
        }
    }
    dirs
}

/// The bundles of the extensions `names` beside the generated modules in `module_dirs`. Another
/// extension's bundle there is never read, so a broken one can't fail these.
pub(crate) fn find_bundles(module_dirs: &[PathBuf], names: &[&str]) -> Result<Vec<FoundBundle>> {
    let mut bundles = Vec::new();
    for module_dir in module_dirs {
        let dir = bundle_dir(module_dir);
        for path in children(&dir)? {
            if !path.file_name().is_some_and(|file| {
                names
                    .iter()
                    .any(|name| file.to_string_lossy() == format!("{name}-core.zip"))
            }) {
                continue;
            }
            let zip = std::fs::read(&path)
                .into_alien_error()
                .context(file_error("read", &path))?;
            let manifest = read_manifest(&zip).map_err(|message| {
                AlienError::new(ErrorData::ReleaseFailed {
                    message: format!("{}: {message}", path.display()),
                })
            })?;
            let workspace_root = match module_dir.parent() {
                Some(root) if super::is_extension_workspace(root)? => Some(root.to_path_buf()),
                _ => None,
            };
            bundles.push(FoundBundle {
                dir: dir.clone(),
                zip,
                manifest,
                workspace_root,
            });
        }
    }
    Ok(bundles)
}

/// The controller binary of extension `name` whose sha256 is `sha256`, in one of the bundles
/// beside the generated modules in `module_dirs`, or else under `project_dir`, for
/// the manager `alien dev` runs: `None` when no bundle there has it, or why the bundles could not
/// be read.
pub(crate) fn local_controller(
    module_dirs: &[PathBuf],
    project_dir: &Path,
    name: &str,
    sha256: &str,
) -> Result<Option<PathBuf>> {
    let find = |dirs: &[PathBuf]| -> Result<Option<PathBuf>> {
        Ok(find_bundles(dirs, &[name])?
            .iter()
            .filter(|bundle| bundle.manifest.name == name)
            .flat_map(|bundle| {
                bundle
                    .manifest
                    .controllers
                    .iter()
                    .filter_map(|(target, binary)| {
                        binary
                            .as_ref()
                            .filter(|binary| binary.sha256.eq_ignore_ascii_case(sha256))
                            .map(|_| bundle.dir.join(format!("{name}-controller-{target}")))
                    })
            })
            .find(|path| path.is_file()))
    };
    // The recorded folders may be an earlier release's, so the project's own are searched too.
    match find(module_dirs)? {
        Some(found) => Ok(Some(found)),
        None => find(&local_module_dirs(project_dir)),
    }
}

/// `project_dir` and the directories under it, at most `SEARCH_DEPTH` levels down, through no
/// symlink and no hidden directory, `target` or `node_modules`.
fn project_dirs(project_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    collect_dirs(project_dir, 0, &mut dirs);
    dirs
}

fn collect_dirs(dir: &Path, depth: usize, dirs: &mut Vec<PathBuf>) {
    dirs.push(dir.to_path_buf());
    if depth == SEARCH_DEPTH {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            warn!(
                "{} can't be read, so no extension under it is found: {error}",
                dir.display()
            );
            return;
        }
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        // `file_type` doesn't follow a symlink, so a link to a directory is not a directory.
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && name != "node_modules" && name != "target"
        })
        .map(|entry| entry.path())
        .collect();
    children.sort();
    for child in children {
        collect_dirs(&child, depth + 1, dirs);
    }
}

/// The entries of `dir`, sorted: none when it doesn't exist, and an error when it can't be read.
fn children(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(Vec::new())
        }
        Err(error) => {
            return Err(error)
                .into_alien_error()
                .context(file_error("read", dir))
        }
    };
    let mut children = Vec::new();
    for entry in entries {
        children.push(
            entry
                .into_alien_error()
                .context(file_error("read", dir))?
                .path(),
        );
    }
    children.sort();
    Ok(children)
}

/// The entries of `dir` for the scan for generated modules, which skips a folder it can't read
/// as the walk of the project does.
fn scanned_children(dir: &Path) -> Vec<PathBuf> {
    children(dir).unwrap_or_else(|error| {
        warn!(
            "{} can't be read, so no extension under it is found: {error}",
            dir.display()
        );
        Vec::new()
    })
}

fn read_manifest(zip: &[u8]) -> std::result::Result<ExtensionBundleManifest, String> {
    let text = zip_entry(zip, alien_core::EXTENSION_BUNDLE_MANIFEST)?;
    serde_json::from_slice(&text).map_err(|error| format!("manifest.json can't be read: {error}"))
}

fn zip_entry(zip: &[u8], name: &str) -> std::result::Result<Vec<u8>, String> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))
        .map_err(|error| format!("not a zip: {error}"))?;
    let mut entry = archive
        .by_name(name)
        .map_err(|_| format!("the bundle has no {name}"))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{name} can't be read: {error}"))?;
    Ok(bytes)
}

/// Finds the bundle of each extension beside the generated modules in `module_dirs`, checks it,
/// and returns the objects the release uploads. A stack without extensions passes none, and
/// nothing here runs for it.
pub(crate) async fn check_listed(
    extensions: &[ExtensionManifest],
    module_dirs: &[PathBuf],
) -> Result<Vec<Upload>> {
    if extensions.is_empty() {
        return Ok(Vec::new());
    }
    let names: Vec<&str> = extensions
        .iter()
        .map(|extension| extension.name.as_str())
        .collect();
    let bundles = find_bundles(module_dirs, &names)?;
    let mut uploads = Vec::new();
    for extension in extensions {
        let bundle = pinned_bundle(&bundles, extension)?;
        check_bundle(extension, bundle)
            .map_err(|message| AlienError::new(ErrorData::ReleaseFailed { message }))?;
        check_shipped(bundle).await?;
        uploads.extend(bundle_uploads(bundle));
    }
    Ok(uploads)
}

/// Checks the modules the bundle ships against its manifest, then runs the importer check
/// `alien extensions test` runs on them: the zip's `hooks.wasm` on the workspace's importer
/// fixtures, and each state it writes loaded into the zip's `controller.wasm`. It runs no
/// build.
async fn check_shipped(bundle: &FoundBundle) -> Result<()> {
    let manifest = &bundle.manifest;
    let name = &manifest.name;
    let failed = |message: String| AlienError::new(ErrorData::ReleaseFailed { message });
    let wasm = zip_entry(&bundle.zip, alien_core::EXTENSION_HOOKS_MODULE).map_err(failed)?;
    let sha256 = hex::encode(Sha256::digest(&wasm));
    if sha256 != manifest.hooks_sha256 {
        return Err(failed(format!(
            "the extension `{name}`'s hooks.wasm is not the module its manifest lists"
        )));
    }
    let controller_module = match &manifest.controller_module_sha256 {
        Some(expected) => {
            let module =
                zip_entry(&bundle.zip, alien_core::EXTENSION_CONTROLLER_MODULE).map_err(failed)?;
            if !hex::encode(Sha256::digest(&module)).eq_ignore_ascii_case(expected) {
                return Err(failed(format!(
                    "the extension `{name}`'s controller.wasm is not the module its manifest lists"
                )));
            }
            Some((module, expected.to_ascii_lowercase()))
        }
        None => {
            // Alien runs a cloud platform's controller in the controller module only.
            let unrunnable =
                manifest
                    .resource_types
                    .iter()
                    .find_map(|(resource_type, descriptor)| {
                        descriptor
                            .controller_platforms
                            .iter()
                            .find(|platform| **platform != Platform::Local)
                            .map(|platform| (resource_type, platform))
                    });
            if let Some((resource_type, platform)) = unrunnable {
                return Err(failed(format!(
                    "the extension `{name}` registers a controller for `{resource_type}` on {platform}, and its bundle has no controller module to run it. Register the controller in the controller crate's library, in the function `export_controllers!` names, and run `alien extensions build`"
                )));
            }
            None
        }
    };
    let Some(root) = &bundle.workspace_root else {
        info!(
            "The extension `{name}` ships no importer fixtures here, so its importers are not run"
        );
        return Ok(());
    };
    let checked = check_shipped_imports(
        root,
        &wasm,
        &sha256,
        controller_module
            .as_ref()
            .map(|(module, sha256)| (module.as_slice(), sha256.as_str())),
    )
    .await
    .context(ErrorData::ReleaseFailed {
        message: format!("the importers the extension `{name}` ships fail the importer check"),
    })?;
    info!(
        "The extension `{name}`'s shipped importers pass on {} fixtures; {} load into its controller module",
        checked.fixtures.len(),
        checked.loaded.len()
    );
    Ok(())
}

/// The bundle whose zip has the digest the stack pins.
fn pinned_bundle<'a>(
    bundles: &'a [FoundBundle],
    extension: &ExtensionManifest,
) -> Result<&'a FoundBundle> {
    if let Some(bundle) = bundles
        .iter()
        .find(|bundle| bundle.digest() == extension.digest)
    {
        return Ok(bundle);
    }
    let same_name: Vec<&FoundBundle> = bundles
        .iter()
        .filter(|bundle| bundle.manifest.name == extension.name)
        .collect();
    let message = match same_name.first() {
        Some(bundle) => format!(
            "the stack pins the extension `{}` at {}, and its bundle in {} is {}: the stack was built from another build of the extension. Run `alien extensions build` in its workspace, then release without `--prebuilt`",
            extension.name,
            extension.digest,
            bundle.dir.display(),
            bundle.digest()
        ),
        None => format!(
            "the stack lists the extension `{}`, and no generated module's folder has its bundle in ../target/alien-extension/. The release looks beside the module alien.ts imports for it; with `--prebuilt` or `alien dev --skip-build`, or for a module that recorded no folder, in `generated/` folders at most {SEARCH_DEPTH} levels below the project and in each package in its node_modules. Run `alien extensions build` in its workspace",
            extension.name
        ),
    };
    Err(AlienError::new(ErrorData::ReleaseFailed { message }))
}

/// The checks on one bundle that need no build.
fn check_bundle(
    extension: &ExtensionManifest,
    bundle: &FoundBundle,
) -> std::result::Result<(), String> {
    let manifest = &bundle.manifest;
    let name = &extension.name;
    check_name(&manifest.name).map_err(|error| error.to_string())?;
    if manifest.name != *name || manifest.version != extension.version {
        return Err(format!(
            "the stack lists the extension `{name}` {}, and its bundle is `{}` {}",
            extension.version, manifest.name, manifest.version
        ));
    }
    if manifest.resource_types != extension.resource_types {
        return Err(format!(
            "the resource types the stack lists for the extension `{name}` differ from its bundle's; the generated module was changed by hand. Run `alien extensions build` again"
        ));
    }
    if manifest.protocol.hooks != HOOKS_PROTOCOL {
        return Err(format!(
            "the extension `{name}`'s hooks module speaks hooks ABI {}, and this Alien runs {HOOKS_PROTOCOL}",
            manifest.protocol.hooks
        ));
    }
    // Equality, while Alien speaks one step protocol version.
    let step_protocol = alien_infra::step_protocol::protocol::PROTOCOL_VERSION;
    match manifest.protocol.controller {
        Some(controller) if controller != step_protocol => {
            return Err(format!(
                "the extension `{name}`'s controller speaks step protocol {controller}, and this Alien speaks {step_protocol}"
            ));
        }
        None if manifest.controllers.values().any(Option::is_some) => {
            return Err(format!(
                "the extension `{name}`'s bundle ships controller binaries, and its manifest names no step protocol for them"
            ));
        }
        _ => {}
    }
    for (target, binary) in &manifest.controllers {
        let Some(binary) = binary else { continue };
        let path = bundle.dir.join(format!("{name}-controller-{target}"));
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("{} can't be read: {error}", path.display()))?;
        if hex::encode(Sha256::digest(&bytes)) != binary.sha256 {
            return Err(format!(
                "{} is not the binary the bundle's manifest lists for {target}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn bundle_uploads(bundle: &FoundBundle) -> Vec<Upload> {
    let name = &bundle.manifest.name;
    let mut uploads = vec![Upload {
        extension: name.clone(),
        object: "core".to_string(),
        path: bundle.dir.join(format!("{name}-core.zip")),
        sha256: hex::encode(Sha256::digest(&bundle.zip)),
        size: bundle.zip.len() as u64,
    }];
    for (target, binary) in &bundle.manifest.controllers {
        if let Some(binary) = binary {
            uploads.push(Upload {
                extension: name.clone(),
                object: target.to_string(),
                path: bundle.dir.join(format!("{name}-controller-{target}")),
                sha256: binary.sha256.clone(),
                size: binary.size,
            });
        }
    }
    uploads
}

fn file_error(operation: &str, path: &Path) -> ErrorData {
    ErrorData::FileOperationFailed {
        operation: operation.to_string(),
        file_path: path.display().to_string(),
        reason: format!("could not {operation} it"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ControllerBinary, ExtensionProtocols, Stack};
    use indexmap::IndexMap;

    fn bundle(
        dir: &Path,
        controllers: IndexMap<BinaryTarget, Option<ControllerBinary>>,
    ) -> FoundBundle {
        FoundBundle {
            dir: dir.to_path_buf(),
            zip: b"zip".to_vec(),
            manifest: ExtensionBundleManifest {
                name: "acme".to_string(),
                version: "0.1.0".to_string(),
                protocol: ExtensionProtocols {
                    hooks: HOOKS_PROTOCOL,
                    controller: Some(alien_infra::step_protocol::protocol::PROTOCOL_VERSION),
                },
                hooks_sha256: String::new(),
                controllers,
                resource_types: IndexMap::new(),
                controller_module_sha256: None,
                egress: Vec::new(),
            },
            workspace_root: None,
        }
    }

    fn listed() -> ExtensionManifest {
        ExtensionManifest {
            name: "acme".to_string(),
            version: "0.1.0".to_string(),
            digest: String::new(),
            resource_types: IndexMap::new(),
            egress: Vec::new(),
        }
    }

    #[test]
    fn refuses_a_binary_the_manifest_does_not_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("acme-controller-linux-x64"), b"binary").unwrap();
        let mut controllers = IndexMap::new();
        for target in BinaryTarget::ALL {
            controllers.insert(*target, None);
        }
        controllers.insert(
            BinaryTarget::LinuxX64,
            Some(ControllerBinary {
                sha256: hex::encode(Sha256::digest(b"binary")),
                size: 6,
            }),
        );
        let bundle = bundle(dir.path(), controllers);
        // The other targets have no binary: a deployment runs the controller module, and only
        // `alien dev` runs a binary, on the machine that built it.
        check_bundle(&listed(), &bundle).unwrap();

        std::fs::write(dir.path().join("acme-controller-linux-x64"), b"changed").unwrap();
        let problem = check_bundle(&listed(), &bundle).unwrap_err();
        assert!(problem.contains("is not the binary"), "{problem}");
    }

    #[test]
    fn refuses_controller_binaries_with_no_step_protocol() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("acme-controller-linux-x64"), b"binary").unwrap();
        let mut controllers = IndexMap::new();
        controllers.insert(
            BinaryTarget::LinuxX64,
            Some(ControllerBinary {
                sha256: hex::encode(Sha256::digest(b"binary")),
                size: 6,
            }),
        );
        let mut bundle = bundle(dir.path(), controllers);
        bundle.manifest.protocol.controller = None;
        let problem = check_bundle(&listed(), &bundle).unwrap_err();
        assert!(problem.contains("names no step protocol"), "{problem}");
    }

    #[test]
    fn refuses_another_protocol_and_a_reserved_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut other = bundle(dir.path(), IndexMap::new());
        other.manifest.protocol.controller = Some(2);
        assert!(check_bundle(&listed(), &other)
            .unwrap_err()
            .contains("step protocol 2"));
        let mut other = bundle(dir.path(), IndexMap::new());
        other.manifest.protocol.hooks = 2;
        assert!(check_bundle(&listed(), &other)
            .unwrap_err()
            .contains("hooks ABI 2"));
        let mut reserved = bundle(dir.path(), IndexMap::new());
        reserved.manifest.name = "alien".to_string();
        let mut listed_reserved = listed();
        listed_reserved.name = "alien".to_string();
        assert!(check_bundle(&listed_reserved, &reserved)
            .unwrap_err()
            .contains("reserved"));
        let mut stem = bundle(dir.path(), IndexMap::new());
        stem.manifest.name = "outputs".to_string();
        let mut listed_stem = listed();
        listed_stem.name = "outputs".to_string();
        assert!(check_bundle(&listed_stem, &stem)
            .unwrap_err()
            .contains("Terraform module"));
    }

    /// The walk for the workspaces `alien release` builds: no symlink, no installed package and
    /// no hidden directory, and each workspace once.
    #[cfg(unix)]
    #[test]
    fn finds_each_extension_workspace_once_through_no_symlink() {
        let project = tempfile::tempdir().unwrap();
        let project = project.path().canonicalize().unwrap();
        let workspace = |dir: &str| {
            std::fs::create_dir_all(project.join(dir)).unwrap();
            std::fs::write(
                project.join(dir).join("Cargo.toml"),
                "[workspace]\n[workspace.metadata.alien-extension]\nhooks = \"hooks\"\n",
            )
            .unwrap();
        };
        workspace("acme");
        workspace("extensions/backups");
        workspace("node_modules/@acme/tags");
        workspace(".cache/old");
        workspace("acme/target/copy");
        std::fs::create_dir_all(project.join("app")).unwrap();
        std::fs::write(
            project.join("app/Cargo.toml"),
            "[package]\nname = \"app\"\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(project.join("acme"), project.join("acme-link")).unwrap();
        // A loop: the walk must not follow it.
        std::os::unix::fs::symlink(&project, project.join("extensions/up")).unwrap();
        // A workspace outside the project, reached only through a link.
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            outside.path().join("Cargo.toml"),
            "[workspace]\n[workspace.metadata.alien-extension]\nhooks = \"hooks\"\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), project.join("extensions/outside")).unwrap();
        assert_eq!(
            extension_workspaces(&project).unwrap(),
            [project.join("acme"), project.join("extensions/backups")]
        );
    }

    /// An extension workspace whose `cargo metadata` fails.
    fn broken_workspace(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"hooks\"]\n\n[workspace.metadata.alien-extension]\nhooks = \"hooks\"\n",
        )
        .unwrap();
    }

    /// A stack without extensions evaluates once and builds nothing, whatever the project holds.
    #[tokio::test]
    async fn a_stack_without_extensions_builds_no_workspace() {
        let project = tempfile::tempdir().unwrap();
        broken_workspace(&project.path().join("tools/wip-ext"));
        let evaluated = std::sync::atomic::AtomicUsize::new(0);
        let configuration = evaluate_building_extensions(project.path(), &[], || async {
            evaluated.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(loaded(Stack::new("my-app".to_string()).build()))
        })
        .await
        .unwrap();
        assert!(configuration.stack.extensions.is_empty());
        assert_eq!(evaluated.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// When alien.ts doesn't evaluate, each workspace is built first. A failed build is a
    /// warning, reported with the second evaluation's error.
    #[tokio::test]
    async fn an_alien_ts_that_still_fails_reports_the_failed_builds() {
        let project = tempfile::tempdir().unwrap();
        let evaluated = std::sync::atomic::AtomicUsize::new(0);
        let evaluate = || async {
            evaluated.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err::<LoadedConfiguration, _>(AlienError::new(ErrorData::ConfigurationError {
                message: "Cannot find module './acme/generated'".to_string(),
            }))
        };
        // No workspace: the error as it is, after one evaluation.
        let error = evaluate_building_extensions(project.path(), &[], evaluate)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Cannot find module"), "{error}");
        assert_eq!(evaluated.load(std::sync::atomic::Ordering::SeqCst), 1);

        broken_workspace(&project.path().join("acme"));
        let error = evaluate_building_extensions(project.path(), &[], evaluate)
            .await
            .unwrap_err();
        assert_eq!(evaluated.load(std::sync::atomic::Ordering::SeqCst), 3);
        let report = format!("{error:?}");
        assert!(report.contains("these builds failed"), "{report}");
        assert!(report.contains("acme did not build"), "{report}");
        assert!(report.contains("Cannot find module"), "{report}");
    }

    /// A project whose extension workspaces can't be listed still reports why alien.ts didn't
    /// evaluate.
    #[tokio::test]
    async fn an_unlisted_workspace_keeps_the_alien_ts_error() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("acme")).unwrap();
        std::fs::write(project.path().join("acme/Cargo.toml"), "[workspace\n").unwrap();

        let error = evaluate_building_extensions(project.path(), &[], || async {
            Err::<LoadedConfiguration, _>(AlienError::new(ErrorData::ConfigurationError {
                message: "Cannot find module './acme/generated'".to_string(),
            }))
        })
        .await
        .unwrap_err();

        let report = format!("{error:?}");
        assert!(report.contains("Cannot find module"), "{report}");
        assert!(
            report.contains("extension workspaces could not be listed"),
            "{report}"
        );
    }

    /// The module `alien extensions build` generates for `name`, as far as the release reads it.
    fn generated_module(root: &Path, name: &str) {
        std::fs::create_dir_all(root.join("generated")).unwrap();
        std::fs::write(
            root.join("generated/index.ts"),
            format!("const extension: ExtensionManifest = {{ \"name\": \"{name}\" }}\n\nexport default extension\n"),
        )
        .unwrap();
    }

    /// `stack`, as alien.ts that imports no generated module loads.
    fn loaded(stack: Stack) -> LoadedConfiguration {
        LoadedConfiguration {
            stack,
            config_file: PathBuf::from("alien.ts"),
            extension_modules: Vec::new(),
        }
    }

    fn listing(names: &[&str]) -> Stack {
        let mut stack = Stack::new("my-app".to_string()).build();
        for name in names {
            stack.extensions.push(ExtensionManifest {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                digest: String::new(),
                resource_types: IndexMap::new(),
                egress: Vec::new(),
            });
        }
        stack
    }

    /// A listed extension whose source no longer builds fails the release, even when alien.ts
    /// evaluated only after the fallback built another workspace: its last good build doesn't
    /// ship.
    #[tokio::test]
    async fn a_listed_extension_that_does_not_build_fails_after_the_fallback() {
        let project = tempfile::tempdir().unwrap();
        let budget = project.path().join("budget");
        broken_workspace(&budget);
        generated_module(&budget, "budget");
        broken_workspace(&project.path().join("tags"));
        let evaluated = std::sync::atomic::AtomicUsize::new(0);
        let error = evaluate_building_extensions(project.path(), &[], || async {
            match evaluated.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                0 => Err(AlienError::new(ErrorData::ConfigurationError {
                    message: "Cannot find module './tags/generated'".to_string(),
                })),
                _ => Ok(loaded(listing(&["budget"]))),
            }
        })
        .await
        .unwrap_err();
        let report = format!("{error:?}");
        assert!(report.contains("budget did not build"), "{report}");
        assert_eq!(evaluated.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// The fallback builds only workspaces whose `generated/` module doesn't exist yet, so an
    /// alien.ts that fails for another reason builds nothing.
    #[tokio::test]
    async fn the_fallback_builds_only_workspaces_with_no_generated_module() {
        let project = tempfile::tempdir().unwrap();
        let budget = project.path().join("budget");
        broken_workspace(&budget);
        generated_module(&budget, "budget");
        let evaluated = std::sync::atomic::AtomicUsize::new(0);
        let error = evaluate_building_extensions(project.path(), &[], || async {
            evaluated.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err::<LoadedConfiguration, _>(AlienError::new(ErrorData::ConfigurationError {
                message: "alien.ts: Unexpected token".to_string(),
            }))
        })
        .await
        .unwrap_err();
        let report = format!("{error:?}");
        assert!(report.contains("Unexpected token"), "{report}");
        assert!(!report.contains("did not build"), "{report}");
        assert_eq!(evaluated.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// Only the listed extensions' workspaces build. An installed package in node_modules ships
    /// its bundle and never builds; a workspace linked into node_modules resolves to its own
    /// folder and does.
    #[cfg(unix)]
    #[test]
    fn builds_the_listed_workspaces_and_no_installed_package() {
        let project = tempfile::tempdir().unwrap();
        let project = project.path().canonicalize().unwrap();
        for (dir, name) in [
            ("node_modules/@acme/vendored", "vendored"),
            ("tags", "tags"),
        ] {
            broken_workspace(&project.join(dir));
            generated_module(&project.join(dir), name);
        }
        // A workspace outside the project, which the package manager links into node_modules.
        let outside = tempfile::tempdir().unwrap();
        let linked = outside.path().canonicalize().unwrap().join("linked");
        broken_workspace(&linked);
        generated_module(&linked, "linked");
        std::os::unix::fs::symlink(&linked, project.join("node_modules/linked")).unwrap();
        assert_eq!(
            listed_workspaces(&loaded(listing(&["vendored", "linked"])), &project).unwrap(),
            [linked]
        );
    }

    /// The node_modules rule reads the path below the project root, so a project that sits in a
    /// folder named node_modules still builds its own extensions.
    #[test]
    fn a_project_inside_a_node_modules_folder_builds_its_workspaces() {
        let parent = tempfile::tempdir().unwrap();
        let project = parent
            .path()
            .canonicalize()
            .unwrap()
            .join("node_modules/app");
        broken_workspace(&project.join("acme"));
        generated_module(&project.join("acme"), "acme");
        assert_eq!(
            listed_workspaces(&loaded(listing(&["acme"])), &project).unwrap(),
            [project.join("acme")]
        );
    }

    /// alien.ts may import an extension from outside the project, which builds where the module
    /// alien.ts imported sits.
    #[tokio::test]
    async fn an_extension_imported_from_beside_the_project_builds_there() {
        let parent = tempfile::tempdir().unwrap();
        let parent = parent.path().canonicalize().unwrap();
        let project = parent.join("app");
        std::fs::create_dir_all(&project).unwrap();
        let acme = parent.join("acme");
        broken_workspace(&acme);
        generated_module(&acme, "acme");
        let url = url::Url::from_file_path(acme.join("generated/index.ts"))
            .unwrap()
            .to_string();
        let error = evaluate_building_extensions(&project, &[], || async {
            Ok(LoadedConfiguration {
                stack: listing(&["acme"]),
                config_file: project.join("alien.ts"),
                extension_modules: vec![crate::config::ExtensionModule {
                    name: "acme".to_string(),
                    url: url.clone(),
                }],
            })
        })
        .await
        .unwrap_err();
        let report = format!("{error:?}");
        assert!(report.contains("acme did not build"), "{report}");
    }

    /// The recorded folders are an earlier release's and hold no such bundle: the lookup still
    /// finds the controller under the project.
    #[test]
    fn finds_a_controller_in_the_project_when_the_recorded_folders_miss() {
        let project = tempfile::tempdir().unwrap();
        let project = project.path().canonicalize().unwrap();
        generated_module(&project.join("acme"), "acme");
        let dir = project.join("acme/target/alien-extension");
        std::fs::create_dir_all(&dir).unwrap();
        let sha256 = hex::encode(Sha256::digest(b"binary"));
        let mut controllers = IndexMap::new();
        controllers.insert(
            BinaryTarget::LinuxX64,
            Some(ControllerBinary {
                sha256: sha256.clone(),
                size: 6,
            }),
        );
        let manifest = bundle(&dir, controllers).manifest;
        std::fs::write(
            dir.join("acme-core.zip"),
            zip_of(&[(
                alien_core::EXTENSION_BUNDLE_MANIFEST,
                &serde_json::to_vec(&manifest).unwrap(),
            )]),
        )
        .unwrap();
        std::fs::write(dir.join("acme-controller-linux-x64"), b"binary").unwrap();
        let earlier = tempfile::tempdir().unwrap();

        assert_eq!(
            local_controller(&[earlier.path().to_path_buf()], &project, "acme", &sha256).unwrap(),
            Some(dir.join("acme-controller-linux-x64"))
        );
    }

    /// `alien dev`'s manager finds a controller in the bundle beside a module alien.ts imports
    /// from outside the project, where the scan of the project never looks.
    #[test]
    fn finds_a_controller_beside_a_module_imported_from_outside_the_project() {
        let parent = tempfile::tempdir().unwrap();
        let parent = parent.path().canonicalize().unwrap();
        let project = parent.join("app");
        std::fs::create_dir_all(&project).unwrap();
        generated_module(&parent.join("acme"), "acme");
        let dir = parent.join("acme/target/alien-extension");
        std::fs::create_dir_all(&dir).unwrap();
        let sha256 = hex::encode(Sha256::digest(b"binary"));
        let mut controllers = IndexMap::new();
        controllers.insert(
            BinaryTarget::LinuxX64,
            Some(ControllerBinary {
                sha256: sha256.clone(),
                size: 6,
            }),
        );
        let manifest = bundle(&dir, controllers).manifest;
        std::fs::write(
            dir.join("acme-core.zip"),
            zip_of(&[(
                alien_core::EXTENSION_BUNDLE_MANIFEST,
                &serde_json::to_vec(&manifest).unwrap(),
            )]),
        )
        .unwrap();
        std::fs::write(dir.join("acme-controller-linux-x64"), b"binary").unwrap();
        let module_dirs = imported_module_dirs(
            &LoadedConfiguration {
                stack: listing(&["acme"]),
                config_file: project.join("alien.ts"),
                extension_modules: vec![crate::config::ExtensionModule {
                    name: "acme".to_string(),
                    url: url::Url::from_file_path(parent.join("acme/generated/index.ts"))
                        .unwrap()
                        .to_string(),
                }],
            },
            &project,
        )
        .unwrap();

        assert_eq!(
            local_controller(&module_dirs, &project, "acme", &sha256).unwrap(),
            Some(dir.join("acme-controller-linux-x64"))
        );
        assert_eq!(
            local_controller(&[], &project, "acme", &sha256).unwrap(),
            None
        );
    }

    /// A module alien.ts loaded from something other than a file names no folder to build, so the
    /// release says so rather than look for the extension elsewhere.
    #[test]
    fn a_module_loaded_from_a_url_that_is_not_a_file_is_refused() {
        let project = tempfile::tempdir().unwrap();
        let error = imported_module_dirs(
            &LoadedConfiguration {
                stack: listing(&["acme"]),
                config_file: project.path().join("alien.ts"),
                extension_modules: vec![crate::config::ExtensionModule {
                    name: "acme".to_string(),
                    url: "http://localhost/acme/generated/index.ts".to_string(),
                }],
            },
            project.path(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("which is not a file"), "{error}");
    }

    /// Another extension's bundle beside the modules is never read, so a broken one can't fail
    /// the bundles the stack lists.
    #[test]
    fn a_bundle_the_stack_does_not_list_is_not_read() {
        let project = tempfile::tempdir().unwrap();
        let project = project.path().canonicalize().unwrap();
        generated_module(&project.join("tags"), "tags");
        let dir = project.join("tags/target/alien-extension");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tags-core.zip"), "not a zip").unwrap();
        let module_dirs = local_module_dirs(&project);
        assert_eq!(module_dirs, [project.join("tags/generated")]);
        assert!(find_bundles(&module_dirs, &["acme"]).unwrap().is_empty());
    }

    #[test]
    fn an_unreadable_generated_module_is_an_error() {
        let project = tempfile::tempdir().unwrap();
        let budget = project.path().join("budget");
        broken_workspace(&budget);
        std::fs::create_dir_all(budget.join("generated")).unwrap();
        std::fs::write(budget.join("generated/index.ts"), "export {}\n").unwrap();
        let error = listed_workspaces(&loaded(listing(&["budget"])), project.path()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("declares no `extension` manifest"),
            "{error}"
        );
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// An installed package's bundle has no importer fixtures here, and its hooks.wasm is still
    /// the one its manifest lists.
    #[tokio::test]
    async fn the_shipped_module_is_checked_without_a_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let mut installed = bundle(dir.path(), IndexMap::new());
        installed.zip = zip_of(&[(alien_core::EXTENSION_HOOKS_MODULE, b"module")]);
        installed.manifest.hooks_sha256 = hex::encode(Sha256::digest(b"module"));
        check_shipped(&installed).await.unwrap();
        installed.manifest.hooks_sha256 = hex::encode(Sha256::digest(b"another module"));
        let error = check_shipped(&installed).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("hooks.wasm is not the module its manifest lists"),
            "{error}"
        );
    }

    /// The controller module a bundle ships is the one its manifest lists, with or without the
    /// extension's workspace at hand, or the release is refused before the module runs.
    #[tokio::test]
    async fn refuses_a_controller_module_the_manifest_does_not_list() {
        let dir = tempfile::tempdir().unwrap();
        let mut installed = bundle(dir.path(), IndexMap::new());
        installed.manifest.hooks_sha256 = hex::encode(Sha256::digest(b"module"));
        installed.manifest.controller_module_sha256 =
            Some(hex::encode(Sha256::digest(b"controller")));
        installed.zip = zip_of(&[
            (alien_core::EXTENSION_HOOKS_MODULE, b"module"),
            (alien_core::EXTENSION_CONTROLLER_MODULE, b"controller"),
        ]);
        check_shipped(&installed).await.unwrap();

        installed.zip = zip_of(&[
            (alien_core::EXTENSION_HOOKS_MODULE, b"module"),
            (
                alien_core::EXTENSION_CONTROLLER_MODULE,
                b"another controller",
            ),
        ]);
        let error = check_shipped(&installed).await.unwrap_err();
        assert!(
            error.to_string().contains(
                "the extension `acme`'s controller.wasm is not the module its manifest lists"
            ),
            "{error}"
        );

        installed.zip = zip_of(&[(alien_core::EXTENSION_HOOKS_MODULE, b"module")]);
        let error = check_shipped(&installed).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("the bundle has no controller.wasm"),
            "{error}"
        );
    }

    /// A bundle whose manifest registers a cloud platform's controller and ships no controller
    /// module has nothing to run it with: Alien runs a binary on the local platform only.
    #[tokio::test]
    async fn refuses_a_cloud_controller_with_no_controller_module() {
        let dir = tempfile::tempdir().unwrap();
        let mut bundle = bundle(dir.path(), IndexMap::new());
        bundle.manifest.hooks_sha256 = hex::encode(Sha256::digest(b"module"));
        bundle.zip = zip_of(&[(alien_core::EXTENSION_HOOKS_MODULE, b"module")]);
        let descriptor = |platforms: serde_json::Value| -> alien_core::ResourceTypeDescriptor {
            serde_json::from_value(serde_json::json!({
                "lifecycles": ["live"],
                "holdsData": false,
                "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
                "controllerPlatforms": platforms,
                "permissionSets": [],
                "link": { "permissionSets": [] },
                "schemas": { "config": {}, "outputs": {} }
            }))
            .unwrap()
        };
        bundle.manifest.resource_types.insert(
            alien_core::ResourceType::from("acme/dns-zone"),
            descriptor(serde_json::json!(["local"])),
        );
        check_shipped(&bundle).await.unwrap();

        bundle.manifest.resource_types.insert(
            alien_core::ResourceType::from("acme/dns-zone"),
            descriptor(serde_json::json!(["aws", "local"])),
        );
        let error = check_shipped(&bundle).await.unwrap_err();
        assert!(
            error.to_string().contains(
                "registers a controller for `acme/dns-zone` on aws, and its bundle has no controller module"
            ),
            "{error}"
        );
    }

    #[test]
    fn finds_the_bundle_the_stack_pins_and_names_a_stale_one() {
        let project = tempfile::tempdir().unwrap();
        let project = project.path().canonicalize().unwrap();
        let dir = project.join("acme/target/alien-extension");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = bundle(&dir, IndexMap::new()).manifest;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            alien_core::EXTENSION_BUNDLE_MANIFEST,
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zip, &serde_json::to_vec(&manifest).unwrap()).unwrap();
        let zip = zip.finish().unwrap().into_inner();
        std::fs::write(dir.join("acme-core.zip"), &zip).unwrap();

        std::fs::create_dir_all(project.join("acme/generated")).unwrap();
        std::fs::write(project.join("acme/generated/index.ts"), "").unwrap();
        let module_dirs = local_module_dirs(&project);
        assert_eq!(module_dirs, [project.join("acme/generated")]);
        let bundles = find_bundles(&module_dirs, &["acme"]).unwrap();
        assert_eq!(bundles.len(), 1);
        let mut pinned = listed();
        pinned.digest = format!("sha256:{}", hex::encode(Sha256::digest(&zip)));
        assert_eq!(pinned_bundle(&bundles, &pinned).unwrap().dir, dir);
        pinned.digest = "sha256:00".to_string();
        let error = pinned_bundle(&bundles, &pinned).unwrap_err();
        assert!(
            error.to_string().contains("another build of the extension"),
            "{error}"
        );
    }
}
