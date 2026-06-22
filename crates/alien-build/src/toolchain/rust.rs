use super::{cache_utils, Toolchain, ToolchainContext, ToolchainOutput};
use crate::command_output::{image_build_error_with_output, wait_with_captured_output};
use crate::error::{ErrorData, Result};
use alien_core::AlienEvent;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;
use tokio::fs;
use tokio::process::Command;
use tracing::{error, info, warn};

/// Rust toolchain implementation using Cargo with Zig cross-compilation
#[derive(Debug, Clone)]
pub struct RustToolchain {
    /// Name of the binary to build and run
    pub binary_name: String,
}

struct CargoProjectMetadata {
    target_directory: PathBuf,
    workspace_root: PathBuf,
}

impl RustToolchain {
    /// Check if the source directory contains a valid Rust project
    pub fn is_rust_project(src_dir: &Path) -> bool {
        src_dir.join("Cargo.toml").exists()
    }

    /// Check if a cargo output line contains meaningful content
    /// Returns true for lines that start with alphanumeric characters or certain symbols
    /// Filters out decorative lines like arrows, pipes, help text, etc.
    fn is_meaningful_cargo_line(line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }

        // Filter out specific cargo formatting patterns
        if trimmed.starts_with("-->")
            || trimmed.starts_with("|")
            || trimmed.starts_with("= ")
            || trimmed.starts_with("^")
        {
            return false;
        }

        // Check if the first character indicates meaningful content
        match trimmed.chars().next() {
            Some(c) if c.is_alphanumeric() => true, // Letters and numbers
            Some('(') | Some('[') | Some('{') => true, // Opening brackets
            Some('"') | Some('\'') => true,         // Quoted strings
            Some('+') | Some('-') | Some('*') => true, // List markers (but not arrows)
            _ => false,                             // Filter out other decorative characters
        }
    }

    /// Expand tilde (~) in path to home directory
    fn expand_home_dir(&self, path: &str) -> PathBuf {
        if path.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(&path[2..]);
            }
        }
        PathBuf::from(path)
    }

    /// Get cache paths for Rust builds
    async fn get_cache_paths(&self, context: &ToolchainContext) -> Result<Vec<PathBuf>> {
        let target_dir = self.get_target_directory(&context.src_dir).await?;

        Ok(vec![
            self.expand_home_dir("~/.cargo/registry"), // Downloaded crate files
            self.expand_home_dir("~/.cargo/git"),      // Git dependencies
            target_dir,                                // Compiled artifacts (workspace-aware)
        ])
    }

    /// Generate cache key from Cargo.lock and build configuration
    async fn generate_cache_key(&self, context: &ToolchainContext) -> Result<String> {
        let cargo_metadata = self.get_cargo_metadata(&context.src_dir).await?;
        let cargo_lock_hash =
            cache_utils::hash_files(&["Cargo.lock"], &cargo_metadata.workspace_root).await?;
        let build_mode = if context.debug_mode {
            "debug"
        } else {
            "release"
        };

        Ok(format!(
            "{}-{}-{}-{}",
            cargo_lock_hash,
            context.build_target.rust_target_triple(),
            build_mode,
            self.binary_name
        ))
    }

    /// Get the target directory for the Rust project.
    /// In workspace scenarios, this returns the workspace target directory.
    /// Otherwise, it returns the project-local target directory.
    async fn get_target_directory(&self, src_dir: &Path) -> Result<PathBuf> {
        Ok(self.get_cargo_metadata(src_dir).await?.target_directory)
    }

    async fn get_cargo_metadata(&self, src_dir: &Path) -> Result<CargoProjectMetadata> {
        // Use cargo metadata to get the actual target directory
        let metadata_output = Command::new("cargo")
            .args(&["metadata", "--format-version", "1", "--no-deps"])
            .current_dir(src_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: self.binary_name.clone(),
                reason: "Failed to execute cargo metadata".to_string(),
                build_output: None,
            })?;

        if !metadata_output.status.success() {
            return Err(image_build_error_with_output(
                self.binary_name.clone(),
                "cargo metadata failed",
                &metadata_output,
            ));
        }

        let stdout = String::from_utf8_lossy(&metadata_output.stdout);
        let metadata: Value = serde_json::from_str(&stdout).into_alien_error().context(
            ErrorData::ImageBuildFailed {
                resource_name: self.binary_name.clone(),
                reason: "Failed to parse cargo metadata JSON".to_string(),
                build_output: None,
            },
        )?;

        let target_directory = metadata
            .get("target_directory")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ImageBuildFailed {
                    resource_name: self.binary_name.clone(),
                    reason: "cargo metadata missing target_directory field".to_string(),
                    build_output: None,
                })
            })?;
        let workspace_root = metadata
            .get("workspace_root")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ImageBuildFailed {
                    resource_name: self.binary_name.clone(),
                    reason: "cargo metadata missing workspace_root field".to_string(),
                    build_output: None,
                })
            })?;

        Ok(CargoProjectMetadata {
            target_directory: PathBuf::from(target_directory),
            workspace_root: PathBuf::from(workspace_root),
        })
    }
}

#[async_trait]
impl Toolchain for RustToolchain {
    async fn build(&self, context: &ToolchainContext) -> Result<ToolchainOutput> {
        let build_started = Instant::now();
        info!("Building Rust project with binary: {}", self.binary_name);

        // Validate that this is a Rust project
        if !Self::is_rust_project(&context.src_dir) {
            return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                resource_id: self.binary_name.clone(),
                reason: "Source directory does not contain Cargo.toml".to_string(),
            }));
        }

        // Generate cache key and setup cache paths
        let cache_key = self.generate_cache_key(context).await?;
        let cache_paths = self.get_cache_paths(context).await?;

        info!("Using cache key: {}", cache_key);

        // Try to restore cache if available
        cache_utils::restore_cache(context.cache_store.as_deref(), &cache_key, &cache_paths)
            .await?;

        // Determine the build strategy based on target and host OS.
        // - Linux musl: cargo zigbuild (zig provides the musl C toolchain)
        // - Windows MSVC from non-Windows: cargo xwin build (xwin provides MSVC CRT/SDK)
        // - Native (Windows on Windows, macOS on macOS): cargo build
        let strategy = context.build_target.cargo_build_strategy();

        // Install the cross-compilation tool if needed
        if let Some(package) = strategy.install_package() {
            let tool_binary = format!(
                "cargo-{}",
                package.strip_prefix("cargo-").unwrap_or(package)
            );
            let tool_path = self.expand_home_dir(&format!("~/.cargo/bin/{}", tool_binary));
            if !tool_path.exists() {
                info!("{} not found, installing...", package);
                let install_output = Command::new("cargo")
                    .args(&["install", package])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .await
                    .into_alien_error()
                    .context(ErrorData::ImageBuildFailed {
                        resource_name: self.binary_name.clone(),
                        reason: format!("Failed to execute cargo install {}", package),
                        build_output: None,
                    })?;

                if !install_output.status.success() {
                    return Err(image_build_error_with_output(
                        self.binary_name.clone(),
                        format!("Failed to install {}", package),
                        &install_output,
                    ));
                }
                info!("Successfully installed {}", package);
            } else {
                info!("{} already installed", package);
            }
        }

        // Check if target is installed, install if not present (still needed for std library)
        let list_targets_output = Command::new("rustup")
            .args(&["target", "list", "--installed"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: self.binary_name.clone(),
                reason: "Failed to execute rustup target list".to_string(),
                build_output: None,
            })?;

        let installed_targets = String::from_utf8_lossy(&list_targets_output.stdout);
        let target_installed = installed_targets
            .lines()
            .any(|line| line.trim() == context.build_target.rust_target_triple());

        if !target_installed {
            info!(
                "Target {} not found, installing...",
                context.build_target.rust_target_triple()
            );
            let install_target_output = Command::new("rustup")
                .args(&["target", "add", context.build_target.rust_target_triple()])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: self.binary_name.clone(),
                    reason: "Failed to execute rustup target add".to_string(),
                    build_output: None,
                })?;

            if !install_target_output.status.success() {
                return Err(image_build_error_with_output(
                    self.binary_name.clone(),
                    format!(
                        "Failed to install target {}",
                        context.build_target.rust_target_triple()
                    ),
                    &install_target_output,
                ));
            }
            info!(
                "Successfully installed target {}",
                context.build_target.rust_target_triple()
            );
        } else {
            info!(
                "Target {} already installed",
                context.build_target.rust_target_triple()
            );
        }

        // Determine the expected binary path before building so stale corrupt
        // artifacts from interrupted builds cannot be reused by Cargo.
        let target_dir_base = self.get_target_directory(&context.src_dir).await?;
        let target_subdir = if context.debug_mode {
            "debug"
        } else {
            "release"
        };
        let target_platform_dir = target_dir_base
            .join(context.build_target.rust_target_triple())
            .join(target_subdir);
        let binary_filename = format!(
            "{}{}",
            self.binary_name,
            context.build_target.binary_extension()
        );
        let binary_path = target_platform_dir.join(&binary_filename);

        if binary_path.exists() {
            if let Some(reason) = super::executable_format_error(&binary_path, context.build_target)
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: self.binary_name.clone(),
                    reason: format!(
                        "Failed to inspect existing binary at {}",
                        binary_path.display()
                    ),
                    build_output: None,
                })?
            {
                warn!(
                    binary = %binary_path.display(),
                    reason = %reason,
                    "Removing stale invalid Rust build artifact before rebuilding"
                );
                fs::remove_file(&binary_path)
                    .await
                    .into_alien_error()
                    .context(ErrorData::FileOperationFailed {
                        operation: "remove file".to_string(),
                        file_path: binary_path.display().to_string(),
                        reason: "Failed to remove stale invalid Rust build artifact".to_string(),
                    })?;
            }
        }

        // Build the project for the target platform.
        // Always pass --target for consistent output directory structure.
        let strategy_args = strategy.cargo_args();
        let mut args: Vec<&str> = strategy_args.iter().copied().collect();
        args.extend_from_slice(&[
            "--target",
            context.build_target.rust_target_triple(),
            "--bin",
            &self.binary_name,
        ]);

        // Add --release flag only in release mode
        if !context.debug_mode {
            args.push("--release");
        }

        let build_command = strategy.display_name();
        info!(
            "Running {} with args: {:?} in directory: {}",
            build_command,
            args,
            context.src_dir.display()
        );

        // Use in_scope for automatic event lifecycle management
        let compile_started = Instant::now();
        AlienEvent::CompilingCode {
            language: "rust".to_string(),
            progress: None,
        }
        .in_scope(|compilation_event| async move {
            let mut child = Command::new("cargo")
                .args(&args)
                .current_dir(&context.src_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: self.binary_name.clone(),
                    reason: format!("Failed to execute {}", build_command),
                    build_output: None,
                })?;

            let (output, captured_output) = wait_with_captured_output(
                &mut child,
                &self.binary_name,
                "Failed to read cargo build output",
                &format!("Failed to wait for {} completion", build_command),
                |line| {
                    let compilation_event = &compilation_event;
                    async move {
                        // Update the event with the latest line for progress.
                        // Only show meaningful cargo output, filtering out decorative lines.
                        if Self::is_meaningful_cargo_line(&line.line) {
                            let _ = compilation_event
                                .update(AlienEvent::CompilingCode {
                                    language: "rust".to_string(),
                                    progress: Some(line.line.trim().to_string()),
                                })
                                .await; // Ignore update errors to not fail the build
                        }
                    }
                },
            )
            .await?;

            if !output.success() {
                let build_output = captured_output.display();
                error!(
                    binary = %self.binary_name,
                    "{} failed. Build output:\n{}",
                    build_command, build_output
                );
                return Err(AlienError::new(ErrorData::ImageBuildFailed {
                    resource_name: self.binary_name.clone(),
                    reason: format!("{} failed", build_command),
                    build_output: Some(build_output),
                }));
            }

            info!("{} completed successfully", build_command);
            Ok(())
        })
        .await?;
        info!(
            "{} for binary '{}' target '{}' completed in {:.2}s",
            build_command,
            self.binary_name,
            context.build_target.rust_target_triple(),
            compile_started.elapsed().as_secs_f64()
        );

        // Verify the binary was built
        if !binary_path.exists() {
            return Err(AlienError::new(ErrorData::ImageBuildFailed {
                resource_name: self.binary_name.clone(),
                reason: format!(
                    "Expected binary not found at: {}. Target directory: {}",
                    binary_path.display(),
                    target_dir_base.display()
                ),
                build_output: None,
            }));
        }
        super::validate_executable_format(&binary_path, context.build_target, &self.binary_name)?;

        info!("Successfully built Rust binary: {}", binary_path.display());

        // Save updated cache only after validating the build output.
        cache_utils::save_cache(context.cache_store.as_deref(), &cache_key, &cache_paths).await?;

        // Determine if we need alien-runtime in the image
        // Local native resources use embedded runtime in the agent (no runtime in image)
        // Everything else (containers on any platform, functions on cloud) needs alien-runtime
        let needs_runtime_in_image =
            context.is_container || context.runtime_platform_name != "local";

        if !needs_runtime_in_image {
            // Local native resource - runtime is embedded in the agent
            // Package the application binary, and any extra assets the project
            // wants shipped alongside it. Convention: anything in a top-level
            // `vendor/` directory next to Cargo.toml gets copied into the
            // image under `/app/vendor/`. This is how a daemon can ship
            // helper binaries or data files it needs at runtime without
            // baking them into the Rust binary itself.
            let runtime_command = vec![format!("./{}", binary_filename)];

            let mut layers = vec![super::LayerSpec {
                files: vec![super::FileSpec {
                    host_path: binary_path.clone(),
                    container_path: format!("./{}", binary_filename),
                    mode: Some(0o755), // Executable
                }],
                description: "Application binary".to_string(),
            }];

            let vendor_dir = context.src_dir.join("vendor");
            if vendor_dir.is_dir() {
                info!(
                    "Including vendor directory in image: {}",
                    vendor_dir.display()
                );
                layers.push(super::LayerSpec {
                    files: vec![super::FileSpec {
                        host_path: vendor_dir,
                        container_path: "./vendor".to_string(),
                        mode: None,
                    }],
                    description: "Vendor assets".to_string(),
                });
            }

            info!(
                "Rust toolchain prepared image inputs for binary '{}' target '{}' in {:.2}s",
                self.binary_name,
                context.build_target.rust_target_triple(),
                build_started.elapsed().as_secs_f64()
            );

            return Ok(ToolchainOutput {
                build_strategy: super::ImageBuildStrategy::FromScratch { layers },
                runtime_command,
            });
        }

        // Need alien-runtime in the image (containers or cloud functions)
        // Use the universal alien-base image that includes alien-runtime with ENTRYPOINT
        let base_images = vec!["ghcr.io/alienplatform/alien-base:latest".to_string()];

        // Runtime command: -- separator required by alien-runtime CLI, then application binary
        // Base image ENTRYPOINT is ["/app/alien-runtime"] so CMD must start with "--"
        let runtime_command = vec!["--".to_string(), format!("./{}", binary_filename)];

        let mut files_to_package = vec![super::FileSpec {
            host_path: binary_path,
            container_path: format!("./{}", binary_filename),
            mode: Some(0o755),
        }];

        // Convention (mirrors the local-platform/from-scratch path): include
        // a top-level `vendor/` directory next to Cargo.toml under `/app/vendor/`
        // in the image. This is how a daemon ships helper binaries or data
        // files it needs at runtime — e.g. bear-agent-loader bundling the
        // bear-agent binary it later installs onto the host. Without this,
        // source-built cloud daemons can't ship vendored assets at all
        // (the local path bundles them; this path silently dropped them).
        let vendor_dir = context.src_dir.join("vendor");
        if vendor_dir.is_dir() {
            info!(
                "Including vendor directory in image: {}",
                vendor_dir.display()
            );
            files_to_package.push(super::FileSpec {
                host_path: vendor_dir,
                container_path: "./vendor".to_string(),
                mode: None,
            });
        }

        let output = ToolchainOutput {
            build_strategy: super::ImageBuildStrategy::FromBaseImage {
                base_images,
                files_to_package,
            },
            runtime_command,
        };

        info!(
            "Rust toolchain prepared image inputs for binary '{}' target '{}' in {:.2}s",
            self.binary_name,
            context.build_target.rust_target_triple(),
            build_started.elapsed().as_secs_f64()
        );

        Ok(output)
    }

    fn dev_command(&self, _src_dir: &Path) -> Vec<String> {
        vec![
            "cargo".to_string(),
            "run".to_string(),
            "--bin".to_string(),
            self.binary_name.clone(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[test]
    fn test_is_rust_project() {
        let temp_dir = tempdir().unwrap();

        // Not a Rust project initially
        assert!(!RustToolchain::is_rust_project(temp_dir.path()));

        // Create Cargo.toml to make it a Rust project
        std::fs::write(
            temp_dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"",
        )
        .unwrap();
        assert!(RustToolchain::is_rust_project(temp_dir.path()));
    }

    #[test]
    fn test_meaningful_cargo_line_filtering() {
        // These lines should be kept (meaningful)
        assert!(RustToolchain::is_meaningful_cargo_line(
            "   Compiling my_project v0.1.0 (/home/user/projects/my_project)"
        ));
        assert!(RustToolchain::is_meaningful_cargo_line(
            "    Checking proc-macro v0.1.0"
        ));
        assert!(RustToolchain::is_meaningful_cargo_line(
            "warning: variable `unused_var` is never used"
        ));
        assert!(RustToolchain::is_meaningful_cargo_line(
            "error[E0308]: mismatched types"
        ));
        assert!(RustToolchain::is_meaningful_cargo_line(
            "error: aborting due to previous error; 3 warnings emitted"
        ));
        assert!(RustToolchain::is_meaningful_cargo_line(
            "For more information about this error, try `rustc --explain E0308`."
        ));

        // These lines should be filtered out (decorative)
        assert!(!RustToolchain::is_meaningful_cargo_line(
            "  |         ^^^^^^^^^^"
        ));
        assert!(!RustToolchain::is_meaningful_cargo_line(
            " --> src/main.rs:10:9"
        ));
        assert!(!RustToolchain::is_meaningful_cargo_line("  |"));
        assert!(!RustToolchain::is_meaningful_cargo_line(
            "  = note: `#[warn(unused_variables)]` on by default"
        ));
        assert!(!RustToolchain::is_meaningful_cargo_line(
            "  = help: consider prefixing with an underscore: `_unused_var`"
        ));
        assert!(!RustToolchain::is_meaningful_cargo_line(
            "  = note: expected type `String`"
        ));
        assert!(RustToolchain::is_meaningful_cargo_line(
            "             found type `{integer}`"
        ));

        // Edge cases
        assert!(!RustToolchain::is_meaningful_cargo_line(""));
        assert!(!RustToolchain::is_meaningful_cargo_line("   "));
    }

    #[test]
    fn test_dev_command() {
        let toolchain = RustToolchain {
            binary_name: "my-app".to_string(),
        };

        let cmd = toolchain.dev_command(Path::new("./"));
        assert_eq!(cmd, vec!["cargo", "run", "--bin", "my-app"]);
    }

    #[tokio::test]
    async fn test_cache_key_generation() {
        let temp_dir = tempdir().unwrap();

        // Create a minimal Rust project
        fs::write(
            temp_dir.path().join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
"#,
        )
        .await
        .unwrap();
        fs::create_dir_all(temp_dir.path().join("src"))
            .await
            .unwrap();
        fs::write(temp_dir.path().join("src/main.rs"), "fn main() {}\n")
            .await
            .unwrap();

        fs::write(
            temp_dir.path().join("Cargo.lock"),
            "# This file is automatically @generated by Cargo.",
        )
        .await
        .unwrap();

        let toolchain = RustToolchain {
            binary_name: "test".to_string(),
        };

        let context = ToolchainContext {
            src_dir: temp_dir.path().to_path_buf(),
            build_dir: temp_dir.path().join("build"),
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: alien_core::BinaryTarget::LinuxX64,
            runtime_platform_name: "aws".to_string(),
            debug_mode: false,
            is_container: false,
        };

        let cache_key = toolchain.generate_cache_key(&context).await.unwrap();
        assert!(cache_key.contains("x86_64-unknown-linux-musl"));
        assert!(cache_key.contains("release"));
        assert!(cache_key.contains("test"));
    }
}
