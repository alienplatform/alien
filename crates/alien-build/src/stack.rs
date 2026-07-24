use super::*;

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
pub(super) fn requested_host_binary_only(targets: Option<&[BinaryTarget]>) -> bool {
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
