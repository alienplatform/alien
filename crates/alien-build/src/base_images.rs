use crate::cache::{compute_function_content_hash, finalize_artifact_dir, temp_artifact_dir};
use crate::command_output::image_build_error_with_output;
use crate::error::{ErrorData, Result};
use crate::settings::{BinaryTargetExt, BuildSettings};
use crate::toolchain;
use alien_core::{alien_event, AlienEvent};
use alien_error::{Context, IntoAlienError};
use std::error::Error as StdError;
use std::path::Path;
use std::time::Duration;
use tokio::fs;
use tracing::info;

pub(crate) const BASE_IMAGE_BUILD_MAX_ATTEMPTS: usize = 3;

/// Return the ordered base-image inputs that affect a source artifact.
/// Host-process and Dockerfile builds do not use the source toolchain bases.
pub(crate) fn effective_source_base_images(
    toolchain_config: &alien_core::ToolchainConfig,
    settings: &BuildSettings,
    workload: toolchain::WorkloadKind,
    host_process: bool,
) -> Vec<String> {
    if host_process || matches!(toolchain_config, alien_core::ToolchainConfig::Docker { .. }) {
        return vec![];
    }

    let defaults = match workload {
        toolchain::WorkloadKind::Worker => toolchain::WORKER_BASE_IMAGES,
        toolchain::WorkloadKind::Container | toolchain::WorkloadKind::Daemon => {
            toolchain::DIRECT_BASE_IMAGES
        }
    }
    .iter()
    .map(|image| (*image).to_string())
    .collect::<Vec<_>>();

    base_images_for_workload(&defaults, settings.override_base_image.as_deref(), workload)
}

/// Apply a feature-versioned runtime base only to Worker source images.
pub(crate) fn base_images_for_workload(
    base_images: &[String],
    override_base_image: Option<&str>,
    workload: toolchain::WorkloadKind,
) -> Vec<String> {
    if workload == toolchain::WorkloadKind::Worker {
        if let Some(override_image) = override_base_image {
            return vec![override_image.to_string()];
        }
    }

    base_images.to_vec()
}

/// Decide the ENTRYPOINT/CMD pair an image gets from a [`ToolchainOutput`].
///
/// - `entrypoint: Some` — direct-entrypoint images (source-built
///   Containers/Daemons): the compiled binary overrides the plain base
///   image's entrypoint and clears inherited CMD. CMD is set only when
///   `runtime_command` is nonempty — direct images carry no runtime wrapper
///   and no `--` separator.
/// - `entrypoint: None` — keep the base image's entrypoint (e.g. alien-base's
///   `alien-worker-runtime`) and always set CMD from `runtime_command`.
///
/// The resulting image shapes are pinned by `tests/image_shape_tests.rs`.
pub(crate) fn image_entrypoint_and_cmd(
    output: &toolchain::ToolchainOutput,
) -> (Option<Vec<String>>, Option<Vec<String>>) {
    match &output.entrypoint {
        Some(entrypoint) => {
            let cmd = if output.runtime_command.is_empty() {
                None
            } else {
                Some(output.runtime_command.clone())
            };
            (Some(entrypoint.clone()), cmd)
        }
        None => (None, Some(output.runtime_command.clone())),
    }
}

/// Apply the [`image_entrypoint_and_cmd`] contract to a dockdash image
/// builder. Used by both the base-image and from-scratch build paths.
pub(crate) fn apply_image_command(
    mut builder: dockdash::ImageBuilder,
    output: &toolchain::ToolchainOutput,
) -> dockdash::ImageBuilder {
    let (entrypoint, cmd) = image_entrypoint_and_cmd(output);
    if let Some(entrypoint) = entrypoint {
        builder = builder.entrypoint(entrypoint);
    }
    if let Some(cmd) = cmd {
        builder = builder.cmd(cmd);
    }
    builder
}

pub(crate) fn base_image_build_retry_delay(attempt: usize) -> Duration {
    match attempt {
        1 => Duration::from_secs(2),
        2 => Duration::from_secs(5),
        _ => Duration::from_secs(10),
    }
}

pub(crate) fn is_retryable_dockdash_image_pull_error(error: &dockdash::Error) -> bool {
    match error {
        dockdash::Error::ImagePull { source, .. } => {
            source
                .as_deref()
                .map(is_retryable_image_pull_source)
                .unwrap_or(false)
                || is_retryable_image_pull_text(&error.to_string())
        }
        _ => false,
    }
}

fn is_retryable_image_pull_text(message: &str) -> bool {
    const RETRYABLE_MARKERS: &[&str] = &[
        "error sending request",
        "client error (sendrequest)",
        "connection error",
        "connection aborted",
        "connection reset",
        "connection refused",
        "connection closed",
        "timed out",
        "unexpected eof",
        "broken pipe",
        "temporary failure in name resolution",
        "dns error",
    ];

    let message = message.to_ascii_lowercase();
    RETRYABLE_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
}

fn is_retryable_image_pull_source(source: &(dyn StdError + Send + Sync + 'static)) -> bool {
    let mut current = Some(source as &(dyn StdError + 'static));

    while let Some(error) = current {
        if let Some(oci_error) = error.downcast_ref::<oci_client::errors::OciDistributionError>() {
            return is_retryable_oci_error(oci_error);
        }

        if let Some(reqwest_error) = error.downcast_ref::<reqwest::Error>() {
            return reqwest_error.is_timeout()
                || reqwest_error.is_connect()
                || reqwest_error
                    .status()
                    .map(|status| status.is_server_error() || status.as_u16() == 429)
                    .unwrap_or(false);
        }

        if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
            return matches!(
                io_error.kind(),
                std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::WouldBlock
            );
        }

        current = error.source();
    }

    false
}

fn is_retryable_oci_error(error: &oci_client::errors::OciDistributionError) -> bool {
    match error {
        oci_client::errors::OciDistributionError::ServerError { code, .. } => {
            *code >= 500 || *code == 429
        }
        oci_client::errors::OciDistributionError::RequestError(error) => {
            error.is_timeout()
                || error.is_connect()
                || error
                    .status()
                    .map(|status| status.is_server_error() || status.as_u16() == 429)
                    .unwrap_or(false)
        }
        oci_client::errors::OciDistributionError::IoError(error) => matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::WouldBlock
        ),
        _ => false,
    }
}

/// Pull a Docker image and export it to OCI tarballs for each target architecture.
/// This handles both registry images (nginx:latest) and local images (my-app:v1).
#[alien_event(AlienEvent::BuildingResource {
    resource_name: container_name.to_string(),
    resource_type: "container".to_string(),
    related_resources: vec![],
})]
pub(crate) async fn pull_and_export_image(
    image: &str,
    container_name: &str,
    _stack_id: &str,
    settings: &BuildSettings,
    build_output_dir: &Path,
) -> Result<String> {
    info!(
        "Pulling and exporting image '{}' for container '{}'",
        image, container_name
    );

    let targets = settings.get_targets();

    info!(
        "Exporting image '{}' for {} target(s): {:?}",
        image,
        targets.len(),
        targets
    );

    let container_dir = temp_artifact_dir(build_output_dir, container_name);
    fs::create_dir_all(&container_dir)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: container_dir.display().to_string(),
            reason: "Failed to create container directory for export".to_string(),
        })?;

    // Pull the image for each target architecture
    use std::process::Stdio;
    use tokio::process::Command;

    for target in targets {
        let arch_str = match target.to_dockdash_arch() {
            dockdash::Arch::Amd64 => "amd64",
            dockdash::Arch::ARM64 => "arm64",
            _ => "amd64", // Fallback for other architectures
        };
        let platform_str = format!("linux/{}", arch_str);

        info!("Pulling image '{}' for platform {}", image, platform_str);

        // Pull the image with specific platform
        let pull_output = Command::new("docker")
            .args(&["pull", "--platform", &platform_str, image])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: container_name.to_string(),
                reason: "Failed to execute docker pull".to_string(),
                build_output: None,
            })?;

        if !pull_output.status.success() {
            return Err(image_build_error_with_output(
                container_name,
                format!("docker pull failed for image '{}'", image),
                &pull_output,
            ));
        }

        info!("Successfully pulled image '{}' for {}", image, platform_str);

        // Export to OCI tarball
        let target_filename = format!("{}.oci.tar", target.runtime_platform_id());
        let output_tarball = container_dir.join(&target_filename);

        info!("Exporting to OCI tarball: {}", output_tarball.display());

        let save_output = Command::new("docker")
            .args(&["save", "-o", &output_tarball.to_string_lossy(), image])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .into_alien_error()
            .context(ErrorData::ImageBuildFailed {
                resource_name: container_name.to_string(),
                reason: "Failed to execute docker save".to_string(),
                build_output: None,
            })?;

        if !save_output.status.success() {
            return Err(image_build_error_with_output(
                container_name,
                "docker save failed",
                &save_output,
            ));
        }

        // Flatten the saved archive to a single image manifest before the OCI reader sees it.
        crate::toolchain::docker::DockerToolchain::normalize_oci_archive(
            &output_tarball,
            arch_str,
        )?;

        info!(
            "Successfully exported {} to {}",
            image,
            output_tarball.display()
        );
    }

    // Compute content hash
    let content_hash = compute_function_content_hash(&container_dir).await?;
    let short_hash = &content_hash[..8];

    // Rename directory to include content hash
    let hashed_dir_name = format!("{}-{}", container_name, short_hash);
    let final_output_dir = build_output_dir.join(&hashed_dir_name);

    let finalized_dir = finalize_artifact_dir(&container_dir, &final_output_dir, "export").await?;

    info!(
        "Completed export for container '{}'. Images directory: {} (hash: {})",
        container_name,
        final_output_dir.display(),
        short_hash
    );

    Ok(finalized_dir)
}
