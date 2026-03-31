//! Cross-account registry access automation for deployments.
//!
//! Grants and revokes artifact registry access based on deployment state
//! during sync/reconcile. AWS ECR and GCP GAR use IAM-based cross-account
//! access; Azure ACR uses pull tokens generated via `generate_credentials`.

use std::sync::Arc;

use alien_bindings::{
    traits::{
        ArtifactRegistry, ArtifactRegistryPermissions, AwsCrossAccountAccess,
        ComputeServiceType, CrossAccountAccess, GcpCrossAccountAccess,
    },
    BindingsProviderApi,
};
use alien_core::{
    AwsEnvironmentInfo, DeploymentStatus, EnvironmentInfo, GcpEnvironmentInfo,
    ImagePullCredentials, RemoteStackManagementOutputs, StackState,
};
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
    stack_state: Option<&StackState>,
) {
    let access = match build_cross_account_access(environment_info, stack_state) {
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
    stack_state: Option<&StackState>,
) {
    let access = match build_cross_account_access(environment_info, stack_state) {
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
/// For Azure, generates pull credentials via `generate_credentials` instead
/// of IAM-based cross-account access. Returns `Some(ImagePullCredentials)`
/// for Azure, `None` for AWS/GCP (which use IAM grants).
///
/// Returns without error if the bindings provider is unavailable or the
/// artifact registry cannot be loaded — registry access is best-effort.
pub async fn reconcile_registry_access(
    bindings_provider: &Arc<dyn BindingsProviderApi>,
    deployment_id: &str,
    environment_info: &EnvironmentInfo,
    status: &DeploymentStatus,
    stack_state: Option<&StackState>,
) -> Option<ImagePullCredentials> {
    let artifact_registry = match bindings_provider.load_artifact_registry("artifacts").await {
        Ok(ar) => ar,
        Err(e) => {
            warn!(
                deployment_id = %deployment_id,
                error = %e,
                "Could not load artifact registry, skipping registry access reconciliation"
            );
            return None;
        }
    };

    let repo_id = deployment_id;

    if *status == DeploymentStatus::Deleted {
        revoke_registry_access(artifact_registry.as_ref(), repo_id, environment_info, stack_state)
            .await;
        return None;
    }

    if !needs_registry_access(status) {
        return None;
    }

    // Azure ACR uses pull tokens, not IAM-based cross-account access.
    if matches!(environment_info, EnvironmentInfo::Azure(_)) {
        return generate_azure_pull_credentials(artifact_registry.as_ref(), repo_id).await;
    }

    // AWS/GCP: grant IAM-based cross-account access.
    ensure_registry_access(artifact_registry.as_ref(), repo_id, environment_info, stack_state)
        .await;
    None
}

/// Generate Azure ACR pull credentials via the artifact registry binding.
async fn generate_azure_pull_credentials(
    artifact_registry: &dyn ArtifactRegistry,
    repo_id: &str,
) -> Option<ImagePullCredentials> {
    match artifact_registry
        .generate_credentials(repo_id, ArtifactRegistryPermissions::Pull, None)
        .await
    {
        Ok(creds) => {
            info!(
                repo_id = %repo_id,
                "Azure ACR pull credentials generated"
            );
            Some(ImagePullCredentials {
                username: creds.username,
                password: creds.password,
            })
        }
        Err(e) => {
            warn!(
                repo_id = %repo_id,
                error = %e,
                "Failed to generate Azure ACR pull credentials"
            );
            None
        }
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

/// Extract RSM access configuration from stack state to include in cross-account access.
fn extract_rsm_access_configuration(stack_state: Option<&StackState>) -> Option<String> {
    let stack_state = stack_state?;
    for (_id, resource) in &stack_state.resources {
        if resource.resource_type == "remote-stack-management" {
            if let Some(ref outputs) = resource.outputs {
                if let Some(rsm) = outputs.downcast_ref::<RemoteStackManagementOutputs>() {
                    return Some(rsm.access_configuration.clone());
                }
            }
        }
    }
    None
}

fn build_cross_account_access(
    environment_info: &EnvironmentInfo,
    stack_state: Option<&StackState>,
) -> Option<CrossAccountAccess> {
    let rsm_access = extract_rsm_access_configuration(stack_state);

    match environment_info {
        EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id, region, ..
        }) => {
            let role_arns = rsm_access.into_iter().collect();
            Some(CrossAccountAccess::Aws(AwsCrossAccountAccess {
                account_ids: vec![account_id.clone()],
                regions: vec![region.clone()],
                allowed_service_types: vec![ComputeServiceType::Function],
                role_arns,
            }))
        }
        EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_number, ..
        }) => {
            let service_account_emails = rsm_access.into_iter().collect();
            Some(CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                project_numbers: vec![project_number.clone()],
                allowed_service_types: vec![ComputeServiceType::Function],
                service_account_emails,
            }))
        }
        _ => None,
    }
}
