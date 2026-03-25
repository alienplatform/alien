pub mod artifact_registry;
pub mod auth;
pub mod credentials;
pub mod gcp;
pub mod initialize;
pub mod telemetry_query;

use std::sync::Arc;

use axum::{
    routing::{get, post},
    Extension, Router,
};

use crate::providers::platform_api::PlatformState;

/// Build the platform-specific routes (agent init, telemetry query, artifact registry,
/// credential resolution, GCP OAuth). Merged into the main router only in Platform mode.
pub fn build_platform_routes(
    ext: Arc<PlatformState>,
) -> Router<crate::routes::AppState> {
    Router::new()
        // Agent initialization (platform-specific: creates deployment+token via Platform API)
        .route(
            "/v1/initialize",
            post(initialize::initialize_agent),
        )
        // DeepStore query/search proxy (JWT-validated)
        .route("/v1/search", post(telemetry_query::search_logs))
        .route("/v1/logs/search", post(telemetry_query::search_logs))
        .route(
            "/v1/field-capabilities",
            post(telemetry_query::field_capabilities),
        )
        .route(
            "/v1/drafts/documents",
            post(telemetry_query::fetch_draft_documents),
        )
        // Artifact registry management
        .route(
            "/v1/artifact-registry/repositories",
            post(artifact_registry::create_repository),
        )
        .route(
            "/v1/artifact-registry/repositories/{repo_id}",
            get(artifact_registry::get_repository),
        )
        .route(
            "/v1/artifact-registry/repositories/{repo_id}/credentials",
            post(artifact_registry::get_credentials),
        )
        .route(
            "/v1/artifact-registry/repositories/{repo_id}/cross-account-access/add",
            post(artifact_registry::add_cross_account_access),
        )
        .route(
            "/v1/artifact-registry/repositories/{repo_id}/cross-account-access/remove",
            post(artifact_registry::remove_cross_account_access),
        )
        // BYOB credential resolution
        .route(
            "/v1/deployment/resolve-credentials",
            post(credentials::resolve_credentials),
        )
        // GCP OAuth onboarding
        .route(
            "/v1/google-cloud-login",
            get(gcp::google_cloud_login),
        )
        .route(
            "/v1/google-cloud-login/callback",
            get(gcp::google_cloud_callback),
        )
        .route(
            "/v1/gcp/project-metadata",
            post(gcp::get_project_metadata),
        )
        .layer(Extension(ext))
}
