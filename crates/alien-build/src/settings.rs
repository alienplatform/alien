use alien_core::{BinaryTarget, Platform};
use dockdash::{Arch, PushOptions};

/// Extension trait to add dockdash-specific functionality to BinaryTarget
pub trait BinaryTargetExt {
    /// Convert to dockdash Arch enum for image building
    fn to_dockdash_arch(&self) -> Arch;
}

impl BinaryTargetExt for BinaryTarget {
    fn to_dockdash_arch(&self) -> Arch {
        match self {
            BinaryTarget::WindowsX64 | BinaryTarget::LinuxX64 => Arch::Amd64,
            BinaryTarget::LinuxArm64 | BinaryTarget::DarwinArm64 => Arch::ARM64,
        }
    }
}

/// Enum to hold platform-specific build settings.
#[derive(Debug, Clone)]
pub enum PlatformBuildSettings {
    Aws {
        /// The default AWS managing account ID.
        managing_account_id: Option<String>,
    },
    Gcp {},
    Azure {},
    Kubernetes {},
    Local {},
    Test {},
}

impl PlatformBuildSettings {
    /// Returns the corresponding Platform enum variant.
    pub fn platform(&self) -> Platform {
        match self {
            PlatformBuildSettings::Aws { .. } => Platform::Aws,
            PlatformBuildSettings::Gcp { .. } => Platform::Gcp,
            PlatformBuildSettings::Azure { .. } => Platform::Azure,
            PlatformBuildSettings::Kubernetes { .. } => Platform::Kubernetes,
            PlatformBuildSettings::Local { .. } => Platform::Local,
            PlatformBuildSettings::Test { .. } => Platform::Test,
        }
    }
}

/// Configuration for the build system.
#[derive(Debug, Clone)]
pub struct BuildSettings {
    /// The cloud platform and its specific settings for the build.
    pub platform: PlatformBuildSettings,
    /// The base directory where the built stack and OCI image tarballs will be saved.
    /// The final output will be in a subdirectory named after the platform (e.g. `<output_directory>/aws`).
    pub output_directory: String,
    /// Target OS/architecture combinations to build for. If None, uses platform-specific defaults.
    /// AWS default: [LinuxArm64]
    /// GCP default: [LinuxX64]
    /// Azure default: [LinuxX64]
    /// Kubernetes default: [LinuxArm64]
    /// Local default: [WindowsX64, LinuxX64, LinuxArm64, DarwinArm64]
    pub targets: Option<Vec<BinaryTarget>>,
    /// Optional cache URL for build caching (e.g., s3://bucket/path, gcs://bucket/path).
    /// If None, no build caching will be used.
    pub cache_url: Option<String>,
    /// Optional override for the base image used in container builds.
    /// If provided, this will override the default base image from toolchains.
    /// Useful for testing or using custom base images.
    pub override_base_image: Option<String>,
    /// Build in debug mode for faster builds (default: false for release builds).
    /// Debug builds are faster but produce larger binaries.
    pub debug_mode: bool,
}

/// Settings for pushing built images to a registry.
#[derive(Debug, Clone)]
pub struct PushSettings {
    /// The repository (e.g., Docker Hub namespace or private registry URL)
    /// where the built function images will be pushed.
    pub repository: String,
    /// Push options (auth, protocol, etc.)
    pub options: PushOptions,
}

impl BuildSettings {
    /// Get the build targets (OS/arch combinations) for this build.
    /// Returns specified targets or platform-specific defaults if not specified.
    pub fn get_targets(&self) -> Vec<BinaryTarget> {
        self.targets
            .as_ref()
            .cloned()
            .unwrap_or_else(|| BinaryTarget::defaults_for_platform(self.platform.platform()))
    }
}
