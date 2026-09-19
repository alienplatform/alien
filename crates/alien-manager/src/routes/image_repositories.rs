//! Image repository provisioning.
//!
//! ECR needs a project's repository to exist before a push, while GAR and ACR
//! create it on first push. This endpoint makes the three behave the same and
//! returns the repository name to push to through the registry proxy.

use alien_core::Platform;
use alien_error::Context;
use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::{auth, AppState};
use crate::error::ErrorData;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionImageRepositoryRequest {
    pub project_id: String,
    pub platform: Platform,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionImageRepositoryResponse {
    /// Repository path under the manager's registry host, e.g. `alien-artifacts-prj_x`.
    pub repository: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/image-repositories", post(provision_image_repository))
}

async fn provision_image_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProvisionImageRepositoryRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if !state
        .authz
        .can_provision_image_repository(&subject, &request.project_id)
    {
        return ErrorData::forbidden("Caller cannot provision this project's image repository")
            .into_response();
    }

    match provision(&state, &request.project_id, request.platform).await {
        Ok(repository) => Json(ProvisionImageRepositoryResponse { repository }).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn provision(
    state: &AppState,
    project_id: &str,
    platform: Platform,
) -> crate::error::Result<String> {
    if project_id.is_empty() || project_id.contains('/') {
        return Err(ErrorData::bad_request(
            "projectId must be a single path segment",
        ));
    }
    // The local registry names repositories differently from its proxy route,
    // so only cloud registries return a name the proxy can push to.
    if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        return Err(ErrorData::bad_request(format!(
            "Image repositories are provisioned for cloud registries only, not '{platform}'"
        )));
    }
    let route = state
        .registry_routing_table
        .route_for_platform(platform)
        .ok_or_else(|| {
            ErrorData::internal(format!(
                "No artifact registry is configured for '{platform}'"
            ))
        })?;
    let failed = || ErrorData::ImageRepositoryProvisioningFailed {
        project_id: project_id.to_string(),
        platform,
    };
    let registry = route
        .provider
        .load_artifact_registry(&route.binding_name)
        .await
        .context(failed())?;
    let repository = registry
        .create_repository(project_id)
        .await
        .context(failed())?;
    Ok(repository.name)
}
