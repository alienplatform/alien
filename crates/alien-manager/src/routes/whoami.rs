//! Whoami endpoint - returns identity from auth subject.

use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

use super::{auth, AppState};
use crate::traits::TokenType;

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WhoamiResponse {
    pub kind: String,
    pub id: String,
    pub scope: ScopeInfo,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ScopeInfo {
    pub r#type: String,
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

    let (kind, id, scope_type, dg_id, dp_id) = match subject.scope.token_type {
        TokenType::Admin => (
            "serviceAccount",
            subject.token_id.clone(),
            "admin",
            None,
            None,
        ),
        TokenType::DeploymentGroup => (
            "serviceAccount",
            subject
                .scope
                .deployment_group_id
                .clone()
                .unwrap_or_else(|| subject.token_id.clone()),
            "deployment-group",
            subject.scope.deployment_group_id.clone(),
            None,
        ),
        TokenType::Deployment => (
            "serviceAccount",
            subject
                .scope
                .deployment_id
                .clone()
                .unwrap_or_else(|| subject.token_id.clone()),
            "deployment",
            None,
            subject.scope.deployment_id.clone(),
        ),
    };

    Json(WhoamiResponse {
        kind: kind.to_string(),
        id: id.to_string(),
        scope: ScopeInfo {
            r#type: scope_type.to_string(),
            deployment_group_id: dg_id,
            deployment_id: dp_id,
        },
    })
    .into_response()
}
