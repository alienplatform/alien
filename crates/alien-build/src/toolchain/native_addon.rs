//! Native bindings addon staging for the TypeScript toolchain.
//!
//! `@alienplatform/bindings` ships a napi addon that the compiled binary embeds
//! statically. Before `bun build --compile` runs, the TARGET platform's addon
//! must be staged next to the bindings package's `dist/native.js` under the
//! literal file name its `./native` entry imports. This module owns locating
//! the right addon (installed prebuild, workspace dev build, or a source build
//! via the napi CLI) and copying it into place.
//!
//! The bindings package's `dist/` is found by **real module resolution**, not a
//! hard-coded `src_dir/node_modules/@alienplatform/bindings` path. A
//! Container/Daemon depends on the bindings package directly, but a Worker
//! reaches it only *transitively through `@alienplatform/sdk`* — so its addon
//! lives under the SDK's dependency (a pnpm virtual-store sibling), not the
//! app's own `node_modules`. Resolving via bun (which honors pnpm symlinks and
//! package `exports`) stages the addon at exactly the path `bun build --compile`
//! will resolve the app's `@alienplatform/bindings/native` import to.
//!
//! The per-source-directory build serialization lock lives in `typescript.rs`,
//! since it also guards the generated bootstrap written by that toolchain.

use crate::error::{ErrorData, Result};
use alien_core::BinaryTarget;
use alien_error::{AlienError, Context, IntoAlienError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::process::Command;
use tracing::info;

/// One napi-addon package that a compiled binary embeds statically. Two ship
/// today — `@alienplatform/bindings` (kv/storage/queue/vault/container) and
/// `@alienplatform/ai-gateway` (the `ai()` client) — and each stages its own
/// addon next to its own `dist/native.js` under the literal file name its
/// `./native` entry imports.
struct NativeAddonSpec {
    /// npm package carrying the JS side (e.g. "@alienplatform/bindings").
    package: &'static str,
    /// Package name without the `@alienplatform/` scope (e.g. "bindings"), used
    /// for the sibling prebuild directory `<scope>/<scoped_name>-<triple>`.
    scoped_name: &'static str,
    /// File name the package's `./native` entry statically imports
    /// (`import addon from "./<staged_file>"` next to `dist/native.js`).
    staged_file: &'static str,
    /// Workspace crate dir under `crates/` — also the addon file-name prefix
    /// (`<crate_dir>.<triple>.node`).
    crate_dir: &'static str,
}

/// The bindings addon: required whenever the app resolves it (every consumer
/// uses some binding), so a missing addon for the target fails the build.
const BINDINGS: NativeAddonSpec = NativeAddonSpec {
    package: "@alienplatform/bindings",
    scoped_name: "bindings",
    staged_file: "alien-bindings.node",
    crate_dir: "alien-bindings-node",
};

/// The AI-gateway addon: staged best-effort. A Worker resolves it transitively
/// through the SDK even when it never calls `ai()`, so a missing addon for the
/// target is skipped (not a build error) — requiring it would regress every
/// non-AI Worker. When the addon is present it is embedded so a compiled
/// `ai()`/`getAiConnection()` resolves.
const AI_GATEWAY: NativeAddonSpec = NativeAddonSpec {
    package: "@alienplatform/ai-gateway",
    scoped_name: "ai-gateway",
    staged_file: "alien-ai-gateway.node",
    crate_dir: "alien-ai-gateway-node",
};

/// Map a build target to the napi triple used in prebuild package names
/// (`@alienplatform/bindings-<triple>`) and addon file names
/// (`alien-bindings-node.<triple>.node`). Mirrors `platformTriple()` in
/// `packages/bindings/src/loader.ts`. `None` means no addon exists for the
/// target.
fn napi_triple(target: BinaryTarget) -> Option<&'static str> {
    match target {
        BinaryTarget::LinuxX64 => Some("linux-x64-gnu"),
        BinaryTarget::LinuxArm64 => Some("linux-arm64-gnu"),
        BinaryTarget::DarwinArm64 => Some("darwin-arm64"),
        // No Windows prebuild is published for the bindings addon.
        BinaryTarget::WindowsX64 => None,
    }
}

/// Locate the native addon binary for `triple`, trying (in order):
///
/// 1. The per-platform prebuild package (`@alienplatform/bindings-<triple>`,
///    an `optionalDependency` of the bindings package). Checked both as a
///    sibling of the *resolved* bindings package (`<@scope>/bindings-<triple>`,
///    where pnpm links a Worker's transitive prebuild) and in the app's own
///    `node_modules` (flat npm installs / a direct dependency) — how
///    npm-installed apps get it.
/// 2. The workspace dev addon (`crates/alien-bindings-node/…​.node`, found by
///    walking up from the app) — repo-internal checkouts.
/// 3. When in-repo and building for the host triple: source-build the dev
///    addon via the napi CLI, then use it.
///
/// `bindings_dist` is the resolved `dist/` directory of the bindings package
/// (see {@link resolve_bindings_dist_dir}); its grandparent is the package
/// scope directory where a sibling prebuild is linked.
///
/// Returns `Ok(None)` when no source exists (the caller turns that into a
/// build error naming the missing prebuild package).
async fn find_addon_source(
    src_dir: &Path,
    spec: &NativeAddonSpec,
    addon_dist: &Path,
    triple: &str,
    addon_file_name: &str,
    resource_name: &str,
    checked: &mut Vec<String>,
) -> Result<Option<PathBuf>> {
    // 1a. Prebuild linked next to the resolved package. `addon_dist` is
    // `<scope>/@alienplatform/<name>/dist`, so the prebuild
    // `@alienplatform/<name>-<triple>` sits at `<scope>/@alienplatform/
    // <name>-<triple>` — two levels up from dist, then the sibling name.
    if let Some(scope_dir) = addon_dist.parent().and_then(Path::parent) {
        let sibling_prebuild = scope_dir
            .join(format!("{}-{}", spec.scoped_name, triple))
            .join(addon_file_name);
        if sibling_prebuild.is_file() {
            return Ok(Some(sibling_prebuild));
        }
        checked.push(sibling_prebuild.display().to_string());
    }

    // 1b. Prebuild package installed in the app's own node_modules.
    let prebuild = src_dir
        .join("node_modules")
        .join(format!("{}-{}", spec.package, triple))
        .join(addon_file_name);
    if prebuild.is_file() {
        return Ok(Some(prebuild));
    }
    checked.push(prebuild.display().to_string());

    // 2. Workspace dev addon. Walk up from the app directory AND from the
    //    resolved bindings dist: an app outside the repo that links the
    //    workspace's bindings package (pnpm link, test fixtures under a temp
    //    dir) resolves `bindings_dist` to the real in-repo path, which is the
    //    only anchor that reaches `crates/alien-bindings-node` in that case.
    //    The dist path is canonicalized so a symlinked node_modules entry
    //    walks the repo, not the app's directory again.
    let canonical_dist = addon_dist.canonicalize().ok();
    let mut workspace_addon_crate: Option<PathBuf> = None;
    'anchors: for anchor in std::iter::once(src_dir).chain(canonical_dist.as_deref()) {
        let mut dir = Some(anchor);
        while let Some(current) = dir {
            let crate_dir = current.join("crates").join(spec.crate_dir);
            if crate_dir.is_dir() {
                workspace_addon_crate = Some(crate_dir.clone());
                let dev_addon = crate_dir.join(addon_file_name);
                if dev_addon.is_file() {
                    return Ok(Some(dev_addon));
                }
                checked.push(dev_addon.display().to_string());
                break 'anchors;
            }
            dir = current.parent();
        }
        checked.push(format!(
            "(no crates/{} above {})",
            spec.crate_dir,
            anchor.display()
        ));
    }

    // 3. In-repo, host-triple build: source-build the dev addon with napi.
    let host_triple = napi_triple(BinaryTarget::current_os());
    let Some(crate_dir) = workspace_addon_crate else {
        return Ok(None);
    };
    if host_triple != Some(triple) {
        checked.push(format!(
            "(source-build skipped: host triple {:?} != target '{triple}')",
            host_triple
        ));
        return Ok(None);
    }

    info!(
        "Native addon {} not built yet; running `napi build --platform --release` in {}",
        addon_file_name,
        crate_dir.display()
    );
    let output = Command::new("npx")
        .args(["napi", "build", "--platform", "--release"])
        .current_dir(&crate_dir)
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "Failed to execute `npx napi build --platform --release` in {}",
                crate_dir.display()
            ),
            build_output: None,
        })?;
    if !output.status.success() {
        let mut build_output = String::from_utf8_lossy(&output.stdout).into_owned();
        build_output.push_str(&String::from_utf8_lossy(&output.stderr));
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "`napi build --platform --release` failed in {} while building the native bindings addon",
                crate_dir.display()
            ),
            build_output: Some(build_output),
        }));
    }

    let dev_addon = crate_dir.join(addon_file_name);
    if dev_addon.is_file() {
        Ok(Some(dev_addon))
    } else {
        Ok(None)
    }
}

/// bun script that prints the resolved path of a package's `./native` entry
/// (i.e. `.../@alienplatform/<name>/dist/native.js`) or nothing.
///
/// It resolves `<package>/native` first directly from the app (Container/Daemon
/// direct dependency), then — failing that — from the resolved location of
/// `@alienplatform/sdk` (a Worker's only path to the package). Using bun's own
/// resolver honors pnpm symlinks and package `exports` maps, so the printed path
/// is exactly what `bun build --compile` will embed.
fn resolve_native_script(package: &str) -> String {
    format!(
        r#"
const path = require("path");
const from = process.env.ALIEN_ADDON_RESOLVE_FROM;
const nativeSpec = "{package}/native";
const tryResolve = (s, base) => {{ try {{ return Bun.resolveSync(s, base); }} catch {{ return null; }} }};
let route = "direct";
let nativeEntry = tryResolve(nativeSpec, from);
if (!nativeEntry) {{
  const sdkEntry = tryResolve("@alienplatform/sdk", from);
  if (sdkEntry) {{ nativeEntry = tryResolve(nativeSpec, path.dirname(sdkEntry)); route = "sdk"; }}
}}
if (nativeEntry) process.stdout.write(route + "\n" + nativeEntry);
"#
    )
}

/// Resolve the `dist/` directory of `spec.package` as the app itself resolves it
/// — directly, or transitively through `@alienplatform/sdk` (the only path a
/// Worker has). Returns `None` when the app depends on neither, i.e. it is not a
/// consumer of that package and staging is a no-op.
///
/// Resolution is delegated to bun (already required by the compile step) so pnpm
/// symlinks and package `exports` are honored — the naive
/// `src_dir/node_modules/<package>` path does not exist for a Worker, whose copy
/// lives under the SDK's dependency. This function is exercised by the SDK-entry
/// compiled-artifact oracle (`packages/package-layout/steps/compile.ts` covers
/// the `/native` entry; the Worker/SDK entry is covered by deployment E2E), not
/// a hermetic unit test, since faithful resolution requires bun and a real
/// installed layout.
async fn resolve_addon_dist_dir(
    src_dir: &Path,
    spec: &NativeAddonSpec,
    resource_name: &str,
) -> Result<Option<(PathBuf, AddonResolutionRoute)>> {
    let output = Command::new("bun")
        .args(["-e", &resolve_native_script(spec.package)])
        .env("ALIEN_ADDON_RESOLVE_FROM", src_dir)
        .current_dir(src_dir)
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "Failed to run bun to resolve {}. Is Bun installed?",
                spec.package
            ),
            build_output: None,
        })?;

    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "bun failed while resolving {} from {}",
                spec.package,
                src_dir.display()
            ),
            build_output: Some(String::from_utf8_lossy(&output.stderr).into_owned()),
        }));
    }

    let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if resolved.is_empty() {
        // App depends on neither this package nor the SDK: no addon to embed.
        return Ok(None);
    }
    // First line is the resolution route ("direct" or "sdk"), second the
    // resolved native entry path — the route decides which package the
    // generated entry wrapper can import `installEmbeddedAddon` from.
    let (route_str, native_entry) = resolved.split_once('\n').ok_or_else(|| {
        AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!("Unexpected output resolving {}: '{resolved}'", spec.package),
            build_output: None,
        })
    })?;
    let route = match route_str {
        "sdk" => AddonResolutionRoute::ViaSdk,
        "direct" => AddonResolutionRoute::DirectBindings,
        other => {
            // The resolver script only emits "direct" or "sdk"; anything else
            // means the script and this parser drifted — fail here, not with
            // an unresolvable import deep inside `bun build --compile`.
            return Err(AlienError::new(ErrorData::ImageBuildFailed {
                resource_name: resource_name.to_string(),
                reason: format!(
                    "Unexpected resolution route '{other}' for {} from the resolver script",
                    spec.package
                ),
                build_output: None,
            }));
        }
    };

    let dist = Path::new(&native_entry)
        .parent()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ImageBuildFailed {
                resource_name: resource_name.to_string(),
                reason: format!(
                    "Resolved {} native entry '{}' has no parent directory",
                    spec.package, native_entry
                ),
                build_output: None,
            })
        })?
        .to_path_buf();
    Ok(Some((dist, route)))
}

/// How the app resolves `@alienplatform/bindings`: as a direct dependency, or
/// only transitively through `@alienplatform/sdk`. The generated compile entry
/// must import `installEmbeddedAddon` through a specifier the app can resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AddonResolutionRoute {
    /// The app depends on `@alienplatform/bindings` itself.
    DirectBindings,
    /// Bindings only resolve through `@alienplatform/sdk` (its dependency).
    ViaSdk,
}

/// A staged addon plus the exclusivity that keeps it valid: the guard holds a
/// per-dist-path lock from staging until the caller's compile finishes, so a
/// concurrent build staging a DIFFERENT target into the same shared
/// `dist/alien-bindings.node` cannot swap the file mid-embed. Same-target
/// concurrency was already safe (atomic rename, identical content), but this
/// serializes conservatively — per-target staging locations are the real fix
/// for multi-target throughput.
pub(super) struct StagedAddon {
    pub route: AddonResolutionRoute,
    // One guard per staged addon (bindings, and ai-gateway when present). Each
    // holds its per-dist-path lock from staging until the caller's compile
    // finishes, so a concurrent build cannot swap a staged file mid-embed.
    _guards: Vec<tokio::sync::OwnedMutexGuard<()>>,
}

/// Workspace dev addon files that exist for the requested targets, walking
/// up from `anchor` (typically the realpath of the resolved bindings
/// package). Used by the build cache key: the compiled binary embeds these
/// bytes, so a rebuilt addon must invalidate cached artifacts.
pub(crate) fn workspace_addon_inputs(anchor: &Path, targets: &[BinaryTarget]) -> Vec<PathBuf> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    for spec in [&BINDINGS, &AI_GATEWAY] {
        let mut crate_dir: Option<PathBuf> = None;
        let mut dir = Some(anchor);
        while let Some(current) = dir {
            let candidate = current.join("crates").join(spec.crate_dir);
            if candidate.is_dir() {
                crate_dir = Some(candidate);
                break;
            }
            dir = current.parent();
        }
        let Some(crate_dir) = crate_dir else {
            continue;
        };
        inputs.extend(
            targets
                .iter()
                .filter_map(|target| napi_triple(*target))
                .map(|triple| crate_dir.join(format!("{}.{triple}.node", spec.crate_dir)))
                .filter(|path| path.is_file()),
        );
    }
    inputs.sort();
    inputs.dedup();
    inputs
}

/// One lock per staged-addon path (canonicalized bindings dist).
static STAGING_LOCKS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
> = std::sync::OnceLock::new();

async fn lock_staged_path(staged: &Path) -> tokio::sync::OwnedMutexGuard<()> {
    let key = staged
        .canonicalize()
        .unwrap_or_else(|_| staged.to_path_buf());
    let lock = {
        let map = STAGING_LOCKS.get_or_init(Default::default);
        let mut map = map.lock().expect("staging lock map poisoned");
        Arc::clone(map.entry(key).or_default())
    };
    lock.lock_owned().await
}

/// Stage the TARGET platform's native addons next to each package's
/// `dist/native.js` so `bun build --compile` can embed them.
///
/// Each package's `./native` entry imports its addon through a literal specifier
/// (`./alien-bindings.node`, `./alien-ai-gateway.node`); this fulfills that
/// staging contract for both. Bindings is required whenever the app resolves it —
/// a missing addon for the target fails with a clear error naming the prebuild,
/// since otherwise `bun build --compile` fails with an opaque unresolved-import.
/// The ai-gateway addon is staged best-effort: a Worker resolves it transitively
/// through the SDK even when it never calls `ai()`, so a missing addon there is
/// skipped, not a build error (requiring it would regress every non-AI Worker).
/// Returns the primary (bindings) resolution route — the generated compile entry
/// installs both via `@alienplatform/sdk/native` — plus the guards that keep the
/// staged files valid until the compile finishes. `None` when the app consumes
/// no native addons.
pub(super) async fn stage_native_addon(
    src_dir: &Path,
    target: BinaryTarget,
    resource_name: &str,
) -> Result<Option<StagedAddon>> {
    // Bindings is the primary addon: its resolution route drives the generated
    // entry, and an app that resolves neither it nor the SDK embeds nothing.
    let Some((bindings_dist, route)) =
        resolve_addon_dist_dir(src_dir, &BINDINGS, resource_name).await?
    else {
        return Ok(None);
    };
    let mut guards = Vec::new();
    // Take the per-path staging lock BEFORE writing, and hand it to the caller so
    // it survives until the compile that embeds the staged file has finished.
    guards.push(lock_staged_path(&bindings_dist.join(BINDINGS.staged_file)).await);
    stage_addon_into(src_dir, &BINDINGS, &bindings_dist, target, resource_name, true).await?;

    // AI gateway: best-effort. Stage it only when the app resolves it AND an
    // addon for the target exists; skipping keeps non-AI Workers building.
    if let Some((ai_dist, _ai_route)) =
        resolve_addon_dist_dir(src_dir, &AI_GATEWAY, resource_name).await?
    {
        let ai_guard = lock_staged_path(&ai_dist.join(AI_GATEWAY.staged_file)).await;
        if stage_addon_into(src_dir, &AI_GATEWAY, &ai_dist, target, resource_name, false)
            .await?
            .is_some()
        {
            guards.push(ai_guard);
        }
    }

    Ok(Some(StagedAddon {
        route,
        _guards: guards,
    }))
}

/// Source the target addon for `spec` and copy it into `addon_dist` as the
/// staged `spec.staged_file`. Split from {@link stage_native_addon} so the
/// sourcing and copy logic is unit-testable against a fixture `dist/` directory
/// without invoking bun's resolver.
///
/// `required` distinguishes the two addons: bindings (`required = true`) errors
/// when no addon exists for the target; ai-gateway (`required = false`) returns
/// `Ok(None)` instead, so a Worker that resolves ai-gateway through the SDK but
/// has no addon for the target still builds (it just can't call `ai()` in the
/// compiled binary). Returns `Some(staged_path)` when it staged, `None` when it
/// skipped an optional addon.
async fn stage_addon_into(
    src_dir: &Path,
    spec: &NativeAddonSpec,
    addon_dist: &Path,
    target: BinaryTarget,
    resource_name: &str,
    required: bool,
) -> Result<Option<PathBuf>> {
    let Some(triple) = napi_triple(target) else {
        if !required {
            info!(
                "No {} native addon exists for build target '{}'; skipping (optional)",
                spec.package, target
            );
            return Ok(None);
        }
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "{} is installed, but no native addon exists for build target '{}'. \
                 Native addons support linux-x64, linux-arm64, and darwin-arm64 targets.",
                spec.package, target
            ),
            build_output: None,
        }));
    };
    let addon_file_name = format!("{}.{}.node", spec.crate_dir, triple);

    let mut checked = Vec::new();
    let Some(source) = find_addon_source(
        src_dir,
        spec,
        addon_dist,
        triple,
        &addon_file_name,
        resource_name,
        &mut checked,
    )
    .await?
    else {
        if !required {
            info!(
                "Optional native addon {} for target '{}' not found; skipping (the app may not \
                 use it). Checked: {}",
                spec.package,
                target,
                checked.join(", ")
            );
            return Ok(None);
        }
        let lib_name = format!("lib{}.so", spec.crate_dir.replace('-', "_"));
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "{pkg} is installed, but the native addon for target '{target}' was not found. \
                 Install the prebuild package '{pkg}-{triple}' (it ships {addon_file_name}), \
                 or, in the alien workspace, build the dev addon with \
                 `npx napi build --platform --release` in crates/{crate_dir}. \
                 Cross-building from another OS: zig/napi-cross cannot build this cdylib; \
                 build natively in Docker instead: \
                 `docker run --rm --platform linux/<arch> -v <workspace>:/work \
                 -e CARGO_TARGET_DIR=/tmp/target -w /work/crates/{crate_dir} \
                 rust:1-bookworm sh -c 'apt-get update -qq && apt-get install -y -qq \
                 protobuf-compiler && cargo build --release --lib && \
                 cp /tmp/target/release/{lib_name} {addon_file_name}'`. \
                 Checked: {checked}.",
                pkg = spec.package,
                crate_dir = spec.crate_dir,
                checked = checked.join(", "),
            ),
            build_output: None,
        }));
    };

    // The staged path is a SHARED singleton (`native.js` imports the literal
    // `./<staged_file>`), and concurrent builds — parallel containers in one
    // stack, parallel tests — all stage into it. Write via a unique temp file +
    // atomic rename so a concurrent `bun build --compile` never reads a
    // half-written addon, and never delete it after a build (see the cleanup in
    // `typescript.rs`): removing it would yank the file out from under a
    // concurrent compile. The dist directory is build output, so a lingering
    // copy is expected debris and the next staging simply renames over it.
    let staged = addon_dist.join(spec.staged_file);
    let staged_tmp = addon_dist.join(format!("{}.staging-{}", spec.staged_file, std::process::id()));
    let stage_result = async {
        fs::copy(&source, &staged_tmp).await?;
        fs::rename(&staged_tmp, &staged).await
    }
    .await;
    if stage_result.is_err() {
        // Best-effort temp cleanup before surfacing the real error.
        let _ = fs::remove_file(&staged_tmp).await;
    }
    stage_result
        .into_alien_error()
        .context(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "Failed to stage native addon {} to {}",
                source.display(),
                staged.display()
            ),
            build_output: None,
        })?;
    info!(
        "Staged {} native addon for {}: {} -> {}",
        spec.package,
        target,
        source.display(),
        staged.display()
    );
    Ok(Some(staged))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    /// Create `<dir>/node_modules/@alienplatform/bindings/dist/native.js` and
    /// return the `dist/` path — the directory an addon is staged into, standing
    /// in for the one {@link resolve_bindings_dist_dir} produces at runtime.
    /// (The tests drive {@link stage_addon_into} directly with this path, so no
    /// bun resolution is needed; the resolver is verified by the compiled
    /// artifact oracle.)
    async fn install_fake_bindings_package(app_dir: &Path) -> PathBuf {
        install_fake_addon_package(app_dir, &BINDINGS).await
    }

    /// Create `<dir>/node_modules/<spec.package>/dist/native.js` and return the
    /// `dist/` path — the directory an addon is staged into.
    async fn install_fake_addon_package(app_dir: &Path, spec: &NativeAddonSpec) -> PathBuf {
        let dist = app_dir
            .join("node_modules")
            .join(spec.package)
            .join("dist");
        fs::create_dir_all(&dist).await.unwrap();
        fs::write(dist.join("native.js"), "// fake native entry")
            .await
            .unwrap();
        dist
    }

    #[test]
    fn napi_triple_matches_bindings_loader_mapping() {
        assert_eq!(napi_triple(BinaryTarget::LinuxX64), Some("linux-x64-gnu"));
        assert_eq!(
            napi_triple(BinaryTarget::LinuxArm64),
            Some("linux-arm64-gnu")
        );
        assert_eq!(napi_triple(BinaryTarget::DarwinArm64), Some("darwin-arm64"));
        assert_eq!(napi_triple(BinaryTarget::WindowsX64), None);
    }

    #[tokio::test]
    async fn staging_copies_target_addon_from_installed_prebuild_package() {
        let app = tempdir().unwrap();
        let bindings_dist = install_fake_bindings_package(app.path()).await;

        // Install the TARGET platform's prebuild package (a linux addon, as a
        // cross-build from any host would need). It sits beside the bindings
        // package in the same scope directory — where both the resolved-sibling
        // (1a) and app-node_modules (1b) lookups find it.
        let prebuild_dir = app
            .path()
            .join("node_modules")
            .join("@alienplatform/bindings-linux-arm64-gnu");
        fs::create_dir_all(&prebuild_dir).await.unwrap();
        let addon_bytes = b"fake-linux-arm64-addon";
        fs::write(
            prebuild_dir.join("alien-bindings-node.linux-arm64-gnu.node"),
            addon_bytes,
        )
        .await
        .unwrap();

        let staged = stage_addon_into(
            app.path(),
            &BINDINGS,
            &bindings_dist,
            BinaryTarget::LinuxArm64,
            "app",
            true,
        )
        .await
        .expect("staging should succeed from the installed prebuild")
        .expect("required addon must stage");

        assert_eq!(
            staged,
            bindings_dist.join(BINDINGS.staged_file),
            "the addon must land next to dist/native.js under the exact name its static import uses"
        );
        assert_eq!(fs::read(&staged).await.unwrap(), addon_bytes);
    }

    #[tokio::test]
    async fn staging_prefers_prebuild_over_workspace_dev_addon() {
        let root = tempdir().unwrap();
        // Fake workspace: <root>/crates/alien-bindings-node with a dev addon.
        let crate_dir = root.path().join("crates").join("alien-bindings-node");
        fs::create_dir_all(&crate_dir).await.unwrap();
        fs::write(
            crate_dir.join("alien-bindings-node.linux-x64-gnu.node"),
            b"workspace-dev-addon",
        )
        .await
        .unwrap();

        // App inside the workspace with a prebuild linked beside the bindings package.
        let app_dir = root.path().join("apps").join("svc");
        let bindings_dist = install_fake_bindings_package(&app_dir).await;
        let prebuild_dir = app_dir
            .join("node_modules")
            .join("@alienplatform/bindings-linux-x64-gnu");
        fs::create_dir_all(&prebuild_dir).await.unwrap();
        fs::write(
            prebuild_dir.join("alien-bindings-node.linux-x64-gnu.node"),
            b"app-prebuild-addon",
        )
        .await
        .unwrap();

        let staged = stage_addon_into(
            &app_dir,
            &BINDINGS,
            &bindings_dist,
            BinaryTarget::LinuxX64,
            "svc",
            true,
        )
        .await
        .unwrap()
        .expect("required addon must stage");
        assert_eq!(fs::read(&staged).await.unwrap(), b"app-prebuild-addon");
    }

    #[tokio::test]
    async fn staging_falls_back_to_workspace_dev_addon() {
        let root = tempdir().unwrap();
        let crate_dir = root.path().join("crates").join("alien-bindings-node");
        fs::create_dir_all(&crate_dir).await.unwrap();
        fs::write(
            crate_dir.join("alien-bindings-node.linux-x64-gnu.node"),
            b"workspace-dev-addon",
        )
        .await
        .unwrap();

        let app_dir = root.path().join("apps").join("svc");
        let bindings_dist = install_fake_bindings_package(&app_dir).await;

        let staged = stage_addon_into(
            &app_dir,
            &BINDINGS,
            &bindings_dist,
            BinaryTarget::LinuxX64,
            "svc",
            true,
        )
        .await
        .unwrap()
        .expect("required addon must stage");
        assert_eq!(fs::read(&staged).await.unwrap(), b"workspace-dev-addon");
    }

    #[tokio::test]
    async fn staging_failure_names_the_missing_prebuild_package() {
        let app = tempdir().unwrap();
        let bindings_dist = install_fake_bindings_package(app.path()).await;

        // Cross target (never the host triple), no prebuild installed, and no
        // workspace crate above the temp dir — no addon source exists.
        let target = if BinaryTarget::current_os() == BinaryTarget::LinuxArm64 {
            BinaryTarget::LinuxX64
        } else {
            BinaryTarget::LinuxArm64
        };
        let triple = napi_triple(target).unwrap();

        let error = stage_addon_into(app.path(), &BINDINGS, &bindings_dist, target, "app", true)
            .await
            .expect_err("staging must fail when no addon source exists");
        let message = error.to_string();
        assert!(
            message.contains(&format!("@alienplatform/bindings-{}", triple)),
            "error must name the missing prebuild package, got: {message}"
        );
    }

    #[tokio::test]
    async fn staging_fails_for_targets_without_an_addon() {
        let app = tempdir().unwrap();
        let bindings_dist = install_fake_bindings_package(app.path()).await;

        let error = stage_addon_into(
            app.path(),
            &BINDINGS,
            &bindings_dist,
            BinaryTarget::WindowsX64,
            "app",
            true,
        )
        .await
        .expect_err("windows has no native addon");
        assert!(
            error.to_string().contains("windows-x64"),
            "error should name the unsupported target, got: {error}"
        );
    }

    /// The ai-gateway addon is staged best-effort: when its addon for the target
    /// is missing, staging returns `Ok(None)` (skip) instead of failing — a
    /// non-AI Worker that resolves ai-gateway through the SDK must still build.
    #[tokio::test]
    async fn optional_addon_skips_when_source_is_missing() {
        let app = tempdir().unwrap();
        let ai_dist = install_fake_addon_package(app.path(), &AI_GATEWAY).await;

        // Cross target (never the host), no prebuild, no workspace crate above
        // the temp dir — no ai-gateway addon source exists.
        let target = if BinaryTarget::current_os() == BinaryTarget::LinuxArm64 {
            BinaryTarget::LinuxX64
        } else {
            BinaryTarget::LinuxArm64
        };

        let staged = stage_addon_into(app.path(), &AI_GATEWAY, &ai_dist, target, "app", false)
            .await
            .expect("optional staging must not error when the addon is missing");
        assert!(
            staged.is_none(),
            "a missing optional addon must be skipped (None), not staged"
        );
        assert!(
            !ai_dist.join(AI_GATEWAY.staged_file).exists(),
            "nothing should be staged when the optional addon is absent"
        );
    }

    /// When the ai-gateway addon IS present it stages under its own literal file
    /// name (`alien-ai-gateway.node`) next to its own dist/native.js.
    #[tokio::test]
    async fn optional_ai_gateway_addon_stages_when_present() {
        let app = tempdir().unwrap();
        let ai_dist = install_fake_addon_package(app.path(), &AI_GATEWAY).await;

        let prebuild_dir = app
            .path()
            .join("node_modules")
            .join("@alienplatform/ai-gateway-linux-arm64-gnu");
        fs::create_dir_all(&prebuild_dir).await.unwrap();
        let addon_bytes = b"fake-ai-gateway-linux-arm64-addon";
        fs::write(
            prebuild_dir.join("alien-ai-gateway-node.linux-arm64-gnu.node"),
            addon_bytes,
        )
        .await
        .unwrap();

        let staged = stage_addon_into(
            app.path(),
            &AI_GATEWAY,
            &ai_dist,
            BinaryTarget::LinuxArm64,
            "app",
            false,
        )
        .await
        .expect("staging should succeed from the installed prebuild")
        .expect("present optional addon must stage");
        assert_eq!(staged, ai_dist.join(AI_GATEWAY.staged_file));
        assert_eq!(fs::read(&staged).await.unwrap(), addon_bytes);
    }
}
