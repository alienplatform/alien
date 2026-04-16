//! Build configuration endpoint.
//!
//! Returns the repository name prefix for a given platform, so the CLI can
//! determine where to push images without hardcoding the repo path.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::{auth, AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildConfigQuery {
    pub platform: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildConfigResponse {
    pub repository_name: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/build-config", get(get_build_config))
}

async fn get_build_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<BuildConfigQuery>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if let Err(e) = auth::require_admin(&subject) {
        return e.into_response();
    }

    let platform = query.platform.as_deref();

    // If platform specified, find the prefix for that platform.
    if let Some(platform_str) = platform {
        let platform: alien_core::Platform = match platform_str.parse() {
            Ok(p) => p,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("Unknown platform: {}", platform_str),
                )
                    .into_response()
            }
        };

        if let Some(prefix) = state.registry_routing_table.prefix_for_platform(platform) {
            return Json(BuildConfigResponse {
                repository_name: prefix.to_string(),
            })
            .into_response();
        }

        return (
            StatusCode::NOT_FOUND,
            format!(
                "No artifact registry configured for platform '{}'",
                platform_str
            ),
        )
            .into_response();
    }

    // No platform specified -- return the default (catch-all) route.
    let routes = &state.registry_routing_table;
    if routes.is_empty() {
        return (StatusCode::NOT_FOUND, "No artifact registries configured").into_response();
    }

    // Resolve with empty repo name to get the catch-all / first route.
    if let Some(route) = routes.resolve("") {
        return Json(BuildConfigResponse {
            repository_name: route.prefix.clone(),
        })
        .into_response();
    }

    (
        StatusCode::BAD_REQUEST,
        "Multiple registries configured. Specify ?platform=aws|gcp|azure|local",
    )
        .into_response()
}
