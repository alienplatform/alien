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
pub mod rust;
pub mod typescript;

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
    /// Whether this is building a Container resource (vs Worker)
    /// Containers need alien-worker-runtime in the image on all platforms for command support
    pub is_container: bool,
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
    /// Runtime command for the container
    pub runtime_command: Vec<String>,
}

/// Trait for implementing programming language toolchains
#[async_trait]
pub trait Toolchain: Send + Sync {
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
