use crate::base_images::effective_source_base_images;
use crate::error::{ErrorData, Result};
use crate::push::generate_unique_tag;
use crate::settings::BuildSettings;
use crate::toolchain;
use alien_core::{BinaryTarget, Platform, ToolchainConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;
use tracing::info;

pub(crate) const ARTIFACT_CACHE_METADATA_FILE: &str = ".alien-build-cache.json";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ArtifactCacheMetadata {
    cache_key: String,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CargoMetadata {
    packages: Vec<CargoMetadataPackage>,
    resolve: Option<CargoMetadataResolve>,
    workspace_root: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CargoMetadataPackage {
    id: String,
    manifest_path: PathBuf,
    source: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CargoMetadataResolve {
    root: Option<String>,
    nodes: Vec<CargoMetadataNode>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CargoMetadataNode {
    id: String,
    dependencies: Vec<String>,
}

pub(crate) fn temp_artifact_dir(build_output_dir: &Path, resource_name: &str) -> PathBuf {
    build_output_dir.join(format!(".{}-tmp-{}", resource_name, generate_unique_tag()))
}

pub(crate) async fn finalize_artifact_dir(
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

pub(crate) async fn compute_source_artifact_cache_key(
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

pub(crate) async fn hash_build_input_source(
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
pub(crate) async fn hash_typescript_dependency_inputs(
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

pub(crate) async fn hash_rust_build_input_graph(src_dir: &Path, hasher: &mut Sha256) -> Result<()> {
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

pub(crate) async fn read_cargo_metadata(src_dir: &Path) -> Result<CargoMetadata> {
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

pub(crate) fn local_cargo_package_ids(metadata: &CargoMetadata) -> HashSet<String> {
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

pub(crate) async fn hash_source_directory(src_dir: &Path, hasher: &mut Sha256) -> Result<()> {
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

pub(crate) fn collect_source_files(
    base_dir: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
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

pub(crate) fn is_ignored_source_cache_path(file_name: &str) -> bool {
    matches!(
        file_name,
        ".git" | ".alien" | ".alien-build" | "target" | "node_modules" | "alien-bindings.node" // staged addon: derived artifact, hashed via its source
    ) || file_name.ends_with(".bun-build")
}

pub(crate) async fn find_cached_artifact_dir(
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

pub(crate) async fn find_cached_artifact_dir_in(
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

pub(crate) async fn write_artifact_cache_metadata(
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

/// Compute a content hash of all OCI tarballs in a directory.
///
/// This hash is used to detect code changes between builds. When the source code
/// changes, the OCI tarball contents change, producing a different hash. This hash
/// is then included in the output directory name, ensuring the executor detects
/// config changes and plans an UPDATE.
pub(crate) async fn compute_function_content_hash(dir: &Path) -> Result<String> {
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
