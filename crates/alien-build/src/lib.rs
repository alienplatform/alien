mod base_images;
mod cache;
pub(crate) mod command_output;
pub mod dependencies;
pub mod error;
pub mod merge;
pub mod plan;
mod push;
pub mod settings;
mod stack;
pub mod toolchain;

#[cfg(test)]
mod tests;

pub use push::push_stack;
pub use stack::build_stack;
#[cfg(test)]
use stack::requested_host_binary_only;

use alien_core::{
    alien_event, AlienEvent, BinaryTarget, Container, ContainerCode, Daemon, DaemonCode, Platform,
    Stack, ToolchainConfig, WorkerCode,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_preflights::runner::PreflightRunner;
use base_images::{
    apply_image_command, base_image_build_retry_delay, base_images_for_workload,
    is_retryable_dockdash_image_pull_error, pull_and_export_image, BASE_IMAGE_BUILD_MAX_ATTEMPTS,
};
use cache::{
    compute_function_content_hash, compute_source_artifact_cache_key, finalize_artifact_dir,
    find_cached_artifact_dir, temp_artifact_dir, write_artifact_cache_metadata,
};
use dockdash::{Image as DockDashImage, Layer as DockDashLayer, PullPolicy};
use error::{DockdashResultExt, ErrorData, Result};
use push::generate_unique_tag;
use reqwest::Url;
use settings::{BinaryTargetExt, BuildSettings, PlatformBuildSettings};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::fs;
use tokio::time::sleep;

use tracing::{info, warn};

fn strip_local_daemon_only_compute_clusters(stack: &mut Stack, platform: Platform) {
    if platform != Platform::Local {
        return;
    }

    let daemon_clusters: HashSet<String> = stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<Daemon>())
        .filter_map(|daemon| daemon.cluster.clone())
        .collect();

    if daemon_clusters.is_empty() {
        return;
    }

    let container_clusters: HashSet<String> = stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<Container>())
        .filter_map(|container| container.cluster.clone())
        .collect();

    let removed_clusters: HashSet<String> = stack
        .resources()
        .filter(|(id, entry)| {
            entry.config.resource_type().as_ref() == "compute-cluster"
                && daemon_clusters.contains(*id)
                && !container_clusters.contains(*id)
        })
        .map(|(id, _)| id.clone())
        .collect();

    if removed_clusters.is_empty() {
        return;
    }

    stack
        .resources
        .retain(|id, _| !removed_clusters.contains(id));

    for (_, entry) in stack.resources_mut() {
        let Some(daemon) = entry.config.downcast_mut::<Daemon>() else {
            continue;
        };
        if daemon
            .cluster
            .as_ref()
            .is_some_and(|cluster| removed_clusters.contains(cluster))
        {
            daemon.cluster = None;
        }
    }
}

/// Build a resource (worker, container, or dependency) for one or more OS/architecture targets
///
/// Always saves OCI tarballs to a consistent directory structure:
/// `build_output_dir/resource_name/{target}.oci.tar`
#[alien_event(AlienEvent::BuildingResource {
    resource_name: resource_name.to_string(),
    resource_type: workload.as_str().to_string(),
    related_resources: related_resources.to_vec(),
})]
async fn build_resource(
    src: &str,
    toolchain_config: &alien_core::ToolchainConfig,
    resource_name: &str,
    stack_id: &str,
    settings: &BuildSettings,
    build_output_dir: &Path,
    workload: toolchain::WorkloadKind,
    related_resources: &[String],
) -> Result<String> {
    let resource_started = Instant::now();
    // Get target list from settings (uses platform defaults if not specified)
    let targets = settings.get_targets();

    info!(
        "Building resource '{}' for {} target(s): {:?}",
        resource_name,
        targets.len(),
        targets
    );

    // Validate the source directory before the artifact-cache hash walks it,
    // so a missing or invalid project fails with a clear config error instead
    // of an I/O error from the hasher.
    if !Path::new(src).is_dir() {
        return Err(AlienError::new(ErrorData::InvalidResourceConfig {
            resource_id: resource_name.to_string(),
            reason: format!("Source directory '{}' not found", src),
        }));
    }
    toolchain::create_toolchain(toolchain_config).validate_source(Path::new(src), resource_name)?;

    let cache_key_started = Instant::now();
    let artifact_cache_key =
        compute_source_artifact_cache_key(src, toolchain_config, settings, &targets, workload)
            .await?;
    let cache_key_secs = cache_key_started.elapsed().as_secs_f64();

    let platform_name = settings.platform.runtime_platform().as_str();
    let lookup_started = Instant::now();
    let cached_dir = find_cached_artifact_dir(
        build_output_dir,
        resource_name,
        &targets,
        &artifact_cache_key,
    )
    .await?;
    let lookup_secs = lookup_started.elapsed().as_secs_f64();

    if let Some(cached_dir) = cached_dir {
        info!(
            resource = resource_name,
            platform = platform_name,
            artifact_cache = "HIT",
            cache_key = &artifact_cache_key[..12],
            key_secs = format!("{cache_key_secs:.2}").as_str(),
            lookup_secs = format!("{lookup_secs:.2}").as_str(),
            "Artifact cache HIT for resource '{}' on platform '{}': reusing {} (skipping {} target build(s))",
            resource_name,
            platform_name,
            cached_dir.display(),
            targets.len()
        );
        return Ok(cached_dir.to_string_lossy().into_owned());
    }

    info!(
        resource = resource_name,
        platform = platform_name,
        artifact_cache = "MISS",
        cache_key = &artifact_cache_key[..12],
        key_secs = format!("{cache_key_secs:.2}").as_str(),
        lookup_secs = format!("{lookup_secs:.2}").as_str(),
        "Artifact cache MISS for resource '{}' on platform '{}': building {} target(s): {:?}",
        resource_name,
        platform_name,
        targets.len(),
        targets
    );

    // Build into a unique staging directory so concurrent builds do not race on
    // the same path before the hashed output is finalized.
    let resource_dir = temp_artifact_dir(build_output_dir, resource_name);
    fs::create_dir_all(&resource_dir)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: resource_dir.display().to_string(),
            reason: "Failed to create function directory for build".to_string(),
        })?;

    // Build for each target in parallel
    // Spawn tasks for each target to build concurrently
    let build_tasks: Vec<_> = targets
        .iter()
        .map(|target| {
            let src = src.to_string();
            let toolchain_config = toolchain_config.clone();
            let resource_name = resource_name.to_string();
            let stack_id = stack_id.to_string();
            let settings = settings.clone();
            let target = *target;
            let resource_dir = resource_dir.clone();

            tokio::spawn(async move {
                info!("Building for target: {:?}", target);
                let target_started = Instant::now();

                // Create target-specific output path
                // Always use target ID in filename for consistency
                let target_filename = format!("{}.oci.tar", target.runtime_platform_id());
                let target_output_path = resource_dir.join(&target_filename);

                // Build with toolchain for this specific target
                let result = build_target_to_file(
                    &src,
                    &toolchain_config,
                    &resource_name,
                    &stack_id,
                    &settings,
                    &target,
                    &target_output_path,
                    workload,
                )
                .await?;

                info!(
                    resource = resource_name.as_str(),
                    platform = settings.platform.runtime_platform().as_str(),
                    target = target.runtime_platform_id(),
                    target_secs = format!("{:.2}", target_started.elapsed().as_secs_f64()).as_str(),
                    "Successfully built target {} for resource '{}' in {:.2}s at: {}",
                    target.runtime_platform_id(),
                    resource_name,
                    target_started.elapsed().as_secs_f64(),
                    target_output_path.display()
                );

                Ok((target, result))
            })
        })
        .collect();

    // Wait for all builds to complete and collect results
    let mut build_results = Vec::new();
    for task in build_tasks {
        let result = task.await.map_err(|e| {
            AlienError::new(ErrorData::ImageBuildFailed {
                resource_name: resource_name.to_string(),
                reason: format!("Build task panicked or was cancelled: {}", e),
                build_output: None,
            })
        })??;
        build_results.push(result);
    }

    // Compute content hash of all built tarballs
    // This ensures the executor detects code changes between builds
    let content_hash = compute_function_content_hash(&resource_dir).await?;
    let short_hash = &content_hash[..8];

    // Rename directory to include content hash
    let hashed_dir_name = format!("{}-{}", resource_name, short_hash);
    let final_output_dir = build_output_dir.join(&hashed_dir_name);

    let finalized_dir = finalize_artifact_dir(&resource_dir, &final_output_dir, "build").await?;
    write_artifact_cache_metadata(&PathBuf::from(&finalized_dir), &artifact_cache_key).await?;

    // Return the directory path containing all OCI tarballs (with content hash)
    info!(
        "Completed build for resource '{}' in {:.2}s. Images directory: {} (hash: {})",
        resource_name,
        resource_started.elapsed().as_secs_f64(),
        final_output_dir.display(),
        short_hash
    );
    Ok(finalized_dir)
}

fn paths_resolve_to_same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

async fn materialize_complete_oci_tarball(tarball_path: &Path, output_path: &Path) -> Result<bool> {
    if paths_resolve_to_same_file(tarball_path, output_path) {
        return Ok(false);
    }

    fs::copy(tarball_path, output_path)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "copy file".to_string(),
            file_path: tarball_path.display().to_string(),
            reason: format!("Failed to copy OCI tarball to {}", output_path.display()),
        })?;

    Ok(true)
}

/// Build a specific OS/architecture target to an OCI tarball file
#[allow(clippy::too_many_arguments)]
async fn build_target_to_file(
    src: &str,
    toolchain_config: &alien_core::ToolchainConfig,
    resource_name: &str,
    stack_id: &str,
    settings: &BuildSettings,
    target: &BinaryTarget,
    output_path: &Path,
    workload: toolchain::WorkloadKind,
) -> Result<String> {
    info!(
        "Starting toolchain build for resource: {} (target: {})",
        resource_name,
        target.runtime_platform_id()
    );

    // Parse cache store from cache URL if provided
    let cache_store = if let Some(cache_url) = &settings.cache_url {
        let url =
            Url::parse(cache_url)
                .into_alien_error()
                .context(ErrorData::BuildConfigInvalid {
                    message: format!("Invalid cache URL: {}", cache_url),
                })?;

        let (store, _path) = object_store::parse_url(&url).into_alien_error().context(
            ErrorData::BuildConfigInvalid {
                message: format!("Failed to parse cache URL: {}", cache_url),
            },
        )?;

        Some(std::sync::Arc::from(store))
    } else {
        None
    };

    // The build directory is the parent of output_path (e.g., .alien/build/local/function-name/)
    // This is where intermediate artifacts (bootstrap files, compiled binaries) should go.
    // Using this directory keeps the user's source directory clean.
    let build_dir = output_path.parent().unwrap_or(output_path).to_path_buf();

    // Create toolchain context
    let toolchain_context = toolchain::ToolchainContext {
        src_dir: PathBuf::from(src),
        build_dir,
        cache_store,
        cache_prefix: format!(
            "{}-{}-{}",
            stack_id,
            resource_name,
            target.runtime_platform_id()
        ),
        build_target: *target,
        runtime_platform_name: settings.platform.runtime_platform().as_str().to_string(),
        debug_mode: settings.debug_mode,
        workload,
    };

    // Create and run toolchain
    let toolchain = toolchain::create_toolchain(toolchain_config);
    let toolchain_output = toolchain.build(&toolchain_context).await?;

    // Build image with dockdash
    let image_tag = generate_unique_tag();
    let image_name_for_build = format!(
        "{}:{}{}",
        resource_name,
        target.runtime_platform_id(),
        image_tag
    );

    // Build image based on strategy
    match &toolchain_output.build_strategy {
        toolchain::ImageBuildStrategy::CompleteOCITarball { tarball_path } => {
            // Toolchain produced a complete OCI tarball (e.g., Docker toolchain)
            // Use it as-is without re-packaging
            if materialize_complete_oci_tarball(tarball_path, output_path).await? {
                info!("Copied complete OCI tarball to {}", output_path.display());
            }
        }

        toolchain::ImageBuildStrategy::FromBaseImage {
            base_images,
            files_to_package,
        } => {
            // Cloud platform flow - build from base image
            // The override is specifically the Worker runtime base. Direct
            // Container/Daemon images must remain on their plain bases even
            // when a mixed stack builds Workers from a feature-versioned
            // alien-base image.
            let base_images_to_try = base_images_for_workload(
                base_images,
                settings.override_base_image.as_deref(),
                workload,
            );

            if base_images_to_try.is_empty() {
                return Err(AlienError::new(ErrorData::BuildConfigInvalid {
                    message: "No base images available to build from".to_string(),
                }));
            }

            let mut last_error_msg: Option<String> = None;
            let mut built_image_result: Option<(DockDashImage, _)> = None;

            for (index, base_image) in base_images_to_try.iter().enumerate() {
                info!(
                    "Attempting to build with base image ({}/{}): {}",
                    index + 1,
                    base_images_to_try.len(),
                    base_image
                );

                let mut build_result = Err(dockdash::Error::Generic {
                    message: "base image build was not attempted".to_string(),
                    source: None,
                });

                for attempt in 1..=BASE_IMAGE_BUILD_MAX_ATTEMPTS {
                    // Rebuild the lightweight application layer for each retry because
                    // dockdash layers are consumed by the image builder.
                    let mut app_layer_builder = DockDashLayer::builder().map_dockdash_err()?;

                    for file_spec in files_to_package {
                        let absolute_container_path = if file_spec.container_path.starts_with("/") {
                            file_spec.container_path.clone()
                        } else if file_spec.container_path.starts_with("./") {
                            format!("/app/{}", &file_spec.container_path[2..])
                        } else {
                            format!("/app/{}", file_spec.container_path)
                        };

                        if file_spec.host_path.is_dir() {
                            app_layer_builder = app_layer_builder
                                .directory(&file_spec.host_path, &absolute_container_path)
                                .map_dockdash_err()?;
                        } else if file_spec.host_path.is_file() {
                            app_layer_builder = app_layer_builder
                                .file(
                                    &file_spec.host_path,
                                    &absolute_container_path,
                                    file_spec.mode,
                                )
                                .map_dockdash_err()?;
                        }
                    }

                    let app_layer = app_layer_builder.build().await.map_dockdash_err()?;

                    let mut image_builder = DockDashImage::builder()
                        .from(base_image)
                        .platform(target.oci_os(), &target.to_dockdash_arch())
                        .pull_policy(PullPolicy::Always)
                        .layer(app_layer);

                    // Built-in Worker and direct bases are public. Do not let
                    // unrelated DOCKER_USERNAME/PASSWORD values get sent to
                    // GHCR, cgr.dev, or Docker Hub. A feature-versioned Worker
                    // override may be private, so it keeps dockdash's explicit
                    // environment-auth path.
                    if workload != toolchain::WorkloadKind::Worker
                        || settings.override_base_image.is_none()
                    {
                        image_builder = image_builder.auth(dockdash::RegistryAuth::Anonymous);
                    }

                    image_builder = apply_image_command(image_builder, &toolchain_output);

                    build_result = image_builder
                        .output_to(output_path.to_path_buf())
                        .output_name_and_tag(&image_name_for_build)
                        .build()
                        .await;

                    match &build_result {
                        Ok(_) => break,
                        Err(error)
                            if attempt < BASE_IMAGE_BUILD_MAX_ATTEMPTS
                                && is_retryable_dockdash_image_pull_error(error) =>
                        {
                            let delay = base_image_build_retry_delay(attempt);
                            warn!(
                                base_image,
                                attempt,
                                max_attempts = BASE_IMAGE_BUILD_MAX_ATTEMPTS,
                                delay_secs = delay.as_secs(),
                                error = %error,
                                "Transient base image pull/build failure, retrying"
                            );
                            sleep(delay).await;
                        }
                        Err(_) => break,
                    }
                }

                match build_result {
                    Ok(result) => {
                        info!("Successfully built image with base image: {}", base_image);
                        built_image_result = Some(result);
                        break;
                    }
                    Err(e) => {
                        if e.is_manifest_not_found() {
                            tracing::warn!(
                                "Base image '{}' not found in registry, trying next fallback if available",
                                base_image
                            );
                            last_error_msg = Some(e.to_string());
                        } else {
                            warn!("Failed to build with base image '{}': {}.", base_image, e);
                            return Err(e).map_dockdash_err();
                        }
                    }
                }
            }

            // Check if we successfully built the image
            let (_built_image, _diagnostics) = match built_image_result {
                Some(result) => result,
                None => {
                    let error_message = if let Some(last_err) = last_error_msg {
                        format!(
                            "All base images failed. Tried: {:?}. Last error: {}",
                            base_images_to_try, last_err
                        )
                    } else {
                        format!("All base images failed. Tried: {:?}", base_images_to_try)
                    };

                    return Err(AlienError::new(ErrorData::ImageBuildFailed {
                        resource_name: resource_name.to_string(),
                        reason: error_message,
                        build_output: None,
                    }));
                }
            };
        }

        toolchain::ImageBuildStrategy::FromScratch { layers } => {
            // Local platform flow - build from scratch
            info!("Building from scratch for local platform");

            let mut all_layers = Vec::new();

            // Add toolchain-specified layers (runtime binary, app code, etc.)
            for layer_spec in layers {
                info!("Adding layer: {}", layer_spec.description);
                let mut layer_builder = DockDashLayer::builder().map_dockdash_err()?;

                for file_spec in &layer_spec.files {
                    let absolute_container_path = if file_spec.container_path.starts_with("/") {
                        file_spec.container_path.clone()
                    } else if file_spec.container_path.starts_with("./") {
                        format!("/app/{}", &file_spec.container_path[2..])
                    } else {
                        format!("/app/{}", file_spec.container_path)
                    };

                    if file_spec.host_path.is_dir() {
                        layer_builder = layer_builder
                            .directory(&file_spec.host_path, &absolute_container_path)
                            .map_dockdash_err()?;
                    } else if file_spec.host_path.is_file() {
                        layer_builder = layer_builder
                            .file(
                                &file_spec.host_path,
                                &absolute_container_path,
                                file_spec.mode,
                            )
                            .map_dockdash_err()?;
                    }
                }

                let layer = layer_builder.build().await.map_dockdash_err()?;
                all_layers.push(layer);
            }

            // Build from scratch (no base image - don't call .from() at all)
            let mut image_builder = DockDashImage::builder()
                .platform(target.oci_os(), &target.to_dockdash_arch())
                .working_dir("/app"); // Set working directory so ./app resolves correctly
            image_builder = apply_image_command(image_builder, &toolchain_output);

            for layer in all_layers {
                image_builder = image_builder.layer(layer);
            }

            let (_built_image, _diagnostics) = image_builder
                .output_to(output_path.to_path_buf())
                .output_name_and_tag(&image_name_for_build)
                .build()
                .await
                .map_dockdash_err()?;

            info!("Successfully built image from scratch");
        }
    }

    info!(
        "Successfully built OCI image for resource {} (target: {}) at {}",
        resource_name,
        target.runtime_platform_id(),
        output_path.display()
    );

    Ok(output_path.to_string_lossy().into_owned())
}
