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
    Platform, RegistryAccess, RemoteStackManagementOutputs, ResourceEntry, RuntimeMetadata,
    Sandbox, SandboxCode, Stack, StackState, Worker, WorkerCode,
};
use alien_error::{AlienError, Context};
use tracing::{debug, info, warn};

use crate::auth::Subject;
use crate::error::{ErrorData, Result};
use crate::traits::DeploymentStore;

/// What one grant attempt left behind. `written` marks that a write was attempted, so cleanup
/// still has something to revoke even from a partial failure. `complete` marks that the grant
/// covers everything needed, which is what lets reconcile stop retrying.
#[derive(Clone, Copy, PartialEq, Eq)]
struct GrantOutcome {
    written: bool,
    complete: bool,
}

/// Grants cross-account registry access on one repository for a deployment.
async fn ensure_registry_access(
    artifact_registry: &dyn ArtifactRegistry,
    repo_id: &str,
    environment_info: &EnvironmentInfo,
    stack_state: Option<&StackState>,
    declares_sandbox: bool,
) -> GrantOutcome {
    let rsm_access = extract_rsm_management_identity(stack_state);
    // The project number is what names the compute agents, and an import predating that field
    // leaves it empty — the write then carries the management identity alone and would seal a
    // grant no agent is on, which nothing the record compares could ever reopen.
    let names_compute_agents = !matches!(
        environment_info,
        EnvironmentInfo::Gcp(GcpEnvironmentInfo { project_number, .. }) if project_number.is_empty()
    );
    let has_management_sa = rsm_access.is_some();

    let access = match build_cross_account_access(environment_info, stack_state, declares_sandbox) {
        Some(a) => a,
        None => {
            return GrantOutcome {
                written: false,
                complete: false,
            }
        }
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
            // The policy now holds this deployment's members either way, so cleanup must revoke
            // it. It is only complete once the management SA is on it: Cloud Run and Lambda need
            // that identity to update a service from a cross-project image, and the RSM outputs
            // naming it can arrive after the first grant.
            GrantOutcome {
                written: true,
                complete: has_management_sa && names_compute_agents,
            }
        }
        Err(e) => {
            warn!(
                repo_id = %repo_id,
                platform = %environment_info.platform(),
                error = %e,
                "Failed to grant registry cross-account access"
            );
            GrantOutcome {
                written: true,
                complete: false,
            }
        }
    }
}

/// Revokes cross-account registry access for a deployment during cleanup.
///
/// Called during sync/reconcile once runtime teardown is complete.
async fn revoke_registry_access(
    artifact_registry: &dyn ArtifactRegistry,
    repo_id: &str,
    environment_info: &EnvironmentInfo,
    stack_state: Option<&StackState>,
    deployment_store: &dyn DeploymentStore,
    deployment_id: &str,
    project_id: &str,
) -> Result<()> {
    if let EnvironmentInfo::Gcp(GcpEnvironmentInfo { project_number, .. }) = environment_info {
        if let Some(service_account_email) = extract_rsm_management_identity(stack_state) {
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

        // The binding's own repository held every Alien project's images before per-project
        // repositories, so a deployment of any project in this GCP project still pulls from it.
        let consumer_project = (repo_id != artifact_registry.upstream_repository_prefix())
            .then(|| project_id.to_string());
        let shared_with_another_deployment = deployment_store
            .has_other_gcp_project_deployment(
                &Subject::system(),
                project_number,
                consumer_project,
                deployment_id,
            )
            .await
            .context(ErrorData::RegistryAccessCleanupFailed {
                deployment_id: deployment_id.to_string(),
                reason: "active GCP deployments could not be checked before revoking shared access"
                    .to_string(),
            })?;
        if shared_with_another_deployment {
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
                // Every type, not this deployment's: the member names the project, and the guard
                // above has already proved no deployment is left in it. Removal is a `retain`, so
                // naming one this deployment never added removes nothing.
                allowed_service_types: all_gcp_service_types(),
                service_account_emails: Vec::new(),
            }),
            "last project consumer's shared registry access",
            deployment_id,
        )
        .await?;
        return Ok(());
    }

    // The AWS arm names its own principal, so the sandbox flag does not reach it.
    let Some(access) = build_cross_account_access(environment_info, stack_state, false) else {
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
    // Read before the wrap: `RegistryAccessCleanupFailed` answers 500 for every source, so a
    // repository that is already gone is only visible here. It carries no grant, and a delete
    // that keeps failing on it never completes.
    match artifact_registry
        .remove_cross_account_access(repo_id, access)
        .await
    {
        Ok(()) => {}
        Err(error) if error.http_status_code == Some(404) => {
            info!(
                repo_id = %repo_id,
                platform = %environment_info.platform(),
                access_kind = %access_kind,
                "Repository is already gone, so its access is too"
            );
            return Ok(());
        }
        // A role the manager has held for the deployment's whole life is not still propagating,
        // so a denial is a permission it lacks. Retrying cannot grant it.
        Err(error)
            if matches!(
                error.error,
                Some(alien_bindings::error::ErrorData::RemoteAccessDenied { .. })
            ) =>
        {
            return Err(error).context(ErrorData::RegistryAccessRevokeDenied {
                deployment_id: deployment_id.to_string(),
                repository: repo_id.to_string(),
            })
        }
        Err(error) => {
            return Err(error).context(ErrorData::RegistryAccessCleanupFailed {
                deployment_id: deployment_id.to_string(),
                reason: format!("failed to revoke {access_kind} for repository '{repo_id}'"),
            })
        }
    }

    info!(
        repo_id = %repo_id,
        platform = %environment_info.platform(),
        access_kind = %access_kind,
        "Registry access revoked"
    );
    Ok(())
}

/// Loads the artifact registry from the bindings provider and applies the
/// appropriate grant or revoke based on deployment status.
///
/// Records what the grant opened, and grants again whenever that record differs from what the
/// deployment needs now — a repository it gained, a compute service that pulls as another
/// principal — or when the last attempt did not finish. A repository it stopped needing stays in
/// the record so the delete still revokes it. The caller persists the updated state.
///
/// Returns without error if the bindings provider is unavailable or the
/// artifact registry cannot be loaded — registry access is best-effort.
pub async fn reconcile_registry_access(
    bindings_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    deployment_id: &str,
    project_id: &str,
    state: &mut DeploymentState,
) {
    reconcile_registry_access_with_proxy()
        .bindings_provider(bindings_provider)
        .target_bindings_providers(target_bindings_providers)
        .deployment_id(deployment_id)
        .project_id(project_id)
        .state(state)
        .call()
        .await;
}

/// Reconciles registry access, also recognizing worker images published through the configured
/// manager proxy. The proxy URL must come from manager configuration, never from a release.
#[bon::builder]
pub async fn reconcile_registry_access_with_proxy(
    bindings_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    deployment_id: &str,
    project_id: &str,
    state: &mut DeploymentState,
    proxy_url: Option<&str>,
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

    let recorded = state
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.registry_access.clone());

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

    let (repo_ids, refused) =
        repository_access_with_proxy(artifact_registry.as_ref(), state, project_id, proxy_url);
    for image in &refused {
        warn!(
            deployment_id = %deployment_id,
            resource_id = %image.resource_id,
            repository = %image.repository,
            project_id = %project_id,
            "Refusing registry access for an image outside its project's repository"
        );
    }
    let refused_any = !refused.is_empty();
    if repo_ids.is_empty() {
        let metadata = state
            .runtime_metadata
            .get_or_insert_with(RuntimeMetadata::default);
        // A grant made before the record existed is named by this marker alone; clearing it would
        // leave the delete nothing to revoke.
        if refused_any && metadata.registry_access.is_some() {
            metadata.registry_access_granted = false;
        }
        return;
    }
    let sandbox_declared = declares_sandbox(state);
    let needed = RegistryAccess {
        repositories: repo_ids.clone(),
        service_types: service_type_names(environment_info, sandbox_declared),
    };

    // What the policy already says, not merely that something was once written: a deployment that
    // gained a resource pulling as another principal, or a repository it did not have, needs the
    // policy rewritten, and re-writing an unchanged one costs a call every tick.
    let complete = state
        .runtime_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.registry_access_granted);
    if !needs_regrant(recorded.as_ref(), &needed, complete) {
        return;
    }

    // Each repository joins the record as it is proved, so a grant that fails halfway still names
    // what it opened. Assembling the whole list first would leave a live grant unrecorded.
    let mut opened: Vec<String> = Vec::new();
    let mut all_complete = true;
    for repo_id in &repo_ids {
        let outcome = ensure_registry_access(
            artifact_registry.as_ref(),
            repo_id,
            environment_info,
            state.stack_state.as_ref(),
            sandbox_declared,
        )
        .await;
        if outcome.written {
            opened.push(repo_id.clone());
        }
        all_complete &= outcome.complete;
    }

    // A repository the deployment stopped naming keeps its grant, recorded so the delete revokes
    // it. Revoking it here would cut a sibling deployment off a policy it still pulls through.
    opened.extend(previously_opened(recorded.as_ref(), &repo_ids));
    opened.extend(legacy_shared_repository(
        artifact_registry.as_ref(),
        state,
        &platform,
    ));

    opened.sort();
    opened.dedup();
    let granted_everything =
        all_complete && repo_ids.iter().all(|repo_id| opened.contains(repo_id));
    let rm = state
        .runtime_metadata
        .get_or_insert_with(RuntimeMetadata::default);
    // The record says what the policy now holds, so an unchanged one is not rewritten every tick.
    // The marker tracks completion of the allowed grants, not image validation. A refused
    // reference must not make an unchanged, successful grant write again on every tick.
    rm.registry_access = Some(RegistryAccess {
        repositories: opened,
        service_types: needed.service_types,
    });
    rm.registry_access_granted = granted_everything;
}

/// Whether the policy has to be written again.
///
/// Repositories compare by containment — the record may be a superset, since a dropped one stays
/// for the delete to revoke. Service types must match exactly, since any change needs the policy
/// rewritten. An unfinished write (`complete` false) is retried regardless of either.
fn needs_regrant(
    recorded: Option<&RegistryAccess>,
    needed: &RegistryAccess,
    complete: bool,
) -> bool {
    let Some(recorded) = recorded else {
        return true;
    };
    !complete
        || recorded.service_types != needed.service_types
        || !needed
            .repositories
            .iter()
            .all(|repository| recorded.repositories.contains(repository))
}

/// Repositories the record names that the deployment no longer needs.
/// The GCP binding's own repository, when this deployment may hold a grant written there before
/// per-project repositories: one recorded by the marker alone, or one for an image under it.
///
/// Every older project's images live in that repository, so a grant left on it after teardown
/// keeps the target project's service agent reading all of them.
fn legacy_shared_repository(
    artifact_registry: &dyn ArtifactRegistry,
    state: &DeploymentState,
    platform: &Platform,
) -> Option<String> {
    if *platform != Platform::Gcp {
        return None;
    }
    let shared = artifact_registry.upstream_repository_prefix();
    if shared.is_empty() {
        return None;
    }
    let marker_only = state.runtime_metadata.as_ref().is_some_and(|metadata| {
        metadata.registry_access_granted && metadata.registry_access.is_none()
    });
    let image_under_shared = |stack: &Stack| {
        stack.resources().any(|(_, entry)| {
            [SandboxImageSource::None, sandbox_image_source(*platform)]
                .into_iter()
                .filter_map(|source| resource_image_reference(entry, source))
                .filter_map(image_repository_path)
                .any(|path| path.starts_with(&format!("{shared}/")))
        })
    };
    let legacy_image = state
        .current_release
        .as_ref()
        .is_some_and(|release| image_under_shared(&release.stack))
        || state
            .target_release
            .as_ref()
            .is_some_and(|release| image_under_shared(&release.stack))
        || state
            .runtime_metadata
            .as_ref()
            .and_then(|metadata| metadata.prepared_stack.as_ref())
            .is_some_and(image_under_shared);
    (marker_only || legacy_image).then_some(shared)
}

fn previously_opened(recorded: Option<&RegistryAccess>, needed: &[String]) -> Vec<String> {
    recorded
        .map(|access| {
            access
                .repositories
                .iter()
                .filter(|repository| !needed.contains(repository))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// The compute services a grant admits on this platform, as the record stores them.
///
/// Read through `build_cross_account_access`, the same call the grant itself makes — a record
/// derived any other way could disagree with the policy it describes, and the comparison that
/// stops the retry would never settle.
fn service_type_names(environment_info: &EnvironmentInfo, declares_sandbox: bool) -> Vec<String> {
    let mut names: Vec<String> =
        match build_cross_account_access(environment_info, None, declares_sandbox) {
            Some(CrossAccountAccess::Aws(access)) => access
                .allowed_service_types
                .iter()
                .map(service_type_name)
                .collect(),
            Some(CrossAccountAccess::Gcp(access)) => access
                .allowed_service_types
                .iter()
                .map(service_type_name)
                .collect(),
            _ => Vec::new(),
        };
    names.sort();
    names
}

/// These strings are persisted in `RegistryAccess::service_types` and compared by `needs_regrant`,
/// so renaming one re-grants every existing deployment on its next reconcile.
fn service_type_name(service_type: &ComputeServiceType) -> String {
    match service_type {
        ComputeServiceType::Worker => "worker".to_string(),
        ComputeServiceType::Sandbox => "sandbox".to_string(),
    }
}

/// Revokes registry access once runtime teardown is complete.
/// Revoke at TeardownRequired before imported or native setup deletion.
///
/// Cloud Run's service agent is project-scoped, so multiple deployments in
/// the same GCP project and Alien project share one GAR reader grant. The final active consumer
/// removes that shared grant; every deployment still removes its own remote
/// management service-account grant.
pub async fn cleanup_deleted_registry_access(
    deployment_store: &dyn DeploymentStore,
    bindings_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    deployment_id: &str,
    project_id: &str,
    state: &DeploymentState,
) -> Result<()> {
    // Setup deletion removes identity outputs. Revoke deployment-owned access while
    // runtime teardown is complete and those outputs are still recorded.
    match (state.status, state.environment_info.as_ref()) {
        (DeploymentStatus::Deleted, _)
        | (
            DeploymentStatus::TeardownRequired,
            Some(EnvironmentInfo::Aws(_) | EnvironmentInfo::Gcp(_)),
        ) => {}
        _ => return Ok(()),
    }
    let Some(environment_info) = state.environment_info.as_ref() else {
        return Ok(());
    };
    let platform = environment_info.platform();
    if !matches!(platform, Platform::Aws | Platform::Gcp) {
        return Ok(());
    }
    // A record names a live grant even when the marker is false: an incomplete grant is still a
    // grant. Reading the marker alone would walk past one and leave the customer's read in place.
    let recorded_grant = state.runtime_metadata.as_ref().is_some_and(|metadata| {
        metadata.registry_access_granted || metadata.registry_access.is_some()
    });
    if !recorded_grant && !has_registry_backed_image(state, &platform) {
        return Ok(());
    }

    let artifact_registry = match find_artifact_registry(
        bindings_provider,
        target_bindings_providers,
        &platform,
    )
    .await
    {
        RegistryLookup::Loaded(registry) => registry,
        // The IAM write lands before the state that records it, so a binding that exists and
        // will not answer may be hiding a grant this delete is the last chance to revoke.
        RegistryLookup::Unavailable => {
            return Err(AlienError::new(ErrorData::RegistryAccessCleanupFailed {
                deployment_id: deployment_id.to_string(),
                reason: format!("artifact registry binding for '{platform}' is unavailable"),
            }))
        }
        // No binding was ever configured, so no grant was ever written through one. The
        // deployment reached here only because `has_registry_backed_image` cannot tell an
        // image in Alien's registry from a public one without the prefix that binding carries.
        RegistryLookup::NotConfigured => {
            if recorded_grant {
                return Err(AlienError::new(ErrorData::RegistryAccessCleanupFailed {
                        deployment_id: deployment_id.to_string(),
                        reason: format!(
                            "artifact registry binding for '{platform}' is not configured, so a recorded grant cannot be revoked"
                        ),
                    }));
            }
            return Ok(());
        }
    };

    // What the grant recorded, not what the stack says now: a release that dropped the base image
    // or the sandbox itself would otherwise leave the grant named nowhere. A grant recorded before
    // the record existed still derives from the stack, which is all it ever had.
    let recorded = state
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.registry_access.as_ref());
    // Both, not either: the IAM write lands before the state that records it is persisted, so a
    // grant made and then lost to a failed save is named only by the stack it was derived from.
    let mut repo_ids = recorded
        .map(|access| access.repositories.clone())
        .unwrap_or_default();
    repo_ids.extend({
        {
            // A sandbox is only ever granted its own project's repository, so that one is revoked
            // when the registry has it; a lookup that fails for any other reason is not an answer.
            let mut derived =
                repository_ids_for_access(artifact_registry.as_ref(), state, project_id);
            if recorded_grant && matches!(platform, Platform::Aws) && declares_sandbox(state) {
                for own in project_repositories(artifact_registry.as_ref(), project_id) {
                    if !derived.contains(&own) {
                        match artifact_registry.get_repository(&own).await {
                            Ok(_) => derived.push(own),
                            Err(error) if error.http_status_code == Some(404) => {}
                            Err(error) => {
                                return Err(error).context(ErrorData::RegistryAccessCleanupFailed {
                                    deployment_id: deployment_id.to_string(),
                                    reason: format!(
                                        "repository '{own}' could not be read to revoke its grant"
                                    ),
                                })
                            }
                        }
                    }
                }
            }
            derived
        }
    });
    // Older worker grants matched the prefix alone. Keep that derivation for cleanup: a policy
    // write can precede the save of its record, and the new grant rule must not strand that write.
    if matches!(platform, Platform::Aws) {
        let prefixes = RepositoryPrefixes::of(artifact_registry.as_ref());
        if !prefixes.current.is_empty() {
            let mut collect = |stack: &Stack| {
                for (_, entry) in stack.resources() {
                    if let Some(image) = resource_image_reference(entry, SandboxImageSource::None) {
                        if let Some((_, repository)) = prefixes.repository_of(image, false) {
                            repo_ids.push(repository);
                        }
                    }
                }
            };
            if let Some(release) = &state.current_release {
                collect(&release.stack);
            }
            if let Some(release) = &state.target_release {
                collect(&release.stack);
            }
            if let Some(stack) = state
                .runtime_metadata
                .as_ref()
                .and_then(|metadata| metadata.prepared_stack.as_ref())
            {
                collect(stack);
            }
        }
    }

    // Every GCP teardown also revokes on the binding's shared repository. A legacy grant there
    // is kept while any deployment remains in the GCP project, so whichever deployment is last,
    // old or new, must be the one that removes it; a missing repository revokes nothing.
    if platform == Platform::Gcp {
        let shared = artifact_registry.upstream_repository_prefix();
        if !shared.is_empty() {
            repo_ids.push(shared);
        }
    }
    repo_ids.sort();
    repo_ids.dedup();
    if repo_ids.is_empty() {
        if recorded_grant {
            // Only a grant recorded before the record existed reaches this: nothing names the
            // repository any more, and failing the delete forever would not recover it either.
            warn!(
                deployment_id = %deployment_id,
                platform = %platform,
                service_types = ?state
                    .runtime_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.registry_access.as_ref())
                    .map(|access| access.service_types.clone()),
                "A recorded grant names no repository to revoke, so it is left for a human"
            );
        }
        return Ok(());
    }
    for repo_id in repo_ids {
        revoke_registry_access(
            artifact_registry.as_ref(),
            &repo_id,
            environment_info,
            state.stack_state.as_ref(),
            deployment_store,
            deployment_id,
            project_id,
        )
        .await?;
    }
    Ok(())
}

/// Revokes registry access before the state that ends its runtime
/// cleanup is persisted, and returns the state to persist instead.
///
/// The revoke goes first because a persisted `TeardownRequired` hands the deployment to setup
/// teardown, which deletes the identity and the record the revoke reads, so a revoke still owed
/// after that is never made. A registry that denies the revoke fails the delete with that error:
/// the manager lacks a permission, and a retry of the delete revokes once it is granted. Any other
/// failure is returned with nothing persisted, for the caller to retry.
///
/// GCP project service agents are checked again in [`revoke_after_persisting`].
pub async fn revoke_before_persisting(
    deployment_store: &dyn DeploymentStore,
    bindings_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    deployment_id: &str,
    project_id: &str,
    state: DeploymentState,
) -> Result<DeploymentState> {
    if !matches!(state.environment_info, Some(EnvironmentInfo::Aws(_)))
        && !(matches!(state.environment_info, Some(EnvironmentInfo::Gcp(_)))
            && state.status == DeploymentStatus::TeardownRequired)
    {
        return Ok(state);
    }
    match cleanup_deleted_registry_access(
        deployment_store,
        bindings_provider,
        target_bindings_providers,
        deployment_id,
        project_id,
        &state,
    )
    .await
    {
        Ok(()) => Ok(state),
        Err(error)
            if matches!(
                error.error,
                Some(ErrorData::RegistryAccessRevokeDenied { .. })
            ) =>
        {
            warn!(
                deployment_id = %deployment_id,
                status = ?state.status,
                error = %error,
                "The registry denied revoking this deployment's access; failing the delete"
            );
            Ok(DeploymentState {
                status: DeploymentStatus::DeleteFailed,
                error: Some(error.into_generic()),
                retry_requested: false,
                ..state
            })
        }
        Err(error) => Err(error),
    }
}

/// Rechecks GCP registry access after runtime teardown or deletion is persisted.
///
/// GCP removes a project's shared grant only when no other deployment in that project still
/// needs its runtime. Rechecking after persistence lets concurrent teardowns observe each other
/// as complete and remove the final grant; checking only before persistence could leave
/// the other active and both keep it. Imported setup may be forgotten after `TeardownRequired`,
/// so that handoff must also remove the last service-agent grant. AWS revokes before persistence.
pub async fn revoke_after_persisting(
    deployment_store: &dyn DeploymentStore,
    bindings_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    deployment_id: &str,
    project_id: &str,
    state: &DeploymentState,
) -> Result<()> {
    if matches!(state.environment_info, Some(EnvironmentInfo::Aws(_))) {
        return Ok(());
    }
    cleanup_deleted_registry_access(
        deployment_store,
        bindings_provider,
        target_bindings_providers,
        deployment_id,
        project_id,
        state,
    )
    .await
}

fn repository_ids_for_access(
    artifact_registry: &dyn ArtifactRegistry,
    state: &DeploymentState,
    project_id: &str,
) -> Vec<String> {
    repository_access(artifact_registry, state, project_id).0
}

/// An image whose repository a grant leaves out, because it is not the deployment's project's.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RefusedImage {
    resource_id: String,
    repository: String,
}

/// The repositories to grant, and the images refused as not this project's.
fn repository_access(
    artifact_registry: &dyn ArtifactRegistry,
    state: &DeploymentState,
    project_id: &str,
) -> (Vec<String>, Vec<RefusedImage>) {
    repository_access_with_proxy(artifact_registry, state, project_id, None)
}

fn repository_access_with_proxy(
    artifact_registry: &dyn ArtifactRegistry,
    state: &DeploymentState,
    project_id: &str,
    proxy_url: Option<&str>,
) -> (Vec<String>, Vec<RefusedImage>) {
    let prefixes = RepositoryPrefixes::of(artifact_registry);
    if prefixes.current.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let platform = state
        .environment_info
        .as_ref()
        .map(EnvironmentInfo::platform);
    let sandbox_source = platform.map_or(SandboxImageSource::None, sandbox_image_source);

    if matches!(platform, Some(Platform::Aws | Platform::Gcp)) {
        let mut repo_ids = HashSet::new();
        let refused = collect_image_repositories(
            state,
            &prefixes,
            &artifact_registry.registry_endpoint(),
            project_id,
            proxy_url,
            sandbox_source,
            &mut repo_ids,
        );
        let mut repo_ids: Vec<_> = repo_ids.into_iter().collect();
        repo_ids.sort();
        return (repo_ids, refused);
    }

    (Vec::new(), Vec::new())
}

/// The principals a GCP grant names: Cloud Run always, and the sandbox agent when a release
/// declares one.
///
/// Naming the sandbox agent unconditionally would leave every worker-only deployment asking for a
/// principal its project will never have, and Google refuses a binding it cannot resolve (see the
/// two-write split in `gar.rs`'s `add_cross_account_access`), so the grant would never settle.
fn gcp_service_types(declares_sandbox: bool) -> Vec<ComputeServiceType> {
    ComputeServiceType::ALL
        .iter()
        .filter(|service_type| match service_type {
            ComputeServiceType::Worker => true,
            ComputeServiceType::Sandbox => declares_sandbox,
        })
        .cloned()
        .collect()
}

/// Every type a GCP grant can name, for a revoke with no record to go on.
///
/// Removal is a `retain`, so naming a type this deployment never added removes nothing, while
/// omitting one it did add strands that member on the policy.
fn all_gcp_service_types() -> Vec<ComputeServiceType> {
    ComputeServiceType::ALL.to_vec()
}

fn declares_sandbox(state: &DeploymentState) -> bool {
    let has_sandbox = |stack: &Stack| {
        stack
            .resources()
            .any(|(_id, entry)| entry.config.downcast_ref::<Sandbox>().is_some())
    };
    state
        .current_release
        .as_ref()
        .is_some_and(|release| has_sandbox(&release.stack))
        || state
            .target_release
            .as_ref()
            .is_some_and(|release| has_sandbox(&release.stack))
        || state
            .runtime_metadata
            .as_ref()
            .and_then(|runtime_metadata| runtime_metadata.prepared_stack.as_ref())
            .is_some_and(has_sandbox)
}

/// Whether this platform's sandbox takes its root filesystem from an image Alien hosts.
///
/// AWS builds a MicroVM image from a published sandbox bundle and a GCP sandbox pulls its container
/// from Alien's registry, so both earn a grant. Azure names an image from its own catalog, and the
/// remaining platforms have no cross-account grant path at all.
fn sandbox_image_source(platform: Platform) -> SandboxImageSource {
    match platform {
        Platform::Aws => SandboxImageSource::PrivateBaseImage,
        Platform::Gcp => SandboxImageSource::Code,
        _ => SandboxImageSource::None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SandboxImageSource {
    None,
    PrivateBaseImage,
    Code,
}

/// The image a resource pulls from Alien's registry, if it declares one.
///
/// A worker names it in `code.image`. An AWS sandbox's `code.image` is its S3 bundle URI and
/// matches no repository prefix, so `privateBaseImage`, what its Dockerfile pulls, is the only
/// field naming the repository its build needs opened.
fn resource_image_reference(
    entry: &ResourceEntry,
    sandbox_source: SandboxImageSource,
) -> Option<&str> {
    if let Some(worker) = entry.config.downcast_ref::<Worker>() {
        return match &worker.code {
            WorkerCode::Image { image } => Some(image),
            _ => None,
        };
    }
    if let Some(sandbox) = entry.config.downcast_ref::<Sandbox>() {
        return match sandbox_source {
            SandboxImageSource::PrivateBaseImage => sandbox.private_base_image.as_deref(),
            SandboxImageSource::Code => match &sandbox.code {
                SandboxCode::Image { image } => Some(image),
                _ => None,
            },
            SandboxImageSource::None => None,
        };
    }
    None
}

/// The repositories a sandbox in this project can be granted: its own under the current prefix,
/// and under each legacy one a grant may have been written to before the registry was renamed.
/// Empty when the registry does not name repositories by prefix.
fn project_repositories(artifact_registry: &dyn ArtifactRegistry, project_id: &str) -> Vec<String> {
    let prefix = artifact_registry.upstream_repository_prefix();
    if prefix.is_empty() {
        return Vec::new();
    }
    std::iter::once(prefix)
        .chain(artifact_registry.legacy_repository_prefixes())
        .map(|prefix| format!("{prefix}-{project_id}"))
        .collect()
}

/// The prefixes a registry names repositories under: the ones it writes now, and the legacy ones
/// whose released images it still serves and grants.
struct RepositoryPrefixes {
    current: String,
    sandbox: Option<String>,
    legacy: Vec<String>,
    legacy_sandbox: Vec<String>,
}

impl RepositoryPrefixes {
    fn of(artifact_registry: &dyn ArtifactRegistry) -> Self {
        Self {
            current: artifact_registry.upstream_repository_prefix(),
            sandbox: artifact_registry.sandbox_repository_prefix(),
            legacy: artifact_registry.legacy_repository_prefixes(),
            legacy_sandbox: artifact_registry.legacy_sandbox_repository_prefixes(),
        }
    }

    /// The prefix `image`'s repository sits under, and that repository. A sandbox's image may be
    /// in a sandbox namespace; anything else is matched against the repository prefixes only.
    fn repository_of(&self, image: &str, is_sandbox: bool) -> Option<(&str, String)> {
        let sandbox = self
            .sandbox
            .iter()
            .chain(&self.legacy_sandbox)
            .filter(|_| is_sandbox);
        sandbox
            .chain(std::iter::once(&self.current))
            .chain(&self.legacy)
            .find_map(|prefix| {
                repository_path_under_prefix(image, prefix).map(|path| (prefix.as_str(), path))
            })
    }
}

/// The account and region a `{account}.dkr.ecr.{region}.amazonaws.com` host names.
fn ecr_registry_identity(host: &str) -> Option<(&str, &str)> {
    let host = alien_core::image_rewrite::strip_url_scheme(host);
    let host = host.split('/').next()?;
    let (account, rest) = host.split_once(".dkr.ecr.")?;
    // `.com.cn` first: a China host ends with the shorter suffix too, and stripping that one
    // would leave `.cn` on the region and refuse an image the build can pull.
    let region = rest
        .strip_suffix(".amazonaws.com.cn")
        .or_else(|| rest.strip_suffix(".amazonaws.com"))?;
    Some((account, region))
}

/// Whether an image is served by the registry the grant is written on. A stored reference keeps
/// the `{region}` token, which stands for whichever region the deployment renders; any other
/// region names a different repository than the one being opened.
fn served_by_registry(image: &str, registry_endpoint: &str) -> bool {
    let host = alien_core::image_rewrite::strip_url_scheme(image)
        .split('/')
        .next();
    let registry_host = alien_core::image_rewrite::strip_url_scheme(registry_endpoint)
        .split('/')
        .next();
    if registry_host.is_some_and(|host| host.ends_with("-docker.pkg.dev")) {
        return host == registry_host;
    }
    let (Some((account, region)), Some((registry_account, registry_region))) = (
        ecr_registry_identity(image),
        ecr_registry_identity(registry_endpoint),
    ) else {
        return false;
    };
    account == registry_account && (region == "{region}" || region == registry_region)
}

fn collect_image_repositories(
    state: &DeploymentState,
    prefixes: &RepositoryPrefixes,
    registry_endpoint: &str,
    project_id: &str,
    proxy_url: Option<&str>,
    sandbox_source: SandboxImageSource,
    repo_ids: &mut HashSet<String>,
) -> Vec<RefusedImage> {
    let mut refused = Vec::new();
    // Every project shares the prefix, so matching the prefix alone would open a repository
    // outside the deployment's project. Workers and sandbox base images follow the same rule,
    // under whichever prefix, current or legacy, the image was released.
    let prefix = prefixes.current.as_str();
    let mut collect = |stack: &Stack| {
        for (id, entry) in stack.resources() {
            let Some(image) = resource_image_reference(entry, sandbox_source) else {
                continue;
            };
            let is_sandbox = entry.config.downcast_ref::<Sandbox>().is_some();
            let Some((image_prefix, image_path)) = prefixes.repository_of(image, is_sandbox) else {
                continue;
            };
            let own_repository = format!("{image_prefix}-{project_id}");
            let (repo_id, own_repository) = if let Some((cloud_project, gar_prefix)) = prefix
                .split_once('/')
                .filter(|_| matches!(state.environment_info, Some(EnvironmentInfo::Gcp(_))))
            {
                let path = image_path
                    .strip_prefix(&format!("{cloud_project}/"))
                    .unwrap_or_default();
                let owner = alien_core::gar_repository::project_id_for_path(path, gar_prefix);
                let repository = path.split('/').next().unwrap_or_default();
                (
                    format!("{cloud_project}/{repository}"),
                    owner == Some(project_id),
                )
            } else {
                let own = image_path == own_repository
                    || (entry.config.downcast_ref::<Worker>().is_some()
                        && project_id == "default"
                        && image_path == image_prefix);
                (image_path, own)
            };
            // The host too: the grant is written on this registry, so a same-path repository in
            // another account would be granted here and pulled from there.
            let is_worker = entry.config.downcast_ref::<Worker>().is_some();
            let proxy_image = (is_worker || sandbox_source == SandboxImageSource::Code)
                && proxy_url.is_some_and(|proxy| {
                    let host = alien_core::image_rewrite::strip_url_scheme(image)
                        .split_once('/')
                        .map(|(host, _)| host);
                    host == Some(alien_core::image_rewrite::strip_url_scheme(proxy))
                });
            let own_registry = served_by_registry(image, registry_endpoint) || proxy_image;
            // Returned, not logged: cleanup derives repositories here without the proxy URL,
            // so only the grant path knows a refusal is real.
            if !own_repository || !own_registry {
                refused.push(RefusedImage {
                    resource_id: id.to_string(),
                    repository: repo_id,
                });
                continue;
            }
            repo_ids.insert(repo_id);
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
    refused
}

/// Whether the deployment declares any image that could have earned a registry grant.
///
/// Deliberately over-inclusive: the repository prefix is only known once the registry binding
/// loads, so answering `true` too often costs a lookup, while answering `false` too often leaves
/// a live cross-account grant on Alien's registry with nothing left to revoke it.
fn has_registry_backed_image(state: &DeploymentState, platform: &Platform) -> bool {
    let sandbox_source = sandbox_image_source(*platform);

    state
        .current_release
        .as_ref()
        .is_some_and(|release| stack_has_registry_backed_image(&release.stack, sandbox_source))
        || state
            .target_release
            .as_ref()
            .is_some_and(|release| stack_has_registry_backed_image(&release.stack, sandbox_source))
        || state
            .runtime_metadata
            .as_ref()
            .and_then(|runtime_metadata| runtime_metadata.prepared_stack.as_ref())
            .is_some_and(|stack| stack_has_registry_backed_image(stack, sandbox_source))
}

fn stack_has_registry_backed_image(stack: &Stack, sandbox_source: SandboxImageSource) -> bool {
    stack
        .resources()
        .any(|(_id, entry)| resource_image_reference(entry, sandbox_source).is_some())
}

fn repository_path_under_prefix(image: &str, prefix: &str) -> Option<String> {
    let repository = image_repository_path(image)?;

    if repository == prefix
        || repository.starts_with(&format!("{prefix}-"))
        || (prefix.contains('/') && repository.starts_with(&format!("{prefix}/")))
    {
        Some(repository)
    } else {
        None
    }
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

/// The management identity from the stack's remote management outputs, to include in
/// cross-account access: the role ARN on AWS, the service account email on GCP.
///
/// This is `management_resource_id`, not `access_configuration`: a federated GCP deployment's
/// access configuration is its federation descriptor, not an email.
fn extract_rsm_management_identity(stack_state: Option<&StackState>) -> Option<String> {
    let stack_state = stack_state?;
    for (_id, resource) in &stack_state.resources {
        if resource.resource_type == "remote-stack-management" {
            if let Some(ref outputs) = resource.outputs {
                if let Some(rsm) = outputs.downcast_ref::<RemoteStackManagementOutputs>() {
                    return Some(rsm.management_resource_id.clone());
                }
            }
        }
    }
    None
}

fn build_cross_account_access(
    environment_info: &EnvironmentInfo,
    stack_state: Option<&StackState>,
    declares_sandbox: bool,
) -> Option<CrossAccountAccess> {
    let rsm_access = extract_rsm_management_identity(stack_state);

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
                allowed_service_types: gcp_service_types(declares_sandbox),
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
    match find_artifact_registry(primary_provider, target_providers, platform).await {
        RegistryLookup::Loaded(registry) => Some(registry),
        _ => None,
    }
}

/// What the probe for a registry binding found.
///
/// The delete turns on the difference: a manager that never had a binding could never have
/// granted anything, while one whose binding is unreachable may be hiding a live grant.
enum RegistryLookup {
    Loaded(Arc<dyn ArtifactRegistry>),
    NotConfigured,
    Unavailable,
}

async fn find_artifact_registry(
    primary_provider: &Option<Arc<dyn BindingsProviderApi>>,
    target_providers: &HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    platform: &Platform,
) -> RegistryLookup {
    let mut unavailable = false;
    let mut probe = |result: alien_bindings::error::Result<Arc<dyn ArtifactRegistry>>| match result
    {
        Ok(registry) => Some(registry),
        Err(error) => {
            // `BindingNotConfigured` is the contract for "no such binding"; anything else means
            // one is configured and did not answer.
            unavailable |= !matches!(
                error.error,
                Some(alien_bindings::error::ErrorData::BindingNotConfigured { .. })
            );
            None
        }
    };

    if let Some(target) = target_providers.get(platform) {
        for binding_name in ["artifacts", "artifact-registry"] {
            if let Some(registry) = probe(target.load_artifact_registry(binding_name).await) {
                return RegistryLookup::Loaded(registry);
            }
        }
    }

    if let Some(ref primary) = primary_provider {
        for binding_name in ["artifact-registry", "artifacts"] {
            if let Some(registry) = probe(primary.load_artifact_registry(binding_name).await) {
                return RegistryLookup::Loaded(registry);
            }
        }
    }

    if unavailable {
        return RegistryLookup::Unavailable;
    }

    RegistryLookup::NotConfigured
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
        ResourceStatus, SandboxEgress, SandboxLifecyclePolicy, StackResourceState,
    };
    use alien_error::AlienError;
    use async_trait::async_trait;
    use std::sync::Mutex;

    use crate::traits::deployment_store::MockDeploymentStore;

    #[derive(Debug)]
    struct TestArtifactRegistry {
        prefix: String,
        fail_remove: bool,
        removed_repository_is_gone: bool,
    }

    impl alien_bindings::traits::Binding for TestArtifactRegistry {}

    #[async_trait]
    impl ArtifactRegistry for TestArtifactRegistry {
        fn sandbox_repository_prefix(&self) -> Option<String> {
            Some("sandbox-acme/alien-artifacts".to_string())
        }

        fn registry_endpoint(&self) -> String {
            if self.prefix.contains('/') {
                "https://region-docker.pkg.dev".to_string()
            } else {
                "https://123456789012.dkr.ecr.us-east-2.amazonaws.com".to_string()
            }
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
            if self.removed_repository_is_gone {
                return Err(AlienError::new(BindingErrorData::RemoteResourceNotFound {
                    operation_context: "removing cross-account access".to_string(),
                    resource_type: "repository".to_string(),
                    resource_name: _repo_id.to_string(),
                }));
            }
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

    #[derive(Debug, Default)]
    struct RecordingAwsRegistry {
        grants: Mutex<Vec<String>>,
        removals: Mutex<Vec<String>>,
    }

    impl alien_bindings::traits::Binding for RecordingAwsRegistry {}

    #[async_trait]
    impl ArtifactRegistry for RecordingAwsRegistry {
        fn registry_endpoint(&self) -> String {
            TestArtifactRegistry {
                prefix: String::new(),
                fail_remove: false,
                removed_repository_is_gone: false,
            }
            .registry_endpoint()
        }

        fn upstream_repository_prefix(&self) -> String {
            "alien-artifacts".to_string()
        }

        async fn create_repository(&self, _: &str) -> BindingResult<RepositoryResponse> {
            panic!("reconciliation must not create repositories")
        }

        async fn get_repository(&self, _: &str) -> BindingResult<RepositoryResponse> {
            panic!("worker reconciliation needs no repository lookup")
        }

        async fn add_cross_account_access(
            &self,
            repository: &str,
            access: CrossAccountAccess,
        ) -> BindingResult<()> {
            let CrossAccountAccess::Aws(access) = access else {
                panic!("an AWS deployment must grant AWS access");
            };
            assert_eq!(access.role_arns, vec!["test-management-role"]);
            assert_eq!(
                access.allowed_service_types,
                vec![ComputeServiceType::Worker]
            );
            assert_eq!(access.account_ids.len(), 1);
            self.grants
                .lock()
                .expect("grants lock")
                .push(repository.to_string());
            Ok(())
        }

        async fn remove_cross_account_access(
            &self,
            repository: &str,
            access: CrossAccountAccess,
        ) -> BindingResult<()> {
            assert!(matches!(access, CrossAccountAccess::Aws(_)));
            self.removals
                .lock()
                .expect("removals lock")
                .push(repository.to_string());
            Ok(())
        }

        async fn get_cross_account_access(
            &self,
            _: &str,
        ) -> BindingResult<CrossAccountPermissions> {
            panic!("reconciliation must use the registry grant operation")
        }

        async fn generate_credentials(
            &self,
            _: &str,
            _: ArtifactRegistryPermissions,
            _: Option<u32>,
        ) -> BindingResult<ArtifactRegistryCredentials> {
            panic!("AWS reconciliation must not mint pull credentials")
        }

        async fn delete_repository(&self, _: &str) -> BindingResult<()> {
            panic!("cleanup must revoke access, not delete repositories")
        }
    }

    #[tokio::test]
    async fn worker_proxy_scope_preserves_published_images_and_reuses_the_grant() {
        for (project, repository) in [
            (PROJECT, "alien-artifacts-prj_test"),
            ("default", "alien-artifacts"),
        ] {
            let registry = Arc::new(RecordingAwsRegistry::default());
            let provider = recording_provider(registry.clone());
            let mut state = worker_grant_state(&format!("manager.example.com/{repository}:latest"));
            for _ in 0..2 {
                reconcile_registry_access_with_proxy()
                    .bindings_provider(&provider)
                    .target_bindings_providers(&HashMap::new())
                    .deployment_id("deployment")
                    .project_id(project)
                    .state(&mut state)
                    .proxy_url("https://manager.example.com/")
                    .call()
                    .await;
            }
            assert_eq!(
                *registry.grants.lock().expect("grants lock"),
                vec![repository]
            );
            let metadata = state.runtime_metadata.expect("metadata");
            assert!(metadata.registry_access_granted);
            let access = metadata.registry_access.expect("record");
            assert_eq!(access.repositories, vec![repository]);
            assert_eq!(access.service_types, vec!["worker"]);
        }
    }

    #[tokio::test]
    async fn worker_proxy_scope_rejects_a_different_host_and_another_projects_path() {
        for image in [
            "other.example.com/alien-artifacts-prj_test:latest",
            "manager.example.com/alien-artifacts-other:latest",
            "manager.example.com/alien-artifacts:latest",
        ] {
            let registry = Arc::new(RecordingAwsRegistry::default());
            let mut state = worker_grant_state(image);
            reconcile_registry_access_with_proxy()
                .bindings_provider(&recording_provider(registry.clone()))
                .target_bindings_providers(&HashMap::new())
                .deployment_id("deployment")
                .project_id(PROJECT)
                .state(&mut state)
                .proxy_url("https://manager.example.com")
                .call()
                .await;
            assert!(registry.grants.lock().expect("grants lock").is_empty());
            let metadata = state.runtime_metadata.expect("metadata");
            assert!(!metadata.registry_access_granted);
            assert!(metadata.registry_access.is_none());
        }
    }

    fn recording_provider(
        registry: Arc<RecordingAwsRegistry>,
    ) -> Option<Arc<dyn BindingsProviderApi>> {
        Some(Arc::new(TestBindingsProvider {
            binding_name: "artifact-registry",
            registry,
        }))
    }

    fn worker_grant_state(image: &str) -> DeploymentState {
        let mut state = aws_state_with_stack(worker_stack(image));
        state.stack_state = Some(registered_stack_state("test-management-role"));
        state
    }

    async fn assert_worker_grant(repository: &str, other_host: bool, expected: bool) {
        let registry = Arc::new(RecordingAwsRegistry::default());
        let host = if other_host {
            "https://other.example.com".to_string()
        } else {
            registry.registry_endpoint()
        };
        let mut state = worker_grant_state(&format!("{host}/{repository}:latest"));
        reconcile_registry_access(
            &recording_provider(registry.clone()),
            &HashMap::new(),
            "deployment",
            PROJECT,
            &mut state,
        )
        .await;
        let grants = registry.grants.lock().expect("grants lock");
        assert_eq!(
            *grants,
            if expected {
                vec![repository.to_string()]
            } else {
                vec![]
            }
        );
        let metadata = state.runtime_metadata.expect("metadata");
        assert_eq!(metadata.registry_access_granted, expected);
        assert_eq!(
            metadata.registry_access.map(|access| access.repositories),
            expected.then(|| vec![repository.to_string()])
        );
        assert!(registry.removals.lock().expect("removals lock").is_empty());
    }

    #[tokio::test]
    async fn worker_scope_refuses_a_sibling_repository() {
        assert_worker_grant("alien-artifacts-other", false, false).await;
    }

    #[tokio::test]
    async fn worker_scope_grants_its_own_repository() {
        assert_worker_grant("alien-artifacts-prj_test", false, true).await;
    }

    #[tokio::test]
    async fn worker_scope_refuses_another_registry_host() {
        assert_worker_grant("alien-artifacts-prj_test", true, false).await;
    }

    #[tokio::test]
    async fn worker_scope_refuses_the_bare_prefix_for_a_named_project() {
        assert_worker_grant("alien-artifacts", false, false).await;
    }

    #[tokio::test]
    async fn worker_scope_checks_current_target_and_prepared_releases() {
        let registry = Arc::new(RecordingAwsRegistry::default());
        let own = format!(
            "{}/alien-artifacts-prj_test:latest",
            registry.registry_endpoint()
        );
        let other = format!(
            "{}/alien-artifacts-other:latest",
            registry.registry_endpoint()
        );
        for rejected_slot in 0..3 {
            let mut state = worker_grant_state(if rejected_slot == 2 { &other } else { &own });
            let release = |image: &str| ReleaseInfo {
                release_id: None,
                version: None,
                description: None,
                stack: worker_stack(image),
            };
            state.current_release = Some(release(if rejected_slot == 0 { &other } else { &own }));
            state.target_release = Some(release(if rejected_slot == 1 { &other } else { &own }));
            registry.grants.lock().expect("grants lock").clear();
            for _ in 0..2 {
                reconcile_registry_access(
                    &recording_provider(registry.clone()),
                    &HashMap::new(),
                    "deployment",
                    PROJECT,
                    &mut state,
                )
                .await;
            }
            assert_eq!(
                *registry.grants.lock().expect("grants lock"),
                vec!["alien-artifacts-prj_test"]
            );
            let metadata = state.runtime_metadata.expect("metadata");
            assert!(metadata.registry_access_granted);
            assert_eq!(
                metadata.registry_access.expect("record").repositories,
                vec!["alien-artifacts-prj_test"]
            );
        }
    }

    #[tokio::test]
    async fn worker_scope_clears_a_completed_marker_when_the_next_release_is_refused() {
        let registry = Arc::new(RecordingAwsRegistry::default());
        let mut state = worker_grant_state(&format!(
            "{}/alien-artifacts-other:latest",
            registry.registry_endpoint()
        ));
        let metadata = state.runtime_metadata.as_mut().expect("metadata");
        metadata.registry_access_granted = true;
        metadata.registry_access = Some(RegistryAccess {
            repositories: vec!["alien-artifacts-other".to_string()],
            service_types: vec!["worker".to_string()],
        });
        reconcile_registry_access(
            &recording_provider(registry.clone()),
            &HashMap::new(),
            "deployment",
            PROJECT,
            &mut state,
        )
        .await;
        assert!(registry.grants.lock().expect("grants lock").is_empty());
        let metadata = state.runtime_metadata.as_ref().expect("metadata");
        assert!(!metadata.registry_access_granted);
        assert_eq!(
            metadata
                .registry_access
                .as_ref()
                .expect("record")
                .repositories,
            vec!["alien-artifacts-other"]
        );
        state.status = DeploymentStatus::Deleted;
        cleanup_deleted_registry_access(
            &MockDeploymentStore::new(),
            &recording_provider(registry.clone()),
            &HashMap::new(),
            "deployment",
            PROJECT,
            &state,
        )
        .await
        .expect("recorded grant must revoke");
        assert_eq!(
            *registry.removals.lock().expect("removals lock"),
            vec!["alien-artifacts-other"]
        );
    }

    /// WARN lines logged on this thread while `run` runs.
    async fn warnings_during<F: std::future::Future<Output = ()>>(run: F) -> String {
        #[derive(Clone, Default)]
        struct Captured(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Captured {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("log lock").extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        run.await;
        drop(guard);
        let logs = captured.0.lock().expect("log lock").clone();
        String::from_utf8(logs).expect("utf-8 logs")
    }

    /// Cleanup derives repositories without the manager's proxy URL, so an image published
    /// through the proxy looked like another project's. Every deletion of such a deployment
    /// logged a refusal for its own repository. Refusals are logged where access is granted.
    #[tokio::test]
    async fn cleanup_of_a_proxy_published_worker_logs_no_refusal() {
        let registry = Arc::new(RecordingAwsRegistry::default());
        let mut state = worker_grant_state("manager.example.com/alien-artifacts-prj_test:latest");
        let granted = warnings_during(
            reconcile_registry_access_with_proxy()
                .bindings_provider(&recording_provider(registry.clone()))
                .target_bindings_providers(&HashMap::new())
                .deployment_id("deployment")
                .project_id(PROJECT)
                .state(&mut state)
                .proxy_url("https://manager.example.com/")
                .call(),
        )
        .await;
        assert_eq!(
            *registry.grants.lock().expect("grants lock"),
            vec!["alien-artifacts-prj_test"]
        );
        assert!(!granted.contains("Refusing"), "{granted}");

        state.status = DeploymentStatus::Deleted;
        let cleanup = warnings_during(async {
            cleanup_deleted_registry_access(
                &MockDeploymentStore::new(),
                &recording_provider(registry.clone()),
                &HashMap::new(),
                "deployment",
                PROJECT,
                &state,
            )
            .await
            .expect("the grant must revoke");
        })
        .await;
        assert_eq!(
            *registry.removals.lock().expect("removals lock"),
            vec!["alien-artifacts-prj_test"]
        );
        assert!(!cleanup.contains("Refusing"), "{cleanup}");

        // Granting an image outside the project still logs its refusal, naming the deployment.
        let mut refused = worker_grant_state("manager.example.com/alien-artifacts-other:latest");
        let logged = warnings_during(
            reconcile_registry_access_with_proxy()
                .bindings_provider(&recording_provider(registry.clone()))
                .target_bindings_providers(&HashMap::new())
                .deployment_id("refused-deployment")
                .project_id(PROJECT)
                .state(&mut refused)
                .proxy_url("https://manager.example.com/")
                .call(),
        )
        .await;
        assert!(
            logged
                .contains("Refusing registry access for an image outside its project's repository")
                && logged.contains("deployment_id=refused-deployment")
                && logged.contains("repository=alien-artifacts-other"),
            "{logged}"
        );
    }

    #[tokio::test]
    async fn worker_scope_cleanup_revokes_an_older_grant_without_a_record() {
        let registry = Arc::new(RecordingAwsRegistry::default());
        let mut state =
            worker_grant_state("https://manager.example.com/alien-artifacts-other:latest");
        state.status = DeploymentStatus::Deleted;
        cleanup_deleted_registry_access(
            &MockDeploymentStore::new(),
            &recording_provider(registry.clone()),
            &HashMap::new(),
            "deployment",
            PROJECT,
            &state,
        )
        .await
        .expect("older grant must revoke");
        assert_eq!(
            *registry.removals.lock().expect("removals lock"),
            vec!["alien-artifacts-other"]
        );
        assert!(registry.grants.lock().expect("grants lock").is_empty());
    }

    #[derive(Debug)]
    struct TestBindingsProvider {
        binding_name: &'static str,
        registry: Arc<dyn ArtifactRegistry>,
    }

    /// The contract for "no such binding", which is what the delete reads to tell a manager that
    /// never had a registry from one whose registry will not answer.
    fn missing_binding(binding_name: &str) -> alien_bindings::error::Error {
        AlienError::new(BindingErrorData::BindingNotConfigured {
            binding_name: binding_name.to_string(),
            env_var: alien_core::bindings::binding_env_var_name(binding_name),
        })
    }

    /// Marks a provider whose binding exists but cannot be loaded.
    const UNREACHABLE_BINDING: &str = "unreachable";

    /// A binding that exists and will not answer, as opposed to one that was never configured.
    fn unreachable_binding(binding_name: &str) -> alien_bindings::error::Error {
        AlienError::new(BindingErrorData::BindingConfigInvalid {
            binding_name: binding_name.to_string(),
            env_var: alien_core::bindings::binding_env_var_name(binding_name),
            reason: "credentials could not be resolved".to_string(),
        })
    }

    #[async_trait]
    impl BindingsProviderApi for TestBindingsProvider {
        async fn load_artifact_registry(
            &self,
            binding_name: &str,
        ) -> BindingResult<Arc<dyn ArtifactRegistry>> {
            if self.binding_name == UNREACHABLE_BINDING {
                return Err(unreachable_binding(binding_name));
            }
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

    /// The bundle URI a stored AWS sandbox carries in `code.image`.
    ///
    /// An AWS sandbox's `code.image` is an S3 bundle URI, so this — not a registry reference — is
    /// what the scan reads there.
    const STORED_BUNDLE_URI: &str = "s3://acme-sandbox-bundles/sandbox-bundle/abc123/bundle.zip";

    /// A stored AWS sandbox, with the base image its bundle pulls named separately.
    ///
    /// `None` is a sandbox built on a public base: it authenticates to nothing and so earns no
    /// grant.
    fn sandbox_stack(private_base_image: Option<&str>) -> Stack {
        sandbox_stack_with_code(STORED_BUNDLE_URI, private_base_image)
    }

    /// A GCP sandbox pulls `code.image` itself, so its own reference is what the grant reads.
    fn sandbox_stack_with_code(image: &str, private_base_image: Option<&str>) -> Stack {
        Stack::new("test-stack".to_string())
            .add(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: image.to_string(),
                    })
                    .maybe_private_base_image(private_base_image.map(str::to_string))
                    .egress(SandboxEgress::Internet)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
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

    #[tokio::test]
    async fn load_artifact_registry_prefers_target_provider() {
        let primary_registry: Arc<dyn ArtifactRegistry> = Arc::new(TestArtifactRegistry {
            prefix: "artifacts/default".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        });
        let target_registry: Arc<dyn ArtifactRegistry> = Arc::new(TestArtifactRegistry {
            prefix: "alien-e2e".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
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
            Some("123456789012.dkr.ecr.us-east-2.amazonaws.com".to_string())
        );
    }

    const PROJECT: &str = "prj_test";

    #[test]
    fn aws_registry_access_skips_container_only_stack() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let state = aws_state_with_stack(Stack::new("test-stack".to_string()).build());

        assert!(repository_ids_for_access(&registry, &state, PROJECT).is_empty());
    }

    #[test]
    fn aws_registry_access_uses_worker_image_repository() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let state = aws_state_with_stack(worker_stack(
            "123456789012.dkr.ecr.us-east-2.amazonaws.com/alien-artifacts-prj_test:test-worker-abc123",
        ));

        assert_eq!(
            repository_ids_for_access(&registry, &state, PROJECT),
            vec!["alien-artifacts-prj_test".to_string()]
        );
    }

    #[test]
    fn registry_access_skips_empty_repository_prefix() {
        let registry = TestArtifactRegistry {
            prefix: String::new(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let state = gcp_state_with_stack(worker_stack(
            "manager.example.com/prj_test/test-worker:abc123",
        ));

        assert!(repository_ids_for_access(&registry, &state, PROJECT).is_empty());
    }

    #[test]
    fn gar_grants_only_the_own_project_repository() {
        let registry = TestArtifactRegistry {
            prefix: "vendor/artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        for (owner, expected) in [
            (
                PROJECT,
                vec![format!(
                    "vendor/{}",
                    alien_core::gar_repository::project_repository("artifacts", PROJECT).unwrap()
                )],
            ),
            ("prj_other", Vec::new()),
        ] {
            let state = gcp_state_with_stack(worker_stack(&format!(
                "region-docker.pkg.dev/vendor/{}/{owner}/worker:v1",
                alien_core::gar_repository::project_repository("artifacts", owner).unwrap()
            )));
            let (repositories, refused) = repository_access(&registry, &state, PROJECT);
            assert_eq!(repositories, expected);
            assert_eq!(!refused.is_empty(), owner != PROJECT);
        }
    }

    /// The granted repositories, and whether any image was refused.
    fn any_refused(access: (Vec<String>, Vec<RefusedImage>)) -> (Vec<String>, bool) {
        (access.0, !access.1.is_empty())
    }

    #[test]
    fn gcp_worker_and_sandbox_grants_name_only_their_project_repository() {
        let registry = TestArtifactRegistry {
            prefix: "vendor/artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let own = alien_core::gar_repository::project_repository("artifacts", PROJECT).unwrap();
        for host in [
            "region-docker.pkg.dev",
            "manager.example.com",
            "other.example.com",
        ] {
            let mut state = gcp_state_with_stack(worker_stack(&format!(
                "{host}/vendor/{own}/{PROJECT}/worker:v1"
            )));
            state.target_release = Some(ReleaseInfo {
                release_id: Some("rel_sandbox".to_string()),
                version: None,
                description: None,
                stack: sandbox_stack_with_code(
                    &format!("{host}/vendor/{own}/{PROJECT}/sandbox:v1"),
                    None,
                ),
            });
            assert_eq!(
                any_refused(repository_access_with_proxy(
                    &registry,
                    &state,
                    PROJECT,
                    Some("https://manager.example.com")
                )),
                if host == "other.example.com" {
                    (vec![], true)
                } else {
                    (vec![format!("vendor/{own}")], false)
                }
            );
        }
    }

    #[test]
    fn standalone_gcp_releases_use_the_default_project_repository() {
        let registry = TestArtifactRegistry {
            prefix: "vendor/artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let state = gcp_state_with_stack(worker_stack(
            "region-docker.pkg.dev/vendor/artifacts-default/default/worker:v1",
        ));
        assert_eq!(
            any_refused(repository_access(&registry, &state, "default")),
            (vec!["vendor/artifacts-default".to_string()], false)
        );
        assert_eq!(
            any_refused(repository_access(&registry, &state, PROJECT)),
            (vec![], true)
        );
        for path in [
            "artifacts/worker",
            "artifacts/default/worker",
            "artifacts-shared/agents",
            "artifacts-prj-test/prj_other/worker",
            "artifacts-default/prj_test/worker",
        ] {
            let state = gcp_state_with_stack(worker_stack(&format!(
                "region-docker.pkg.dev/vendor/{path}:v1"
            )));
            assert_eq!(
                any_refused(repository_access(&registry, &state, "default")),
                (vec![], true)
            );
        }
    }

    /// What a GCP revoke asked the registry to remove, one entry per call.
    #[derive(Debug, Default)]
    struct RecordingGcpRegistry {
        removals: Mutex<Vec<(Vec<String>, Vec<String>)>>,
    }

    impl alien_bindings::traits::Binding for RecordingGcpRegistry {}

    #[async_trait]
    impl ArtifactRegistry for RecordingGcpRegistry {
        fn registry_endpoint(&self) -> String {
            "us-central1-docker.pkg.dev".to_string()
        }

        fn upstream_repository_prefix(&self) -> String {
            "vendor-project/alien-artifacts".to_string()
        }

        async fn create_repository(&self, _repo_name: &str) -> BindingResult<RepositoryResponse> {
            unimplemented!("not needed for revoke tests")
        }

        async fn get_repository(&self, _repo_id: &str) -> BindingResult<RepositoryResponse> {
            unimplemented!("not needed for revoke tests")
        }

        async fn add_cross_account_access(
            &self,
            _repo_id: &str,
            _access: CrossAccountAccess,
        ) -> BindingResult<()> {
            unimplemented!("not needed for revoke tests")
        }

        async fn remove_cross_account_access(
            &self,
            _repo_id: &str,
            access: CrossAccountAccess,
        ) -> BindingResult<()> {
            let CrossAccountAccess::Gcp(access) = access else {
                panic!("a GCP revoke must name GCP members");
            };
            self.removals
                .lock()
                .expect("removals lock")
                .push((access.project_numbers, access.service_account_emails));
            Ok(())
        }

        async fn get_cross_account_access(
            &self,
            _repo_id: &str,
        ) -> BindingResult<CrossAccountPermissions> {
            unimplemented!("not needed for revoke tests")
        }

        async fn generate_credentials(
            &self,
            _repo_id: &str,
            _permissions: ArtifactRegistryPermissions,
            _ttl_seconds: Option<u32>,
        ) -> BindingResult<ArtifactRegistryCredentials> {
            unimplemented!("not needed for revoke tests")
        }

        async fn delete_repository(&self, _repo_id: &str) -> BindingResult<()> {
            unimplemented!("not needed for revoke tests")
        }
    }

    const PROJECT_NUMBER: &str = "123456789012";

    fn gcp_environment() -> EnvironmentInfo {
        EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_number: PROJECT_NUMBER.to_string(),
            project_id: "customer-project".to_string(),
            region: "us-central1".to_string(),
        })
    }

    /// A store that answers the shared-grant question, and fails the test if the revoke reads
    /// whole deployment records instead.
    fn store_answering(answer: std::result::Result<bool, &'static str>) -> MockDeploymentStore {
        let mut store = MockDeploymentStore::new();
        store
            .expect_has_other_gcp_project_deployment()
            .withf(|caller, project_number, project_id, excluding| {
                caller.is_system()
                    && project_number == PROJECT_NUMBER
                    && project_id.as_deref() == Some("default")
                    && excluding == "dep_deleted"
            })
            .times(1)
            .returning(move |_, _, _, _| {
                answer.map_err(|message| {
                    AlienError::new(alien_error::GenericError {
                        message: message.to_string(),
                    })
                })
            });
        store.expect_list_deployments().times(0);
        store
    }

    /// A store for a GCP teardown, which asks once for its project's repository and once, across
    /// every Alien project, for the binding's shared repository.
    fn store_answering_teardown(answer: bool) -> MockDeploymentStore {
        let mut store = MockDeploymentStore::new();
        for project in [Some("default".to_string()), None] {
            store
                .expect_has_other_gcp_project_deployment()
                .withf(move |caller, project_number, project_id, excluding| {
                    caller.is_system()
                        && project_number == PROJECT_NUMBER
                        && *project_id == project
                        && excluding == "dep_deleted"
                })
                .times(1)
                .returning(move |_, _, _, _| Ok(answer));
        }
        store.expect_list_deployments().times(0);
        store
    }

    async fn revoke_gcp(
        registry: &RecordingGcpRegistry,
        store: &MockDeploymentStore,
    ) -> Result<()> {
        revoke_registry_access(
            registry,
            "vendor-project/alien-artifacts-default",
            &gcp_environment(),
            None,
            store,
            "dep_deleted",
            "default",
        )
        .await
    }

    const GCP_MANAGEMENT_EMAIL: &str = "a-dep-management@customer-project.iam.gserviceaccount.com";

    /// The remote management outputs of a GCP deployment under both protocols: a federated one
    /// stores its federation descriptor as the access configuration, an older one the email.
    fn gcp_management_outputs() -> [(&'static str, RemoteStackManagementOutputs); 2] {
        let descriptor = serde_json::json!({
            "assertionAudience": "urn:deployment-access:v1:0123456789abcdef",
            "poolId": "deployment-0123456789abcdef0123",
            "projectNumber": PROJECT_NUMBER,
            "providerId": "manager",
            "serviceAccountEmail": GCP_MANAGEMENT_EMAIL,
            "trust": {
                "issuer": "https://issuer.example.com",
                "subject": "system:serviceaccount:platform:manager"
            }
        });
        [
            (
                "federated",
                RemoteStackManagementOutputs {
                    management_resource_id: GCP_MANAGEMENT_EMAIL.to_string(),
                    access_configuration: descriptor.to_string(),
                    legacy_remote_bindings_access: None,
                },
            ),
            (
                "before federation",
                RemoteStackManagementOutputs {
                    management_resource_id: GCP_MANAGEMENT_EMAIL.to_string(),
                    access_configuration: GCP_MANAGEMENT_EMAIL.to_string(),
                    legacy_remote_bindings_access: None,
                },
            ),
        ]
    }

    fn gcp_stack_state(outputs: RemoteStackManagementOutputs) -> StackState {
        let remote_management = RemoteStackManagement::new("management".to_string()).build();
        let mut stack_state =
            StackState::with_resource_prefix(Platform::Gcp, "test-prefix".to_string());
        stack_state.resources.insert(
            remote_management.id.clone(),
            StackResourceState::builder()
                .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(remote_management))
                .outputs(ResourceOutputs::new(outputs))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );
        stack_state
    }

    #[test]
    fn gcp_grant_names_the_management_service_account_under_both_protocols() {
        for (case, outputs) in gcp_management_outputs() {
            let stack_state = gcp_stack_state(outputs);
            let access = build_cross_account_access(&gcp_environment(), Some(&stack_state), false)
                .expect("GCP cross-account access should be configured");
            let CrossAccountAccess::Gcp(access) = access else {
                panic!("GCP environment must produce GCP cross-account access");
            };
            assert_eq!(
                access.service_account_emails,
                vec![GCP_MANAGEMENT_EMAIL.to_string()],
                "{case}"
            );
            assert_eq!(
                access.project_numbers,
                vec![PROJECT_NUMBER.to_string()],
                "{case}"
            );
        }
    }

    #[tokio::test]
    async fn gcp_revoke_removes_the_management_service_account_under_both_protocols() {
        for (case, outputs) in gcp_management_outputs() {
            let registry = RecordingGcpRegistry::default();
            // Another deployment still uses the project, so only the deployment's own member goes.
            let store = store_answering(Ok(true));
            let stack_state = gcp_stack_state(outputs);

            revoke_registry_access(
                &registry,
                "vendor-project/alien-artifacts-default",
                &gcp_environment(),
                Some(&stack_state),
                &store,
                "dep_deleted",
                "default",
            )
            .await
            .expect("revoke should succeed");

            assert_eq!(
                *registry.removals.lock().expect("removals lock"),
                vec![(Vec::<String>::new(), vec![GCP_MANAGEMENT_EMAIL.to_string()])],
                "{case}"
            );
        }
    }

    #[tokio::test]
    async fn the_last_deployment_in_a_gcp_project_revokes_its_shared_grant() {
        let registry = RecordingGcpRegistry::default();
        let store = store_answering(Ok(false));

        revoke_gcp(&registry, &store)
            .await
            .expect("revoke should succeed");

        assert_eq!(
            *registry.removals.lock().expect("removals lock"),
            vec![(vec![PROJECT_NUMBER.to_string()], Vec::<String>::new())],
            "the project's service-agent members must be removed"
        );
    }

    #[tokio::test]
    async fn a_shared_grant_stays_while_another_deployment_uses_the_project() {
        let registry = RecordingGcpRegistry::default();
        let store = store_answering(Ok(true));

        revoke_gcp(&registry, &store)
            .await
            .expect("keeping the grant is a successful revoke");

        assert!(
            registry.removals.lock().expect("removals lock").is_empty(),
            "another deployment still pulls through the project grant"
        );
    }

    #[tokio::test]
    async fn an_unanswered_consumer_check_keeps_the_grant_and_fails_the_revoke() {
        let registry = RecordingGcpRegistry::default();
        let store = store_answering(Err("store unavailable"));

        let error = revoke_gcp(&registry, &store)
            .await
            .expect_err("an unknown consumer set must not revoke");

        assert_eq!(error.code, "REGISTRY_ACCESS_CLEANUP_FAILED");
        assert!(registry.removals.lock().expect("removals lock").is_empty());
    }

    #[tokio::test]
    async fn gcp_teardown_revokes_access_before_imported_setup_is_forgotten() {
        let registry = Arc::new(RecordingGcpRegistry::default());
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
            binding_name: "artifact-registry",
            registry: registry.clone(),
        });
        let providers = Some(provider);
        let targets = HashMap::new();
        let store = store_answering_teardown(true);
        let email = "management@test-project.iam.gserviceaccount.com";
        let mut state = gcp_state_with_stack(worker_stack(
            "us-central1-docker.pkg.dev/vendor-project/alien-artifacts-default/default:latest",
        ));
        state.status = DeploymentStatus::TeardownRequired;
        state.stack_state = Some(registered_stack_state(email));
        let state = revoke_before_persisting(
            &store,
            &providers,
            &targets,
            "dep_deleted",
            "default",
            state,
        )
        .await
        .unwrap();
        assert_eq!(state.status, DeploymentStatus::TeardownRequired);
        // The deployment's own account leaves both its project repository and the shared one.
        assert_eq!(
            *registry.removals.lock().unwrap(),
            vec![(Vec::<String>::new(), vec![email.to_string()]); 2]
        );
        registry.removals.lock().unwrap().clear();
        // Imported setup may be forgotten after this handoff, without reaching Deleted.
        // A sibling has also reached the persisted runtime teardown handoff.
        let store = store_answering_teardown(false);
        revoke_after_persisting(
            &store,
            &providers,
            &targets,
            "dep_deleted",
            "default",
            &state,
        )
        .await
        .unwrap();
        assert_eq!(
            *registry.removals.lock().unwrap(),
            vec![
                (Vec::<String>::new(), vec![email.to_string()]),
                (vec![PROJECT_NUMBER.to_string()], Vec::<String>::new()),
                (Vec::<String>::new(), vec![email.to_string()]),
                (vec![PROJECT_NUMBER.to_string()], Vec::<String>::new()),
            ]
        );
    }

    #[tokio::test]
    async fn teardown_required_revokes_registry_access_before_setup_deletion() {
        let registry: Arc<dyn ArtifactRegistry> = Arc::new(TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: true,
            removed_repository_is_gone: false,
        });
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
            binding_name: "artifact-registry",
            registry,
        });
        let mut state = aws_state_with_stack(worker_stack(
            "123456789012.dkr.ecr.us-east-2.amazonaws.com/alien-artifacts-prj_test:latest",
        ));
        state.status = DeploymentStatus::TeardownRequired;
        let error = cleanup_deleted_registry_access(
            &MockDeploymentStore::new(),
            &Some(provider),
            &HashMap::new(),
            "dep_test",
            "prj_test",
            &state,
        )
        .await
        .expect_err("setup handoff must attempt the revoke and propagate its failure");
        assert_eq!(error.code, "REGISTRY_ACCESS_CLEANUP_FAILED");
    }

    #[tokio::test]
    async fn registry_cleanup_propagates_permission_removal_failure() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: true,
            removed_repository_is_gone: false,
        };
        let environment_info = EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
        });
        let access = build_cross_account_access(&environment_info, None, false)
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
    /// `code.image` holds the S3 bundle, so the sandbox's repository comes from `privateBaseImage`.
    #[test]
    fn aws_registry_access_uses_sandbox_base_image_repository_not_the_bundle() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        // A stored reference: resolved onto the release registry and still
        // region-templated, because one bundle key serves every regional store and the host
        // carries the token the emitters resolve later.
        let state = aws_state_with_stack(sandbox_stack(Some(
            "123456789012.dkr.ecr.{region}.amazonaws.com/alien-artifacts-prj_test:agents-abc123",
        )));

        assert_eq!(
            repository_ids_for_access(&registry, &state, PROJECT),
            vec!["alien-artifacts-prj_test".to_string()],
            "the grant names the repository hosting the base image, and the {{region}} token in \
             its host does not stop the repository being derived"
        );
    }

    #[test]
    fn aws_registry_access_skips_sandbox_images_outside_the_repository_prefix() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };

        for image in [
            "ubuntu:24.04",
            "ghcr.io/acme/sandbox:latest",
            // Under the shared prefix but a sibling project's: granting it would open that
            // project's images to this deployment's account.
            "123456789012.dkr.ecr.{region}.amazonaws.com/alien-artifacts-prj_other:agents-abc123",
            "123456789012.dkr.ecr.{region}.amazonaws.com/alien-artifacts:agents-abc123",
            // This project's repository path, in another account: granting here would open a
            // repository the build never pulls from.
            "210987654321.dkr.ecr.{region}.amazonaws.com/alien-artifacts-prj_test:agents-abc123",
            // The configured account, another region: the grant would open a repository in
            // us-east-2 while the build pulls from eu-west-1.
            "123456789012.dkr.ecr.eu-west-1.amazonaws.com/alien-artifacts-prj_test:agents-abc123",
        ] {
            let state = aws_state_with_stack(sandbox_stack(Some(image)));
            assert!(
                repository_ids_for_access(&registry, &state, PROJECT).is_empty(),
                "'{image}' is not this project's repository and must not produce a grant"
            );
        }
    }

    #[test]
    fn aws_sandbox_only_stack_grants_the_customer_account_and_rsm_role() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let role_arn = "arn:aws:iam::123456789012:role/alien-rsm-role";
        let mut state = aws_state_with_stack(sandbox_stack(Some(
            "123456789012.dkr.ecr.{region}.amazonaws.com/alien-artifacts-prj_test:agents-abc123",
        )));
        state.stack_state = Some(registered_stack_state(role_arn));

        assert_eq!(
            repository_ids_for_access(&registry, &state, PROJECT),
            vec!["alien-artifacts-prj_test".to_string()]
        );

        let access = build_cross_account_access(
            state.environment_info.as_ref().expect("AWS environment"),
            state.stack_state.as_ref(),
            declares_sandbox(&state),
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

        let access = build_cross_account_access(&environment_info, Some(&stack_state), false)
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
    fn cleanup_guard_covers_a_sandbox_only_stack() {
        let state = aws_state_with_stack(sandbox_stack(Some(
            "123456789012.dkr.ecr.us-east-2.amazonaws.com/alien-artifacts-prj_test:agents-abc123",
        )));

        assert!(
            has_registry_backed_image(&state, &Platform::Aws),
            "a partial AWS sandbox grant must still be cleaned up on delete"
        );
        assert!(
            has_registry_backed_image(&state, &Platform::Gcp),
            "a GCP sandbox pulls its container from Alien's registry, so its grant is cleaned up too"
        );
        assert!(
            !has_registry_backed_image(&state, &Platform::Azure),
            "an Azure sandbox reads nothing of ours, so cleanup stays a no-op there"
        );

        // A public base image authenticates to nothing, so no grant was ever made and there is
        // nothing for cleanup to revoke. The bundle URI in `code.image` is not a registry
        // reference and must not be mistaken for one.
        let public_state = aws_state_with_stack(sandbox_stack(None));
        assert!(
            !has_registry_backed_image(&public_state, &Platform::Aws),
            "a sandbox built on a public base earns no grant, so cleanup has nothing to revoke"
        );
    }

    /// The guard is prefix-blind, so it admits a GCP sandbox whose image Alien never hosted. That
    /// deployment was never granted, and on a manager that hosts no images there is no binding to
    /// load, so cleanup has to answer that there is nothing to strand rather than erroring.
    #[test]
    fn a_never_granted_gcp_sandbox_on_a_public_image_still_reaches_the_binding_lookup() {
        let state = gcp_state_with_stack(sandbox_stack_with_code("ubuntu:24.04", None));

        assert!(
            state
                .runtime_metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.registry_access_granted),
            "the fixture must be a deployment that was never granted"
        );
        assert!(
            has_registry_backed_image(&state, &Platform::Gcp),
            "the prefix-blind guard admits it, so cleanup proceeds past the early return"
        );
        assert!(
            repository_ids_for_access(
                &TestArtifactRegistry {
                    prefix: "alien-artifacts-prj_test".to_string(),
                    fail_remove: false,
                    removed_repository_is_gone: false,
                },
                &state,
                PROJECT
            )
            .is_empty(),
            "nothing under the prefix, which is why no grant was ever recorded"
        );
    }

    #[test]
    fn aws_registry_access_covers_a_worker_and_a_sandbox_together() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let stack = Stack::new("test-stack".to_string())
            .add(
                Worker::new("test-worker".to_string())
                    .code(WorkerCode::Image {
                        image: "123456789012.dkr.ecr.us-east-2.amazonaws.com/alien-artifacts-prj_test:test-worker-abc123"
                            .to_string(),
                    })
                    .permissions("execution".to_string())
                    .build(),
                ResourceLifecycle::Live,
            )
            .add(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: STORED_BUNDLE_URI.to_string(),
                    })
                    .private_base_image(
                        "123456789012.dkr.ecr.{region}.amazonaws.com/\
                         alien-artifacts-prj_test:agents-abc123"
                            .to_string(),
                    )
                    .egress(SandboxEgress::Internet)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();

        assert_eq!(
            repository_ids_for_access(&registry, &aws_state_with_stack(stack), PROJECT),
            vec!["alien-artifacts-prj_test".to_string()],
            "a worker and a sandbox in one project share its repository and one grant"
        );
    }

    #[test]
    fn a_refused_sandbox_base_image_leaves_the_grant_incomplete() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let stack = Stack::new("test-stack".to_string())
            .add(
                Worker::new("test-worker".to_string())
                    .code(WorkerCode::Image {
                        image: "123456789012.dkr.ecr.us-east-2.amazonaws.com/alien-artifacts-prj_test:test-worker-abc123"
                            .to_string(),
                    })
                    .permissions("execution".to_string())
                    .build(),
                ResourceLifecycle::Live,
            )
            .add(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: STORED_BUNDLE_URI.to_string(),
                    })
                    .private_base_image(
                        "123456789012.dkr.ecr.{region}.amazonaws.com/\
                         alien-artifacts-prj_other:agents-abc123"
                            .to_string(),
                    )
                    .egress(SandboxEgress::Internet)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();

        let (repo_ids, refused) =
            repository_access(&registry, &aws_state_with_stack(stack), PROJECT);
        assert_eq!(repo_ids, vec!["alien-artifacts-prj_test".to_string()]);
        assert!(
            !refused.is_empty(),
            "the worker's grant must not mark the deployment fully granted"
        );
    }

    /// An ECR registry renamed from `acme-artifacts` to `artifacts`, as its binding describes it.
    #[derive(Debug)]
    struct RenamedRegistry;

    impl alien_bindings::traits::Binding for RenamedRegistry {}

    #[async_trait]
    impl ArtifactRegistry for RenamedRegistry {
        fn registry_endpoint(&self) -> String {
            "https://123456789012.dkr.ecr.us-east-2.amazonaws.com".to_string()
        }

        fn upstream_repository_prefix(&self) -> String {
            "artifacts".to_string()
        }

        fn sandbox_repository_prefix(&self) -> Option<String> {
            Some("sandbox-cache/artifacts".to_string())
        }

        fn legacy_repository_prefixes(&self) -> Vec<String> {
            vec!["acme-artifacts".to_string()]
        }

        fn legacy_sandbox_repository_prefixes(&self) -> Vec<String> {
            vec!["sandbox-acme/acme-artifacts".to_string()]
        }

        async fn create_repository(&self, _: &str) -> BindingResult<RepositoryResponse> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn get_repository(&self, _: &str) -> BindingResult<RepositoryResponse> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn add_cross_account_access(
            &self,
            _: &str,
            _: CrossAccountAccess,
        ) -> BindingResult<()> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn remove_cross_account_access(
            &self,
            _: &str,
            _: CrossAccountAccess,
        ) -> BindingResult<()> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn get_cross_account_access(
            &self,
            _: &str,
        ) -> BindingResult<CrossAccountPermissions> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn generate_credentials(
            &self,
            _: &str,
            _: ArtifactRegistryPermissions,
            _: Option<u32>,
        ) -> BindingResult<ArtifactRegistryCredentials> {
            unimplemented!("not needed for registry access repo-id tests")
        }

        async fn delete_repository(&self, _: &str) -> BindingResult<()> {
            unimplemented!("not needed for registry access repo-id tests")
        }
    }

    const RENAMED_HOST: &str = "123456789012.dkr.ecr.{region}.amazonaws.com";

    /// A deployment still on a release from before the rename keeps the grant on the repository
    /// that release names, beside the one its next release is pushed to.
    #[test]
    fn a_release_from_before_a_registry_rename_keeps_its_grant() {
        let mut state = aws_state_with_stack(worker_stack(&format!(
            "{RENAMED_HOST}/artifacts-{PROJECT}:v2"
        )));
        state.current_release = Some(ReleaseInfo {
            release_id: Some("rel_old".to_string()),
            version: None,
            description: None,
            stack: worker_stack(&format!("{RENAMED_HOST}/acme-artifacts-{PROJECT}:v1")),
        });

        let (repositories, refused) = repository_access(&RenamedRegistry, &state, PROJECT);
        assert_eq!(
            repositories,
            vec![
                format!("acme-artifacts-{PROJECT}"),
                format!("artifacts-{PROJECT}")
            ]
        );
        assert!(refused.is_empty(), "{refused:?}");
    }

    #[test]
    fn a_sandbox_base_from_before_a_registry_rename_keeps_its_grant() {
        for (base, repository) in [
            (
                format!("{RENAMED_HOST}/sandbox-acme/acme-artifacts-{PROJECT}:v1"),
                format!("sandbox-acme/acme-artifacts-{PROJECT}"),
            ),
            (
                format!("{RENAMED_HOST}/acme-artifacts-{PROJECT}:v1"),
                format!("acme-artifacts-{PROJECT}"),
            ),
            (
                format!("{RENAMED_HOST}/sandbox-cache/artifacts-{PROJECT}:v2"),
                format!("sandbox-cache/artifacts-{PROJECT}"),
            ),
        ] {
            let state = aws_state_with_stack(sandbox_stack(Some(&base)));
            assert_eq!(
                repository_ids_for_access(&RenamedRegistry, &state, PROJECT),
                vec![repository]
            );
        }
    }

    #[test]
    fn a_legacy_repository_of_another_project_is_refused() {
        let state = aws_state_with_stack(worker_stack(&format!(
            "{RENAMED_HOST}/acme-artifacts-prj_other:v1"
        )));
        let (repositories, refused) = repository_access(&RenamedRegistry, &state, PROJECT);
        assert!(repositories.is_empty());
        assert_eq!(
            refused,
            vec![RefusedImage {
                resource_id: "test-worker".to_string(),
                repository: "acme-artifacts-prj_other".to_string(),
            }]
        );
    }

    #[test]
    fn cleanup_revokes_a_sandbox_grant_under_either_name() {
        assert_eq!(
            project_repositories(&RenamedRegistry, PROJECT),
            vec![
                format!("artifacts-{PROJECT}"),
                format!("acme-artifacts-{PROJECT}")
            ]
        );
    }

    /// The fallback a grant recorded before the record existed still depends on: the stack no
    /// longer names the repository, and the one a sandbox is ever granted is derived instead.
    #[test]
    fn a_dropped_base_image_still_names_the_repository_cleanup_must_revoke() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let dropped = aws_state_with_stack(sandbox_stack(None));

        assert!(
            repository_ids_for_access(&registry, &dropped, PROJECT).is_empty(),
            "nothing in the stack names a repository once the field is gone"
        );

        assert_eq!(
            project_repositories(&registry, PROJECT),
            vec!["alien-artifacts-prj_test".to_string()]
        );
    }

    /// A repository joins the record when its write is attempted, so the record alone cannot say
    /// the grant is finished: a failed one, or one still missing the management identity, would
    /// otherwise be sealed on the first tick and never retried.
    #[test]
    fn an_unfinished_grant_is_granted_again() {
        let needed = RegistryAccess {
            repositories: vec!["alien-artifacts-prj_test".to_string()],
            service_types: vec!["worker".to_string()],
        };

        assert!(needs_regrant(None, &needed, false));
        assert!(
            needs_regrant(Some(&needed), &needed, false),
            "the same repositories, recorded but not finished, are granted again"
        );
        assert!(
            !needs_regrant(Some(&needed), &needed, true),
            "a finished grant of the same shape is not rewritten every tick"
        );
        let gained = RegistryAccess {
            service_types: vec!["sandbox".to_string(), "worker".to_string()],
            ..needed.clone()
        };
        assert!(
            needs_regrant(Some(&needed), &gained, true),
            "a deployment that pulls as another principal needs the policy rewritten"
        );
    }

    /// Exercises the 404 branch in `remove_registry_access`: a repository gone before the
    /// deployment must not fail the delete.
    #[tokio::test]
    async fn revoking_a_repository_that_is_already_gone_completes() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: true,
        };
        let environment_info = EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: "123456789012".to_string(),
            region: "us-east-2".to_string(),
        });
        let access = build_cross_account_access(&environment_info, None, false)
            .expect("AWS registry access should be configured");

        remove_registry_access(
            &registry,
            "alien-artifacts-prj_test",
            &environment_info,
            access,
            "registry cross-account access",
            "dep_test",
        )
        .await
        .expect("a repository that is gone leaves nothing to revoke");
    }

    /// A repository the deployment stopped needing keeps its grant until something revokes it, and
    /// after the reconcile that drops it nothing else would: the delete reads the record.
    #[test]
    fn a_repository_the_record_names_and_the_deployment_dropped_is_revoked() {
        let recorded = RegistryAccess {
            repositories: vec![
                "alien-artifacts-prj_test".to_string(),
                "alien-artifacts-prj_test/api".to_string(),
            ],
            service_types: vec!["worker".to_string()],
        };

        assert_eq!(
            previously_opened(Some(&recorded), &["alien-artifacts-prj_test".to_string()]),
            vec!["alien-artifacts-prj_test/api".to_string()]
        );
        assert!(previously_opened(Some(&recorded), &recorded.repositories).is_empty());
        assert!(previously_opened(None, &["alien-artifacts-prj_test".to_string()]).is_empty());
    }

    /// The record is what a later reconcile compares against, so a grant made before a resource
    /// existed is completed rather than skipped: a GCP deployment granted for workers alone asks
    /// for the sandbox agent as soon as it declares one.
    #[test]
    fn a_grant_recorded_without_the_sandbox_agent_is_not_taken_as_complete() {
        let granted_for_workers = RegistryAccess {
            repositories: vec!["test-project/alien-artifacts".to_string()],
            service_types: vec!["worker".to_string()],
        };
        let gcp = EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_number: "123456789012".to_string(),
            project_id: "test-project".to_string(),
            region: "us-central1".to_string(),
        });
        let needed = RegistryAccess {
            repositories: granted_for_workers.repositories.clone(),
            service_types: service_type_names(&gcp, true),
        };

        assert_eq!(needed.service_types, vec!["sandbox", "worker"]);
        assert_ne!(
            granted_for_workers, needed,
            "a policy naming one principal is not what a deployment pulling as two needs"
        );
        assert_eq!(
            needed,
            RegistryAccess {
                repositories: needed.repositories.clone(),
                service_types: service_type_names(&gcp, true),
            },
            "and an unchanged one compares equal, so the policy is not rewritten every tick"
        );
    }

    /// The grant is written on one registry, so the reference has to name that one: same account,
    /// and the same region unless it still carries the token a stored reference keeps.
    #[test]
    fn a_reference_is_served_by_the_registry_only_in_its_own_account_and_region() {
        let registry = "https://123456789012.dkr.ecr.us-east-2.amazonaws.com";
        for served in [
            "123456789012.dkr.ecr.us-east-2.amazonaws.com/alien-artifacts-prj_test:v1",
            "123456789012.dkr.ecr.{region}.amazonaws.com/alien-artifacts-prj_test:v1",
        ] {
            assert!(served_by_registry(served, registry), "{served}");
        }
        for elsewhere in [
            "210987654321.dkr.ecr.us-east-2.amazonaws.com/alien-artifacts-prj_test:v1",
            "123456789012.dkr.ecr.eu-west-1.amazonaws.com/alien-artifacts-prj_test:v1",
            "ghcr.io/acme/base:v1",
        ] {
            assert!(!served_by_registry(elsewhere, registry), "{elsewhere}");
        }

        // Mirrors `ecr_registry_identity`: `.com.cn` must be tried before the shorter suffix.
        let china = "https://123456789012.dkr.ecr.cn-north-1.amazonaws.com.cn";
        assert!(served_by_registry(
            "123456789012.dkr.ecr.cn-north-1.amazonaws.com.cn/alien-artifacts-prj_test:v1",
            china
        ));
        assert!(served_by_registry(
            "123456789012.dkr.ecr.{region}.amazonaws.com.cn/alien-artifacts-prj_test:v1",
            china
        ));
        assert!(!served_by_registry(
            "123456789012.dkr.ecr.cn-northwest-1.amazonaws.com.cn/alien-artifacts-prj_test:v1",
            china
        ));
    }

    #[test]
    fn gcp_registry_access_covers_sandbox_images() {
        let registry = TestArtifactRegistry {
            prefix: "test-project/alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let state = gcp_state_with_stack(sandbox_stack_with_code(
            "region-docker.pkg.dev/test-project/alien-artifacts-prj-test/prj_test/sandbox:abc123",
            None,
        ));

        assert_eq!(
            repository_ids_for_access(&registry, &state, PROJECT),
            vec!["test-project/alien-artifacts-prj-test".to_string()],
            "a GCP sandbox pulls its container from Alien's registry, so it earns the grant"
        );
    }

    /// A manager that never had a registry binding could not have granted anything, so its delete
    /// completes; one whose binding exists and will not load may be hiding a grant, so it fails.
    /// `has_registry_backed_image` cannot tell the two apart — it has no prefix to match against.
    #[tokio::test]
    async fn a_binding_that_was_never_configured_does_not_block_a_delete() {
        let registry: Arc<dyn ArtifactRegistry> = Arc::new(TestArtifactRegistry {
            prefix: "test-project/alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        });
        let absent: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
            binding_name: "something-else",
            registry: registry.clone(),
        });
        let unreachable: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
            binding_name: UNREACHABLE_BINDING,
            registry,
        });

        assert!(matches!(
            find_artifact_registry(&Some(absent), &HashMap::new(), &Platform::Gcp).await,
            RegistryLookup::NotConfigured
        ));
        assert!(matches!(
            find_artifact_registry(&Some(unreachable), &HashMap::new(), &Platform::Gcp).await,
            RegistryLookup::Unavailable
        ));
    }

    /// Pins `gcp_service_types`: the sandbox agent is named only where a release declares one.
    #[test]
    fn the_gcp_grant_names_the_sandbox_agent_only_where_a_sandbox_is_declared() {
        let gcp_access = |declares_sandbox| {
            let access = build_cross_account_access(
                &EnvironmentInfo::Gcp(GcpEnvironmentInfo {
                    project_number: "123456789012".to_string(),
                    project_id: "test-project".to_string(),
                    region: "us-central1".to_string(),
                }),
                None,
                declares_sandbox,
            )
            .expect("a GCP environment builds a cross-account access");
            let CrossAccountAccess::Gcp(gcp) = access else {
                panic!("a GCP environment must build GCP cross-account access");
            };
            gcp.allowed_service_types
        };

        assert_eq!(gcp_access(false), vec![ComputeServiceType::Worker]);
        assert_eq!(
            gcp_access(true),
            vec![ComputeServiceType::Worker, ComputeServiceType::Sandbox]
        );
    }

    /// Pins `service_type_names`: the record's sandbox principal follows the same declaration flag.
    #[test]
    fn a_declared_sandbox_is_what_names_the_agent_in_the_record() {
        let gcp = EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_number: "123456789012".to_string(),
            project_id: "test-project".to_string(),
            region: "us-central1".to_string(),
        });

        assert_eq!(service_type_names(&gcp, false), vec!["worker"]);
        assert_eq!(service_type_names(&gcp, true), vec!["sandbox", "worker"]);
    }

    /// Pins `needs_regrant`'s containment check: a record that is a superset of what's needed does
    /// not trigger a regrant, but a missing repository or a changed principal does.
    #[test]
    fn a_record_carrying_a_dropped_repository_is_not_granted_again() {
        let needed = RegistryAccess {
            repositories: vec!["alien-artifacts-prj_test".to_string()],
            service_types: vec!["worker".to_string()],
        };
        let with_dropped = RegistryAccess {
            repositories: vec![
                "alien-artifacts-prj_gone".to_string(),
                "alien-artifacts-prj_test".to_string(),
            ],
            service_types: needed.service_types.clone(),
        };

        assert!(!needs_regrant(Some(&with_dropped), &needed, true));
        assert!(
            needs_regrant(Some(&with_dropped), &needed, false),
            "an unfinished grant is still retried"
        );

        let missing_one = RegistryAccess {
            repositories: vec!["alien-artifacts-prj_gone".to_string()],
            service_types: needed.service_types.clone(),
        };
        assert!(
            needs_regrant(Some(&missing_one), &needed, true),
            "a repository the record does not name has no grant to reuse"
        );

        let other_principals = RegistryAccess {
            repositories: with_dropped.repositories.clone(),
            service_types: vec!["sandbox".to_string(), "worker".to_string()],
        };
        assert!(
            needs_regrant(Some(&other_principals), &needed, true),
            "a change of principal rewrites every repository"
        );
    }

    /// The revoke a delete owes, run through the transport that persists each deployment step.
    #[cfg(feature = "sqlite")]
    mod revoke_before_handoff {
        use super::*;
        use alien_core::{DeploymentConfig, EnvironmentVariablesSnapshot, StackSettings};
        use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};

        use crate::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore};
        use crate::traits::deployment_store::{
            CreateDeploymentGroupParams, CreateDeploymentParams,
        };
        use crate::transports::ManagerTransport;

        const IMAGE: &str =
            "123456789012.dkr.ecr.us-east-2.amazonaws.com/alien-artifacts-prj_test:latest";
        const GCP_PREFIX: &str = "vendor-project/alien-artifacts";
        const GCP_IMAGE: &str =
            "us-central1-docker.pkg.dev/vendor-project/alien-artifacts-prj-test/prj_test/test-worker:abc123";

        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Answer {
            Revoke,
            Deny,
            Fail,
        }

        /// A registry that records the deployment's persisted status at each revoke, which is what
        /// setup teardown would have read at that moment.
        struct RecordingRegistry {
            platform: Platform,
            answer: Mutex<Answer>,
            store: Arc<SqliteDeploymentStore>,
            deployment_id: Mutex<String>,
            statuses_at_revoke: Mutex<Vec<String>>,
        }

        impl std::fmt::Debug for RecordingRegistry {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("RecordingRegistry").finish_non_exhaustive()
            }
        }

        impl alien_bindings::traits::Binding for RecordingRegistry {}

        #[async_trait]
        impl ArtifactRegistry for RecordingRegistry {
            fn registry_endpoint(&self) -> String {
                match self.platform {
                    Platform::Gcp => "us-central1-docker.pkg.dev".to_string(),
                    _ => "https://123456789012.dkr.ecr.us-east-2.amazonaws.com".to_string(),
                }
            }

            fn upstream_repository_prefix(&self) -> String {
                match self.platform {
                    Platform::Gcp => GCP_PREFIX.to_string(),
                    _ => "alien-artifacts".to_string(),
                }
            }

            async fn create_repository(
                &self,
                _repo_name: &str,
            ) -> BindingResult<RepositoryResponse> {
                unimplemented!("not needed for revoke tests")
            }

            async fn get_repository(&self, _repo_id: &str) -> BindingResult<RepositoryResponse> {
                unimplemented!("not needed for revoke tests")
            }

            async fn add_cross_account_access(
                &self,
                _repo_id: &str,
                _access: CrossAccountAccess,
            ) -> BindingResult<()> {
                unimplemented!("not needed for revoke tests")
            }

            async fn remove_cross_account_access(
                &self,
                repo_id: &str,
                _access: CrossAccountAccess,
            ) -> BindingResult<()> {
                let deployment_id = self.deployment_id.lock().expect("id lock").clone();
                let status = self
                    .store
                    .get_deployment(&Subject::system(), &deployment_id)
                    .await
                    .expect("deployment read")
                    .expect("deployment exists")
                    .status;
                self.statuses_at_revoke
                    .lock()
                    .expect("statuses lock")
                    .push(status);
                match *self.answer.lock().expect("answer lock") {
                    Answer::Revoke => Ok(()),
                    Answer::Deny => Err(AlienError::new(BindingErrorData::RemoteAccessDenied {
                        operation_context: format!(
                            "Failed to delete ECR repository policy for '{repo_id}'"
                        ),
                        resource_type: "ECR Repository".to_string(),
                        resource_name: repo_id.to_string(),
                    })),
                    Answer::Fail => Err(AlienError::new(BindingErrorData::Other {
                        message: "simulated registry outage".to_string(),
                    })),
                }
            }

            async fn get_cross_account_access(
                &self,
                _repo_id: &str,
            ) -> BindingResult<CrossAccountPermissions> {
                unimplemented!("not needed for revoke tests")
            }

            async fn generate_credentials(
                &self,
                _repo_id: &str,
                _permissions: ArtifactRegistryPermissions,
                _ttl_seconds: Option<u32>,
            ) -> BindingResult<ArtifactRegistryCredentials> {
                unimplemented!("not needed for revoke tests")
            }

            async fn delete_repository(&self, _repo_id: &str) -> BindingResult<()> {
                unimplemented!("not needed for revoke tests")
            }
        }

        struct Harness {
            store: Arc<SqliteDeploymentStore>,
            registry: Arc<RecordingRegistry>,
            transport: ManagerTransport,
            deployment_id: String,
        }

        /// An AWS deployment whose runtime cleanup is under way, as the manager loop holds it.
        async fn deleting_deployment(answer: Answer) -> Harness {
            deleting_deployment_on(Platform::Aws, answer).await
        }

        async fn deleting_deployment_on(platform: Platform, answer: Answer) -> Harness {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.keep().join("manager.db");
            let db = Arc::new(
                SqliteDatabase::new(path.to_str().expect("utf-8 path"))
                    .await
                    .expect("database"),
            );
            let store = Arc::new(SqliteDeploymentStore::new(db));
            let system = Subject::system();
            let group = store
                .create_deployment_group(
                    &system,
                    CreateDeploymentGroupParams {
                        name: "group".to_string(),
                        max_deployments: 10,
                        setup: Default::default(),
                    },
                )
                .await
                .expect("group");
            let deployment = store
                .create_deployment(
                    &system,
                    CreateDeploymentParams {
                        deployment_protocol_version:
                            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                        name: "worker".to_string(),
                        deployment_group_id: group.id,
                        platform,
                        base_platform: None,
                        stack_settings: Default::default(),
                        stack_state: None,
                        environment_variables: None,
                        public_subdomain: None,
                        input_values: Default::default(),
                        setup_item: None,
                        deployment_token: None,
                    },
                )
                .await
                .expect("deployment");
            store
                .set_delete_pending(&system, &deployment.id)
                .await
                .expect("delete pending");

            let registry = Arc::new(RecordingRegistry {
                platform,
                answer: Mutex::new(answer),
                store: store.clone(),
                deployment_id: Mutex::new(deployment.id.clone()),
                statuses_at_revoke: Mutex::new(Vec::new()),
            });
            let provider: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
                binding_name: "artifact-registry",
                registry: registry.clone(),
            });
            let transport = ManagerTransport::new(
                store.clone(),
                Some(provider),
                HashMap::new(),
                "prj_test".to_string(),
                "manager-loop".to_string(),
                None,
            );
            let harness = Harness {
                store,
                registry,
                transport,
                deployment_id: deployment.id,
            };
            harness
                .checkpoint(DeploymentStatus::Deleting)
                .await
                .expect("runtime cleanup checkpoint");
            harness
        }

        impl Harness {
            fn state(&self, status: DeploymentStatus) -> DeploymentState {
                let mut state = match self.registry.platform {
                    Platform::Gcp => {
                        let mut state = gcp_state_with_stack(worker_stack(GCP_IMAGE));
                        state.runtime_metadata = Some(RuntimeMetadata {
                            registry_access_granted: true,
                            // A grant on this revision records what it opened; one with the
                            // marker alone predates per-project repositories.
                            registry_access: Some(RegistryAccess {
                                repositories: vec![format!("{GCP_PREFIX}-prj-test")],
                                service_types: vec!["worker".to_string()],
                            }),
                            ..RuntimeMetadata::default()
                        });
                        state
                    }
                    _ => aws_state_with_stack(worker_stack(IMAGE)),
                };
                state.status = status;
                state
            }

            async fn checkpoint(
                &self,
                status: DeploymentStatus,
            ) -> std::result::Result<StepReconcileResult, AlienError> {
                self.transport
                    .reconcile_step(
                        &self.deployment_id,
                        &self.state(status),
                        &DeploymentConfig::builder()
                            .stack_settings(StackSettings::default())
                            .environment_variables(EnvironmentVariablesSnapshot {
                                variables: Vec::new(),
                                hash: String::new(),
                                created_at: String::new(),
                            })
                            .external_bindings(Default::default())
                            .allow_frozen_changes(false)
                            .build(),
                        false,
                        None,
                        Vec::new(),
                        Vec::new(),
                    )
                    .await
            }

            async fn persisted_status(&self) -> String {
                self.store
                    .get_deployment(&Subject::system(), &self.deployment_id)
                    .await
                    .expect("deployment read")
                    .expect("deployment exists")
                    .status
            }

            fn statuses_at_revoke(&self) -> Vec<String> {
                self.registry
                    .statuses_at_revoke
                    .lock()
                    .expect("statuses lock")
                    .clone()
            }
        }

        #[tokio::test]
        async fn the_revoke_runs_before_setup_teardown_can_take_the_deployment() {
            let harness = deleting_deployment(Answer::Revoke).await;

            harness
                .checkpoint(DeploymentStatus::TeardownRequired)
                .await
                .expect("the handoff checkpoint should succeed");

            assert_eq!(
                harness.statuses_at_revoke(),
                vec!["deleting".to_string()],
                "the grant must be revoked while the deployment is still deleting: once \
                 teardown-required is saved, setup teardown deletes the identity and the record"
            );
            assert_eq!(harness.persisted_status().await, "teardown-required");
        }

        #[tokio::test]
        async fn a_denied_revoke_fails_the_delete_and_a_retry_hands_off_once_granted() {
            let harness = deleting_deployment(Answer::Deny).await;

            let result = harness
                .checkpoint(DeploymentStatus::TeardownRequired)
                .await
                .expect("a denied revoke must end the step, not ask the runner to retry it");

            let failed = result
                .state
                .expect("the runner must adopt the failed delete");
            assert_eq!(failed.status, DeploymentStatus::DeleteFailed);
            let error = failed.error.expect("the failed delete carries the denial");
            assert_eq!(error.code, "REGISTRY_ACCESS_REVOKE_DENIED");
            assert!(!error.retryable);
            assert!(
                error.message.contains("alien-artifacts-prj_test"),
                "the error names the repository: {}",
                error.message
            );
            assert_eq!(
                harness.persisted_status().await,
                "delete-failed",
                "setup teardown must not be offered a deployment whose grant is still open"
            );

            // The permission is granted and the delete retried: runtime cleanup reaches the
            // handoff again and this time revokes.
            *harness.registry.answer.lock().expect("answer lock") = Answer::Revoke;
            harness
                .checkpoint(DeploymentStatus::Deleting)
                .await
                .expect("retry");
            harness
                .checkpoint(DeploymentStatus::TeardownRequired)
                .await
                .expect("the retried handoff should succeed");
            assert_eq!(harness.persisted_status().await, "teardown-required");
            assert_eq!(
                harness.statuses_at_revoke(),
                vec!["deleting".to_string(), "deleting".to_string()]
            );
        }

        #[tokio::test]
        async fn a_failed_revoke_persists_nothing_for_the_runner_to_retry() {
            let harness = deleting_deployment(Answer::Fail).await;

            let Err(error) = harness.checkpoint(DeploymentStatus::TeardownRequired).await else {
                panic!("an unfinished revoke must not let the step complete");
            };

            assert!(
                error.retryable,
                "a registry outage is retried by the runner"
            );
            assert_eq!(
                harness.persisted_status().await,
                "deleting",
                "nothing is persisted, so setup teardown cannot take the deployment before the \
                 retried revoke"
            );
        }

        #[tokio::test]
        async fn gcp_revokes_the_shared_grant_only_after_its_deletion_is_saved() {
            let harness = deleting_deployment_on(Platform::Gcp, Answer::Revoke).await;

            harness
                .checkpoint(DeploymentStatus::Deleted)
                .await
                .expect("the deletion checkpoint should succeed");

            let statuses = harness.statuses_at_revoke();
            assert!(!statuses.is_empty());
            assert_eq!(
                statuses,
                vec!["deleted".to_string(); statuses.len()],
                "the shared grant is removed only when no other deployment in the project is \
                 saved as active, so each deletion must be saved before it asks: two deletions \
                 asking first would each see the other active and both keep the grant"
            );
        }
    }

    /// A deployment granted before per-project GAR repositories recorded its grant on the
    /// binding's shared repository. Its teardown must revoke that grant through the real GAR
    /// binding instead of failing every retry with a repository outside the project namespace.
    #[tokio::test]
    async fn gcp_teardown_revokes_a_grant_recorded_on_the_shared_repository() {
        use alien_bindings::providers::artifact_registry::GarArtifactRegistry;
        use alien_core::bindings::{
            ArtifactRegistryBinding, BindingValue, GarArtifactRegistryBinding,
        };
        use alien_core::{GcpClientConfig, GcpCredentials, GcpServiceOverrides};
        use axum::{extract::State, http::Uri, Json, Router};

        const AGENT: &str =
            "serviceAccount:service-123456789012@serverless-robot-prod.iam.gserviceaccount.com";
        type Policies = Arc<Mutex<Vec<(String, serde_json::Value)>>>;
        async fn gar(
            State(written): State<Policies>,
            uri: Uri,
            body: String,
        ) -> Json<serde_json::Value> {
            if let Some(path) = uri.path().strip_suffix(":setIamPolicy") {
                let input: serde_json::Value = serde_json::from_str(&body).unwrap();
                written
                    .lock()
                    .unwrap()
                    .push((path.to_string(), input["policy"].clone()));
                return Json(input["policy"].clone());
            }
            Json(serde_json::json!({"etag": "e1", "bindings": [
                {"role": "roles/artifactregistry.reader", "members": [AGENT, "user:keep@example.com"]}
            ]}))
        }

        for status in [
            DeploymentStatus::TeardownRequired,
            DeploymentStatus::Deleted,
        ] {
            let written: Policies = Arc::default();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new().fallback(gar).with_state(written.clone());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let config = GcpClientConfig {
                project_id: "vendor".to_string(),
                region: "us-central1".to_string(),
                project_number: None,
                credentials: GcpCredentials::AccessToken {
                    token: "local".to_string(),
                },
                service_overrides: Some(GcpServiceOverrides {
                    endpoints: HashMap::from([("artifactregistry".to_string(), endpoint)]),
                }),
            };
            let registry = GarArtifactRegistry::new(
                "artifact-registry".to_string(),
                ArtifactRegistryBinding::Gar(GarArtifactRegistryBinding {
                    repository_name: BindingValue::Value("alien-artifacts".to_string()),
                    pull_service_account_email: None,
                    push_service_account_email: None,
                }),
                &config,
            )
            .await
            .unwrap();
            let provider: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
                binding_name: "artifact-registry",
                registry: Arc::new(registry),
            });
            let providers = Some(provider);
            let mut state = gcp_state_with_stack(worker_stack(
                "us-central1-docker.pkg.dev/vendor/alien-artifacts/prj_test/worker:v1",
            ));
            state.status = status;
            state.runtime_metadata = Some(RuntimeMetadata {
                registry_access: Some(RegistryAccess {
                    repositories: vec!["vendor/alien-artifacts".to_string()],
                    service_types: vec!["worker".to_string()],
                }),
                registry_access_granted: true,
                ..Default::default()
            });
            let mut store = MockDeploymentStore::new();
            store
                .expect_has_other_gcp_project_deployment()
                .returning(|_, _, _, _| Ok(false));
            let targets = HashMap::new();
            let result = if status == DeploymentStatus::TeardownRequired {
                revoke_before_persisting(&store, &providers, &targets, "dep_old", PROJECT, state)
                    .await
                    .map(|state| assert_eq!(state.status, DeploymentStatus::TeardownRequired))
            } else {
                revoke_after_persisting(&store, &providers, &targets, "dep_old", PROJECT, &state)
                    .await
            };
            server.abort();
            assert!(result.is_ok(), "{status:?} teardown failed: {result:?}");
            let written = written.lock().unwrap();
            assert_eq!(written.len(), 1, "{status:?}: {written:?}");
            assert!(written[0].0.ends_with("/repositories/alien-artifacts"));
            assert_eq!(
                written[0].1["bindings"][0]["members"],
                serde_json::json!(["user:keep@example.com"]),
                "{status:?}: the shared grant is removed and unrelated members kept"
            );
        }
    }
    /// A GCP grant made before the grant record existed is named by the marker alone and sits on
    /// the binding's shared repository. A reconcile that refuses the legacy image path must not
    /// erase that marker, and the delete must still revoke the shared grant: otherwise the target
    /// project's service agent keeps reading every project's older images.
    #[tokio::test]
    async fn gcp_marker_only_shared_grant_survives_a_refusing_reconcile_and_is_revoked() {
        use alien_bindings::providers::artifact_registry::GarArtifactRegistry;
        use alien_core::bindings::{
            ArtifactRegistryBinding, BindingValue, GarArtifactRegistryBinding,
        };
        use alien_core::{GcpClientConfig, GcpCredentials, GcpServiceOverrides};
        use axum::{extract::State, http::Uri, Json, Router};

        const AGENT: &str =
            "serviceAccount:service-123456789012@serverless-robot-prod.iam.gserviceaccount.com";
        type Policies = Arc<Mutex<Vec<(String, serde_json::Value)>>>;
        async fn gar(
            State(written): State<Policies>,
            uri: Uri,
            body: String,
        ) -> Json<serde_json::Value> {
            if let Some(path) = uri.path().strip_suffix(":setIamPolicy") {
                let input: serde_json::Value = serde_json::from_str(&body).unwrap();
                written
                    .lock()
                    .unwrap()
                    .push((path.to_string(), input["policy"].clone()));
                return Json(input["policy"].clone());
            }
            Json(serde_json::json!({"etag": "e1", "bindings": [
                {"role": "roles/artifactregistry.reader", "members": [AGENT, "user:keep@example.com"]}
            ]}))
        }

        // Each answer the consumer check can give: no consumer anywhere in the GCP project, or
        // a deployment of another Alien project that still pulls from the shared repository.
        for other_consumer in [false, true] {
            let written: Policies = Arc::default();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new().fallback(gar).with_state(written.clone());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let config = GcpClientConfig {
                project_id: "vendor".to_string(),
                region: "us-central1".to_string(),
                project_number: None,
                credentials: GcpCredentials::AccessToken {
                    token: "local".to_string(),
                },
                service_overrides: Some(GcpServiceOverrides {
                    endpoints: HashMap::from([("artifactregistry".to_string(), endpoint)]),
                }),
            };
            let registry = GarArtifactRegistry::new(
                "artifact-registry".to_string(),
                ArtifactRegistryBinding::Gar(GarArtifactRegistryBinding {
                    repository_name: BindingValue::Value("alien-artifacts".to_string()),
                    pull_service_account_email: None,
                    push_service_account_email: None,
                }),
                &config,
            )
            .await
            .unwrap();
            let provider: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
                binding_name: "artifact-registry",
                registry: Arc::new(registry),
            });
            let providers = Some(provider);
            let targets = HashMap::new();
            let mut state = gcp_state_with_stack(worker_stack(
                "us-central1-docker.pkg.dev/vendor/alien-artifacts/prj_test/worker:v1",
            ));
            state.status = DeploymentStatus::Running;
            state.runtime_metadata = Some(RuntimeMetadata {
                registry_access_granted: true,
                ..Default::default()
            });

            reconcile_registry_access(&providers, &targets, "dep_old", PROJECT, &mut state).await;
            let metadata = state.runtime_metadata.clone().unwrap();
            assert!(
                metadata.registry_access_granted,
                "the marker is the only record of the shared grant"
            );
            assert!(
                written.lock().unwrap().is_empty(),
                "a legacy path is never granted"
            );

            state.status = DeploymentStatus::Deleted;
            let mut store = MockDeploymentStore::new();
            // The shared repository served every Alien project, so the check spans them all.
            store
                .expect_has_other_gcp_project_deployment()
                .withf(|_, project_number, project_id, excluding| {
                    project_number == "123456789012"
                        && project_id.is_none()
                        && excluding == "dep_old"
                })
                .times(1)
                .returning(move |_, _, _, _| Ok(other_consumer));
            let result =
                revoke_after_persisting(&store, &providers, &targets, "dep_old", PROJECT, &state)
                    .await;
            server.abort();
            assert!(result.is_ok(), "{result:?}");
            let written = written.lock().unwrap();
            if other_consumer {
                assert!(
                    written.is_empty(),
                    "another project's deployment still pulls: {written:?}"
                );
            } else {
                assert_eq!(written.len(), 1, "{written:?}");
                assert!(written[0].0.ends_with("/repositories/alien-artifacts"));
                assert_eq!(
                    written[0].1["bindings"][0]["members"],
                    serde_json::json!(["user:keep@example.com"])
                );
            }
        }
    }
    /// A legacy deployment torn down beside a newer one keeps the shared repository's grant for
    /// it. The newer deployment, the last one in the GCP project, must remove that grant at its own
    /// teardown even though nothing it ran came from the shared repository.
    #[tokio::test]
    async fn the_last_gcp_deployment_removes_a_shared_grant_an_older_one_left() {
        use alien_bindings::providers::artifact_registry::GarArtifactRegistry;
        use alien_core::bindings::{
            ArtifactRegistryBinding, BindingValue, GarArtifactRegistryBinding,
        };
        use alien_core::{GcpClientConfig, GcpCredentials, GcpServiceOverrides};
        use axum::{extract::State, http::Uri, Json, Router};

        const AGENT: &str =
            "serviceAccount:service-123456789012@serverless-robot-prod.iam.gserviceaccount.com";
        type Policies = Arc<Mutex<Vec<(String, serde_json::Value)>>>;
        async fn gar(
            State(written): State<Policies>,
            uri: Uri,
            body: String,
        ) -> Json<serde_json::Value> {
            if let Some(path) = uri.path().strip_suffix(":setIamPolicy") {
                let input: serde_json::Value = serde_json::from_str(&body).unwrap();
                written
                    .lock()
                    .unwrap()
                    .push((path.to_string(), input["policy"].clone()));
                return Json(input["policy"].clone());
            }
            Json(serde_json::json!({"etag": "e1", "bindings": [
                {"role": "roles/artifactregistry.reader", "members": [AGENT]}
            ]}))
        }

        let written: Policies = Arc::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(gar).with_state(written.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = GcpClientConfig {
            project_id: "vendor".to_string(),
            region: "us-central1".to_string(),
            project_number: None,
            credentials: GcpCredentials::AccessToken {
                token: "local".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("artifactregistry".to_string(), endpoint)]),
            }),
        };
        let registry = GarArtifactRegistry::new(
            "artifact-registry".to_string(),
            ArtifactRegistryBinding::Gar(GarArtifactRegistryBinding {
                repository_name: BindingValue::Value("alien-artifacts".to_string()),
                pull_service_account_email: None,
                push_service_account_email: None,
            }),
            &config,
        )
        .await
        .unwrap();
        let provider: Arc<dyn BindingsProviderApi> = Arc::new(TestBindingsProvider {
            binding_name: "artifact-registry",
            registry: Arc::new(registry),
        });
        let providers = Some(provider);
        let mut state = gcp_state_with_stack(worker_stack(
            "us-central1-docker.pkg.dev/vendor/alien-artifacts-prj-test/prj_test/worker:v2",
        ));
        state.status = DeploymentStatus::Deleted;
        state.runtime_metadata = Some(RuntimeMetadata {
            registry_access_granted: true,
            registry_access: Some(RegistryAccess {
                repositories: vec!["vendor/alien-artifacts-prj-test".to_string()],
                service_types: vec!["worker".to_string()],
            }),
            ..Default::default()
        });
        let mut store = MockDeploymentStore::new();
        store
            .expect_has_other_gcp_project_deployment()
            .times(2)
            .returning(|_, _, _, _| Ok(false));
        let result = revoke_after_persisting(
            &store,
            &providers,
            &HashMap::new(),
            "dep_new",
            PROJECT,
            &state,
        )
        .await;
        server.abort();
        assert!(result.is_ok(), "{result:?}");
        let mut repositories: Vec<String> = written
            .lock()
            .unwrap()
            .iter()
            .map(|(path, policy)| {
                assert!(
                    policy["bindings"].as_array().is_none_or(Vec::is_empty),
                    "{path}: {policy}"
                );
                path.rsplit('/').next().unwrap().to_string()
            })
            .collect();
        repositories.sort();
        assert_eq!(
            repositories,
            vec!["alien-artifacts", "alien-artifacts-prj-test"],
            "both the project repository and the shared one lose the service agent"
        );
    }

    #[test]
    fn sandbox_upgrade_and_rollback_grant_both_repositories_but_refuse_neighbors() {
        let registry = TestArtifactRegistry {
            prefix: "alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let old_image = "123456789012.dkr.ecr.{region}.amazonaws.com/alien-artifacts-prj_test:old";
        let cache_image =
            "123456789012.dkr.ecr.{region}.amazonaws.com/sandbox-acme/alien-artifacts-prj_test:new";
        let mut state = aws_state_with_stack(sandbox_stack(Some(old_image)));
        state.current_release = Some(ReleaseInfo {
            release_id: Some("old".to_string()),
            version: None,
            description: None,
            stack: sandbox_stack(Some(old_image)),
        });
        state.target_release = Some(ReleaseInfo {
            release_id: Some("new".to_string()),
            version: None,
            description: None,
            stack: sandbox_stack(Some(cache_image)),
        });
        let (repositories, refused) = repository_access(&registry, &state, PROJECT);
        assert!(refused.is_empty());
        assert_eq!(
            repositories,
            vec![
                "alien-artifacts-prj_test",
                "sandbox-acme/alien-artifacts-prj_test"
            ]
        );
        std::mem::swap(&mut state.current_release, &mut state.target_release);
        assert_eq!(
            repository_access(&registry, &state, PROJECT).0,
            repositories,
            "rollback keeps access to both sides of the transition"
        );
        for image in [
            "123456789012.dkr.ecr.{region}.amazonaws.com/sandbox-acme/alien-artifacts-prj_other:new",
            "999999999999.dkr.ecr.{region}.amazonaws.com/sandbox-acme/alien-artifacts-prj_test:new",
            "123456789012.dkr.ecr.{region}.amazonaws.com/sandbox-acme/alien-artifacts-prj_test/nested:new",
        ] {
            let (repositories, refused) = repository_access(&registry, &aws_state_with_stack(sandbox_stack(Some(image))), PROJECT);
            assert!(repositories.is_empty(), "{image}");
            assert_eq!(refused.len(), 1, "{image}");
        }
    }
}
