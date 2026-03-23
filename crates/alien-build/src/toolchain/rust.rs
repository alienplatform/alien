use super::{cache_utils, Toolchain, ToolchainContext, ToolchainOutput};
use crate::error::{ErrorData, Result};
use alien_core::AlienEvent;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::info;

/// Rust toolchain implementation using Cargo with Zig cross-compilation
#[derive(Debug, Clone)]
pub struct RustToolchain {
    /// Name of the binary to build and run
    pub binary_name: String,
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
        let cargo_lock_hash = cache_utils::hash_files(&["**/Cargo.lock"], &context.src_dir).await?;
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
                function_name: self.binary_name.clone(),
                reason: "Failed to execute cargo metadata".to_string(),
                build_output: None,
            })?;

        if !metadata_output.status.success() {
            let stderr = String::from_utf8_lossy(&metadata_output.stderr);
            return Err(AlienError::new(ErrorData::ImageBuildFailed {
                function_name: self.binary_name.clone(),
                reason: "cargo metadata failed".to_string(),
                build_output: Some(stderr.to_string()),
            }));
        }

        let stdout = String::from_utf8_lossy(&metadata_output.stdout);
        let metadata: Value = serde_json::from_str(&stdout).into_alien_error().context(
            ErrorData::ImageBuildFailed {
                function_name: self.binary_name.clone(),
                reason: "Failed to parse cargo metadata JSON".to_string(),
                build_output: None,
            },
        )?;

        let target_directory = metadata
            .get("target_directory")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ImageBuildFailed {
                    function_name: self.binary_name.clone(),
                    reason: "cargo metadata missing target_directory field".to_string(),
                    build_output: None,
                })
            })?;

        Ok(PathBuf::from(target_directory))
    }
}

#[async_trait]
impl Toolchain for RustToolchain {
    async fn build(&self, context: &ToolchainContext) -> Result<ToolchainOutput> {
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

        // For Darwin (macOS) targets, use native cargo build instead of cargo-zigbuild
        // cargo-zigbuild doesn't work well with macOS frameworks
        let use_zigbuild = !context.build_target.is_darwin();

        // Check if cargo-zigbuild is installed for non-Darwin targets, install if not present
        if use_zigbuild {
            let zigbuild_path = self.expand_home_dir("~/.cargo/bin/cargo-zigbuild");
            if !zigbuild_path.exists() {
                info!("cargo-zigbuild not found, installing...");
                let install_zigbuild_output = Command::new("cargo")
                    .args(&["install", "cargo-zigbuild"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .await
                    .into_alien_error()
                    .context(ErrorData::ImageBuildFailed {
                        function_name: self.binary_name.clone(),
                        reason: "Failed to execute cargo install cargo-zigbuild".to_string(),
                        build_output: None,
                    })?;

                if !install_zigbuild_output.status.success() {
                    let stderr = String::from_utf8_lossy(&install_zigbuild_output.stderr);
                    return Err(AlienError::new(ErrorData::ImageBuildFailed {
                        function_name: self.binary_name.clone(),
                        reason: "Failed to install cargo-zigbuild".to_string(),
                        build_output: Some(stderr.to_string()),
                    }));
                }
                info!("Successfully installed cargo-zigbuild");
            } else {
                info!("cargo-zigbuild already installed");
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
                function_name: self.binary_name.clone(),
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
                    function_name: self.binary_name.clone(),
                    reason: "Failed to execute rustup target add".to_string(),
                    build_output: None,
                })?;

            if !install_target_output.status.success() {
                let stderr = String::from_utf8_lossy(&install_target_output.stderr);
                return Err(AlienError::new(ErrorData::ImageBuildFailed {
                    function_name: self.binary_name.clone(),
                    reason: format!(
                        "Failed to install target {}",
                        context.build_target.rust_target_triple()
                    ),
                    build_output: Some(stderr.to_string()),
                }));
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

        // Build the project for the target platform
        // Use native cargo build for Darwin (macOS) targets, zigbuild for others
        let mut args = if use_zigbuild {
            vec![
                "zigbuild",
                "--target",
                context.build_target.rust_target_triple(),
                "--bin",
                &self.binary_name,
            ]
        } else {
            vec![
                "build",
                "--target",
                context.build_target.rust_target_triple(),
                "--bin",
                &self.binary_name,
            ]
        };

        // Add --release flag only in release mode
        if !context.debug_mode {
            args.push("--release");
        }

        let build_command = if use_zigbuild {
            "cargo zigbuild"
        } else {
            "cargo build"
        };
        info!(
            "Running {} with args: {:?} in directory: {}",
            build_command,
            args,
            context.src_dir.display()
        );

        // Use in_scope for automatic event lifecycle management
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
                    function_name: self.binary_name.clone(),
                    reason: format!("Failed to execute {}", build_command),
                    build_output: None,
                })?;

            // Read stderr line by line for progress updates
            let stderr = child.stderr.take().unwrap();
            let mut stderr_reader = BufReader::new(stderr).lines();
            let mut stderr_lines = Vec::new();

            // Process stderr output line by line
            while let Some(line) = stderr_reader.next_line().await.into_alien_error().context(
                ErrorData::ImageBuildFailed {
                    function_name: self.binary_name.clone(),
                    reason: "Failed to read cargo build output".to_string(),
                    build_output: None,
                },
            )? {
                stderr_lines.push(line.clone());

                // Update the event with the latest line for progress
                // Only show meaningful cargo output, filtering out decorative lines
                if Self::is_meaningful_cargo_line(&line) {
                    let _ = compilation_event
                        .update(AlienEvent::CompilingCode {
                            language: "rust".to_string(),
                            progress: Some(line.trim().to_string()),
                        })
                        .await; // Ignore update errors to not fail the build
                }
            }

            // Wait for the process to complete
            let output =
                child
                    .wait()
                    .await
                    .into_alien_error()
                    .context(ErrorData::ImageBuildFailed {
                        function_name: self.binary_name.clone(),
                        reason: format!("Failed to wait for {} completion", build_command),
                        build_output: None,
                    })?;

            if !output.success() {
                let stderr_output = stderr_lines.join("\n");
                return Err(AlienError::new(ErrorData::ImageBuildFailed {
                    function_name: self.binary_name.clone(),
                    reason: format!("{} failed", build_command),
                    build_output: Some(stderr_output),
                }));
            }

            info!("{} completed successfully", build_command);
            Ok(())
        })
        .await?;

        // Save updated cache if available
        cache_utils::save_cache(context.cache_store.as_deref(), &cache_key, &cache_paths).await?;

        // Determine the actual target directory (workspace-aware) and binary path
        let target_dir_base = self.get_target_directory(&context.src_dir).await?;
        let target_subdir = if context.debug_mode {
            "debug"
        } else {
            "release"
        };
        let target_platform_dir = target_dir_base
            .join(context.build_target.rust_target_triple())
            .join(target_subdir);
        let binary_path = target_platform_dir.join(&self.binary_name);

        // Verify the binary was built
        if !binary_path.exists() {
            return Err(AlienError::new(ErrorData::ImageBuildFailed {
                function_name: self.binary_name.clone(),
                reason: format!(
                    "Expected binary not found at: {}. Target directory: {}",
                    binary_path.display(),
                    target_dir_base.display()
                ),
                build_output: None,
            }));
        }

        info!("Successfully built Rust binary: {}", binary_path.display());

        // Determine if we need alien-runtime in the image
        // Functions on local platform use embedded runtime in agent (no runtime in image)
        // Everything else (containers on any platform, functions on cloud) needs alien-runtime
        let needs_runtime_in_image = context.is_container || context.platform_name != "local";

        if !needs_runtime_in_image {
            // Function on local platform - runtime is embedded in operator
            // Just package the application binary
            let runtime_command = vec![format!("./{}", self.binary_name)];

            return Ok(ToolchainOutput {
                build_strategy: super::ImageBuildStrategy::FromScratch {
                    layers: vec![super::LayerSpec {
                        files: vec![super::FileSpec {
                            host_path: binary_path.clone(),
                            container_path: format!("./{}", self.binary_name),
                            mode: Some(0o755), // Executable
                        }],
                        description: "Application binary".to_string(),
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
        let runtime_command = vec!["--".to_string(), format!("./{}", self.binary_name)];

        Ok(ToolchainOutput {
            build_strategy: super::ImageBuildStrategy::FromBaseImage {
                base_images,
                files_to_package: vec![(binary_path, format!("./{}", self.binary_name))],
            },
            runtime_command,
        })
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
            platform_name: "aws".to_string(),
            debug_mode: false,
            is_container: false,
        };

        let cache_key = toolchain.generate_cache_key(&context).await.unwrap();
        assert!(cache_key.contains("x86_64-unknown-linux-musl"));
        assert!(cache_key.contains("release"));
        assert!(cache_key.contains("test"));
    }
}
