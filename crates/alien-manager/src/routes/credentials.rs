//! Credential resolution endpoint.

use axum::{
    extract::{Json, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};

use crate::error::ErrorData;

use super::{auth, AppState};

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResolveCredentialsRequest {
    pub deployment_id: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResolveCredentialsResponse {
    pub client_config: serde_json::Value,
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/resolve-credentials", post(resolve_credentials))
}

// --- Handler ---

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/resolve-credentials",
    tag = "credentials",
    request_body = ResolveCredentialsRequest,
    responses(
        (status = 200, description = "Credentials resolved successfully", body = ResolveCredentialsResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn resolve_credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ResolveCredentialsRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // Admin or deployment group token (own group)
    if let Err(e) = auth::require_admin_or_any_group(&subject) {
        return e.into_response();
    }

    // Get the deployment
    let deployment = match state
        .deployment_store
        .get_deployment(&req.deployment_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&req.deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };

    // If deployment group token, verify it can access this deployment's group
    if subject.is_deployment_group() {
        if let Err(e) = auth::require_admin_or_group(&subject, &deployment.deployment_group_id) {
            return e.into_response();
        }
    }

    // Resolve credentials
    match state.credential_resolver.resolve(&deployment).await {
        Ok(client_config) => {
            let config_value = serde_json::to_value(&client_config).unwrap_or_default();
            Json(ResolveCredentialsResponse {
                client_config: config_value,
            })
            .into_response()
        }
        Err(e) => e.into_response(),
    }
}
