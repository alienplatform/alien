use alien_core::{BinaryTarget, ToolchainConfig};
use async_trait::async_trait;
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
    /// Target platform name (aws, gcp, azure, etc.)
    pub platform_name: String,
    /// Whether to build in debug mode (faster builds, larger binaries)
    pub debug_mode: bool,
    /// Whether this is building a Container resource (vs Function)
    /// Containers need alien-runtime in the image on all platforms for command support
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
        files_to_package: Vec<(PathBuf, String)>,
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
