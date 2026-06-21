//! Resolve a user-supplied deployment spec to a deployment record.
//!
//! Three commands (`debug`, `commands`, `deployments {get,delete,retry,
//! redeploy}`) all accept the same spec forms and used to each carry a
//! near-identical resolver that walked `list_deployments` and matched on
//! name. That had two problems: it broke once a workspace had more
//! deployments than fit in one page, and the loose name-only matching
//! could silently target the wrong deployment in a multi-tenant workspace
//! where the same name exists under different groups.
//!
//! This module is the single resolver. It accepts exactly two spec forms:
//!
//! - `dep_<id>` — looked up directly via `get_deployment(id)`.
//! - `<group>/<name>` — listed with the `deployment_group` filter pinned
//!   to `<group>` (the platform resolves the group name to an ID
//!   server-side); the result set is then filtered locally for an exact
//!   `name` match.
//!
//! Anything else (bare names, empty parts, more than one `/`) is rejected
//! up front with an actionable error. There is intentionally no fuzzy
//! search or pagination walk — both invite the same class of bug.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context};
use alien_manager_api::types::DeploymentResponse;
use alien_manager_api::{Client, SdkResultExt};

/// Resolve `spec` to a deployment.
///
/// `is_dev` only affects the hint surfaced in error messages
/// (`alien dev deployments ls` vs `alien deployments ls`).
pub async fn resolve(
    manager: &Client,
    spec: &str,
    is_dev: bool,
) -> Result<DeploymentResponse> {
    if spec.starts_with("dep_") {
        return resolve_by_id(manager, spec).await;
    }

    match spec.split_once('/') {
        Some((group, name)) if !group.is_empty() && !name.is_empty() && !name.contains('/') => {
            resolve_by_group_and_name(manager, group, name, is_dev).await
        }
        _ => Err(invalid_spec_error(spec)),
    }
}

async fn resolve_by_id(manager: &Client, id: &str) -> Result<DeploymentResponse> {
    manager
        .get_deployment()
        .id(id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("Deployment '{}' was not found.", id),
            url: None,
        })
        .map(|r| r.into_inner())
}

async fn resolve_by_group_and_name(
    manager: &Client,
    group: &str,
    name: &str,
    is_dev: bool,
) -> Result<DeploymentResponse> {
    // The platform's `deploymentGroup` filter accepts either an ID or an
    // exact name (it resolves name→id internally) — so this is the strict
    // path: server narrows by group, we then filter the (small) result set
    // for an exact name match.
    // The manager SDK exposes the filter as `deployment_group_id`, but the
    // platform accepts either an ID or a group *name* on that param — it
    // resolves name→id internally. We pass the user-supplied group name.
    let response = manager
        .list_deployments()
        .deployment_group_id(group)
        .include(vec!["deploymentGroup".to_string()])
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!(
                "Failed to list deployments in group '{}' for resolution",
                group
            ),
            url: None,
        })?
        .into_inner();

    let matches: Vec<DeploymentResponse> = response
        .items
        .into_iter()
        .filter(|d| {
            d.name.as_str() == name
                && d.deployment_group
                    .as_ref()
                    .map(|dg| dg.name.as_str())
                    == Some(group)
        })
        .collect();

    let list_cmd = if is_dev {
        "alien dev deployments ls"
    } else {
        "alien deployments ls"
    };

    match matches.len() {
        0 => Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment".to_string(),
            message: format!(
                "Deployment '{group}/{name}' not found. Verify the group and name with `{list_cmd}`."
            ),
        })),
        1 => Ok(matches.into_iter().next().expect("len == 1")),
        // Same workspace can't legitimately have two deployments with the
        // same `<group>/<name>` pair — the platform enforces uniqueness.
        // Surface loudly rather than pick one.
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment".to_string(),
            message: format!(
                "Multiple deployments matched '{group}/{name}'. Resolve by `dep_...` ID instead."
            ),
        })),
    }
}

fn invalid_spec_error(spec: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ValidationError {
        field: "deployment".to_string(),
        message: format!(
            "Invalid deployment spec '{spec}'. Expected either:\n  - `dep_<id>` (e.g. dep_7i6ynan6zoil4rj2eldvw95hmfua), or\n  - `<group>/<name>` (e.g. acme/prod).\nBare names are no longer accepted; the same name can exist under multiple groups."
        ),
    })
}
