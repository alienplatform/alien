pub(crate) mod command_output;
pub mod dependencies;
pub mod error;
pub mod merge;
pub mod plan;
pub mod settings;
pub mod toolchain;

use alien_core::{
    alien_event, AlienEvent, BinaryTarget, Container, ContainerCode, Daemon, DaemonCode, Platform,
    Stack, ToolchainConfig, Worker, WorkerCode,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_preflights::runner::PreflightRunner;
use command_output::image_build_error_with_output;
use dockdash::{Image as DockDashImage, Layer as DockDashLayer, PullPolicy};
use error::{DockdashResultExt, ErrorData, Result};
use oci_client::client::{Client as OciClient, ClientConfig as OciClientConfig};
use oci_client::manifest::{
    ImageIndexEntry, OciImageIndex, Platform as OciPlatform, IMAGE_MANIFEST_MEDIA_TYPE,
    OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE,
};
use oci_client::Reference;
use rand::distr::Alphanumeric;
use rand::Rng;
use reqwest::Url;
use settings::{BinaryTargetExt, BuildSettings, PlatformBuildSettings, PushSettings};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::error::Error as StdError;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::process::Command;
use tokio::time::sleep;

use tracing::{info, warn};

const BASE_IMAGE_BUILD_MAX_ATTEMPTS: usize = 3;
const ARTIFACT_CACHE_METADATA_FILE: &str = ".alien-build-cache.json";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ArtifactCacheMetadata {
    cache_key: String,
}

#[derive(Debug, serde::Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoMetadataPackage>,
    resolve: Option<CargoMetadataResolve>,
    workspace_root: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
struct CargoMetadataPackage {
    id: String,
    manifest_path: PathBuf,
    source: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct CargoMetadataResolve {
    root: Option<String>,
    nodes: Vec<CargoMetadataNode>,
}

#[derive(Debug, serde::Deserialize)]
struct CargoMetadataNode {
    id: String,
    dependencies: Vec<String>,
}

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

/// A compute resource that has a locally-built image directory and needs to be pushed to a registry.
struct ResourcePushTarget {
    /// Stack resource keys that should be updated with the pushed image URI.
    resource_ids: Vec<String>,
    /// Resource IDs sharing this push target. The first name is used for logging and image tagging.
    resource_names: Vec<String>,
    /// Display name for events/logging ("worker", "container", etc.)
    resource_type: &'static str,
    /// Local directory containing OCI tarballs produced by `alien build`
    local_image_dir: PathBuf,
}

impl ResourcePushTarget {
    fn resource_name(&self) -> &str {
        self.resource_names
            .first()
            .expect("push target should have at least one resource name")
    }

    fn display_resource_name(&self) -> String {
        if self.resource_names.len() > 1 {
            format!("{} (shared)", self.resource_names.join(", "))
        } else {
            self.resource_name().to_string()
        }
    }

    fn push_result_updates(&self, image_uri: String) -> Vec<(String, String)> {
        self.resource_ids
            .iter()
            .map(|resource_id| (resource_id.clone(), image_uri.clone()))
            .collect()
    }
}

fn push_target_for_local_image<'a>(
    targets: &'a mut Vec<ResourcePushTarget>,
    resource_type: &'static str,
    local_image_dir: &Path,
) -> Option<&'a mut ResourcePushTarget> {
    targets.iter_mut().find(|target| {
        target.resource_type == resource_type && target.local_image_dir == local_image_dir
    })
}

fn add_push_target_resource(
    targets: &mut Vec<ResourcePushTarget>,
    resource_id: String,
    resource_name: String,
    resource_type: &'static str,
    local_image_dir: PathBuf,
) {
    if let Some(target) = push_target_for_local_image(targets, resource_type, &local_image_dir) {
        target.resource_ids.push(resource_id);
        target.resource_names.push(resource_name);
        return;
    }

    targets.push(ResourcePushTarget {
        resource_ids: vec![resource_id],
        resource_names: vec![resource_name],
        resource_type,
        local_image_dir,
    });
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
        if let Some(func) = resource_entry.config.downcast_ref::<Worker>() {
            match &func.code {
                WorkerCode::Image { image } => {
                    let path = PathBuf::from(image);
                    if path.exists() && path.is_dir() {
                        info!(
                            "Worker '{}' has local image directory, queuing for push",
                            func.id
                        );
                        add_push_target_resource(
                            &mut targets,
                            resource_id.clone(),
                            func.id.clone(),
                            "worker",
                            path,
                        );
                    } else {
                        info!("Worker '{}' already has remote image: {}", func.id, image);
                    }
                }
                WorkerCode::Source { .. } => {
                    return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                        resource_id: func.id.clone(),
                        reason: "Worker has source code instead of built image. Run 'alien build' first.".to_string(),
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
                        add_push_target_resource(
                            &mut targets,
                            resource_id.clone(),
                            container.id.clone(),
                            "container",
                            path,
                        );
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
        } else if let Some(daemon) = resource_entry.config.downcast_ref::<Daemon>() {
            match &daemon.code {
                DaemonCode::Image { image } => {
                    let path = PathBuf::from(image);
                    if path.exists() && path.is_dir() {
                        info!(
                            "Daemon '{}' has local image directory, queuing for push",
                            daemon.id
                        );
                        add_push_target_resource(
                            &mut targets,
                            resource_id.clone(),
                            daemon.id.clone(),
                            "daemon",
                            path,
                        );
                    } else {
                        info!("Daemon '{}' already has remote image: {}", daemon.id, image);
                    }
                }
                DaemonCode::Source { .. } => {
                    return Err(AlienError::new(ErrorData::InvalidResourceConfig {
                        resource_id: daemon.id.clone(),
                        reason: "Daemon has source code instead of built image. Run 'alien build' first.".to_string(),
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
            if let Some(func) = resource_entry.1.config.downcast_mut::<Worker>() {
                func.code = WorkerCode::Image { image: image_uri };
            } else if let Some(container) = resource_entry.1.config.downcast_mut::<Container>() {
                container.code = ContainerCode::Image { image: image_uri };
            } else if let Some(daemon) = resource_entry.1.config.downcast_mut::<Daemon>() {
                daemon.code = DaemonCode::Image { image: image_uri };
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
    destination: push_settings.destination_label.clone(),
})]
pub async fn push_stack(
    mut stack: Stack,
    platform: Platform,
    push_settings: &PushSettings,
) -> Result<Stack> {
    let push_started = Instant::now();
    info!(
        "Starting image push process to registry: {}",
        push_settings.repository
    );

    let to_push = collect_push_targets(&stack)?;

    let resource_count = to_push
        .iter()
        .map(|target| target.resource_ids.len())
        .sum::<usize>();

    info!(
        "Pushing {} artifact(s) for {} resource(s) to registry",
        to_push.len(),
        resource_count
    );

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
            let resource_name = target.resource_name().to_string();
            let display_resource_name = target.display_resource_name();
            let resource_names = target.resource_names.clone();
            let repository = push_settings.repository.clone();
            let push_opts = push_settings.options.clone();
            let bus = current_bus.clone();
            let cancel_token = cancel_token.clone();

            tokio::spawn(async move {
                let resource_name_for_warning = resource_name.clone();

                let push_work = async move {
                    let target_resource_count = target.resource_ids.len();

                    if resource_names.len() > 1 {
                        info!(
                            "Starting parallel push for shared {} artifact '{}': {:?}",
                            target.resource_type, resource_name, resource_names
                        );
                    } else {
                        info!(
                            "Starting parallel push for {} '{}'",
                            target.resource_type, resource_name
                        );
                    }

                    if cancel_token.is_cancelled() {
                        return Err(AlienError::new(ErrorData::BuildCanceled {
                            resource_name: resource_name.clone(),
                        }));
                    }

                    let artifact_push_started = Instant::now();
                    let result = tokio::select! {
                        result = push_resource_images(
                            &display_resource_name,
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
                        Ok(image_uri) => info!(
                            resource = resource_name.as_str(),
                            push_secs = format!("{:.2}", artifact_push_started.elapsed().as_secs_f64()).as_str(),
                            "Successfully pushed {} '{}' in {:.2}s to: {}",
                            target.resource_type,
                            resource_name,
                            artifact_push_started.elapsed().as_secs_f64(),
                            image_uri
                        ),
                        Err(e) => info!("Failed to push {} '{}': {}", target.resource_type, resource_name, e),
                    }

                    result.map(|image_uri| {
                        (
                            target_resource_count,
                            target.push_result_updates(image_uri),
                        )
                    })
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
            Ok(push_result) => match push_result {
                Ok((target_resource_count, updates)) => {
                    push_results.extend(updates);
                    completed_tasks += 1;
                    info!(
                        "Applied pushed image to {} resource(s)",
                        target_resource_count
                    );
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
        "Completed parallel pushing of {} artifact(s)",
        completed_tasks
    );

    apply_pushed_images(&mut stack, push_results);

    info!(
        "Image push process completed in {:.2}s. Stack updated with {} resource image URL(s).",
        push_started.elapsed().as_secs_f64(),
        resource_count
    );

    Ok(stack)
}

/// Push all OCI tarballs for a resource to the registry
#[alien_event(AlienEvent::PushingResource {
    resource_name: display_resource_name.to_string(),
    resource_type: resource_type.to_string(),
})]
async fn push_resource_images(
    display_resource_name: &str,
    resource_name: &str,
    resource_type: &str,
    images_dir: &Path,
    repository: &str,
    push_options: &dockdash::PushOptions,
) -> Result<String> {
    let push_resource_started = Instant::now();
    info!(
        "Pushing images for resource '{}' from {}",
        display_resource_name,
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

    // Container images are linux; darwin/windows tarballs (produced for `local` host
    // binaries) are not registry container images, so they're excluded from the push.
    let linux_tarballs = select_linux_tarballs(&oci_files);

    // No linux image (unusual) — push whatever tarballs are present.
    if linux_tarballs.is_empty() {
        for oci_file in &oci_files {
            let image = DockDashImage::from_tarball(oci_file).map_dockdash_err()?;
            image
                .push(&image_uri, &push_opts_with_progress)
                .await
                .map_dockdash_err()?;
        }
        info!(
            "Pushed resource '{}' in {:.2}s",
            display_resource_name,
            push_resource_started.elapsed().as_secs_f64()
        );

        return Ok(image_uri);
    }

    // Single arch: push the image straight to the tag — no index needed.
    if let [(_, only)] = linux_tarballs.as_slice() {
        info!("Pushing {} to {}", only.display(), image_uri);
        let image = DockDashImage::from_tarball(only).map_dockdash_err()?;
        image
            .push(&image_uri, &push_opts_with_progress)
            .await
            .map_dockdash_err()?;
        info!(
            "Pushed resource '{}' in {:.2}s",
            display_resource_name,
            push_resource_started.elapsed().as_secs_f64()
        );

        return Ok(image_uri);
    }

    // Multi-arch: push each image to a per-arch child tag, then publish a manifest list
    // (OCI image index) at the shared tag — otherwise the last single-arch push wins the tag.
    let oci_client = OciClient::new(OciClientConfig {
        protocol: push_options.protocol.clone(),
        ..Default::default()
    });
    let mut entries = Vec::new();
    for (target, oci_file) in &linux_tarballs {
        let child_uri = format!("{}-{}", image_uri, target.runtime_platform_id());
        info!("Pushing {} as {}", oci_file.display(), child_uri);
        let image = DockDashImage::from_tarball(oci_file).map_dockdash_err()?;
        image
            .push(&child_uri, &push_opts_with_progress)
            .await
            .map_dockdash_err()?;

        // The index entry's digest+size must reflect the manifest the registry stored
        // (dockdash pushes a converted manifest), so read it back rather than hashing the tarball.
        let child_ref = Reference::try_from(child_uri.as_str())
            .into_alien_error()
            .context(ErrorData::InvalidResourceConfig {
                resource_id: resource_name.to_string(),
                reason: format!("Invalid image reference '{child_uri}'"),
            })?;
        let (manifest_bytes, digest) = oci_client
            .pull_manifest_raw(
                &child_ref,
                &push_options.auth,
                &[OCI_IMAGE_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE],
            )
            .await
            .into_alien_error()
            .context(ErrorData::ImagePushFailed {
                image: child_uri.clone(),
                reason: "Failed to read back the pushed manifest".to_string(),
            })?;
        let media_type = manifest_media_type(&manifest_bytes)
            .unwrap_or_else(|| OCI_IMAGE_MEDIA_TYPE.to_string());
        entries.push(image_index_entry(
            *target,
            digest,
            manifest_bytes.len() as i64,
            media_type,
        ));
    }

    let index = assemble_image_index(entries);
    let index_ref = Reference::try_from(image_uri.as_str())
        .into_alien_error()
        .context(ErrorData::InvalidResourceConfig {
            resource_id: resource_name.to_string(),
            reason: format!("Invalid image reference '{image_uri}'"),
        })?;
    oci_client
        .push_manifest_list(&index_ref, &push_options.auth, index)
        .await
        .into_alien_error()
        .context(ErrorData::ImagePushFailed {
            image: image_uri.clone(),
            reason: "Failed to push the multi-arch manifest list".to_string(),
        })?;
    info!(
        "Pushed multi-arch image {} ({} arches)",
        image_uri,
        linux_tarballs.len()
    );

    info!(
        "Pushed resource '{}' in {:.2}s",
        display_resource_name,
        push_resource_started.elapsed().as_secs_f64()
    );

    Ok(image_uri)
}

/// Pair each linux OCI tarball with its target, sorted for deterministic ordering.
/// darwin/windows tarballs are excluded — they are host binaries, not container images.
fn select_linux_tarballs(oci_files: &[PathBuf]) -> Vec<(BinaryTarget, PathBuf)> {
    let mut out: Vec<(BinaryTarget, PathBuf)> = oci_files
        .iter()
        .filter_map(|path| oci_tarball_target(path).map(|target| (target, path.clone())))
        .filter(|(target, _)| target.oci_os() == "linux")
        .collect();
    out.sort_by_key(|(target, _)| target.runtime_platform_id());
    out
}

/// `<runtime_platform_id>.oci.tar` → its `BinaryTarget`.
fn oci_tarball_target(path: &Path) -> Option<BinaryTarget> {
    let name = path.file_name()?.to_str()?;
    BinaryTarget::from_runtime_platform_id(name.strip_suffix(".oci.tar")?)
}

/// One OCI image index entry for a built target.
fn image_index_entry(
    target: BinaryTarget,
    digest: String,
    size: i64,
    media_type: String,
) -> ImageIndexEntry {
    ImageIndexEntry {
        media_type,
        digest,
        size,
        platform: Some(OciPlatform {
            architecture: target.oci_arch().to_string(),
            os: target.oci_os().to_string(),
            os_version: None,
            os_features: None,
            variant: None,
            features: None,
        }),
        annotations: None,
    }
}

/// Wrap per-arch manifest entries in an OCI image index (manifest list).
fn assemble_image_index(manifests: Vec<ImageIndexEntry>) -> OciImageIndex {
    OciImageIndex {
        schema_version: 2,
        media_type: Some(OCI_IMAGE_INDEX_MEDIA_TYPE.to_string()),
        manifests,
        artifact_type: None,
        annotations: None,
    }
}

/// Read the `mediaType` field from a raw manifest, if present.
fn manifest_media_type(bytes: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()?
        .get("mediaType")?
        .as_str()
        .map(|s| s.to_string())
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

async fn compute_source_artifact_cache_key(
    src: &str,
    toolchain_config: &alien_core::ToolchainConfig,
    settings: &BuildSettings,
    targets: &[BinaryTarget],
    workload: toolchain::WorkloadKind,
) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"alien-build-artifact-cache-v3");
    hasher.update(src.as_bytes());
    hasher.update(
        serde_json::to_vec(toolchain_config)
            .into_alien_error()
            .context(ErrorData::JsonSerializationError {
                message: "Failed to serialize toolchain config for build cache key".to_string(),
            })?,
    );
    // Source artifact bytes are platform-independent for equivalent target sets.
    // The actual differences are target triples, debug/release mode, workload
    // kind (which decides the image shape: runtime for Workers, direct
    // entrypoint for Containers/Daemons), base image, and whether the built
    // binary runs as a host process. This lets e.g. GCP and Azure reuse the
    // same linux-x64 artifacts.
    let host_process = workload != toolchain::WorkloadKind::Container
        && settings.platform.runtime_platform() == Platform::Local;
    hasher.update(host_process.to_string().as_bytes());
    hasher.update(settings.debug_mode.to_string().as_bytes());
    hasher.update(workload.as_str().as_bytes());
    for base_image in
        effective_source_base_images(toolchain_config, settings, workload, host_process)
    {
        hasher.update(b"\0base-image\0");
        hasher.update(base_image.as_bytes());
    }
    for target in targets {
        hasher.update(target.runtime_platform_id().as_bytes());
    }

    hash_build_input_source(src, toolchain_config, targets, &mut hasher).await?;

    Ok(format!("{:x}", hasher.finalize()))
}

async fn hash_build_input_source(
    src: &str,
    toolchain_config: &alien_core::ToolchainConfig,
    targets: &[BinaryTarget],
    hasher: &mut Sha256,
) -> Result<()> {
    match toolchain_config {
        ToolchainConfig::Rust { .. } => hash_rust_build_input_graph(Path::new(src), hasher).await,
        ToolchainConfig::TypeScript { .. } => {
            hash_source_directory(Path::new(src), hasher).await?;
            hash_typescript_dependency_inputs(Path::new(src), targets, hasher).await
        }
        _ => hash_source_directory(Path::new(src), hasher).await,
    }
}

/// Hash the build inputs a TypeScript app pulls in from OUTSIDE its source
/// directory, which `hash_source_directory` cannot see (`node_modules` is
/// excluded from the source walk):
///
/// - the `dist/` content of every `@alienplatform/*` package the app has
///   installed, resolved through its symlink to the real location — a
///   `workspace:`/`file:` dependency changes content without changing any
///   version number, and the compiled binary bundles that content;
/// - the workspace dev addon files for the requested targets — the compiled
///   binary embeds the addon, so a rebuilt addon must invalidate the cached
///   artifact.
///
/// Registry-installed dependencies are content-addressed by the lockfile,
/// which lives in the source directory and is already hashed.
async fn hash_typescript_dependency_inputs(
    src_dir: &Path,
    targets: &[BinaryTarget],
    hasher: &mut Sha256,
) -> Result<()> {
    let scope_dir = src_dir.join("node_modules").join("@alienplatform");
    let Ok(entries) = std::fs::read_dir(&scope_dir) else {
        // No workspace packages installed — nothing extra to hash.
        return Ok(());
    };

    let mut packages: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    packages.sort();

    let mut bindings_realpath: Option<PathBuf> = None;
    for package_dir in packages {
        let Ok(realpath) = package_dir.canonicalize() else {
            continue;
        };
        if package_dir.file_name().is_some_and(|n| n == "bindings") {
            bindings_realpath = Some(realpath.clone());
        }
        hasher.update(b"alienplatform-package");
        hasher.update(package_dir.to_string_lossy().as_bytes());
        let dist = realpath.join("dist");
        if dist.is_dir() {
            hash_source_directory(&dist, hasher).await?;
        }
    }

    // Workspace dev addons for the requested targets, anchored on the real
    // bindings package location (mirrors the staging lookup's anchor).
    if let Some(bindings) = bindings_realpath {
        for addon in toolchain::native_addon::workspace_addon_inputs(&bindings, targets) {
            hasher.update(b"native-addon");
            hasher.update(addon.to_string_lossy().as_bytes());
            let bytes = fs::read(&addon).await.into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "read file".to_string(),
                    file_path: addon.display().to_string(),
                    reason: "Failed to read native addon for build cache key".to_string(),
                },
            )?;
            hasher.update(&bytes);
        }
    }

    Ok(())
}

async fn hash_rust_build_input_graph(src_dir: &Path, hasher: &mut Sha256) -> Result<()> {
    let metadata = read_cargo_metadata(src_dir).await?;
    hasher.update(b"rust-cargo-metadata-v1");

    let lockfile = metadata.workspace_root.join("Cargo.lock");
    if lockfile.is_file() {
        hasher.update(b"cargo-lock");
        let lockfile_bytes = fs::read(&lockfile).await.into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "read file".to_string(),
                file_path: lockfile.display().to_string(),
                reason: "Failed to read Cargo.lock for build cache key".to_string(),
            },
        )?;
        hasher.update(lockfile_bytes);
    }

    // Workspace-level toolchain configuration changes the produced binary even
    // when no package source changes: rust-toolchain pins the compiler,
    // .cargo/config.toml can change rustflags, linker, or profile settings.
    // These live at the workspace root, outside the per-package directories
    // hashed below, so hash them explicitly. Absent files contribute nothing,
    // which keeps existing cache keys stable for projects without them.
    for toolchain_file in [
        "rust-toolchain.toml",
        "rust-toolchain",
        ".cargo/config.toml",
        ".cargo/config",
    ] {
        let path = metadata.workspace_root.join(toolchain_file);
        if path.is_file() {
            let contents = fs::read(&path).await.into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "read file".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to read toolchain config file for build cache key".to_string(),
                },
            )?;
            hasher.update(b"toolchain-config-file");
            hasher.update(toolchain_file.as_bytes());
            hasher.update(contents);
        }
    }

    let local_package_ids = local_cargo_package_ids(&metadata);
    let mut local_packages: Vec<_> = metadata
        .packages
        .iter()
        .filter(|package| local_package_ids.contains(&package.id))
        .collect();
    local_packages.sort_by(|left, right| left.id.cmp(&right.id));

    for package in local_packages {
        hasher.update(b"local-package");
        hasher.update(package.id.as_bytes());
        hasher.update(package.manifest_path.to_string_lossy().as_bytes());
        let package_dir = package.manifest_path.parent().ok_or_else(|| {
            AlienError::new(ErrorData::BuildConfigInvalid {
                message: format!(
                    "Cargo metadata package '{}' has manifest path without parent: {}",
                    package.id,
                    package.manifest_path.display()
                ),
            })
        })?;
        hash_source_directory(package_dir, hasher).await?;
    }

    Ok(())
}

async fn read_cargo_metadata(src_dir: &Path) -> Result<CargoMetadata> {
    let manifest_path = src_dir.join("Cargo.toml");
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(&manifest_path)
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::ImageBuildFailed {
            resource_name: src_dir.display().to_string(),
            reason: "Failed to execute cargo metadata for build cache key".to_string(),
            build_output: None,
        })?;

    if !output.status.success() {
        let mut build_output = String::new();
        build_output.push_str(&String::from_utf8_lossy(&output.stdout));
        build_output.push_str(&String::from_utf8_lossy(&output.stderr));
        return Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: src_dir.display().to_string(),
            reason: "cargo metadata failed while computing build cache key".to_string(),
            build_output: Some(build_output),
        }));
    }

    serde_json::from_slice(&output.stdout)
        .into_alien_error()
        .context(ErrorData::JsonSerializationError {
            message: "Failed to parse cargo metadata JSON for build cache key".to_string(),
        })
}

fn local_cargo_package_ids(metadata: &CargoMetadata) -> HashSet<String> {
    let packages_by_id: HashMap<_, _> = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect();

    let Some(resolve) = &metadata.resolve else {
        return metadata
            .packages
            .iter()
            .filter(|package| package.source.is_none())
            .map(|package| package.id.clone())
            .collect();
    };
    let Some(root) = &resolve.root else {
        return metadata
            .packages
            .iter()
            .filter(|package| package.source.is_none())
            .map(|package| package.id.clone())
            .collect();
    };

    let nodes_by_id: HashMap<_, _> = resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut visited = HashSet::new();
    let mut stack = vec![root.as_str()];

    while let Some(id) = stack.pop() {
        if !visited.insert(id.to_string()) {
            continue;
        }

        let Some(node) = nodes_by_id.get(id) else {
            continue;
        };

        for dependency in &node.dependencies {
            stack.push(dependency);
        }
    }

    visited
        .into_iter()
        .filter(|id| {
            packages_by_id
                .get(id.as_str())
                .map(|package| package.source.is_none())
                .unwrap_or(false)
        })
        .collect()
}

async fn hash_source_directory(src_dir: &Path, hasher: &mut Sha256) -> Result<()> {
    let mut files = Vec::new();
    collect_source_files(src_dir, src_dir, &mut files)?;
    files.sort();

    for relative_path in files {
        let full_path = src_dir.join(&relative_path);
        let contents = fs::read(&full_path).await.into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "read file".to_string(),
                file_path: full_path.display().to_string(),
                reason: "Failed to read source file for build cache key".to_string(),
            },
        )?;
        hasher.update(relative_path.to_string_lossy().as_bytes());
        hasher.update(&contents);
    }

    Ok(())
}

fn collect_source_files(base_dir: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory".to_string(),
                file_path: dir.display().to_string(),
                reason: "Failed to read source directory for build cache key".to_string(),
            })?;

    for entry in entries {
        let entry = entry
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory entry".to_string(),
                file_path: dir.display().to_string(),
                reason: "Failed to iterate source directory for build cache key".to_string(),
            })?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if is_ignored_source_cache_path(file_name.as_ref()) {
            continue;
        }

        let file_type =
            entry
                .file_type()
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "read metadata".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to read source file type for build cache key".to_string(),
                })?;

        if file_type.is_dir() {
            collect_source_files(base_dir, &path, files)?;
        } else if file_type.is_file() {
            let relative_path = path.strip_prefix(base_dir).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "strip prefix".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to compute relative source path for build cache key"
                        .to_string(),
                },
            )?;
            files.push(relative_path.to_path_buf());
        }
    }

    Ok(())
}

fn is_ignored_source_cache_path(file_name: &str) -> bool {
    matches!(
        file_name,
        ".git" | ".alien" | ".alien-build" | "target" | "node_modules" | "alien-bindings.node" // staged addon: derived artifact, hashed via its source
    ) || file_name.ends_with(".bun-build")
}

async fn find_cached_artifact_dir(
    build_output_dir: &Path,
    resource_name: &str,
    targets: &[BinaryTarget],
    artifact_cache_key: &str,
) -> Result<Option<PathBuf>> {
    if let Some(path) =
        find_cached_artifact_dir_in(build_output_dir, resource_name, targets, artifact_cache_key)
            .await?
    {
        return Ok(Some(path));
    }

    let Some(parent_dir) = build_output_dir.parent() else {
        return Ok(None);
    };

    let mut platform_entries = fs::read_dir(parent_dir).await.into_alien_error().context(
        ErrorData::FileOperationFailed {
            operation: "read directory".to_string(),
            file_path: parent_dir.display().to_string(),
            reason: "Failed to read sibling build directories for artifact cache".to_string(),
        },
    )?;

    while let Some(entry) = platform_entries
        .next_entry()
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read directory entry".to_string(),
            file_path: parent_dir.display().to_string(),
            reason: "Failed to iterate sibling build directories for artifact cache".to_string(),
        })?
    {
        let path = entry.path();
        if path == build_output_dir {
            continue;
        }

        let file_type =
            entry
                .file_type()
                .await
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "read metadata".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to read sibling artifact cache directory metadata".to_string(),
                })?;
        if !file_type.is_dir() {
            continue;
        }

        if let Some(cached) =
            find_cached_artifact_dir_in(&path, resource_name, targets, artifact_cache_key).await?
        {
            return Ok(Some(cached));
        }
    }

    Ok(None)
}

async fn find_cached_artifact_dir_in(
    build_output_dir: &Path,
    resource_name: &str,
    targets: &[BinaryTarget],
    artifact_cache_key: &str,
) -> Result<Option<PathBuf>> {
    let mut entries = fs::read_dir(build_output_dir)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read directory".to_string(),
            file_path: build_output_dir.display().to_string(),
            reason: "Failed to read build output directory for artifact cache".to_string(),
        })?;

    let prefix = format!("{resource_name}-");
    while let Some(entry) =
        entries
            .next_entry()
            .await
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory entry".to_string(),
                file_path: build_output_dir.display().to_string(),
                reason: "Failed to iterate build output directory for artifact cache".to_string(),
            })?
    {
        let path = entry.path();
        let file_type =
            entry
                .file_type()
                .await
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "read metadata".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to read artifact cache entry metadata".to_string(),
                })?;
        if !file_type.is_dir() {
            continue;
        }

        let Some(dir_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !dir_name.starts_with(&prefix) {
            continue;
        }

        let has_all_targets = targets.iter().all(|target| {
            path.join(format!("{}.oci.tar", target.runtime_platform_id()))
                .is_file()
        });
        if !has_all_targets {
            continue;
        }

        let Ok(metadata_content) =
            fs::read_to_string(path.join(ARTIFACT_CACHE_METADATA_FILE)).await
        else {
            continue;
        };
        let Ok(metadata) = serde_json::from_str::<ArtifactCacheMetadata>(&metadata_content) else {
            continue;
        };
        if metadata.cache_key == artifact_cache_key {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

async fn write_artifact_cache_metadata(
    artifact_dir: &Path,
    artifact_cache_key: &str,
) -> Result<()> {
    let metadata = ArtifactCacheMetadata {
        cache_key: artifact_cache_key.to_string(),
    };
    let content = serde_json::to_string_pretty(&metadata)
        .into_alien_error()
        .context(ErrorData::JsonSerializationError {
            message: "Failed to serialize build artifact cache metadata".to_string(),
        })?;

    fs::write(artifact_dir.join(ARTIFACT_CACHE_METADATA_FILE), content)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: artifact_dir
                .join(ARTIFACT_CACHE_METADATA_FILE)
                .display()
                .to_string(),
            reason: "Failed to write build artifact cache metadata".to_string(),
        })?;

    Ok(())
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

/// Return the ordered base-image inputs that affect a source artifact.
/// Host-process and Dockerfile builds do not use the source toolchain bases.
fn effective_source_base_images(
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
fn base_images_for_workload(
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
fn image_entrypoint_and_cmd(
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
fn apply_image_command(
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

fn base_image_build_retry_delay(attempt: usize) -> Duration {
    match attempt {
        1 => Duration::from_secs(2),
        2 => Duration::from_secs(5),
        _ => Duration::from_secs(10),
    }
}

fn is_retryable_dockdash_image_pull_error(error: &dockdash::Error) -> bool {
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

    fn toolchain_output(
        entrypoint: Option<Vec<String>>,
        runtime_command: Vec<String>,
    ) -> toolchain::ToolchainOutput {
        toolchain::ToolchainOutput {
            build_strategy: toolchain::ImageBuildStrategy::FromScratch { layers: vec![] },
            entrypoint,
            runtime_command,
        }
    }

    /// Pins the ENTRYPOINT/CMD contract shared by the base-image and
    /// from-scratch build paths (see also tests/image_shape_tests.rs).
    #[test]
    fn image_entrypoint_and_cmd_contract() {
        // Worker: base entrypoint kept, CMD is the separator + binary.
        let worker = toolchain_output(None, vec!["--".to_string(), "./bin".to_string()]);
        assert_eq!(
            image_entrypoint_and_cmd(&worker),
            (None, Some(vec!["--".to_string(), "./bin".to_string()]))
        );

        // Direct entrypoint (Container/Daemon): binary is the entrypoint, no CMD.
        let direct = toolchain_output(Some(vec!["/app/bin".to_string()]), vec![]);
        assert_eq!(
            image_entrypoint_and_cmd(&direct),
            (Some(vec!["/app/bin".to_string()]), None)
        );

        // Local from-scratch (host process): no entrypoint, CMD is the binary.
        let local = toolchain_output(None, vec!["./bin".to_string()]);
        assert_eq!(
            image_entrypoint_and_cmd(&local),
            (None, Some(vec!["./bin".to_string()]))
        );

        // Explicit entrypoint with a nonempty command keeps both.
        let both = toolchain_output(
            Some(vec!["/app/bin".to_string()]),
            vec!["serve".to_string()],
        );
        assert_eq!(
            image_entrypoint_and_cmd(&both),
            (
                Some(vec!["/app/bin".to_string()]),
                Some(vec!["serve".to_string()])
            )
        );
    }

    #[test]
    fn runtime_base_override_only_applies_to_workers() {
        let direct_bases = vec!["cgr.dev/chainguard/wolfi-base:latest".to_string()];
        let runtime_base = "registry.example.com/alien-base:feature";

        assert_eq!(
            base_images_for_workload(&direct_bases, None, toolchain::WorkloadKind::Worker),
            direct_bases,
            "without an override the declared default bases must be preserved"
        );
        assert_eq!(
            base_images_for_workload(
                &direct_bases,
                Some(runtime_base),
                toolchain::WorkloadKind::Worker,
            ),
            vec![runtime_base.to_string()]
        );
        for workload in [
            toolchain::WorkloadKind::Container,
            toolchain::WorkloadKind::Daemon,
        ] {
            assert_eq!(
                base_images_for_workload(&direct_bases, Some(runtime_base), workload),
                direct_bases,
                "{} must not inherit the Worker runtime base",
                workload.as_str()
            );
        }
    }

    #[test]
    fn requested_host_binary_only_gates_container_skip() {
        use BinaryTarget::*;
        // None (defaults to host OS) and empty → containers still build.
        assert!(!requested_host_binary_only(None));
        assert!(!requested_host_binary_only(Some(&[])));
        // Explicit non-Linux-only → nothing for a container to build, skip it.
        assert!(requested_host_binary_only(Some(&[DarwinArm64])));
        assert!(requested_host_binary_only(Some(&[WindowsX64])));
        assert!(requested_host_binary_only(Some(&[DarwinArm64, WindowsX64])));
        // Any Linux target present → containers build for it.
        assert!(!requested_host_binary_only(Some(&[LinuxArm64])));
        assert!(!requested_host_binary_only(Some(&[LinuxX64])));
        assert!(!requested_host_binary_only(Some(&[
            DarwinArm64,
            LinuxArm64
        ])));
    }

    #[test]
    fn local_build_strips_daemon_only_compute_cluster() {
        let cluster = alien_core::ComputeCluster::new("host-runtime".to_string())
            .capacity_group(alien_core::CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m8i.xlarge".to_string()),
                profile: None,
                min_size: 1,
                max_size: 1,
                scale_policy: None,
                nested_virtualization: Some(true),
            })
            .build();
        let daemon = Daemon::new("host-loader".to_string())
            .cluster("host-runtime".to_string())
            .permissions("loader".to_string())
            .code(DaemonCode::Image {
                image: "registry.example.com/host-loader:latest".to_string(),
            })
            .build();
        let mut stack = Stack::new("host-loader-stack".to_string())
            .add(cluster, alien_core::ResourceLifecycle::Frozen)
            .add(daemon, alien_core::ResourceLifecycle::Live)
            .build();

        strip_local_daemon_only_compute_clusters(&mut stack, Platform::Local);

        assert!(!stack.resources.contains_key("host-runtime"));
        let daemon = stack
            .resources()
            .find(|(id, _)| *id == "host-loader")
            .and_then(|(_, entry)| entry.config.downcast_ref::<Daemon>())
            .expect("daemon should remain");
        assert_eq!(daemon.cluster, None);
    }

    #[tokio::test]
    async fn machines_build_rejects_workers_before_writing_artifacts() {
        let output = tempdir().unwrap();
        let worker = Worker::new("job".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "registry.example.com/job:latest".to_string(),
            })
            .build();
        let stack = Stack::new("machines-worker".to_string())
            .add(worker, alien_core::ResourceLifecycle::Live)
            .build();
        let settings = BuildSettings {
            output_directory: output.path().display().to_string(),
            platform: PlatformBuildSettings::Machines {},
            targets: Some(BinaryTarget::LINUX.to_vec()),
            cache_url: None,
            override_base_image: None,
            debug_mode: false,
        };

        let error = build_stack(stack, &settings)
            .await
            .expect_err("machines worker should fail build-time preflight");

        assert_eq!(error.code, "STACK_PROCESSOR_FAILED");
        let serialized = serde_json::to_string(&error).expect("error should serialize");
        assert!(serialized.contains("MACHINES_UNSUPPORTED_RESOURCE"));
        assert!(!output.path().join("build").join("machines").exists());
    }

    #[test]
    fn source_cache_hash_ignores_build_artifacts() {
        let src = tempdir().unwrap();
        std::fs::create_dir_all(src.path().join(".alien-build")).unwrap();
        std::fs::create_dir_all(src.path().join("node_modules")).unwrap();
        std::fs::write(src.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            src.path().join(".alien-build/__alien_bootstrap.ts"),
            "generated",
        )
        .unwrap();
        std::fs::write(
            src.path().join(".18ba89dff9ff58bf-00000000.bun-build"),
            "generated",
        )
        .unwrap();
        std::fs::write(src.path().join("node_modules/module.js"), "dependency").unwrap();

        let mut files = Vec::new();
        collect_source_files(src.path(), src.path(), &mut files).unwrap();
        files.sort();

        assert_eq!(files, vec![PathBuf::from("package.json")]);
    }

    fn docker_available() -> bool {
        Command::new("docker")
            .arg("info")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// True if a real OCI registry answers at `base/v2/` (200 or 401). Used to gate the
    /// multi-arch push test. Run one with: `docker run -d -p 5050:5000 registry:2`.
    async fn registry_available(base: &str) -> bool {
        match reqwest::get(format!("{base}/v2/")).await {
            Ok(resp) => resp.status().is_success() || resp.status().as_u16() == 401,
            Err(_) => false,
        }
    }

    fn test_container(name: &str, image: String) -> Container {
        Container::new(name.to_string())
            .code(ContainerCode::Image { image })
            .cpu(alien_core::ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .permissions("container-execution".to_string())
            .build()
    }

    #[test]
    fn retryable_image_pull_detects_oci_server_errors() {
        let error = dockdash::Error::ImagePull {
            image_ref: "ghcr.io/example/base:tag".to_string(),
            message: "Failed to pull layer blob sha256:abc".to_string(),
            source: Some(Box::new(
                oci_client::errors::OciDistributionError::ServerError {
                    code: 502,
                    url: "https://ghcr.io/v2/example/base/blobs/sha256:abc".to_string(),
                    message: "Bad Gateway".to_string(),
                },
            )),
        };

        assert!(is_retryable_dockdash_image_pull_error(&error));
    }

    #[test]
    fn retryable_image_pull_detects_opaque_transport_errors() {
        let error = dockdash::Error::ImagePull {
            image_ref: "ghcr.io/example/base:tag".to_string(),
            message: "Failed to pull layer blob sha256:abc".to_string(),
            source: Some(Box::new(std::io::Error::other(
                "error sending request for url (https://ghcr.io/v2/example/base/blobs/sha256:abc): client error (SendRequest): connection error",
            ))),
        };

        assert!(is_retryable_dockdash_image_pull_error(&error));
    }

    #[test]
    fn retryable_image_pull_rejects_auth_and_not_found_errors() {
        let auth_error = dockdash::Error::ImagePull {
            image_ref: "ghcr.io/example/base:tag".to_string(),
            message: "Failed to pull layer blob sha256:abc".to_string(),
            source: Some(Box::new(
                oci_client::errors::OciDistributionError::UnauthorizedError {
                    url: "https://ghcr.io/v2/example/base/blobs/sha256:abc".to_string(),
                },
            )),
        };
        let missing_error = dockdash::Error::ImagePull {
            image_ref: "ghcr.io/example/base:tag".to_string(),
            message: "Failed to pull manifest".to_string(),
            source: Some(Box::new(
                oci_client::errors::OciDistributionError::ImageManifestNotFoundError(
                    "ghcr.io/example/base:tag".to_string(),
                ),
            )),
        };

        assert!(!is_retryable_dockdash_image_pull_error(&auth_error));
        assert!(!is_retryable_dockdash_image_pull_error(&missing_error));
    }

    #[test]
    fn oci_tarball_target_maps_runtime_platform_ids() {
        assert_eq!(
            oci_tarball_target(Path::new("/x/linux-aarch64.oci.tar")),
            Some(BinaryTarget::LinuxArm64)
        );
        assert_eq!(
            oci_tarball_target(Path::new("linux-x64.oci.tar")),
            Some(BinaryTarget::LinuxX64)
        );
        assert_eq!(oci_tarball_target(Path::new("stack.json")), None);
        assert_eq!(oci_tarball_target(Path::new("linux-arm64.oci.tar")), None); // CLI spelling, not a tarball name
    }

    #[test]
    fn select_linux_tarballs_keeps_only_linux_sorted() {
        let files = vec![
            PathBuf::from("/b/windows-x64.oci.tar"),
            PathBuf::from("/b/linux-x64.oci.tar"),
            PathBuf::from("/b/darwin-aarch64.oci.tar"),
            PathBuf::from("/b/linux-aarch64.oci.tar"),
        ];
        let selected = select_linux_tarballs(&files);
        assert_eq!(
            selected.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
            vec![BinaryTarget::LinuxArm64, BinaryTarget::LinuxX64], // sorted by runtime id: linux-aarch64 < linux-x64
        );
    }

    #[test]
    fn assemble_image_index_sets_oci_index_shape() {
        let entry = image_index_entry(
            BinaryTarget::LinuxArm64,
            "sha256:abc".to_string(),
            123,
            OCI_IMAGE_MEDIA_TYPE.to_string(),
        );
        let platform = entry.platform.as_ref().unwrap();
        assert_eq!(platform.architecture, "arm64");
        assert_eq!(platform.os, "linux");

        let index = assemble_image_index(vec![entry]);
        assert_eq!(index.schema_version, 2);
        assert_eq!(
            index.media_type.as_deref(),
            Some(OCI_IMAGE_INDEX_MEDIA_TYPE)
        );
        assert_eq!(index.manifests.len(), 1);
        assert_eq!(index.manifests[0].digest, "sha256:abc");
        assert_eq!(index.manifests[0].size, 123);
    }

    #[test]
    fn manifest_media_type_reads_field_or_none() {
        assert_eq!(
            manifest_media_type(br#"{"mediaType":"application/vnd.oci.image.manifest.v1+json"}"#),
            Some("application/vnd.oci.image.manifest.v1+json".to_string())
        );
        assert_eq!(manifest_media_type(br#"{"schemaVersion":2}"#), None);
        assert_eq!(manifest_media_type(b"not json"), None);
    }

    #[test]
    fn collect_push_targets_groups_resources_that_share_local_image_directory() {
        let temp_root = tempdir().unwrap();
        let shared_dir = temp_root.path().join("shared-image");
        let unique_dir = temp_root.path().join("unique-image");
        std::fs::create_dir_all(&shared_dir).unwrap();
        std::fs::create_dir_all(&unique_dir).unwrap();

        let shared_image = shared_dir.to_string_lossy().into_owned();
        let unique_image = unique_dir.to_string_lossy().into_owned();

        let messaging_gateway = test_container("messaging-gateway", shared_image.clone());
        let billing_worker = test_container("billing-worker", shared_image);
        let postgres = test_container("postgres", unique_image);
        let remote = test_container("remote", "registry.example.com/remote:latest".to_string());

        let mut stack = Stack::new("push-dedupe".to_string())
            .add(messaging_gateway, alien_core::ResourceLifecycle::Frozen)
            .add(billing_worker, alien_core::ResourceLifecycle::Frozen)
            .add(postgres, alien_core::ResourceLifecycle::Frozen)
            .add(remote, alien_core::ResourceLifecycle::Frozen)
            .build();

        let targets = collect_push_targets(&stack).unwrap();

        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets[0].resource_names,
            vec![
                "messaging-gateway".to_string(),
                "billing-worker".to_string()
            ]
        );
        assert_eq!(
            targets[0].resource_ids,
            vec![
                "messaging-gateway".to_string(),
                "billing-worker".to_string()
            ]
        );
        assert_eq!(targets[0].resource_type, "container");
        assert_eq!(targets[0].local_image_dir, shared_dir);
        assert_eq!(targets[1].resource_names, vec!["postgres".to_string()]);

        let mut updates = targets[0].push_result_updates("registry.example.com/shared:tag".into());
        updates.extend(targets[1].push_result_updates("registry.example.com/postgres:tag".into()));
        apply_pushed_images(&mut stack, updates);

        let images = stack
            .resources()
            .filter_map(|(id, entry)| {
                entry
                    .config
                    .downcast_ref::<Container>()
                    .and_then(|container| match &container.code {
                        ContainerCode::Image { image } => Some((id.clone(), image.clone())),
                        ContainerCode::Source { .. } => None,
                    })
            })
            .collect::<HashMap<_, _>>();

        assert_eq!(
            images.get("messaging-gateway").unwrap(),
            "registry.example.com/shared:tag"
        );
        assert_eq!(
            images.get("billing-worker").unwrap(),
            "registry.example.com/shared:tag"
        );
        assert_eq!(
            images.get("postgres").unwrap(),
            "registry.example.com/postgres:tag"
        );
        assert_eq!(
            images.get("remote").unwrap(),
            "registry.example.com/remote:latest"
        );
    }

    #[test]
    fn collect_push_targets_handles_daemons_like_other_compute() {
        let temp_root = tempdir().unwrap();
        let daemon_dir = temp_root.path().join("daemon-image");
        std::fs::create_dir_all(&daemon_dir).unwrap();

        let local_daemon = Daemon::new("agent".to_string())
            .permissions("execution".to_string())
            .code(DaemonCode::Image {
                image: daemon_dir.to_string_lossy().into_owned(),
            })
            .build();
        let remote_daemon = Daemon::new("collector".to_string())
            .permissions("execution".to_string())
            .code(DaemonCode::Image {
                image: "registry.example.com/collector:latest".to_string(),
            })
            .build();

        let mut stack = Stack::new("daemon-push".to_string())
            .add(local_daemon, alien_core::ResourceLifecycle::Live)
            .add(remote_daemon, alien_core::ResourceLifecycle::Live)
            .build();

        let targets = collect_push_targets(&stack).unwrap();
        assert_eq!(
            targets.len(),
            1,
            "only the local-dir daemon is queued for push"
        );
        assert_eq!(targets[0].resource_names, vec!["agent".to_string()]);
        assert_eq!(targets[0].resource_type, "daemon");
        assert_eq!(targets[0].local_image_dir, daemon_dir);

        let updates = targets[0].push_result_updates("registry.example.com/agent:tag".into());
        apply_pushed_images(&mut stack, updates);
        let agent = stack
            .resources()
            .find(|(id, _)| *id == "agent")
            .and_then(|(_, e)| e.config.downcast_ref::<Daemon>().cloned())
            .expect("agent daemon should exist");
        assert_eq!(
            agent.code,
            DaemonCode::Image {
                image: "registry.example.com/agent:tag".to_string()
            }
        );

        // An unbuilt source daemon fails fast, same as workers and containers.
        let source_daemon = Daemon::new("raw".to_string())
            .permissions("execution".to_string())
            .code(DaemonCode::Source {
                src: ".".to_string(),
                toolchain: ToolchainConfig::Rust {
                    binary_name: "raw".to_string(),
                },
            })
            .build();
        let source_stack = Stack::new("daemon-source".to_string())
            .add(source_daemon, alien_core::ResourceLifecycle::Live)
            .build();
        let error = match collect_push_targets(&source_stack) {
            Err(error) => error,
            Ok(_) => panic!("source daemon must be rejected"),
        };
        assert!(error.to_string().contains("Run 'alien build' first"));
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
    async fn source_artifact_cache_key_is_shared_for_equivalent_cloud_builds() {
        let src_dir = tempdir().unwrap();
        std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
        std::fs::write(
            src_dir.path().join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(src_dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

        let toolchain = ToolchainConfig::Rust {
            binary_name: "app".to_string(),
        };
        let targets = vec![BinaryTarget::LinuxX64];
        let gcp = BuildSettings {
            output_directory: src_dir.path().join("out").to_string_lossy().into_owned(),
            platform: PlatformBuildSettings::Gcp {},
            targets: Some(targets.clone()),
            cache_url: None,
            override_base_image: Some("registry.example.com/base:tag".to_string()),
            debug_mode: false,
        };
        let azure = BuildSettings {
            platform: PlatformBuildSettings::Azure {},
            override_base_image: Some("registry.example.com/base:other-tag".to_string()),
            ..gcp.clone()
        };

        let gcp_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &gcp,
            &targets,
            crate::toolchain::WorkloadKind::Container,
        )
        .await
        .unwrap();
        let azure_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &azure,
            &targets,
            crate::toolchain::WorkloadKind::Container,
        )
        .await
        .unwrap();

        assert_eq!(
            gcp_key, azure_key,
            "direct workloads must ignore the Worker runtime-base override"
        );
        let gcp_daemon_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &gcp,
            &targets,
            crate::toolchain::WorkloadKind::Daemon,
        )
        .await
        .unwrap();
        let azure_daemon_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &azure,
            &targets,
            crate::toolchain::WorkloadKind::Daemon,
        )
        .await
        .unwrap();
        assert_eq!(
            gcp_daemon_key, azure_daemon_key,
            "Daemon artifacts must ignore the Worker runtime-base override"
        );

        let gcp_worker_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &gcp,
            &targets,
            crate::toolchain::WorkloadKind::Worker,
        )
        .await
        .unwrap();
        let azure_worker_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &azure,
            &targets,
            crate::toolchain::WorkloadKind::Worker,
        )
        .await
        .unwrap();
        assert_ne!(
            gcp_worker_key, azure_worker_key,
            "Worker artifacts must include their runtime base in the cache key"
        );

        let docker_toolchain = ToolchainConfig::Docker {
            dockerfile: None,
            build_args: None,
            target: None,
        };
        let gcp_docker_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &docker_toolchain,
            &gcp,
            &targets,
            crate::toolchain::WorkloadKind::Worker,
        )
        .await
        .unwrap();
        let azure_docker_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &docker_toolchain,
            &azure,
            &targets,
            crate::toolchain::WorkloadKind::Worker,
        )
        .await
        .unwrap();
        assert_eq!(
            gcp_docker_key, azure_docker_key,
            "Dockerfile builds own their base and must ignore the source Worker override"
        );

        let local_a = BuildSettings {
            platform: PlatformBuildSettings::Local {},
            ..gcp
        };
        let local_b = BuildSettings {
            platform: PlatformBuildSettings::Local {},
            ..azure
        };
        let local_a_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &local_a,
            &targets,
            crate::toolchain::WorkloadKind::Worker,
        )
        .await
        .unwrap();
        let local_b_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &local_b,
            &targets,
            crate::toolchain::WorkloadKind::Worker,
        )
        .await
        .unwrap();
        assert_eq!(
            local_a_key, local_b_key,
            "Local Workers run from scratch and must ignore the cloud runtime base"
        );
    }

    #[tokio::test]
    async fn rust_source_artifact_cache_key_includes_local_path_dependencies() {
        let workspace_dir = tempdir().unwrap();
        let app_dir = workspace_dir.path().join("app");
        let dep_dir = workspace_dir.path().join("dep");
        std::fs::create_dir_all(app_dir.join("src")).unwrap();
        std::fs::create_dir_all(dep_dir.join("src")).unwrap();
        std::fs::write(
            workspace_dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\", \"dep\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::write(
            app_dir.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndep = { path = \"../dep\" }\n",
        )
        .unwrap();
        std::fs::write(app_dir.join("src/main.rs"), "fn main() { dep::value(); }\n").unwrap();
        std::fs::write(
            dep_dir.join("Cargo.toml"),
            "[package]\nname = \"dep\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(dep_dir.join("src/lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();

        let toolchain = ToolchainConfig::Rust {
            binary_name: "app".to_string(),
        };
        let targets = vec![BinaryTarget::LinuxX64];
        let settings = BuildSettings {
            output_directory: workspace_dir
                .path()
                .join("out")
                .to_string_lossy()
                .into_owned(),
            platform: PlatformBuildSettings::Gcp {},
            targets: Some(targets.clone()),
            cache_url: None,
            override_base_image: None,
            debug_mode: false,
        };

        let first_key = compute_source_artifact_cache_key(
            app_dir.to_str().unwrap(),
            &toolchain,
            &settings,
            &targets,
            crate::toolchain::WorkloadKind::Container,
        )
        .await
        .unwrap();

        std::fs::write(dep_dir.join("src/lib.rs"), "pub fn value() -> u32 { 2 }\n").unwrap();

        let second_key = compute_source_artifact_cache_key(
            app_dir.to_str().unwrap(),
            &toolchain,
            &settings,
            &targets,
            crate::toolchain::WorkloadKind::Container,
        )
        .await
        .unwrap();

        assert_ne!(first_key, second_key);
    }

    #[tokio::test]
    async fn rust_source_artifact_cache_key_includes_workspace_toolchain_files() {
        // Toolchain files live at the workspace root, not inside the member's
        // package directory, so this must use a real `[workspace]` layout —
        // otherwise package_dir == workspace_root and hash_source_directory
        // picks the files up as ordinary source, masking a broken/deleted
        // workspace-root hashing loop.
        let workspace_dir = tempdir().unwrap();
        let app_dir = workspace_dir.path().join("app");
        std::fs::create_dir_all(app_dir.join("src")).unwrap();
        std::fs::write(
            workspace_dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::write(
            app_dir.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(app_dir.join("src/main.rs"), "fn main() {}\n").unwrap();

        let toolchain = ToolchainConfig::Rust {
            binary_name: "app".to_string(),
        };
        let targets = vec![BinaryTarget::LinuxX64];
        let settings = BuildSettings {
            output_directory: workspace_dir
                .path()
                .join("out")
                .to_string_lossy()
                .into_owned(),
            platform: PlatformBuildSettings::Gcp {},
            targets: Some(targets.clone()),
            cache_url: None,
            override_base_image: None,
            debug_mode: false,
        };

        let key = |dir: &Path| {
            let dir = dir.to_str().unwrap().to_string();
            let toolchain = toolchain.clone();
            let settings = settings.clone();
            let targets = targets.clone();
            async move {
                compute_source_artifact_cache_key(
                    &dir,
                    &toolchain,
                    &settings,
                    &targets,
                    crate::toolchain::WorkloadKind::Container,
                )
                .await
                .unwrap()
            }
        };

        let without_toolchain_file = key(&app_dir).await;

        std::fs::write(
            workspace_dir.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.84.0\"\n",
        )
        .unwrap();
        let with_pinned_toolchain = key(&app_dir).await;
        assert_ne!(
            without_toolchain_file, with_pinned_toolchain,
            "pinning the compiler via a workspace-root rust-toolchain.toml must invalidate the artifact cache key"
        );

        std::fs::write(
            workspace_dir.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.85.0\"\n",
        )
        .unwrap();
        let with_changed_toolchain = key(&app_dir).await;
        assert_ne!(
            with_pinned_toolchain, with_changed_toolchain,
            "changing the content of the workspace-root rust-toolchain.toml must invalidate the artifact cache key"
        );

        std::fs::create_dir_all(workspace_dir.path().join(".cargo")).unwrap();
        std::fs::write(
            workspace_dir.path().join(".cargo/config.toml"),
            "[build]\nrustflags = [\"-C\", \"target-cpu=native\"]\n",
        )
        .unwrap();
        let with_cargo_config = key(&app_dir).await;
        assert_ne!(
            with_changed_toolchain, with_cargo_config,
            "changing rustflags via workspace-root .cargo/config.toml must invalidate the artifact cache key"
        );
    }

    #[tokio::test]
    async fn source_artifact_cache_key_differs_across_target_triples() {
        let src_dir = tempdir().unwrap();
        std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
        std::fs::write(
            src_dir.path().join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(src_dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

        let toolchain = ToolchainConfig::Rust {
            binary_name: "app".to_string(),
        };
        let key_for = |targets: Vec<BinaryTarget>| {
            let dir = src_dir.path().to_str().unwrap().to_string();
            let out = src_dir.path().join("out").to_string_lossy().into_owned();
            let toolchain = toolchain.clone();
            async move {
                let settings = BuildSettings {
                    output_directory: out,
                    platform: PlatformBuildSettings::Gcp {},
                    targets: Some(targets.clone()),
                    cache_url: None,
                    override_base_image: None,
                    debug_mode: false,
                };
                compute_source_artifact_cache_key(
                    &dir,
                    &toolchain,
                    &settings,
                    &targets,
                    crate::toolchain::WorkloadKind::Container,
                )
                .await
                .unwrap()
            }
        };

        let x64_key = key_for(vec![BinaryTarget::LinuxX64]).await;
        let arm64_key = key_for(vec![BinaryTarget::LinuxArm64]).await;
        assert_ne!(
            x64_key, arm64_key,
            "different target triples must not share build artifacts"
        );
    }

    /// Reuse invariant, end to end at the cache layer: after one platform's build
    /// produces artifacts, an equivalent-target build for another platform finds
    /// them (one build total), while a build for a different triple misses even
    /// though the tarball file exists (two builds total).
    #[tokio::test]
    async fn equivalent_platform_build_reuses_artifact_but_differing_triple_rebuilds() {
        let src_dir = tempdir().unwrap();
        std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
        std::fs::write(
            src_dir.path().join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(src_dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

        let toolchain = ToolchainConfig::Rust {
            binary_name: "app".to_string(),
        };
        let out_root = tempdir().unwrap();
        let settings_for =
            |platform: PlatformBuildSettings, targets: &[BinaryTarget]| BuildSettings {
                output_directory: out_root.path().to_string_lossy().into_owned(),
                platform,
                targets: Some(targets.to_vec()),
                cache_url: None,
                override_base_image: None,
                debug_mode: false,
            };
        let x64 = vec![BinaryTarget::LinuxX64];
        let arm64 = vec![BinaryTarget::LinuxArm64];

        // "First build" (gcp, linux-x64): produce the hashed artifact directory
        // exactly as build_resource finalizes it.
        let gcp_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &settings_for(PlatformBuildSettings::Gcp {}, &x64),
            &x64,
            crate::toolchain::WorkloadKind::Container,
        )
        .await
        .unwrap();
        let gcp_dir = out_root.path().join("build").join("gcp");
        let artifact_dir = gcp_dir.join("app-12345678");
        fs::create_dir_all(&artifact_dir).await.unwrap();
        fs::write(artifact_dir.join("linux-x64.oci.tar"), b"oci")
            .await
            .unwrap();
        // Also stage an arm64 tarball so the differing-triple case below is
        // decided by the cache key, not by a missing target file.
        fs::write(artifact_dir.join("linux-arm64.oci.tar"), b"oci")
            .await
            .unwrap();
        write_artifact_cache_metadata(&artifact_dir, &gcp_key)
            .await
            .unwrap();

        // "Second build" (azure, same source, same linux-x64 target): the key
        // matches and the sibling-platform lookup finds the gcp artifacts, so
        // no second compile happens.
        let azure_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &settings_for(PlatformBuildSettings::Azure {}, &x64),
            &x64,
            crate::toolchain::WorkloadKind::Container,
        )
        .await
        .unwrap();
        assert_eq!(gcp_key, azure_key, "equivalent platforms must share keys");

        let azure_dir = out_root.path().join("build").join("azure");
        fs::create_dir_all(&azure_dir).await.unwrap();
        let reused = find_cached_artifact_dir(&azure_dir, "app", &x64, &azure_key)
            .await
            .unwrap();
        assert_eq!(
            reused,
            Some(artifact_dir.clone()),
            "same inputs + equivalent targets must reuse the one built artifact"
        );

        // "Third build" (aws, linux-arm64): the tarball file exists, but the
        // key differs, so the lookup misses and a real build would run.
        let aws_key = compute_source_artifact_cache_key(
            src_dir.path().to_str().unwrap(),
            &toolchain,
            &settings_for(
                PlatformBuildSettings::Aws {
                    managing_account_id: None,
                },
                &arm64,
            ),
            &arm64,
            crate::toolchain::WorkloadKind::Container,
        )
        .await
        .unwrap();
        assert_ne!(gcp_key, aws_key);

        let aws_dir = out_root.path().join("build").join("aws");
        fs::create_dir_all(&aws_dir).await.unwrap();
        let miss = find_cached_artifact_dir(&aws_dir, "app", &arm64, &aws_key)
            .await
            .unwrap();
        assert_eq!(miss, None, "a differing triple must trigger its own build");
    }

    #[tokio::test]
    async fn artifact_cache_lookup_reuses_sibling_platform_directory() {
        let temp_root = tempdir().unwrap();
        let build_root = temp_root.path().join("build");
        let gcp_dir = build_root.join("gcp");
        let azure_dir = build_root.join("azure");
        let cached_dir = gcp_dir.join("alien-manager-abcdef12");

        fs::create_dir_all(&cached_dir).await.unwrap();
        fs::create_dir_all(&azure_dir).await.unwrap();
        fs::write(cached_dir.join("linux-x64.oci.tar"), b"oci")
            .await
            .unwrap();
        write_artifact_cache_metadata(&cached_dir, "cache-key")
            .await
            .unwrap();

        let found = find_cached_artifact_dir(
            &azure_dir,
            "alien-manager",
            &[BinaryTarget::LinuxX64],
            "cache-key",
        )
        .await
        .unwrap();

        assert_eq!(found, Some(cached_dir));
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

    /// End-to-end: build two arches into one resource dir, push, and assert the pushed tag
    /// resolves to a real multi-arch manifest list (not a single overwritten arch).
    /// Gated on docker + a local registry (`docker run -d -p 5050:5000 registry:2`).
    #[tokio::test]
    async fn multiarch_push_produces_manifest_list() {
        use crate::toolchain::{docker::DockerToolchain, Toolchain, ToolchainContext};

        const REGISTRY: &str = "localhost:5050";
        if !docker_available() {
            eprintln!("Skipping multiarch_push_produces_manifest_list: docker not available");
            return;
        }
        if !registry_available(&format!("http://{REGISTRY}")).await {
            eprintln!(
                "Skipping multiarch_push_produces_manifest_list: no registry at {REGISTRY} (run: docker run -d -p 5050:5000 registry:2)"
            );
            return;
        }

        let src = tempfile::tempdir().unwrap();
        let build_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            src.path().join("Dockerfile"),
            "FROM alpine:latest\nCMD [\"echo\", \"hi\"]\n",
        )
        .unwrap();

        // Build both linux arches into the same resource dir.
        for target in [BinaryTarget::LinuxArm64, BinaryTarget::LinuxX64] {
            let toolchain = DockerToolchain {
                dockerfile: None,
                build_args: None,
                target: None,
            };
            let context = ToolchainContext {
                src_dir: src.path().to_path_buf(),
                build_dir: build_dir.path().to_path_buf(),
                cache_store: None,
                cache_prefix: "test".to_string(),
                build_target: target,
                runtime_platform_name: "aws".to_string(),
                debug_mode: false,
                workload: crate::toolchain::WorkloadKind::Container,
            };
            toolchain
                .build(&context)
                .await
                .expect("docker build should succeed");
        }
        assert!(build_dir.path().join("linux-aarch64.oci.tar").exists());
        assert!(build_dir.path().join("linux-x64.oci.tar").exists());

        let container = Container::new("web".to_string())
            .code(ContainerCode::Image {
                image: build_dir.path().to_string_lossy().into_owned(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .permissions("web".to_string())
            .build();
        let stack = Stack::new("multiarch-test".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .build();

        let push_settings = PushSettings {
            repository: format!("{REGISTRY}/alien-multiarch-test"),
            destination_label: None,
            options: dockdash::PushOptions {
                auth: dockdash::RegistryAuth::Anonymous,
                protocol: dockdash::ClientProtocol::Http,
                ..Default::default()
            },
        };

        let pushed = push_stack(stack, Platform::Aws, &push_settings)
            .await
            .expect("push should succeed");

        let image_uri = pushed
            .resources()
            .filter_map(|(_, entry)| entry.config.downcast_ref::<Container>())
            .find_map(|c| match &c.code {
                ContainerCode::Image { image } => Some(image.clone()),
                _ => None,
            })
            .expect("container should carry a pushed image URI");
        assert!(
            image_uri.contains(REGISTRY),
            "expected a registry URI, got {image_uri}"
        );

        // The pushed tag must resolve to an image index with both linux arches.
        let client = OciClient::new(OciClientConfig {
            protocol: dockdash::ClientProtocol::Http,
            ..Default::default()
        });
        let reference = Reference::try_from(image_uri.as_str()).unwrap();
        let (bytes, _digest) = client
            .pull_manifest_raw(
                &reference,
                &dockdash::RegistryAuth::Anonymous,
                &[
                    OCI_IMAGE_INDEX_MEDIA_TYPE,
                    "application/vnd.docker.distribution.manifest.list.v2+json",
                ],
            )
            .await
            .expect("should pull a manifest list");
        let index: OciImageIndex =
            serde_json::from_slice(&bytes).expect("pushed tag should be an image index");
        let mut platforms: Vec<(String, String)> = index
            .manifests
            .iter()
            .filter_map(|m| {
                m.platform
                    .as_ref()
                    .map(|p| (p.os.clone(), p.architecture.clone()))
            })
            .collect();
        platforms.sort();
        assert_eq!(
            platforms,
            vec![
                ("linux".to_string(), "amd64".to_string()),
                ("linux".to_string(), "arm64".to_string()),
            ],
            "pushed tag must be a real multi-arch index"
        );
    }

    /// End-to-end: build a single arch into a resource dir, push, and assert the pushed tag
    /// resolves to a plain image manifest (not an index). This is the path every current
    /// single-platform release (aws/gcp/azure) takes, so the direct branch must stay intact.
    /// Gated on docker + a local registry (`docker run -d -p 5050:5000 registry:2`).
    #[tokio::test]
    async fn singlearch_push_produces_single_manifest() {
        use crate::toolchain::{docker::DockerToolchain, Toolchain, ToolchainContext};

        const REGISTRY: &str = "localhost:5050";
        if !docker_available() {
            eprintln!("Skipping singlearch_push_produces_single_manifest: docker not available");
            return;
        }
        if !registry_available(&format!("http://{REGISTRY}")).await {
            eprintln!(
                "Skipping singlearch_push_produces_single_manifest: no registry at {REGISTRY} (run: docker run -d -p 5050:5000 registry:2)"
            );
            return;
        }

        let src = tempfile::tempdir().unwrap();
        let build_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            src.path().join("Dockerfile"),
            "FROM alpine:latest\nCMD [\"echo\", \"hi\"]\n",
        )
        .unwrap();

        // Build a single linux arch into the resource dir.
        let toolchain = DockerToolchain {
            dockerfile: None,
            build_args: None,
            target: None,
        };
        let context = ToolchainContext {
            src_dir: src.path().to_path_buf(),
            build_dir: build_dir.path().to_path_buf(),
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: BinaryTarget::LinuxArm64,
            runtime_platform_name: "aws".to_string(),
            debug_mode: false,
            workload: crate::toolchain::WorkloadKind::Container,
        };
        toolchain
            .build(&context)
            .await
            .expect("docker build should succeed");
        assert!(build_dir.path().join("linux-aarch64.oci.tar").exists());

        let container = Container::new("web".to_string())
            .code(ContainerCode::Image {
                image: build_dir.path().to_string_lossy().into_owned(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .permissions("web".to_string())
            .build();
        let stack = Stack::new("singlearch-test".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .build();

        let push_settings = PushSettings {
            repository: format!("{REGISTRY}/alien-singlearch-test"),
            destination_label: None,
            options: dockdash::PushOptions {
                auth: dockdash::RegistryAuth::Anonymous,
                protocol: dockdash::ClientProtocol::Http,
                ..Default::default()
            },
        };

        let pushed = push_stack(stack, Platform::Aws, &push_settings)
            .await
            .expect("push should succeed");

        let image_uri = pushed
            .resources()
            .filter_map(|(_, entry)| entry.config.downcast_ref::<Container>())
            .find_map(|c| match &c.code {
                ContainerCode::Image { image } => Some(image.clone()),
                _ => None,
            })
            .expect("container should carry a pushed image URI");
        assert!(
            image_uri.contains(REGISTRY),
            "expected a registry URI, got {image_uri}"
        );

        // The pushed tag must resolve to a plain image manifest, NOT an index: it has a
        // `config` descriptor and no `manifests` array.
        let client = OciClient::new(OciClientConfig {
            protocol: dockdash::ClientProtocol::Http,
            ..Default::default()
        });
        let reference = Reference::try_from(image_uri.as_str()).unwrap();
        let (bytes, _digest) = client
            .pull_manifest_raw(
                &reference,
                &dockdash::RegistryAuth::Anonymous,
                &[OCI_IMAGE_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE],
            )
            .await
            .expect("should pull a manifest");
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).expect("pushed tag should be valid JSON");
        assert!(
            value.get("config").is_some(),
            "single-arch push must produce an image manifest with a config descriptor, got: {value}"
        );
        assert!(
            value.get("manifests").is_none(),
            "single-arch push must not produce a manifest index, got: {value}"
        );
    }

    /// End-to-end seam: build two arches into two separate partial outputs (one per native
    /// runner), run `merge_build_outputs` to combine them, load the merged stack exactly as
    /// the release path does (deserialize stack.json), then push — asserting the merged dir
    /// resolves to a real multi-arch index. This exercises the merge→load→push chain as one
    /// flow, not as independent halves. Gated on docker + a local registry.
    #[tokio::test]
    async fn merge_then_push_produces_manifest_list() {
        use crate::toolchain::{docker::DockerToolchain, Toolchain, ToolchainContext};

        const REGISTRY: &str = "localhost:5050";
        if !docker_available() {
            eprintln!("Skipping merge_then_push_produces_manifest_list: docker not available");
            return;
        }
        if !registry_available(&format!("http://{REGISTRY}")).await {
            eprintln!(
                "Skipping merge_then_push_produces_manifest_list: no registry at {REGISTRY} (run: docker run -d -p 5050:5000 registry:2)"
            );
            return;
        }

        let src = tempfile::tempdir().unwrap();
        let input_root = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(
            src.path().join("Dockerfile"),
            "FROM alpine:latest\nCMD [\"echo\", \"hi\"]\n",
        )
        .unwrap();

        // Build each arch into its own partial: <input>/<partial>/build/aws/<dir>/<tarball>,
        // with a stack.json whose code.image is that partial's absolute artifact dir — the
        // exact shape a native-runner `alien build --output-dir` upload produces.
        for (partial, target, dir_name) in [
            ("arm", BinaryTarget::LinuxArm64, "web-aaaa1111"),
            ("x64", BinaryTarget::LinuxX64, "web-bbbb2222"),
        ] {
            let platform_dir = input_root.path().join(partial).join("build").join("aws");
            let artifact_dir = platform_dir.join(dir_name);
            std::fs::create_dir_all(&artifact_dir).unwrap();

            let toolchain = DockerToolchain {
                dockerfile: None,
                build_args: None,
                target: None,
            };
            let context = ToolchainContext {
                src_dir: src.path().to_path_buf(),
                build_dir: artifact_dir.clone(),
                cache_store: None,
                cache_prefix: "test".to_string(),
                build_target: target,
                runtime_platform_name: "aws".to_string(),
                debug_mode: false,
                workload: crate::toolchain::WorkloadKind::Container,
            };
            toolchain
                .build(&context)
                .await
                .expect("docker build should succeed");

            let image = artifact_dir
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let container = Container::new("web".to_string())
                .code(ContainerCode::Image { image })
                .cpu(alien_core::ResourceSpec {
                    min: "0.5".to_string(),
                    desired: "1".to_string(),
                })
                .memory(alien_core::ResourceSpec {
                    min: "512Mi".to_string(),
                    desired: "1Gi".to_string(),
                })
                .permissions("web".to_string())
                .build();
            let stack = Stack::new("merge-push-test".to_string())
                .add(container, alien_core::ResourceLifecycle::Live)
                .build();
            std::fs::write(
                platform_dir.join("stack.json"),
                serde_json::to_string_pretty(&stack).unwrap(),
            )
            .unwrap();
        }

        // Merge the two partials into one .alien.
        let platforms = crate::merge::merge_build_outputs(input_root.path(), out.path())
            .expect("merge should succeed");
        assert_eq!(platforms, vec!["aws".to_string()]);

        // Load the merged stack the way the release path does, then push it.
        let merged_json = std::fs::read_to_string(out.path().join("build/aws/stack.json")).unwrap();
        let merged_stack: Stack =
            serde_json::from_str(&merged_json).expect("merged stack.json should deserialize");

        let push_settings = PushSettings {
            repository: format!("{REGISTRY}/alien-merge-push-test"),
            destination_label: None,
            options: dockdash::PushOptions {
                auth: dockdash::RegistryAuth::Anonymous,
                protocol: dockdash::ClientProtocol::Http,
                ..Default::default()
            },
        };

        let pushed = push_stack(merged_stack, Platform::Aws, &push_settings)
            .await
            .expect("push of the merged stack should succeed");

        let image_uri = pushed
            .resources()
            .filter_map(|(_, entry)| entry.config.downcast_ref::<Container>())
            .find_map(|c| match &c.code {
                ContainerCode::Image { image } => Some(image.clone()),
                _ => None,
            })
            .expect("container should carry a pushed image URI");

        let client = OciClient::new(OciClientConfig {
            protocol: dockdash::ClientProtocol::Http,
            ..Default::default()
        });
        let reference = Reference::try_from(image_uri.as_str()).unwrap();
        let (bytes, _digest) = client
            .pull_manifest_raw(
                &reference,
                &dockdash::RegistryAuth::Anonymous,
                &[
                    OCI_IMAGE_INDEX_MEDIA_TYPE,
                    "application/vnd.docker.distribution.manifest.list.v2+json",
                ],
            )
            .await
            .expect("should pull a manifest list");
        let index: OciImageIndex = serde_json::from_slice(&bytes)
            .expect("merged-then-pushed tag should be an image index");
        let mut platforms: Vec<(String, String)> = index
            .manifests
            .iter()
            .filter_map(|m| {
                m.platform
                    .as_ref()
                    .map(|p| (p.os.clone(), p.architecture.clone()))
            })
            .collect();
        platforms.sort();
        assert_eq!(
            platforms,
            vec![
                ("linux".to_string(), "amd64".to_string()),
                ("linux".to_string(), "arm64".to_string()),
            ],
            "merged stack must push as a real multi-arch index"
        );
    }
}
