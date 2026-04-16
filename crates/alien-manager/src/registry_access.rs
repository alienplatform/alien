//! Cross-account registry access automation for deployments.
//!
//! Grants and revokes artifact registry access based on deployment state
//! during sync/reconcile. AWS ECR and GCP GAR use IAM-based cross-account
//! access; Azure ACR uses pull tokens generated via `generate_credentials`.

use std::collections::HashMap;
use std::sync::Arc;

use alien_bindings::{
    traits::{
        ArtifactRegistry, AwsCrossAccountAccess, ComputeServiceType, CrossAccountAccess,
        GcpCrossAccountAccess,
    },
    BindingsProviderApi,
};
use alien_core::{
    AwsEnvironmentInfo, DeploymentState, DeploymentStatus, EnvironmentInfo, GcpEnvironmentInfo,
    Platform, RemoteStackManagementOutputs, RuntimeMetadata, StackState,
};
use tracing::{debug, info, warn};

/// Ensures cross-account registry access is granted for a deployment.
///
/// Returns `true` if access was fully granted — including the management
/// service account when available. Returns `false` if the grant failed or
/// the management SA was not yet available (so the caller will re-try on
/// the next reconcile iteration).
async fn ensure_registry_access(
    artifact_registry: &dyn ArtifactRegistry,
    repo_id: &str,
    environment_info: &EnvironmentInfo,
    stack_state: Option<&StackState>,
) -> bool {
    let rsm_access = extract_rsm_access_configuration(stack_state);
    let has_management_sa = rsm_access.is_some();

    let access = match build_cross_account_access(environment_info, stack_state) {
        Some(a) => a,
        None => return false,
    };

    match artifact_registry
        .add_cross_account_access(repo_id, access)
        .await
    {
        Ok(()) => {
            info!(
                repo_id = %repo_id,
                platform = %environment_info.platform(),
                has_management_sa = %has_management_sa,
                "Registry cross-account access granted"
            );
            // Only consider fully granted when the management SA was included.
            // Cloud Run (GCP) and Lambda (AWS) require the management SA to have
            // artifact registry access when updating services with cross-project images.
            // If RSM outputs aren't available yet, return false so the next reconcile
            // iteration will re-grant with the management SA included.
            has_management_sa
        }
        Err(e) => {
            warn!(
                repo_id = %repo_id,
                platform = %environment_info.platform(),
                error = %e,
                "Failed to grant registry cross-account access"
            );
            false
        }
    }
}

/// Revokes cross-account registry access for a deployment during cleanup.
///
/// Called during sync/reconcile when the deployment reaches `Deleted` status.
async fn revoke_registry_access(
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
/// On successful grant, sets `registry_access_granted` on the deployment state
/// so subsequent reconcile calls skip the (expensive) cloud API call.
/// The caller is responsible for persisting the updated state.
///
/// Returns without error if the bindings provider is unavailable or the
/// artifact registry cannot be loaded — registry access is best-effort.
pub async fn reconcile_registry_access(
    bindings_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    deployment_id: &str,
    state: &mut DeploymentState,
) {
    let environment_info = match &state.environment_info {
        Some(env_info) => env_info,
        None => return,
    };

    let platform = environment_info.platform();
    let status = &state.status;

    // Deletion always runs (to clean up).
    if *status == DeploymentStatus::Deleted {
        let artifact_registry =
            match load_artifact_registry(bindings_provider, target_bindings_providers, &platform)
                .await
            {
                Some(ar) => ar,
                None => return,
            };

        // Empty repo_id means "use just the repository prefix" — the prefix
        // is already baked into make_full_repo_name via self.repository_prefix.
        // Passing the prefix here would double it (e.g. "alien-quickstart-alien-quickstart").
        let repo_id = "";

        // Azure ACR: clean up scope map + token.
        if matches!(environment_info, EnvironmentInfo::Azure(_)) {
            if let Some(repo_name) = extract_acr_image_repo(state.stack_state.as_ref()) {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                deployment_id.hash(&mut hasher);
                let prefix = format!("d{:x}", hasher.finish());
                let namespaced_repo = format!("{}--{}", prefix, repo_name);

                if let Err(e) = artifact_registry
                    .cleanup_credentials(&namespaced_repo)
                    .await
                {
                    warn!(
                        deployment_id = %deployment_id,
                        namespaced_repo = %namespaced_repo,
                        error = %e,
                        "Failed to clean up Azure ACR credentials"
                    );
                }
            }
        }

        // AWS/GCP: revoke IAM-based cross-account access.
        revoke_registry_access(
            artifact_registry.as_ref(),
            &repo_id,
            environment_info,
            state.stack_state.as_ref(),
        )
        .await;
        return;
    }

    if !needs_registry_access(status) {
        return;
    }

    // Already granted — nothing to do.
    let already_granted = state
        .runtime_metadata
        .as_ref()
        .map_or(false, |rm| rm.registry_access_granted);
    if already_granted {
        return;
    }

    let artifact_registry = match load_artifact_registry(
        bindings_provider,
        target_bindings_providers,
        &platform,
    )
    .await
    {
        Some(ar) => ar,
        None => {
            debug!(
                deployment_id = %deployment_id,
                "No artifact registry binding available, skipping registry access reconciliation"
            );
            return;
        }
    };

    // Empty repo_id means "use just the repository prefix" — the prefix
    // is already baked into make_full_repo_name via self.repository_prefix.
    // Passing the prefix here would double it (e.g. "alien-quickstart-alien-quickstart").
    let repo_id = "";

    // AWS/GCP: grant IAM-based cross-account access.
    let granted = ensure_registry_access(
        artifact_registry.as_ref(),
        &repo_id,
        environment_info,
        state.stack_state.as_ref(),
    )
    .await;

    if granted {
        let rm = state
            .runtime_metadata
            .get_or_insert_with(RuntimeMetadata::default);
        rm.registry_access_granted = true;
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

/// Extract the ACR image repo name from the stack state's function resources.
///
/// Downcasts function configs to `Function` to read the image URI,
/// then extracts the repo path from ACR URIs like
/// `registry.azurecr.io/http-server:tag` → `http-server`.
fn extract_acr_image_repo(stack_state: Option<&StackState>) -> Option<String> {
    use alien_core::{Function, FunctionCode};

    let stack_state = stack_state?;
    for (_id, resource) in &stack_state.resources {
        let image_uri = if let Some(func) = resource.config.downcast_ref::<Function>() {
            match &func.code {
                FunctionCode::Image { image } => Some(image.as_str()),
                FunctionCode::Source { .. } => None,
            }
        } else {
            None
        };

        if let Some(image) = image_uri {
            if image.contains(".azurecr.io/") {
                let after_host = image.split(".azurecr.io/").nth(1)?;
                let repo_path = after_host.split(':').next()?;
                if !repo_path.is_empty() {
                    return Some(repo_path.to_string());
                }
            }
        }
    }
    None
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
        EnvironmentInfo::Gcp(GcpEnvironmentInfo { project_number, .. }) => {
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

/// Derive the native image registry host for platforms that pull directly
/// from ECR/GAR (Lambda, Cloud Run). Returns None for proxy-pull platforms.
///
/// Used by the deployment loop, agent sync, and push reconcile to set
/// `native_image_host` on `DeploymentConfig` so controllers can resolve
/// proxy URIs to native URIs via `resolve_native_image_uri()`.
pub async fn derive_native_image_host(
    bindings_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    platform: &Platform,
) -> Option<String> {
    use alien_core::image_rewrite::strip_url_scheme;

    // Lambda (AWS) and Cloud Run (GCP) require native registry URIs (ECR/GAR).
    // Azure Container Apps pulls from the manager's proxy directly (not ACR).
    if !matches!(platform, Platform::Aws | Platform::Gcp) {
        return None;
    }

    let ar = load_artifact_registry(bindings_provider, target_bindings_providers, platform).await?;

    let endpoint = ar.registry_endpoint();
    if endpoint.is_empty() {
        return None;
    }

    Some(strip_url_scheme(&endpoint).to_string())
}

/// Load the artifact registry from the correct per-platform provider.
///
/// Strategy:
/// 1. Determine the deployment's platform from environment_info
/// 2. Look up the per-target provider for that platform (e.g., the AWS target
///    provider has `ALIEN_AWS_ARTIFACTS_BINDING` with ECR config)
/// 3. Try loading binding name `"artifacts"` (helm chart / E2E pattern)
/// 4. Fall back to the primary provider with `"artifact-registry"` (private
///    manager / Alien app pattern where the resource is named `artifact-registry`)
pub async fn load_artifact_registry(
    primary_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    platform: &Platform,
) -> Option<Arc<dyn ArtifactRegistry>> {
    // Try per-target provider first. Different setups use different binding names:
    // - "artifacts" (helm chart / E2E)
    // - "artifact-registry" (standalone manager TOML)
    if let Some(target) = target_providers.get(platform) {
        for binding_name in ["artifacts", "artifact-registry"] {
            if let Ok(ar) = target.load_artifact_registry(binding_name).await {
                return Some(ar);
            }
        }
    }

    // Fall back to primary provider with "artifact-registry" name
    // (private manager / Alien app pattern: ALIEN_ARTIFACT_REGISTRY_BINDING)
    if let Some(ref primary) = primary_provider {
        if let Ok(ar) = primary.load_artifact_registry("artifact-registry").await {
            return Some(ar);
        }
        // Also try "artifacts" on primary (in case it's configured there)
        if let Ok(ar) = primary.load_artifact_registry("artifacts").await {
            return Some(ar);
        }
    }

    None
}
