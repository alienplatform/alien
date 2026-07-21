//! Merge partial `.alien-*` build outputs (one per native-runner group) into one
//! coherent `.alien` directory for a single all-platform release.
//!
//! Each native runner builds a subset of platform/target pairs into its own output dir
//! and uploads it as a CI artifact. The release job downloads them all and runs
//! `alien build merge` to combine them: per platform, the partials are identical except
//! for each compute resource's per-target artifact dir, so merge unions those tarballs
//! into one dir per resource and rewrites the stack to point at it.

use crate::error::{ErrorData, Result};
use alien_core::{Container, ContainerCode, Daemon, DaemonCode, Stack, Worker, WorkerCode};
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tracing::info;

/// A partial output for one platform: the on-disk `build/<platform>` dir (where the
/// artifacts actually live after download) and the stack it declared.
struct PartialPlatform {
    /// `<input>/<artifact>/build/<platform>` — where this partial's files really are.
    dir: PathBuf,
    stack: Stack,
}

/// Merge every partial build output found under `input_dir` into `output_dir`.
/// Returns the platforms that were merged. Fails fast on incompatible partials or on a
/// duplicate target tarball that isn't byte-identical.
pub fn merge_build_outputs(input_dir: &Path, output_dir: &Path) -> Result<Vec<String>> {
    let mut by_platform: BTreeMap<String, Vec<PartialPlatform>> = BTreeMap::new();

    for artifact in read_subdirs(input_dir)? {
        let build_dir = artifact.join("build");
        if !build_dir.is_dir() {
            continue;
        }
        for platform_dir in read_subdirs(&build_dir)? {
            let stack_json = platform_dir.join("stack.json");
            if !stack_json.is_file() {
                continue;
            }
            let platform = platform_dir
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::BuildConfigInvalid {
                        message: format!("Invalid platform dir name: {}", platform_dir.display()),
                    })
                })?
                .to_string();
            let stack = read_stack(&stack_json)?;
            by_platform
                .entry(platform)
                .or_default()
                .push(PartialPlatform {
                    dir: platform_dir,
                    stack,
                });
        }
    }

    if by_platform.is_empty() {
        return Err(AlienError::new(ErrorData::BuildConfigInvalid {
            message: format!(
                "No partial build outputs found under {} (expected <artifact>/build/<platform>/stack.json)",
                input_dir.display()
            ),
        }));
    }

    let mut merged = Vec::new();
    let mut artifact_rewrites = BTreeMap::new();
    for (platform, partials) in &by_platform {
        merge_platform(platform, partials, output_dir, &mut artifact_rewrites)?;
        merged.push(platform.clone());
    }
    rewrite_cross_platform_images(&merged, output_dir, &artifact_rewrites)?;
    Ok(merged)
}

fn merge_platform(
    platform: &str,
    partials: &[PartialPlatform],
    output_dir: &Path,
    artifact_rewrites: &mut BTreeMap<(String, String, String), String>,
) -> Result<()> {
    // All partials for a platform must be the same stack except for image references and
    // whether each compute resource was built here (`blank_images` erases both — see its
    // doc). The resource set and the rest of every resource's config must still match, so a
    // resource present in one partial but absent from another still fails fast below.
    let reference = blank_images(&partials[0].stack);
    for partial in &partials[1..] {
        if blank_images(&partial.stack) != reference {
            return Err(AlienError::new(ErrorData::BuildConfigInvalid {
                message: format!(
                    "Partial stacks for platform '{platform}' are not compatible (they differ in something other than image references). Were they built from the same source?"
                ),
            }));
        }
    }

    let out_platform_dir = output_dir.join("build").join(platform);
    create_dir_all(&out_platform_dir)?;

    let mut merged_stack = partials[0].stack.clone();
    let resource_ids: Vec<String> = merged_stack.resources().map(|(id, _)| id.clone()).collect();

    for resource_id in resource_ids {
        // A partial's stored `code.image` is the build runner's absolute path, which doesn't
        // exist here — resolve the artifact dir by joining its basename to the partial's real dir.
        let mut tarballs: BTreeMap<String, PathBuf> = BTreeMap::new();
        let mut source_locations = BTreeSet::new();
        let mut is_local = false;

        for partial in partials {
            let Some(image) = compute_image(&partial.stack, &resource_id) else {
                continue; // not a compute resource
            };
            let Some(basename) = Path::new(&image).file_name() else {
                continue;
            };
            let artifact_dir = partial.dir.join(basename);
            if !artifact_dir.is_dir() {
                continue; // remote-URI image (no local dir) — leave it untouched
            }
            source_locations.insert(build_artifact_location(&image).unwrap_or_else(|| {
                (
                    platform.to_string(),
                    basename.to_string_lossy().into_owned(),
                )
            }));
            is_local = true;
            for tar in read_oci_tarballs(&artifact_dir)? {
                let name = tar
                    .file_name()
                    .and_then(|n| n.to_str())
                    .expect("read_oci_tarballs returns UTF-8 .oci.tar file names")
                    .to_string();
                match tarballs.get(&name) {
                    Some(existing) if !files_equal(existing, &tar)? => {
                        return Err(AlienError::new(ErrorData::BuildConfigInvalid {
                            message: format!(
                                "Resource '{resource_id}' on platform '{platform}' has two different '{name}' tarballs across partials"
                            ),
                        }));
                    }
                    Some(_) => {} // byte-identical duplicate — keep the first
                    None => {
                        tarballs.insert(name, tar);
                    }
                }
            }
        }

        if !is_local {
            continue; // remote-URI or non-compute resource passes through unchanged
        }

        let short_hash = content_hash(&tarballs)?;
        let merged_dir = out_platform_dir.join(format!("{resource_id}-{short_hash}"));
        create_dir_all(&merged_dir)?;
        for (name, src) in &tarballs {
            let dest = merged_dir.join(name);
            std::fs::copy(src, &dest).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "copy".to_string(),
                    file_path: src.display().to_string(),
                    reason: format!("Failed to copy tarball into merged dir {}", dest.display()),
                },
            )?;
        }
        let absolute = merged_dir.canonicalize().into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "canonicalize".to_string(),
                file_path: merged_dir.display().to_string(),
                reason: "Failed to resolve merged artifact dir".to_string(),
            },
        )?;
        set_compute_image(
            &mut merged_stack,
            &resource_id,
            absolute.to_string_lossy().into_owned(),
        );
        for (source_platform, basename) in source_locations {
            artifact_rewrites.insert(
                (source_platform, resource_id.clone(), basename),
                absolute.to_string_lossy().into_owned(),
            );
        }
        info!(
            "Merged {} tarball(s) for resource '{}' on platform '{}'",
            tarballs.len(),
            resource_id,
            platform
        );
    }

    write_stack(&out_platform_dir.join("stack.json"), &merged_stack)?;
    Ok(())
}

fn rewrite_cross_platform_images(
    platforms: &[String],
    output_dir: &Path,
    artifact_rewrites: &BTreeMap<(String, String, String), String>,
) -> Result<()> {
    for platform in platforms {
        let stack_path = output_dir.join("build").join(platform).join("stack.json");
        let mut stack = read_stack(&stack_path)?;
        let resource_ids: Vec<String> = stack.resources().map(|(id, _)| id.clone()).collect();
        let mut changed = false;

        for resource_id in resource_ids {
            let Some(image) = compute_image(&stack, &resource_id) else {
                continue;
            };
            let Some((source_platform, basename)) = build_artifact_location(&image) else {
                continue;
            };
            let Some(merged_image) =
                artifact_rewrites.get(&(source_platform, resource_id.clone(), basename))
            else {
                continue;
            };
            if &image != merged_image {
                set_compute_image(&mut stack, &resource_id, merged_image.clone());
                changed = true;
            }
        }

        if changed {
            write_stack(&stack_path, &stack)?;
        }
    }
    Ok(())
}

fn build_artifact_location(image: &str) -> Option<(String, String)> {
    let components = Path::new(image)
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();

    components.windows(4).find_map(|window| {
        let is_build_root = window[0] == ".alien"
            || window[0]
                .strip_prefix(".alien-")
                .is_some_and(|suffix| !suffix.is_empty());
        (is_build_root && window[1] == "build")
            .then(|| (window[2].to_string(), window[3].to_string()))
    })
}

fn write_stack(stack_path: &Path, stack: &Stack) -> Result<()> {
    let stack_json = serde_json::to_string_pretty(stack)
        .into_alien_error()
        .context(ErrorData::JsonSerializationError {
            message: "Failed to serialize merged stack.json".to_string(),
        })?;
    std::fs::write(stack_path, stack_json)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: stack_path.display().to_string(),
            reason: "Failed to write merged stack.json".to_string(),
        })?;
    Ok(())
}

/// Clone a stack with every compute resource forced to `Image{""}`, so two partials can be
/// compared ignoring both the per-target image reference (expected to differ) and whether
/// the resource was built here vs skipped (`Image` vs `Source` — a skipped container is left
/// `Source`; see `set_compute_image`). Everything else must still match for the compat check.
fn blank_images(stack: &Stack) -> Stack {
    let mut blanked = stack.clone();
    let ids: Vec<String> = blanked.resources().map(|(id, _)| id.clone()).collect();
    for id in ids {
        set_compute_image(&mut blanked, &id, String::new());
    }
    blanked
}

/// Read a compute resource's `code.image` (Worker, Container, or Daemon), if it is one.
fn compute_image(stack: &Stack, resource_id: &str) -> Option<String> {
    let entry = stack.resources().find(|(id, _)| *id == resource_id)?.1;
    if let Some(worker) = entry.config.downcast_ref::<Worker>() {
        if let WorkerCode::Image { image } = &worker.code {
            return Some(image.clone());
        }
    } else if let Some(container) = entry.config.downcast_ref::<Container>() {
        if let ContainerCode::Image { image } = &container.code {
            return Some(image.clone());
        }
    } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
        if let DaemonCode::Image { image } = &daemon.code {
            return Some(image.clone());
        }
    }
    None
}

/// Point a compute resource (Worker, Container, or Daemon) at `image`, converting it to
/// `Image` code regardless of its current variant. No-op for non-compute resources.
///
/// Variant-insensitive on purpose — the write counterpart to the variant-sensitive
/// [`compute_image`]: a container skipped on a non-Linux runner group is left as `Source`
/// in that partial. Converting any variant lets [`blank_images`] normalize a `Source` and
/// an `Image` of the same resource to the same `Image{""}` (the compat check treats "built
/// here" vs "skipped here" as compatible), and lets the merge write-back stamp the merged
/// dir even when `partials[0]` is the partial that skipped it.
fn set_compute_image(stack: &mut Stack, resource_id: &str, image: String) {
    let Some((_, entry)) = stack.resources_mut().find(|(id, _)| *id == resource_id) else {
        return;
    };
    if let Some(worker) = entry.config.downcast_mut::<Worker>() {
        worker.code = WorkerCode::Image { image };
    } else if let Some(container) = entry.config.downcast_mut::<Container>() {
        container.code = ContainerCode::Image { image };
    } else if let Some(daemon) = entry.config.downcast_mut::<Daemon>() {
        daemon.code = DaemonCode::Image { image };
    }
}

/// Deterministic content hash (first 8 hex) over a target→path set, sorted by target name.
fn content_hash(files: &BTreeMap<String, PathBuf>) -> Result<String> {
    let mut hasher = Sha256::new();
    for (name, path) in files {
        hasher.update(name.as_bytes());
        let bytes =
            std::fs::read(path)
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "read".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to read tarball for merged hash".to_string(),
                })?;
        hasher.update(&bytes);
    }
    Ok(format!("{:x}", hasher.finalize())[..8].to_string())
}

fn files_equal(a: &Path, b: &Path) -> Result<bool> {
    let read = |p: &Path| {
        std::fs::read(p)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: p.display().to_string(),
                reason: "Failed to read tarball for duplicate comparison".to_string(),
            })
    };
    Ok(read(a)? == read(b)?)
}

fn read_stack(path: &Path) -> Result<Stack> {
    let content = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to read partial stack.json".to_string(),
        },
    )?;
    serde_json::from_str(&content)
        .into_alien_error()
        .context(ErrorData::JsonSerializationError {
            message: format!("Failed to parse {}", path.display()),
        })
}

fn read_subdirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    let entries =
        std::fs::read_dir(dir)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory".to_string(),
                file_path: dir.display().to_string(),
                reason: "Failed to list directory".to_string(),
            })?;
    for entry in entries {
        let entry = entry
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory entry".to_string(),
                file_path: dir.display().to_string(),
                reason: "Failed to read directory entry".to_string(),
            })?;
        if entry.path().is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn read_oci_tarballs(dir: &Path) -> Result<Vec<PathBuf>> {
    Ok(read_files(dir)?
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".oci.tar"))
        })
        .collect())
}

fn read_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries =
        std::fs::read_dir(dir)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory".to_string(),
                file_path: dir.display().to_string(),
                reason: "Failed to list artifact dir".to_string(),
            })?;
    for entry in entries {
        let entry = entry
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory entry".to_string(),
                file_path: dir.display().to_string(),
                reason: "Failed to read artifact dir entry".to_string(),
            })?;
        if entry.path().is_file() {
            files.push(entry.path());
        }
    }
    Ok(files)
}

fn create_dir_all(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: dir.display().to_string(),
            reason: "Failed to create output directory".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Container, ResourceLifecycle, ResourceSpec, ToolchainConfig};
    use tempfile::tempdir;

    fn container(name: &str, image: &str) -> Container {
        Container::new(name.to_string())
            .code(ContainerCode::Image {
                image: image.to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .permissions("web".to_string())
            .build()
    }

    /// Same container as `container`, but still `Source` — mirrors what `build_stack` leaves
    /// for a container skipped on a non-Linux runner group (config identical, code unbuilt).
    fn container_source(name: &str) -> Container {
        Container::new(name.to_string())
            .code(ContainerCode::Source {
                src: ".".to_string(),
                toolchain: ToolchainConfig::Docker {
                    dockerfile: None,
                    build_args: None,
                    target: None,
                },
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .permissions("web".to_string())
            .build()
    }

    fn stack_with(resource: &str, image: &str) -> Stack {
        Stack::new("merge-test".to_string())
            .add(container(resource, image), ResourceLifecycle::Live)
            .build()
    }

    /// Write `<root>/<partial>/build/<platform>/{stack.json, <dir_name>/<tarballs>}` with the
    /// stack's `code.image` pointing at the (absolute) artifact dir, mimicking a real partial.
    fn write_partial(
        root: &Path,
        partial: &str,
        platform: &str,
        resource: &str,
        dir_name: &str,
        tarballs: &[(&str, &[u8])],
    ) {
        let platform_dir = root.join(partial).join("build").join(platform);
        let image: String = if tarballs.is_empty() {
            // remote-URI resource (no local dir)
            dir_name.to_string()
        } else {
            let artifact_dir = platform_dir.join(dir_name);
            std::fs::create_dir_all(&artifact_dir).unwrap();
            for (name, bytes) in tarballs {
                std::fs::write(artifact_dir.join(name), bytes).unwrap();
            }
            artifact_dir
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        std::fs::create_dir_all(&platform_dir).unwrap();
        let stack = stack_with(resource, &image);
        std::fs::write(
            platform_dir.join("stack.json"),
            serde_json::to_string_pretty(&stack).unwrap(),
        )
        .unwrap();
    }

    fn worker_image(name: &str, image: &str) -> Worker {
        Worker::new(name.to_string())
            .permissions("api".to_string())
            .code(WorkerCode::Image {
                image: image.to_string(),
            })
            .build()
    }

    /// Write a `local` partial for the mixed-stack test: a Worker `api` (always built, one
    /// `tarball`) plus a Container `web` that is either built (`web_tarball`, a Linux group)
    /// or skipped (`None`, a macOS/Windows group → left `Source`, no artifact dir).
    fn write_mixed_partial(
        root: &Path,
        partial: &str,
        api_tarball: &str,
        web_tarball: Option<&str>,
    ) {
        let platform_dir = root.join(partial).join("build").join("local");
        std::fs::create_dir_all(&platform_dir).unwrap();

        let built_dir = |resource: &str, tarball: &str| -> String {
            let dir = platform_dir.join(format!("{resource}-{partial}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(tarball), tarball.as_bytes()).unwrap();
            dir.canonicalize().unwrap().to_string_lossy().into_owned()
        };

        let api = worker_image("api", &built_dir("api", api_tarball));
        let web = match web_tarball {
            Some(tarball) => container("web", &built_dir("web", tarball)),
            None => container_source("web"),
        };
        // Same insertion order in every partial so the compat check sees one resource set.
        let stack = Stack::new("merge-test".to_string())
            .add(api, ResourceLifecycle::Live)
            .add(web, ResourceLifecycle::Live)
            .build();
        std::fs::write(
            platform_dir.join("stack.json"),
            serde_json::to_string_pretty(&stack).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn merges_mixed_stack_with_container_skipped_on_host_groups() {
        // The real scenario: a Worker `api` builds on all 4 native-runner groups; the Container
        // `web` builds only on the two Linux groups and is skipped (left `Source`) on the macOS
        // and Windows groups. Merge must union each resource over the partials that built it —
        // `api` gets all 4 targets, `web` only the 2 Linux ones — independently.
        let root = tempdir().unwrap();
        let out = tempdir().unwrap();
        write_mixed_partial(
            root.path(),
            "arm",
            "linux-aarch64.oci.tar",
            Some("linux-aarch64.oci.tar"),
        );
        write_mixed_partial(
            root.path(),
            "x64",
            "linux-x64.oci.tar",
            Some("linux-x64.oci.tar"),
        );
        write_mixed_partial(root.path(), "darwin", "darwin-aarch64.oci.tar", None);
        write_mixed_partial(root.path(), "windows", "windows-x64.oci.tar", None);

        let platforms = merge_build_outputs(root.path(), out.path()).unwrap();
        assert_eq!(platforms, vec!["local".to_string()]);
        let merged: Stack = read_stack(&out.path().join("build/local/stack.json")).unwrap();

        let api_dir =
            PathBuf::from(compute_image(&merged, "api").expect("api should be an Image dir"));
        for tar in [
            "linux-aarch64",
            "linux-x64",
            "darwin-aarch64",
            "windows-x64",
        ] {
            assert!(
                api_dir.join(format!("{tar}.oci.tar")).exists(),
                "api missing {tar}"
            );
        }

        let web_dir =
            PathBuf::from(compute_image(&merged, "web").expect("web should be an Image dir"));
        assert!(web_dir.join("linux-aarch64.oci.tar").exists());
        assert!(web_dir.join("linux-x64.oci.tar").exists());
        // The macOS/Windows groups skipped the container, so it has no host-binary tarballs.
        assert!(!web_dir.join("darwin-aarch64.oci.tar").exists());
        assert!(!web_dir.join("windows-x64.oci.tar").exists());
    }

    #[test]
    fn merges_daemon_artifact_dirs_across_partials() {
        // A daemon's per-target artifact dirs from two runner groups union into one dir.
        let root = tempdir().unwrap();
        let out = tempdir().unwrap();
        let write_daemon_partial = |partial: &str, dir_name: &str, tarball: &str| {
            let platform_dir = root.path().join(partial).join("build").join("local");
            let artifact_dir = platform_dir.join(dir_name);
            std::fs::create_dir_all(&artifact_dir).unwrap();
            std::fs::write(artifact_dir.join(tarball), tarball.as_bytes()).unwrap();
            let image = artifact_dir
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let daemon = Daemon::new("agent".to_string())
                .permissions("execution".to_string())
                .code(DaemonCode::Image { image })
                .build();
            let stack = Stack::new("merge-test".to_string())
                .add(daemon, ResourceLifecycle::Live)
                .build();
            std::fs::write(
                platform_dir.join("stack.json"),
                serde_json::to_string_pretty(&stack).unwrap(),
            )
            .unwrap();
        };
        write_daemon_partial("arm", "agent-aaaa1111", "darwin-aarch64.oci.tar");
        write_daemon_partial("x64", "agent-bbbb2222", "linux-x64.oci.tar");

        let platforms = merge_build_outputs(root.path(), out.path()).unwrap();
        assert_eq!(platforms, vec!["local".to_string()]);

        let merged: Stack = read_stack(&out.path().join("build/local/stack.json")).unwrap();
        let image = compute_image(&merged, "agent").expect("agent should have a merged image dir");
        let merged_dir = PathBuf::from(&image);
        assert!(merged_dir.join("darwin-aarch64.oci.tar").exists());
        assert!(merged_dir.join("linux-x64.oci.tar").exists());
    }

    #[test]
    fn merges_two_arches_into_one_dir() {
        let root = tempdir().unwrap();
        let out = tempdir().unwrap();
        write_partial(
            root.path(),
            "arm",
            "kubernetes",
            "web",
            "web-aaaa1111",
            &[("linux-aarch64.oci.tar", b"arm")],
        );
        write_partial(
            root.path(),
            "x64",
            "kubernetes",
            "web",
            "web-bbbb2222",
            &[("linux-x64.oci.tar", b"x64")],
        );

        let platforms = merge_build_outputs(root.path(), out.path()).unwrap();
        assert_eq!(platforms, vec!["kubernetes".to_string()]);

        let merged: Stack = read_stack(&out.path().join("build/kubernetes/stack.json")).unwrap();
        let image = compute_image(&merged, "web").expect("web should still have an image dir");
        let merged_dir = PathBuf::from(&image);
        assert!(
            merged_dir.is_dir(),
            "merged image dir should exist: {image}"
        );
        assert!(merged_dir.join("linux-aarch64.oci.tar").exists());
        assert!(merged_dir.join("linux-x64.oci.tar").exists());
    }

    #[test]
    fn rewrites_other_platforms_that_share_a_renamed_artifact() {
        let root = tempdir().unwrap();
        let out = tempdir().unwrap();
        write_partial(
            root.path(),
            "arm",
            "local",
            "web",
            "web-arm-source",
            &[("linux-aarch64.oci.tar", b"arm")],
        );
        write_partial(
            root.path(),
            "x64",
            "local",
            "web",
            "web-x64-source",
            &[("linux-x64.oci.tar", b"x64")],
        );

        let aws_dir = root.path().join("arm/build/aws");
        std::fs::create_dir_all(&aws_dir).unwrap();
        let aws_stack = stack_with("web", ".alien-arm64/build/local/web-arm-source");
        std::fs::write(
            aws_dir.join("stack.json"),
            serde_json::to_string_pretty(&aws_stack).unwrap(),
        )
        .unwrap();

        assert_eq!(
            merge_build_outputs(root.path(), out.path()).unwrap(),
            vec!["aws".to_string(), "local".to_string()]
        );

        let local = read_stack(&out.path().join("build/local/stack.json")).unwrap();
        let aws = read_stack(&out.path().join("build/aws/stack.json")).unwrap();
        let merged_image = compute_image(&local, "web").unwrap();
        assert_eq!(
            compute_image(&aws, "web").as_deref(),
            Some(merged_image.as_str())
        );
        assert!(Path::new(&merged_image)
            .join("linux-aarch64.oci.tar")
            .is_file());
        assert!(Path::new(&merged_image).join("linux-x64.oci.tar").is_file());
    }

    #[test]
    fn rejects_conflicting_duplicate_tarball() {
        let root = tempdir().unwrap();
        let out = tempdir().unwrap();
        write_partial(
            root.path(),
            "a",
            "aws",
            "web",
            "web-aaaa1111",
            &[("linux-aarch64.oci.tar", b"one")],
        );
        write_partial(
            root.path(),
            "b",
            "aws",
            "web",
            "web-bbbb2222",
            &[("linux-aarch64.oci.tar", b"two")],
        );
        assert!(merge_build_outputs(root.path(), out.path()).is_err());
    }

    #[test]
    fn accepts_byte_identical_duplicate() {
        let root = tempdir().unwrap();
        let out = tempdir().unwrap();
        write_partial(
            root.path(),
            "a",
            "aws",
            "web",
            "web-aaaa1111",
            &[("linux-aarch64.oci.tar", b"same")],
        );
        write_partial(
            root.path(),
            "b",
            "aws",
            "web",
            "web-bbbb2222",
            &[("linux-aarch64.oci.tar", b"same")],
        );
        assert_eq!(
            merge_build_outputs(root.path(), out.path()).unwrap(),
            vec!["aws".to_string()]
        );
    }

    #[test]
    fn rejects_incompatible_stacks() {
        let root = tempdir().unwrap();
        let out = tempdir().unwrap();
        // Different resource sets across partials → not compatible.
        write_partial(
            root.path(),
            "a",
            "aws",
            "web",
            "web-aaaa1111",
            &[("linux-aarch64.oci.tar", b"x")],
        );
        write_partial(
            root.path(),
            "b",
            "aws",
            "api",
            "api-bbbb2222",
            &[("linux-x64.oci.tar", b"y")],
        );
        assert!(merge_build_outputs(root.path(), out.path()).is_err());
    }

    #[test]
    fn passes_through_remote_uri_resources() {
        let root = tempdir().unwrap();
        let out = tempdir().unwrap();
        // No tarballs → the stack's image is a remote URI with no local dir.
        write_partial(
            root.path(),
            "a",
            "aws",
            "web",
            "registry.example.com/web:tag",
            &[],
        );
        write_partial(
            root.path(),
            "b",
            "aws",
            "web",
            "registry.example.com/web:tag",
            &[],
        );

        assert_eq!(
            merge_build_outputs(root.path(), out.path()).unwrap(),
            vec!["aws".to_string()]
        );
        let merged: Stack = read_stack(&out.path().join("build/aws/stack.json")).unwrap();
        assert_eq!(
            compute_image(&merged, "web").as_deref(),
            Some("registry.example.com/web:tag")
        );
    }
}
