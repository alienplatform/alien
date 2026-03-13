use super::{cache_utils, Toolchain, ToolchainContext, ToolchainOutput};
use crate::dependencies::install_dependencies;
use crate::error::{ErrorData, Result};
use alien_core::AlienEvent;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::info;

/// Bootstrap wrapper template for TypeScript applications.
///
/// This wrapper:
/// 1. Imports the user's module
/// 2. Detects if the default export has a `fetch` method (Hono, Elysia, etc.)
/// 3. If so, starts Bun.serve() on a random port and registers with the runtime
/// 4. Enters the event loop via ctx.run()
const BOOTSTRAP_TEMPLATE: &str = r#"/**
 * Alien Bootstrap - Auto-generated wrapper for TypeScript applications.
 * DO NOT EDIT - This file is generated during the build process.
 */
import userModule from "__USER_ENTRY__"
import { AlienContext } from "@aliendotdev/bindings"

async function __alienBootstrap() {
  // Create context from environment (connects to runtime via gRPC)
  const ctx = await AlienContext.fromEnv()

  // Detect default export with fetch method (Hono, Elysia, Express adapter, etc.)
  const defaultExport = userModule?.default ?? userModule
  
  if (defaultExport && typeof defaultExport === "object" && "fetch" in defaultExport) {
    const fetchHandler = typeof defaultExport.fetch === "function" 
      ? defaultExport.fetch.bind(defaultExport)
      : defaultExport.fetch

    const server = Bun.serve({
      fetch: fetchHandler,
      port: 0, // Random available port
    })

    await ctx.registerHttpServer(server.port)
    // console.log(`HTTP server started on port ${server.port}`)
  }

  // Enter the event loop (handles events, commands, and keeps process alive)
  await ctx.run()
}

__alienBootstrap().catch((error) => {
  console.error("Alien bootstrap error:", error)
  process.exit(1)
})
"#;

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
                function_name: "typescript-project".to_string(),
                reason: "Failed to read package.json".to_string(),
                build_output: None,
            })?;

        let package_json: Value = serde_json::from_str(&content).into_alien_error().context(
            ErrorData::ImageBuildFailed {
                function_name: "typescript-project".to_string(),
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
                function_name: "typescript-project".to_string(),
                reason: "Failed to read package.json".to_string(),
                build_output: None,
            })?;

        let package_json: Value = serde_json::from_str(&content).into_alien_error().context(
            ErrorData::ImageBuildFailed {
                function_name: "typescript-project".to_string(),
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
            function_name: "typescript-project".to_string(),
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
    fn get_cache_paths(&self, context: &ToolchainContext) -> Vec<PathBuf> {
        vec![context.src_dir.join("node_modules")]
    }

    /// Generate cache key from lock files and package.json
    async fn generate_cache_key(&self, context: &ToolchainContext) -> Result<String> {
        let mut patterns = Self::get_lock_file_patterns().to_vec();
        patterns.push("**/package.json");

        let lock_hash = cache_utils::hash_files(&patterns, &context.src_dir).await?;

        Ok(format!(
            "{}-typescript-{}",
            lock_hash,
            context.build_target.runtime_platform_id()
        ))
    }

    /// Generate the bootstrap wrapper file that handles automatic HTTP server registration.
    ///
    /// The wrapper imports the user's module, detects `export default` with a `fetch` method,
    /// starts Bun.serve() on a random port, registers with the runtime, and enters the event loop.
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
                function_name: "typescript-project".to_string(),
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
                function_name: "typescript-project".to_string(),
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
    async fn build(&self, context: &ToolchainContext) -> Result<ToolchainOutput> {
        info!("Building TypeScript project with bun build --compile");

        // Validate that this is a TypeScript/JavaScript project
        if !Self::is_typescript_project(&context.src_dir) {
            return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                resource_id: "typescript-project".to_string(),
                reason: "Source directory does not contain package.json".to_string(),
            }));
        }

        // Get binary name and entry point
        let binary_name = self.get_binary_name(&context.src_dir).await?;
        let entry_point = self.detect_entry_point(&context.src_dir).await?;

        info!("Binary name: {}, Entry point: {}", binary_name, entry_point);

        // Generate cache key and setup cache paths
        let cache_key = self.generate_cache_key(context).await?;
        let cache_paths = self.get_cache_paths(context);

        info!("Using cache key: {}", cache_key);

        // Try to restore cache (node_modules only)
        let cache_restored =
            cache_utils::restore_cache(context.cache_store.as_deref(), &cache_key, &cache_paths)
                .await?;

        // Install dependencies only if not cached
        if cache_restored {
            info!("Skipping dependency installation (restored from cache)");
        } else {
            install_dependencies(&context.src_dir)
                .await
                .context(ErrorData::ImageBuildFailed {
                    function_name: binary_name.clone(),
                    reason: "Failed to install dependencies".to_string(),
                    build_output: None,
                })?;
        }

        // Create .alien-build/ inside source directory for the bootstrap file.
        // This location allows bun to resolve node_modules from the source directory.
        // The bootstrap will be cleaned up after the build completes.
        let bootstrap_dir = context.src_dir.join(".alien-build");
        fs::create_dir_all(&bootstrap_dir)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                function_name: binary_name.clone(),
                reason: "Failed to create bootstrap directory".to_string(),
                build_output: None,
            })?;

        // Ensure the final build output directory exists
        fs::create_dir_all(&context.build_dir)
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                function_name: binary_name.clone(),
                reason: "Failed to create build output directory".to_string(),
                build_output: None,
            })?;

        // Generate bootstrap wrapper that handles automatic HTTP server registration
        let bootstrap_path = self
            .generate_bootstrap_wrapper(&context.src_dir, &entry_point, &bootstrap_dir)
            .await?;

        // Binary is output directly to the proper build directory (not inside source)
        let binary_path = context.build_dir.join(&binary_name);

        // Build bun compile arguments based on target
        let target_arg = context.build_target.bun_target();

        // Compile the bootstrap wrapper (which imports the user's entry point)
        info!(
            "Compiling with: bun build --compile --no-compile-autoload-dotenv --no-compile-autoload-bunfig --target {} --outfile {} {}",
            target_arg,
            binary_path.display(),
            bootstrap_path.display()
        );

        // Clone values for use in async block
        let binary_name_clone = binary_name.clone();
        let binary_path_clone = binary_path.clone();
        let target_arg_clone = target_arg.to_string();
        let bootstrap_path_str = bootstrap_path.to_string_lossy().to_string();

        // Helper to clean up .alien-build/ from source directory
        let cleanup_bootstrap = |bootstrap_dir: PathBuf| async move {
            if bootstrap_dir.exists() {
                if let Err(e) = fs::remove_dir_all(&bootstrap_dir).await {
                    tracing::warn!(
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
                "--outfile",
            ];
            let binary_path_str = binary_path_clone.to_string_lossy();
            args.push(&binary_path_str);
            args.push(&bootstrap_path_str);

            let mut child = Command::new("bun")
                .args(&args)
                .current_dir(&context.src_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    function_name: binary_name_clone.clone(),
                    reason: "Failed to execute bun build --compile. Is Bun installed?".to_string(),
                    build_output: None,
                })?;

            // Read stderr for progress
            let stderr = child.stderr.take().unwrap();
            let mut stderr_reader = BufReader::new(stderr).lines();
            let mut stderr_lines = Vec::new();

            while let Some(line) = stderr_reader.next_line().await.into_alien_error().context(
                ErrorData::ImageBuildFailed {
                    function_name: binary_name_clone.clone(),
                    reason: "Failed to read bun build output".to_string(),
                    build_output: None,
                },
            )? {
                stderr_lines.push(line.clone());

                let trimmed_line = line.trim();
                if !trimmed_line.is_empty() {
                    let _ = compilation_event
                        .update(AlienEvent::CompilingCode {
                            language: "typescript".to_string(),
                            progress: Some(trimmed_line.to_string()),
                        })
                        .await;
                }
            }

            let output =
                child
                    .wait()
                    .await
                    .into_alien_error()
                    .context(ErrorData::ImageBuildFailed {
                        function_name: binary_name_clone.clone(),
                        reason: "Failed to wait for bun build --compile".to_string(),
                        build_output: None,
                    })?;

            if !output.success() {
                let stderr_output = stderr_lines.join("\n");
                return Err(AlienError::new(ErrorData::ImageBuildFailed {
                    function_name: binary_name_clone.clone(),
                    reason: "bun build --compile failed".to_string(),
                    build_output: Some(stderr_output),
                }));
            }

            info!("bun build --compile completed successfully");
            Ok(())
        })
        .await;

        // Always clean up .alien-build/ from source directory, even if build failed
        cleanup_bootstrap(bootstrap_dir).await;

        // Now propagate any build error
        build_result?;

        // Verify binary was created
        if !binary_path.exists() {
            return Err(AlienError::new(ErrorData::ImageBuildFailed {
                function_name: binary_name.clone(),
                reason: format!("Compiled binary not found at {}", binary_path.display()),
                build_output: None,
            }));
        }

        info!(
            "Successfully compiled TypeScript to single executable: {}",
            binary_path.display()
        );

        // Save updated cache
        cache_utils::save_cache(context.cache_store.as_deref(), &cache_key, &cache_paths).await?;

        // Determine if we need alien-runtime in the image
        // Functions on local platform use embedded runtime in operator (no runtime in image)
        // Everything else (containers on any platform, functions on cloud) needs alien-runtime
        let needs_runtime_in_image = context.is_container || context.platform_name != "local";

        if !needs_runtime_in_image {
            // Function on local platform - runtime is embedded in operator
            let runtime_command = vec![format!("./{}", binary_name)];

            return Ok(ToolchainOutput {
                build_strategy: super::ImageBuildStrategy::FromScratch {
                    layers: vec![super::LayerSpec {
                        files: vec![super::FileSpec {
                            host_path: binary_path.clone(),
                            container_path: format!("./{}", binary_name),
                            mode: Some(0o755), // Executable
                        }],
                        description: "Compiled application binary".to_string(),
                    }],
                },
                runtime_command,
            });
        }

        // Need alien-runtime in the image (containers or cloud functions)
        // Use the universal alien-base image that includes alien-runtime with ENTRYPOINT
        let base_images = vec!["ghcr.io/alienplatform/alien-base:latest".to_string()];

        // Runtime command: -- separator required by alien-runtime CLI, then application binary
        // Base image ENTRYPOINT is ["/app/alien-runtime"] so CMD must start with "--"
        let runtime_command = vec!["--".to_string(), format!("./{}", binary_name)];

        Ok(ToolchainOutput {
            build_strategy: super::ImageBuildStrategy::FromBaseImage {
                base_images,
                files_to_package: vec![(binary_path, format!("./{}", binary_name))],
            },
            runtime_command,
        })
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
