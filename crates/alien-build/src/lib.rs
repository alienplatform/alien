mod base_images;
mod cache;
pub(crate) mod command_output;
pub mod dependencies;
pub mod error;
pub mod merge;
pub mod plan;
mod push;
pub mod settings;
pub mod toolchain;

#[cfg(test)]
mod tests;

pub use push::push_stack;

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

/// Dedupe key for identifying containers that can share the same binary build.
/// Containers with the same (src, toolchain_type, binary_name) produce identical binaries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DedupeKey {
    src: String,
    toolchain_type: ToolchainType,
    binary_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ToolchainType {
    Rust,
    TypeScript,
    Docker,
}

impl DedupeKey {
    /// Extract dedupe key from source path and toolchain config
    fn from_source_and_toolchain(src: &str, toolchain: &ToolchainConfig) -> Self {
        match toolchain {
            ToolchainConfig::Rust { binary_name } => Self {
                src: src.to_string(),
                toolchain_type: ToolchainType::Rust,
                binary_name: binary_name.clone(),
            },
            ToolchainConfig::TypeScript { binary_name } => Self {
                src: src.to_string(),
                toolchain_type: ToolchainType::TypeScript,
                binary_name: binary_name.clone().unwrap_or_else(|| "default".to_string()),
            },
            ToolchainConfig::Docker { dockerfile, .. } => Self {
                src: src.to_string(),
                toolchain_type: ToolchainType::Docker,
                binary_name: dockerfile
                    .clone()
                    .unwrap_or_else(|| "Dockerfile".to_string()),
            },
        }
    }
}

/// True when the caller explicitly requested only non-Linux (host-binary) targets, so a
/// container — which is always a Linux image — has nothing to build here. `None` (the
/// default, host OS) returns false, so a plain `alien build --platforms local` still builds
/// containers as a host-Linux image; only an explicit `--targets darwin-arm64` (the macOS /
/// Windows runner group of a native-runner CI build) skips them.
fn requested_host_binary_only(targets: Option<&[BinaryTarget]>) -> bool {
    targets.is_some_and(|ts| !ts.is_empty() && ts.iter().all(|t| t.oci_os() != "linux"))
}

/// Builds a given `Stack`, processing `WorkerCode::Source` into `WorkerCode::Image`,
/// building and pushing container images, generating platform-specific templates,
/// and saving the result to the output directory.
#[alien_event(AlienEvent::BuildingStack {
    stack: stack.id().to_string(),
})]
pub async fn build_stack(mut stack: Stack, settings: &BuildSettings) -> Result<Stack> {
    let build_stack_started = Instant::now();
    info!(
        "Starting stack build process for platform: {:?}...",
        settings.platform.runtime_platform()
    );

    // Run preflights (compile-time checks only)
    let preflight_runner = PreflightRunner::new();
    let preflight_started = Instant::now();
    let preflight_summary = AlienEvent::RunningPreflights {
        stack: stack.id().to_string(),
        platform: settings.platform.runtime_platform().as_str().to_string(),
    }
    .in_scope(|_| async {
        preflight_runner
            .run_build_time_preflights(&stack, settings.platform.runtime_platform())
            .await
            .context(ErrorData::StackProcessorFailed {
                message: "Failed to run build-time preflights".to_string(),
            })
    })
    .await?;

    // Log preflight results
    if preflight_summary.warning_count > 0 {
        for warning in preflight_summary.get_warnings() {
            tracing::warn!("Preflight warning: {}", warning);
        }
    }

    info!(
        "Build-time preflights completed in {:.2}s: {} checks passed, {} warnings",
        preflight_started.elapsed().as_secs_f64(),
        preflight_summary.passed_checks,
        preflight_summary.warning_count
    );

    let base_output_dir = PathBuf::from(&settings.output_directory);
    let platform_name = settings.platform.runtime_platform().as_str();
    let output_dir = base_output_dir.join("build").join(platform_name);
    info!("Target output directory: {}", output_dir.display());

    // Keep prior hashed artifacts in place so concurrent builds/releases do not
    // invalidate each other's image directories. The latest stack.json overwrite
    // still makes the newest build authoritative.
    fs::create_dir_all(&output_dir)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: output_dir.display().to_string(),
            reason: "I/O error during directory creation".to_string(),
        })?;
    info!("Ensured output directory exists: {}", output_dir.display());

    let stack_id = stack.id().to_string();

    // Collect functions that need building
    let mut functions_to_build = Vec::new();
    let mut daemons_to_build: Vec<(String, Daemon, String, ToolchainConfig)> = Vec::new();

    for (id, resource_entry) in stack.resources() {
        if let Some(func) = resource_entry.config.downcast_ref::<alien_core::Worker>() {
            info!("Processing function: {}", func.id);
            match &func.code {
                WorkerCode::Source { src, toolchain } => {
                    info!(
                        "Worker '{}' has source code. Queued for parallel build.",
                        func.id
                    );
                    functions_to_build.push((
                        id.clone(), // Include resource ID in the tuple
                        func.clone(),
                        src.clone(),
                        toolchain.clone(),
                    ));
                }
                WorkerCode::Image { .. } => {
                    info!("Worker '{}' already has an image. Skipping.", func.id);
                }
            }
        } else if let Some(daemon) = resource_entry.config.downcast_ref::<Daemon>() {
            info!("Processing daemon: {}", daemon.id);
            match &daemon.code {
                DaemonCode::Source { src, toolchain } => {
                    info!(
                        "Daemon '{}' has source code. Queued for parallel build.",
                        daemon.id
                    );
                    daemons_to_build.push((
                        id.clone(),
                        daemon.clone(),
                        src.clone(),
                        toolchain.clone(),
                    ));
                }
                DaemonCode::Image { .. } => {
                    info!("Daemon '{}' already has an image. Skipping.", daemon.id);
                }
            }
        }
    }

    // Collect containers that need building or exporting. On a host-binary-only build
    // there is nothing to collect — see `requested_host_binary_only`; the containers'
    // Linux images come from the Linux runner groups and are combined by `alien build merge`.
    let mut containers_to_build = Vec::new();
    let mut containers_to_export = Vec::new();
    let skip_containers = requested_host_binary_only(settings.targets.as_deref());

    if skip_containers {
        let container_count = stack
            .resources()
            .filter(|(_, e)| e.config.downcast_ref::<Container>().is_some())
            .count();
        if container_count > 0 {
            info!(
                "Skipping {} container(s) for host-binary-only targets {:?}; their Linux images come from the Linux runner groups.",
                container_count,
                settings.get_targets()
            );
        }
    } else {
        for (id, resource_entry) in stack.resources() {
            if let Some(container) = resource_entry.config.downcast_ref::<Container>() {
                info!("Processing container: {}", container.id);
                match &container.code {
                    ContainerCode::Source { src, toolchain } => {
                        info!(
                            "Container '{}' has source code. Queued for parallel build.",
                            container.id
                        );
                        containers_to_build.push((
                            id.clone(),
                            container.clone(),
                            src.clone(),
                            toolchain.clone(),
                        ));
                    }
                    ContainerCode::Image { image } => {
                        info!(
                            "Container '{}' has image reference '{}'. Queued for pull and export.",
                            container.id, image
                        );
                        containers_to_export.push((id.clone(), container.clone(), image.clone()));
                    }
                }
            }
        }
    }

    // Build all functions in parallel with fail-fast behavior
    if !functions_to_build.is_empty() {
        // Get targets for this build
        let build_targets = settings.get_targets();

        info!(
            "Building {} functions for {} target(s): {:?}",
            functions_to_build.len(),
            build_targets.len(),
            build_targets
        );

        // Get current event bus to propagate to spawned tasks
        let current_bus = alien_core::EventBus::current();

        // Create cancellation token for fail-fast behavior
        let cancel_token = tokio_util::sync::CancellationToken::new();

        let build_tasks: Vec<_> = functions_to_build
            .into_iter()
            .map(|(resource_id, func, src, toolchain)| {
                let func_id = func.id.clone();
                let stack_id = stack_id.clone();
                let settings = settings.clone();
                let output_dir = output_dir.clone();
                let bus = current_bus.clone();
                let cancel_token = cancel_token.clone();

                tokio::spawn(async move {
                    // Clone func_id for use in error logging
                    let func_id_for_warning = func_id.clone();

                    // Define the actual work function
                    let build_work = async move {
                        info!("Starting parallel build for resource: {}", func_id);

                        // Check if we're already cancelled
                        if cancel_token.is_cancelled() {
                            return (
                                resource_id.clone(),
                                func,
                                Err(AlienError::new(ErrorData::BuildCanceled {
                                    resource_name: func_id.clone(),
                                })),
                            );
                        }

                        // Build for all targets (handles both single and multiple targets)
                        let result = tokio::select! {
                            result = build_resource(
                                &src,
                                &toolchain,
                                &func_id,
                                &stack_id,
                                &settings,
                                &output_dir,
                                toolchain::WorkloadKind::Worker,
                                &[],
                            ) => result,
                            _ = cancel_token.cancelled() => {
                                info!("Build for worker '{}' was cancelled", func_id);
                                Err(AlienError::new(ErrorData::BuildCanceled {
                                    resource_name: func_id.clone()
                                }))
                            }
                        };

                        match &result {
                            Ok(image_uri) => {
                                info!(
                                    "Successfully built OCI image for resource '{}' to: {}",
                                    func_id, image_uri
                                );
                            }
                            Err(e) => {
                                info!("Failed to build worker '{}': {}", func_id, e);
                            }
                        }

                        (resource_id, func, result)
                    };

                    // CRITICAL: Run with event bus context to ensure events propagate properly
                    match bus {
                        Some(bus) => bus.run(|| build_work).await,
                        None => {
                            tracing::debug!(
                                "No event bus context available for parallel build of worker '{}'",
                                func_id_for_warning
                            );
                            build_work.await
                        }
                    }
                })
            })
            .collect();

        // Wait for first failure or all completions (fail-fast)
        let mut build_results = Vec::new(); // Now stores (resource_id, updated_func)
        let mut completed_tasks = 0;

        // Use futures::future::select_all to get results as they complete
        let mut remaining_tasks = build_tasks;
        let mut first_error: Option<AlienError<ErrorData>> = None;

        while !remaining_tasks.is_empty() {
            let (result, _index, rest) = futures::future::select_all(remaining_tasks).await;
            remaining_tasks = rest;

            match result {
                Ok((resource_id, func, build_result)) => {
                    match build_result {
                        Ok(image_uri) => {
                            // Success - update the function
                            let mut updated_func = func;
                            updated_func.code = WorkerCode::Image { image: image_uri };
                            build_results.push((resource_id, updated_func));
                            completed_tasks += 1;
                        }
                        Err(e) => {
                            // Build failed - cancel all remaining tasks and return immediately
                            if first_error.is_none() {
                                first_error = Some(e);
                                cancel_token.cancel();

                                // Cancel the remaining tasks by aborting them
                                for task in remaining_tasks {
                                    task.abort();
                                }
                                break;
                            }
                        }
                    }
                }
                Err(join_error) => {
                    // Task panicked or was aborted
                    if join_error.is_cancelled() {
                        info!("Build task was cancelled");
                    } else {
                        tracing::warn!("Build task failed: {}", join_error);
                        if first_error.is_none() {
                            first_error = Some(AlienError::new(ErrorData::BuildConfigInvalid {
                                message: format!("Build task failed: {}", join_error),
                            }));
                            cancel_token.cancel();
                        }
                    }
                }
            }
        }

        // If we had an error, return it
        if let Some(error) = first_error {
            return Err(error);
        }

        // Update the stack with the successfully built functions
        for (resource_id, updated_func) in build_results {
            if let Some(resource_entry) = stack.resources_mut().find(|(id, _)| *id == &resource_id)
            {
                resource_entry.1.config = alien_core::Resource::new(updated_func);
            }
        }

        info!(
            "Completed parallel building of {} functions",
            completed_tasks
        );
    }

    // Build all daemons in parallel with fail-fast behavior.
    // Daemons are long-lived native subprocesses that run runtime-less under
    // direct supervision, with no runtime wrapper or transport environment.
    // Their build path mirrors workers: produce an OCI tarball per target,
    // then rewrite the resource's `code` to `DaemonCode::Image` so the local
    // platform's LocalDaemonController can hand the path to extract_daemon_image.
    if !daemons_to_build.is_empty() {
        let build_targets = settings.get_targets();

        info!(
            "Building {} daemons for {} target(s): {:?}",
            daemons_to_build.len(),
            build_targets.len(),
            build_targets
        );

        let current_bus = alien_core::EventBus::current();
        let cancel_token = tokio_util::sync::CancellationToken::new();

        let build_tasks: Vec<_> = daemons_to_build
            .into_iter()
            .map(|(resource_id, daemon, src, toolchain)| {
                let daemon_id = daemon.id.clone();
                let stack_id = stack_id.clone();
                let settings = settings.clone();
                let output_dir = output_dir.clone();
                let bus = current_bus.clone();
                let cancel_token = cancel_token.clone();

                tokio::spawn(async move {
                    let daemon_id_for_warning = daemon_id.clone();

                    let build_work = async move {
                        info!("Starting parallel build for resource: {}", daemon_id);

                        if cancel_token.is_cancelled() {
                            return (
                                resource_id.clone(),
                                daemon,
                                Err(AlienError::new(ErrorData::BuildCanceled {
                                    resource_name: daemon_id.clone(),
                                })),
                            );
                        }

                        let result = tokio::select! {
                            result = build_resource(
                                &src,
                                &toolchain,
                                &daemon_id,
                                &stack_id,
                                &settings,
                                &output_dir,
                                toolchain::WorkloadKind::Daemon,
                                &[],
                            ) => result,
                            _ = cancel_token.cancelled() => {
                                info!("Build for daemon '{}' was cancelled", daemon_id);
                                Err(AlienError::new(ErrorData::BuildCanceled {
                                    resource_name: daemon_id.clone()
                                }))
                            }
                        };

                        match &result {
                            Ok(image_uri) => {
                                info!(
                                    "Successfully built OCI image for resource '{}' to: {}",
                                    daemon_id, image_uri
                                );
                            }
                            Err(e) => {
                                info!("Failed to build daemon '{}': {}", daemon_id, e);
                            }
                        }

                        (resource_id, daemon, result)
                    };

                    match bus {
                        Some(bus) => bus.run(|| build_work).await,
                        None => {
                            tracing::debug!(
                                "No event bus context available for parallel build of daemon '{}'",
                                daemon_id_for_warning
                            );
                            build_work.await
                        }
                    }
                })
            })
            .collect();

        let mut build_results: Vec<(String, Daemon)> = Vec::new();
        let mut completed_tasks = 0;
        let mut remaining_tasks = build_tasks;
        let mut first_error: Option<AlienError<ErrorData>> = None;

        while !remaining_tasks.is_empty() {
            let (result, _index, rest) = futures::future::select_all(remaining_tasks).await;
            remaining_tasks = rest;

            match result {
                Ok((resource_id, daemon, build_result)) => match build_result {
                    Ok(image_uri) => {
                        let mut updated_daemon = daemon;
                        updated_daemon.code = DaemonCode::Image { image: image_uri };
                        build_results.push((resource_id, updated_daemon));
                        completed_tasks += 1;
                    }
                    Err(e) => {
                        if first_error.is_none() {
                            first_error = Some(e);
                            cancel_token.cancel();
                            for task in remaining_tasks {
                                task.abort();
                            }
                            break;
                        }
                    }
                },
                Err(join_error) => {
                    if join_error.is_cancelled() {
                        info!("Build task was cancelled");
                    } else {
                        tracing::warn!("Build task failed: {}", join_error);
                        if first_error.is_none() {
                            first_error = Some(AlienError::new(ErrorData::BuildConfigInvalid {
                                message: format!("Build task failed: {}", join_error),
                            }));
                            cancel_token.cancel();
                        }
                    }
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        for (resource_id, updated_daemon) in build_results {
            if let Some(resource_entry) = stack.resources_mut().find(|(id, _)| *id == &resource_id)
            {
                resource_entry.1.config = alien_core::Resource::new(updated_daemon);
            }
        }

        info!("Completed parallel building of {} daemons", completed_tasks);
    }

    // Build all containers in parallel with fail-fast behavior
    // OPTIMIZATION: Deduplicate containers that use the same binary (same src + toolchain + binary_name)
    if !containers_to_build.is_empty() {
        // For containers, we need Linux targets even on Local platform because
        // Docker always runs Linux containers (via Linux VM on macOS/Windows).
        // Match the host architecture but always use Linux OS.
        let container_settings = if matches!(settings.platform, PlatformBuildSettings::Local { .. })
        {
            let mut s = settings.clone();
            s.targets = Some(vec![BinaryTarget::linux_container_target()]);
            s
        } else {
            settings.clone()
        };

        let build_targets = container_settings.get_targets();

        // Group containers by dedupe key (src, toolchain_type, binary_name)
        let mut dedupe_groups: HashMap<DedupeKey, Vec<(String, Container)>> = HashMap::new();
        for (resource_id, container, src, toolchain) in containers_to_build {
            let dedupe_key = DedupeKey::from_source_and_toolchain(&src, &toolchain);
            dedupe_groups
                .entry(dedupe_key.clone())
                .or_default()
                .push((resource_id, container));
        }

        let unique_builds = dedupe_groups.len();
        let total_containers = dedupe_groups.values().map(|v| v.len()).sum::<usize>();

        info!(
            "Building {} containers ({} unique binaries) for {} target(s): {:?}",
            total_containers,
            unique_builds,
            build_targets.len(),
            build_targets
        );

        if unique_builds < total_containers {
            info!(
                "Deduplication: {} containers share binaries, will build {} unique binaries instead of {}",
                total_containers - unique_builds,
                unique_builds,
                total_containers
            );
        }

        // Get current event bus to propagate to spawned tasks
        let current_bus = alien_core::EventBus::current();

        // Create cancellation token for fail-fast behavior
        let cancel_token = tokio_util::sync::CancellationToken::new();

        // Build one binary per unique dedupe key
        let build_tasks: Vec<_> = dedupe_groups
            .into_iter()
            .map(|(dedupe_key, containers_group)| {
                // Take the first container as representative for building
                let (_representative_resource_id, representative_container) =
                    containers_group.first().unwrap();
                let container_id = representative_container.id.clone();

                // Extract src and toolchain from the representative container
                let (src, toolchain) = match &representative_container.code {
                    ContainerCode::Source { src, toolchain } => (src.clone(), toolchain.clone()),
                    _ => unreachable!("We only collected Source containers"),
                };

                let stack_id = stack_id.clone();
                let settings = container_settings.clone();
                let output_dir = output_dir.clone();
                let bus = current_bus.clone();
                let cancel_token = cancel_token.clone();

                tokio::spawn(async move {
                    let container_id_for_warning = container_id.clone();

                    let build_work = async move {
                        // Build related_resources list: all container IDs in this dedup group
                        let related_resources: Vec<String> = containers_group
                            .iter()
                            .map(|(_, c)| c.id.clone())
                            .collect();

                        if containers_group.len() > 1 {
                            info!(
                                "Building shared binary for {} containers: {:?}",
                                containers_group.len(),
                                related_resources
                            );
                        } else {
                            info!("Starting parallel build for container: {}", container_id);
                        }

                        // Check if we're already cancelled
                        if cancel_token.is_cancelled() {
                            return (dedupe_key.clone(), containers_group, Err(AlienError::new(ErrorData::BuildCanceled {
                                resource_name: container_id.clone()
                            })));
                        }

                        // Build for all targets - reuse build logic since containers use same toolchains
                        let result = tokio::select! {
                            result = build_resource(
                                &src,
                                &toolchain,
                                &container_id,
                                &stack_id,
                                &settings,
                                &output_dir,
                                toolchain::WorkloadKind::Container,
                                &related_resources,
                            ) => result,
                            _ = cancel_token.cancelled() => {
                                info!("Build for container '{}' was cancelled", container_id);
                                Err(AlienError::new(ErrorData::BuildCanceled {
                                    resource_name: container_id.clone()
                                }))
                            }
                        };

                        match &result {
                            Ok(image_uri) => {
                                if containers_group.len() > 1 {
                                    info!(
                                        "Successfully built shared binary for {} containers to: {}",
                                        containers_group.len(), image_uri
                                    );
                                } else {
                                    info!(
                                        "Successfully built OCI image for container '{}' to: {}",
                                        container_id, image_uri
                                    );
                                }
                            }
                            Err(e) => {
                                info!("Failed to build container '{}': {}", container_id, e);
                            }
                        }

                        (dedupe_key, containers_group, result)
                    };

                    match bus {
                        Some(bus) => bus.run(|| build_work).await,
                        None => {
                            tracing::debug!(
                                "No event bus context available for parallel build of container '{}'",
                                container_id_for_warning
                            );
                            build_work.await
                        }
                    }
                })
            })
            .collect();

        // Wait for first failure or all completions (fail-fast)
        let mut build_results = Vec::new();
        let mut completed_tasks = 0;

        let mut remaining_tasks = build_tasks;
        let mut first_error: Option<AlienError<ErrorData>> = None;

        while !remaining_tasks.is_empty() {
            let (result, _index, rest) = futures::future::select_all(remaining_tasks).await;
            remaining_tasks = rest;

            match result {
                Ok((_dedupe_key, containers_group, build_result)) => {
                    match build_result {
                        Ok(image_uri) => {
                            // Success - update ALL containers in this group with the same image
                            for (resource_id, container) in containers_group {
                                let mut updated_container = container;
                                updated_container.code = ContainerCode::Image {
                                    image: image_uri.clone(),
                                };
                                build_results.push((resource_id, updated_container));
                            }
                            completed_tasks += 1;
                        }
                        Err(e) => {
                            // Build failed - cancel all remaining tasks
                            if first_error.is_none() {
                                first_error = Some(e);
                                cancel_token.cancel();

                                for task in remaining_tasks {
                                    task.abort();
                                }
                                break;
                            }
                        }
                    }
                }
                Err(join_error) => {
                    if join_error.is_cancelled() {
                        info!("Container build task was cancelled");
                    } else {
                        tracing::warn!("Container build task failed: {}", join_error);
                        if first_error.is_none() {
                            first_error = Some(AlienError::new(ErrorData::BuildConfigInvalid {
                                message: format!("Container build task failed: {}", join_error),
                            }));
                            cancel_token.cancel();
                        }
                    }
                }
            }
        }

        // If we had an error, return it
        if let Some(error) = first_error {
            return Err(error);
        }

        // Update the stack with the successfully built containers
        for (resource_id, updated_container) in build_results {
            if let Some(resource_entry) = stack.resources_mut().find(|(id, _)| *id == &resource_id)
            {
                resource_entry.1.config = alien_core::Resource::new(updated_container);
            }
        }

        info!(
            "Completed parallel building of {} unique binaries for {} containers",
            completed_tasks, total_containers
        );
    }

    // Export pre-built container images (pull and convert to OCI tarballs)
    if !containers_to_export.is_empty() {
        let container_settings = if matches!(settings.platform, PlatformBuildSettings::Local { .. })
        {
            let mut s = settings.clone();
            s.targets = Some(vec![BinaryTarget::linux_container_target()]);
            s
        } else {
            settings.clone()
        };

        let build_targets = container_settings.get_targets();

        info!(
            "Exporting {} pre-built container images for {} target(s): {:?}",
            containers_to_export.len(),
            build_targets.len(),
            build_targets
        );

        let current_bus = alien_core::EventBus::current();
        let cancel_token = tokio_util::sync::CancellationToken::new();

        let export_tasks: Vec<_> = containers_to_export
            .into_iter()
            .map(|(resource_id, container, image)| {
                let container_id = container.id.clone();
                let stack_id = stack_id.clone();
                let settings = container_settings.clone();
                let output_dir = output_dir.clone();
                let bus = current_bus.clone();
                let cancel_token = cancel_token.clone();

                tokio::spawn(async move {
                    let container_id_for_warning = container_id.clone();

                    let export_work = async move {
                        info!(
                            "Starting pull and export for container '{}' image: {}",
                            container_id, image
                        );

                        if cancel_token.is_cancelled() {
                            return (
                                resource_id.clone(),
                                container,
                                Err(AlienError::new(ErrorData::BuildCanceled {
                                    resource_name: container_id.clone(),
                                })),
                            );
                        }

                        let result = tokio::select! {
                            result = pull_and_export_image(
                                &image,
                                &container_id,
                                &stack_id,
                                &settings,
                                &output_dir,
                            ) => result,
                            _ = cancel_token.cancelled() => {
                                info!("Export for container '{}' was cancelled", container_id);
                                Err(AlienError::new(ErrorData::BuildCanceled {
                                    resource_name: container_id.clone()
                                }))
                            }
                        };

                        match &result {
                            Ok(image_uri) => {
                                info!(
                                    "Successfully exported container '{}' to: {}",
                                    container_id, image_uri
                                );
                            }
                            Err(e) => {
                                info!("Failed to export container '{}': {}", container_id, e);
                            }
                        }

                        (resource_id, container, result)
                    };

                    match bus {
                        Some(bus) => bus.run(|| export_work).await,
                        None => {
                            tracing::warn!(
                                "No event bus context available for export of container '{}'",
                                container_id_for_warning
                            );
                            export_work.await
                        }
                    }
                })
            })
            .collect();

        let mut export_results = Vec::new();
        let mut completed_tasks = 0;
        let mut remaining_tasks = export_tasks;
        let mut first_error: Option<AlienError<ErrorData>> = None;

        while !remaining_tasks.is_empty() {
            let (result, _index, rest) = futures::future::select_all(remaining_tasks).await;
            remaining_tasks = rest;

            match result {
                Ok((resource_id, container, export_result)) => match export_result {
                    Ok(image_uri) => {
                        let mut updated_container = container;
                        updated_container.code = ContainerCode::Image { image: image_uri };
                        export_results.push((resource_id, updated_container));
                        completed_tasks += 1;
                    }
                    Err(e) => {
                        if first_error.is_none() {
                            first_error = Some(e);
                            cancel_token.cancel();

                            for task in remaining_tasks {
                                task.abort();
                            }
                            break;
                        }
                    }
                },
                Err(join_error) => {
                    if join_error.is_cancelled() {
                        info!("Container export task was cancelled");
                    } else {
                        tracing::warn!("Container export task failed: {}", join_error);
                        if first_error.is_none() {
                            first_error = Some(AlienError::new(ErrorData::BuildConfigInvalid {
                                message: format!("Container export task failed: {}", join_error),
                            }));
                            cancel_token.cancel();
                        }
                    }
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        for (resource_id, updated_container) in export_results {
            if let Some(resource_entry) = stack.resources_mut().find(|(id, _)| *id == &resource_id)
            {
                resource_entry.1.config = alien_core::Resource::new(updated_container);
            }
        }

        info!("Completed exporting {} container images", completed_tasks);
    }

    strip_local_daemon_only_compute_clusters(&mut stack, settings.platform.runtime_platform());

    // Save the modified stack configuration to stack.json
    let stack_json_path = output_dir.join("stack.json");
    let stack_json_content = serde_json::to_string_pretty(&stack)
        .into_alien_error()
        .context(ErrorData::JsonSerializationError {
            message: "Failed to serialize stack configuration to JSON".to_string(),
        })?;
    fs::write(&stack_json_path, stack_json_content)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: stack_json_path.display().to_string(),
            reason: "I/O error during stack.json file write".to_string(),
        })?;
    info!(
        "Saved built stack configuration to {}",
        stack_json_path.display()
    );

    info!(
        "Stack build process completed in {:.2}s.",
        build_stack_started.elapsed().as_secs_f64()
    );
    Ok(stack)
}

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
