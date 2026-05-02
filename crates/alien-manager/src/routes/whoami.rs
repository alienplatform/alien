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
            "deploymentGroup",
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

    Json(WhoamiResponse {
        kind: kind.to_string(),
        id,
        workspace_id: subject.workspace_id.clone(),
        role: serde_json::to_value(&subject.role)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default(),
        scope: ScopeInfo {
            r#type: scope_type.to_string(),
            project_id,
            deployment_group_id: dg_id,
            deployment_id: dp_id,
        },
    })
    .into_response()
}
