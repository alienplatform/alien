use super::native_addon::{stage_native_addon, AddonResolutionRoute};
use super::{cache_utils, Toolchain, ToolchainContext, ToolchainOutput, WorkloadKind};
use crate::command_output::wait_with_captured_output;
use crate::dependencies::install_dependencies;
use crate::error::{ErrorData, Result};
use alien_core::AlienEvent;
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
__NATIVE_INSTALL__import * as userModule from "__USER_ENTRY__"
import { runWorker } from "@alienplatform/sdk/worker-runtime"

runWorker(userModule).catch((error) => {
  console.error("Alien bootstrap error:", error)
  process.exit(1)
})
"#;

/// Registers the bun-embedded bindings addon with the default loader before the
/// app runs, so a plain `import { kv } from "@alienplatform/bindings"` (directly
/// or re-exported through the SDK) resolves inside the single-file binary, which
/// has no prebuild package or dev checkout for the loader's normal resolution to
/// walk. An explicit call — not a bare side-effect import — so it survives the
/// `sideEffects: false` tree-shaking of the packages it flows through.
///
/// Two variants, because the specifier must be resolvable from the compiled
/// entry's location:
///
/// - **Workers** depend only on `@alienplatform/sdk`; `@alienplatform/bindings`
///   is transitive through it, so the Worker bootstrap cannot resolve
///   `@alienplatform/bindings/native` directly. It goes through the SDK's
///   `./native` bridge, which re-exports `installEmbeddedAddon` from the
///   bindings package it *can* resolve.
/// - **Containers/Daemons** usually depend on `@alienplatform/bindings`
///   directly and import `@alienplatform/bindings/native`; when one gets its
///   bindings through the SDK instead (only `@alienplatform/sdk` in its
///   dependencies), the wrapper must go through the SDK bridge exactly like a
///   Worker — the direct specifier would not resolve. The staging step reports
///   which route the app resolves (see `AddonResolutionRoute`).
const WORKER_NATIVE_INSTALL_SNIPPET: &str =
    "import { installEmbeddedAddon } from \"@alienplatform/sdk/native\"\ninstallEmbeddedAddon()\n";
const DIRECT_NATIVE_INSTALL_SNIPPET: &str =
    "import { installEmbeddedAddon } from \"@alienplatform/bindings/native\"\ninstallEmbeddedAddon()\n";

/// Rewrite a user entry point (relative to `src_dir`) into an import specifier
/// usable from a generated file under `src_dir/.alien-build/`.
fn bootstrap_relative_import(user_entry_point: &str) -> String {
    let stripped = user_entry_point
        .strip_prefix("./")
        .unwrap_or(user_entry_point);
    format!("../{stripped}")
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
        embed_native: bool,
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

        // Convert user entry point to a relative path from the bootstrap file
        // location (src_dir/.alien-build/__alien_bootstrap.ts): bun resolves
        // imports relative to the entry file, so go up one level to src_dir.
        let user_import_path = bootstrap_relative_import(user_entry_point);

        // Generate the bootstrap code by replacing the placeholders. The native
        // install line is present only when the binary embeds the addon, and a
        // Worker reaches it through the SDK's `./native` bridge (the Worker
        // cannot resolve `@alienplatform/bindings/native` directly).
        let native_install = if embed_native {
            WORKER_NATIVE_INSTALL_SNIPPET
        } else {
            ""
        };
        let bootstrap_code = BOOTSTRAP_TEMPLATE
            .replace("__NATIVE_INSTALL__", native_install)
            .replace("__USER_ENTRY__", &user_import_path);

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

    /// Generate a thin entry wrapper for a source-built Container/Daemon whose
    /// binary embeds the bindings addon. It registers the embedded addon, then
    /// imports the user's entry point (which runs exactly as it would compiled
    /// directly). Used only when an addon is staged; otherwise the user entry
    /// is compiled directly with no wrapper.
    async fn generate_direct_entry_wrapper(
        &self,
        _src_dir: &Path,
        user_entry_point: &str,
        output_dir: &Path,
        addon_route: AddonResolutionRoute,
    ) -> Result<PathBuf> {
        fs::create_dir_all(output_dir)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: "typescript-project".to_string(),
                reason: "Failed to create entry-wrapper directory".to_string(),
                build_output: None,
            })?;

        let user_import_path = bootstrap_relative_import(user_entry_point);
        // Import the embedded-addon installer through the package the app can
        // actually resolve: bindings when it is a direct dependency, otherwise
        // the SDK's ./native bridge (an SDK-only Container/Daemon).
        let native_install = match addon_route {
            AddonResolutionRoute::DirectBindings => DIRECT_NATIVE_INSTALL_SNIPPET,
            AddonResolutionRoute::ViaSdk => WORKER_NATIVE_INSTALL_SNIPPET,
        };
        let wrapper_code = format!("{native_install}import \"{user_import_path}\"\n");

        let wrapper_path = output_dir.join("__alien_entry.ts");
        fs::write(&wrapper_path, &wrapper_code)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: "typescript-project".to_string(),
                reason: "Failed to write entry wrapper".to_string(),
                build_output: None,
            })?;

        info!(
            "Generated direct-entry wrapper at {} (importing {})",
            wrapper_path.display(),
            user_import_path
        );

        Ok(wrapper_path)
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
        // Stage the TARGET platform's native bindings addon next to the
        // bindings package's dist/native.js (when the app has
        // @alienplatform/bindings installed) so bun embeds it into the binary.
        // Staged before choosing the compile entry: when an addon is embedded,
        // the compiled entry must pull in `@alienplatform/bindings/native` to
        // register it with the default loader — a compiled binary has no
        // prebuild package or dev checkout for the normal resolution to find.
        let staged = stage_native_addon(&src_dir, context.build_target, &binary_name).await?;
        let embed_native = staged.is_some();

        // Workers compile a generated bootstrap that hands the app to
        // `runWorker` (the binary runs behind alien-worker-runtime).
        // Containers and Daemons compile the user's entry point directly —
        // their binary is the image entrypoint and owns its own lifecycle —
        // except when an addon is embedded, where they get a thin wrapper that
        // registers it before importing the user entry.
        let (compile_entry, bootstrap_dir) = if context.workload == WorkloadKind::Worker {
            // .alien-build/ lives inside the source directory so bun can
            // resolve node_modules; it is cleaned up after the build.
            let bootstrap_dir = src_dir.join(".alien-build");
            let bootstrap_path = self
                .generate_bootstrap_wrapper(&src_dir, &entry_point, &bootstrap_dir, embed_native)
                .await?;
            (bootstrap_path, Some(bootstrap_dir))
        } else if let Some(staged_addon) = &staged {
            let route = &staged_addon.route;
            let bootstrap_dir = src_dir.join(".alien-build");
            let wrapper_path = self
                .generate_direct_entry_wrapper(&src_dir, &entry_point, &bootstrap_dir, *route)
                .await?;
            (wrapper_path, Some(bootstrap_dir))
        } else {
            (src_dir.join(entry_point.trim_start_matches("./")), None)
        };

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
        let use_cjs_format = embed_native;

        // Helper to clean up the generated bootstrap staged into the source
        // directory (Workers / wrapped entries). The staged native addon is
        // deliberately NOT removed: it lives at a shared singleton path
        // inside the bindings package's dist (`./alien-bindings.node`), and
        // concurrent builds — parallel containers in one stack, parallel
        // tests — embed it while another build may still be compiling.
        // Staging renames over it atomically, and dist is build output, so
        // leaving it in place is both safe and required.
        let cleanup_staged_files = |bootstrap_dir: Option<PathBuf>| async move {
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
        cleanup_staged_files(bootstrap_dir).await;

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
