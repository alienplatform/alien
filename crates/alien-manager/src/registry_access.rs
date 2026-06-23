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
    Platform, RemoteStackManagementOutputs, RuntimeMetadata, Stack, StackState, Worker, WorkerCode,
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

        // The shared deployment-image repository is named after
        // `upstream_repository_prefix()` — the same identifier the proxy
        // routes pushes to and `alien release` writes images to. Empty means
        // the platform has no such repo (Azure ACR pushes to the registry
        // root; Local doesn't support cross-account).
        let repo_id = artifact_registry.upstream_repository_prefix();
        if repo_id.is_empty() {
            return;
        }

        // Revoke cross-account access (AWS/GCP only; Azure/Local are no-ops).
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
        collect_worker_image_repositories(state, &prefix, &mut repo_ids);
        let mut repo_ids: Vec<_> = repo_ids.into_iter().collect();
        repo_ids.sort();
        return repo_ids;
    }

    if !has_worker_image_in_repository_prefix(state, &prefix) {
        return Vec::new();
    }

    let repo_ids = HashSet::from([prefix]);
    let mut repo_ids: Vec<_> = repo_ids.into_iter().collect();
    repo_ids.sort();
    repo_ids
}

fn collect_worker_image_repositories(
    state: &DeploymentState,
    prefix: &str,
    repo_ids: &mut HashSet<String>,
) {
    if let Some(release) = &state.current_release {
        collect_worker_image_repositories_from_stack(&release.stack, prefix, repo_ids);
    }
    if let Some(release) = &state.target_release {
        collect_worker_image_repositories_from_stack(&release.stack, prefix, repo_ids);
    }
    if let Some(runtime_metadata) = &state.runtime_metadata {
        if let Some(stack) = &runtime_metadata.prepared_stack {
            collect_worker_image_repositories_from_stack(stack, prefix, repo_ids);
        }
    }
}

fn collect_worker_image_repositories_from_stack(
    stack: &Stack,
    prefix: &str,
    repo_ids: &mut HashSet<String>,
) {
    for (_id, entry) in stack.resources() {
        let Some(worker) = entry.config.downcast_ref::<Worker>() else {
            continue;
        };
        let WorkerCode::Image { image } = &worker.code else {
            continue;
        };
        if let Some(repo_id) = ecr_repository_from_image(image, prefix) {
            repo_ids.insert(repo_id);
        }
    }
}

fn has_worker_image_in_repository_prefix(state: &DeploymentState, prefix: &str) -> bool {
    state
        .current_release
        .as_ref()
        .is_some_and(|release| stack_has_worker_image_in_repository_prefix(&release.stack, prefix))
        || state.target_release.as_ref().is_some_and(|release| {
            stack_has_worker_image_in_repository_prefix(&release.stack, prefix)
        })
        || state
            .runtime_metadata
            .as_ref()
            .and_then(|runtime_metadata| runtime_metadata.prepared_stack.as_ref())
            .is_some_and(|stack| stack_has_worker_image_in_repository_prefix(stack, prefix))
}

fn stack_has_worker_image_in_repository_prefix(stack: &Stack, prefix: &str) -> bool {
    stack.resources().any(|(_id, entry)| {
        let Some(worker) = entry.config.downcast_ref::<Worker>() else {
            return false;
        };
        let WorkerCode::Image { image } = &worker.code else {
            return false;
        };
        image_repository_matches_prefix(image, prefix)
    })
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
    use alien_bindings::error::{ErrorData as BindingErrorData, Result};
    use alien_bindings::traits::{
        ArtifactRegistryCredentials, ArtifactRegistryPermissions, BindingsProviderApi,
        CrossAccountPermissions, RepositoryResponse,
    };
    use alien_core::{ReleaseInfo, ResourceLifecycle};
    use alien_error::AlienError;
    use async_trait::async_trait;

    #[derive(Debug)]
    struct TestArtifactRegistry {
        prefix: String,
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

        async fn create_repository(&self, _repo_name: &str) -> Result<RepositoryResponse> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn get_repository(&self, _repo_id: &str) -> Result<RepositoryResponse> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn add_cross_account_access(
            &self,
            _repo_id: &str,
            _access: CrossAccountAccess,
        ) -> Result<()> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn remove_cross_account_access(
            &self,
            _repo_id: &str,
            _access: CrossAccountAccess,
        ) -> Result<()> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn get_cross_account_access(
            &self,
            _repo_id: &str,
        ) -> Result<CrossAccountPermissions> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn generate_credentials(
            &self,
            _repo_id: &str,
            _permissions: ArtifactRegistryPermissions,
            _ttl_seconds: Option<u32>,
        ) -> Result<ArtifactRegistryCredentials> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn delete_repository(&self, _repo_id: &str) -> Result<()> {
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
            reason: "not found".to_string(),
        })
    }

    #[async_trait]
    impl BindingsProviderApi for TestBindingsProvider {
        async fn load_artifact_registry(
            &self,
            binding_name: &str,
        ) -> Result<Arc<dyn ArtifactRegistry>> {
            if binding_name == self.binding_name {
                Ok(self.registry.clone())
            } else {
                Err(missing_binding(binding_name))
            }
        }

        async fn load_storage(
            &self,
            binding_name: &str,
        ) -> Result<Arc<dyn alien_bindings::traits::Storage>> {
            Err(missing_binding(binding_name))
        }

        async fn load_build(
            &self,
            binding_name: &str,
        ) -> Result<Arc<dyn alien_bindings::traits::Build>> {
            Err(missing_binding(binding_name))
        }

        async fn load_vault(
            &self,
            binding_name: &str,
        ) -> Result<Arc<dyn alien_bindings::traits::Vault>> {
            Err(missing_binding(binding_name))
        }

        async fn load_kv(&self, binding_name: &str) -> Result<Arc<dyn alien_bindings::traits::Kv>> {
            Err(missing_binding(binding_name))
        }

        async fn load_queue(
            &self,
            binding_name: &str,
        ) -> Result<Arc<dyn alien_bindings::traits::Queue>> {
            Err(missing_binding(binding_name))
        }

        async fn load_worker(
            &self,
            binding_name: &str,
        ) -> Result<Arc<dyn alien_bindings::traits::Worker>> {
            Err(missing_binding(binding_name))
        }

        async fn load_container(
            &self,
            binding_name: &str,
        ) -> Result<Arc<dyn alien_bindings::traits::Container>> {
            Err(missing_binding(binding_name))
        }

        async fn load_service_account(
            &self,
            binding_name: &str,
        ) -> Result<Arc<dyn alien_bindings::traits::ServiceAccount>> {
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

    #[tokio::test]
    async fn load_artifact_registry_prefers_target_provider() {
        let primary_registry: Arc<dyn ArtifactRegistry> = Arc::new(TestArtifactRegistry {
            prefix: "artifacts/default".to_string(),
        });
        let target_registry: Arc<dyn ArtifactRegistry> = Arc::new(TestArtifactRegistry {
            prefix: "alien-e2e".to_string(),
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
        };
        let state = aws_state_with_stack(Stack::new("test-stack".to_string()).build());

        assert!(repository_ids_for_access(&registry, &state).is_empty());
    }

    #[test]
    fn aws_registry_access_uses_worker_image_repository() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
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
    fn gcp_registry_access_requires_worker_image_under_prefix() {
        let registry = TestArtifactRegistry {
            prefix: "test-project/alien-artifacts".to_string(),
        };
        let state = gcp_state_with_stack(worker_stack(
            "manager.example.com/test-project/alien-artifacts/test-worker:abc123",
        ));

        assert_eq!(
            repository_ids_for_access(&registry, &state),
            vec!["test-project/alien-artifacts".to_string()]
        );
    }
}
