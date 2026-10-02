//! `GET /v1/manager` — what this manager is and what it serves.
//!
//! Clients use it to address the manager the way deployments reach it (its
//! public URL) and to decide which features to offer, instead of guessing
//! from how they connected.

use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

use super::{auth, AppState};
use crate::auth::Role;
use crate::error::ErrorData;

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ManagerInfoResponse {
    /// Public URL deployments and customers use to reach this manager.
    pub url: String,
    /// Registry host (`host[:port]`) release images and charts are pulled from.
    pub registry_host: String,
    /// Manager version.
    pub version: String,
    /// Features this manager serves.
    pub capabilities: ManagerCapabilities,
    /// Operator image the charts this manager serves install.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_image: Option<String>,
    /// Public key (`ed25519:<base64>`) that air-gapped environments verify
    /// bundles from this manager with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_signing_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ManagerCapabilities {
    /// Requests into deployments through `/v1/deployments/{id}/tunnels/...`.
    pub tunnels: bool,
    /// Helm charts at `oci://<registryHost>/charts/<stack>`.
    pub charts: bool,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/manager", get(manager_info))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/manager",
    tag = "manager",
    responses(
        (status = 200, description = "Manager information", body = ManagerInfoResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn manager_info(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(e) => return e.into_response(),
    };
    if subject.role == Role::ComputePlanner {
        return ErrorData::forbidden("Compute plan credentials cannot inspect manager identity")
            .into_response();
    }
    let url = state.config.base_url();
    Json(ManagerInfoResponse {
        registry_host: alien_core::image_rewrite::strip_url_scheme(&url).to_string(),
        url: url.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: ManagerCapabilities {
            tunnels: state.tunnels.is_some(),
            charts: state.charts.is_some(),
        },
        operator_image: state
            .charts
            .as_ref()
            .map(|charts| charts.deployed_operator_image(&state.config.base_url())),
        bundle_signing_key: state
            .bundle_signing_key
            .as_ref()
            .map(|key| key.public_key()),
    })
    .into_response()
}
