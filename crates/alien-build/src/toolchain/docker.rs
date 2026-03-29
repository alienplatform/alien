use super::{Toolchain, ToolchainContext, ToolchainOutput};
use crate::error::{ErrorData, Result};
use crate::settings::BinaryTargetExt;
use alien_core::AlienEvent;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::info;

/// Docker toolchain implementation using Docker buildx for multi-architecture builds.
///
/// This toolchain:
/// 1. Validates Dockerfile exists in the source directory
/// 2. Builds multi-architecture images using `docker buildx build`
/// 3. Exports OCI tarballs for each target architecture
/// 4. Returns paths to the built tarballs
#[derive(Debug, Clone)]
pub struct DockerToolchain {
    /// Dockerfile path relative to src (default: "Dockerfile")
    pub dockerfile: Option<String>,
    /// Build arguments for docker build
    pub build_args: Option<HashMap<String, String>>,
    /// Multi-stage build target
    pub target: Option<String>,
}

impl DockerToolchain {
    /// Check if the source directory contains a Dockerfile
    pub fn has_dockerfile(src_dir: &Path, dockerfile: Option<&String>) -> bool {
        let dockerfile_name = dockerfile.map(|s| s.as_str()).unwrap_or("Dockerfile");
        src_dir.join(dockerfile_name).exists()
    }

    /// Generate a temporary tag for the build
    fn generate_temp_tag(function_name: &str) -> String {
        use rand::distr::Alphanumeric;
        use rand::Rng;

        let random_suffix: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(8)
            .map(char::from)
            .collect::<String>()
            .to_lowercase();

        format!("alien-build-{}:{}", function_name, random_suffix)
    }

    fn humanize_buildx_failure(stderr_output: &str) -> String {
        let lower = stderr_output.to_ascii_lowercase();

        if lower.contains("cannot connect to the docker daemon")
            || lower.contains("is the docker daemon running")
            || lower.contains("docker.sock")
        {
            return "Docker is installed but the daemon is unavailable. Start Docker or OrbStack and retry.".to_string();
        }

        "docker buildx build failed".to_string()
    }
}

#[async_trait]
impl Toolchain for DockerToolchain {
    async fn build(&self, context: &ToolchainContext) -> Result<ToolchainOutput> {
        let dockerfile_name = self.dockerfile.as_deref().unwrap_or("Dockerfile");

        info!(
            "Building Docker image from {} in {}",
            dockerfile_name,
            context.src_dir.display()
        );

        // Validate Dockerfile exists
        let dockerfile_path = context.src_dir.join(dockerfile_name);
        if !dockerfile_path.exists() {
            return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                resource_id: dockerfile_name.to_string(),
                reason: format!("Dockerfile not found at: {}", dockerfile_path.display()),
            }));
        }

        // Build arguments for docker buildx build
        // Note: Target architecture is automatically handled by build_target
        let temp_tag = Self::generate_temp_tag("docker-build");
        let arch_str = match context.build_target.to_dockdash_arch() {
            dockdash::Arch::Amd64 => "amd64",
            dockdash::Arch::ARM64 => "arm64",
            _ => "amd64", // Fallback for other architectures
        };
        let platform_str = format!("linux/{}", arch_str);

        let mut args = vec![
            "buildx",
            "build",
            "--platform",
            &platform_str,
            "--load", // Load image into Docker daemon so we can export it
            "-f",
            dockerfile_name,
        ];

        // Add build args if provided
        let build_arg_strings: Vec<String> = self
            .build_args
            .as_ref()
            .map(|args| args.iter().map(|(k, v)| format!("{}={}", k, v)).collect())
            .unwrap_or_default();

        for build_arg in &build_arg_strings {
            args.push("--build-arg");
            args.push(build_arg);
        }

        // Add target if specified
        let target_str;
        if let Some(target) = &self.target {
            target_str = target.clone();
            args.push("--target");
            args.push(&target_str);
        }

        // Add tag and context
        args.push("-t");
        args.push(&temp_tag);
        args.push("."); // Build context is the src_dir

        info!("Running docker buildx build with args: {:?}", args);

        // Run docker buildx build with progress reporting
        AlienEvent::CompilingCode {
            language: "docker".to_string(),
            progress: None,
        }
        .in_scope(|compilation_event| async move {
            let mut child = Command::new("docker")
                .args(&args)
                .current_dir(&context.src_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    function_name: "docker-build".to_string(),
                    reason: "Failed to execute docker buildx build. Is Docker installed?"
                        .to_string(),
                    build_output: None,
                })?;

            // Read stderr for progress (docker outputs to stderr)
            let stderr = child.stderr.take().unwrap();
            let mut stderr_reader = BufReader::new(stderr).lines();
            let mut stderr_lines = Vec::new();

            while let Some(line) = stderr_reader.next_line().await.into_alien_error().context(
                ErrorData::ImageBuildFailed {
                    function_name: "docker-build".to_string(),
                    reason: "Failed to read docker build output".to_string(),
                    build_output: None,
                },
            )? {
                stderr_lines.push(line.clone());

                let trimmed_line = line.trim();
                if !trimmed_line.is_empty() {
                    let _ = compilation_event
                        .update(AlienEvent::CompilingCode {
                            language: "docker".to_string(),
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
                        function_name: "docker-build".to_string(),
                        reason: "Failed to wait for docker build completion".to_string(),
                        build_output: None,
                    })?;

            if !output.success() {
                let stderr_output = stderr_lines.join("\n");
                return Err(AlienError::new(ErrorData::ImageBuildFailed {
                    function_name: "docker-build".to_string(),
                    reason: Self::humanize_buildx_failure(&stderr_output),
                    build_output: Some(stderr_output),
                }));
            }

            info!("docker buildx build completed successfully");
            Ok(())
        })
        .await?;

        // Export the built image to OCI tarball
        let output_tarball = context.build_dir.join(format!(
            "{}.oci.tar",
            context.build_target.runtime_platform_id()
        ));

        info!(
            "Exporting Docker image {} to OCI tarball: {}",
            temp_tag,
            output_tarball.display()
        );

        let output_tarball_str = output_tarball.to_string_lossy().to_string();
        let save_args = vec!["save", "-o", &output_tarball_str, &temp_tag];

        let save_output = Command::new("docker")
            .args(&save_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                function_name: "docker-build".to_string(),
                reason: "Failed to execute docker save".to_string(),
                build_output: None,
            })?;

        if !save_output.status.success() {
            let stderr = String::from_utf8_lossy(&save_output.stderr);
            return Err(AlienError::new(ErrorData::ImageBuildFailed {
                function_name: "docker-build".to_string(),
                reason: "docker save failed".to_string(),
                build_output: Some(stderr.to_string()),
            }));
        }

        info!("Successfully exported Docker image to OCI tarball");

        // Clean up the temporary image
        let _ = Command::new("docker")
            .args(&["rmi", &temp_tag])
            .output()
            .await;

        // Extract CMD from the built image for the runtime_command field
        let runtime_command = Self::extract_cmd_from_tarball(&output_tarball)?;

        info!("Extracted CMD from Docker image: {:?}", runtime_command);

        // Docker builds produce complete OCI images - return absolute path
        // The build system will detect if source == dest and skip the copy
        Ok(ToolchainOutput {
            build_strategy: super::ImageBuildStrategy::CompleteOCITarball {
                tarball_path: output_tarball,
            },
            runtime_command,
        })
    }

    fn dev_command(&self, _src_dir: &Path) -> Vec<String> {
        vec!["docker".to_string(), "run".to_string()]
    }
}

impl DockerToolchain {
    /// Extract CMD from OCI tarball using dockdash
    fn extract_cmd_from_tarball(tarball_path: &Path) -> Result<Vec<String>> {
        use dockdash::Image;

        let image = Image::from_tarball(tarball_path)
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                function_name: "docker-build".to_string(),
                reason: "Failed to read OCI tarball".to_string(),
                build_output: None,
            })?;

        let metadata =
            image
                .get_metadata()
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    function_name: "docker-build".to_string(),
                    reason: "Failed to read image metadata from tarball".to_string(),
                    build_output: None,
                })?;

        // Extract CMD from config
        // If no CMD, return empty vec (container will fail with "no command specified")
        Ok(metadata.cmd.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::BinaryTarget;
    use dockdash::Image;
    use std::collections::HashMap;
    use std::process::Command;
    use tempfile::tempdir;
    use tokio::fs;

    fn docker_available() -> bool {
        Command::new("docker")
            .arg("info")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn test_has_dockerfile() {
        let temp_dir = tempdir().unwrap();

        // No Dockerfile
        assert!(!DockerToolchain::has_dockerfile(temp_dir.path(), None));

        // Create default Dockerfile
        std::fs::write(temp_dir.path().join("Dockerfile"), "FROM nginx").unwrap();
        assert!(DockerToolchain::has_dockerfile(temp_dir.path(), None));

        // Custom dockerfile name
        std::fs::write(temp_dir.path().join("Dockerfile.prod"), "FROM nginx").unwrap();
        assert!(DockerToolchain::has_dockerfile(
            temp_dir.path(),
            Some(&"Dockerfile.prod".to_string())
        ));
    }

    #[test]
    fn test_generate_temp_tag() {
        let tag1 = DockerToolchain::generate_temp_tag("my-app");
        let tag2 = DockerToolchain::generate_temp_tag("my-app");

        assert!(tag1.starts_with("alien-build-my-app:"));
        assert!(tag2.starts_with("alien-build-my-app:"));
        assert_ne!(tag1, tag2); // Should be unique
    }

    #[tokio::test]
    async fn test_docker_toolchain_build() {
        if !docker_available() {
            eprintln!("Skipping test_docker_toolchain_build: docker not available");
            return;
        }

        let temp_dir = tempdir().unwrap();
        let build_dir = tempdir().unwrap();

        // Create a simple Dockerfile
        let dockerfile_content = r#"
FROM alpine:latest
WORKDIR /app
RUN echo "Hello from Docker" > hello.txt
CMD ["cat", "hello.txt"]
"#;
        fs::write(temp_dir.path().join("Dockerfile"), dockerfile_content)
            .await
            .unwrap();

        let toolchain = DockerToolchain {
            dockerfile: None,
            build_args: None,
            target: None,
        };

        let context = ToolchainContext {
            src_dir: temp_dir.path().to_path_buf(),
            build_dir: build_dir.path().to_path_buf(),
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: BinaryTarget::linux_container_target(),
            platform_name: "aws".to_string(),
            debug_mode: false,
            is_container: true,
        };

        // Test assumes Docker is running (per user requirement)
        let output = toolchain
            .build(&context)
            .await
            .expect("Docker toolchain build should succeed (Docker must be running)");

        // Verify output
        assert_eq!(
            output.runtime_command,
            vec!["cat".to_string(), "hello.txt".to_string()],
            "Dockerfile CMD should be captured"
        );

        // Verify OCI tarball was created
        let target = BinaryTarget::linux_container_target();
        let tarball_path = build_dir
            .path()
            .join(format!("{}.oci.tar", target.runtime_platform_id()));
        assert!(
            tarball_path.exists(),
            "OCI tarball should exist at {}",
            tarball_path.display()
        );

        // Verify tarball is valid OCI format using dockdash
        let image = Image::from_tarball(&tarball_path).expect("OCI tarball should be valid");

        let metadata = image
            .get_metadata()
            .expect("Should be able to read image metadata");

        // Verify CMD from Dockerfile is in metadata
        assert!(
            metadata.cmd.is_some(),
            "Image should have CMD from Dockerfile"
        );
    }

    #[tokio::test]
    async fn test_docker_toolchain_with_build_args() {
        if !docker_available() {
            eprintln!("Skipping test_docker_toolchain_with_build_args: docker not available");
            return;
        }

        let temp_dir = tempdir().unwrap();
        let build_dir = tempdir().unwrap();

        // Create Dockerfile that uses build arg
        let dockerfile_content = r#"
FROM alpine:latest
ARG VERSION=unknown
WORKDIR /app
RUN echo "Version: $VERSION" > version.txt
"#;
        fs::write(temp_dir.path().join("Dockerfile"), dockerfile_content)
            .await
            .unwrap();

        let mut build_args = HashMap::new();
        build_args.insert("VERSION".to_string(), "1.2.3".to_string());

        let toolchain = DockerToolchain {
            dockerfile: None,
            build_args: Some(build_args),
            target: None,
        };

        let context = ToolchainContext {
            src_dir: temp_dir.path().to_path_buf(),
            build_dir: build_dir.path().to_path_buf(),
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: BinaryTarget::linux_container_target(),
            platform_name: "aws".to_string(),
            debug_mode: false,
            is_container: true,
        };

        // Test assumes Docker is running
        let _output = toolchain
            .build(&context)
            .await
            .expect("Docker toolchain build with args should succeed");

        let target = BinaryTarget::linux_container_target();
        let tarball_path = build_dir
            .path()
            .join(format!("{}.oci.tar", target.runtime_platform_id()));
        assert!(tarball_path.exists(), "OCI tarball should exist");

        // Verify the image is valid
        let _image = Image::from_tarball(&tarball_path).expect("OCI tarball should be valid");
    }

    #[tokio::test]
    async fn test_docker_toolchain_missing_dockerfile_fails() {
        let temp_dir = tempdir().unwrap();
        let build_dir = tempdir().unwrap();

        // No Dockerfile in directory
        let toolchain = DockerToolchain {
            dockerfile: None,
            build_args: None,
            target: None,
        };

        let context = ToolchainContext {
            src_dir: temp_dir.path().to_path_buf(),
            build_dir: build_dir.path().to_path_buf(),
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: BinaryTarget::linux_container_target(),
            platform_name: "aws".to_string(),
            debug_mode: false,
            is_container: true,
        };

        let result = toolchain.build(&context).await;

        // Should fail with clear error about missing Dockerfile
        assert!(result.is_err(), "Should fail when Dockerfile is missing");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Dockerfile not found"),
            "Error should mention missing Dockerfile: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_docker_toolchain_custom_dockerfile() {
        if !docker_available() {
            eprintln!("Skipping test_docker_toolchain_custom_dockerfile: docker not available");
            return;
        }

        let temp_dir = tempdir().unwrap();
        let build_dir = tempdir().unwrap();

        // Create Dockerfile.prod
        let dockerfile_content = r#"
FROM alpine:latest
LABEL test=custom-dockerfile
WORKDIR /app
"#;
        fs::write(temp_dir.path().join("Dockerfile.prod"), dockerfile_content)
            .await
            .unwrap();

        let toolchain = DockerToolchain {
            dockerfile: Some("Dockerfile.prod".to_string()),
            build_args: None,
            target: None,
        };

        let context = ToolchainContext {
            src_dir: temp_dir.path().to_path_buf(),
            build_dir: build_dir.path().to_path_buf(),
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: BinaryTarget::linux_container_target(),
            platform_name: "aws".to_string(),
            debug_mode: false,
            is_container: true,
        };

        // Should succeed with custom dockerfile
        let _output = toolchain
            .build(&context)
            .await
            .expect("Should build with custom Dockerfile name");

        let target = BinaryTarget::linux_container_target();
        let tarball_path = build_dir
            .path()
            .join(format!("{}.oci.tar", target.runtime_platform_id()));
        assert!(tarball_path.exists(), "OCI tarball should exist");
    }
}
