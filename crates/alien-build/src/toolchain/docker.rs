use super::{Toolchain, ToolchainContext, ToolchainOutput};
use crate::command_output::{image_build_error_with_output, wait_with_captured_output};
use crate::error::{ErrorData, Result};
use crate::settings::BinaryTargetExt;
use alien_core::AlienEvent;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
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
    fn generate_temp_tag(resource_name: &str) -> String {
        use rand::distr::Alphanumeric;
        use rand::Rng;

        let random_suffix: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(8)
            .map(char::from)
            .collect::<String>()
            .to_lowercase();

        format!("alien-build-{}:{}", resource_name, random_suffix)
    }

    fn humanize_buildx_failure(stderr_output: &str) -> String {
        let lower = stderr_output.to_ascii_lowercase();

        if lower.contains("cannot connect to the docker daemon")
            || lower.contains("is the docker daemon running")
            || lower.contains("docker.sock")
        {
            "Docker is installed but the daemon is unavailable. Start Docker or OrbStack and retry."
        } else {
            "docker buildx build failed"
        }
        .to_string()
    }

    /// `ImageBuildFailed` for a docker setup step (no captured build output).
    fn docker_build_error(reason: impl Into<String>) -> ErrorData {
        ErrorData::ImageBuildFailed {
            resource_name: "docker-build".to_string(),
            reason: reason.into(),
            build_output: None,
        }
    }

    /// std reports x86_64/aarch64; docker wants amd64/arm64 — map the host CPU.
    fn host_docker_arch() -> Option<&'static str> {
        match std::env::consts::ARCH {
            "x86_64" => Some("amd64"),
            "aarch64" => Some("arm64"),
            _ => None,
        }
    }

    /// Whether the builder's `Platforms:` line lists `linux/<arch>`. A substring
    /// match is safe for our amd64/arm64 targets.
    fn inspect_reports_platform(inspect_stdout: &str, target_arch: &str) -> bool {
        let needle = format!("linux/{}", target_arch);
        inspect_stdout
            .lines()
            .filter(|line| line.trim_start().starts_with("Platforms:"))
            .any(|line| line.contains(&needle))
    }

    /// Whether the active builder already supports `target_arch` (the default `docker` builder
    /// reflects the kernel's binfmt handlers). A failed inspect counts as "no".
    async fn host_can_emulate(target_arch: &str) -> Result<bool> {
        let output = Command::new("docker")
            .args(["buildx", "inspect", "default"])
            .output()
            .await
            .into_alien_error()
            .context(Self::docker_build_error(
                "Could not inspect the default buildx builder",
            ))?;
        if !output.status.success() {
            return Ok(false);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(Self::inspect_reports_platform(&stdout, target_arch))
    }

    /// Whether the build is blocked: a non-native target the active builder can't build.
    /// Pure, so the decision is unit-tested without docker.
    fn build_blocked(host_arch: Option<&str>, target_arch: &str, builder_supports: bool) -> bool {
        host_arch != Some(target_arch) && !builder_supports
    }

    /// Fail fast when the active builder can't build `target_arch`. We don't set up emulation
    /// here — that's the CI runner's job; a native target, or one the builder already supports,
    /// passes through.
    async fn ensure_builder_supports_arch(target_arch: &str) -> Result<()> {
        let host_arch = Self::host_docker_arch();
        if host_arch == Some(target_arch) {
            return Ok(());
        }
        let builder_supports = Self::host_can_emulate(target_arch).await?;
        if Self::build_blocked(host_arch, target_arch, builder_supports) {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "Cannot build linux/{t} on this host: the active buildx builder doesn't support that architecture. Build on a native {t} runner, or configure a buildx builder with emulation for it.",
                t = target_arch
            ))));
        }
        Ok(())
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

        Self::ensure_builder_supports_arch(arch_str).await?;

        let mut args: Vec<&str> = vec![
            "buildx",
            "build",
            "--platform",
            platform_str.as_str(),
            "--load", // export into the docker daemon so we can `docker save` it
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
                    resource_name: "docker-build".to_string(),
                    reason: "Failed to execute docker buildx build. Is Docker installed?"
                        .to_string(),
                    build_output: None,
                })?;

            let (output, captured_output) = wait_with_captured_output(
                &mut child,
                "docker-build",
                "Failed to read docker build output",
                "Failed to wait for docker build completion",
                |line| {
                    let compilation_event = &compilation_event;
                    async move {
                        let trimmed_line = line.line.trim();
                        if !trimmed_line.is_empty() {
                            let _ = compilation_event
                                .update(AlienEvent::CompilingCode {
                                    language: "docker".to_string(),
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
                return Err(AlienError::new(ErrorData::ImageBuildFailed {
                    resource_name: "docker-build".to_string(),
                    reason: Self::humanize_buildx_failure(&build_output),
                    build_output: Some(build_output),
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
                resource_name: "docker-build".to_string(),
                reason: "Failed to execute docker save".to_string(),
                build_output: None,
            })?;

        if !save_output.status.success() {
            return Err(image_build_error_with_output(
                "docker-build",
                "docker save failed",
                &save_output,
            ));
        }

        info!("Successfully exported Docker image to OCI tarball");

        // Flatten the saved archive to a single image manifest before the OCI reader sees it.
        Self::normalize_oci_archive(&output_tarball, arch_str)?;

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
                resource_name: "docker-build".to_string(),
                reason: "Failed to read OCI tarball".to_string(),
                build_output: None,
            })?;

        let metadata =
            image
                .get_metadata()
                .into_alien_error()
                .context(ErrorData::ImageBuildFailed {
                    resource_name: "docker-build".to_string(),
                    reason: "Failed to read image metadata from tarball".to_string(),
                    build_output: None,
                })?;

        // Extract CMD from config
        // If no CMD, return empty vec (container will fail with "no command specified")
        Ok(metadata.cmd.unwrap_or_default())
    }

    /// Rewrite an OCI archive's `index.json` to point straight at the single image manifest
    /// for `target_arch`.
    ///
    /// The containerd image store's `docker save` writes a nested `index.json` → image-index →
    /// [image manifest, attestation]; our reader (ocipkg, via dockdash) treats the first
    /// descriptor as an image manifest and fails with "missing field `config`". The classic
    /// store already writes a flat layout, so this is a no-op there.
    pub(crate) fn normalize_oci_archive(tarball_path: &Path, target_arch: &str) -> Result<()> {
        // Buffer the small JSON entries (index + manifests); layer blobs are large and not
        // needed to resolve the descriptor, so skip reading their bodies.
        const SMALL_BLOB_BYTES: u64 = 1 << 20;
        let mut index: Option<Value> = None;
        let mut blobs: HashMap<String, Value> = HashMap::new();

        let archive_file =
            File::open(tarball_path)
                .into_alien_error()
                .context(docker_read_error(
                    "Failed to open OCI tarball for normalization",
                ))?;
        let mut archive = tar::Archive::new(archive_file);
        for entry in archive
            .entries()
            .into_alien_error()
            .context(docker_read_error("Failed to read OCI tarball entries"))?
        {
            let mut entry = entry
                .into_alien_error()
                .context(docker_read_error("Failed to read OCI tarball entry"))?;
            let path = entry
                .path()
                .into_alien_error()
                .context(docker_read_error("Failed to read OCI tarball entry path"))?
                .to_string_lossy()
                .into_owned();

            if path == "index.json" {
                index = Some(read_entry_json(&mut entry)?);
            } else if path.starts_with("blobs/") && entry.size() <= SMALL_BLOB_BYTES {
                // Only descriptors are stored by digest; key small blobs by "sha256:<hex>".
                if let Some(digest) = blob_path_to_digest(&path) {
                    if let Ok(value) = read_entry_json(&mut entry) {
                        blobs.insert(digest, value);
                    }
                }
            }
        }

        // No index.json means `docker save` produced something unusable — fail here with a
        // precise message instead of letting it surface as an opaque read error downstream.
        let index = index.ok_or_else(|| {
            AlienError::new(docker_read_error("OCI tarball is missing index.json"))
        })?;

        let chosen = select_image_manifest_descriptor(&index, &blobs, target_arch)?;
        if index_points_only_at(&index, &chosen) {
            return Ok(());
        }

        let flat_index = json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [chosen],
        });
        let flat_index_bytes =
            serde_json::to_vec(&flat_index)
                .into_alien_error()
                .context(docker_read_error(
                    "Failed to serialize flattened index.json",
                ))?;

        rewrite_archive_index_json(tarball_path, &flat_index_bytes)
    }
}

/// Build the read-side error variant used while normalizing the OCI archive.
fn docker_read_error(reason: &str) -> ErrorData {
    ErrorData::ImageBuildFailed {
        resource_name: "docker-build".to_string(),
        reason: reason.to_string(),
        build_output: None,
    }
}

/// Parse `blobs/sha256/<hex>` into the descriptor digest `sha256:<hex>`.
fn blob_path_to_digest(path: &str) -> Option<String> {
    let rest = path.strip_prefix("blobs/")?;
    let (algorithm, hex) = rest.split_once('/')?;
    Some(format!("{algorithm}:{hex}"))
}

fn read_entry_json<R: Read>(entry: &mut R) -> Result<Value> {
    let mut buf = Vec::new();
    entry
        .read_to_end(&mut buf)
        .into_alien_error()
        .context(docker_read_error("Failed to read OCI tarball entry body"))?;
    serde_json::from_slice(&buf)
        .into_alien_error()
        .context(docker_read_error("Failed to parse OCI JSON entry"))
}

fn is_index_media_type(media_type: &str) -> bool {
    media_type == "application/vnd.oci.image.index.v1+json"
        || media_type == "application/vnd.docker.distribution.manifest.list.v2+json"
}

fn is_manifest_media_type(media_type: &str) -> bool {
    media_type == "application/vnd.oci.image.manifest.v1+json"
        || media_type == "application/vnd.docker.distribution.manifest.v2+json"
}

fn descriptor_architecture(descriptor: &Value) -> Option<&str> {
    descriptor
        .get("platform")
        .and_then(|p| p.get("architecture"))
        .and_then(|a| a.as_str())
}

/// Walk an index's descriptors (following nested indexes) and collect the real image
/// manifests, skipping attestation manifests (platform `unknown/unknown`).
fn collect_image_manifests(
    manifests: Option<&Value>,
    blobs: &HashMap<String, Value>,
    out: &mut Vec<Value>,
) {
    let Some(descriptors) = manifests.and_then(|m| m.as_array()) else {
        return;
    };
    for descriptor in descriptors {
        let media_type = descriptor
            .get("mediaType")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        if is_index_media_type(media_type) {
            if let Some(nested) = descriptor
                .get("digest")
                .and_then(|d| d.as_str())
                .and_then(|d| blobs.get(d))
            {
                collect_image_manifests(nested.get("manifests"), blobs, out);
            }
        } else if is_manifest_media_type(media_type)
            && descriptor_architecture(descriptor) != Some("unknown")
        {
            out.push(descriptor.clone());
        }
    }
}

/// Resolve the one image-manifest descriptor to keep: the one matching `target_arch`, or
/// the only candidate when architecture isn't recorded. Fails when none can be found.
fn select_image_manifest_descriptor(
    index: &Value,
    blobs: &HashMap<String, Value>,
    target_arch: &str,
) -> Result<Value> {
    let mut candidates = Vec::new();
    collect_image_manifests(index.get("manifests"), blobs, &mut candidates);

    if let Some(descriptor) = candidates
        .iter()
        .find(|d| descriptor_architecture(d) == Some(target_arch))
    {
        return Ok(descriptor.clone());
    }
    if let [only] = candidates.as_slice() {
        return Ok(only.clone());
    }
    Err(AlienError::new(docker_read_error(&format!(
        "OCI archive has no image manifest for architecture '{target_arch}'"
    ))))
}

/// Whether `index.json` already contains exactly the chosen descriptor (classic store).
fn index_points_only_at(index: &Value, chosen: &Value) -> bool {
    index
        .get("manifests")
        .and_then(|m| m.as_array())
        .is_some_and(|m| m.len() == 1 && m[0].get("digest") == chosen.get("digest"))
}

/// Stream the archive into a sibling temp file, replacing only `index.json`, then swap it
/// in. Other entries (blobs, oci-layout) are copied verbatim.
fn rewrite_archive_index_json(tarball_path: &Path, new_index: &[u8]) -> Result<()> {
    let temp_path = tarball_path.with_extension("tar.normalizing");

    {
        let source = File::open(tarball_path)
            .into_alien_error()
            .context(docker_read_error("Failed to open OCI tarball for rewrite"))?;
        let dest = File::create(&temp_path)
            .into_alien_error()
            .context(docker_read_error("Failed to create normalized OCI tarball"))?;
        let mut archive = tar::Archive::new(source);
        let mut builder = tar::Builder::new(dest);

        for entry in archive
            .entries()
            .into_alien_error()
            .context(docker_read_error("Failed to read OCI tarball entries"))?
        {
            let mut entry = entry
                .into_alien_error()
                .context(docker_read_error("Failed to read OCI tarball entry"))?;
            let path = entry
                .path()
                .into_alien_error()
                .context(docker_read_error("Failed to read OCI tarball entry path"))?
                .into_owned();
            let mut header = entry.header().clone();

            if path == Path::new("index.json") {
                header.set_size(new_index.len() as u64);
                builder
                    .append_data(&mut header, &path, new_index)
                    .into_alien_error()
                    .context(docker_read_error("Failed to write normalized index.json"))?;
            } else {
                builder
                    .append_data(&mut header, &path, &mut entry)
                    .into_alien_error()
                    .context(docker_read_error("Failed to copy OCI tarball entry"))?;
            }
        }

        builder
            .into_inner()
            .into_alien_error()
            .context(docker_read_error(
                "Failed to finalize normalized OCI tarball",
            ))?;
    }

    std::fs::rename(&temp_path, tarball_path)
        .into_alien_error()
        .context(docker_read_error(
            "Failed to replace OCI tarball with normalized copy",
        ))
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
            runtime_platform_name: "aws".to_string(),
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

    #[test]
    fn test_inspect_reports_platform() {
        // A bare runner's default builder lists only the native arch (+ compatible variants).
        let bare = "Name:   default\nDriver: docker\n\nNodes:\nName:      default\nStatus:    running\nPlatforms: linux/amd64, linux/amd64/v2, linux/amd64/v3, linux/386\n";
        assert!(DockerToolchain::inspect_reports_platform(bare, "amd64"));
        assert!(
            !DockerToolchain::inspect_reports_platform(bare, "arm64"),
            "a bare amd64 runner must not report arm64 — that's what triggers the fail-fast"
        );

        // A machine with QEMU registered (Docker Desktop / OrbStack) lists emulated platforms too.
        let emulated =
            "Nodes:\nName: default\nPlatforms: linux/arm64, linux/amd64, linux/amd64/v2, linux/arm/v7\n";
        assert!(DockerToolchain::inspect_reports_platform(emulated, "arm64"));
        assert!(DockerToolchain::inspect_reports_platform(emulated, "amd64"));

        // The needle is exact enough: arm64 must not match an arm/v7 entry, amd64 must not match arm64.
        assert!(!DockerToolchain::inspect_reports_platform(
            "Platforms: linux/arm/v7, linux/arm/v6\n",
            "arm64"
        ));
        assert!(!DockerToolchain::inspect_reports_platform(
            "Platforms: linux/arm64\n",
            "amd64"
        ));

        // A non-Platforms line mentioning the arch must not count.
        assert!(!DockerToolchain::inspect_reports_platform(
            "Name: linux/arm64-builder\nPlatforms: linux/amd64\n",
            "arm64"
        ));
    }

    /// The capability decision is pure: a native target, or a non-native one the builder
    /// supports, is allowed; a non-native target the builder can't build is blocked (→ fail-fast).
    #[test]
    fn test_build_blocked() {
        // Native target — allowed regardless of builder support.
        assert!(!DockerToolchain::build_blocked(
            Some("amd64"),
            "amd64",
            false
        ));
        // Non-native but the builder supports it (e.g. a dev machine with emulation) — allowed.
        assert!(!DockerToolchain::build_blocked(
            Some("arm64"),
            "amd64",
            true
        ));
        // Non-native and the builder can't build it (a bare CI runner) — blocked.
        assert!(DockerToolchain::build_blocked(
            Some("arm64"),
            "amd64",
            false
        ));
        // Unrecognized host that can't build the target — blocked.
        assert!(DockerToolchain::build_blocked(None, "amd64", false));
        // Unrecognized host but the builder supports the target — allowed.
        assert!(!DockerToolchain::build_blocked(None, "amd64", true));
    }

    #[test]
    fn select_image_manifest_resolves_nested_index_and_skips_attestation() {
        // The shape a containerd-store `docker save` writes: index.json points at a nested
        // image-index whose entries are the real image manifest plus an attestation.
        let index = json!({
            "schemaVersion": 2,
            "manifests": [
                { "mediaType": "application/vnd.oci.image.index.v1+json", "digest": "sha256:nested" }
            ]
        });
        let mut blobs = HashMap::new();
        blobs.insert(
            "sha256:nested".to_string(),
            json!({
                "manifests": [
                    { "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "sha256:image",
                      "platform": { "architecture": "arm64", "os": "linux" } },
                    { "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "sha256:attest",
                      "platform": { "architecture": "unknown", "os": "unknown" } }
                ]
            }),
        );

        let chosen = select_image_manifest_descriptor(&index, &blobs, "arm64").unwrap();
        assert_eq!(chosen.get("digest").unwrap(), "sha256:image");
        // The nested index is not flat, so a rewrite is required.
        assert!(!index_points_only_at(&index, &chosen));
    }

    #[test]
    fn select_image_manifest_leaves_flat_index_untouched() {
        // The classic image store already writes index.json → single image manifest.
        let index = json!({
            "schemaVersion": 2,
            "manifests": [
                { "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "sha256:image",
                  "platform": { "architecture": "amd64", "os": "linux" } }
            ]
        });
        let chosen = select_image_manifest_descriptor(&index, &HashMap::new(), "amd64").unwrap();
        assert_eq!(chosen.get("digest").unwrap(), "sha256:image");
        assert!(index_points_only_at(&index, &chosen));
    }

    #[test]
    fn select_image_manifest_errors_when_arch_absent() {
        let index = json!({
            "manifests": [
                { "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "sha256:a",
                  "platform": { "architecture": "arm64", "os": "linux" } },
                { "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "sha256:b",
                  "platform": { "architecture": "amd64", "os": "linux" } }
            ]
        });
        assert!(select_image_manifest_descriptor(&index, &HashMap::new(), "ppc64le").is_err());
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
            runtime_platform_name: "aws".to_string(),
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
            runtime_platform_name: "aws".to_string(),
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
            runtime_platform_name: "aws".to_string(),
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
