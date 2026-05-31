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
    Kubernetes {
        /// Cloud platform backing the Kubernetes cluster, when known.
        ///
        /// This does not change the runtime platform. It only lets builds pick
        /// the same default image architecture as the cluster node pool.
        base_platform: Option<Platform>,
    },
    Local {},
    Test {},
}

impl PlatformBuildSettings {
    /// Platform where the built application will run.
    ///
    /// For managed Kubernetes this is always [`Platform::Kubernetes`]. The
    /// optional base cloud only influences build defaults such as image
    /// architecture; it is not the application runtime platform.
    pub fn runtime_platform(&self) -> Platform {
        match self {
            PlatformBuildSettings::Aws { .. } => Platform::Aws,
            PlatformBuildSettings::Gcp { .. } => Platform::Gcp,
            PlatformBuildSettings::Azure { .. } => Platform::Azure,
            PlatformBuildSettings::Kubernetes { .. } => Platform::Kubernetes,
            PlatformBuildSettings::Local { .. } => Platform::Local,
            PlatformBuildSettings::Test { .. } => Platform::Test,
        }
    }

    /// Cloud platform backing a managed Kubernetes cluster, when known.
    pub fn base_platform(&self) -> Option<Platform> {
        match self {
            PlatformBuildSettings::Kubernetes { base_platform } => *base_platform,
            PlatformBuildSettings::Aws { .. }
            | PlatformBuildSettings::Gcp { .. }
            | PlatformBuildSettings::Azure { .. }
            | PlatformBuildSettings::Local { .. }
            | PlatformBuildSettings::Test { .. } => None,
        }
    }

    /// Backward-compatible alias for the runtime platform.
    pub fn platform(&self) -> Platform {
        self.runtime_platform()
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
    /// Kubernetes default: [LinuxX64, LinuxArm64], or the base cloud default when base_platform is set
    /// Local default: [current host target]
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
    /// Human-readable push destination for terminal progress.
    pub destination_label: Option<String>,
    /// Push options (auth, protocol, etc.)
    pub options: PushOptions,
}

impl BuildSettings {
    /// Get the build targets (OS/arch combinations) for this build.
    /// Returns specified targets or platform-specific defaults if not specified.
    pub fn get_targets(&self) -> Vec<BinaryTarget> {
        self.targets.as_ref().cloned().unwrap_or_else(|| {
            if let Some(base_platform) = self.platform.base_platform() {
                return BinaryTarget::defaults_for_platform(base_platform);
            }

            BinaryTarget::defaults_for_platform(self.platform.runtime_platform())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(platform: PlatformBuildSettings) -> BuildSettings {
        BuildSettings {
            platform,
            output_directory: "target/test-build-settings".to_string(),
            targets: None,
            cache_url: None,
            override_base_image: None,
            debug_mode: true,
        }
    }

    #[test]
    fn kubernetes_without_base_platform_builds_all_linux_targets() {
        let settings = settings(PlatformBuildSettings::Kubernetes {
            base_platform: None,
        });

        assert_eq!(
            settings.get_targets(),
            vec![BinaryTarget::LinuxX64, BinaryTarget::LinuxArm64]
        );
    }

    #[test]
    fn kubernetes_with_base_platform_uses_base_cloud_default_targets() {
        let eks = settings(PlatformBuildSettings::Kubernetes {
            base_platform: Some(Platform::Aws),
        });
        let gke = settings(PlatformBuildSettings::Kubernetes {
            base_platform: Some(Platform::Gcp),
        });
        let aks = settings(PlatformBuildSettings::Kubernetes {
            base_platform: Some(Platform::Azure),
        });

        assert_eq!(eks.get_targets(), vec![BinaryTarget::LinuxArm64]);
        assert_eq!(gke.get_targets(), vec![BinaryTarget::LinuxX64]);
        assert_eq!(aks.get_targets(), vec![BinaryTarget::LinuxX64]);
    }

    #[test]
    fn kubernetes_with_base_platform_keeps_kubernetes_runtime_platform() {
        let settings = PlatformBuildSettings::Kubernetes {
            base_platform: Some(Platform::Gcp),
        };

        assert_eq!(settings.runtime_platform(), Platform::Kubernetes);
        assert_eq!(settings.platform(), Platform::Kubernetes);
        assert_eq!(settings.base_platform(), Some(Platform::Gcp));
    }
}
