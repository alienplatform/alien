//! Health check endpoint.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use super::AppState;

#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    /// True when operation result contracts are persisted before commands
    /// become executable.
    #[serde(default)]
    pub operation_result_contract: bool,
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Server is healthy", body = HealthResponse)
    )
))]
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        operation_result_contract: state.command_server.persists_operation_result_contract(),
    })
}
