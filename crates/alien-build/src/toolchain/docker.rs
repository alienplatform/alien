use super::{Toolchain, ToolchainContext, ToolchainOutput};
use crate::command_output::wait_with_captured_output;
use crate::error::{ErrorData, Result};
use crate::settings::BinaryTargetExt;
use alien_core::AlienEvent;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env::VarError;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, warn};

const ENTERPRISE_CA_CERT_PATH_ENV: &str = "ALIEN_ENTERPRISE_CA_CERT_PATH";

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
    owned: bool,
}

impl DockerToolchain {
    /// Check if the source directory contains a Dockerfile
    pub fn has_dockerfile(src_dir: &Path, dockerfile: Option<&String>) -> bool {
        let dockerfile_name = dockerfile.map(|s| s.as_str()).unwrap_or("Dockerfile");
        src_dir.join(dockerfile_name).exists()
    }

    #[cfg(test)]
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

    fn generate_builder_name() -> String {
        use rand::distr::Alphanumeric;
        use rand::Rng;

        let suffix: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(8)
            .map(char::from)
            .collect::<String>()
            .to_lowercase();
        format!("alien-build-{}-{suffix}", std::process::id())
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

    fn enterprise_ca_secret() -> Result<Option<String>> {
        let path = match std::env::var(ENTERPRISE_CA_CERT_PATH_ENV) {
            Ok(path) => path,
            Err(VarError::NotPresent) => return Ok(None),
            Err(VarError::NotUnicode(_)) => {
                return Err(AlienError::new(Self::docker_build_error(format!(
                    "{ENTERPRISE_CA_CERT_PATH_ENV} must contain a valid UTF-8 file path"
                ))));
            }
        };
        if path.is_empty() {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "{ENTERPRISE_CA_CERT_PATH_ENV} must not be empty"
            ))));
        }

        let file = File::open(&path)
            .into_alien_error()
            .context(Self::docker_build_error(format!(
                "{ENTERPRISE_CA_CERT_PATH_ENV} must name a readable file"
            )))?;
        let metadata = file
            .metadata()
            .into_alien_error()
            .context(Self::docker_build_error(format!(
                "Could not inspect the file named by {ENTERPRISE_CA_CERT_PATH_ENV}"
            )))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "{ENTERPRISE_CA_CERT_PATH_ENV} must name a non-empty regular file"
            ))));
        }

        Ok(Some(format!("id=enterprise_ca,src={path}")))
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

    async fn create_builder(builder_name: &str) -> Result<()> {
        let output = Command::new("docker")
            .args([
                "buildx",
                "create",
                "--name",
                builder_name,
                "--driver",
                "docker-container",
            ])
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
                owned: false,
            });
        }
        if driver != "docker" {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "The current buildx driver '{driver}' does not support OCI archive export"
            ))));
        }

        let name = Self::generate_builder_name();
        if let Err(error) = Self::create_builder(&name).await {
            let _ = Self::remove_builder(&name).await;
            return Err(error);
        }
        Ok(BuildxBuilder { name, owned: true })
    }

    async fn remove_builder(builder_name: &str) -> Result<()> {
        let output = Command::new("docker")
            .args(["buildx", "rm", "--force", builder_name])
            .output()
            .await
            .into_alien_error()
            .context(Self::docker_build_error(
                "Could not remove the scoped buildx builder",
            ))?;
        if !output.status.success() {
            return Err(AlienError::new(Self::docker_build_error(format!(
                "Could not remove the scoped buildx builder: {}",
                String::from_utf8_lossy(&output.stderr).trim()
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
        let output_tarball = context.build_dir.join(format!(
            "{}.oci.tar",
            context.build_target.runtime_platform_id()
        ));
        let output = format!("type=oci,dest={}", output_tarball.display());
        let arch_str = match context.build_target.to_dockdash_arch() {
            dockdash::Arch::Amd64 => "amd64",
            dockdash::Arch::ARM64 => "arm64",
            _ => "amd64", // Fallback for other architectures
        };
        let platform_str = format!("linux/{}", arch_str);

        let builder = Self::select_builder().await?;
        if let Err(error) = Self::ensure_builder_supports_arch(&builder.name, arch_str).await {
            if builder.owned {
                if let Err(cleanup_error) = Self::remove_builder(&builder.name).await {
                    warn!(%cleanup_error, builder_name = %builder.name, "failed to clean up buildx builder");
                }
            }
            return Err(error);
        }

        let mut args = vec![
            "buildx".to_string(),
            "build".to_string(),
            "--builder".to_string(),
            builder.name.clone(),
            "--platform".to_string(),
            platform_str,
            "--output".to_string(),
            output,
            "-f".to_string(),
            dockerfile_name.to_string(),
        ];

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

        if let Some(secret) = Self::enterprise_ca_secret()? {
            args.push("--secret".to_string());
            args.push(secret);
        }

        // Add target if specified
        if let Some(target) = &self.target {
            args.push("--target".to_string());
            args.push(target.clone());
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

        let cleanup_result = if builder.owned {
            Self::remove_builder(&builder.name).await
        } else {
            Ok(())
        };
        if let Err(build_error) = build_result {
            if let Err(cleanup_error) = cleanup_result {
                warn!(%cleanup_error, builder_name = %builder.name, "failed to clean up buildx builder after a failed build");
            }
            return Err(build_error);
        }
        cleanup_result?;

        // Buildx may wrap the image manifest in an index alongside provenance attestations.
        // Flatten the archive before the runtime OCI reader sees it.
        Self::normalize_oci_archive(&output_tarball, arch_str)?;

        // Extract CMD from the built image for the runtime_command field
        let runtime_command = Self::extract_cmd_from_tarball(&output_tarball)?;

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
        let mut has_docker_manifest = false;

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
                has_docker_manifest = true;
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
        let new_index = if index_points_only_at(&index, &chosen) {
            None
        } else {
            Some(json!({
                "schemaVersion": 2,
                "mediaType": "application/vnd.oci.image.index.v1+json",
                "manifests": [chosen.clone()],
            }))
        };
        let docker_manifest = if has_docker_manifest {
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
        let cert_path = std::env::var(ENTERPRISE_CA_CERT_PATH_ENV)
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
