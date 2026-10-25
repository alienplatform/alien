//! Health check endpoint.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use super::AppState;

#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    /// Oldest setup registration format accepted by this manager.
    #[serde(default)]
    pub min_setup_import_format_version: u32,
    /// Newest setup registration format accepted by this manager.
    #[serde(default)]
    pub max_setup_import_format_version: u32,
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
        min_setup_import_format_version:
            alien_core::import::MIN_SUPPORTED_SETUP_IMPORT_FORMAT_VERSION,
        max_setup_import_format_version:
            alien_core::import::MAX_SUPPORTED_SETUP_IMPORT_FORMAT_VERSION,
        operation_result_contract: state.command_server.persists_operation_result_contract(),
    })
}
