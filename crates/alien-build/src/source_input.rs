use crate::error::{ErrorData, Result};
use alien_core::{BinaryTarget, ToolchainConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use tokio::fs;

const FORMAT_VERSION: &[u8] = b"alien-docker-source-input-v1";

/// Hex SHA-256 of everything a Docker source build reads: the files under `src` by their
/// relative paths, the Dockerfile, the build args, the build stage, and the target platforms.
///
/// The absolute location of `src` is not an input, so two machines with the same tree agree.
/// Files the build cache ignores (`.git`, `node_modules`, `target`, ...) and symlinks are not
/// hashed. Base images named in `FROM` are hashed by name only, never by their registry digest.
pub async fn docker_source_input_hash(
    src: &Path,
    toolchain: &ToolchainConfig,
    targets: &[BinaryTarget],
) -> Result<String> {
    let ToolchainConfig::Docker {
        dockerfile,
        build_args,
        target,
    } = toolchain
    else {
        return Err(AlienError::new(ErrorData::InvalidResourceConfig {
            resource_id: src.display().to_string(),
            reason: "Only a Docker build has a source input hash".to_string(),
        }));
    };

    let mut hasher = Sha256::new();
    field(&mut hasher, FORMAT_VERSION);

    let dockerfile = dockerfile.as_deref().unwrap_or("Dockerfile");
    field(&mut hasher, b"dockerfile");
    field(&mut hasher, dockerfile.as_bytes());
    field(&mut hasher, &read(&src.join(dockerfile)).await?);

    let mut build_args = build_args.iter().flatten().collect::<Vec<_>>();
    build_args.sort();
    for (name, value) in build_args {
        field(&mut hasher, b"build-arg");
        field(&mut hasher, name.as_bytes());
        field(&mut hasher, value.as_bytes());
    }
    if let Some(target) = target {
        field(&mut hasher, b"target");
        field(&mut hasher, target.as_bytes());
    }
    for platform in targets {
        field(&mut hasher, b"platform");
        field(&mut hasher, platform.runtime_platform_id().as_bytes());
    }

    let mut files = Vec::new();
    crate::collect_source_files(src, src, &mut files)?;
    let mut files = files
        .into_iter()
        .map(|relative| (portable_path(&relative), relative))
        .collect::<Vec<_>>();
    files.sort();
    for (portable, relative) in files {
        field(&mut hasher, b"file");
        field(&mut hasher, portable.as_bytes());
        field(&mut hasher, &read(&src.join(relative)).await?);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Length-prefixed, so no two different input sequences hash the same bytes.
fn field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn portable_path(relative: &Path) -> String {
    relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

async fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read file".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to read a build input for the source input hash".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn docker(dockerfile: Option<&str>) -> ToolchainConfig {
        ToolchainConfig::Docker {
            dockerfile: dockerfile.map(str::to_string),
            build_args: None,
            target: None,
        }
    }

    fn sandbox_tree() -> TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            dir.path().join("Dockerfile"),
            "FROM alpine:3.20\nCOPY app /app\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("Sandbox.dockerfile"), "FROM alpine:3.20\n").unwrap();
        std::fs::create_dir_all(dir.path().join("app/bin")).unwrap();
        std::fs::write(dir.path().join("app/bin/run.sh"), "#!/bin/sh\necho hi\n").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/dep")).unwrap();
        std::fs::write(dir.path().join("node_modules/dep/index.js"), "one").unwrap();
        dir
    }

    async fn hash(dir: &Path, toolchain: &ToolchainConfig) -> String {
        docker_source_input_hash(dir, toolchain, &[BinaryTarget::LinuxArm64])
            .await
            .expect("the tree should hash")
    }

    #[tokio::test]
    async fn the_same_tree_at_two_paths_hashes_the_same() {
        let first = sandbox_tree();
        let second = sandbox_tree();
        assert_ne!(first.path(), second.path());

        assert_eq!(
            hash(first.path(), &docker(None)).await,
            hash(second.path(), &docker(None)).await
        );
    }

    #[tokio::test]
    async fn a_changed_file_changes_the_hash() {
        let dir = sandbox_tree();
        let before = hash(dir.path(), &docker(None)).await;

        std::fs::write(dir.path().join("app/bin/run.sh"), "#!/bin/sh\necho bye\n").unwrap();
        assert_ne!(hash(dir.path(), &docker(None)).await, before);
    }

    #[tokio::test]
    async fn a_moved_file_changes_the_hash() {
        let dir = sandbox_tree();
        let before = hash(dir.path(), &docker(None)).await;

        std::fs::rename(
            dir.path().join("app/bin/run.sh"),
            dir.path().join("app/run.sh"),
        )
        .unwrap();
        assert_ne!(hash(dir.path(), &docker(None)).await, before);
    }

    #[tokio::test]
    async fn the_build_configuration_is_an_input() {
        let dir = sandbox_tree();
        let default = hash(dir.path(), &docker(None)).await;

        assert_eq!(hash(dir.path(), &docker(Some("Dockerfile"))).await, default);
        assert_ne!(
            hash(dir.path(), &docker(Some("Sandbox.dockerfile"))).await,
            default
        );
        let with_arg = ToolchainConfig::Docker {
            dockerfile: None,
            build_args: Some(HashMap::from([("VERSION".to_string(), "2".to_string())])),
            target: None,
        };
        assert_ne!(hash(dir.path(), &with_arg).await, default);
        let with_stage = ToolchainConfig::Docker {
            dockerfile: None,
            build_args: None,
            target: Some("runtime".to_string()),
        };
        assert_ne!(hash(dir.path(), &with_stage).await, default);
        assert_ne!(
            docker_source_input_hash(dir.path(), &docker(None), &[BinaryTarget::LinuxX64])
                .await
                .unwrap(),
            default
        );
    }

    #[tokio::test]
    async fn ignored_directories_are_not_inputs() {
        let dir = sandbox_tree();
        let before = hash(dir.path(), &docker(None)).await;

        std::fs::write(dir.path().join("node_modules/dep/index.js"), "two").unwrap();
        assert_eq!(hash(dir.path(), &docker(None)).await, before);
    }

    #[tokio::test]
    async fn a_missing_dockerfile_is_an_error() {
        let dir = sandbox_tree();
        docker_source_input_hash(
            dir.path(),
            &docker(Some("Missing.dockerfile")),
            &[BinaryTarget::LinuxArm64],
        )
        .await
        .expect_err("a build without its Dockerfile has no inputs to hash");
    }
}
