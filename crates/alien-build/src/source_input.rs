use crate::dockerignore::DockerIgnore;
use crate::error::{ErrorData, Result};
use crate::settings::BuildSettings;
use alien_core::ToolchainConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use tokio::fs;

const FORMAT_VERSION: &[u8] = b"alien-docker-source-input-v3";

/// Hex SHA-256 of everything a Docker source build reads: the build context Docker sends from
/// `src`, the Dockerfile, the build args, the build stage, and the target platforms.
///
/// The context is every entry under `src` that the build's `.dockerignore` keeps, by relative
/// path: file contents and executable bit, symlink targets as written, and directories. The
/// absolute location of `src` is not an input, so two machines with the same tree agree. Base
/// images named in `FROM` are hashed by name only, never by their registry digest.
pub async fn docker_source_input_hash(
    src: &Path,
    toolchain: &ToolchainConfig,
    settings: &BuildSettings,
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
    for platform in settings.get_targets() {
        field(&mut hasher, b"platform");
        field(&mut hasher, platform.runtime_platform_id().as_bytes());
    }

    let context = BuildContext {
        src,
        ignore: dockerignore(src, dockerfile).await?,
        output: output_inside(src, Path::new(&settings.output_directory))?,
    };
    let mut entries = Vec::new();
    context.collect(Path::new(""), &[], false, &mut entries)?;
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (portable, entry) in entries {
        match entry {
            Entry::Directory => {
                field(&mut hasher, b"dir");
                field(&mut hasher, portable.as_bytes());
            }
            Entry::File {
                relative,
                executable,
            } => {
                field(&mut hasher, b"file");
                field(&mut hasher, portable.as_bytes());
                field(&mut hasher, &[u8::from(executable)]);
                field(&mut hasher, &read(&src.join(relative)).await?);
            }
            Entry::Symlink(target) => {
                field(&mut hasher, b"symlink");
                field(&mut hasher, portable.as_bytes());
                field(&mut hasher, target.as_os_str().as_encoded_bytes());
            }
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

enum Entry {
    Directory,
    File { relative: PathBuf, executable: bool },
    Symlink(PathBuf),
}

struct BuildContext<'a> {
    src: &'a Path,
    ignore: DockerIgnore,
    /// The build writes its output here between the hash before a build and the one after it,
    /// so hashing it, or a directory that exists only to hold it, would report every build as
    /// edited mid-build. Docker still sends it.
    output: Option<PathBuf>,
}

impl BuildContext<'_> {
    /// `parent_matches` is `relative_dir`'s per-pattern ignore results, which its entries inherit.
    /// Like Docker's walk, a permission error on an excluded entry skips it instead of failing.
    fn collect(
        &self,
        relative_dir: &Path,
        parent_matches: &[bool],
        dir_excluded: bool,
        entries: &mut Vec<(String, Entry)>,
    ) -> Result<()> {
        let dir = self.src.join(relative_dir);
        let read_failed = |path: &Path| ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to read the build context for the source input hash".to_string(),
        };
        let skippable = |excluded: bool, error: &std::io::Error| {
            excluded && error.kind() == ErrorKind::PermissionDenied
        };
        let listing = match std::fs::read_dir(&dir) {
            Ok(listing) => listing,
            Err(error) if skippable(dir_excluded, &error) => return Ok(()),
            Err(error) => return Err(error).into_alien_error().context(read_failed(&dir)),
        };
        for dir_entry in listing {
            let dir_entry = match dir_entry {
                Ok(dir_entry) => dir_entry,
                Err(error) if skippable(dir_excluded, &error) => return Ok(()),
                Err(error) => return Err(error).into_alien_error().context(read_failed(&dir)),
            };
            let relative = relative_dir.join(dir_entry.file_name());
            if self.output.as_deref() == Some(relative.as_path()) {
                continue;
            }
            let path = dir_entry.path();
            let portable = portable_path(&relative);
            let (excluded, matches) = self.ignore.excludes(&portable, parent_matches);
            // Does not follow symlinks: Docker sends a symlink as the link itself.
            let metadata = match dir_entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if skippable(excluded, &error) => continue,
                Err(error) => return Err(error).into_alien_error().context(read_failed(&path)),
            };
            let holds_output = self
                .output
                .as_ref()
                .is_some_and(|output| output.starts_with(&relative));

            if metadata.is_dir() {
                if excluded && !self.ignore.has_exclusions() {
                    continue;
                }
                let sent_before = entries.len();
                self.collect(&relative, &matches, excluded, entries)?;
                // Docker sends an excluded directory when it holds a re-included entry.
                if (!excluded && !holds_output) || entries.len() > sent_before {
                    entries.push((portable, Entry::Directory));
                }
            } else if excluded {
                continue;
            } else if metadata.is_symlink() {
                let target = std::fs::read_link(&path)
                    .into_alien_error()
                    .context(read_failed(&path))?;
                entries.push((portable, Entry::Symlink(target)));
            } else if metadata.is_file() {
                entries.push((
                    portable,
                    Entry::File {
                        relative,
                        executable: is_executable(&metadata),
                    },
                ));
            }
        }
        Ok(())
    }
}

/// Docker reads `<dockerfile>.dockerignore` beside the Dockerfile when it exists, and the
/// context root's `.dockerignore` otherwise.
async fn dockerignore(src: &Path, dockerfile: &str) -> Result<DockerIgnore> {
    for path in [
        src.join(format!("{dockerfile}.dockerignore")),
        src.join(".dockerignore"),
    ] {
        match fs::read_to_string(&path).await {
            Ok(contents) => return DockerIgnore::parse(&contents, &path),
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(ErrorData::FileOperationFailed {
                        operation: "read file".to_string(),
                        file_path: path.display().to_string(),
                        reason: "Failed to read the build's ignore file".to_string(),
                    })
            }
        }
    }
    Ok(DockerIgnore::empty())
}

/// `output` relative to `src`, when the build writes its output inside the context.
fn output_inside(src: &Path, output: &Path) -> Result<Option<PathBuf>> {
    let canonical = |path: &Path| {
        std::fs::canonicalize(path)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "resolve path".to_string(),
                file_path: path.display().to_string(),
                reason: "Failed to resolve a path for the source input hash".to_string(),
            })
    };
    // Before the first build the output does not exist yet, so resolve its deepest existing
    // ancestor and append the rest.
    let mut existing = output;
    let mut missing = Vec::new();
    while !existing.exists() {
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                existing = parent;
            }
            _ => return Ok(None),
        }
        if existing.as_os_str().is_empty() {
            existing = Path::new(".");
        }
    }
    let mut output = canonical(existing)?;
    output.extend(missing.iter().rev());
    let src = canonical(src)?;
    Ok(output
        .strip_prefix(&src)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(Path::to_path_buf))
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    false
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
    use crate::settings::PlatformBuildSettings;
    use alien_core::BinaryTarget;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn docker(dockerfile: Option<&str>) -> ToolchainConfig {
        ToolchainConfig::Docker {
            dockerfile: dockerfile.map(str::to_string),
            build_args: None,
            target: None,
        }
    }

    fn settings(targets: &[BinaryTarget], output_directory: &Path) -> BuildSettings {
        BuildSettings {
            platform: PlatformBuildSettings::Machines {},
            output_directory: output_directory.display().to_string(),
            targets: Some(targets.to_vec()),
            cache_url: None,
            override_base_image: None,
            debug_mode: false,
            rebuild: false,
            pull_base_images: false,
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
        docker_source_input_hash(
            dir,
            toolchain,
            &settings(&[BinaryTarget::LinuxArm64], &dir.join(".alien")),
        )
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
            docker_source_input_hash(
                dir.path(),
                &docker(None),
                &settings(&[BinaryTarget::LinuxX64], &dir.path().join(".alien")),
            )
            .await
            .unwrap(),
            default
        );
    }

    #[tokio::test]
    async fn every_directory_docker_sends_is_an_input() {
        let dir = sandbox_tree();
        let before = hash(dir.path(), &docker(None)).await;

        std::fs::write(dir.path().join("node_modules/dep/index.js"), "two").unwrap();
        let after_node_modules = hash(dir.path(), &docker(None)).await;
        assert_ne!(after_node_modules, before);

        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_ne!(hash(dir.path(), &docker(None)).await, after_node_modules);
    }

    #[tokio::test]
    async fn dockerignored_paths_are_not_inputs() {
        let dir = sandbox_tree();
        std::fs::write(dir.path().join(".dockerignore"), "node_modules\n**/*.log\n").unwrap();
        std::fs::write(dir.path().join("app/debug.log"), "one").unwrap();
        let before = hash(dir.path(), &docker(None)).await;

        std::fs::write(dir.path().join("node_modules/dep/index.js"), "two").unwrap();
        std::fs::write(dir.path().join("app/debug.log"), "two").unwrap();
        assert_eq!(hash(dir.path(), &docker(None)).await, before);

        std::fs::write(dir.path().join(".dockerignore"), "**/*.log\n").unwrap();
        assert_ne!(hash(dir.path(), &docker(None)).await, before);
    }

    #[tokio::test]
    async fn a_dockerfile_specific_ignore_file_replaces_the_root_one() {
        let dir = sandbox_tree();
        std::fs::write(dir.path().join(".dockerignore"), "app\n").unwrap();
        std::fs::write(
            dir.path().join("Sandbox.dockerfile.dockerignore"),
            "node_modules\n",
        )
        .unwrap();
        let toolchain = docker(Some("Sandbox.dockerfile"));
        let before = hash(dir.path(), &toolchain).await;

        std::fs::write(dir.path().join("node_modules/dep/index.js"), "two").unwrap();
        assert_eq!(hash(dir.path(), &toolchain).await, before);
        std::fs::write(dir.path().join("app/bin/run.sh"), "#!/bin/sh\necho bye\n").unwrap();
        assert_ne!(hash(dir.path(), &toolchain).await, before);
    }

    #[tokio::test]
    async fn a_file_reincluded_under_an_ignored_directory_is_an_input() {
        let dir = sandbox_tree();
        std::fs::write(
            dir.path().join(".dockerignore"),
            "node_modules\n!node_modules/dep/index.js\n",
        )
        .unwrap();
        let before = hash(dir.path(), &docker(None)).await;

        std::fs::write(dir.path().join("node_modules/dep/index.js"), "two").unwrap();
        assert_ne!(hash(dir.path(), &docker(None)).await, before);
    }

    #[tokio::test]
    async fn a_pattern_skipped_at_a_reincluded_directory_does_not_drop_its_files() {
        let dir = sandbox_tree();
        std::fs::write(
            dir.path().join(".dockerignore"),
            "node_modules\n!node_modules/dep\n**/node_modules\n",
        )
        .unwrap();
        let before = hash(dir.path(), &docker(None)).await;

        std::fs::write(dir.path().join("node_modules/dep/index.js"), "two").unwrap();
        assert_ne!(hash(dir.path(), &docker(None)).await, before);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_unreadable_directory_fails_the_hash_only_when_docker_would_send_it() {
        use std::os::unix::fs::PermissionsExt;

        let dir = sandbox_tree();
        std::fs::write(dir.path().join(".dockerignore"), "pgdata\n!pgdata/keep\n").unwrap();
        let pgdata = dir.path().join("pgdata");
        std::fs::create_dir(&pgdata).unwrap();
        std::fs::set_permissions(&pgdata, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root reads through mode 000, so there is no unreadable directory to test.
        if std::fs::read_dir(&pgdata).is_ok() {
            return;
        }
        let excluded = docker_source_input_hash(
            dir.path(),
            &docker(None),
            &settings(&[BinaryTarget::LinuxArm64], &dir.path().join(".alien")),
        )
        .await;

        std::fs::write(dir.path().join(".dockerignore"), "").unwrap();
        let sent = docker_source_input_hash(
            dir.path(),
            &docker(None),
            &settings(&[BinaryTarget::LinuxArm64], &dir.path().join(".alien")),
        )
        .await;
        std::fs::set_permissions(&pgdata, std::fs::Permissions::from_mode(0o755)).unwrap();

        excluded.expect("Docker skips an excluded directory it cannot read");
        sent.expect_err("Docker fails on a directory it sends but cannot read");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_retargeted_symlink_changes_the_hash() {
        let dir = sandbox_tree();
        let link = dir.path().join("app/current");
        std::os::unix::fs::symlink("bin", &link).unwrap();
        let before = hash(dir.path(), &docker(None)).await;

        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink("../node_modules", &link).unwrap();
        assert_ne!(hash(dir.path(), &docker(None)).await, before);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn making_a_file_executable_changes_the_hash() {
        use std::os::unix::fs::PermissionsExt;

        let dir = sandbox_tree();
        let script = dir.path().join("app/bin/run.sh");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
        let before = hash(dir.path(), &docker(None)).await;

        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_ne!(hash(dir.path(), &docker(None)).await, before);
    }

    #[tokio::test]
    async fn the_build_output_inside_the_source_is_not_an_input() {
        for existing_before_the_build in [None, Some(".alien"), Some(".alien/remote-sandbox")] {
            let dir = sandbox_tree();
            if let Some(existing) = existing_before_the_build {
                std::fs::create_dir_all(dir.path().join(existing)).unwrap();
            }
            let output = dir.path().join(".alien/remote-sandbox");
            let settings = settings(&[BinaryTarget::LinuxArm64], &output);
            let before = docker_source_input_hash(dir.path(), &docker(None), &settings)
                .await
                .unwrap();

            std::fs::create_dir_all(output.join("build/aws")).unwrap();
            std::fs::write(output.join("build/aws/stack.json"), "{}").unwrap();
            let after = docker_source_input_hash(dir.path(), &docker(None), &settings)
                .await
                .unwrap();
            assert_eq!(after, before, "{existing_before_the_build:?} existed first");
        }
    }

    #[tokio::test]
    async fn a_missing_dockerfile_is_an_error() {
        let dir = sandbox_tree();
        docker_source_input_hash(
            dir.path(),
            &docker(Some("Missing.dockerfile")),
            &settings(&[BinaryTarget::LinuxArm64], &dir.path().join(".alien")),
        )
        .await
        .expect_err("a build without its Dockerfile has no inputs to hash");
    }
}
