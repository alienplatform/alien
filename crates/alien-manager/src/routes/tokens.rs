//! Token management REST API endpoints.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::error::ErrorData;
use crate::traits::TokenRecord;

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

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/tokens", get(list_tokens))
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

async fn list_tokens(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };    if !subject.is_workspace_admin() {
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

async fn delete_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };    if !subject.is_workspace_admin() {
        return ErrorData::forbidden("Admin access required").into_response();
    }

    match state.token_store.delete_token(&id).await {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}
