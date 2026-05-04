//! Deployment group REST API endpoints.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::error::ErrorData;
use crate::ids;
use crate::traits::{
    CreateDeploymentGroupParams, CreateTokenParams, DeploymentGroupRecord, TokenType,
};

use super::{auth, AppState};

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateDeploymentGroupRequest {
    pub name: String,
    #[serde(default = "default_max_deployments")]
    pub max_deployments: i64,
}

fn default_max_deployments() -> i64 {
    100
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentGroupResponse {
    pub id: String,
    pub name: String,
    pub max_deployments: i64,
    pub deployment_count: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ListDeploymentGroupsResponse {
    pub items: Vec<DeploymentGroupResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateTokenResponse {
    pub token: String,
    pub deployment_group_id: String,
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/deployment-groups",
            post(create_deployment_group).get(list_deployment_groups),
        )
        .route("/v1/deployment-groups/{id}", get(get_deployment_group))
        .route(
            "/v1/deployment-groups/{id}/tokens",
            post(create_deployment_group_token),
        )
}

// --- Helpers ---

fn record_to_response(dg: &DeploymentGroupRecord) -> DeploymentGroupResponse {
    DeploymentGroupResponse {
        id: dg.id.clone(),
        name: dg.name.clone(),
        max_deployments: dg.max_deployments,
        deployment_count: dg.deployment_count,
        created_at: dg.created_at.to_rfc3339(),
    }
}

// --- Handlers ---

/// Every handler in this file runs `auth::require_auth(&state, &headers)`
/// and then threads `&subject` into the `DeploymentStore` calls — see the
/// trait doc on [`DeploymentStore`] for the convention.
///
/// `POST /v1/deployment-groups` — Inbound: workspace bearer.
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/deployment-groups",
    tag = "deployment-groups",
    request_body = CreateDeploymentGroupRequest,
    responses(
        (status = 200, description = "Deployment group created", body = DeploymentGroupResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn create_deployment_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateDeploymentGroupRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    // Single-project: the only valid project_id is "default". The configured
    // `Authz` impl decides whether the subject can create here.
    if !state.authz.can_create_deployment_group(&subject, "default") {
        return ErrorData::forbidden("Cannot create deployment group").into_response();
    }

    let dg = match state
        .deployment_store
        .create_deployment_group(
            &subject,
            CreateDeploymentGroupParams {
                name: req.name,
                max_deployments: req.max_deployments,
            },
        )
        .await
    {
        Ok(dg) => dg,
        Err(e) => return e.into_response(),
    };

    Json(record_to_response(&dg)).into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployment-groups",
    tag = "deployment-groups",
    responses(
        (status = 200, description = "List of deployment groups", body = ListDeploymentGroupsResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn list_deployment_groups(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let groups = match state.deployment_store.list_deployment_groups(&subject).await {
        Ok(g) => g,
        Err(e) => return e.into_response(),
    };

    // Pattern 1 (List Endpoints): per `authorization-guidelines.md`, scope-
    // filter the result then run `Authz.can_read_*` per item to produce the
    // visible subset.
    let items: Vec<DeploymentGroupResponse> = groups
        .iter()
        .filter(|dg| state.authz.can_read_deployment_group(&subject, dg))
        .map(record_to_response)
        .collect();

    Json(ListDeploymentGroupsResponse {
        items,
        next_cursor: None,
    })
    .into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployment-groups/{id}",
    tag = "deployment-groups",
    params(
        ("id" = String, Path, description = "Deployment group ID")
    ),
    responses(
        (status = 200, description = "Deployment group found", body = DeploymentGroupResponse),
        (status = 404, description = "Deployment group not found")
    ),
    security(
        ("bearer" = [])
    )
))]
async fn get_deployment_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let dg = match state.deployment_store.get_deployment_group(&subject, &id).await {
        Ok(Some(dg)) => dg,
        Ok(None) => return ErrorData::not_found_group(&id).into_response(),
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_read_deployment_group(&subject, &dg) {
        return ErrorData::forbidden("Cannot read deployment group").into_response();
    }

    Json(record_to_response(&dg)).into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/deployment-groups/{id}/tokens",
    tag = "deployment-groups",
    params(
        ("id" = String, Path, description = "Deployment group ID")
    ),
    responses(
        (status = 200, description = "Token created", body = CreateTokenResponse),
        (status = 404, description = "Deployment group not found")
    ),
    security(
        ("bearer" = [])
    )
))]
async fn create_deployment_group_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // Verify group exists, then authorize on the loaded entity.
    let dg = match state.deployment_store.get_deployment_group(&subject, &id).await {
        Ok(Some(dg)) => dg,
        Ok(None) => return ErrorData::not_found_group(&id).into_response(),
        Err(e) => return e.into_response(),
    };    if !state.authz.can_update_deployment_group(&subject, &dg) {
        return ErrorData::forbidden("Cannot mint tokens for this deployment group")
            .into_response();
    }

    // Generate deployment group token
    let (raw_token, key_prefix, key_hash) =
        ids::generate_token(TokenType::DeploymentGroup.prefix());

    match state
        .token_store
        .create_token(CreateTokenParams {
            token_type: TokenType::DeploymentGroup,
            key_prefix,
            key_hash,
            deployment_group_id: Some(id.clone()),
            deployment_id: None,
        })
        .await
    {
        Ok(_) => Json(CreateTokenResponse {
            token: raw_token,
            deployment_group_id: id,
        })
        .into_response(),
        Err(e) => e.into_response(),
    }
}
