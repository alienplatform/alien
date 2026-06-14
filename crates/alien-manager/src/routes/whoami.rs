//! Whoami endpoint — returns identity from the unified auth Subject.

use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

use super::{auth, AppState};
use crate::auth::{Scope, SubjectKind};

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WhoamiResponse {
    pub kind: String,
    pub id: String,
    pub workspace_id: String,
    /// Required by the platform-SDK `ServiceAccountSubject` parser when
    /// the CLI looks up a deployment-group token by workspace name.
    pub workspace_name: String,
    pub role: String,
    pub scope: ScopeInfo,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ScopeInfo {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/whoami", get(whoami))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/whoami",
    tag = "whoami",
    responses(
        (status = 200, description = "Returns the identity of the authenticated caller", body = WhoamiResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn whoami(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let (kind, id) = match &subject.kind {
        SubjectKind::User { id, .. } => ("user", id.clone()),
        SubjectKind::ServiceAccount { id } => ("serviceAccount", id.clone()),
    };

    let (scope_type, project_id, dg_id, dp_id) = match &subject.scope {
        Scope::Workspace => ("workspace", None, None, None),
        Scope::Project { project_id } => ("project", Some(project_id.clone()), None, None),
        Scope::DeploymentGroup {
            project_id,
            deployment_group_id,
        } => (
            // Note: the platform OpenAPI spec uses "deployment-group" (hyphen)
            // for the SubjectScope discriminator; the standalone manager's
            // serializer was emitting "deploymentGroup" (camelCase) and the
            // CLI couldn't parse it as a valid Subject variant. Aligned to
            // the platform spec so `alien deploy --token ...` works against
            // the standalone manager too.
            "deployment-group",
            Some(project_id.clone()),
            Some(deployment_group_id.clone()),
            None,
        ),
        Scope::Deployment {
            project_id,
            deployment_id,
        } => (
            "deployment",
            Some(project_id.clone()),
            None,
            Some(deployment_id.clone()),
        ),
    };

    // Translate the internal kebab-case role serialization to the
    // platform-spec dotted form (e.g. "deploymentGroup-deployer" /
    // "deployment-group-deployer" → "deployment-group.deployer"). The CLI's
    // shared platform SDK parses this enum strictly and rejects any
    // other shape.
    let role_internal = serde_json::to_value(&subject.role)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let role = platform_role_str(&role_internal);

    Json(WhoamiResponse {
        kind: kind.to_string(),
        id,
        workspace_id: subject.workspace_id.clone(),
        // Standalone manager doesn't track workspace names separately —
        // single-tenant OSS mode reuses the workspace id as the name.
        workspace_name: subject.workspace_id.clone(),
        role,
        scope: ScopeInfo {
            r#type: scope_type.to_string(),
            project_id,
            deployment_group_id: dg_id,
            deployment_id: dp_id,
        },
    })
    .into_response()
}

/// Map internal `Role` enum (rename_all = "kebab-case", e.g.
/// `deployment-group-deployer`) to the platform OpenAPI spec form
/// (`{scope}.{role}` with a dot), e.g. `deployment-group.deployer`.
fn platform_role_str(internal: &str) -> String {
    match internal {
        "workspace-viewer" => "workspace.viewer".to_string(),
        "workspace-member" => "workspace.member".to_string(),
        "workspace-admin" => "workspace.admin".to_string(),
        "project-viewer" => "project.viewer".to_string(),
        "project-developer" => "project.developer".to_string(),
        "deployment-viewer" => "deployment.viewer".to_string(),
        "deployment-manager" => "deployment.manager".to_string(),
        "deployment-group-deployer" => "deployment-group.deployer".to_string(),
        "manager-runtime" => "manager.runtime".to_string(),
        other => other.to_string(),
    }
}
