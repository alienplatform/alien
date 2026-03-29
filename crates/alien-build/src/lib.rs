pub mod dependencies;
pub mod error;
pub mod settings;
pub mod toolchain;

use alien_core::{
    alien_event, AlienEvent, BinaryTarget, Container, ContainerCode, Function, FunctionCode,
    Platform, Stack, ToolchainConfig,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_preflights::runner::PreflightRunner;
use dockdash::{Image as DockDashImage, Layer as DockDashLayer, PullPolicy};
use error::{DockdashResultExt, ErrorData, Result};
use rand::distr::Alphanumeric;
use rand::Rng;
use reqwest::Url;
use settings::{BinaryTargetExt, BuildSettings, PlatformBuildSettings, PushSettings};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

use tracing::info;

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

/// Builds a given `Stack`, processing `FunctionCode::Source` into `FunctionCode::Image`,
/// building and pushing container images, generating platform-specific templates,
/// and saving the result to the output directory.
#[alien_event(AlienEvent::BuildingStack {
    stack: stack.id().to_string(),
})]
pub async fn build_stack(mut stack: Stack, settings: &BuildSettings) -> Result<Stack> {
    info!(
        "Starting stack build process for platform: {:?}...",
        settings.platform.platform()
    );

    // Run preflights (compile-time checks only)
    let preflight_runner = PreflightRunner::new();
    let preflight_summary = AlienEvent::RunningPreflights {
        stack: stack.id().to_string(),
        platform: settings.platform.platform().as_str().to_string(),
    }
    .in_scope(|_| async {
        preflight_runner
            .run_build_time_preflights(&stack, settings.platform.platform())
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
        "Build-time preflights completed: {} checks passed, {} warnings",
        preflight_summary.passed_checks, preflight_summary.warning_count
    );

    let base_output_dir = PathBuf::from(&settings.output_directory);
    let platform_name = settings.platform.platform().as_str();
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

    for (id, resource_entry) in stack.resources() {
        if let Some(func) = resource_entry.config.downcast_ref::<alien_core::Function>() {
            info!("Processing function: {}", func.id);
            match &func.code {
                FunctionCode::Source { src, toolchain } => {
                    info!(
                        "Function '{}' has source code. Queued for parallel build.",
                        func.id
                    );
                    functions_to_build.push((
                        id.clone(), // Include resource ID in the tuple
                        func.clone(),
                        src.clone(),
                        toolchain.clone(),
                    ));
                }
                FunctionCode::Image { .. } => {
                    info!("Function '{}' already has an image. Skipping.", func.id);
                }
            }
        }
    }

    // Collect containers that need building or exporting
    let mut containers_to_build = Vec::new();
    let mut containers_to_export = Vec::new();

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
                        info!("Starting parallel build for function: {}", func_id);

                        // Check if we're already cancelled
                        if cancel_token.is_cancelled() {
                            return (resource_id.clone(), func, Err(AlienError::new(ErrorData::BuildCanceled {
                                resource_name: func_id.clone()
                            })));
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
                                false, // is_container = false for Function resources
                                "function",
                                &[],
                            ) => result,
                            _ = cancel_token.cancelled() => {
                                info!("Build for function '{}' was cancelled", func_id);
                                Err(AlienError::new(ErrorData::BuildCanceled {
                                    resource_name: func_id.clone()
                                }))
                            }
                        };

                        match &result {
                            Ok(image_uri) => {
                                info!(
                                    "Successfully built OCI image for function '{}' to: {}",
                                    func_id, image_uri
                                );
                            }
                            Err(e) => {
                                info!("Failed to build function '{}': {}", func_id, e);
                            }
                        }

                        (resource_id, func, result)
                    };

                    // CRITICAL: Run with event bus context to ensure events propagate properly
                    match bus {
                        Some(bus) => bus.run(|| build_work).await,
                        None => {
                            tracing::debug!(
                                "No event bus context available for parallel build of function '{}'",
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
                            updated_func.code = FunctionCode::Image { image: image_uri };
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
                                true, // is_container = true for Container resources
                                "container",
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

    info!("Stack build process completed.");
    Ok(stack)
}

/// A compute resource that has a locally-built image directory and needs to be pushed to a registry.
struct ResourcePushTarget {
    /// Stack resource key (used to locate the resource for updating after push)
    resource_id: String,
    /// The resource's own ID (e.g. `func.id`) — used for logging and image tagging
    resource_name: String,
    /// Display name for events/logging ("function", "container", etc.)
    resource_type: &'static str,
    /// Local directory containing OCI tarballs produced by `alien build`
    local_image_dir: PathBuf,
}

/// Scans all resources in the stack and returns those with locally-built images that need
/// pushing to a registry.
///
/// Returns an error if any compute resource still has unbuilt source code — that means
/// `alien build` was not run first.
///
/// To add support for a new compute resource type, add an `else if` branch here and in
/// [`apply_pushed_images`].
fn collect_push_targets(stack: &Stack) -> Result<Vec<ResourcePushTarget>> {
    let mut targets = Vec::new();

    for (resource_id, resource_entry) in stack.resources() {
        if let Some(func) = resource_entry.config.downcast_ref::<Function>() {
            match &func.code {
                FunctionCode::Image { image } => {
                    let path = PathBuf::from(image);
                    if path.exists() && path.is_dir() {
                        info!(
                            "Function '{}' has local image directory, queuing for push",
                            func.id
                        );
                        targets.push(ResourcePushTarget {
                            resource_id: resource_id.clone(),
                            resource_name: func.id.clone(),
                            resource_type: "function",
                            local_image_dir: path,
                        });
                    } else {
                        info!("Function '{}' already has remote image: {}", func.id, image);
                    }
                }
                FunctionCode::Source { .. } => {
                    return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                        resource_id: func.id.clone(),
                        reason: "Function has source code instead of built image. Run 'alien build' first.".to_string(),
                    }));
                }
            }
        } else if let Some(container) = resource_entry.config.downcast_ref::<Container>() {
            match &container.code {
                ContainerCode::Image { image } => {
                    let path = PathBuf::from(image);
                    if path.exists() && path.is_dir() {
                        info!(
                            "Container '{}' has local image directory, queuing for push",
                            container.id
                        );
                        targets.push(ResourcePushTarget {
                            resource_id: resource_id.clone(),
                            resource_name: container.id.clone(),
                            resource_type: "container",
                            local_image_dir: path,
                        });
                    } else {
                        info!(
                            "Container '{}' already has remote image: {}",
                            container.id, image
                        );
                    }
                }
                ContainerCode::Source { .. } => {
                    return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                        resource_id: container.id.clone(),
                        reason: "Container has source code instead of built image. Run 'alien build' first.".to_string(),
                    }));
                }
            }
        }
    }

    Ok(targets)
}

/// Applies pushed registry URIs back to their respective resources in the stack.
///
/// To add support for a new compute resource type, add an `else if` branch here and in
/// [`collect_push_targets`].
fn apply_pushed_images(stack: &mut Stack, updates: Vec<(String, String)>) {
    for (resource_id, image_uri) in updates {
        if let Some(resource_entry) = stack.resources_mut().find(|(id, _)| *id == &resource_id) {
            if let Some(func) = resource_entry.1.config.downcast_mut::<Function>() {
                func.code = FunctionCode::Image { image: image_uri };
            } else if let Some(container) = resource_entry.1.config.downcast_mut::<Container>() {
                container.code = ContainerCode::Image { image: image_uri };
            }
        }
    }
}

/// Push built images from local OCI tarballs to a container registry.
/// Reads a stack with local image directory references, pushes them to the registry,
/// and returns an updated stack with pushed image URLs (in memory, not saved to disk).
#[alien_event(AlienEvent::PushingStack {
    stack: stack.id().to_string(),
    platform: platform.as_str().to_string(),
})]
pub async fn push_stack(
    mut stack: Stack,
    platform: Platform,
    push_settings: &PushSettings,
) -> Result<Stack> {
    info!(
        "Starting image push process to registry: {}",
        push_settings.repository
    );

    let to_push = collect_push_targets(&stack)?;

    info!("Pushing {} resource(s) to registry", to_push.len());

    if to_push.is_empty() {
        info!("Image push process completed. No local images to push.");
        return Ok(stack);
    }

    // Push all resources in parallel with fail-fast behavior
    let current_bus = alien_core::EventBus::current();
    let cancel_token = tokio_util::sync::CancellationToken::new();

    let push_tasks: Vec<_> = to_push
        .into_iter()
        .map(|target| {
            let resource_name = target.resource_name.clone();
            let repository = push_settings.repository.clone();
            let push_opts = push_settings.options.clone();
            let bus = current_bus.clone();
            let cancel_token = cancel_token.clone();

            tokio::spawn(async move {
                let resource_name_for_warning = resource_name.clone();

                let push_work = async move {
                    info!("Starting parallel push for {} '{}'", target.resource_type, resource_name);

                    if cancel_token.is_cancelled() {
                        return (target.resource_id, Err(AlienError::new(ErrorData::BuildCanceled {
                            resource_name: resource_name.clone(),
                        })));
                    }

                    let result = tokio::select! {
                        result = push_resource_images(
                            &resource_name,
                            target.resource_type,
                            &target.local_image_dir,
                            &repository,
                            &push_opts,
                        ) => result,
                        _ = cancel_token.cancelled() => {
                            info!("Push for {} '{}' was cancelled", target.resource_type, resource_name);
                            Err(AlienError::new(ErrorData::BuildCanceled {
                                resource_name: resource_name.clone(),
                            }))
                        }
                    };

                    match &result {
                        Ok(image_uri) => info!("Successfully pushed {} '{}' to: {}", target.resource_type, resource_name, image_uri),
                        Err(e) => info!("Failed to push {} '{}': {}", target.resource_type, resource_name, e),
                    }

                    (target.resource_id, result)
                };

                match bus {
                    Some(bus) => bus.run(|| push_work).await,
                    None => {
                        tracing::warn!("No event bus context available for parallel push of '{}'", resource_name_for_warning);
                        push_work.await
                    }
                }
            })
        })
        .collect();

    // Wait for first failure or all completions (fail-fast)
    let mut push_results: Vec<(String, String)> = Vec::new();
    let mut completed_tasks = 0;
    let mut remaining_tasks = push_tasks;
    let mut first_error: Option<AlienError<ErrorData>> = None;

    while !remaining_tasks.is_empty() {
        let (result, _index, rest) = futures::future::select_all(remaining_tasks).await;
        remaining_tasks = rest;

        match result {
            Ok((resource_id, push_result)) => match push_result {
                Ok(image_uri) => {
                    push_results.push((resource_id, image_uri));
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
                    info!("Push task was cancelled");
                } else {
                    tracing::warn!("Push task failed: {}", join_error);
                    if first_error.is_none() {
                        first_error = Some(AlienError::new(ErrorData::ImagePushFailed {
                            image: "unknown".to_string(),
                            reason: format!("Push task failed: {}", join_error),
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

    info!(
        "Completed parallel pushing of {} resource(s)",
        completed_tasks
    );

    apply_pushed_images(&mut stack, push_results);

    info!(
        "Image push process completed. Stack updated with {} remote image URL(s).",
        completed_tasks
    );

    Ok(stack)
}

/// Push all OCI tarballs for a resource to the registry
#[alien_event(AlienEvent::PushingResource {
    resource_name: resource_name.to_string(),
    resource_type: resource_type.to_string(),
})]
async fn push_resource_images(
    resource_name: &str,
    resource_type: &str,
    images_dir: &Path,
    repository: &str,
    push_options: &dockdash::PushOptions,
) -> Result<String> {
    info!(
        "Pushing images for resource '{}' from {}",
        resource_name,
        images_dir.display()
    );

    // Generate unique tag for this push
    let image_tag = generate_unique_tag();
    // Resource name is part of the tag, not a path segment
    let full_tag = format!("{}-{}", resource_name, image_tag);
    let image_uri = format!("{}:{}", repository, full_tag);

    // Find all OCI tarball files in the directory
    let mut oci_files = Vec::new();
    let mut entries = fs::read_dir(images_dir).await.into_alien_error().context(
        ErrorData::FileOperationFailed {
            operation: "read directory".to_string(),
            file_path: images_dir.display().to_string(),
            reason: "Failed to list OCI tarballs".to_string(),
        },
    )?;

    while let Some(entry) =
        entries
            .next_entry()
            .await
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory entry".to_string(),
                file_path: images_dir.display().to_string(),
                reason: "Failed to read directory entry".to_string(),
            })?
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("tar")
            && path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.contains(".oci."))
                .unwrap_or(false)
        {
            oci_files.push(path);
        }
    }

    if oci_files.is_empty() {
        return Err(AlienError::new(ErrorData::InvalidResourceConfig {
            resource_id: resource_name.to_string(),
            reason: format!("No OCI tarball files found in {}", images_dir.display()),
        }));
    }

    info!(
        "Found {} OCI tarball(s) to push for resource '{}'",
        oci_files.len(),
        resource_name
    );

    // Create push options with progress callback
    let mut push_opts_with_progress = push_options.clone();

    // Add progress callback that emits alien events
    struct AlienPushProgressCallback {
        resource_name: String,
    }

    #[async_trait::async_trait]
    impl dockdash::PushProgressCallback for AlienPushProgressCallback {
        async fn on_progress(&self, progress: dockdash::PushProgressInfo) {
            // Emit PushingImage event with progress
            let _ = AlienEvent::PushingImage {
                image: self.resource_name.clone(),
                progress: Some(alien_core::PushProgress {
                    operation: progress.operation,
                    layers_uploaded: progress.layers_uploaded,
                    total_layers: progress.total_layers,
                    bytes_uploaded: progress.bytes_uploaded,
                    total_bytes: progress.total_bytes,
                }),
            }
            .emit()
            .await;
        }
    }

    push_opts_with_progress.progress_callback = Some(Box::new(AlienPushProgressCallback {
        resource_name: resource_name.to_string(),
    }));

    // Push each OCI tarball (multi-architecture manifest)
    for oci_file in &oci_files {
        info!("Pushing {}", oci_file.display());

        // Load the OCI tarball
        let image = DockDashImage::from_tarball(oci_file).map_dockdash_err()?;

        // Push to registry with progress callback
        image
            .push(&image_uri, &push_opts_with_progress)
            .await
            .map_dockdash_err()?;

        info!(
            "Successfully pushed {} to {}",
            oci_file.display(),
            image_uri
        );
    }

    Ok(image_uri)
}

fn generate_unique_tag() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect::<String>()
        .to_lowercase()
}

fn temp_artifact_dir(build_output_dir: &Path, resource_name: &str) -> PathBuf {
    build_output_dir.join(format!(".{}-tmp-{}", resource_name, generate_unique_tag()))
}

async fn finalize_artifact_dir(
    temp_dir: &Path,
    final_dir: &Path,
    artifact_kind: &str,
) -> Result<String> {
    match fs::rename(temp_dir, final_dir).await {
        Ok(()) => Ok(final_dir.to_string_lossy().into_owned()),
        Err(_rename_error) if final_dir.exists() => {
            let _ = fs::remove_dir_all(temp_dir).await;
            info!(
                "Reusing existing {} artifacts at {}",
                artifact_kind,
                final_dir.display()
            );
            Ok(final_dir.to_string_lossy().into_owned())
        }
        Err(rename_error) => {
            Err(rename_error)
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "rename directory".to_string(),
                    file_path: temp_dir.display().to_string(),
                    reason: format!(
                        "Failed to rename {} directory to {}",
                        artifact_kind,
                        final_dir.display()
                    ),
                })
        }
    }
}

/// Build a resource (function, container, or worker) for one or more OS/architecture targets
///
/// Always saves OCI tarballs to a consistent directory structure:
/// `build_output_dir/function_name/{target}.oci.tar`
#[alien_event(AlienEvent::BuildingResource {
    resource_name: function_name.to_string(),
    resource_type: resource_type.to_string(),
    related_resources: related_resources.to_vec(),
})]
async fn build_resource(
    src: &str,
    toolchain_config: &alien_core::ToolchainConfig,
    function_name: &str,
    stack_id: &str,
    settings: &BuildSettings,
    build_output_dir: &Path,
    is_container: bool,
    resource_type: &str,
    related_resources: &[String],
) -> Result<String> {
    // Get target list from settings (uses platform defaults if not specified)
    let targets = settings.get_targets();

    info!(
        "Building function '{}' for {} target(s): {:?}",
        function_name,
        targets.len(),
        targets
    );

    // Build into a unique staging directory so concurrent builds do not race on
    // the same path before the hashed output is finalized.
    let function_dir = temp_artifact_dir(build_output_dir, function_name);
    fs::create_dir_all(&function_dir)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: function_dir.display().to_string(),
            reason: "Failed to create function directory for build".to_string(),
        })?;

    // Build for each target in parallel
    // Spawn tasks for each target to build concurrently
    let build_tasks: Vec<_> = targets
        .iter()
        .map(|target| {
            let src = src.to_string();
            let toolchain_config = toolchain_config.clone();
            let function_name = function_name.to_string();
            let stack_id = stack_id.to_string();
            let settings = settings.clone();
            let target = *target;
            let function_dir = function_dir.clone();

            tokio::spawn(async move {
                info!("Building for target: {:?}", target);

                // Create target-specific output path
                // Always use target ID in filename for consistency
                let target_filename = format!("{}.oci.tar", target.runtime_platform_id());
                let target_output_path = function_dir.join(&target_filename);

                // Build with toolchain for this specific target
                let result = build_target_to_file(
                    &src,
                    &toolchain_config,
                    &function_name,
                    &stack_id,
                    &settings,
                    &target,
                    &target_output_path,
                    is_container,
                )
                .await?;

                info!(
                    "Successfully built target {} for function '{}' at: {}",
                    target.runtime_platform_id(),
                    function_name,
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
                function_name: function_name.to_string(),
                reason: format!("Build task panicked or was cancelled: {}", e),
                build_output: None,
            })
        })??;
        build_results.push(result);
    }

    // Compute content hash of all built tarballs
    // This ensures the executor detects code changes between builds
    let content_hash = compute_function_content_hash(&function_dir).await?;
    let short_hash = &content_hash[..8];

    // Rename directory to include content hash
    let hashed_dir_name = format!("{}-{}", function_name, short_hash);
    let final_output_dir = build_output_dir.join(&hashed_dir_name);

    let finalized_dir = finalize_artifact_dir(&function_dir, &final_output_dir, "build").await?;

    // Return the directory path containing all OCI tarballs (with content hash)
    info!(
        "Completed build for function '{}'. Images directory: {} (hash: {})",
        function_name,
        final_output_dir.display(),
        short_hash
    );
    Ok(finalized_dir)
}

/// Build a specific OS/architecture target to an OCI tarball file
#[allow(clippy::too_many_arguments)]
async fn build_target_to_file(
    src: &str,
    toolchain_config: &alien_core::ToolchainConfig,
    function_name: &str,
    stack_id: &str,
    settings: &BuildSettings,
    target: &BinaryTarget,
    output_path: &Path,
    is_container: bool,
) -> Result<String> {
    info!(
        "Starting toolchain build for function: {} (target: {})",
        function_name,
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
            function_name,
            target.runtime_platform_id()
        ),
        build_target: *target,
        platform_name: settings.platform.platform().as_str().to_string(),
        debug_mode: settings.debug_mode,
        is_container,
    };

    // Create and run toolchain
    let toolchain = toolchain::create_toolchain(toolchain_config);
    let toolchain_output = toolchain.build(&toolchain_context).await?;

    // Build image with dockdash
    let image_tag = generate_unique_tag();
    let image_name_for_build = format!(
        "{}:{}{}",
        function_name,
        target.runtime_platform_id(),
        image_tag
    );

    // Build image based on strategy
    match &toolchain_output.build_strategy {
        toolchain::ImageBuildStrategy::CompleteOCITarball { tarball_path } => {
            // Toolchain produced a complete OCI tarball (e.g., Docker toolchain)
            // Use it as-is without re-packaging
            if tarball_path != output_path {
                fs::copy(tarball_path, output_path)
                    .await
                    .into_alien_error()
                    .context(ErrorData::FileOperationFailed {
                        operation: "copy file".to_string(),
                        file_path: tarball_path.display().to_string(),
                        reason: format!("Failed to copy OCI tarball to {}", output_path.display()),
                    })?;

                info!("Copied complete OCI tarball to {}", output_path.display());
            }
        }

        toolchain::ImageBuildStrategy::FromBaseImage {
            base_images,
            files_to_package,
        } => {
            // Cloud platform flow - build from base image
            let base_images_to_try: Vec<String> =
                if let Some(override_image) = &settings.override_base_image {
                    vec![override_image.clone()]
                } else {
                    base_images.clone()
                };

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

                // Application files layer
                let mut app_layer_builder = DockDashLayer::builder().map_dockdash_err()?;

                for (host_path, container_path) in files_to_package {
                    let absolute_container_path = if container_path.starts_with("/") {
                        container_path.clone()
                    } else if container_path.starts_with("./") {
                        format!("/app/{}", &container_path[2..])
                    } else {
                        format!("/app/{}", container_path)
                    };

                    if host_path.is_dir() {
                        app_layer_builder = app_layer_builder
                            .directory(host_path, &absolute_container_path)
                            .map_dockdash_err()?;
                    } else if host_path.is_file() {
                        app_layer_builder = app_layer_builder
                            .file(host_path, &absolute_container_path, None)
                            .map_dockdash_err()?;
                    }
                }

                let app_layer = app_layer_builder.build().await.map_dockdash_err()?;

                let image_builder = DockDashImage::builder()
                    .from(base_image)
                    .platform(target.oci_os(), &target.to_dockdash_arch())
                    .pull_policy(PullPolicy::Always)
                    .layer(app_layer)
                    .cmd(toolchain_output.runtime_command.clone()); // Set CMD from toolchain

                let build_result = image_builder
                    .output_to(output_path.to_path_buf())
                    .output_name_and_tag(&image_name_for_build)
                    .build()
                    .await;

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
                            tracing::warn!(
                                "Failed to build with base image '{}': {}. Not retrying.",
                                base_image,
                                e
                            );
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
                        function_name: function_name.to_string(),
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
                .working_dir("/app") // Set working directory so ./app resolves correctly
                .cmd(toolchain_output.runtime_command.clone()); // Set CMD from toolchain

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
        "Successfully built OCI image for function {} (target: {}) at {}",
        function_name,
        target.runtime_platform_id(),
        output_path.display()
    );

    Ok(output_path.to_string_lossy().into_owned())
}

/// Pull a Docker image and export it to OCI tarballs for each target architecture.
/// This handles both registry images (nginx:latest) and local images (my-app:v1).
#[alien_event(AlienEvent::BuildingResource {
    resource_name: container_name.to_string(),
    resource_type: "container".to_string(),
    related_resources: vec![],
})]
async fn pull_and_export_image(
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
                function_name: container_name.to_string(),
                reason: "Failed to execute docker pull".to_string(),
                build_output: None,
            })?;

        if !pull_output.status.success() {
            let stderr = String::from_utf8_lossy(&pull_output.stderr);
            return Err(AlienError::new(ErrorData::ImageBuildFailed {
                function_name: container_name.to_string(),
                reason: format!("docker pull failed for image '{}'", image),
                build_output: Some(stderr.to_string()),
            }));
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
                function_name: container_name.to_string(),
                reason: "Failed to execute docker save".to_string(),
                build_output: None,
            })?;

        if !save_output.status.success() {
            let stderr = String::from_utf8_lossy(&save_output.stderr);
            return Err(AlienError::new(ErrorData::ImageBuildFailed {
                function_name: container_name.to_string(),
                reason: "docker save failed".to_string(),
                build_output: Some(stderr.to_string()),
            }));
        }

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

/// Compute a content hash of all OCI tarballs in a directory.
///
/// This hash is used to detect code changes between builds. When the source code
/// changes, the OCI tarball contents change, producing a different hash. This hash
/// is then included in the output directory name, ensuring the executor detects
/// config changes and plans an UPDATE.
async fn compute_function_content_hash(dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut entries =
        fs::read_dir(dir)
            .await
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory".to_string(),
                file_path: dir.display().to_string(),
                reason: "Failed to read function build directory for hashing".to_string(),
            })?;

    // Collect all OCI tarball paths and sort for deterministic hashing
    let mut tarball_paths: Vec<PathBuf> = vec![];
    while let Some(entry) =
        entries
            .next_entry()
            .await
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory entry".to_string(),
                file_path: dir.display().to_string(),
                reason: "Failed to iterate build directory entries".to_string(),
            })?
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "tar") {
            tarball_paths.push(path);
        }
    }
    tarball_paths.sort();

    // Hash contents of all tarballs in deterministic order
    for path in tarball_paths {
        let contents =
            fs::read(&path)
                .await
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "read file".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to read OCI tarball for hashing".to_string(),
                })?;
        hasher.update(&contents);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockdash::Image;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::tempdir;

    fn docker_available() -> bool {
        Command::new("docker")
            .arg("info")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn test_pull_and_export_alpine() {
        if !docker_available() {
            eprintln!("Skipping test_pull_and_export_alpine: docker not available");
            return;
        }

        tracing_subscriber::fmt::try_init().ok();

        let build_dir = tempdir().unwrap();
        let settings = BuildSettings {
            output_directory: build_dir.path().to_str().unwrap().to_string(),
            platform: PlatformBuildSettings::Test {},
            targets: Some(vec![BinaryTarget::LinuxX64]),
            cache_url: None,
            override_base_image: None,
            debug_mode: false,
        };

        // Pull alpine:latest (small, always available)
        let result = pull_and_export_image(
            "alpine:latest",
            "test-alpine",
            "test-stack",
            &settings,
            build_dir.path(),
        )
        .await;

        assert!(
            result.is_ok(),
            "Should successfully pull and export alpine:latest"
        );

        let image_dir = result.unwrap();
        let image_path = PathBuf::from(&image_dir);

        // Verify directory exists and has content hash
        assert!(image_path.exists(), "Image directory should exist");
        assert!(
            image_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("test-alpine-"),
            "Directory should have content hash suffix"
        );

        // Verify OCI tarball was created
        let tarball_path = image_path.join("linux-x64.oci.tar");
        assert!(tarball_path.exists(), "OCI tarball should exist");

        // Verify tarball is valid OCI format
        let image = Image::from_tarball(&tarball_path).expect("OCI tarball should be valid");

        let metadata = image
            .get_metadata()
            .expect("Should be able to read image metadata");

        // Alpine has a CMD
        assert!(
            metadata.cmd.is_some() || metadata.entrypoint.is_some(),
            "Alpine image should have entrypoint or cmd"
        );
    }

    #[tokio::test]
    async fn test_pull_nonexistent_image_fails() {
        if !docker_available() {
            eprintln!("Skipping test_pull_nonexistent_image_fails: docker not available");
            return;
        }

        tracing_subscriber::fmt::try_init().ok();

        let build_dir = tempdir().unwrap();
        let settings = BuildSettings {
            output_directory: build_dir.path().to_str().unwrap().to_string(),
            platform: PlatformBuildSettings::Test {},
            targets: Some(vec![BinaryTarget::LinuxX64]),
            cache_url: None,
            override_base_image: None,
            debug_mode: false,
        };

        // Try to pull non-existent image
        let result = pull_and_export_image(
            "this-image-definitely-does-not-exist-xyz123:nonexistent",
            "test-nonexistent",
            "test-stack",
            &settings,
            build_dir.path(),
        )
        .await;

        // Should fail with docker pull error
        assert!(result.is_err(), "Should fail for non-existent image");
        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("docker pull failed") || err_str.contains("not found"),
            "Error should mention docker pull failure: {}",
            err_str
        );
    }

    #[tokio::test]
    async fn test_pull_and_export_produces_hash() {
        if !docker_available() {
            eprintln!("Skipping test_pull_and_export_produces_hash: docker not available");
            return;
        }

        tracing_subscriber::fmt::try_init().ok();

        let build_dir = tempdir().unwrap();
        let settings = BuildSettings {
            output_directory: build_dir.path().to_str().unwrap().to_string(),
            platform: PlatformBuildSettings::Test {},
            targets: Some(vec![BinaryTarget::LinuxX64]),
            cache_url: None,
            override_base_image: None,
            debug_mode: false,
        };

        // Pull alpine image
        let result = pull_and_export_image(
            "alpine:latest",
            "test-alpine",
            "test-stack",
            &settings,
            build_dir.path(),
        )
        .await
        .expect("Pull should succeed");

        // Verify directory name has hash suffix
        let path = PathBuf::from(&result);
        let dir_name = path.file_name().unwrap().to_str().unwrap();

        // Should be in format: test-alpine-XXXXXXXX (8 char hash)
        assert!(
            dir_name.starts_with("test-alpine-"),
            "Should have container name prefix"
        );

        let hash_part = dir_name.strip_prefix("test-alpine-").unwrap();
        assert_eq!(hash_part.len(), 8, "Hash should be 8 characters");
        assert!(
            hash_part.chars().all(|c| c.is_ascii_hexdigit()),
            "Hash should be hexadecimal"
        );

        // Verify hash is based on tarball content
        // (Pulling same tag multiple times might get different content if image updated,
        // which is exactly why we hash - to detect changes!)
        let tarball_path = path.join("linux-x64.oci.tar");
        assert!(tarball_path.exists(), "Tarball should exist");
    }

    #[tokio::test]
    async fn finalize_artifact_dir_reuses_existing_final_directory() {
        let temp_root = tempdir().unwrap();
        let temp_dir = temp_root.path().join(".agent-tmp-1234");
        let final_dir = temp_root.path().join("agent-abcdef12");

        fs::create_dir_all(&temp_dir).await.unwrap();
        fs::write(temp_dir.join("linux-x64.oci.tar"), b"new-build")
            .await
            .unwrap();

        fs::create_dir_all(&final_dir).await.unwrap();
        fs::write(final_dir.join("linux-x64.oci.tar"), b"existing-build")
            .await
            .unwrap();

        let resolved = finalize_artifact_dir(&temp_dir, &final_dir, "build")
            .await
            .unwrap();

        assert_eq!(resolved, final_dir.to_string_lossy());
        assert!(final_dir.exists());
        assert!(!temp_dir.exists());
        assert_eq!(
            fs::read(final_dir.join("linux-x64.oci.tar")).await.unwrap(),
            b"existing-build"
        );
    }

    #[test]
    fn temp_artifact_dir_is_hidden_and_unique() {
        let build_output_dir = PathBuf::from("/tmp/build-output");

        let first = temp_artifact_dir(&build_output_dir, "agent");
        let second = temp_artifact_dir(&build_output_dir, "agent");

        assert_ne!(first, second);
        assert_eq!(first.parent().unwrap(), build_output_dir.as_path());
        assert!(first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".agent-tmp-"));
        assert!(second
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".agent-tmp-"));
    }
}
