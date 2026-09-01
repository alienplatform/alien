//! Cross-account registry access automation for deployments.
//!
//! Grants and revokes artifact registry access based on deployment state
//! during sync/reconcile. AWS ECR and GCP GAR use IAM-based cross-account
//! access; Azure ACR uses pull tokens generated via `generate_credentials`.

use std::collections::{HashMap, HashSet};
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
    Platform, RemoteStackManagementOutputs, ResourceEntry, RuntimeMetadata, Sandbox, SandboxCode,
    Stack, StackState, Worker, WorkerCode,
};
use alien_error::{AlienError, Context};
use tracing::{debug, info, warn};

use crate::auth::Subject;
use crate::error::{ErrorData, Result};
use crate::traits::deployment_store::{DeploymentFilter, DeploymentRecord};
use crate::traits::DeploymentStore;

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
    deployment_store: &dyn DeploymentStore,
    deployment_id: &str,
) -> Result<()> {
    if let EnvironmentInfo::Gcp(GcpEnvironmentInfo { project_number, .. }) = environment_info {
        if let Some(service_account_email) = extract_rsm_access_configuration(stack_state) {
            remove_registry_access(
                artifact_registry,
                repo_id,
                environment_info,
                CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                    project_numbers: Vec::new(),
                    allowed_service_types: Vec::new(),
                    service_account_emails: vec![service_account_email],
                }),
                "deployment-owned registry access",
                deployment_id,
            )
            .await?;
        }

        let deployments = deployment_store
            .list_deployments(
                &Subject::system(),
                &DeploymentFilter {
                    platforms: Some(vec![Platform::Gcp]),
                    ..Default::default()
                },
            )
            .await
            .context(ErrorData::RegistryAccessCleanupFailed {
                deployment_id: deployment_id.to_string(),
                reason: "active GCP deployments could not be checked before revoking shared access"
                    .to_string(),
            })?;

        if deployments.iter().any(|deployment| {
            is_other_active_gcp_project_consumer(deployment, deployment_id, project_number)
        }) {
            debug!(
                deployment_id = %deployment_id,
                project_number = %project_number,
                "Keeping shared Cloud Run registry access for another active deployment"
            );
            return Ok(());
        }

        let project_numbers = if project_number.is_empty() {
            Vec::new()
        } else {
            vec![project_number.clone()]
        };
        remove_registry_access(
            artifact_registry,
            repo_id,
            environment_info,
            CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                project_numbers,
                allowed_service_types: vec![ComputeServiceType::Worker],
                service_account_emails: Vec::new(),
            }),
            "last project consumer's shared registry access",
            deployment_id,
        )
        .await?;
        return Ok(());
    }

    let Some(access) = build_cross_account_access(environment_info, stack_state) else {
        return Ok(());
    };
    remove_registry_access(
        artifact_registry,
        repo_id,
        environment_info,
        access,
        "registry cross-account access",
        deployment_id,
    )
    .await
}

async fn remove_registry_access(
    artifact_registry: &dyn ArtifactRegistry,
    repo_id: &str,
    environment_info: &EnvironmentInfo,
    access: CrossAccountAccess,
    access_kind: &str,
    deployment_id: &str,
) -> Result<()> {
    artifact_registry
        .remove_cross_account_access(repo_id, access)
        .await
        .context(ErrorData::RegistryAccessCleanupFailed {
            deployment_id: deployment_id.to_string(),
            reason: format!("failed to revoke {access_kind} for repository '{repo_id}'"),
        })?;

    info!(
        repo_id = %repo_id,
        platform = %environment_info.platform(),
        access_kind = %access_kind,
        "Registry access revoked"
    );
    Ok(())
}

fn is_other_active_gcp_project_consumer(
    deployment: &DeploymentRecord,
    deleted_deployment_id: &str,
    project_number: &str,
) -> bool {
    deployment.id != deleted_deployment_id
        && deployment.status != "deleted"
        && matches!(
            &deployment.environment_info,
            Some(EnvironmentInfo::Gcp(GcpEnvironmentInfo {
                project_number: other_project_number,
                ..
            })) if other_project_number == project_number
        )
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

    // Cleanup runs after the Deleted state is persisted. Doing it here would
    // let two concurrent deletions both observe the other deployment as active,
    // leaving the shared project-level grant behind.
    if *status == DeploymentStatus::Deleted {
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

    let repo_ids = repository_ids_for_access(artifact_registry.as_ref(), state);
    if repo_ids.is_empty() {
        return;
    }

    // AWS/GCP: grant IAM-based cross-account access.
    let mut granted = true;
    for repo_id in repo_ids {
        granted &= ensure_registry_access(
            artifact_registry.as_ref(),
            &repo_id,
            environment_info,
            state.stack_state.as_ref(),
        )
        .await;
    }

    if granted {
        let rm = state
            .runtime_metadata
            .get_or_insert_with(RuntimeMetadata::default);
        rm.registry_access_granted = true;
    }
}

/// Revokes registry access after a Deleted state has been persisted.
///
/// Cloud Run's service agent is project-scoped, so multiple deployments in
/// the same GCP project share one GAR reader grant. The final active consumer
/// removes that shared grant; every deployment still removes its own remote
/// management service-account grant.
pub async fn cleanup_deleted_registry_access(
    deployment_store: &dyn DeploymentStore,
    bindings_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    deployment_id: &str,
    state: &DeploymentState,
) -> Result<()> {
    if state.status != DeploymentStatus::Deleted {
        return Ok(());
    }
    let Some(environment_info) = state.environment_info.as_ref() else {
        return Ok(());
    };
    let platform = environment_info.platform();
    if !matches!(platform, Platform::Aws | Platform::Gcp) {
        return Ok(());
    }
    let registry_access_granted = state
        .runtime_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.registry_access_granted);
    // A successful initial grant can precede the remote-management identity
    // becoming available, in which case `registry_access_granted` remains
    // false so reconciliation retries the complete grant. Still clean up that
    // partial grant when the deployment is deleted.
    if !registry_access_granted && !has_registry_backed_image(state, &platform) {
        return Ok(());
    }

    let Some(artifact_registry) =
        load_artifact_registry(bindings_provider, target_bindings_providers, &platform).await
    else {
        return Err(AlienError::new(ErrorData::RegistryAccessCleanupFailed {
            deployment_id: deployment_id.to_string(),
            reason: format!("artifact registry binding for '{platform}' is unavailable"),
        }));
    };

    let repo_ids = repository_ids_for_access(artifact_registry.as_ref(), state);
    if repo_ids.is_empty() {
        return if registry_access_granted {
            Err(AlienError::new(ErrorData::RegistryAccessCleanupFailed {
                deployment_id: deployment_id.to_string(),
                reason: "repository identifiers for the recorded registry grant are unavailable"
                    .to_string(),
            }))
        } else {
            Ok(())
        };
    }

    for repo_id in repo_ids {
        revoke_registry_access(
            artifact_registry.as_ref(),
            &repo_id,
            environment_info,
            state.stack_state.as_ref(),
            deployment_store,
            deployment_id,
        )
        .await?;
    }
    Ok(())
}

fn repository_ids_for_access(
    artifact_registry: &dyn ArtifactRegistry,
    state: &DeploymentState,
) -> Vec<String> {
    let prefix = artifact_registry.upstream_repository_prefix();
    if prefix.is_empty() {
        return Vec::new();
    }

    if matches!(
        state
            .environment_info
            .as_ref()
            .map(EnvironmentInfo::platform),
        Some(Platform::Aws)
    ) {
        let mut repo_ids = HashSet::new();
        collect_image_repositories(state, &prefix, INCLUDE_SANDBOX, &mut repo_ids);
        let mut repo_ids: Vec<_> = repo_ids.into_iter().collect();
        repo_ids.sort();
        return repo_ids;
    }

    if !has_image_in_repository_prefix(state, &prefix, EXCLUDE_SANDBOX) {
        return Vec::new();
    }

    let repo_ids = HashSet::from([prefix]);
    let mut repo_ids: Vec<_> = repo_ids.into_iter().collect();
    repo_ids.sort();
    repo_ids
}

/// Only the AWS sandbox takes its root filesystem from an image Alien hosts, so counting one on
/// any other provider would claim a grant that provider never needs.
const INCLUDE_SANDBOX: bool = true;
const EXCLUDE_SANDBOX: bool = false;

/// The image a resource pulls from Alien's registry, if it declares one.
fn resource_image_reference(entry: &ResourceEntry, include_sandbox: bool) -> Option<&str> {
    if let Some(worker) = entry.config.downcast_ref::<Worker>() {
        return match &worker.code {
            WorkerCode::Image { image } => Some(image),
            _ => None,
        };
    }
    if include_sandbox {
        if let Some(sandbox) = entry.config.downcast_ref::<Sandbox>() {
            return match &sandbox.code {
                SandboxCode::Image { image } => Some(image),
                _ => None,
            };
        }
    }
    None
}

fn collect_image_repositories(
    state: &DeploymentState,
    prefix: &str,
    include_sandbox: bool,
    repo_ids: &mut HashSet<String>,
) {
    let mut collect = |stack: &Stack| {
        for (_id, entry) in stack.resources() {
            let Some(image) = resource_image_reference(entry, include_sandbox) else {
                continue;
            };
            if let Some(repo_id) = ecr_repository_from_image(image, prefix) {
                repo_ids.insert(repo_id);
            }
        }
    };

    if let Some(release) = &state.current_release {
        collect(&release.stack);
    }
    if let Some(release) = &state.target_release {
        collect(&release.stack);
    }
    if let Some(runtime_metadata) = &state.runtime_metadata {
        if let Some(stack) = &runtime_metadata.prepared_stack {
            collect(stack);
        }
    }
}

fn has_image_in_repository_prefix(
    state: &DeploymentState,
    prefix: &str,
    include_sandbox: bool,
) -> bool {
    let stack_matches = |stack: &Stack| {
        stack.resources().any(|(_id, entry)| {
            resource_image_reference(entry, include_sandbox)
                .is_some_and(|image| image_repository_matches_prefix(image, prefix))
        })
    };

    state
        .current_release
        .as_ref()
        .is_some_and(|release| stack_matches(&release.stack))
        || state
            .target_release
            .as_ref()
            .is_some_and(|release| stack_matches(&release.stack))
        || state
            .runtime_metadata
            .as_ref()
            .and_then(|runtime_metadata| runtime_metadata.prepared_stack.as_ref())
            .is_some_and(stack_matches)
}

/// Whether the deployment declares any image that could have earned a registry grant.
///
/// Deliberately over-inclusive: the repository prefix is only known once the registry binding
/// loads, so answering `true` too often costs a lookup, while answering `false` too often leaves
/// a live cross-account grant on Alien's registry with nothing left to revoke it.
fn has_registry_backed_image(state: &DeploymentState, platform: &Platform) -> bool {
    let include_sandbox = matches!(platform, Platform::Aws);

    state
        .current_release
        .as_ref()
        .is_some_and(|release| stack_has_registry_backed_image(&release.stack, include_sandbox))
        || state
            .target_release
            .as_ref()
            .is_some_and(|release| stack_has_registry_backed_image(&release.stack, include_sandbox))
        || state
            .runtime_metadata
            .as_ref()
            .and_then(|runtime_metadata| runtime_metadata.prepared_stack.as_ref())
            .is_some_and(|stack| stack_has_registry_backed_image(stack, include_sandbox))
}

fn stack_has_registry_backed_image(stack: &Stack, include_sandbox: bool) -> bool {
    stack
        .resources()
        .any(|(_id, entry)| resource_image_reference(entry, include_sandbox).is_some())
}

fn ecr_repository_from_image(image: &str, prefix: &str) -> Option<String> {
    let repository = image_repository_path(image)?;

    if repository == prefix || repository.starts_with(&format!("{prefix}-")) {
        Some(repository)
    } else {
        None
    }
}

fn image_repository_matches_prefix(image: &str, prefix: &str) -> bool {
    image_repository_path(image).is_some_and(|repository| {
        repository == prefix
            || repository.starts_with(&format!("{prefix}-"))
            || repository.starts_with(&format!("{prefix}/"))
    })
}

fn image_repository_path(image: &str) -> Option<String> {
    let without_scheme = alien_core::image_rewrite::strip_url_scheme(image);
    let (_host, path) = without_scheme.split_once('/')?;
    let path_without_digest = path.split_once('@').map_or(path, |(path, _)| path);
    let repository = match path_without_digest.rsplit_once(':') {
        Some((repo, tag)) if !repo.contains(':') && !tag.contains('/') => repo,
        _ => path_without_digest,
    };

    Some(repository.to_string())
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
                allowed_service_types: vec![ComputeServiceType::Worker],
                role_arns,
            }))
        }
        EnvironmentInfo::Gcp(GcpEnvironmentInfo { project_number, .. }) => {
            let service_account_emails = rsm_access.into_iter().collect();
            let project_numbers = if project_number.is_empty() {
                Vec::new()
            } else {
                vec![project_number.clone()]
            };
            Some(CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                project_numbers,
                allowed_service_types: vec![ComputeServiceType::Worker],
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

/// Load the artifact registry that owns deployment images for a target platform.
///
/// Cloud image routes are registered from per-target providers, while the
/// primary provider is the embedded local fallback. Prefer the target provider
/// so registry access reconciliation and native image host derivation use the
/// same ECR/GAR registry that the proxy pushed images to.
pub async fn load_artifact_registry(
    primary_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    platform: &Platform,
) -> Option<Arc<dyn ArtifactRegistry>> {
    if let Some(target) = target_providers.get(platform) {
        for binding_name in ["artifacts", "artifact-registry"] {
            if let Ok(ar) = target.load_artifact_registry(binding_name).await {
                return Some(ar);
            }
        }
    }

    if let Some(ref primary) = primary_provider {
        for binding_name in ["artifact-registry", "artifacts"] {
            if let Ok(ar) = primary.load_artifact_registry(binding_name).await {
                return Some(ar);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_bindings::error::{ErrorData as BindingErrorData, Result as BindingResult};
    use alien_bindings::providers::artifact_registry::ecr::cross_account_repository_policy;
    use alien_bindings::traits::{
        ArtifactRegistryCredentials, ArtifactRegistryPermissions, BindingsProviderApi,
        CrossAccountPermissions, RepositoryResponse,
    };
    use alien_core::{
        ReleaseInfo, RemoteStackManagement, Resource, ResourceLifecycle, ResourceOutputs,
        ResourceStatus, SandboxEgress, SandboxSessionPolicy, StackResourceState,
    };
    use alien_error::AlienError;
    use async_trait::async_trait;

    #[derive(Debug)]
    struct TestArtifactRegistry {
        prefix: String,
        fail_remove: bool,
    }

    impl alien_bindings::traits::Binding for TestArtifactRegistry {}

    #[async_trait]
    impl ArtifactRegistry for TestArtifactRegistry {
        fn registry_endpoint(&self) -> String {
            format!("https://{}.example.com", self.prefix)
        }

        fn upstream_repository_prefix(&self) -> String {
            self.prefix.clone()
        }

        async fn create_repository(&self, _repo_name: &str) -> BindingResult<RepositoryResponse> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn get_repository(&self, _repo_id: &str) -> BindingResult<RepositoryResponse> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn add_cross_account_access(
            &self,
            _repo_id: &str,
            _access: CrossAccountAccess,
        ) -> BindingResult<()> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn remove_cross_account_access(
            &self,
            _repo_id: &str,
            _access: CrossAccountAccess,
        ) -> BindingResult<()> {
            if self.fail_remove {
                Err(AlienError::new(BindingErrorData::Other {
                    message: "simulated registry IAM failure".to_string(),
                }))
            } else {
                Ok(())
            }
        }

        async fn get_cross_account_access(
            &self,
            _repo_id: &str,
        ) -> BindingResult<CrossAccountPermissions> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn generate_credentials(
            &self,
            _repo_id: &str,
            _permissions: ArtifactRegistryPermissions,
            _ttl_seconds: Option<u32>,
        ) -> BindingResult<ArtifactRegistryCredentials> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn delete_repository(&self, _repo_id: &str) -> BindingResult<()> {
            unimplemented!("not needed for registry access repo-id tests")
        }
    }

    #[derive(Debug)]
    struct TestBindingsProvider {
        binding_name: &'static str,
        registry: Arc<dyn ArtifactRegistry>,
    }

    fn missing_binding(binding_name: &str) -> alien_bindings::error::Error {
        AlienError::new(BindingErrorData::BindingConfigInvalid {
            binding_name: binding_name.to_string(),
            env_var: alien_core::bindings::binding_env_var_name(binding_name),
            reason: "not found".to_string(),
        })
    }

    #[async_trait]
    impl BindingsProviderApi for TestBindingsProvider {
        async fn load_artifact_registry(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn ArtifactRegistry>> {
            if binding_name == self.binding_name {
                Ok(self.registry.clone())
            } else {
                Err(missing_binding(binding_name))
            }
        }

        async fn load_storage(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::Storage>> {
            Err(missing_binding(binding_name))
        }

        async fn load_build(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::Build>> {
            Err(missing_binding(binding_name))
        }

        async fn load_vault(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::Vault>> {
            Err(missing_binding(binding_name))
        }

        async fn load_kv(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::Kv>> {
            Err(missing_binding(binding_name))
        }

        async fn load_postgres(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::Postgres>> {
            Err(missing_binding(binding_name))
        }

        async fn load_queue(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::Queue>> {
            Err(missing_binding(binding_name))
        }

        async fn load_worker(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::Worker>> {
            Err(missing_binding(binding_name))
        }

        async fn load_container(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::Container>> {
            Err(missing_binding(binding_name))
        }

        async fn load_service_account(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::ServiceAccount>> {
            Err(missing_binding(binding_name))
        }

        async fn load_sandbox(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn alien_bindings::traits::Sandbox>> {
            Err(missing_binding(binding_name))
        }
    }

    fn aws_state_with_stack(stack: Stack) -> DeploymentState {
        DeploymentState {
            status: DeploymentStatus::InitialSetup,
            platform: Platform::Aws,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: Some(EnvironmentInfo::Aws(AwsEnvironmentInfo {
                account_id: "123456789012".to_string(),
                region: "us-east-2".to_string(),
            })),
            runtime_metadata: Some(RuntimeMetadata {
                prepared_stack: Some(stack),
                ..RuntimeMetadata::default()
            }),
            retry_requested: false,
            protocol_version: 1,
        }
    }

    fn gcp_state_with_stack(stack: Stack) -> DeploymentState {
        DeploymentState {
            status: DeploymentStatus::InitialSetup,
            platform: Platform::Gcp,
            current_release: Some(ReleaseInfo {
                release_id: Some("rel_test".to_string()),
                version: None,
                description: None,
                stack,
            }),
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: Some(EnvironmentInfo::Gcp(GcpEnvironmentInfo {
                project_number: "123456789012".to_string(),
                project_id: "test-project".to_string(),
                region: "us-central1".to_string(),
            })),
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: 1,
        }
    }

    fn worker_stack(image: &str) -> Stack {
        Stack::new("test-stack".to_string())
            .add(
                Worker::new("test-worker".to_string())
                    .code(WorkerCode::Image {
                        image: image.to_string(),
                    })
                    .permissions("execution".to_string())
                    .build(),
                ResourceLifecycle::Live,
            )
            .build()
    }

    fn sandbox_stack(image: &str) -> Stack {
        Stack::new("test-stack".to_string())
            .add(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: image.to_string(),
                    })
                    .egress(SandboxEgress::Allow)
                    .session(SandboxSessionPolicy {
                        max_lifetime_seconds: None,
                        idle_suspend_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build()
    }

    /// A registered deployment's stack state: the RSM role ARN only exists once the customer's
    /// setup stack reported its outputs, which is what makes the grant legal at all.
    fn registered_stack_state(role_arn: &str) -> StackState {
        let remote_management = RemoteStackManagement::new("management".to_string()).build();
        let mut stack_state =
            StackState::with_resource_prefix(Platform::Aws, "test-prefix".to_string());
        stack_state.resources.insert(
            remote_management.id.clone(),
            StackResourceState::builder()
                .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(remote_management))
                .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                    management_resource_id: role_arn.to_string(),
                    access_configuration: role_arn.to_string(),
                    legacy_remote_bindings_access: None,
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );
        stack_state
    }

    fn gcp_deployment_record(id: &str, status: &str, project_number: &str) -> DeploymentRecord {
        DeploymentRecord {
            id: id.to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: id.to_string(),
            deployment_group_id: "dg_test".to_string(),
            platform: Platform::Gcp,
            deployment_protocol_version: 1,
            base_platform: None,
            status: status.to_string(),
            stack_settings: None,
            stack_state: None,
            environment_info: Some(EnvironmentInfo::Gcp(GcpEnvironmentInfo {
                project_number: project_number.to_string(),
                project_id: "test-project".to_string(),
                region: "us-central1".to_string(),
            })),
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_config: None,
            deployment_token: None,
            input_values: Default::default(),
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: chrono::Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn load_artifact_registry_prefers_target_provider() {
        let primary_registry: Arc<dyn ArtifactRegistry> = Arc::new(TestArtifactRegistry {
            prefix: "artifacts/default".to_string(),
            fail_remove: false,
        });
        let target_registry: Arc<dyn ArtifactRegistry> = Arc::new(TestArtifactRegistry {
            prefix: "alien-e2e".to_string(),
            fail_remove: false,
        });
        let primary_provider: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
            binding_name: "artifact-registry",
            registry: primary_registry,
        });
        let target_provider: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
            binding_name: "artifacts",
            registry: target_registry,
        });
        let target_providers = HashMap::from([(Platform::Aws, target_provider)]);

        let registry =
            load_artifact_registry(&Some(primary_provider), &target_providers, &Platform::Aws)
                .await
                .expect("registry should load");

        assert_eq!(registry.upstream_repository_prefix(), "alien-e2e");
        assert_eq!(
            derive_native_image_host(&None, &target_providers, &Platform::Aws).await,
            Some("alien-e2e.example.com".to_string())
        );
    }

    #[test]
    fn aws_registry_access_skips_container_only_stack() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: false,
        };
        let state = aws_state_with_stack(Stack::new("test-stack".to_string()).build());

        assert!(repository_ids_for_access(&registry, &state).is_empty());
    }

    #[test]
    fn aws_registry_access_uses_worker_image_repository() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: false,
        };
        let state = aws_state_with_stack(worker_stack(
            "manager.example.com/alien-artifacts-prj_test:test-worker-abc123",
        ));

        assert_eq!(
            repository_ids_for_access(&registry, &state),
            vec!["alien-artifacts-prj_test".to_string()]
        );
    }

    #[test]
    fn registry_access_skips_empty_repository_prefix() {
        let registry = TestArtifactRegistry {
            prefix: String::new(),
            fail_remove: false,
        };
        let state = gcp_state_with_stack(worker_stack(
            "manager.example.com/prj_test/test-worker:abc123",
        ));

        assert!(repository_ids_for_access(&registry, &state).is_empty());
    }

    #[test]
    fn gcp_registry_access_requires_worker_image_under_prefix() {
        let registry = TestArtifactRegistry {
            prefix: "test-project/alien-artifacts".to_string(),
            fail_remove: false,
        };
        let state = gcp_state_with_stack(worker_stack(
            "manager.example.com/test-project/alien-artifacts/test-worker:abc123",
        ));

        assert_eq!(
            repository_ids_for_access(&registry, &state),
            vec!["test-project/alien-artifacts".to_string()]
        );
    }

    #[test]
    fn gcp_shared_registry_access_is_kept_only_for_active_project_consumers() {
        let running_same_project = gcp_deployment_record("dep_other", "running", "123456789012");
        assert!(is_other_active_gcp_project_consumer(
            &running_same_project,
            "dep_deleted",
            "123456789012"
        ));

        let deleted_same_project = gcp_deployment_record("dep_other", "deleted", "123456789012");
        assert!(!is_other_active_gcp_project_consumer(
            &deleted_same_project,
            "dep_deleted",
            "123456789012"
        ));

        let running_other_project = gcp_deployment_record("dep_other", "running", "999999999999");
        assert!(!is_other_active_gcp_project_consumer(
            &running_other_project,
            "dep_deleted",
            "123456789012"
        ));

        assert!(!is_other_active_gcp_project_consumer(
            &gcp_deployment_record("dep_deleted", "running", "123456789012"),
            "dep_deleted",
            "123456789012"
        ));
    }

    #[tokio::test]
    async fn registry_cleanup_propagates_permission_removal_failure() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: true,
        };
        let environment_info = EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
        });
        let access = build_cross_account_access(&environment_info, None)
            .expect("AWS registry access should be configured");

        let error = remove_registry_access(
            &registry,
            "alien-artifacts-prj_test",
            &environment_info,
            access,
            "registry cross-account access",
            "dep_test",
        )
        .await
        .expect_err("permission removal failure must block cleanup");

        assert_eq!(error.code, "REGISTRY_ACCESS_CLEANUP_FAILED");
        assert!(error.retryable);
        assert!(error.internal);
        assert_eq!(
            error.source.as_ref().map(|source| source.code.as_str()),
            Some("BINDINGS_ERROR")
        );
    }
    #[test]
    fn aws_registry_access_uses_sandbox_image_repository() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: false,
        };
        // A sandbox base image pushed through the registry proxy, which is the only shape that
        // names a repository Alien hosts.
        let state = aws_state_with_stack(sandbox_stack(
            "manager.example.com/alien-artifacts-prj_test:agents-abc123",
        ));

        assert_eq!(
            repository_ids_for_access(&registry, &state),
            vec!["alien-artifacts-prj_test".to_string()],
            "a sandbox-only stack must resolve the repository its base image lives in"
        );
    }

    #[test]
    fn aws_registry_access_skips_sandbox_images_outside_the_repository_prefix() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: false,
        };

        for image in [
            "s3://acme-artifacts/agents/bundle.zip",
            "ubuntu:24.04",
            "ghcr.io/acme/sandbox:latest",
            "manager.example.com/alien-artifacts-prj_other:agents-abc123",
        ] {
            let state = aws_state_with_stack(sandbox_stack(image));
            assert!(
                repository_ids_for_access(&registry, &state).is_empty(),
                "'{image}' names no repository Alien hosts and must not produce a grant"
            );
        }
    }

    #[test]
    fn aws_sandbox_only_stack_grants_the_customer_account_and_rsm_role() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: false,
        };
        let role_arn = "arn:aws:iam::123456789012:role/alien-rsm-role";
        let mut state = aws_state_with_stack(sandbox_stack(
            "manager.example.com/alien-artifacts-prj_test:agents-abc123",
        ));
        state.stack_state = Some(registered_stack_state(role_arn));

        assert_eq!(
            repository_ids_for_access(&registry, &state),
            vec!["alien-artifacts-prj_test".to_string()]
        );

        let access = build_cross_account_access(
            state.environment_info.as_ref().expect("AWS environment"),
            state.stack_state.as_ref(),
        )
        .expect("AWS cross-account access should be configured");
        let CrossAccountAccess::Aws(aws_access) = access else {
            panic!("AWS environment must produce AWS cross-account access");
        };

        let policy = cross_account_repository_policy(&aws_access);
        let statements = policy["Statement"]
            .as_array()
            .expect("policy must carry statements");
        let cross_account = statements
            .iter()
            .find(|statement| statement["Sid"] == "CrossAccountRolePermission")
            .expect("the customer principals statement must be written");

        assert_eq!(
            cross_account["Principal"]["AWS"],
            serde_json::json!(["arn:aws:iam::123456789012:root", role_arn]),
            "the policy must name the customer account root and the RSM role"
        );
        assert_eq!(
            cross_account["Action"],
            serde_json::json!([
                "ecr:BatchCheckLayerAvailability",
                "ecr:GetDownloadUrlForLayer",
                "ecr:BatchGetImage",
                "ecr:GetRepositoryPolicy",
                "ecr:SetRepositoryPolicy"
            ]),
            "the MicroVM build pulls layers and manifests through this statement"
        );
        assert_eq!(policy["Version"], "2012-10-17");
    }

    /// The whole document for one account and role. The policy is built from the environment and
    /// the RSM outputs alone — never from stack resources — so this pins what every deployment
    /// gets, whichever resources earned it.
    #[test]
    fn aws_repository_policy_document_is_pinned() {
        let role_arn = "arn:aws:iam::123456789012:role/alien-rsm-role";
        let stack_state = registered_stack_state(role_arn);
        let environment_info = EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
        });

        let access = build_cross_account_access(&environment_info, Some(&stack_state))
            .expect("AWS cross-account access should be configured");
        let CrossAccountAccess::Aws(aws_access) = access else {
            panic!("AWS environment must produce AWS cross-account access");
        };
        let policy = cross_account_repository_policy(&aws_access);

        assert_eq!(
            policy,
            serde_json::json!({
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Sid": "CrossAccountRolePermission",
                        "Effect": "Allow",
                        "Principal": {
                            "AWS": ["arn:aws:iam::123456789012:root", role_arn]
                        },
                        "Action": [
                            "ecr:BatchCheckLayerAvailability",
                            "ecr:GetDownloadUrlForLayer",
                            "ecr:BatchGetImage",
                            "ecr:GetRepositoryPolicy",
                            "ecr:SetRepositoryPolicy"
                        ]
                    },
                    {
                        "Sid": "LambdaECRImageCrossAccountRetrievalPolicy",
                        "Effect": "Allow",
                        "Principal": { "Service": "lambda.amazonaws.com" },
                        "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"],
                        "Condition": {
                            "StringLike": {
                                "aws:sourceArn": ["arn:aws:lambda:us-east-2:123456789012:function:*"]
                            }
                        }
                    }
                ]
            })
        );
    }

    #[test]
    fn aws_cleanup_guard_covers_a_sandbox_only_stack() {
        let state = aws_state_with_stack(sandbox_stack(
            "manager.example.com/alien-artifacts-prj_test:agents-abc123",
        ));

        assert!(
            has_registry_backed_image(&state, &Platform::Aws),
            "a partial AWS sandbox grant must still be cleaned up on delete"
        );
        assert!(
            !has_registry_backed_image(&state, &Platform::Gcp),
            "no GCP sandbox pulls from Alien's registry, so cleanup must stay a no-op there"
        );

        // The guard is deliberately looser than the repository scan and asks only whether an
        // image is declared, matching what a Worker naming a public image has always done.
        let bundle_state = aws_state_with_stack(sandbox_stack("s3://acme-artifacts/bundle.zip"));
        assert!(has_registry_backed_image(&bundle_state, &Platform::Aws));
    }

    #[test]
    fn aws_registry_access_covers_a_worker_and_a_sandbox_together() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: false,
        };
        let stack = Stack::new("test-stack".to_string())
            .add(
                Worker::new("test-worker".to_string())
                    .code(WorkerCode::Image {
                        image: "manager.example.com/alien-artifacts-prj_test:test-worker-abc123"
                            .to_string(),
                    })
                    .permissions("execution".to_string())
                    .build(),
                ResourceLifecycle::Live,
            )
            .add(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: "manager.example.com/alien-artifacts-prj_test-sandbox:agents-abc123"
                            .to_string(),
                    })
                    .egress(SandboxEgress::Allow)
                    .session(SandboxSessionPolicy {
                        max_lifetime_seconds: None,
                        idle_suspend_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();

        assert_eq!(
            repository_ids_for_access(&registry, &aws_state_with_stack(stack)),
            vec![
                "alien-artifacts-prj_test".to_string(),
                "alien-artifacts-prj_test-sandbox".to_string()
            ],
            "each resource keeps its own repository and neither displaces the other"
        );
    }

    #[test]
    fn gcp_registry_access_ignores_sandbox_images() {
        let registry = TestArtifactRegistry {
            prefix: "test-project/alien-artifacts".to_string(),
            fail_remove: false,
        };
        let state = gcp_state_with_stack(sandbox_stack(
            "manager.example.com/test-project/alien-artifacts/agents:abc123",
        ));

        assert!(
            repository_ids_for_access(&registry, &state).is_empty(),
            "a GCP sandbox takes no image from Alien's registry, so it must grant nothing"
        );
    }
}
