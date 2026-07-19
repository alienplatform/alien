use crate::error::{DockdashResultExt, ErrorData, Result};
use crate::settings::PushSettings;
use alien_core::{
    alien_event, AlienEvent, BinaryTarget, Container, ContainerCode, Daemon, DaemonCode, Platform,
    Stack, Worker, WorkerCode,
};
use alien_error::{AlienError, Context, IntoAlienError};
use dockdash::Image as DockDashImage;
use oci_client::client::{Client as OciClient, ClientConfig as OciClientConfig};
use oci_client::manifest::{
    ImageIndexEntry, OciImageIndex, Platform as OciPlatform, IMAGE_MANIFEST_MEDIA_TYPE,
    OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE,
};
use oci_client::Reference;
use rand::distr::Alphanumeric;
use rand::Rng;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::fs;
use tracing::info;

/// A compute resource that has a locally-built image directory and needs to be pushed to a registry.
pub(crate) struct ResourcePushTarget {
    /// Stack resource keys that should be updated with the pushed image URI.
    pub(crate) resource_ids: Vec<String>,
    /// Resource IDs sharing this push target. The first name is used for logging and image tagging.
    pub(crate) resource_names: Vec<String>,
    /// Display name for events/logging ("worker", "container", etc.)
    pub(crate) resource_type: &'static str,
    /// Local directory containing OCI tarballs produced by `alien build`
    pub(crate) local_image_dir: PathBuf,
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

    pub(crate) fn push_result_updates(&self, image_uri: String) -> Vec<(String, String)> {
        self.resource_ids
            .iter()
            .map(|resource_id| (resource_id.clone(), image_uri.clone()))
            .collect()
    }
}

pub(crate) fn push_target_for_local_image<'a>(
    targets: &'a mut Vec<ResourcePushTarget>,
    resource_type: &'static str,
    local_image_dir: &Path,
) -> Option<&'a mut ResourcePushTarget> {
    targets.iter_mut().find(|target| {
        target.resource_type == resource_type && target.local_image_dir == local_image_dir
    })
}

pub(crate) fn add_push_target_resource(
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
pub(crate) fn collect_push_targets(stack: &Stack) -> Result<Vec<ResourcePushTarget>> {
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
pub(crate) fn apply_pushed_images(stack: &mut Stack, updates: Vec<(String, String)>) {
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
pub(crate) async fn push_resource_images(
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
pub(crate) fn select_linux_tarballs(oci_files: &[PathBuf]) -> Vec<(BinaryTarget, PathBuf)> {
    let mut out: Vec<(BinaryTarget, PathBuf)> = oci_files
        .iter()
        .filter_map(|path| oci_tarball_target(path).map(|target| (target, path.clone())))
        .filter(|(target, _)| target.oci_os() == "linux")
        .collect();
    out.sort_by_key(|(target, _)| target.runtime_platform_id());
    out
}

/// `<runtime_platform_id>.oci.tar` → its `BinaryTarget`.
pub(crate) fn oci_tarball_target(path: &Path) -> Option<BinaryTarget> {
    let name = path.file_name()?.to_str()?;
    BinaryTarget::from_runtime_platform_id(name.strip_suffix(".oci.tar")?)
}

/// One OCI image index entry for a built target.
pub(crate) fn image_index_entry(
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
pub(crate) fn assemble_image_index(manifests: Vec<ImageIndexEntry>) -> OciImageIndex {
    OciImageIndex {
        schema_version: 2,
        media_type: Some(OCI_IMAGE_INDEX_MEDIA_TYPE.to_string()),
        manifests,
        artifact_type: None,
        annotations: None,
    }
}

/// Read the `mediaType` field from a raw manifest, if present.
pub(crate) fn manifest_media_type(bytes: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()?
        .get("mediaType")?
        .as_str()
        .map(|s| s.to_string())
}

pub(crate) fn generate_unique_tag() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect::<String>()
        .to_lowercase()
}
