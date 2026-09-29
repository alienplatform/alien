//! Token management REST API endpoints.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::error::ErrorData;
use crate::ids;
use crate::traits::{CreateTokenParams, TokenRecord, TokenType};

use super::{auth, AppState};

// --- Response types ---

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub id: String,
    pub token_type: String,
    pub key_prefix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ListTokensResponse {
    pub items: Vec<TokenResponse>,
}

/// Token kinds an admin can create through the API.
#[derive(Debug, Clone, Copy, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CreatableTokenType {
    /// Calls deployment tunnels and nothing else.
    Tunnel,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateTokenRequest {
    #[serde(rename = "type")]
    pub token_type: CreatableTokenType,
    /// Limit the token to one deployment group's deployments.
    #[serde(default)]
    pub deployment_group_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreatedTokenResponse {
    /// The raw token. Shown once; only its hash is stored.
    pub token: String,
    #[serde(flatten)]
    pub record: TokenResponse,
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/tokens", get(list_tokens).post(create_token))
        .route("/v1/tokens/{id}", axum::routing::delete(delete_token))
}

// --- Helpers ---

fn record_to_response(record: &TokenRecord) -> TokenResponse {
    TokenResponse {
        id: record.id.clone(),
        token_type: record.token_type.to_string(),
        key_prefix: record.key_prefix.clone(),
        deployment_group_id: record.deployment_group_id.clone(),
        deployment_id: record.deployment_id.clone(),
        created_at: record.created_at.to_rfc3339(),
    }
}

// --- Handlers ---

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/tokens",
    tag = "tokens",
    responses((status = 200, description = "Tokens", body = ListTokensResponse)),
    security(("bearer" = []))
))]
async fn list_tokens(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if !subject.is_workspace_admin() {
        return ErrorData::forbidden("Admin access required").into_response();
    }

    match state.token_store.list_tokens().await {
        Ok(tokens) => Json(ListTokensResponse {
            items: tokens.iter().map(record_to_response).collect(),
        })
        .into_response(),
        Err(e) => e.into_response(),
    }
}

/// `POST /v1/tokens` — create a scoped token (admin only).
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/tokens",
    tag = "tokens",
    request_body = CreateTokenRequest,
    responses((status = 201, description = "Token created", body = CreatedTokenResponse)),
    security(("bearer" = []))
))]
async fn create_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateTokenRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if !subject.is_workspace_admin() {
        return ErrorData::forbidden("Admin access required").into_response();
    }
    if let Some(group_id) = &req.deployment_group_id {
        match state
            .deployment_store
            .get_deployment_group(&subject, group_id)
            .await
        {
            Ok(Some(_)) => {}
            Ok(None) => {
                return alien_error::AlienError::new(ErrorData::DeploymentGroupNotFound {
                    deployment_group_id: group_id.clone(),
                })
                .into_response()
            }
            Err(e) => return e.into_response(),
        }
    }
    let token_type = match req.token_type {
        CreatableTokenType::Tunnel => TokenType::Tunnel,
    };
    let (raw_token, key_prefix, key_hash) = ids::generate_token(token_type.prefix());
    match state
        .token_store
        .create_token(CreateTokenParams {
            token_type,
            key_prefix,
            key_hash,
            deployment_group_id: req.deployment_group_id,
            deployment_id: None,
        })
        .await
    {
        Ok(record) => (
            axum::http::StatusCode::CREATED,
            Json(CreatedTokenResponse {
                token: raw_token,
                record: record_to_response(&record),
            }),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    delete,
    path = "/v1/tokens/{id}",
    tag = "tokens",
    params(("id" = String, Path, description = "Token ID")),
    responses((status = 204, description = "Token revoked")),
    security(("bearer" = []))
))]
async fn delete_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if !subject.is_workspace_admin() {
        return ErrorData::forbidden("Admin access required").into_response();
    }

    match state.token_store.delete_token(&id).await {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}
