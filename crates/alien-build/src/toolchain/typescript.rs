use super::{cache_utils, Toolchain, ToolchainContext, ToolchainOutput, WorkloadKind};
use crate::command_output::wait_with_captured_output;
use crate::dependencies::install_dependencies;
use crate::error::{ErrorData, Result};
use alien_core::{AlienEvent, BinaryTarget};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tokio::fs;
use tokio::process::Command;
use tracing::{error, info};

/// Bootstrap wrapper template for TypeScript **Worker** applications.
///
/// Only Workers get a generated bootstrap: their compiled binary runs behind
/// `alien-worker-runtime`. Container and Daemon builds compile the user's
/// entry point directly — the binary is the image entrypoint and owns its own
/// process lifecycle (e.g. `export default { fetch }` is auto-served by Bun on
/// `PORT`).
///
/// This wrapper imports the user's module (which registers command/event
/// handlers as an import side effect) and hands its default export to
/// `runWorker`. `runWorker` (in `@alienplatform/sdk/worker-runtime`) owns the
/// Worker protocol wiring: it connects to the runtime, detects an HTTP `fetch`
/// handler and serves it on `127.0.0.1` (always loopback — the runtime/agent
/// is co-located in the same container or host, and registers the dynamic
/// port over gRPC), then registers the app's handlers and enters the
/// task-dispatch loop — draining `waitUntil` work on shutdown.
const BOOTSTRAP_TEMPLATE: &str = r#"/**
 * Alien Bootstrap - Auto-generated wrapper for TypeScript applications.
 * DO NOT EDIT - This file is generated during the build process.
 */
import * as userModule from "__USER_ENTRY__"
import { runWorker } from "@alienplatform/sdk/worker-runtime"

runWorker(userModule).catch((error) => {
  console.error("Alien bootstrap error:", error)
  process.exit(1)
})
"#;

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

/// Per-source-directory build locks.
///
/// Multi-target builds share one source directory but run in parallel. Both
/// the generated bootstrap (`.alien-build/`) and the staged native addon live
/// *inside* that shared directory — and the staged addon's bytes differ per
/// target — so the stage → compile → clean-up section must be serialized per
/// source directory.
static SRC_DIR_BUILD_LOCKS: OnceLock<StdMutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> =
    OnceLock::new();

fn src_dir_build_lock(src_dir: &Path) -> Arc<tokio::sync::Mutex<()>> {
    let map = SRC_DIR_BUILD_LOCKS.get_or_init(Default::default);
    let mut guard = map.lock().expect("source directory lock map poisoned");
    guard.entry(src_dir.to_path_buf()).or_default().clone()
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
async fn stage_native_addon(
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

/// TypeScript toolchain implementation using `bun build --compile` to create single executables.
///
/// This toolchain:
/// 1. Installs dependencies using the detected package manager (bun, pnpm, npm)
/// 2. Generates a bootstrap wrapper that handles HTTP server registration
/// 3. Compiles to a single executable using `bun build --compile`
/// 4. Packages only the compiled binary (no node_modules, no dist/)
#[derive(Debug, Clone)]
pub struct TypeScriptToolchain {
    /// Name of the compiled binary (e.g., "my-api"). If None, derived from package.json name.
    pub binary_name: Option<String>,
}

impl TypeScriptToolchain {
    fn absolute_path(path: &Path) -> Result<PathBuf> {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }

        let current_dir =
            std::env::current_dir()
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: "typescript-project".to_string(),
                    reason: "Failed to resolve current directory".to_string(),
                    build_output: None,
                })?;

        Ok(current_dir.join(path))
    }

    /// Check if the source directory contains a valid TypeScript/JavaScript project
    pub fn is_typescript_project(src_dir: &Path) -> bool {
        src_dir.join("package.json").exists()
    }

    /// Get the binary name, either from config or from package.json
    async fn get_binary_name(&self, src_dir: &Path) -> Result<String> {
        if let Some(ref name) = self.binary_name {
            return Ok(name.clone());
        }

        // Read from package.json
        let package_json_path = src_dir.join("package.json");
        let content = fs::read_to_string(&package_json_path)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: "typescript-project".to_string(),
                reason: "Failed to read package.json".to_string(),
                build_output: None,
            })?;

        let package_json: Value = serde_json::from_str(&content).into_alien_error().context(
            ErrorData::ImageBuildFailed {
                resource_name: "typescript-project".to_string(),
                reason: "Failed to parse package.json".to_string(),
                build_output: None,
            },
        )?;

        let name = package_json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("server")
            .to_string();

        // Sanitize the name for use as a binary (replace @ and / with -)
        let sanitized = name
            .trim_start_matches('@')
            .replace('/', "-")
            .replace(' ', "-");

        Ok(sanitized)
    }

    /// Detect entry point from package.json or use defaults
    async fn detect_entry_point(&self, src_dir: &Path) -> Result<String> {
        let package_json_path = src_dir.join("package.json");
        let content = fs::read_to_string(&package_json_path)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: "typescript-project".to_string(),
                reason: "Failed to read package.json".to_string(),
                build_output: None,
            })?;

        let package_json: Value = serde_json::from_str(&content).into_alien_error().context(
            ErrorData::ImageBuildFailed {
                resource_name: "typescript-project".to_string(),
                reason: "Failed to parse package.json".to_string(),
                build_output: None,
            },
        )?;

        // Try various entry point fields
        let candidates = [
            package_json.get("main").and_then(|v| v.as_str()),
            package_json.get("module").and_then(|v| v.as_str()),
            package_json.get("source").and_then(|v| v.as_str()),
        ];

        for candidate in candidates.iter().flatten() {
            let path = src_dir.join(candidate);
            if path.exists() {
                return Ok(candidate.to_string());
            }
        }

        // Check common default locations
        let defaults = [
            "./src/index.ts",
            "./src/index.js",
            "./index.ts",
            "./index.js",
        ];
        for default in defaults {
            let path = src_dir.join(default);
            if path.exists() {
                return Ok(default.to_string());
            }
        }

        Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: "typescript-project".to_string(),
            reason:
                "Could not detect entry point. Set 'main' in package.json or create src/index.ts"
                    .to_string(),
            build_output: None,
        }))
    }

    /// Get lock file patterns for cache key generation
    fn get_lock_file_patterns() -> &'static [&'static str] {
        &[
            "**/package-lock.json",
            "**/pnpm-lock.yaml",
            "**/bun.lock",
            "**/bun.lockb",
        ]
    }

    /// Get cache paths for TypeScript builds (dependencies only)
    fn get_cache_paths(&self, src_dir: &Path) -> Vec<PathBuf> {
        vec![src_dir.join("node_modules")]
    }

    /// Generate cache key from lock files and package.json
    async fn generate_cache_key(
        &self,
        context: &ToolchainContext,
        src_dir: &Path,
    ) -> Result<String> {
        let mut patterns = Self::get_lock_file_patterns().to_vec();
        patterns.push("**/package.json");

        let lock_hash = cache_utils::hash_files(&patterns, src_dir).await?;

        Ok(format!(
            "{}-typescript-{}",
            lock_hash,
            context.build_target.runtime_platform_id()
        ))
    }

    /// Generate the bootstrap wrapper file that hands the user's module to `runWorker`.
    ///
    /// The wrapper imports the user's module and calls `runWorker` from
    /// `@alienplatform/sdk/worker-runtime`, which detects an `export default` with
    /// a `fetch` method, starts Bun.serve(), registers with the runtime, and
    /// enters the event loop.
    async fn generate_bootstrap_wrapper(
        &self,
        _src_dir: &Path,
        user_entry_point: &str,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        // Create the bootstrap directory
        fs::create_dir_all(output_dir)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: "typescript-project".to_string(),
                reason: "Failed to create bootstrap directory".to_string(),
                build_output: None,
            })?;

        // Convert user entry point to a relative path from the bootstrap file location.
        // The bootstrap file is at: src_dir/.alien-build/__alien_bootstrap.ts
        // User entry is relative to src_dir (e.g., "./src/index.ts")
        //
        // We need to generate an import path that works from the bootstrap location.
        // Since bun build --compile resolves imports relative to the entry file,
        // we go up one level from .alien-build/ back to src_dir, then to the user entry.
        let user_import_path = if user_entry_point.starts_with("./") {
            format!("../{}", &user_entry_point[2..])
        } else if user_entry_point.starts_with("../") {
            format!("../{}", user_entry_point)
        } else {
            format!("../{}", user_entry_point)
        };

        // Generate the bootstrap code by replacing the placeholder
        let bootstrap_code = BOOTSTRAP_TEMPLATE.replace("__USER_ENTRY__", &user_import_path);

        // Write the bootstrap file
        let bootstrap_path = output_dir.join("__alien_bootstrap.ts");
        fs::write(&bootstrap_path, &bootstrap_code)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: "typescript-project".to_string(),
                reason: "Failed to write bootstrap wrapper".to_string(),
                build_output: None,
            })?;

        info!(
            "Generated bootstrap wrapper at {} (importing {})",
            bootstrap_path.display(),
            user_import_path
        );

        Ok(bootstrap_path)
    }
}

#[async_trait]
impl Toolchain for TypeScriptToolchain {
    fn validate_source(&self, src_dir: &Path, resource_name: &str) -> Result<()> {
        if !Self::is_typescript_project(src_dir) {
            return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                resource_id: resource_name.to_string(),
                reason: "Source directory does not contain package.json".to_string(),
            }));
        }
        Ok(())
    }

    async fn build(&self, context: &ToolchainContext) -> Result<ToolchainOutput> {
        info!("Building TypeScript project with bun build --compile");

        let src_dir = Self::absolute_path(&context.src_dir)?;
        let build_dir = Self::absolute_path(&context.build_dir)?;

        // Get binary name and entry point
        let binary_name = self.get_binary_name(&src_dir).await?;
        let entry_point = self.detect_entry_point(&src_dir).await?;

        info!("Binary name: {}, Entry point: {}", binary_name, entry_point);

        // Generate cache key and setup cache paths
        let cache_key = self.generate_cache_key(context, &src_dir).await?;
        let cache_paths = self.get_cache_paths(&src_dir);

        info!("Using cache key: {}", cache_key);

        // Try to restore cache (node_modules only)
        let cache_restored =
            cache_utils::restore_cache(context.cache_store.as_deref(), &cache_key, &cache_paths)
                .await?;

        // Install dependencies only if not cached
        if cache_restored {
            info!("Skipping dependency installation (restored from cache)");
        } else {
            install_dependencies(&src_dir)
                .await
                .context(ErrorData::ImageBuildFailed {
                    resource_name: binary_name.clone(),
                    reason: "Failed to install dependencies".to_string(),
                    build_output: None,
                })?;
        }

        // Ensure the final build output directory exists
        fs::create_dir_all(&build_dir)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: binary_name.clone(),
                reason: "Failed to create build output directory".to_string(),
                build_output: None,
            })?;

        // Serialize the stage → compile → clean-up section per source
        // directory: parallel multi-target builds share the same src_dir, and
        // both the generated bootstrap and the staged native addon (whose
        // bytes differ per target!) live inside it.
        let build_lock = src_dir_build_lock(&src_dir);
        let _build_guard = build_lock.lock().await;

        // Workers compile a generated bootstrap that hands the app to
        // `runWorker` (the binary runs behind alien-worker-runtime).
        // Containers and Daemons compile the user's entry point directly —
        // their binary is the image entrypoint and owns its own lifecycle.
        let (compile_entry, bootstrap_dir) = if context.workload == WorkloadKind::Worker {
            // .alien-build/ lives inside the source directory so bun can
            // resolve node_modules; it is cleaned up after the build.
            let bootstrap_dir = src_dir.join(".alien-build");
            let bootstrap_path = self
                .generate_bootstrap_wrapper(&src_dir, &entry_point, &bootstrap_dir)
                .await?;
            (bootstrap_path, Some(bootstrap_dir))
        } else {
            (src_dir.join(entry_point.trim_start_matches("./")), None)
        };

        // Stage the TARGET platform's native bindings addon next to the
        // bindings package's dist/native.js (when the app has
        // @alienplatform/bindings installed) so bun embeds it into the binary.
        let staged_addon = stage_native_addon(&src_dir, context.build_target, &binary_name).await?;

        // Binary is output directly to the proper build directory (not inside source).
        // On Windows, bun appends .exe to the outfile path automatically.
        let binary_filename = format!("{}{}", binary_name, context.build_target.binary_extension());
        let binary_path = build_dir.join(&binary_filename);

        // Build bun compile arguments based on target
        let target_arg = context.build_target.bun_target();

        info!(
            "Compiling with: bun build --compile --no-compile-autoload-dotenv --no-compile-autoload-bunfig --target {} --outfile {} {}",
            target_arg,
            binary_path.display(),
            compile_entry.display()
        );

        // Clone values for use in async block
        let binary_name_clone = binary_name.clone();
        let binary_path_clone = binary_path.clone();
        let target_arg_clone = target_arg.to_string();
        let compile_entry_str = compile_entry.to_string_lossy().to_string();
        let use_cjs_format = staged_addon.is_some();

        // Helper to clean up build-time files staged into the source
        // directory: the generated bootstrap (Workers) and the staged native
        // addon (bindings apps).
        let cleanup_staged_files =
            |bootstrap_dir: Option<PathBuf>, staged_addon: Option<PathBuf>| async move {
                if let Some(bootstrap_dir) = bootstrap_dir {
                    if bootstrap_dir.exists() {
                        if let Err(e) = fs::remove_dir_all(&bootstrap_dir).await {
                            tracing::debug!(
                                "Failed to clean up bootstrap directory {}: {}",
                                bootstrap_dir.display(),
                                e
                            );
                        } else {
                            info!(
                                "Cleaned up bootstrap directory: {}",
                                bootstrap_dir.display()
                            );
                        }
                    }
                }
                if let Some(staged_addon) = staged_addon {
                    if let Err(e) = fs::remove_file(&staged_addon).await {
                        tracing::debug!(
                            "Failed to clean up staged native addon {}: {}",
                            staged_addon.display(),
                            e
                        );
                    } else {
                        info!("Cleaned up staged native addon: {}", staged_addon.display());
                    }
                }
            };

        let build_result = AlienEvent::CompilingCode {
            language: "typescript".to_string(),
            progress: None,
        }
        .in_scope(|compilation_event| async move {
            let mut args = vec![
                "build",
                "--compile",
                // Disable automatic config loading for security and deterministic behavior
                // See: https://bun.com/docs/bundler/executables#disabling-config-loading-at-runtime
                "--no-compile-autoload-dotenv",
                "--no-compile-autoload-bunfig",
                "--target",
                &target_arg_clone,
            ];
            // `bun build --compile` mis-generates the ESM loader shim for a
            // statically imported .node addon: the binary embeds the addon but
            // crashes on load with "ReferenceError: __require is not defined".
            // --format=cjs is the verified workaround (see
            // packages/bindings/scripts/compile-smoke.ts), applied only when
            // an addon is staged so plain apps keep the default ESM output.
            if use_cjs_format {
                args.push("--format=cjs");
            }
            args.push("--outfile");
            let binary_path_str = binary_path_clone.to_string_lossy();
            args.push(&binary_path_str);
            args.push(&compile_entry_str);

            let mut child = Command::new("bun")
                .args(&args)
                .current_dir(&src_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: binary_name_clone.clone(),
                    reason: "Failed to execute bun build --compile. Is Bun installed?".to_string(),
                    build_output: None,
                })?;

            let (output, captured_output) = wait_with_captured_output(
                &mut child,
                &binary_name_clone,
                "Failed to read bun build output",
                "Failed to wait for bun build --compile",
                |line| {
                    let compilation_event = &compilation_event;
                    async move {
                        let trimmed_line = line.line.trim();
                        if !trimmed_line.is_empty() {
                            let _ = compilation_event
                                .update(AlienEvent::CompilingCode {
                                    language: "typescript".to_string(),
                                    progress: Some(trimmed_line.to_string()),
                                })
                                .await;
                        }
                    }
                },
            )
            .await?;

            if !output.success() {
                let build_output = captured_output.display();
                error!(
                    binary = %binary_name_clone,
                    "bun build --compile failed. Build output:\n{}",
                    build_output
                );
                return Err(AlienError::new(ErrorData::ImageBuildFailed {
                    resource_name: binary_name_clone.clone(),
                    reason: "bun build --compile failed".to_string(),
                    build_output: Some(build_output),
                }));
            }

            info!("bun build --compile completed successfully");
            Ok(())
        })
        .await;

        // Always clean up staged files from the source directory, even if the build failed
        cleanup_staged_files(bootstrap_dir, staged_addon).await;

        // Now propagate any build error
        build_result?;

        // Verify binary was created
        if !binary_path.exists() {
            return Err(AlienError::new(ErrorData::ImageBuildFailed {
                resource_name: binary_name.clone(),
                reason: format!("Compiled binary not found at {}", binary_path.display()),
                build_output: None,
            }));
        }
        super::validate_executable_format(&binary_path, context.build_target, &binary_name)?;

        info!(
            "Successfully compiled TypeScript to single executable: {}",
            binary_path.display()
        );

        // Save updated cache
        cache_utils::save_cache(context.cache_store.as_deref(), &cache_key, &cache_paths).await?;

        // Image shape (runtime for Workers, direct entrypoint for
        // Containers/Daemons) is decided per workload kind in
        // `image_output_for_binary`.
        Ok(super::image_output_for_binary(
            context,
            binary_path,
            &binary_filename,
            vec![],
        ))
    }

    fn dev_command(&self, _src_dir: &Path) -> Vec<String> {
        // For development, use bun run dev
        vec!["bun".to_string(), "run".to_string(), "dev".to_string()]
    }
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

    #[test]
    fn test_is_typescript_project() {
        let temp_dir = tempdir().unwrap();

        // Not a TypeScript project initially
        assert!(!TypeScriptToolchain::is_typescript_project(temp_dir.path()));

        // Create package.json to make it a TypeScript project
        std::fs::write(
            temp_dir.path().join("package.json"),
            r#"{"name": "test", "version": "1.0.0"}"#,
        )
        .unwrap();
        assert!(TypeScriptToolchain::is_typescript_project(temp_dir.path()));
    }

    #[test]
    fn test_absolute_path_resolves_relative_paths() {
        let relative = Path::new("relative-project");
        let resolved = TypeScriptToolchain::absolute_path(relative).unwrap();

        assert!(resolved.is_absolute());
        assert!(resolved.ends_with(relative));
    }

    #[tokio::test]
    async fn test_get_binary_name_from_config() {
        let toolchain = TypeScriptToolchain {
            binary_name: Some("my-custom-binary".to_string()),
        };

        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join("package.json"),
            r#"{"name": "ignored-name"}"#,
        )
        .await
        .unwrap();

        let name = toolchain.get_binary_name(temp_dir.path()).await.unwrap();
        assert_eq!(name, "my-custom-binary");
    }

    #[tokio::test]
    async fn test_get_binary_name_from_package_json() {
        let toolchain = TypeScriptToolchain { binary_name: None };

        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join("package.json"),
            r#"{"name": "my-api-server"}"#,
        )
        .await
        .unwrap();

        let name = toolchain.get_binary_name(temp_dir.path()).await.unwrap();
        assert_eq!(name, "my-api-server");
    }

    #[tokio::test]
    async fn test_get_binary_name_sanitizes_scoped_packages() {
        let toolchain = TypeScriptToolchain { binary_name: None };

        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join("package.json"),
            r#"{"name": "@myorg/my-package"}"#,
        )
        .await
        .unwrap();

        let name = toolchain.get_binary_name(temp_dir.path()).await.unwrap();
        assert_eq!(name, "myorg-my-package");
    }

    #[tokio::test]
    async fn test_detect_entry_point_from_main() {
        let toolchain = TypeScriptToolchain { binary_name: None };

        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join("package.json"),
            r#"{"name": "test", "main": "./src/server.ts"}"#,
        )
        .await
        .unwrap();

        // Create the entry point file
        fs::create_dir_all(temp_dir.path().join("src"))
            .await
            .unwrap();
        fs::write(temp_dir.path().join("src/server.ts"), "")
            .await
            .unwrap();

        let entry = toolchain.detect_entry_point(temp_dir.path()).await.unwrap();
        assert_eq!(entry, "./src/server.ts");
    }

    #[tokio::test]
    async fn test_detect_entry_point_default() {
        let toolchain = TypeScriptToolchain { binary_name: None };

        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join("package.json"), r#"{"name": "test"}"#)
            .await
            .unwrap();

        // Create default entry point
        fs::create_dir_all(temp_dir.path().join("src"))
            .await
            .unwrap();
        fs::write(temp_dir.path().join("src/index.ts"), "")
            .await
            .unwrap();

        let entry = toolchain.detect_entry_point(temp_dir.path()).await.unwrap();
        assert_eq!(entry, "./src/index.ts");
    }

    #[test]
    fn test_dev_command() {
        let toolchain = TypeScriptToolchain { binary_name: None };

        let cmd = toolchain.dev_command(Path::new("./"));
        assert_eq!(cmd, vec!["bun", "run", "dev"]);
    }
}
