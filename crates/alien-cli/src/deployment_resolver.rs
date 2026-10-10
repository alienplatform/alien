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
//! - `<group>/<name>` — listed with exact group and name filters. The
//!   platform resolves the group name to an ID server-side.
//!
//! Anything else (bare names, empty parts, more than one `/`) is rejected
//! up front with an actionable error. There is intentionally no fuzzy
//! search or pagination walk.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context};
use alien_manager_api::types::DeploymentResponse;
use alien_manager_api::{Client, SdkResultExt};

/// Resolve `spec` to a deployment.
///
/// `is_dev` only affects the hint surfaced in error messages
/// (`alien dev deployments ls` vs `alien deployments ls`).
pub async fn resolve(manager: &Client, spec: &str, is_dev: bool) -> Result<DeploymentResponse> {
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
    match manager
        .get_deployment()
        .id(id)
        .send()
        .await
        .into_sdk_error()
        .await
    {
        Ok(response) => Ok(response.into_inner()),
        // Only a 404 means the deployment doesn't exist; a server or network error says
        // nothing about that and keeps its own retryable status for the caller.
        Err(error) => {
            let message = if error.http_status_code == Some(404) {
                format!("Deployment '{id}' was not found.")
            } else {
                format!("Failed to read deployment '{id}'")
            };
            Err(error).context(ErrorData::ApiRequestFailed { message, url: None })
        }
    }
}

async fn resolve_by_group_and_name(
    manager: &Client,
    group: &str,
    name: &str,
    is_dev: bool,
) -> Result<DeploymentResponse> {
    // The manager SDK exposes the filter as `deployment_group_id`, but the
    // platform accepts either an ID or a group *name* on that param — it
    // resolves name→id internally. We pass the user-supplied group name.
    let response = manager
        .list_deployments()
        .deployment_group_id(group)
        .name(name)
        .include(vec!["deploymentGroup".to_string()])
        .send()
        .await
        .into_sdk_error()
        .await
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
                && d.deployment_group.as_ref().map(|dg| dg.name.as_str()) == Some(group)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::render_human_error;
    use httpmock::{Method::GET, MockServer};
    use serde_json::json;

    /// `alien deployments get dep_...` resolves through `get_deployment`. A
    /// failure the manager reports with a status the route's OpenAPI does not
    /// list (400 here) must reach the user with the manager's code, message
    /// and source, not as "Unexpected response: 400 Bad Request".
    #[tokio::test]
    async fn get_by_id_surfaces_the_manager_error_for_an_unlisted_status() {
        let server = MockServer::start_async().await;
        let read = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/deployments/dep_test");
                then.status(400).json_body(json!({
                    "code": "DEPLOYMENT_CONTEXT_UNAVAILABLE",
                    "message": "Failed to load deployment context",
                    "retryable": false,
                    "internal": false,
                    "httpStatusCode": 400,
                    "source": {
                        "code": "HOSTNAME_CONFLICT",
                        "message": "Hostname 'a.example.com' is used by both resources 'd1' and 'd2'",
                        "retryable": false,
                        "internal": false,
                        "httpStatusCode": 400
                    }
                }));
            })
            .await;

        let error = resolve(&Client::new(&server.base_url()), "dep_test", false)
            .await
            .expect_err("a 400 from the manager should fail the lookup");
        read.assert_async().await;

        assert_eq!(error.code, "API_REQUEST_FAILED");
        assert_eq!(error.http_status_code, Some(400));
        assert!(!error.retryable);
        let manager_error = error.source.as_ref().expect("manager error is kept");
        assert_eq!(manager_error.code, "DEPLOYMENT_CONTEXT_UNAVAILABLE");
        assert_eq!(manager_error.message, "Failed to load deployment context");
        let cause = manager_error
            .source
            .as_ref()
            .expect("the manager's source error is kept");
        assert_eq!(cause.code, "HOSTNAME_CONFLICT");

        let rendered = render_human_error(&error);
        assert!(
            rendered.contains("Failed to load deployment context"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Hostname 'a.example.com' is used by both resources 'd1' and 'd2'"),
            "{rendered}"
        );
        assert!(!rendered.contains("Unexpected response"), "{rendered}");
    }

    /// A status the route does list (404) keeps the manager's error too, and
    /// still maps to the "not found" message.
    #[tokio::test]
    async fn get_by_id_keeps_the_manager_error_for_a_listed_status() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/deployments/dep_test");
                then.status(404).json_body(json!({
                    "code": "DEPLOYMENT_NOT_FOUND",
                    "message": "Deployment 'dep_test' not found",
                    "retryable": false,
                    "internal": false,
                    "httpStatusCode": 404
                }));
            })
            .await;

        let error = resolve(&Client::new(&server.base_url()), "dep_test", false)
            .await
            .expect_err("a 404 should fail the lookup");

        assert_eq!(error.http_status_code, Some(404));
        assert!(error
            .message
            .contains("Deployment 'dep_test' was not found."));
        let manager_error = error.source.as_ref().expect("manager error is kept");
        assert_eq!(manager_error.code, "DEPLOYMENT_NOT_FOUND");
        assert_eq!(manager_error.message, "Deployment 'dep_test' not found");
    }
}
