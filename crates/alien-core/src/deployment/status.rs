//! Deployment status enum and lifecycle phase checks.

use serde::{Deserialize, Serialize};

/// Deployment status in the deployment lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentStatus {
    Pending,
    InitialSetup,
    InitialSetupFailed,
    Provisioning,
    ProvisioningFailed,
    Running,
    RefreshFailed,
    UpdatePending,
    Updating,
    UpdateFailed,
    DeletePending,
    Deleting,
    DeleteFailed,
    Deleted,
}

impl DeploymentStatus {
    /// Check if deployment is synced (current state matches desired state).
    ///
    /// When synced, no more deployment steps are needed *for the current operation*.
    /// Note: This doesn't mean the deployment is "done forever":
    /// - `Running` → heartbeats continue, updates can come
    /// - `*Failed` → can be retried
    /// - `Deleted` → can be recreated
    ///
    /// "Synced" means: "we've reached the goal of the current deployment phase"
    pub fn is_synced(&self) -> bool {
        matches!(
            self,
            DeploymentStatus::Running
                | DeploymentStatus::InitialSetupFailed
                | DeploymentStatus::ProvisioningFailed
                | DeploymentStatus::UpdateFailed
                | DeploymentStatus::DeleteFailed
                | DeploymentStatus::RefreshFailed
                | DeploymentStatus::Deleted
        )
    }

    /// Check if deployment is in a failed state that requires retry to proceed.
    pub fn is_failed(&self) -> bool {
        matches!(
            self,
            DeploymentStatus::InitialSetupFailed
                | DeploymentStatus::ProvisioningFailed
                | DeploymentStatus::UpdateFailed
                | DeploymentStatus::DeleteFailed
                | DeploymentStatus::RefreshFailed
        )
    }
}
