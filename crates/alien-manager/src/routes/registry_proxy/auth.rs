use super::*;

// ---------------------------------------------------------------------------
// Auth helpers
// ---------------------------------------------------------------------------

/// Validate that the caller has push permissions. The project_id comes
/// from the **routing table** — each provider composes repo names as
/// `{prefix}{sep}{name}` (`-` for ECR, `/` for GAR/ACR/Local), so the
/// routing-table prefix lookup gives us the project's name unambiguously.
/// Pushes that don't match any prefix fall back to `"default"`; the
/// configured [`crate::auth::Authz`] impl decides whether to allow.
pub(super) fn require_push_auth(
    state: &AppState,
    subject: &Subject,
    repo_name: &str,
) -> Result<(), Response> {
    let project_id = state
        .registry_routing_table
        .project_id_for_repo(repo_name)
        .unwrap_or("default");
    if state.authz.can_push_image(subject, project_id, repo_name) {
        Ok(())
    } else {
        Err(oci_error(
            StatusCode::FORBIDDEN,
            "DENIED",
            "Caller cannot push images to this project.",
        ))
    }
}

/// Validate that a deployment token can access the requested repo.
///
/// Uses the pull validation cache to avoid repeated DB lookups. Workspace-
/// scoped subjects bypass repo validation (they can pull anything in the
/// workspace).
pub(super) async fn validate_pull_access(
    state: &AppState,
    subject: &Subject,
    repo_name: &str,
) -> Result<(), Response> {
    let deployment_id = match &subject.scope {
        Scope::Workspace | Scope::Project { .. } => return Ok(()),
        Scope::DeploymentGroup { .. } => {
            return Err(oci_error(
                StatusCode::FORBIDDEN,
                "DENIED",
                "Registry proxy pulls require a deployment token",
            ))
        }
        Scope::Command { .. } => {
            return Err(oci_error(
                StatusCode::FORBIDDEN,
                "DENIED",
                "Command payload tokens cannot pull images",
            ))
        }
        Scope::Deployment {
            project_id,
            deployment_id,
        } => {
            // A deployment token may always pull from its own project's
            // artifact repository. Source-built resources (Worker/Container/
            // Daemon) publish their images there under `{prefix}-{project_id}`,
            // and those repos are not discoverable from the stack's `image`
            // fields — so the per-release allow-list below would otherwise
            // reject them (e.g. a source-built Daemon).
            if state.registry_routing_table.project_id_for_repo(repo_name)
                == Some(project_id.as_str())
            {
                return Ok(());
            }
            deployment_id.as_str()
        }
    };

    // Check cache first.
    let repo_names = if let Some((_release_id, cached_repos)) =
        state.pull_validation_cache.get(deployment_id)
    {
        cached_repos
    } else {
        // Cache miss — query DB.
        // The caller has already been authenticated and scoped to this
        // deployment. Use that subject for the deployment lookup so platform
        // managers can hydrate pull deployments through their pull sync path.
        let system = crate::auth::Subject::system();
        let deployment = state
            .deployment_store
            .get_deployment(subject, deployment_id)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to get deployment for registry proxy");
                oci_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Failed to resolve deployment",
                )
            })?
            .ok_or_else(|| {
                oci_error(
                    StatusCode::NOT_FOUND,
                    "NAME_UNKNOWN",
                    format!("Deployment {} not found", deployment_id),
                )
            })?;

        let release_id = deployment
            .current_release_id
            .as_deref()
            .or(deployment.desired_release_id.as_deref())
            .ok_or_else(|| {
                oci_error(
                    StatusCode::NOT_FOUND,
                    "NAME_UNKNOWN",
                    "Deployment has no release",
                )
            })?
            .to_string();

        let release = state
            .release_store
            .get_release(&system, &release_id)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to get release for registry proxy");
                oci_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Failed to resolve release",
                )
            })?
            .ok_or_else(|| {
                oci_error(
                    StatusCode::NOT_FOUND,
                    "NAME_UNKNOWN",
                    format!("Release {} not found", release_id),
                )
            })?;

        let repos = release
            .stacks
            .values()
            .flat_map(|stack| extract_repo_names(stack))
            .collect::<Vec<_>>();

        // Cache the result.
        state
            .pull_validation_cache
            .insert(deployment_id.to_string(), release_id, repos.clone());

        repos
    };

    if !repo_names.iter().any(|r| r == repo_name) {
        return Err(oci_error(
            StatusCode::FORBIDDEN,
            "DENIED",
            format!(
                "Repository '{}' not found in deployment's release",
                repo_name
            ),
        ));
    }

    Ok(())
}

/// Extract the set of repo names from a release's stack.
pub(super) fn extract_repo_names(stack: &alien_core::Stack) -> Vec<String> {
    use alien_core::image_rewrite::strip_registry_host;
    use alien_core::{Container, ContainerCode, Daemon, DaemonCode, Worker, WorkerCode};

    let mut repos = Vec::new();

    for (_resource_id, entry) in stack.resources() {
        let image = if let Some(func) = entry.config.downcast_ref::<Worker>() {
            match &func.code {
                WorkerCode::Image { image } => Some(image.as_str()),
                WorkerCode::Source { .. } => None,
            }
        } else if let Some(container) = entry.config.downcast_ref::<Container>() {
            match &container.code {
                ContainerCode::Image { image } => Some(image.as_str()),
                ContainerCode::Source { .. } => None,
            }
        } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
            match &daemon.code {
                DaemonCode::Image { image } => Some(image.as_str()),
                DaemonCode::Source { .. } => None,
            }
        } else {
            None
        };

        if let Some(image_uri) = image {
            if let Some(stripped) = strip_registry_host(image_uri) {
                let repo = stripped.split(':').next().unwrap_or(&stripped);
                let repo = repo.split('@').next().unwrap_or(repo);
                if !repo.is_empty() && !repos.contains(&repo.to_string()) {
                    repos.push(repo.to_string());
                }
            }
        }
    }

    repos
}
