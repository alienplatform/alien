//! Cross-compilation build target types
//!
//! These types identify target OS/architecture combinations used by the open-source
//! build system (alien-build) for cross-compilation.

use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Types of source binaries used for package building
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SourceBinaryType {
    /// alien-deploy binary
    Cli,
    /// alien-terraform binary
    Terraform,
    /// alien-agent binary
    Agent,
}

impl SourceBinaryType {
    /// Returns the binary filename (without extension)
    pub fn binary_name(&self) -> &'static str {
        match self {
            SourceBinaryType::Cli => "alien-deploy",
            SourceBinaryType::Terraform => "alien-terraform",
            SourceBinaryType::Agent => "alien-agent",
        }
    }
}

impl std::fmt::Display for SourceBinaryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceBinaryType::Cli => write!(f, "cli"),
            SourceBinaryType::Terraform => write!(f, "terraform"),
            SourceBinaryType::Agent => write!(f, "agent"),
        }
    }
}

/// Target OS and architecture for compiled binaries.
///
/// Used as keys in package output maps (CLI binaries, Terraform providers, etc.)
/// and for cross-compilation target selection during builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum BinaryTarget {
    /// Windows x64 (x86_64-pc-windows-gnu)
    WindowsX64,
    /// Linux x86_64 (musl)
    LinuxX64,
    /// Linux ARM64 (musl)
    LinuxArm64,
    /// macOS ARM64 (Apple Silicon)
    DarwinArm64,
}

impl BinaryTarget {
    /// All supported binary targets
    pub const ALL: &'static [BinaryTarget] = &[
        BinaryTarget::WindowsX64,
        BinaryTarget::LinuxX64,
        BinaryTarget::LinuxArm64,
        BinaryTarget::DarwinArm64,
    ];

    /// Linux-only targets (for container/operator builds)
    pub const LINUX: &'static [BinaryTarget] = &[BinaryTarget::LinuxX64, BinaryTarget::LinuxArm64];

    /// Get the Rust target triple for this platform
    pub fn rust_target_triple(&self) -> &'static str {
        match self {
            Self::WindowsX64 => "x86_64-pc-windows-gnu",
            Self::LinuxX64 => "x86_64-unknown-linux-musl",
            Self::LinuxArm64 => "aarch64-unknown-linux-musl",
            Self::DarwinArm64 => "aarch64-apple-darwin",
        }
    }

    /// Get the binary extension for this platform
    pub fn binary_extension(&self) -> &'static str {
        match self {
            Self::WindowsX64 => ".exe",
            _ => "",
        }
    }

    /// Get the platform identifier for runtime downloads (e.g., "linux-x64")
    pub fn runtime_platform_id(&self) -> &'static str {
        match self {
            Self::WindowsX64 => "windows-x64",
            Self::LinuxX64 => "linux-x64",
            Self::LinuxArm64 => "linux-aarch64",
            Self::DarwinArm64 => "darwin-aarch64",
        }
    }

    /// Get the OCI os string for this target
    pub fn oci_os(&self) -> &'static str {
        match self {
            Self::WindowsX64 => "windows",
            Self::LinuxX64 | Self::LinuxArm64 => "linux",
            Self::DarwinArm64 => "darwin",
        }
    }

    /// Get the OCI architecture string for this target
    pub fn oci_arch(&self) -> &'static str {
        match self {
            Self::WindowsX64 | Self::LinuxX64 => "amd64",
            Self::LinuxArm64 | Self::DarwinArm64 => "arm64",
        }
    }

    /// Get the Bun cross-compilation target for `bun build --compile --target`
    pub fn bun_target(&self) -> &'static str {
        match self {
            Self::WindowsX64 => "bun-windows-x64",
            Self::LinuxX64 => "bun-linux-x64",
            Self::LinuxArm64 => "bun-linux-arm64",
            Self::DarwinArm64 => "bun-darwin-arm64",
        }
    }

    /// Terraform registry platform key (os_arch format)
    pub fn terraform_key(&self) -> &'static str {
        match self {
            BinaryTarget::LinuxX64 => "linux_amd64",
            BinaryTarget::LinuxArm64 => "linux_arm64",
            BinaryTarget::DarwinArm64 => "darwin_arm64",
            BinaryTarget::WindowsX64 => "windows_amd64",
        }
    }

    /// Terraform OS string
    pub fn terraform_os(&self) -> &'static str {
        match self {
            BinaryTarget::LinuxX64 | BinaryTarget::LinuxArm64 => "linux",
            BinaryTarget::DarwinArm64 => "darwin",
            BinaryTarget::WindowsX64 => "windows",
        }
    }

    /// Terraform architecture string
    pub fn terraform_arch(&self) -> &'static str {
        match self {
            BinaryTarget::LinuxX64 | BinaryTarget::WindowsX64 => "amd64",
            BinaryTarget::LinuxArm64 | BinaryTarget::DarwinArm64 => "arm64",
        }
    }

    /// Check if this target is a Darwin/macOS target
    pub fn is_darwin(&self) -> bool {
        matches!(self, Self::DarwinArm64)
    }

    /// Check if this is a Windows target
    pub fn is_windows(&self) -> bool {
        matches!(self, Self::WindowsX64)
    }

    /// Get the Linux container target matching the current host architecture.
    /// Containers always run Linux (even on macOS via Docker's Linux VM),
    /// so we map the host architecture to the corresponding Linux target.
    pub fn linux_container_target() -> Self {
        match Self::current_os() {
            Self::DarwinArm64 | Self::LinuxArm64 => Self::LinuxArm64,
            Self::LinuxX64 | Self::WindowsX64 => Self::LinuxX64,
        }
    }

    /// Get all possible targets as a Vec
    pub fn all() -> Vec<Self> {
        Self::ALL.to_vec()
    }

    /// Get default targets for a platform
    pub fn defaults_for_platform(platform: crate::Platform) -> Vec<Self> {
        match platform {
            crate::Platform::Aws => vec![Self::LinuxArm64],
            crate::Platform::Gcp => vec![Self::LinuxX64],
            crate::Platform::Azure => vec![Self::LinuxX64],
            crate::Platform::Kubernetes => vec![Self::LinuxArm64],
            crate::Platform::Local => vec![Self::current_os()],
            crate::Platform::Test => vec![Self::LinuxX64],
        }
    }

    /// Detect the current OS target
    pub fn current_os() -> Self {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return Self::WindowsX64;

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return Self::LinuxX64;

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return Self::LinuxArm64;

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return Self::DarwinArm64;

        #[cfg(not(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "aarch64")
        )))]
        {
            Self::LinuxX64
        }
    }
}

impl std::fmt::Display for BinaryTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BinaryTarget::WindowsX64 => write!(f, "windows-x64"),
            BinaryTarget::LinuxX64 => write!(f, "linux-x64"),
            BinaryTarget::LinuxArm64 => write!(f, "linux-arm64"),
            BinaryTarget::DarwinArm64 => write!(f, "darwin-arm64"),
        }
    }
}

impl std::str::FromStr for BinaryTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "windows-x64" => Ok(BinaryTarget::WindowsX64),
            "linux-x64" => Ok(BinaryTarget::LinuxX64),
            "linux-arm64" => Ok(BinaryTarget::LinuxArm64),
            "darwin-arm64" => Ok(BinaryTarget::DarwinArm64),
            _ => Err(format!("Unknown binary target: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BinaryTarget;
    use crate::Platform;

    #[test]
    fn local_platform_defaults_to_current_host_target() {
        assert_eq!(
            BinaryTarget::defaults_for_platform(Platform::Local),
            vec![BinaryTarget::current_os()]
        );
    }

    #[test]
    fn cloud_platform_defaults_remain_stable() {
        assert_eq!(
            BinaryTarget::defaults_for_platform(Platform::Aws),
            vec![BinaryTarget::LinuxArm64]
        );
        assert_eq!(
            BinaryTarget::defaults_for_platform(Platform::Gcp),
            vec![BinaryTarget::LinuxX64]
        );
        assert_eq!(
            BinaryTarget::defaults_for_platform(Platform::Azure),
            vec![BinaryTarget::LinuxX64]
        );
        assert_eq!(
            BinaryTarget::defaults_for_platform(Platform::Kubernetes),
            vec![BinaryTarget::LinuxArm64]
        );
    }
}
