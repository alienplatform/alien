//! REST API route handlers for alien-manager.

pub mod build_config;
pub mod commands;
pub mod credentials;
pub mod deployment_groups;
pub mod deployments;
pub mod health;
pub mod install;
pub mod platforms;
pub mod registry_proxy;
pub mod releases;
pub mod sync;
pub mod telemetry;
pub mod tokens;
pub mod vault;
pub mod whoami;

mod auth;

use std::collections::HashMap;
use std::sync::Arc;

use alien_bindings::traits::Kv;
use alien_bindings::BindingsProviderApi;
use alien_commands::server::{CommandServer, HasCommandServer};
use alien_core::Platform;
use axum::{
    routing::{get, post},
    Router,
};
use http::{header, Method};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::auth::Authz;
use crate::traits::*;

/// Shared state for all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub deployment_store: Arc<dyn DeploymentStore>,
    pub release_store: Arc<dyn ReleaseStore>,
    pub token_store: Arc<dyn TokenStore>,
    pub auth_validator: Arc<dyn AuthValidator>,
    /// Authorization policy. Default: [`crate::providers::OssAuthz`].
    /// Handlers receive a unified [`crate::auth::Subject`] from
    /// `auth_validator` and call `authz.can_*(&subject, &entity)` for every
    /// authorization decision.
    pub authz: Arc<dyn Authz>,
    pub telemetry_backend: Arc<dyn TelemetryBackend>,
    pub credential_resolver: Arc<dyn CredentialResolver>,
    pub command_server: Arc<CommandServer>,
    pub config: Arc<crate::config::ManagerConfig>,
    pub bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
    pub target_bindings_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    /// General-purpose KV store for manager operational data.
    pub kv: Arc<dyn Kv>,
    /// Shared HTTP client for upstream registry requests (connection pooling).
    pub http_client: reqwest::Client,
    /// Cache for upstream registry credentials (avoids per-request generation).
    pub credential_cache: Arc<registry_proxy::CredentialCache>,
    /// Cache for pull validation (deployment → release → repo names).
    pub pull_validation_cache: Arc<registry_proxy::PullValidationCache>,
    /// Routing table mapping repo path prefixes to upstream registries.
    pub registry_routing_table: Arc<registry_proxy::RegistryRoutingTable>,
}

impl HasCommandServer for AppState {
    fn command_server(&self) -> &Arc<CommandServer> {
        &self.command_server
    }
}

/// Create the complete router with all routes (standalone mode).
pub fn create_router(state: AppState) -> Router {
    let cors = cors_layer(&state.config);
    create_router_inner(
        state,
        RouterOptions {
            include_initialize: true,
            include_install: true,
        },
    )
    .layer(cors)
}

/// Route inclusion options for embedding alien-manager in another process.
pub struct RouterOptions {
    pub include_initialize: bool,
    pub include_install: bool,
}

/// Like [`create_router`], but lets the caller opt-out of specific routes.
///
/// Use `RouterOptions` to control which routes are included when embedding alien-manager
/// in a process that overrides certain routes via `extra_routes`.
pub fn create_router_inner(state: AppState, options: RouterOptions) -> Router {
    let mut router = Router::new()
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
        // Commands (authenticated handlers defined in routes/commands.rs)
        .merge(commands::router())
        // Telemetry
        .route("/v1/logs", post(telemetry::ingest_logs))
        .route("/v1/traces", post(telemetry::ingest_traces))
        .route("/v1/metrics", post(telemetry::ingest_metrics))
        // Sync (acquire / reconcile / release / agent-sync)
        .merge(sync::router())
        // Credentials
        .merge(credentials::router())
        // Vault secrets
        .merge(vault::router())
        // Token management (list, revoke)
        .merge(tokens::router())
        // OCI Distribution registry proxy for pull-model image delivery.
        // /v2/ is reserved by OCI spec — next manager API version will be /v3/.
        .merge(registry_proxy::router())
        // Build configuration (repo prefix discovery for CLI).
        .merge(build_config::router())
        // Configured platforms discovery.
        .merge(platforms::router());

    if options.include_install {
        router = router.merge(install::router());
    }
    if options.include_initialize {
        router = router.merge(sync::initialize_router());
    }

    router.with_state(state)
}

/// Build a CORS layer from the manager config.
/// Applied after all routes (including platform routes) are merged.
pub fn cors_layer(config: &crate::config::ManagerConfig) -> CorsLayer {
    let allowed_origins = config
        .allowed_origins
        .clone()
        .unwrap_or_else(|| vec![config.base_url()]);
    let origins: Vec<http::HeaderValue> = allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::HEAD,
            Method::PATCH,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
}
