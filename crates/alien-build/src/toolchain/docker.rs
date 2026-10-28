use super::{Toolchain, ToolchainContext, ToolchainOutput};
use crate::command_output::wait_with_captured_output;
use crate::error::{ErrorData, Result};
use crate::settings::BinaryTargetExt;
use alien_core::AlienEvent;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use flate2::{write::GzEncoder, Compression};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
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

struct BuildxBuilder {
    name: String,
}

impl DockerToolchain {
    const ENTERPRISE_CA_ENV_VARS: [&'static str; 4] = [
        "ALIEN_ENTERPRISE_CA_CERT_PATH",
        "SSL_CERT_FILE",
        "REQUESTS_CA_BUNDLE",
        "NODE_EXTRA_CA_CERTS",
    ];

    /// Check if the source directory contains a Dockerfile
    pub fn has_dockerfile(src_dir: &Path, dockerfile: Option<&String>) -> bool {
        let dockerfile_name = dockerfile.map(|s| s.as_str()).unwrap_or("Dockerfile");
        src_dir.join(dockerfile_name).exists()
    }

    pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf> {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }

        let current_dir =
            std::env::current_dir()
                .into_alien_error()
                .context(Self::docker_build_error(
                    "Failed to resolve the Docker build output directory",
                ))?;

        Ok(current_dir.join(path))
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

    /// The base image buildx names when the registry has no variant of it for the target platform.
    fn base_image_without_platform_in_output(build_output: &str) -> Option<&str> {
        build_output.lines().find_map(|line| {
            let (resolving, _) = line.split_once(": no match for platform in manifest")?;
            resolving
                .rsplit_once("failed to resolve source metadata for ")
                .map(|(_, image)| image)
        })
    }

    /// `ImageBuildFailed` for a docker setup step (no captured build output).
    fn docker_build_error(reason: impl Into<String>) -> ErrorData {
        ErrorData::ImageBuildFailed {
            resource_name: "docker-build".to_string(),
            reason: reason.into(),
            build_output: None,
        }
    }

    fn enterprise_ca_path() -> Result<Option<PathBuf>> {
        Self::enterprise_ca_path_from(|name| std::env::var_os(name))
    }

    fn enterprise_ca_path_from(
        mut env_value: impl FnMut(&str) -> Option<OsString>,
    ) -> Result<Option<PathBuf>> {
        for name in Self::ENTERPRISE_CA_ENV_VARS {
            let Some(value) = env_value(name) else {
                continue;
            };
            let path = PathBuf::from(value);
            let valid = File::open(&path)
                .and_then(|mut file| {
                    let mut first_byte = [0_u8; 1];
                    file.read_exact(&mut first_byte)
                })
                .is_ok();
            if !valid {
                return Err(AlienError::new(Self::docker_build_error(format!(
                    "{name} must point to a readable, non-empty certificate file"
                ))));
            }
            return Ok(Some(path));
        }
        Ok(None)
    }

    fn redacted_build_args(args: &[String]) -> Vec<&str> {
        args.iter()
            .enumerate()
            .map(|(index, arg)| {
                if index > 0 && args[index - 1] == "--secret" {
                    "<redacted>"
                } else {
                    arg.as_str()
                }
            })
            .collect()
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

    fn inspect_field<'a>(inspect_stdout: &'a str, field: &str) -> Option<&'a str> {
        inspect_stdout.lines().find_map(|line| {
            line.strip_prefix(field)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
    }

    fn driver_supports_oci_export(driver: &str) -> bool {
        matches!(
            driver,
            "docker-container" | "kubernetes" | "remote" | "cloud"
        )
    }

    async fn builder_supports_arch(builder_name: &str, target_arch: &str) -> Result<bool> {
        let output = Command::new("docker")
            .args(["buildx", "inspect", builder_name])
            .output()
            .await
            .into_alien_error()
            .context(Self::docker_build_error(
                "Could not inspect the scoped buildx builder",
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

    async fn ensure_builder_supports_arch(builder_name: &str, target_arch: &str) -> Result<()> {
        let host_arch = Self::host_docker_arch();
        if host_arch == Some(target_arch) {
            return Ok(());
        }
        let builder_supports = Self::builder_supports_arch(builder_name, target_arch).await?;
        if Self::build_blocked(host_arch, target_arch, builder_supports) {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "Cannot build linux/{t} on this host: the active buildx builder doesn't support that architecture. Build on a native {t} runner, or configure a buildx builder with emulation for it.",
                t = target_arch
            ))));
        }
        Ok(())
    }

    async fn create_builder(builder_name: &str, endpoint: &str) -> Result<()> {
        let mut command = Command::new("docker");
        command.args([
            "buildx",
            "create",
            "--name",
            builder_name,
            "--driver",
            "docker-container",
        ]);
        if endpoint != "default" {
            command.arg(endpoint);
        }
        let output =
            command
                .output()
                .await
                .into_alien_error()
                .context(Self::docker_build_error(
                    "Could not create a scoped docker-container buildx builder",
                ))?;
        if !output.status.success() {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "Could not create a scoped docker-container buildx builder: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))));
        }

        Self::bootstrap_builder(builder_name).await
    }

    async fn bootstrap_builder(builder_name: &str) -> Result<()> {
        let output = Command::new("docker")
            .args(["buildx", "inspect", "--bootstrap", builder_name])
            .output()
            .await
            .into_alien_error()
            .context(Self::docker_build_error(
                "Could not bootstrap the scoped buildx builder",
            ))?;
        if !output.status.success() {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "Could not bootstrap the scoped buildx builder: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))));
        }
        Ok(())
    }

    async fn select_builder() -> Result<BuildxBuilder> {
        let output = Command::new("docker")
            .args(["buildx", "inspect"])
            .output()
            .await
            .into_alien_error()
            .context(Self::docker_build_error(
                "Could not inspect the current buildx builder",
            ))?;
        if !output.status.success() {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "Could not inspect the current buildx builder: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))));
        }
        let inspect = String::from_utf8_lossy(&output.stdout);
        let name = Self::inspect_field(&inspect, "Name:").ok_or_else(|| {
            AlienError::new(Self::docker_build_error(
                "Current buildx builder inspection did not report a name",
            ))
        })?;
        let driver = Self::inspect_field(&inspect, "Driver:").ok_or_else(|| {
            AlienError::new(Self::docker_build_error(
                "Current buildx builder inspection did not report a driver",
            ))
        })?;

        if Self::driver_supports_oci_export(driver) {
            Self::bootstrap_builder(name).await?;
            return Ok(BuildxBuilder {
                name: name.to_string(),
            });
        }
        if driver != "docker" {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "The current buildx driver '{driver}' does not support OCI archive export"
            ))));
        }

        // Scope the persistent fallback to the actual daemon, not the current source
        // path or process. BuildKit validates content and manages its own cache GC.
        let endpoint = Self::inspect_field(&inspect, "Endpoint:").ok_or_else(|| {
            AlienError::new(Self::docker_build_error(
                "Selected Docker builder did not report an endpoint",
            ))
        })?;
        let mut daemon_command = Command::new("docker");
        if endpoint != "default" {
            daemon_command.args([
                if endpoint.contains("://") {
                    "--host"
                } else {
                    "--context"
                },
                endpoint,
            ]);
        }
        let daemon = daemon_command
            .args(["info", "--format", "{{.ID}}"])
            .output()
            .await
            .into_alien_error()
            .context(Self::docker_build_error(
                "Could not identify the Docker daemon",
            ))?;
        if !daemon.status.success() || daemon.stdout.iter().all(u8::is_ascii_whitespace) {
            return Err(AlienError::new(Self::docker_build_error(
                "Docker did not report a daemon ID for the build cache",
            )));
        }
        let mut scope = Sha256::new();
        scope.update(&daemon.stdout);
        if endpoint != "default" {
            scope.update(b"\0");
            scope.update(endpoint.as_bytes());
        }
        let digest = format!("{:x}", scope.finalize());
        let name = format!("alien-cache-v1-{}", &digest[..16]);
        let existing = Command::new("docker")
            .args(["buildx", "inspect", &name])
            .output()
            .await
            .into_alien_error()
            .context(Self::docker_build_error(
                "Could not inspect the cached builder",
            ))?;
        if !existing.status.success() {
            if let Err(error) = Self::create_builder(&name, endpoint).await {
                // Another process can create the same daemon-scoped builder first.
                // Never remove it: concurrent builds may already be using it.
                let raced = Command::new("docker")
                    .args(["buildx", "inspect", &name])
                    .output()
                    .await
                    .into_alien_error()
                    .context(Self::docker_build_error(
                        "Could not inspect the shared builder",
                    ))?;
                if !raced.status.success() {
                    return Err(error);
                }
            }
        }
        Self::bootstrap_builder(&name).await?;
        Ok(BuildxBuilder { name })
    }
}

impl DockerToolchain {
    pub(crate) async fn build_with_contexts(
        &self,
        context: &ToolchainContext,
        additional_contexts: &[(&str, &Path)],
    ) -> Result<ToolchainOutput> {
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
        let output_tarball = Self::absolute_path(&context.build_dir.join(format!(
            "{}.oci.tar",
            context.build_target.runtime_platform_id()
        )))?;
        let lambda_worker = context.runtime_platform_name == "aws"
            && context.workload == super::WorkloadKind::Worker;
        let output = if lambda_worker {
            format!(
                "type=oci,dest={},compression=gzip,force-compression=true,rewrite-timestamp=true",
                output_tarball.display()
            )
        } else {
            format!(
                "type=oci,dest={},rewrite-timestamp=true",
                output_tarball.display()
            )
        };
        let arch_str = match context.build_target.to_dockdash_arch() {
            dockdash::Arch::Amd64 => "amd64",
            dockdash::Arch::ARM64 => "arm64",
            _ => "amd64", // Fallback for other architectures
        };
        let platform_str = format!("linux/{}", arch_str);

        let builder = Self::select_builder().await?;
        Self::ensure_builder_supports_arch(&builder.name, arch_str).await?;
        let cache = super::docker_cache::DockerCache::restore(context, self).await?;

        let mut args = vec![
            "buildx".to_string(),
            "build".to_string(),
            "--builder".to_string(),
            builder.name.clone(),
            "--platform".to_string(),
            platform_str.clone(),
            "--output".to_string(),
            output,
            "-f".to_string(),
            dockerfile_name.to_string(),
        ];

        if let Some(cache) = &cache {
            cache.append_args(&mut args);
        }

        if lambda_worker {
            // Lambda accepts a single architecture and no attestation manifest.
            args.push("--provenance=false".to_string());
            args.push("--sbom=false".to_string());
        }

        // Cold builds must not change image digests because of build time or
        // checkout mtimes. Preserve an explicitly configured source timestamp.
        if !self
            .build_args
            .as_ref()
            .is_some_and(|args| args.contains_key("SOURCE_DATE_EPOCH"))
            && std::env::var_os("SOURCE_DATE_EPOCH").is_none()
        {
            args.push("--build-arg".to_string());
            args.push("SOURCE_DATE_EPOCH=0".to_string());
        }

        // Add build args if provided
        let build_arg_strings: Vec<String> = self
            .build_args
            .as_ref()
            .map(|args| args.iter().map(|(k, v)| format!("{}={}", k, v)).collect())
            .unwrap_or_default();

        for build_arg in &build_arg_strings {
            args.push("--build-arg".to_string());
            args.push(build_arg.clone());
        }

        let enterprise_ca = Self::enterprise_ca_path()?;
        if let Some(path) = &enterprise_ca {
            args.push("--secret".to_string());
            args.push(format!("id=enterprise_ca,src={}", path.display()));
        }

        if context.pull_base_images {
            args.push("--pull".to_string());
        }

        // Add target if specified
        if let Some(target) = &self.target {
            args.push("--target".to_string());
            args.push(target.clone());
        }

        for (name, path) in additional_contexts {
            args.push("--build-context".to_string());
            args.push(format!("{name}={}", path.display()));
        }

        // Add build context
        args.push(".".to_string()); // Build context is the src_dir

        info!(
            "Running docker buildx build with args: {:?}",
            Self::redacted_build_args(&args)
        );

        // Run docker buildx build with progress reporting
        let build_result = AlienEvent::CompilingCode {
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
                if let Some(image) = Self::base_image_without_platform_in_output(&build_output) {
                    return Err(AlienError::new(ErrorData::BaseImagePlatformMissing {
                        image: image.to_string(),
                        platform: platform_str,
                    }));
                }
                return Err(AlienError::new(ErrorData::ImageBuildFailed {
                    resource_name: "docker-build".to_string(),
                    reason: Self::humanize_buildx_failure(&build_output),
                    build_output: Some(build_output),
                }));
            }

            info!("docker buildx build completed successfully");
            Ok(())
        })
        .await;

        build_result?;
        if let Some(cache) = cache {
            cache.save().await?;
        }

        // Buildx may wrap the image manifest in an index alongside provenance attestations.
        // Flatten the archive before the runtime OCI reader sees it.
        let archive_path = output_tarball.clone();
        let runtime_command = tokio::task::spawn_blocking(move || {
            Self::normalize_oci_archive(&archive_path, arch_str)?;
            Self::extract_cmd_from_tarball(&archive_path)
        })
        .await
        .into_alien_error()
        .context(docker_read_error("Image archive normalization task failed"))??;

        info!("Extracted CMD from Docker image: {:?}", runtime_command);

        // Docker builds produce complete OCI images - return absolute path
        // The build system will detect if source == dest and skip the copy
        Ok(ToolchainOutput {
            build_strategy: super::ImageBuildStrategy::CompleteOCITarball {
                tarball_path: output_tarball,
            },
            // The Dockerfile fully controls the image; keep its own entrypoint/cmd.
            entrypoint: None,
            runtime_command,
        })
    }
}

#[async_trait]
impl Toolchain for DockerToolchain {
    async fn build(&self, context: &ToolchainContext) -> Result<ToolchainOutput> {
        self.build_with_contexts(context, &[]).await
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
    /// Buildx can write a nested `index.json` → image-index → [image manifest, attestation];
    /// our reader (ocipkg, via dockdash) treats the first
    /// descriptor as an image manifest and fails with "missing field `config`". The classic
    /// store already writes a flat layout, so this is a no-op there.
    pub(crate) fn normalize_oci_archive(tarball_path: &Path, target_arch: &str) -> Result<()> {
        // Buffer the small JSON entries (index + manifests); layer blobs are large and not
        // needed to resolve the descriptor, so skip reading their bodies.
        const SMALL_BLOB_BYTES: u64 = 1 << 20;
        let mut index: Option<Value> = None;
        let mut blobs: HashMap<String, Value> = HashMap::new();
        let mut saved_manifest = None;

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
            } else if path == "manifest.json" {
                saved_manifest = Some(read_entry_json(&mut entry)?);
            } else if path.starts_with("blobs/") && entry.size() <= SMALL_BLOB_BYTES {
                // Only descriptors are stored by digest; key small blobs by "sha256:<hex>".
                if let Some(digest) = blob_path_to_digest(&path) {
                    if let Ok(value) = read_entry_json(&mut entry) {
                        blobs.insert(digest, value);
                    }
                }
            }
        }

        let Some(index) = index else {
            let saved_manifest = saved_manifest.ok_or_else(|| {
                AlienError::new(docker_read_error(
                    "Image archive has neither an OCI index nor a Docker manifest",
                ))
            })?;
            return convert_docker_archive(tarball_path, target_arch, &saved_manifest);
        };

        let chosen = select_image_manifest_descriptor(&index, &blobs, target_arch)?;
        let new_index = if index_points_only_at(&index, &chosen) {
            None
        } else {
            Some(json!({
                "schemaVersion": 2,
                "mediaType": "application/vnd.oci.image.index.v1+json",
                "manifests": [chosen.clone()],
            }))
        };
        let docker_manifest = if saved_manifest.is_some() {
            None
        } else {
            Some(docker_manifest_for_descriptor(&chosen, &blobs)?)
        };

        if new_index.is_none() && docker_manifest.is_none() {
            return Ok(());
        }

        let new_index_bytes = new_index
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .into_alien_error()
            .context(docker_read_error(
                "Failed to serialize flattened index.json",
            ))?;
        let docker_manifest_bytes = docker_manifest
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .into_alien_error()
            .context(docker_read_error(
                "Failed to serialize Docker manifest.json",
            ))?;

        rewrite_archive(
            tarball_path,
            new_index_bytes.as_deref(),
            docker_manifest_bytes.as_deref(),
        )
    }
}

struct HashingWriter<W> {
    inner: W,
    digest: Sha256,
    size: u64,
}

impl<W> HashingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
            size: 0,
        }
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.digest.update(&bytes[..written]);
        self.size += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Convert a classic `docker save` archive without executing or altering the image.
/// Keep the original config (including ONBUILD) and stream layers instead of buffering them.
/// Gzip layers like the buildx OCI exporter; config diff IDs still name the uncompressed tar.
fn convert_docker_archive(path: &Path, target_arch: &str, saved: &Value) -> Result<()> {
    let failure = || docker_read_error("Invalid single-image Docker archive");
    let images = saved
        .as_array()
        .filter(|images| images.len() == 1)
        .ok_or_else(|| AlienError::new(failure()))?;
    let config_path = images[0]["Config"]
        .as_str()
        .ok_or_else(|| AlienError::new(failure()))?;
    let layers = images[0]["Layers"]
        .as_array()
        .ok_or_else(|| AlienError::new(failure()))?
        .iter()
        .map(|layer| layer.as_str().ok_or_else(|| AlienError::new(failure())))
        .collect::<Result<Vec<_>>>()?;
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut descriptors = HashMap::new();
    let mut compressed_layers = HashMap::new();
    let mut layer_links = HashMap::new();
    let mut layer_diff_ids = HashMap::new();
    let mut expected_diff_ids: Option<Vec<String>> = None;
    let source = File::open(path).into_alien_error().context(failure())?;
    let mut archive = tar::Archive::new(source);
    for entry in archive.entries().into_alien_error().context(failure())? {
        let mut entry = entry.into_alien_error().context(failure())?;
        let name = entry
            .path()
            .into_alien_error()
            .context(failure())?
            .to_string_lossy()
            .into_owned();
        if name != config_path && !layers.contains(&name.as_str()) {
            continue;
        }
        if descriptors.contains_key(&name) || layer_links.contains_key(&name) {
            return Err(AlienError::new(failure()));
        }
        // Docker's classic store deduplicates repeated layers with relative symlinks.
        // Resolve only manifest-listed layers, without extracting or following host paths.
        if name != config_path && entry.header().entry_type().is_symlink() {
            let target = entry
                .link_name()
                .into_alien_error()
                .context(failure())?
                .ok_or_else(|| AlienError::new(failure()))?;
            let target = docker_layer_link_target(Path::new(&name), &target)
                .filter(|target| layers.contains(&target.as_str()))
                .ok_or_else(|| {
                    AlienError::new(docker_read_error(
                        "Docker archive layer link target is invalid",
                    ))
                })?;
            if entry.size() != 0 {
                return Err(AlienError::new(failure()));
            }
            layer_links.insert(name, target);
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(AlienError::new(failure()));
        }
        let mut size = entry.size();
        let mut digest = Sha256::new();
        if name == config_path {
            if size > 1 << 20 {
                return Err(AlienError::new(failure()));
            }
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .into_alien_error()
                .context(failure())?;
            let config: Value = serde_json::from_slice(&bytes)
                .into_alien_error()
                .context(failure())?;
            if config["architecture"] != target_arch || config["os"] != "linux" {
                return Err(AlienError::new(docker_read_error(
                    "Saved image does not match the requested platform",
                )));
            }
            expected_diff_ids = Some(
                config["rootfs"]["diff_ids"]
                    .as_array()
                    .filter(|ids| config["rootfs"]["type"] == "layers" && ids.len() == layers.len())
                    .ok_or_else(|| AlienError::new(failure()))?
                    .iter()
                    .map(|id| {
                        id.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| AlienError::new(failure()))
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
            digest.update(&bytes);
        } else {
            let compressed = tempfile::tempfile_in(directory)
                .into_alien_error()
                .context(failure())?;
            // Hash each side of gzip as bytes pass through; diff IDs refer to the input,
            // while OCI descriptors refer to the compressed bytes stored in the archive.
            let mut uncompressed = HashingWriter::new(GzEncoder::new(
                HashingWriter::new(compressed),
                Compression::default(),
            ));
            std::io::copy(&mut entry, &mut uncompressed)
                .into_alien_error()
                .context(failure())?;
            layer_diff_ids.insert(
                name.clone(),
                format!("sha256:{:x}", uncompressed.digest.finalize()),
            );
            let compressed = uncompressed
                .inner
                .finish()
                .into_alien_error()
                .context(failure())?;
            size = compressed.size;
            digest = compressed.digest;
            let mut compressed = compressed.inner;
            compressed.rewind().into_alien_error().context(failure())?;
            compressed_layers.insert(name.clone(), compressed);
        }
        let digest = format!("sha256:{:x}", digest.finalize());
        descriptors.insert(name.clone(), json!({
            "mediaType": if name == config_path { "application/vnd.oci.image.config.v1+json" } else { "application/vnd.oci.image.layer.v1.tar+gzip" },
            "digest": digest, "size": size,
        }));
    }
    let config = descriptors
        .get(config_path)
        .ok_or_else(|| AlienError::new(failure()))?;
    let expected_diff_ids = expected_diff_ids.ok_or_else(|| AlienError::new(failure()))?;
    let layer_descriptors = layers
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let mut target = *name;
            // A valid chain reaches a regular layer before visiting every link.
            for _ in 0..=layer_links.len() {
                if let Some(descriptor) = descriptors.get(target) {
                    if layer_diff_ids.get(target) != Some(&expected_diff_ids[index]) {
                        return Err(AlienError::new(docker_read_error(
                            "Docker archive layer digest does not match config rootfs.diff_ids",
                        )));
                    }
                    return Ok(descriptor.clone());
                }
                target = layer_links
                    .get(target)
                    .map(String::as_str)
                    .ok_or_else(|| AlienError::new(failure()))?;
            }
            Err(AlienError::new(docker_read_error(
                "Docker archive contains a cyclic layer link",
            )))
        })
        .collect::<Result<Vec<_>>>()?;
    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": config, "layers": layer_descriptors,
    }))
    .into_alien_error()
    .context(failure())?;
    let manifest_digest = format!("sha256:{:x}", Sha256::digest(&manifest));
    let index = json!({"schemaVersion": 2, "manifests": [{
        "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": manifest_digest,
        "size": manifest.len(), "platform": {"os":"linux", "architecture":target_arch},
    }]});
    let blob_path = |descriptor: &Value| {
        descriptor["digest"]
            .as_str()
            .and_then(digest_to_blob_path)
            .ok_or_else(|| AlienError::new(failure()))
    };
    let docker_manifest = json!([{"Config": blob_path(config)?, "RepoTags": images[0]["RepoTags"],
        "Layers": layer_descriptors.iter().map(blob_path).collect::<Result<Vec<_>>>()?}]);
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .into_alien_error()
        .context(failure())?;
    let mut builder = tar::Builder::new(temporary.as_file_mut());
    let mut archive = tar::Archive::new(File::open(path).into_alien_error().context(failure())?);
    for entry in archive.entries().into_alien_error().context(failure())? {
        let mut entry = entry.into_alien_error().context(failure())?;
        let name = entry
            .path()
            .into_alien_error()
            .context(failure())?
            .to_string_lossy()
            .into_owned();
        if let Some(descriptor) = descriptors.get(&name) {
            let mut header = entry.header().clone();
            let source: &mut dyn Read = if let Some(compressed) = compressed_layers.get_mut(&name) {
                header.set_size(
                    descriptor["size"]
                        .as_u64()
                        .ok_or_else(|| AlienError::new(failure()))?,
                );
                compressed
            } else {
                &mut entry
            };
            header.set_cksum();
            builder
                .append_data(&mut header, blob_path(descriptor)?, source)
                .into_alien_error()
                .context(failure())?;
        }
    }
    for (name, bytes) in [
        (
            digest_to_blob_path(&manifest_digest).ok_or_else(|| AlienError::new(failure()))?,
            manifest,
        ),
        (
            "index.json".to_string(),
            serde_json::to_vec(&index)
                .into_alien_error()
                .context(failure())?,
        ),
        (
            "oci-layout".to_string(),
            br#"{"imageLayoutVersion":"1.0.0"}"#.to_vec(),
        ),
        (
            "manifest.json".to_string(),
            serde_json::to_vec(&docker_manifest)
                .into_alien_error()
                .context(failure())?,
        ),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, name, bytes.as_slice())
            .into_alien_error()
            .context(failure())?;
    }
    builder.finish().into_alien_error().context(failure())?;
    drop(builder);
    temporary
        .persist(path)
        .map(|_| ())
        .into_alien_error()
        .context(failure())
}

fn docker_layer_link_target(name: &Path, target: &Path) -> Option<String> {
    // Docker archive paths use Unix separators, regardless of the CLI host.
    if target.to_str()?.contains('\\') {
        return None;
    }
    let mut resolved = name.parent()?.to_path_buf();
    for component in target.components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !resolved.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    resolved.into_os_string().into_string().ok()
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

/// The blob a descriptor points at, when the archive carried it.
fn descriptor_blob<'a>(descriptor: &Value, blobs: &'a HashMap<String, Value>) -> Option<&'a Value> {
    blobs.get(descriptor.get("digest")?.as_str()?)
}

/// Architecture of an image manifest: the descriptor's platform when the index
/// records one, else the `architecture` of the manifest's config blob. A
/// containerd-store `docker save` of an image pulled as a single manifest
/// writes descriptors without a platform.
fn manifest_architecture<'a>(
    descriptor: &'a Value,
    blobs: &'a HashMap<String, Value>,
) -> Option<&'a str> {
    descriptor_architecture(descriptor).or_else(|| {
        let manifest = descriptor_blob(descriptor, blobs)?;
        let config = descriptor_blob(manifest.get("config")?, blobs)?;
        config.get("architecture")?.as_str()
    })
}

/// Whether a manifest is a referrer of another manifest (an attestation or SBOM
/// linked through `subject`) rather than an image. `docker save` exports them
/// next to the image they describe.
fn is_referrer(descriptor: &Value, blobs: &HashMap<String, Value>) -> bool {
    descriptor
        .get("annotations")
        .and_then(|annotations| annotations.get("io.containerd.manifest.subject"))
        .is_some()
        || descriptor_blob(descriptor, blobs)
            .is_some_and(|manifest| manifest.get("subject").is_some())
}

/// Walk an index's descriptors (following nested indexes) and collect the real image
/// manifests, skipping attestations (platform `unknown/unknown` or a `subject`).
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
            if let Some(nested) = descriptor_blob(descriptor, blobs) {
                collect_image_manifests(nested.get("manifests"), blobs, out);
            }
        } else if is_manifest_media_type(media_type)
            && descriptor_architecture(descriptor) != Some("unknown")
            && !is_referrer(descriptor, blobs)
        {
            out.push(descriptor.clone());
        }
    }
}

/// Resolve the one image-manifest descriptor to keep: the one built for `target_arch`.
/// A lone image whose architecture is recorded nowhere is kept as is. A lone image
/// built for another architecture is refused rather than shipped to hosts that
/// cannot run it.
fn select_image_manifest_descriptor(
    index: &Value,
    blobs: &HashMap<String, Value>,
    target_arch: &str,
) -> Result<Value> {
    let mut candidates = Vec::new();
    collect_image_manifests(index.get("manifests"), blobs, &mut candidates);

    if let Some(descriptor) = candidates
        .iter()
        .find(|d| manifest_architecture(d, blobs) == Some(target_arch))
    {
        return Ok(descriptor.clone());
    }
    let found: Vec<&str> = candidates
        .iter()
        .filter_map(|d| manifest_architecture(d, blobs))
        .collect();
    match (candidates.as_slice(), found.as_slice()) {
        ([only], []) => Ok(only.clone()),
        ([_], [built_for]) => Err(AlienError::new(docker_read_error(&format!(
            "built for {built_for} only, but this release needs {target_arch}; \
             publish a multi-architecture image that includes linux/{target_arch}, or set \
             `architecture: \"{}\"` in the requirements of the pool that runs it",
            stack_architecture_name(built_for)
        )))),
        _ => Err(AlienError::new(docker_read_error(&format!(
            "OCI archive has no image manifest for architecture '{target_arch}' (found: {})",
            if found.is_empty() {
                "none".to_string()
            } else {
                found.join(", ")
            }
        )))),
    }
}

/// The stack's spelling of an OCI architecture, as accepted by a pool's `requirements.architecture`.
fn stack_architecture_name(oci_arch: &str) -> &str {
    match oci_arch {
        "amd64" => "x86_64",
        other => other,
    }
}

/// Whether `index.json` already contains exactly the chosen descriptor (classic store).
fn index_points_only_at(index: &Value, chosen: &Value) -> bool {
    index
        .get("manifests")
        .and_then(|m| m.as_array())
        .is_some_and(|m| m.len() == 1 && m[0].get("digest") == chosen.get("digest"))
}

/// Build the compatibility manifest consumed by `docker load` from the selected OCI image
/// manifest. The paths continue to reference the same content-addressed blobs, so the archive
/// stays a valid OCI layout for remote ingestion while also being loadable by a local daemon.
fn docker_manifest_for_descriptor(
    descriptor: &Value,
    blobs: &HashMap<String, Value>,
) -> Result<Value> {
    let digest = descriptor
        .get("digest")
        .and_then(Value::as_str)
        .ok_or_else(|| AlienError::new(docker_read_error("OCI image descriptor has no digest")))?;
    let manifest = blobs.get(digest).ok_or_else(|| {
        AlienError::new(docker_read_error(
            "OCI archive is missing the selected image manifest blob",
        ))
    })?;
    let config = manifest
        .get("config")
        .and_then(|value| value.get("digest"))
        .and_then(Value::as_str)
        .and_then(digest_to_blob_path)
        .ok_or_else(|| {
            AlienError::new(docker_read_error(
                "OCI image manifest has no valid config digest",
            ))
        })?;
    let layers = manifest
        .get("layers")
        .and_then(Value::as_array)
        .ok_or_else(|| AlienError::new(docker_read_error("OCI image manifest has no layers")))?
        .iter()
        .map(|layer| {
            layer
                .get("digest")
                .and_then(Value::as_str)
                .and_then(digest_to_blob_path)
                .ok_or_else(|| {
                    AlienError::new(docker_read_error(
                        "OCI image manifest has an invalid layer digest",
                    ))
                })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(json!([{
        "Config": config,
        "RepoTags": null,
        "Layers": layers,
    }]))
}

fn digest_to_blob_path(digest: &str) -> Option<String> {
    let (algorithm, hex) = digest.split_once(':')?;
    if algorithm != "sha256" || hex.is_empty() {
        return None;
    }
    Some(format!("blobs/{algorithm}/{hex}"))
}

/// Stream the archive into a sibling temp file, replacing only `index.json`, then swap it
/// in. Other entries (blobs, oci-layout) are copied verbatim.
fn rewrite_archive(
    tarball_path: &Path,
    new_index: Option<&[u8]>,
    docker_manifest: Option<&[u8]>,
) -> Result<()> {
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

            if path == Path::new("index.json") && new_index.is_some() {
                let new_index = new_index.expect("checked above");
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

        if let Some(docker_manifest) = docker_manifest {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(docker_manifest.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, "manifest.json", docker_manifest)
                .into_alien_error()
                .context(docker_read_error("Failed to write Docker manifest.json"))?;
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

#[cfg(all(test, unix))]
#[path = "docker_builder_tests.rs"]
mod builder_tests;

#[cfg(test)]
#[path = "docker_repro_tests.rs"]
mod repro_tests;

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

    /// Buildx output for a linux/amd64 build FROM an image that publishes only linux/arm64.
    #[test]
    fn a_base_without_the_target_platform_is_named_from_the_build_output() {
        let output = "#3 [internal] load metadata for public.ecr.aws/lambda/microvms:al2023-minimal\n\
            #3 ERROR: no match for platform in manifest: not found\n\
            ERROR: failed to build: failed to solve: public.ecr.aws/lambda/microvms:al2023-minimal: \
            failed to resolve source metadata for public.ecr.aws/lambda/microvms:al2023-minimal: \
            no match for platform in manifest: not found\n";
        assert_eq!(
            DockerToolchain::base_image_without_platform_in_output(output),
            Some("public.ecr.aws/lambda/microvms:al2023-minimal")
        );
        assert_eq!(
            DockerToolchain::base_image_without_platform_in_output(
                "ERROR: failed to build: failed to solve: process \"/bin/sh -c false\" did not complete successfully: exit code: 1\n"
            ),
            None
        );
    }

    fn saved_image_with_layer_links(path: &Path, links: &[(&str, &str)]) -> Vec<u8> {
        // Real reproduction: on a Docker daemon using the classic image store,
        // `docker pull --platform linux/arm64 clickhouse/clickhouse-server:26.3`
        // then `docker save -o image.tar clickhouse/clickhouse-server:26.3` emits
        // repeated layer.tar entries as relative symlinks (inspect with tar -tvf).
        let layer = vec![0_u8; 1024]; // A valid empty layer tar.
        let digest = format!("sha256:{:x}", Sha256::digest(&layer));
        let mut layers: Vec<_> = links.iter().map(|(name, _)| *name).collect();
        layers.push("original/layer.tar");
        let config = serde_json::to_vec(&json!({
            "architecture": "arm64", "os": "linux",
            "config": {"Cmd": ["/app"], "OnBuild": ["RUN false"]},
            "rootfs": {"type": "layers", "diff_ids": vec![digest; layers.len()]},
            "history": vec![json!({"created_by": "RUN true"}); layers.len()],
        }))
        .unwrap();
        let manifest = serde_json::to_vec(&json!([{
            "Config": "config.json", "RepoTags": ["archive-links:test"], "Layers": layers,
        }]))
        .unwrap();
        let mut builder = tar::Builder::new(File::create(path).unwrap());
        // Put links before their targets, as docker save can do.
        for (name, target) in links {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_mode(0o777);
            header.set_size(0);
            builder.append_link(&mut header, name, target).unwrap();
        }
        for (name, bytes) in [
            ("config.json", config.as_slice()),
            ("original/layer.tar", layer.as_slice()),
            ("manifest.json", manifest.as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            builder.append_data(&mut header, name, bytes).unwrap();
        }
        builder.finish().unwrap();
        config
    }

    fn change_saved_entry(
        path: &Path,
        name: &str,
        change: impl FnOnce(&mut tar::Header, &mut Vec<u8>),
    ) {
        let mut archive = tar::Archive::new(File::open(path).unwrap());
        let mut builder = tar::Builder::new(Vec::new());
        let mut change = Some(change);
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let entry_name = entry.path().unwrap().into_owned();
            let mut header = entry.header().clone();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            if entry_name == Path::new(name) {
                change.take().expect("one matching entry")(&mut header, &mut bytes);
            }
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, entry_name, bytes.as_slice())
                .unwrap();
        }
        assert!(change.is_none(), "entry to change must exist");
        std::fs::write(path, builder.into_inner().unwrap()).unwrap();
    }

    #[test]
    fn saved_image_rejects_unsupported_link_entries_without_replacing_source() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.tar");
        for case in [
            "hardlink",
            "nonempty-link",
            "config-link",
            "directory-target",
        ] {
            saved_image_with_layer_links(&path, &[("linked/layer.tar", "../original/layer.tar")]);
            let entry = match case {
                "config-link" => "config.json",
                "directory-target" => "original/layer.tar",
                _ => "linked/layer.tar",
            };
            change_saved_entry(&path, entry, |header, bytes| match case {
                "hardlink" => header.set_entry_type(tar::EntryType::Link),
                "nonempty-link" => bytes.push(1),
                "config-link" => {
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_link_name("original/layer.tar").unwrap();
                    bytes.clear();
                }
                "directory-target" => {
                    header.set_entry_type(tar::EntryType::Directory);
                    bytes.clear();
                }
                _ => unreachable!(),
            });
            let original = std::fs::read(&path).unwrap();
            assert!(
                DockerToolchain::normalize_oci_archive(&path, "arm64").is_err(),
                "accepted {case}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), original, "replaced {case}");
        }
    }

    #[test]
    fn saved_image_rejects_layer_content_that_disagrees_with_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.tar");
        saved_image_with_layer_links(&path, &[("linked/layer.tar", "../original/layer.tar")]);
        change_saved_entry(&path, "original/layer.tar", |_, bytes| bytes[0] ^= 1);
        let original = std::fs::read(&path).unwrap();
        let error = DockerToolchain::normalize_oci_archive(&path, "arm64").unwrap_err();
        assert!(error
            .to_string()
            .contains("layer digest does not match config rootfs.diff_ids"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn saved_image_requires_one_config_diff_id_per_layer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.tar");
        saved_image_with_layer_links(&path, &[("linked/layer.tar", "../original/layer.tar")]);
        change_saved_entry(&path, "config.json", |_, bytes| {
            let mut config: Value = serde_json::from_slice(bytes).unwrap();
            config["rootfs"]["diff_ids"].as_array_mut().unwrap().pop();
            *bytes = serde_json::to_vec(&config).unwrap();
        });
        let original = std::fs::read(&path).unwrap();
        assert!(DockerToolchain::normalize_oci_archive(&path, "arm64").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn saved_image_resolves_repeated_layers_without_changing_image_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.tar");
        let config = saved_image_with_layer_links(
            &path,
            &[
                ("first/layer.tar", "../second/layer.tar"),
                ("second/layer.tar", "../original/layer.tar"),
            ],
        );
        DockerToolchain::normalize_oci_archive(&path, "arm64").unwrap();
        let metadata = Image::from_tarball(&path).unwrap().get_metadata().unwrap();
        assert_eq!(metadata.cmd.unwrap(), vec!["/app"]);
        let mut archive = tar::Archive::new(File::open(&path).unwrap());
        let mut contents = HashMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            assert!(entry.header().entry_type().is_file());
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            assert!(contents.insert(name, bytes).is_none());
        }
        let manifest: Value = serde_json::from_slice(&contents["manifest.json"]).unwrap();
        assert_eq!(contents[manifest[0]["Config"].as_str().unwrap()], config);
        let layers = manifest[0]["Layers"].as_array().unwrap();
        let config: Value = serde_json::from_slice(&config).unwrap();
        assert_eq!(layers.len(), 3);
        for (position, layer) in layers.iter().enumerate() {
            assert_eq!(layer, &layers[0]);
            let compressed = &contents[layer.as_str().unwrap()];
            assert!(compressed.len() < 1024);
            let mut decoded = Vec::new();
            flate2::read::GzDecoder::new(compressed.as_slice())
                .read_to_end(&mut decoded)
                .unwrap();
            assert_eq!(decoded, vec![0_u8; 1024]);
            assert_eq!(
                config["rootfs"]["diff_ids"][position],
                format!("sha256:{:x}", Sha256::digest(&decoded))
            );
        }
    }

    #[test]
    fn saved_image_preserves_distinct_layer_order_across_links() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.tar");
        let [first, second] = [b"first".as_slice(), b"second".as_slice()].map(|payload| {
            let mut builder = tar::Builder::new(Vec::new());
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(payload.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, "payload", payload)
                .unwrap();
            builder.into_inner().unwrap()
        });
        let first_digest = format!("sha256:{:x}", Sha256::digest(&first));
        let second_digest = format!("sha256:{:x}", Sha256::digest(&second));
        let expected = [
            first_digest.as_str(),
            second_digest.as_str(),
            first_digest.as_str(),
            second_digest.as_str(),
        ];
        let config = serde_json::to_vec(&json!({
            "architecture": "arm64", "os": "linux", "config": {"Cmd": ["/app"]},
            "rootfs": {"type": "layers", "diff_ids": expected},
            "history": vec![json!({"created_by": "COPY payload /payload"}); 4],
        }))
        .unwrap();
        let manifest = serde_json::to_vec(&json!([{
            "Config": "config.json", "RepoTags": ["archive-order:test"],
            "Layers": ["linked/layer.tar", "second/layer.tar", "first/layer.tar", "repeated/layer.tar"],
        }]))
        .unwrap();
        let mut builder = tar::Builder::new(File::create(&path).unwrap());
        for (name, target) in [
            ("linked/layer.tar", "../first/layer.tar"),
            ("repeated/layer.tar", "../second/layer.tar"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_mode(0o777);
            header.set_size(0);
            builder.append_link(&mut header, name, target).unwrap();
        }
        // Archive order deliberately differs from the image's layer order.
        for (name, bytes) in [
            ("second/layer.tar", second.as_slice()),
            ("first/layer.tar", first.as_slice()),
            ("config.json", config.as_slice()),
            ("manifest.json", manifest.as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            builder.append_data(&mut header, name, bytes).unwrap();
        }
        builder.finish().unwrap();
        drop(builder);

        DockerToolchain::normalize_oci_archive(&path, "arm64").unwrap();
        let mut archive = tar::Archive::new(File::open(&path).unwrap());
        let mut contents = HashMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            assert!(contents.insert(name, bytes).is_none());
        }
        let index: Value = serde_json::from_slice(&contents["index.json"]).unwrap();
        let manifest_path =
            digest_to_blob_path(index["manifests"][0]["digest"].as_str().unwrap()).unwrap();
        let oci: Value = serde_json::from_slice(&contents[&manifest_path]).unwrap();
        assert_eq!(oci["layers"].as_array().unwrap().len(), 4);
        let mut expected_paths = Vec::new();
        for (layer, expected_bytes) in oci["layers"].as_array().unwrap().iter().zip([
            first.as_slice(),
            second.as_slice(),
            first.as_slice(),
            second.as_slice(),
        ]) {
            assert_eq!(
                layer["mediaType"],
                "application/vnd.oci.image.layer.v1.tar+gzip"
            );
            let path = digest_to_blob_path(layer["digest"].as_str().unwrap()).unwrap();
            let compressed = &contents[&path];
            assert_eq!(layer["size"], compressed.len());
            assert_eq!(
                layer["digest"],
                format!("sha256:{:x}", Sha256::digest(compressed))
            );
            let mut decoded = Vec::new();
            flate2::read::GzDecoder::new(compressed.as_slice())
                .read_to_end(&mut decoded)
                .unwrap();
            assert_eq!(decoded, expected_bytes);
            expected_paths.push(path);
        }
        assert_eq!(expected_paths.len(), 4);
        assert_eq!(expected_paths[0], expected_paths[2]);
        assert_eq!(expected_paths[1], expected_paths[3]);
        let docker: Value = serde_json::from_slice(&contents["manifest.json"]).unwrap();
        assert_eq!(docker[0]["Layers"], json!(expected_paths));
        assert_eq!(contents[docker[0]["Config"].as_str().unwrap()], config);
    }

    #[test]
    fn saved_image_rejects_invalid_layer_links_without_replacing_source() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.tar");
        for target in [
            "../missing/layer.tar",
            "../config.json",
            "/original/layer.tar",
            "../../original/layer.tar",
            "../original\\layer.tar",
        ] {
            saved_image_with_layer_links(&path, &[("linked/layer.tar", target)]);
            let original = std::fs::read(&path).unwrap();
            let error = DockerToolchain::normalize_oci_archive(&path, "arm64").unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("Docker archive layer link target is invalid"),
                "wrong error for invalid layer link {target}: {error}",
            );
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
        for links in [
            vec![("first/layer.tar", "layer.tar")],
            vec![
                ("first/layer.tar", "../second/layer.tar"),
                ("second/layer.tar", "../first/layer.tar"),
            ],
        ] {
            saved_image_with_layer_links(&path, &links);
            let original = std::fs::read(&path).unwrap();
            let error = DockerToolchain::normalize_oci_archive(&path, "arm64").unwrap_err();
            assert!(error.to_string().contains("cyclic layer link"));
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
    }

    /// Requires a running Docker daemon with the classic image store
    /// (containerd image store disabled). Run explicitly with:
    /// `cargo test -p alien-build --lib saved_image_preserves_config_and_layers_without_running_onbuild -- --ignored`
    #[test]
    #[ignore = "requires a Docker daemon with the classic image store; see test documentation"]
    fn saved_image_preserves_config_and_layers_without_running_onbuild() {
        assert!(
            docker_available(),
            "A running classic-store Docker daemon is required"
        );
        let dir = tempdir().unwrap();
        let tag = format!("alien-export-onbuild-{}", rand::random::<u64>());
        std::fs::write(dir.path().join("payload"), "original layer").unwrap();
        std::fs::write(dir.path().join("Dockerfile"),
            "FROM scratch\nCOPY payload /payload\nONBUILD RUN false\nONBUILD COPY missing /missing\nCMD [\"/payload\"]\n").unwrap();
        let build = Command::new("docker")
            .args(["build", "--platform", "linux/amd64", "-t", &tag])
            .arg(dir.path())
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let inspect = Command::new("docker")
            .args(["image", "inspect", "--format", "{{.Id}}", &tag])
            .output()
            .unwrap();
        assert!(inspect.status.success());
        let original_id = String::from_utf8(inspect.stdout)
            .unwrap()
            .trim()
            .to_string();
        let archive_path = dir.path().join("image.tar");
        let save = Command::new("docker")
            .args(["save", "-o"])
            .arg(&archive_path)
            .arg(&tag)
            .output()
            .unwrap();
        assert!(
            save.status.success(),
            "{}",
            String::from_utf8_lossy(&save.stderr)
        );
        let mut saved = HashMap::new();
        for entry in tar::Archive::new(File::open(&archive_path).unwrap())
            .entries()
            .unwrap()
        {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            saved.insert(name, bytes);
        }
        assert!(
            !saved.contains_key("index.json"),
            "expected classic docker save format"
        );
        let saved_manifest: Value = serde_json::from_slice(&saved["manifest.json"]).unwrap();
        let layer_name = saved_manifest[0]["Layers"][0].as_str().unwrap();
        let tampered_path = dir.path().join("tampered.tar");
        std::fs::copy(&archive_path, &tampered_path).unwrap();
        change_saved_entry(&tampered_path, layer_name, |_, bytes| {
            *bytes.last_mut().unwrap() ^= 1;
        });
        let tampered = std::fs::read(&tampered_path).unwrap();
        let error = DockerToolchain::normalize_oci_archive(&tampered_path, "amd64").unwrap_err();
        assert!(error
            .to_string()
            .contains("does not match config rootfs.diff_ids"));
        assert_eq!(std::fs::read(&tampered_path).unwrap(), tampered);

        DockerToolchain::normalize_oci_archive(&archive_path, "amd64").unwrap();
        let image = Image::from_tarball(&archive_path).unwrap();
        assert_eq!(image.get_metadata().unwrap().cmd.unwrap(), vec!["/payload"]);
        let mut archive = tar::Archive::new(File::open(&archive_path).unwrap());
        let config_path = digest_to_blob_path(&original_id).unwrap();
        let mut contents = HashMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            contents.insert(name, bytes);
        }
        let config = &contents[&config_path];
        assert_eq!(format!("sha256:{:x}", Sha256::digest(&config)), original_id);
        let value: Value = serde_json::from_slice(&config).unwrap();
        assert_eq!(
            value["config"]["OnBuild"],
            json!(["RUN false", "COPY missing /missing"])
        );
        let index: Value = serde_json::from_slice(&contents["index.json"]).unwrap();
        let manifest_path =
            digest_to_blob_path(index["manifests"][0]["digest"].as_str().unwrap()).unwrap();
        let manifest: Value = serde_json::from_slice(&contents[&manifest_path]).unwrap();
        let layers = manifest["layers"].as_array().unwrap();
        assert_eq!(
            layers.len(),
            value["rootfs"]["diff_ids"].as_array().unwrap().len()
        );
        for (position, layer) in layers.iter().enumerate() {
            assert_eq!(
                layer["mediaType"],
                "application/vnd.oci.image.layer.v1.tar+gzip"
            );
            let blob = &contents[&digest_to_blob_path(layer["digest"].as_str().unwrap()).unwrap()];
            assert_eq!(layer["size"], blob.len());
            assert_eq!(
                layer["digest"],
                format!("sha256:{:x}", Sha256::digest(blob))
            );
            let mut decoded = Vec::new();
            flate2::read::GzDecoder::new(blob.as_slice())
                .read_to_end(&mut decoded)
                .unwrap();
            assert_eq!(
                value["rootfs"]["diff_ids"][position],
                format!("sha256:{:x}", Sha256::digest(&decoded))
            );
            assert_eq!(
                decoded,
                saved[saved_manifest[0]["Layers"][position].as_str().unwrap()]
            );
        }
        let load = Command::new("docker")
            .args(["load", "-i"])
            .arg(&archive_path)
            .output()
            .unwrap();
        assert!(
            load.status.success(),
            "{}",
            String::from_utf8_lossy(&load.stderr)
        );
        let cleanup = Command::new("docker")
            .args(["image", "rm", &tag])
            .output()
            .unwrap();
        assert!(cleanup.status.success());
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
    fn absolute_path_resolves_output_before_build_context_changes() {
        let relative = Path::new(".alien/build/aws/router/linux-arm64.oci.tar");
        let resolved = DockerToolchain::absolute_path(relative).unwrap();

        assert!(resolved.is_absolute());
        assert!(resolved.ends_with(relative));
    }

    #[test]
    fn build_logs_redact_enterprise_ca_secret() {
        let sentinel = "/private/sentinel-enterprise-ca.pem";
        let args = vec![
            "buildx".to_string(),
            "build".to_string(),
            "--secret".to_string(),
            format!("id=enterprise_ca,src={sentinel}"),
            ".".to_string(),
        ];

        let rendered = format!("{:?}", DockerToolchain::redacted_build_args(&args));
        assert!(
            !rendered.contains(sentinel),
            "secret path leaked: {rendered}"
        );
        assert!(rendered.contains("<redacted>"), "secret was not redacted");
    }

    #[test]
    fn enterprise_ca_uses_standard_precedence() {
        let temp_dir = tempdir().unwrap();
        let ssl = temp_dir.path().join("ssl.pem");
        let requests = temp_dir.path().join("requests.pem");
        let node = temp_dir.path().join("node.pem");
        for path in [&ssl, &requests, &node] {
            std::fs::write(path, "certificate").unwrap();
        }
        let values = HashMap::from([
            ("SSL_CERT_FILE", ssl.as_os_str().to_owned()),
            ("REQUESTS_CA_BUNDLE", requests.as_os_str().to_owned()),
            ("NODE_EXTRA_CA_CERTS", node.as_os_str().to_owned()),
        ]);

        let selected = DockerToolchain::enterprise_ca_path_from(|name| values.get(name).cloned())
            .expect("certificate discovery should succeed");

        assert_eq!(selected.as_deref(), Some(ssl.as_path()));
    }

    #[test]
    fn enterprise_ca_explicit_override_has_highest_precedence() {
        let temp_dir = tempdir().unwrap();
        let explicit = temp_dir.path().join("explicit.pem");
        let ssl = temp_dir.path().join("ssl.pem");
        std::fs::write(&explicit, "explicit certificate").unwrap();
        std::fs::write(&ssl, "standard certificate").unwrap();
        let values = HashMap::from([
            (
                "ALIEN_ENTERPRISE_CA_CERT_PATH",
                explicit.as_os_str().to_owned(),
            ),
            ("SSL_CERT_FILE", ssl.as_os_str().to_owned()),
        ]);

        let selected = DockerToolchain::enterprise_ca_path_from(|name| values.get(name).cloned())
            .expect("certificate discovery should succeed");

        assert_eq!(selected.as_deref(), Some(explicit.as_path()));
    }

    #[test]
    fn enterprise_ca_is_optional_but_invalid_configuration_fails() {
        let absent = DockerToolchain::enterprise_ca_path_from(|_| None)
            .expect("absent certificate variables should be accepted");
        assert_eq!(absent, None);

        let error = DockerToolchain::enterprise_ca_path_from(|name| {
            (name == "ALIEN_ENTERPRISE_CA_CERT_PATH")
                .then(|| OsString::from("/missing/certificate.pem"))
        })
        .expect_err("an advertised missing certificate must fail fast");
        let message = error.to_string();
        assert!(message.contains("ALIEN_ENTERPRISE_CA_CERT_PATH"));
        assert!(!message.contains("/missing/certificate.pem"));
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
            pull_base_images: false,
            workload: crate::toolchain::WorkloadKind::Container,
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

        // The same artifact is consumed remotely as OCI and locally by `docker load`.
        // Loading and running it proves the compatibility manifest references the real
        // config and layers, rather than merely checking for a filename in the archive.
        let load = Command::new("docker")
            .args(["load", "-i"])
            .arg(&tarball_path)
            .output()
            .expect("docker load should execute");
        assert!(
            load.status.success(),
            "docker load failed: {}",
            String::from_utf8_lossy(&load.stderr)
        );
        let loaded = String::from_utf8_lossy(&load.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("Loaded image ID: "))
            .expect("docker load should report the untagged image ID")
            .to_string();
        let run = Command::new("docker")
            .args(["run", "--rm", &loaded])
            .output()
            .expect("loaded image should run");
        assert!(
            run.status.success(),
            "loaded image failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout).trim(),
            "Hello from Docker"
        );
        let _ = Command::new("docker")
            .args(["image", "rm", &loaded])
            .output();
    }

    #[tokio::test]
    #[ignore = "needs Docker, BuildKit, and ALIEN_ENTERPRISE_CA_CERT_PATH"]
    async fn enterprise_ca_is_forwarded_as_ephemeral_buildkit_secret() {
        assert!(docker_available(), "Docker must be available");
        let cert_path = std::env::var(DockerToolchain::ENTERPRISE_CA_ENV_VARS[0])
            .expect("ALIEN_ENTERPRISE_CA_CERT_PATH must be configured");
        let cert = fs::read(&cert_path)
            .await
            .expect("configured enterprise CA must be readable");
        assert!(
            !cert.is_empty(),
            "configured enterprise CA must not be empty"
        );

        let source = tempdir().expect("source temp dir");
        let build = tempdir().expect("build temp dir");
        fs::write(
            source.path().join("Dockerfile"),
            r#"# syntax=docker/dockerfile:1
FROM alpine:3.20
RUN --mount=type=secret,id=enterprise_ca,required=true test -s /run/secrets/enterprise_ca
CMD ["true"]
"#,
        )
        .await
        .expect("write Dockerfile");

        let toolchain = DockerToolchain {
            dockerfile: None,
            build_args: None,
            target: None,
        };
        let context = ToolchainContext {
            src_dir: source.path().to_path_buf(),
            build_dir: build.path().to_path_buf(),
            cache_store: None,
            cache_prefix: "enterprise-ca-test".to_string(),
            build_target: BinaryTarget::linux_container_target(),
            runtime_platform_name: "local".to_string(),
            debug_mode: false,
            pull_base_images: false,
            workload: crate::toolchain::WorkloadKind::Container,
        };

        toolchain
            .build(&context)
            .await
            .expect("Docker toolchain should forward the configured BuildKit secret");

        let archive = fs::read(build.path().join(format!(
            "{}.oci.tar",
            context.build_target.runtime_platform_id()
        )))
        .await
        .expect("read built OCI archive");
        assert!(
            !archive.windows(cert.len()).any(|window| window == cert),
            "enterprise CA contents must not be persisted in the image archive"
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
        let emulated = "Nodes:\nName: default\nPlatforms: linux/arm64, linux/amd64, linux/amd64/v2, linux/arm/v7\n";
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

    #[test]
    fn selects_oci_capable_buildx_drivers_without_replacing_them() {
        let inspect = "Name:          team-builder\nDriver:        docker-container\n";
        assert_eq!(
            DockerToolchain::inspect_field(inspect, "Name:"),
            Some("team-builder")
        );
        assert_eq!(
            DockerToolchain::inspect_field(inspect, "Driver:"),
            Some("docker-container")
        );
        assert!(DockerToolchain::driver_supports_oci_export(
            "docker-container"
        ));
        assert!(DockerToolchain::driver_supports_oci_export("remote"));
        assert!(!DockerToolchain::driver_supports_oci_export("docker"));
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

    #[test]
    fn select_image_manifest_skips_referrers_and_reads_arch_from_the_config() {
        // `docker save` from a containerd-store daemon of an image pulled as a single
        // manifest: descriptors without a platform, plus the image's attestation referrer.
        let index = json!({
            "manifests": [
                { "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "sha256:image" },
                { "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "sha256:attest" }
            ]
        });
        let blobs = HashMap::from([
            (
                "sha256:image".to_string(),
                json!({ "config": { "digest": "sha256:config" }, "layers": [] }),
            ),
            (
                "sha256:config".to_string(),
                json!({ "architecture": "arm64", "os": "linux" }),
            ),
            (
                "sha256:attest".to_string(),
                json!({ "config": { "digest": "sha256:attest-config" }, "layers": [],
                        "subject": { "digest": "sha256:image" } }),
            ),
        ]);

        let chosen = select_image_manifest_descriptor(&index, &blobs, "arm64").unwrap();
        assert_eq!(chosen.get("digest").unwrap(), "sha256:image");
    }

    #[test]
    fn select_image_manifest_keeps_a_lone_image_without_a_recorded_arch() {
        let index = json!({
            "manifests": [
                { "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "sha256:image" }
            ]
        });
        let chosen = select_image_manifest_descriptor(&index, &HashMap::new(), "arm64").unwrap();
        assert_eq!(chosen.get("digest").unwrap(), "sha256:image");
    }

    fn sha256_digest(bytes: &[u8]) -> String {
        format!("sha256:{:x}", Sha256::digest(bytes))
    }

    /// Write what `docker save` emits for an image pulled as a single manifest from a
    /// containerd-store daemon: an index whose descriptors carry no platform, the image
    /// manifest, its config and layer, and the attestation referrer that came with it.
    /// Returns the image manifest digest.
    fn saved_single_manifest_archive(path: &Path, architecture: &str) -> String {
        let layer = vec![0_u8; 1024];
        let config = serde_json::to_vec(&json!({
            "architecture": architecture, "os": "linux",
            "config": {"Cmd": ["/app"]},
            "rootfs": {"type": "layers", "diff_ids": [sha256_digest(&layer)]},
        }))
        .unwrap();
        let manifest = serde_json::to_vec(&json!({
            "schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"mediaType": "application/vnd.oci.image.config.v1+json",
                       "digest": sha256_digest(&config), "size": config.len()},
            "layers": [{"mediaType": "application/vnd.oci.image.layer.v1.tar",
                        "digest": sha256_digest(&layer), "size": layer.len()}],
        }))
        .unwrap();
        let attestation = br#"{"predicateType":"https://spdx.dev/Document"}"#.to_vec();
        let referrer = serde_json::to_vec(&json!({
            "schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"mediaType": "application/vnd.oci.image.config.v1+json",
                       "digest": sha256_digest(&attestation), "size": attestation.len()},
            "layers": [{"mediaType": "application/vnd.in-toto+json",
                        "digest": sha256_digest(&attestation), "size": attestation.len()}],
            "subject": {"mediaType": "application/vnd.oci.image.manifest.v1+json",
                        "digest": sha256_digest(&manifest), "size": manifest.len()},
        }))
        .unwrap();
        let index = serde_json::to_vec(&json!({
            "schemaVersion": 2, "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [
                {"mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "digest": sha256_digest(&manifest), "size": manifest.len(),
                 "annotations": {"io.containerd.image.name": "registry.example.com/app:1"}},
                {"mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "digest": sha256_digest(&referrer), "size": referrer.len(),
                 "annotations": {"io.containerd.manifest.subject": sha256_digest(&manifest)}},
            ],
        }))
        .unwrap();
        let docker_manifest = serde_json::to_vec(&json!([{
            "Config": digest_to_blob_path(&sha256_digest(&config)).unwrap(),
            "RepoTags": ["registry.example.com/app:1"],
            "Layers": [digest_to_blob_path(&sha256_digest(&layer)).unwrap()],
        }]))
        .unwrap();

        let mut builder = tar::Builder::new(File::create(path).unwrap());
        let entries = [
            (
                "oci-layout".to_string(),
                br#"{"imageLayoutVersion":"1.0.0"}"#.to_vec(),
            ),
            ("index.json".to_string(), index),
            ("manifest.json".to_string(), docker_manifest),
        ]
        .into_iter()
        .chain(
            [manifest.clone(), config, layer, referrer, attestation]
                .into_iter()
                .map(|bytes| (digest_to_blob_path(&sha256_digest(&bytes)).unwrap(), bytes)),
        );
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, name, bytes.as_slice())
                .unwrap();
        }
        builder.finish().unwrap();
        sha256_digest(&manifest)
    }

    fn archive_index(path: &Path) -> Value {
        let mut archive = tar::Archive::new(File::open(path).unwrap());
        let mut entry = archive
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap())
            .find(|entry| entry.path().unwrap() == Path::new("index.json"))
            .expect("index.json");
        read_entry_json(&mut entry).unwrap()
    }

    #[test]
    fn saved_single_manifest_image_is_kept_when_its_config_matches_the_target() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.tar");
        let image_digest = saved_single_manifest_archive(&path, "arm64");

        DockerToolchain::normalize_oci_archive(&path, "arm64").unwrap();

        let manifests = archive_index(&path)["manifests"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(manifests.len(), 1, "{manifests:?}");
        assert_eq!(manifests[0]["digest"], image_digest);
    }

    #[test]
    fn saved_single_manifest_image_for_another_arch_is_refused() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("image.tar");
        saved_single_manifest_archive(&path, "amd64");
        let original = std::fs::read(&path).unwrap();

        let error = DockerToolchain::normalize_oci_archive(&path, "arm64").unwrap_err();

        assert!(
            error.message.contains(
                "built for amd64 only, but this release needs arm64; publish a multi-architecture image that includes linux/arm64, or set `architecture: \"x86_64\"` in the requirements of the pool that runs it"
            ),
            "{}",
            error.message
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            original,
            "archive was rewritten"
        );
    }

    #[test]
    fn docker_manifest_reuses_selected_oci_config_and_layers() {
        let descriptor = json!({ "digest": "sha256:image" });
        let mut blobs = HashMap::new();
        blobs.insert(
            "sha256:image".to_string(),
            json!({
                "config": { "digest": "sha256:config" },
                "layers": [
                    { "digest": "sha256:first" },
                    { "digest": "sha256:second" }
                ]
            }),
        );

        let manifest = docker_manifest_for_descriptor(&descriptor, &blobs).unwrap();
        assert_eq!(manifest[0]["Config"], "blobs/sha256/config");
        assert_eq!(
            manifest[0]["Layers"],
            json!(["blobs/sha256/first", "blobs/sha256/second"])
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
            runtime_platform_name: "aws".to_string(),
            debug_mode: false,
            pull_base_images: false,
            workload: crate::toolchain::WorkloadKind::Container,
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
            pull_base_images: false,
            workload: crate::toolchain::WorkloadKind::Container,
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
            pull_base_images: false,
            workload: crate::toolchain::WorkloadKind::Container,
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
