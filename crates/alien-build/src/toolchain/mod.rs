use crate::error::{ErrorData, Result};
use alien_core::{BinaryTarget, ToolchainConfig};
use alien_error::{AlienError, ContextError, IntoAlienError};
use async_trait::async_trait;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod cache_utils;
pub mod docker;
mod native_addon;
pub mod rust;
pub mod typescript;

/// The kind of compute workload a source build is for.
///
/// The kind determines the image shape: only Worker images bundle
/// `alien-worker-runtime`; Container and Daemon images run the compiled
/// binary directly as the image entrypoint (no wrapper, no `--` separator).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkloadKind {
    /// Task-dispatch workload that runs behind `alien-worker-runtime`.
    Worker,
    /// Long-running containerized service; its binary is the image entrypoint.
    Container,
    /// Long-lived native process (DaemonSet on Kubernetes, host process on
    /// Local); its binary is the image entrypoint.
    Daemon,
}

impl WorkloadKind {
    /// Lowercase resource-type name used in events, logs, and cache keys.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Container => "container",
            Self::Daemon => "daemon",
        }
    }
}

/// Context provided to toolchains during build operations
#[derive(Debug)]
pub struct ToolchainContext {
    /// Source directory being built
    pub src_dir: PathBuf,
    /// Build output directory for the final compiled binary.
    /// This is inside .alien/build/{platform}/{function}/, NOT inside the source directory.
    pub build_dir: PathBuf,
    /// Object store for caching (S3, GCS, ABS, or local) - optional
    pub cache_store: Option<Arc<dyn object_store::ObjectStore>>,
    /// Cache prefix for this project - only used when cache_store is Some
    pub cache_prefix: String,
    /// Target OS/architecture to build for
    pub build_target: BinaryTarget,
    /// Runtime platform name (aws, gcp, azure, kubernetes, local, etc.)
    pub runtime_platform_name: String,
    /// Whether to build in debug mode (faster builds, larger binaries)
    pub debug_mode: bool,
    /// Which compute workload this build is for (decides the image shape).
    pub workload: WorkloadKind,
}

impl ToolchainContext {
    /// Whether the image must bundle `alien-worker-runtime`.
    ///
    /// Only Worker images include the runtime, and only on non-local
    /// platforms — on Local the runtime is embedded in the co-located agent,
    /// which runs the extracted binary itself.
    pub fn needs_worker_runtime_in_image(&self) -> bool {
        self.workload == WorkloadKind::Worker && self.runtime_platform_name != "local"
    }

    /// Whether the built binary is extracted from the image and run directly
    /// as a host process (Local Workers under the agent's embedded runtime and
    /// Local Daemons), rather than being executed inside a container.
    ///
    /// Containers always run under Docker/Kubernetes, even on Local.
    pub fn runs_as_host_process(&self) -> bool {
        self.runtime_platform_name == "local"
            && matches!(self.workload, WorkloadKind::Worker | WorkloadKind::Daemon)
    }
}

/// Specification for a file to add to an OCI layer
#[derive(Debug, Clone)]
pub struct FileSpec {
    /// Path to the file on the host system
    pub host_path: PathBuf,
    /// Path inside the container (e.g., "./app" or "/app/server.js")
    pub container_path: String,
    /// Unix file mode (e.g., 0o755 for executable, 0o644 for regular files)
    /// If None, uses the source file's mode
    pub mode: Option<u32>,
}

/// Specification for a layer in the OCI image
#[derive(Debug, Clone)]
pub struct LayerSpec {
    /// Files to include in this layer
    pub files: Vec<FileSpec>,
    /// Description of this layer for logging
    pub description: String,
}

/// Strategy for building the OCI image
#[derive(Debug, Clone)]
pub enum ImageBuildStrategy {
    /// Build from a base image pulled from a registry (cloud platforms)
    FromBaseImage {
        /// Base images to try (in priority order - will try each until one succeeds)
        base_images: Vec<String>,
        /// Files to package into the image
        files_to_package: Vec<FileSpec>,
    },

    /// Build from scratch with explicit layer control (local platform)
    FromScratch {
        /// Layers to add to the image
        /// Ordered for optimal caching: [runtime_binary, app_code, ...]
        layers: Vec<LayerSpec>,
    },

    /// Toolchain produced a complete OCI tarball - use it as-is
    /// Used by Docker toolchain which runs `docker build` to produce a full image
    CompleteOCITarball {
        /// Path to the pre-built OCI tarball (relative to build_dir)
        tarball_path: PathBuf,
    },
}

/// Output from a toolchain build operation
#[derive(Debug, Clone)]
pub struct ToolchainOutput {
    /// Strategy for building the OCI image
    pub build_strategy: ImageBuildStrategy,
    /// Image `ENTRYPOINT` override. `Some` replaces (and clears any `CMD`
    /// from) the base image — used by Container/Daemon source images whose
    /// compiled binary is the direct entrypoint. `None` keeps the base
    /// image's entrypoint (e.g. alien-base's `/app/alien-worker-runtime`).
    pub entrypoint: Option<Vec<String>>,
    /// Image `CMD` for the container
    pub runtime_command: Vec<String>,
}

/// Base image for Worker source images. It bundles `alien-worker-runtime`
/// with `ENTRYPOINT ["/app/alien-worker-runtime"]`; the Worker image's CMD is
/// `["--", "./<binary>"]` (the `--` separator is required by the runtime CLI).
pub(crate) const WORKER_BASE_IMAGES: &[&str] = &["ghcr.io/alienplatform/alien-base:latest"];

/// Base images for Container/Daemon source images, in fallback order: a plain
/// glibc userland (the same Wolfi base that alien-base builds on) with no
/// runtime and no entrypoint. The compiled binary is set as the direct
/// entrypoint. TypeScript binaries produced by `bun build --compile` are
/// glibc-linked, so `FROM scratch` is not an option here.
pub(crate) const DIRECT_BASE_IMAGES: &[&str] = &[
    "cgr.dev/chainguard/wolfi-base:latest",
    "docker.io/chainguard/wolfi-base:latest",
];

/// Assemble the [`ToolchainOutput`] for a compiled single-binary workload.
///
/// The image shape depends on the workload kind and platform:
/// - Worker on a non-local platform: alien-base image (runtime entrypoint) with
///   CMD `["--", "./<binary>"]`.
/// - Local Worker / local Daemon: from-scratch image; the binary is extracted
///   and run as a host process by the agent, so no userland is needed.
/// - Container (all platforms) / non-local Daemon: Wolfi base image with the
///   binary as the direct `ENTRYPOINT` — no runtime, no `--` separator.
///
/// `extra_layers` (e.g. a Rust project's `vendor/` assets) are appended after
/// the binary layer on the from-scratch path and flattened into
/// `files_to_package` on the base-image paths.
pub(crate) fn image_output_for_binary(
    context: &ToolchainContext,
    binary_path: PathBuf,
    binary_filename: &str,
    extra_layers: Vec<LayerSpec>,
) -> ToolchainOutput {
    let binary_file = FileSpec {
        host_path: binary_path,
        container_path: format!("./{}", binary_filename),
        mode: Some(0o755), // Executable
    };

    if context.runs_as_host_process() {
        // Local Worker/Daemon: the agent extracts the image and runs the
        // binary directly (Workers under the agent's embedded runtime).
        let mut layers = vec![LayerSpec {
            files: vec![binary_file],
            description: "Application binary".to_string(),
        }];
        layers.extend(extra_layers);

        return ToolchainOutput {
            build_strategy: ImageBuildStrategy::FromScratch { layers },
            entrypoint: None,
            runtime_command: vec![format!("./{}", binary_filename)],
        };
    }

    let mut files_to_package = vec![binary_file];
    files_to_package.extend(extra_layers.into_iter().flat_map(|layer| layer.files));

    if context.needs_worker_runtime_in_image() {
        // Worker: run behind alien-worker-runtime. The base image ENTRYPOINT is
        // ["/app/alien-worker-runtime"], so CMD must start with the "--"
        // separator followed by the application binary.
        return ToolchainOutput {
            build_strategy: ImageBuildStrategy::FromBaseImage {
                base_images: WORKER_BASE_IMAGES.iter().map(|s| s.to_string()).collect(),
                files_to_package,
            },
            entrypoint: None,
            runtime_command: vec!["--".to_string(), format!("./{}", binary_filename)],
        };
    }

    // Container / non-local Daemon: the compiled binary IS the entrypoint.
    // The explicit entrypoint also clears any entrypoint/CMD inherited from
    // the base image (including a user-supplied --override-base-image).
    ToolchainOutput {
        build_strategy: ImageBuildStrategy::FromBaseImage {
            base_images: DIRECT_BASE_IMAGES.iter().map(|s| s.to_string()).collect(),
            files_to_package,
        },
        entrypoint: Some(vec![format!("/app/{}", binary_filename)]),
        runtime_command: vec![],
    }
}

/// Trait for implementing programming language toolchains
#[async_trait]
pub trait Toolchain: Send + Sync {
    /// Validate that `src_dir` looks like a project this toolchain can build.
    ///
    /// `build_resource` calls this once, before the artifact-cache hash walks
    /// the source tree, with the resource's real name — so a missing or
    /// invalid project fails with a clear config error instead of an I/O
    /// error from the hasher. The default accepts anything (Docker validates
    /// through `docker build` itself).
    fn validate_source(&self, src_dir: &Path, resource_name: &str) -> crate::error::Result<()> {
        let _ = (src_dir, resource_name);
        Ok(())
    }

    /// Build the source code on the host system with caching
    async fn build(&self, context: &ToolchainContext) -> crate::error::Result<ToolchainOutput>;

    /// Dev command for development - takes source directory to detect package manager/runtime
    fn dev_command(&self, src_dir: &Path) -> Vec<String>;
}

/// Factory function to create a toolchain from configuration
pub fn create_toolchain(config: &ToolchainConfig) -> Box<dyn Toolchain> {
    match config {
        ToolchainConfig::Rust { binary_name } => Box::new(rust::RustToolchain {
            binary_name: binary_name.clone(),
        }),
        ToolchainConfig::TypeScript { binary_name } => Box::new(typescript::TypeScriptToolchain {
            binary_name: binary_name.clone(),
        }),
        ToolchainConfig::Docker {
            dockerfile,
            build_args,
            target,
        } => Box::new(docker::DockerToolchain {
            dockerfile: dockerfile.clone(),
            build_args: build_args.clone(),
            target: target.clone(),
        }),
    }
}

pub(crate) fn executable_format_error(
    path: &Path,
    target: BinaryTarget,
) -> std::result::Result<Option<String>, std::io::Error> {
    let mut file = File::open(path)?;
    let mut header = [0_u8; 4];
    let bytes_read = file.read(&mut header)?;

    if bytes_read < 4 {
        return Ok(Some(format!(
            "compiled binary is too small to be a {} executable",
            target.runtime_platform_id()
        )));
    }

    let is_valid = match target {
        BinaryTarget::LinuxX64 | BinaryTarget::LinuxArm64 => &header == b"\x7fELF",
        BinaryTarget::WindowsX64 => header[0] == b'M' && header[1] == b'Z',
        BinaryTarget::DarwinArm64 => matches!(
            header,
            [0xca, 0xfe, 0xba, 0xbe]
                | [0xbe, 0xba, 0xfe, 0xca]
                | [0xfe, 0xed, 0xfa, 0xcf]
                | [0xcf, 0xfa, 0xed, 0xfe]
        ),
    };

    if is_valid {
        Ok(None)
    } else {
        Ok(Some(format!(
            "compiled binary has invalid executable format for {} (first bytes: {:02x} {:02x} {:02x} {:02x})",
            target.runtime_platform_id(),
            header[0],
            header[1],
            header[2],
            header[3]
        )))
    }
}

pub(crate) fn validate_executable_format(
    path: &Path,
    target: BinaryTarget,
    resource_name: &str,
) -> Result<()> {
    match executable_format_error(path, target).into_alien_error() {
        Ok(Some(reason)) => Err(AlienError::new(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!("{}: {}", path.display(), reason),
            build_output: None,
        })),
        Ok(None) => Ok(()),
        Err(error) => Err(error.context(ErrorData::ImageBuildFailed {
            resource_name: resource_name.to_string(),
            reason: format!("Failed to inspect compiled binary at {}", path.display()),
            build_output: None,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_header(bytes: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("temp file");
        file.write_all(bytes).expect("write header");
        file
    }

    fn context(workload: WorkloadKind, platform: &str) -> ToolchainContext {
        ToolchainContext {
            src_dir: PathBuf::from("/src"),
            build_dir: PathBuf::from("/build"),
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: BinaryTarget::LinuxX64,
            runtime_platform_name: platform.to_string(),
            debug_mode: false,
            workload,
        }
    }

    fn binary_output(workload: WorkloadKind, platform: &str) -> ToolchainOutput {
        image_output_for_binary(
            &context(workload, platform),
            PathBuf::from("/build/app"),
            "app",
            vec![],
        )
    }

    #[test]
    fn worker_cloud_image_bundles_runtime_with_separator_cmd() {
        let output = binary_output(WorkloadKind::Worker, "aws");

        match &output.build_strategy {
            ImageBuildStrategy::FromBaseImage { base_images, .. } => {
                assert_eq!(
                    base_images,
                    &vec!["ghcr.io/alienplatform/alien-base:latest".to_string()],
                    "Worker images must build on alien-base (runtime entrypoint)"
                );
            }
            other => panic!("Worker cloud image should build from alien-base, got {other:?}"),
        }
        assert_eq!(output.entrypoint, None, "keep the runtime entrypoint");
        assert_eq!(
            output.runtime_command,
            vec!["--".to_string(), "./app".to_string()],
            "Worker CMD needs the runtime CLI's -- separator"
        );
    }

    #[test]
    fn local_worker_and_daemon_are_from_scratch_host_binaries() {
        for workload in [WorkloadKind::Worker, WorkloadKind::Daemon] {
            let output = binary_output(workload, "local");

            assert!(
                matches!(
                    output.build_strategy,
                    ImageBuildStrategy::FromScratch { .. }
                ),
                "local {} should package from scratch (agent runs the binary)",
                workload.as_str()
            );
            assert_eq!(output.entrypoint, None);
            assert_eq!(output.runtime_command, vec!["./app".to_string()]);
        }
    }

    #[test]
    fn containers_and_cloud_daemons_get_direct_entrypoint_without_runtime() {
        for (workload, platform) in [
            (WorkloadKind::Container, "local"),
            (WorkloadKind::Container, "aws"),
            (WorkloadKind::Container, "kubernetes"),
            (WorkloadKind::Daemon, "kubernetes"),
        ] {
            let output = binary_output(workload, platform);

            match &output.build_strategy {
                ImageBuildStrategy::FromBaseImage { base_images, .. } => {
                    assert!(
                        base_images
                            .iter()
                            .all(|image| image.contains("wolfi-base")),
                        "{} on {platform} must not include alien-worker-runtime; got {base_images:?}",
                        workload.as_str()
                    );
                }
                other => panic!(
                    "{} on {platform} should build from a plain base image, got {other:?}",
                    workload.as_str()
                ),
            }
            assert_eq!(
                output.entrypoint,
                Some(vec!["/app/app".to_string()]),
                "{} on {platform}: the compiled binary IS the entrypoint",
                workload.as_str()
            );
            assert!(
                output.runtime_command.is_empty(),
                "{} on {platform}: no CMD, no `--` separator",
                workload.as_str()
            );
        }
    }

    #[test]
    fn direct_entrypoint_images_flatten_extra_layers_into_packaged_files() {
        let output = image_output_for_binary(
            &context(WorkloadKind::Daemon, "kubernetes"),
            PathBuf::from("/build/agent"),
            "agent",
            vec![LayerSpec {
                files: vec![FileSpec {
                    host_path: PathBuf::from("/src/vendor"),
                    container_path: "./vendor".to_string(),
                    mode: None,
                }],
                description: "Vendor assets".to_string(),
            }],
        );

        match &output.build_strategy {
            ImageBuildStrategy::FromBaseImage {
                files_to_package, ..
            } => {
                let paths: Vec<&str> = files_to_package
                    .iter()
                    .map(|f| f.container_path.as_str())
                    .collect();
                assert_eq!(paths, vec!["./agent", "./vendor"]);
            }
            other => panic!("expected FromBaseImage, got {other:?}"),
        }
    }

    #[test]
    fn validates_linux_elf_headers() {
        let file = write_header(b"\x7fELFrest");
        assert_eq!(
            executable_format_error(file.path(), BinaryTarget::LinuxX64).unwrap(),
            None
        );
    }

    #[test]
    fn rejects_corrupt_linux_binaries() {
        let file = write_header(&[0, 0, 0, 0, 1, 2, 3, 4]);
        let error = executable_format_error(file.path(), BinaryTarget::LinuxX64)
            .unwrap()
            .expect("expected invalid format");
        assert!(error.contains("invalid executable format"));
        assert!(error.contains("00 00 00 00"));
    }

    #[test]
    fn validates_windows_mz_headers() {
        let file = write_header(b"MZrest");
        assert_eq!(
            executable_format_error(file.path(), BinaryTarget::WindowsX64).unwrap(),
            None
        );
    }

    #[test]
    fn validates_macos_mach_o_headers() {
        let file = write_header(&[0xcf, 0xfa, 0xed, 0xfe, 0, 0]);
        assert_eq!(
            executable_format_error(file.path(), BinaryTarget::DarwinArm64).unwrap(),
            None
        );
    }
}
