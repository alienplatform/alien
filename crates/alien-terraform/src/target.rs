//! Terraform module target.
//!
//! `Aws` / `Gcp` / `Azure` are pure-cloud targets. `Eks` / `Gke` / `Aks`
//! reuse the same cloud emitters and add a Kubernetes identity overlay
//! (IRSA / Workload Identity / UAMI federated identity) on top.

use alien_core::Platform;
use serde::{Deserialize, Serialize};

/// Terraform module target.
///
/// Cloud targets deploy directly to their cloud platform. Managed Kubernetes
/// targets use cloud emitters for setup resources, then deploy the running
/// application to Kubernetes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerraformTarget {
    Aws,
    Gcp,
    Azure,
    Eks,
    Gke,
    Aks,
}

impl TerraformTarget {
    /// The cloud platform whose Terraform emitters back this target.
    pub fn cloud_platform(self) -> Platform {
        match self {
            TerraformTarget::Aws | TerraformTarget::Eks => Platform::Aws,
            TerraformTarget::Gcp | TerraformTarget::Gke => Platform::Gcp,
            TerraformTarget::Azure | TerraformTarget::Aks => Platform::Azure,
        }
    }

    /// The platform where the application runtime will run after import.
    pub fn deployment_platform(self) -> Platform {
        if self.is_kubernetes() {
            Platform::Kubernetes
        } else {
            self.cloud_platform()
        }
    }

    /// Base cloud platform for managed Kubernetes targets.
    pub fn base_platform(self) -> Option<Platform> {
        self.is_kubernetes().then_some(self.cloud_platform())
    }

    /// Whether the Kubernetes identity overlay should run for this target.
    pub fn is_kubernetes(self) -> bool {
        matches!(
            self,
            TerraformTarget::Eks | TerraformTarget::Gke | TerraformTarget::Aks
        )
    }

    /// Stable kebab-case name used in metadata + outputs.
    pub fn name(self) -> &'static str {
        match self {
            TerraformTarget::Aws => "aws",
            TerraformTarget::Gcp => "gcp",
            TerraformTarget::Azure => "azure",
            TerraformTarget::Eks => "eks",
            TerraformTarget::Gke => "gke",
            TerraformTarget::Aks => "aks",
        }
    }
}
