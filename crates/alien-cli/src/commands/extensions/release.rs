//! What `alien release` does with the extensions a stack lists.
//!
//! alien.ts is evaluated first. The workspace of each extension it lists is built, and alien.ts
//! is evaluated again for the digests the builds wrote. When alien.ts doesn't evaluate, as in a
//! fresh checkout, each extension workspace with no `generated/` module yet is built before it
//! is evaluated again. A stack without extensions builds nothing.
//!
//! Every bundle the built stack pins is then found by that digest and checked: its manifest, its
//! protocol versions, its controller binaries and targets, its `controller-url`, and, where its
//! workspace's importer fixtures are at hand, the importers of the module it ships against the
//! controller it ships. The release then has a list of objects to upload. None of this runs
//! cargo for `--prebuilt`.
//!
//! Each extension's bundle is found from the folder its generated module sits in, the
//! workspace's `generated/`: `alien extensions build` writes the bundle to
//! `../target/alien-extension/` from there, in a workspace or in the npm package that ships it.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

use alien_core::{BinaryTarget, ExtensionBundleManifest, ExtensionManifest, Platform, Stack};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::build::{build, check_name, HOOKS_PROTOCOL};
use super::read_workspace;
use super::test::check_shipped_imports;
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

/// What the release checks a stack's extensions with.
pub(crate) struct ReleaseChecks<'a> {
    /// The platforms the release ships the stack for.
    pub platforms: &'a [Platform],
    /// Releases a bundle that lacks a controller target the setup CLI package needs, whose
    /// direct setup on that machine then fails naming the target. With no controller for this
    /// machine, the importers' states are then not loaded into the controller.
    pub allow_missing_controller_targets: bool,
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
pub(crate) fn extension_workspaces(project_dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in project_dirs(project_dir) {
        if !super::is_extension_workspace(&dir) {
            continue;
        }
        let canonical = dir.canonicalize().unwrap_or(dir);
        if !found.contains(&canonical) {
            found.push(canonical);
        }
    }
    found
}

/// The stack alien.ts describes, for a release, with the extensions it lists built first.
///
/// `evaluate` runs alien.ts. A stack that lists no extension is returned as is, and nothing
/// builds, whatever the project holds. For one that does, the workspace in the project of each
/// extension it lists is built, and alien.ts is evaluated again for the digests the builds
/// wrote; an extension with no workspace here, from an installed package, ships the bundle it
/// came with. A listed extension that doesn't build fails the release. Each controller builds
/// for `targets`, or for every target this machine can build when it is empty.
///
/// When alien.ts doesn't evaluate, as in a fresh checkout, each extension workspace with no
/// `generated/` module yet is built first, since alien.ts may import that module. A build that
/// fails there is a warning, reported with the error if alien.ts still doesn't evaluate. Once it
/// evaluates, the extensions it lists build as above.
pub(crate) async fn evaluate_building_extensions<F, Fut>(
    project_dir: &Path,
    targets: &[BinaryTarget],
    evaluate: F,
) -> Result<Stack>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Stack>>,
{
    let (stack, built) = match evaluate().await {
        Ok(stack) => return build_listed(stack, project_dir, targets, &[], &evaluate).await,
        Err(error) => {
            let roots: Vec<PathBuf> = extension_workspaces(project_dir)
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
                Ok(stack) => (stack, built),
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
    build_listed(stack, project_dir, targets, &built, &evaluate).await
}

/// Builds the workspace of each extension `stack` lists, except those in `built`, and evaluates
/// alien.ts again when any built.
async fn build_listed<F, Fut>(
    stack: Stack,
    project_dir: &Path,
    targets: &[BinaryTarget],
    built: &[PathBuf],
    evaluate: &F,
) -> Result<Stack>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Stack>>,
{
    if stack.extensions.is_empty() {
        return Ok(stack);
    }
    let roots: Vec<PathBuf> = listed_workspaces(&stack, project_dir)?
        .into_iter()
        .filter(|root| !built.contains(root))
        .collect();
    if roots.is_empty() {
        return Ok(stack);
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

/// The workspace in the project of each extension `stack` lists: the extension workspace whose
/// `generated/` module, the one alien.ts imports, names it. A module in an installed package,
/// whose path runs through `node_modules`, ships its bundle and is never built; a workspace a
/// package manager links into `node_modules` resolves to its own folder and is.
fn listed_workspaces(stack: &Stack, project_dir: &Path) -> Result<Vec<PathBuf>> {
    let project = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());
    let mut roots: Vec<PathBuf> = Vec::new();
    for module_dir in local_module_dirs(project_dir) {
        // Below the project root, so a project inside a folder named node_modules still builds.
        if module_dir
            .strip_prefix(&project)
            .unwrap_or(&module_dir)
            .components()
            .any(|component| component.as_os_str() == "node_modules")
        {
            continue;
        }
        let Some(root) = module_dir
            .parent()
            .filter(|root| super::is_extension_workspace(root))
        else {
            continue;
        };
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

/// The folder of each generated extension module under `project_dir`, in the project and its
/// subdirectories and in each package in `node_modules`. It stands in for the folders the
/// modules register when the stack is evaluated, which this release does not read yet. A package in `node_modules` may be a symlink, as package managers install them.
pub(crate) fn local_module_dirs(project_dir: &Path) -> Vec<PathBuf> {
    let mut roots = project_dirs(project_dir);
    for package in children(&project_dir.join("node_modules")) {
        if package
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('@'))
        {
            roots.extend(children(&package));
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

/// The bundles beside the generated modules in `module_dirs`.
pub(crate) fn find_bundles(module_dirs: &[PathBuf]) -> Result<Vec<FoundBundle>> {
    let mut bundles = Vec::new();
    for module_dir in module_dirs {
        let dir = bundle_dir(module_dir);
        for path in children(&dir) {
            if !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("-core.zip"))
            {
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
            let workspace_root = module_dir
                .parent()
                .filter(|root| super::is_extension_workspace(root))
                .map(Path::to_path_buf);
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
/// beside the generated modules under `project_dir`, for the manager `alien dev` runs: `None`
/// when no bundle there has it, or why the bundles could not be read.
pub(crate) fn local_controller(
    project_dir: &Path,
    name: &str,
    sha256: &str,
) -> Result<Option<PathBuf>> {
    let bundles = find_bundles(&local_module_dirs(project_dir))?;
    Ok(bundles
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

fn children(dir: &Path) -> Vec<PathBuf> {
    let mut children: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    children.sort();
    children
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
    checks: &ReleaseChecks<'_>,
) -> Result<Vec<Upload>> {
    if extensions.is_empty() {
        return Ok(Vec::new());
    }
    let bundles = find_bundles(module_dirs)?;
    let mut uploads = Vec::new();
    for extension in extensions {
        let bundle = pinned_bundle(&bundles, extension)?;
        check_bundle(extension, bundle, checks)
            .map_err(|message| AlienError::new(ErrorData::ReleaseFailed { message }))?;
        check_shipped(bundle, checks).await?;
        uploads.extend(bundle_uploads(bundle));
    }
    Ok(uploads)
}

/// Runs the importer check `alien extensions test` runs on what the bundle ships: the zip's
/// `hooks.wasm` on the workspace's importer fixtures, and each state it writes loaded into the
/// bundled controller for this machine's target. It runs no build.
async fn check_shipped(bundle: &FoundBundle, checks: &ReleaseChecks<'_>) -> Result<()> {
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
    let Some(root) = &bundle.workspace_root else {
        info!(
            "The extension `{name}` ships no importer fixtures here, so its importers are not run"
        );
        return Ok(());
    };
    let host = BinaryTarget::current_os();
    let controller = match (manifest.protocol.controller, manifest.controllers.get(&host)) {
        (None, _) => None,
        (Some(_), Some(Some(_))) => Some(bundle.dir.join(format!("{name}-controller-{host}"))),
        (Some(_), _) if checks.allow_missing_controller_targets => {
            warn!(
                "The extension `{name}` has no controller for {host}, this machine's target, so the states its importers write are not loaded into its controller"
            );
            None
        }
        (Some(_), _) => {
            return Err(failed(format!(
                "the extension `{name}` has no controller for {host}, this machine's target, so the states its importers write can't be loaded into the controller it ships. Build it with `alien extensions build --target {host}`, or release with `--allow-missing-controller-targets`"
            )))
        }
    };
    let registered: IndexMap<String, Vec<Platform>> = manifest
        .resource_types
        .iter()
        .map(|(resource_type, descriptor)| {
            (
                resource_type.to_string(),
                descriptor.controller_platforms.clone(),
            )
        })
        .collect();
    let checked = check_shipped_imports(
        root,
        &wasm,
        &sha256,
        controller.as_deref().map(|binary| (binary, &registered)),
    )
    .await
    .context(ErrorData::ReleaseFailed {
        message: format!("the importers the extension `{name}` ships fail the importer check"),
    })?;
    info!(
        "The extension `{name}`'s shipped importers pass on {} fixtures; {} load into its controller",
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
            "the stack lists the extension `{}`, and no generated module's folder has its bundle in ../target/alien-extension/. The release looks in `generated/` folders at most {SEARCH_DEPTH} levels below the project, and in each package in its node_modules. Run `alien extensions build` in its workspace",
            extension.name
        ),
    };
    Err(AlienError::new(ErrorData::ReleaseFailed { message }))
}

/// The checks on one bundle that need no build.
fn check_bundle(
    extension: &ExtensionManifest,
    bundle: &FoundBundle,
    checks: &ReleaseChecks<'_>,
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
    if let Some(url) = &manifest.controller_url {
        check_controller_url(url).map_err(|problem| {
            format!("the extension `{name}`'s controller-url `{url}` {problem}")
        })?;
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
    if !manifest.controllers.is_empty() && !checks.allow_missing_controller_targets {
        let missing: Vec<String> = cli_package_targets(checks.platforms)
            .into_iter()
            .filter(|target| !matches!(manifest.controllers.get(target), Some(Some(_))))
            .map(|target| target.to_string())
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "the extension `{name}` has no controller for {}, which the stack's setup CLI package ships. Build them with `alien extensions build --target {}`, or release with `--allow-missing-controller-targets`, and direct setup on those machines fails naming the target",
                missing.join(", "),
                missing.join(",")
            ));
        }
    }
    Ok(())
}

/// The targets the setup CLI package builds for a stack released for `platforms`: the two Linux
/// ones when the stack runs on Linux servers and on no cloud or cluster, and all four otherwise. A project's
/// `cli.binaryTargets` setting overrides them there, which this release can't read.
fn cli_package_targets(platforms: &[Platform]) -> Vec<BinaryTarget> {
    let machines_only = platforms.contains(&Platform::Machines)
        && !platforms.iter().any(|platform| {
            matches!(
                platform,
                Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Kubernetes
            )
        });
    if machines_only {
        BinaryTarget::LINUX.to_vec()
    } else {
        BinaryTarget::ALL.to_vec()
    }
}

/// Refuses a `controller-url` a push deployment's manager must not call: anything but `https`,
/// a single-label host, a host name a private network uses, and an address in any IANA
/// special-purpose range. It reads the URL only; a name that resolves to such an address is
/// refused when the manager calls it.
fn check_controller_url(url: &str) -> std::result::Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("is not a URL: {error}"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "uses `{}`; a controller-url must be https",
            parsed.scheme()
        ));
    }
    let special = match parsed.host() {
        None => return Err("has no host".to_string()),
        Some(url::Host::Ipv4(ip)) => is_special_v4(ip),
        Some(url::Host::Ipv6(ip)) => is_special_v6(ip),
        Some(url::Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            if !domain.contains('.') {
                return Err(
                    "has a single-label host, which only a private network resolves".to_string(),
                );
            }
            [".localhost", ".local", ".internal", ".lan", ".home.arpa"]
                .iter()
                .any(|suffix| domain.ends_with(suffix))
        }
    };
    if special {
        return Err("points at a private or special-purpose address".to_string());
    }
    Ok(())
}

/// Whether `ip` is in an IPv4 special-purpose range (IANA's registry), so no public controller
/// listens there.
fn is_special_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    a == 0 // 0.0.0.0/8, this network
        || a == 10 // private
        || a == 127 // loopback
        || (a == 100 && (64..128).contains(&b)) // 100.64.0.0/10, carrier-grade NAT
        || (a == 169 && b == 254) // link-local
        || (a == 172 && (16..32).contains(&b)) // private
        || (a == 192 && b == 0 && c == 0) // 192.0.0.0/24, IETF protocol assignments
        || (a == 192 && b == 0 && c == 2) // 192.0.2.0/24, documentation
        || (a == 192 && b == 88 && c == 99) // 192.88.99.0/24, 6to4 relay anycast
        || (a == 192 && b == 168) // private
        || (a == 198 && (b == 18 || b == 19)) // 198.18.0.0/15, benchmarking
        || (a == 198 && b == 51 && c == 100) // 198.51.100.0/24, documentation
        || (a == 203 && b == 0 && c == 113) // 203.0.113.0/24, documentation
        || a >= 224 // 224.0.0.0/4 multicast, 240.0.0.0/4 reserved, and broadcast
}

/// Whether `ip` is in an IPv6 special-purpose range, or embeds an IPv4 address that is.
fn is_special_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    // `::a.b.c.d` (compatible) and `::ffff:a.b.c.d` (mapped), which include `::` and `::1`.
    if segments[..5] == [0; 5] && (segments[5] == 0 || segments[5] == 0xffff) {
        return true;
    }
    match segments[0] {
        // 64:ff9b::/96 and 64:ff9b:1::/48, NAT64
        0x0064 => segments[1] == 0xff9b,
        // 100::/64, discard-only
        0x0100 => segments[1..4] == [0; 3],
        // 2001::/23, IETF protocol assignments (Teredo, benchmarking, ORCHID), and
        // 2001:db8::/32, documentation
        0x2001 => segments[1] < 0x0200 || segments[1] == 0x0db8,
        // 2002::/16, 6to4
        0x2002 => true,
        // 3fff::/20, documentation
        0x3fff => segments[1] < 0x1000,
        // fc00::/7 unique local; fe80::/10 link-local, fec0::/10 site-local and ff00::/8
        // multicast, which are everything from fe80 up
        first => (first & 0xfe00) == 0xfc00 || first >= 0xfe80,
    }
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
    use alien_core::{ControllerBinary, ExtensionProtocols};
    use indexmap::IndexMap;

    /// Every IANA special-purpose range, the integer and octal forms of an address, and the host
    /// names only a private network resolves.
    #[test]
    fn refuses_a_controller_url_a_manager_must_not_call() {
        for url in [
            "http://controllers.acme.dev",
            "HTTP://controllers.acme.dev",
            "https://localhost:8443",
            "https://LOCALHOST.",
            "https://intranet",
            "https://controllers.local",
            "https://foo.localhost",
            "https://foo.internal",
            "https://metadata.google.internal",
            "https://10.0.0.5",
            "HTTPS://10.0.0.1",
            "https://user:pw@10.0.0.1",
            "https://172.16.3.4",
            "https://192.168.1.1",
            "https://127.0.0.1",
            "https://2130706433",
            "https://0x7f.1",
            "https://0177.0.0.1",
            "https://127.1",
            "https://169.254.169.254",
            "https://100.64.0.1",
            "https://0.0.0.0",
            "https://0.1.2.3",
            "https://192.0.0.8",
            "https://192.0.2.1",
            "https://198.18.0.1",
            "https://198.51.100.7",
            "https://203.0.113.10",
            "https://224.0.0.1",
            "https://240.0.0.1",
            "https://255.255.255.255",
            "https://[::]",
            "https://[::1]",
            "https://[::127.0.0.1]",
            "https://[::ffff:10.0.0.1]",
            "https://[64:ff9b::a00:1]",
            "https://[100::1]",
            "https://[2001::1]",
            "https://[2001:db8::1]",
            "https://[2002:a00:1::]",
            "https://[3fff::1]",
            "https://[fd00::1]",
            "https://[fe80::1]",
            "https://[fec0::1]",
            "https://[ff02::1]",
            "not a url",
        ] {
            assert!(check_controller_url(url).is_err(), "{url}");
        }
        for url in [
            "https://controllers.acme.dev",
            "https://198.41.0.4:8443/step",
            "https://[2606:4700::1111]",
        ] {
            check_controller_url(url).unwrap();
        }
    }

    /// Only a stack on Linux servers and on no cloud or cluster gets the Linux targets alone; `test` and `local` don't change it.
    #[test]
    fn a_linux_servers_only_stack_needs_only_the_linux_controllers() {
        for platforms in [
            &[Platform::Machines][..],
            &[Platform::Machines, Platform::Test],
            &[Platform::Machines, Platform::Local],
        ] {
            assert_eq!(
                cli_package_targets(platforms),
                BinaryTarget::LINUX.to_vec(),
                "{platforms:?}"
            );
        }
        for platforms in [
            &[Platform::Aws, Platform::Machines][..],
            &[Platform::Kubernetes],
            &[Platform::Kubernetes, Platform::Machines],
            &[Platform::Local],
            &[],
        ] {
            assert_eq!(
                cli_package_targets(platforms),
                BinaryTarget::ALL.to_vec(),
                "{platforms:?}"
            );
        }
    }

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
                controller_url: Some("https://controllers.acme.dev".to_string()),
                hooks_sha256: String::new(),
                controllers,
                resource_types: IndexMap::new(),
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
        }
    }

    #[test]
    fn refuses_a_missing_controller_target_unless_allowed() {
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
        let strict = ReleaseChecks {
            platforms: &[Platform::Aws],
            allow_missing_controller_targets: false,
        };
        let problem = check_bundle(&listed(), &bundle, &strict).unwrap_err();
        assert!(
            problem.contains("windows-x64, linux-arm64, darwin-arm64"),
            "{problem}"
        );
        let allowed = ReleaseChecks {
            allow_missing_controller_targets: true,
            ..strict
        };
        check_bundle(&listed(), &bundle, &allowed).unwrap();

        // A binary that isn't the one the manifest lists.
        std::fs::write(dir.path().join("acme-controller-linux-x64"), b"changed").unwrap();
        let problem = check_bundle(&listed(), &bundle, &allowed).unwrap_err();
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
        let checks = ReleaseChecks {
            platforms: &[Platform::Aws],
            allow_missing_controller_targets: true,
        };
        let problem = check_bundle(&listed(), &bundle, &checks).unwrap_err();
        assert!(problem.contains("names no step protocol"), "{problem}");
    }

    #[test]
    fn refuses_another_protocol_and_a_reserved_name() {
        let dir = tempfile::tempdir().unwrap();
        let checks = ReleaseChecks {
            platforms: &[Platform::Aws],
            allow_missing_controller_targets: true,
        };
        let mut other = bundle(dir.path(), IndexMap::new());
        other.manifest.protocol.controller = Some(2);
        assert!(check_bundle(&listed(), &other, &checks)
            .unwrap_err()
            .contains("step protocol 2"));
        let mut other = bundle(dir.path(), IndexMap::new());
        other.manifest.protocol.hooks = 2;
        assert!(check_bundle(&listed(), &other, &checks)
            .unwrap_err()
            .contains("hooks ABI 2"));
        let mut private = bundle(dir.path(), IndexMap::new());
        private.manifest.controller_url = Some("https://10.0.0.1".to_string());
        assert!(check_bundle(&listed(), &private, &checks)
            .unwrap_err()
            .contains("private or special-purpose address"));
        let mut reserved = bundle(dir.path(), IndexMap::new());
        reserved.manifest.name = "alien".to_string();
        let mut listed_reserved = listed();
        listed_reserved.name = "alien".to_string();
        assert!(check_bundle(&listed_reserved, &reserved, &checks)
            .unwrap_err()
            .contains("reserved"));
        let mut stem = bundle(dir.path(), IndexMap::new());
        stem.manifest.name = "outputs".to_string();
        let mut listed_stem = listed();
        listed_stem.name = "outputs".to_string();
        assert!(check_bundle(&listed_stem, &stem, &checks)
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
            extension_workspaces(&project),
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
        let stack = evaluate_building_extensions(project.path(), &[], || async {
            evaluated.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Stack::new("my-app".to_string()).build())
        })
        .await
        .unwrap();
        assert!(stack.extensions.is_empty());
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
            Err::<Stack, _>(AlienError::new(ErrorData::ConfigurationError {
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

    /// The module `alien extensions build` generates for `name`, as far as the release reads it.
    fn generated_module(root: &Path, name: &str) {
        std::fs::create_dir_all(root.join("generated")).unwrap();
        std::fs::write(
            root.join("generated/index.ts"),
            format!("const extension: ExtensionManifest = {{ \"name\": \"{name}\" }}\n\nexport default extension\n"),
        )
        .unwrap();
    }

    fn listing(names: &[&str]) -> Stack {
        let mut stack = Stack::new("my-app".to_string()).build();
        for name in names {
            stack.extensions.push(ExtensionManifest {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                digest: String::new(),
                resource_types: IndexMap::new(),
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
                _ => Ok(listing(&["budget"])),
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
            Err::<Stack, _>(AlienError::new(ErrorData::ConfigurationError {
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
            listed_workspaces(&listing(&["vendored", "linked"]), &project).unwrap(),
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
            listed_workspaces(&listing(&["acme"]), &project).unwrap(),
            [project.join("acme")]
        );
    }

    #[test]
    fn an_unreadable_generated_module_is_an_error() {
        let project = tempfile::tempdir().unwrap();
        let budget = project.path().join("budget");
        broken_workspace(&budget);
        std::fs::create_dir_all(budget.join("generated")).unwrap();
        std::fs::write(budget.join("generated/index.ts"), "export {}\n").unwrap();
        let error = listed_workspaces(&listing(&["budget"]), project.path()).unwrap_err();
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
        let checks = ReleaseChecks {
            platforms: &[Platform::Aws],
            allow_missing_controller_targets: true,
        };
        let mut installed = bundle(dir.path(), IndexMap::new());
        installed.zip = zip_of(&[(alien_core::EXTENSION_HOOKS_MODULE, b"module")]);
        installed.manifest.hooks_sha256 = hex::encode(Sha256::digest(b"module"));
        check_shipped(&installed, &checks).await.unwrap();
        installed.manifest.hooks_sha256 = hex::encode(Sha256::digest(b"another module"));
        let error = check_shipped(&installed, &checks).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("hooks.wasm is not the module its manifest lists"),
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
        let bundles = find_bundles(&module_dirs).unwrap();
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
