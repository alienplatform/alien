//! Commands list endpoint.
//!
//! The command protocol routes (create, leases, responses) are handled by
//! alien-commands' `create_axum_router()` nested under `/v1`.
//! This module adds the get status endpoint at the server level.

use super::AppState;
use axum::Router;
use serde::Serialize;

// --- Response types ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResponse {
    pub id: String,
    pub deployment_id: String,
    pub name: String,
    pub state: String,
    pub attempt: u32,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatched_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_size_bytes: Option<u64>,
}

// --- Router ---
//
// Note: The full command protocol (create, leases, responses) is handled by
// the alien-commands `create_axum_router()` nested under `/v1` in mod.rs.
// This router does not add additional routes to avoid conflicts with the nested router.

pub fn router() -> Router<AppState> {
    Router::new()
}
