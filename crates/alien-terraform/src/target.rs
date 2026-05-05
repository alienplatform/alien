//! Terraform module target.
//!
//! `Aws` / `Gcp` / `Azure` are pure-cloud targets. `Eks` / `Gke` / `Aks`
//! reuse the same cloud emitters and add a Kubernetes identity overlay
//! (IRSA / Workload Identity / UAMI federated identity) on top.

use alien_core::Platform;
use serde::{Deserialize, Serialize};

/// Terraform module target. Maps to a [`Platform`] for emitter lookup,
/// plus a flag indicating whether the K8s identity overlay should run.
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
    /// The cloud whose emitters back this target.
    pub fn platform(self) -> Platform {
        match self {
            TerraformTarget::Aws | TerraformTarget::Eks => Platform::Aws,
            TerraformTarget::Gcp | TerraformTarget::Gke => Platform::Gcp,
            TerraformTarget::Azure | TerraformTarget::Aks => Platform::Azure,
        }
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
