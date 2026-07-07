//! Native bindings addon staging for the TypeScript toolchain.
//!
//! `@alienplatform/bindings` ships a napi addon that the compiled binary embeds
//! statically. Before `bun build --compile` runs, the TARGET platform's addon
//! must be staged next to the bindings package's `dist/native.js` under the
//! literal file name its `./native` entry imports. This module owns locating
//! the right addon (installed prebuild, workspace dev build, or a source build
//! via the napi CLI) and copying it into place.
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
/// 1. The prebuild package in the app's own `node_modules`
///    (`@alienplatform/bindings-<triple>`) — how npm-installed apps get it.
/// 2. The workspace dev addon (`crates/alien-bindings-node/…​.node`, found by
///    walking up from the app) — repo-internal checkouts.
/// 3. When in-repo and building for the host triple: source-build the dev
///    addon via the napi CLI, then use it.
///
/// Returns `Ok(None)` when no source exists (the caller turns that into a
/// build error naming the missing prebuild package).
async fn find_addon_source(
    src_dir: &Path,
    triple: &str,
    addon_file_name: &str,
    resource_name: &str,
) -> Result<Option<PathBuf>> {
    // 1. Prebuild package installed in the app's node_modules.
    let prebuild = src_dir
        .join("node_modules")
        .join(format!("{}-{}", BINDINGS_PACKAGE, triple))
        .join(addon_file_name);
    if prebuild.is_file() {
        return Ok(Some(prebuild));
    }

    // 2. Workspace dev addon, walking up from the app directory.
    let mut workspace_addon_crate: Option<PathBuf> = None;
    let mut dir = Some(src_dir);
    while let Some(current) = dir {
        let crate_dir = current.join("crates").join("alien-bindings-node");
        if crate_dir.is_dir() {
            workspace_addon_crate = Some(crate_dir.clone());
            let dev_addon = crate_dir.join(addon_file_name);
            if dev_addon.is_file() {
                return Ok(Some(dev_addon));
            }
            break;
        }
        dir = current.parent();
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

/// Stage the TARGET platform's native addon next to the bindings package's
/// `dist/native.js` so `bun build --compile` can embed it.
///
/// The `./native` entry of `@alienplatform/bindings` imports the addon through
/// the literal specifier `./alien-bindings.node` (see
/// `packages/bindings/src/native.ts`); this function fulfills that staging
/// contract. Returns the staged path (for post-build clean-up) when the
/// bindings package is installed, `None` when it isn't. Fails with a clear
/// error naming the missing prebuild package when no addon can be sourced —
/// an installed bindings package without an addon for the target would
/// otherwise fail at `bun build --compile` with an opaque unresolved-import
/// error.
pub(super) async fn stage_native_addon(
    src_dir: &Path,
    target: BinaryTarget,
    resource_name: &str,
) -> Result<Option<PathBuf>> {
    let bindings_dist = src_dir
        .join("node_modules")
        .join(BINDINGS_PACKAGE)
        .join("dist");
    if !bindings_dist.join("native.js").is_file() {
        return Ok(None);
    }

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

    let Some(source) = find_addon_source(src_dir, triple, &addon_file_name, resource_name).await?
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
    Ok(Some(staged))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    /// Create `<dir>/node_modules/@alienplatform/bindings/dist/native.js`,
    /// marking the app as a bindings consumer per the staging contract.
    async fn install_fake_bindings_package(app_dir: &Path) {
        let dist = app_dir
            .join("node_modules")
            .join(BINDINGS_PACKAGE)
            .join("dist");
        fs::create_dir_all(&dist).await.unwrap();
        fs::write(dist.join("native.js"), "// fake native entry")
            .await
            .unwrap();
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
    async fn staging_is_skipped_when_bindings_package_not_installed() {
        let app = tempdir().unwrap();
        fs::write(app.path().join("package.json"), r#"{"name":"app"}"#)
            .await
            .unwrap();

        let staged = stage_native_addon(app.path(), BinaryTarget::LinuxArm64, "app")
            .await
            .expect("staging should be a no-op without the bindings package");
        assert_eq!(staged, None);
    }

    #[tokio::test]
    async fn staging_copies_target_addon_from_installed_prebuild_package() {
        let app = tempdir().unwrap();
        install_fake_bindings_package(app.path()).await;

        // Install the TARGET platform's prebuild package (a linux addon, as a
        // cross-build from any host would need).
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

        let staged = stage_native_addon(app.path(), BinaryTarget::LinuxArm64, "app")
            .await
            .expect("staging should succeed from the installed prebuild")
            .expect("an addon should have been staged");

        assert_eq!(
            staged,
            app.path()
                .join("node_modules")
                .join(BINDINGS_PACKAGE)
                .join("dist")
                .join(STAGED_ADDON_FILE),
            "the addon must land next to dist/native.js under the exact name its static import uses"
        );
        assert_eq!(fs::read(&staged).await.unwrap(), addon_bytes);
    }

    #[tokio::test]
    async fn staging_prefers_app_prebuild_over_workspace_dev_addon() {
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

        // App inside the workspace with its own prebuild installed.
        let app_dir = root.path().join("apps").join("svc");
        install_fake_bindings_package(&app_dir).await;
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

        let staged = stage_native_addon(&app_dir, BinaryTarget::LinuxX64, "svc")
            .await
            .unwrap()
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
        install_fake_bindings_package(&app_dir).await;

        let staged = stage_native_addon(&app_dir, BinaryTarget::LinuxX64, "svc")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fs::read(&staged).await.unwrap(), b"workspace-dev-addon");
    }

    #[tokio::test]
    async fn staging_failure_names_the_missing_prebuild_package() {
        let app = tempdir().unwrap();
        install_fake_bindings_package(app.path()).await;

        // Cross target (never the host triple), no prebuild installed, and no
        // workspace crate above the temp dir — no addon source exists.
        let target = if BinaryTarget::current_os() == BinaryTarget::LinuxArm64 {
            BinaryTarget::LinuxX64
        } else {
            BinaryTarget::LinuxArm64
        };
        let triple = napi_triple(target).unwrap();

        let error = stage_native_addon(app.path(), target, "app")
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
        install_fake_bindings_package(app.path()).await;

        let error = stage_native_addon(app.path(), BinaryTarget::WindowsX64, "app")
            .await
            .expect_err("windows has no native addon");
        assert!(
            error.to_string().contains("windows-x64"),
            "error should name the unsupported target, got: {error}"
        );
    }
}
