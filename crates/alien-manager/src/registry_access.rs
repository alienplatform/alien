//! Cross-account registry access automation for deployments.
//!
//! Grants and revokes artifact registry access based on deployment state
//! during sync/reconcile. AWS ECR and GCP GAR use IAM-based cross-account
//! access; Azure ACR uses pull tokens and is not handled here.

use std::sync::Arc;

use alien_bindings::{
    traits::{
        ArtifactRegistry, AwsCrossAccountAccess, ComputeServiceType, CrossAccountAccess,
        GcpCrossAccountAccess,
    },
    BindingsProviderApi,
};
use alien_core::{AwsEnvironmentInfo, DeploymentStatus, EnvironmentInfo, GcpEnvironmentInfo};
use tracing::{info, warn};

/// Ensures cross-account registry access is granted for a deployment.
///
/// Called during sync/reconcile when `environment_info` is available and the
/// deployment is transitioning toward or has reached Provisioning.
/// This is idempotent — granting the same access twice is safe.
pub async fn ensure_registry_access(
    artifact_registry: &dyn ArtifactRegistry,
    repo_id: &str,
    environment_info: &EnvironmentInfo,
) {
    let access = match build_cross_account_access(environment_info) {
        Some(a) => a,
        None => return,
    };

    match artifact_registry
        .add_cross_account_access(repo_id, access)
        .await
    {
        Ok(()) => {
            info!(
                repo_id = %repo_id,
                platform = %environment_info.platform(),
                "Registry cross-account access granted"
            );
        }
        Err(e) => {
            warn!(
                repo_id = %repo_id,
                platform = %environment_info.platform(),
                error = %e,
                "Failed to grant registry cross-account access"
            );
        }
    }
}

/// Revokes cross-account registry access for a deployment during cleanup.
///
/// Called during sync/reconcile when the deployment reaches `Deleted` status.
pub async fn revoke_registry_access(
    artifact_registry: &dyn ArtifactRegistry,
    repo_id: &str,
    environment_info: &EnvironmentInfo,
) {
    let access = match build_cross_account_access(environment_info) {
        Some(a) => a,
        None => return,
    };

    match artifact_registry
        .remove_cross_account_access(repo_id, access)
        .await
    {
        Ok(()) => {
            info!(
                repo_id = %repo_id,
                platform = %environment_info.platform(),
                "Registry cross-account access revoked"
            );
        }
        Err(e) => {
            warn!(
                repo_id = %repo_id,
                platform = %environment_info.platform(),
                error = %e,
                "Failed to revoke registry cross-account access"
            );
        }
    }
}

/// Loads the artifact registry from the bindings provider and applies the
/// appropriate grant or revoke based on deployment status.
///
/// Returns without error if the bindings provider is unavailable or the
/// artifact registry cannot be loaded — registry access is best-effort.
pub async fn reconcile_registry_access(
    bindings_provider: &Arc<dyn BindingsProviderApi>,
    deployment_id: &str,
    environment_info: &EnvironmentInfo,
    status: &DeploymentStatus,
) {
    let artifact_registry = match bindings_provider.load_artifact_registry("artifacts").await {
        Ok(ar) => ar,
        Err(e) => {
            warn!(
                deployment_id = %deployment_id,
                error = %e,
                "Could not load artifact registry, skipping registry access reconciliation"
            );
            return;
        }
    };

    let repo_id = deployment_id;

    if *status == DeploymentStatus::Deleted {
        revoke_registry_access(artifact_registry.as_ref(), repo_id, environment_info).await;
    } else if needs_registry_access(status) {
        ensure_registry_access(artifact_registry.as_ref(), repo_id, environment_info).await;
    }
}

fn needs_registry_access(status: &DeploymentStatus) -> bool {
    matches!(
        status,
        DeploymentStatus::InitialSetup
            | DeploymentStatus::InitialSetupFailed
            | DeploymentStatus::Provisioning
            | DeploymentStatus::ProvisioningFailed
            | DeploymentStatus::Running
            | DeploymentStatus::RefreshFailed
            | DeploymentStatus::UpdatePending
            | DeploymentStatus::Updating
            | DeploymentStatus::UpdateFailed
    )
}

fn build_cross_account_access(environment_info: &EnvironmentInfo) -> Option<CrossAccountAccess> {
    match environment_info {
        EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id, region, ..
        }) => Some(CrossAccountAccess::Aws(AwsCrossAccountAccess {
            account_ids: vec![account_id.clone()],
            regions: vec![region.clone()],
            allowed_service_types: vec![ComputeServiceType::Function],
            role_arns: vec![],
        })),
        EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_number, ..
        }) => Some(CrossAccountAccess::Gcp(GcpCrossAccountAccess {
            project_numbers: vec![project_number.clone()],
            allowed_service_types: vec![ComputeServiceType::Function],
            service_account_emails: vec![],
        })),
        _ => None,
    }
}
