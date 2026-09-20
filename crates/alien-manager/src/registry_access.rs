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
use crate::traits::deployment_store::{DeploymentFilter, DeploymentRecord};
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
    let rsm_access = extract_rsm_access_configuration(stack_state);
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

    let (repo_ids, refused_any) = repository_access(artifact_registry.as_ref(), state, project_id);
    if repo_ids.is_empty() {
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

    opened.sort();
    opened.dedup();
    let granted_everything =
        all_complete && !refused_any && repo_ids.iter().all(|repo_id| opened.contains(repo_id));
    let rm = state
        .runtime_metadata
        .get_or_insert_with(RuntimeMetadata::default);
    // The record says what the policy now holds, so an unchanged one is not rewritten every tick.
    // The marker says the deployment has everything it asked for, which a refused reference denies
    // until a later release names a repository this project owns — and that release changes what
    // is needed, so the comparison above grants it.
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
    project_id: &str,
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
                if let Some(own) = project_repository(artifact_registry.as_ref(), project_id) {
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
    // GAR binds IAM on the registry itself and ignores the repository, so a GCP grant is always
    // named by the prefix even when nothing else names it.
    if repo_ids.is_empty() && recorded_grant && matches!(platform, Platform::Gcp) {
        let prefix = artifact_registry.upstream_repository_prefix();
        if !prefix.is_empty() {
            repo_ids.push(prefix);
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
        )
        .await?;
    }
    Ok(())
}

fn repository_ids_for_access(
    artifact_registry: &dyn ArtifactRegistry,
    state: &DeploymentState,
    project_id: &str,
) -> Vec<String> {
    repository_access(artifact_registry, state, project_id).0
}

/// The repositories to grant, and whether any sandbox base image was refused as not this
/// project's.
fn repository_access(
    artifact_registry: &dyn ArtifactRegistry,
    state: &DeploymentState,
    project_id: &str,
) -> (Vec<String>, bool) {
    let prefix = artifact_registry.upstream_repository_prefix();
    if prefix.is_empty() {
        return (Vec::new(), false);
    }

    let platform = state
        .environment_info
        .as_ref()
        .map(EnvironmentInfo::platform);
    let sandbox_source = platform.map_or(SandboxImageSource::None, sandbox_image_source);

    if matches!(platform, Some(Platform::Aws)) {
        let mut repo_ids = HashSet::new();
        let refused = collect_image_repositories(
            state,
            &prefix,
            &artifact_registry.registry_endpoint(),
            project_id,
            sandbox_source,
            &mut repo_ids,
        );
        let mut repo_ids: Vec<_> = repo_ids.into_iter().collect();
        repo_ids.sort();
        return (repo_ids, refused);
    }

    // No per-project path to hold a sandbox to here: a GCP sandbox pulls the agents image Alien
    // publishes under the shared prefix, and GAR binds IAM at the repository, so every grant on
    // this platform opens the same shared repository. AWS narrows its own grant per project.
    if !has_image_in_repository_prefix(state, &prefix, sandbox_source) {
        return (Vec::new(), false);
    }

    let repo_ids = HashSet::from([prefix]);
    let mut repo_ids: Vec<_> = repo_ids.into_iter().collect();
    repo_ids.sort();
    (repo_ids, false)
}

fn has_image_in_repository_prefix(
    state: &DeploymentState,
    prefix: &str,
    sandbox_source: SandboxImageSource,
) -> bool {
    let stack_matches = |stack: &Stack| {
        stack.resources().any(|(_id, entry)| {
            resource_image_reference(entry, sandbox_source)
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

/// The one repository a sandbox in this project can be granted, if the registry names repositories
/// by prefix at all.
fn project_repository(
    artifact_registry: &dyn ArtifactRegistry,
    project_id: &str,
) -> Option<String> {
    let prefix = artifact_registry.upstream_repository_prefix();
    (!prefix.is_empty()).then(|| format!("{prefix}-{project_id}"))
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
    prefix: &str,
    registry_endpoint: &str,
    project_id: &str,
    sandbox_source: SandboxImageSource,
    repo_ids: &mut HashSet<String>,
) -> bool {
    let mut refused = false;
    // Every project shares the prefix, so a sandbox's free-text base image could otherwise name
    // a sibling project's repository and open it to this deployment's account.
    let own_repository = format!("{prefix}-{project_id}");
    let mut collect = |stack: &Stack| {
        for (id, entry) in stack.resources() {
            let Some(image) = resource_image_reference(entry, sandbox_source) else {
                continue;
            };
            let Some(repo_id) = ecr_repository_from_image(image, prefix) else {
                continue;
            };
            // The host too: the grant is written on this registry, so a same-path repository in
            // another account would be granted here and pulled from there.
            let own_registry = served_by_registry(image, registry_endpoint);
            if entry.config.downcast_ref::<Sandbox>().is_some()
                && (repo_id != own_repository || !own_registry)
            {
                warn!(
                    resource_id = %id,
                    repository = %repo_id,
                    project_id = %project_id,
                    "Refusing registry access for a sandbox base image outside its project's repository"
                );
                refused = true;
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
    declares_sandbox: bool,
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

    #[derive(Debug)]
    struct TestArtifactRegistry {
        prefix: String,
        fail_remove: bool,
        removed_repository_is_gone: bool,
    }

    impl alien_bindings::traits::Binding for TestArtifactRegistry {}

    #[async_trait]
    impl ArtifactRegistry for TestArtifactRegistry {
        fn registry_endpoint(&self) -> String {
            // The ECR shape, since the sandbox check compares the account a reference names
            // against the account its grant would be written in.
            "https://123456789012.dkr.ecr.us-east-2.amazonaws.com".to_string()
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
                    .egress(SandboxEgress::Allow)
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
            prefix: "alien-artifacts-prj_test".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let state = aws_state_with_stack(worker_stack(
            "manager.example.com/alien-artifacts-prj_test:test-worker-abc123",
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
    fn gcp_registry_access_requires_worker_image_under_prefix() {
        let registry = TestArtifactRegistry {
            prefix: "test-project/alien-artifacts".to_string(),
            fail_remove: false,
            removed_repository_is_gone: false,
        };
        let state = gcp_state_with_stack(worker_stack(
            "manager.example.com/test-project/alien-artifacts/test-worker:abc123",
        ));

        assert_eq!(
            repository_ids_for_access(&registry, &state, PROJECT),
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
                        image: STORED_BUNDLE_URI.to_string(),
                    })
                    .private_base_image(
                        "123456789012.dkr.ecr.{region}.amazonaws.com/\
                         alien-artifacts-prj_test:agents-abc123"
                            .to_string(),
                    )
                    .egress(SandboxEgress::Allow)
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
                        image: STORED_BUNDLE_URI.to_string(),
                    })
                    .private_base_image(
                        "123456789012.dkr.ecr.{region}.amazonaws.com/\
                         alien-artifacts-prj_other:agents-abc123"
                            .to_string(),
                    )
                    .egress(SandboxEgress::Allow)
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
            refused,
            "the worker's grant must not mark the deployment fully granted"
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
            project_repository(&registry, PROJECT),
            Some("alien-artifacts-prj_test".to_string())
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
            "manager.example.com/test-project/alien-artifacts/agents:abc123",
            None,
        ));

        assert_eq!(
            repository_ids_for_access(&registry, &state, PROJECT),
            vec!["test-project/alien-artifacts".to_string()],
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
}
