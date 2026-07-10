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
use tokio::fs;
use tokio::process::Command;
use tracing::info;

/// npm package that carries the JS side of the native bindings.
const BINDINGS_PACKAGE: &str = "@alienplatform/bindings";

/// File name the bindings package's `./native` entry statically imports
/// (`import addon from "./alien-bindings.node"` next to `dist/native.js`).
const STAGED_ADDON_FILE: &str = "alien-bindings.node";

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
    bindings_dist: &Path,
    triple: &str,
    addon_file_name: &str,
    resource_name: &str,
) -> Result<Option<PathBuf>> {
    // 1a. Prebuild linked next to the resolved bindings package. `bindings_dist`
    // is `<scope>/@alienplatform/bindings/dist`, so the prebuild
    // `@alienplatform/bindings-<triple>` sits at `<scope>/@alienplatform/
    // bindings-<triple>` — two levels up from dist, then the sibling name.
    if let Some(scope_dir) = bindings_dist.parent().and_then(Path::parent) {
        let sibling_prebuild = scope_dir
            .join(format!("bindings-{}", triple))
            .join(addon_file_name);
        if sibling_prebuild.is_file() {
            return Ok(Some(sibling_prebuild));
        }
    }

    // 1b. Prebuild package installed in the app's own node_modules.
    let prebuild = src_dir
        .join("node_modules")
        .join(format!("{}-{}", BINDINGS_PACKAGE, triple))
        .join(addon_file_name);
    if prebuild.is_file() {
        return Ok(Some(prebuild));
    }

    // 2. Workspace dev addon. Walk up from the app directory AND from the
    //    resolved bindings dist: an app outside the repo that links the
    //    workspace's bindings package (pnpm link, test fixtures under a temp
    //    dir) resolves `bindings_dist` to the real in-repo path, which is the
    //    only anchor that reaches `crates/alien-bindings-node` in that case.
    //    The dist path is canonicalized so a symlinked node_modules entry
    //    walks the repo, not the app's directory again.
    let canonical_dist = bindings_dist.canonicalize().ok();
    let mut workspace_addon_crate: Option<PathBuf> = None;
    'anchors: for anchor in std::iter::once(src_dir).chain(canonical_dist.as_deref()) {
        let mut dir = Some(anchor);
        while let Some(current) = dir {
            let crate_dir = current.join("crates").join("alien-bindings-node");
            if crate_dir.is_dir() {
                workspace_addon_crate = Some(crate_dir.clone());
                let dev_addon = crate_dir.join(addon_file_name);
                if dev_addon.is_file() {
                    return Ok(Some(dev_addon));
                }
                break 'anchors;
            }
            dir = current.parent();
        }
    }

    // 3. In-repo, host-triple build: source-build the dev addon with napi.
    let host_triple = napi_triple(BinaryTarget::current_os());
    let Some(crate_dir) = workspace_addon_crate else {
        return Ok(None);
    };
    if host_triple != Some(triple) {
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

/// bun script that prints the resolved path of the bindings package's `./native`
/// entry (i.e. `.../@alienplatform/bindings/dist/native.js`) or nothing.
///
/// It resolves `@alienplatform/bindings/native` first directly from the app
/// (Container/Daemon direct dependency), then — failing that — from the
/// resolved location of `@alienplatform/sdk` (a Worker's only path to the
/// bindings package). Using bun's own resolver honors pnpm symlinks and package
/// `exports` maps, so the printed path is exactly what `bun build --compile`
/// will embed.
const RESOLVE_BINDINGS_NATIVE_SCRIPT: &str = r#"
const path = require("path");
const from = process.env.ALIEN_BINDINGS_RESOLVE_FROM;
const tryResolve = (spec, base) => { try { return Bun.resolveSync(spec, base); } catch { return null; } };
let route = "direct";
let nativeEntry = tryResolve("@alienplatform/bindings/native", from);
if (!nativeEntry) {
  const sdkEntry = tryResolve("@alienplatform/sdk", from);
  if (sdkEntry) { nativeEntry = tryResolve("@alienplatform/bindings/native", path.dirname(sdkEntry)); route = "sdk"; }
}
if (nativeEntry) process.stdout.write(route + "\n" + nativeEntry);
"#;

/// Resolve the `dist/` directory of `@alienplatform/bindings` as the app itself
/// resolves the package — directly, or transitively through `@alienplatform/sdk`
/// (the only path a Worker has). Returns `None` when the app depends on neither,
/// i.e. it is not a bindings consumer and staging is a no-op.
///
/// Resolution is delegated to bun (already required by the compile step) so pnpm
/// symlinks and package `exports` are honored — the naive
/// `src_dir/node_modules/@alienplatform/bindings` path does not exist for a
/// Worker, whose bindings copy lives under the SDK's dependency. This function
/// is exercised by the SDK-entry compiled-artifact oracle
/// (`packages/bindings/scripts/compile-smoke.ts` covers the `/native` entry;
/// the Worker/SDK entry is proven by the ALIEN-211 deploy E2E and the manual
/// `bun build --compile` check documented in the PR), not a hermetic unit test,
/// since faithful resolution requires bun and a real installed layout.
async fn resolve_bindings_dist_dir(
    src_dir: &Path,
    resource_name: &str,
) -> Result<Option<(PathBuf, AddonResolutionRoute)>> {
    let output = Command::new("bun")
        .args(["-e", RESOLVE_BINDINGS_NATIVE_SCRIPT])
        .env("ALIEN_BINDINGS_RESOLVE_FROM", src_dir)
        .current_dir(src_dir)
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: "Failed to run bun to resolve @alienplatform/bindings. Is Bun installed?"
                .to_string(),
            build_output: None,
        })?;

    if !output.status.success() {
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "bun failed while resolving @alienplatform/bindings from {}",
                src_dir.display()
            ),
            build_output: Some(String::from_utf8_lossy(&output.stderr).into_owned()),
        }));
    }

    let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if resolved.is_empty() {
        // App depends on neither the bindings package nor the SDK: no addon to embed.
        return Ok(None);
    }
    // First line is the resolution route ("direct" or "sdk"), second the
    // resolved native entry path — the route decides which package the
    // generated entry wrapper can import `installEmbeddedAddon` from.
    let (route_str, native_entry) =
        resolved
            .split_once('\n')
            .ok_or_else(|| {
                AlienError::new(ErrorData::ImageBuildFailed {
                    resource_name: resource_name.to_string(),
                    reason: format!(
                        "Unexpected output resolving @alienplatform/bindings: '{resolved}'"
                    ),
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
                    "Unexpected bindings resolution route '{other}' from the resolver script"
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
                    "Resolved bindings native entry '{}' has no parent directory",
                    native_entry
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

/// Stage the TARGET platform's native addon next to the bindings package's
/// `dist/native.js` so `bun build --compile` can embed it.
///
/// The `./native` entry of `@alienplatform/bindings` imports the addon through
/// the literal specifier `./alien-bindings.node` (see
/// `packages/bindings/src/native.ts`); this function fulfills that staging
/// contract. Returns the staged path (for post-build clean-up) plus how the
/// app resolves the bindings package (directly or via the SDK) when the app
/// consumes it, `None` when it does not. Fails with a clear error naming the
/// missing prebuild package when a consumer has no addon for the target —
/// otherwise `bun build --compile` would fail with an opaque unresolved-import
/// error.
pub(super) async fn stage_native_addon(
    src_dir: &Path,
    target: BinaryTarget,
    resource_name: &str,
) -> Result<Option<(PathBuf, AddonResolutionRoute)>> {
    let Some((bindings_dist, route)) = resolve_bindings_dist_dir(src_dir, resource_name).await?
    else {
        return Ok(None);
    };
    let staged = stage_addon_into(src_dir, &bindings_dist, target, resource_name).await?;
    Ok(Some((staged, route)))
}

/// Source the target addon and copy it into `bindings_dist` as the staged
/// `alien-bindings.node`. Split from {@link stage_native_addon} so the sourcing
/// and copy logic is unit-testable against a fixture `dist/` directory without
/// invoking bun's resolver.
async fn stage_addon_into(
    src_dir: &Path,
    bindings_dist: &Path,
    target: BinaryTarget,
    resource_name: &str,
) -> Result<PathBuf> {
    let Some(triple) = napi_triple(target) else {
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "{} is installed, but no native addon exists for build target '{}'. \
                 Native bindings support linux-x64, linux-arm64, and darwin-arm64 targets.",
                BINDINGS_PACKAGE, target
            ),
            build_output: None,
        }));
    };
    let addon_file_name = format!("alien-bindings-node.{}.node", triple);

    let Some(source) =
        find_addon_source(src_dir, bindings_dist, triple, &addon_file_name, resource_name).await?
    else {
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!(
                "{pkg} is installed, but the native addon for target '{target}' was not found. \
                 Install the prebuild package '{pkg}-{triple}' (it ships {addon_file_name}), \
                 or, in the alien workspace, build the dev addon with \
                 `npx napi build --platform --release` in crates/alien-bindings-node.",
                pkg = BINDINGS_PACKAGE,
            ),
            build_output: None,
        }));
    };

    let staged = bindings_dist.join(STAGED_ADDON_FILE);
    fs::copy(&source, &staged)
        .await
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
        "Staged native addon for {}: {} -> {}",
        target,
        source.display(),
        staged.display()
    );
    Ok(staged)
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
        let dist = app_dir
            .join("node_modules")
            .join(BINDINGS_PACKAGE)
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

        let staged = stage_addon_into(app.path(), &bindings_dist, BinaryTarget::LinuxArm64, "app")
            .await
            .expect("staging should succeed from the installed prebuild");

        assert_eq!(
            staged,
            bindings_dist.join(STAGED_ADDON_FILE),
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

        let staged = stage_addon_into(&app_dir, &bindings_dist, BinaryTarget::LinuxX64, "svc")
            .await
            .unwrap();
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

        let staged = stage_addon_into(&app_dir, &bindings_dist, BinaryTarget::LinuxX64, "svc")
            .await
            .unwrap();
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

        let error = stage_addon_into(app.path(), &bindings_dist, target, "app")
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

        let error = stage_addon_into(app.path(), &bindings_dist, BinaryTarget::WindowsX64, "app")
            .await
            .expect_err("windows has no native addon");
        assert!(
            error.to_string().contains("windows-x64"),
            "error should name the unsupported target, got: {error}"
        );
    }
}
