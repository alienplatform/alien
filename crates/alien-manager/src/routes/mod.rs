//! REST API route handlers for alien-manager.

pub mod commands;
pub mod credentials;
pub mod deployment_groups;
pub mod deployments;
pub mod health;
pub mod releases;
pub mod sync;
pub mod telemetry;
pub mod whoami;

mod auth;

use std::sync::Arc;

use alien_commands::server::{create_axum_router, CommandServer, HasCommandServer};
use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;

use crate::traits::*;

/// Shared state for all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub deployment_store: Arc<dyn DeploymentStore>,
    pub release_store: Arc<dyn ReleaseStore>,
    pub token_store: Arc<dyn TokenStore>,
    pub auth_validator: Arc<dyn AuthValidator>,
    pub telemetry_backend: Arc<dyn TelemetryBackend>,
    pub credential_resolver: Arc<dyn CredentialResolver>,
    pub command_server: Arc<CommandServer>,
    pub config: Arc<crate::config::ManagerConfig>,
}

impl HasCommandServer for AppState {
    fn command_server(&self) -> &Arc<CommandServer> {
        &self.command_server
    }
}

/// Create the complete router with all routes.
pub fn create_router(state: AppState) -> Router {
    // Command server routes (nested under /v1)
    let commands_router: Router<AppState> = create_axum_router();

    Router::new()
        // Health (no auth)
        .route("/health", get(health::health))
        // Identity
        .merge(whoami::router())
        // Deployments
        .merge(deployments::router())
        // Releases
        .merge(releases::router())
        // Deployment groups
        .merge(deployment_groups::router())
        // Commands (list endpoint + command server protocol)
        .merge(commands::router())
        .nest("/v1", commands_router)
        // Telemetry
        .route("/v1/logs", post(telemetry::ingest_logs))
        .route("/v1/traces", post(telemetry::ingest_traces))
        .route("/v1/metrics", post(telemetry::ingest_metrics))
        // Sync
        .merge(sync::router())
        // Credentials
        .merge(credentials::router())
        .with_state(state)
        .layer(CorsLayer::permissive())
}
