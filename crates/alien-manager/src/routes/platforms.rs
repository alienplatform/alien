//! Configured platforms endpoint.
//!
//! Returns the list of platforms that have artifact registries configured,
//! so the CLI can auto-discover which platforms to build and release for.

use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

use super::{auth, AppState};
use crate::error::ErrorData;

#[derive(Debug, Serialize)]
pub struct PlatformsResponse {
    pub platforms: Vec<String>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/platforms", get(get_platforms))
}

async fn get_platforms(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    // Same gate as build-config: any caller that can create a release in the
    // project can read the platform routing table.
    if !state.authz.can_create_release(&subject, "default") {
        return ErrorData::forbidden("Platforms list requires write access").into_response();
    }

    let platforms = state
        .registry_routing_table
        .configured_platforms()
        .into_iter()
        .map(|p| p.as_str().to_string())
        .collect();

    Json(PlatformsResponse { platforms }).into_response()
}
