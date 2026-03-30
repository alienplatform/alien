//! Builder for constructing an `AlienManager` with customizable providers.

use std::sync::Arc;

use alien_error::AlienError;
use tracing::info;

use crate::config::ManagerConfig;
use crate::error::ErrorData;
use crate::server::AlienManager;
use crate::traits::*;

pub struct AlienManagerBuilder {
    config: ManagerConfig,
    deployment_store: Option<Arc<dyn DeploymentStore>>,
    release_store: Option<Arc<dyn ReleaseStore>>,
    token_store: Option<Arc<dyn TokenStore>>,
    credential_resolver: Option<Arc<dyn CredentialResolver>>,
    telemetry_backend: Option<Arc<dyn TelemetryBackend>>,
    auth_validator: Option<Arc<dyn AuthValidator>>,
    server_bindings: Option<ServerBindings>,
    extra_routes: Option<axum::Router<crate::routes::AppState>>,
    platform_routes: Option<axum::Router<crate::routes::AppState>>,
    dev_status_tx: Option<tokio::sync::watch::Sender<()>>,
    log_buffer: Option<Arc<crate::dev::LogBuffer>>,
    /// When `true`, the default `/v1/initialize` route is omitted from the router.
    /// Use this when embedding in a process that overrides initialize via `extra_routes`.
    skip_initialize: bool,
    /// When `true`, the deploy page (`/deploy`) is omitted from the router.
    skip_deploy_page: bool,
    /// When `true`, the install script (`/v1/install`) is omitted from the router.
    skip_install: bool,
    /// Override the command storage backend used by `with_standalone_defaults()`.
    /// When set, this replaces the default `LocalStorage` — useful for tests that
    /// need push-mode runtimes (Lambda/Cloud Run) to access presigned URLs.
    command_storage_override: Option<Arc<dyn alien_bindings::traits::Storage>>,
}

impl AlienManagerBuilder {
    pub fn new(config: ManagerConfig) -> Self {
        Self {
            config,
            deployment_store: None,
            release_store: None,
            token_store: None,
            credential_resolver: None,
            telemetry_backend: None,
            auth_validator: None,
            server_bindings: None,
            extra_routes: None,
            platform_routes: None,
            dev_status_tx: None,
            log_buffer: None,
            skip_initialize: false,
            skip_deploy_page: false,
            skip_install: false,
            command_storage_override: None,
        }
    }

    pub fn deployment_store(mut self, store: Arc<dyn DeploymentStore>) -> Self {
        self.deployment_store = Some(store);
        self
    }

    pub fn release_store(mut self, store: Arc<dyn ReleaseStore>) -> Self {
        self.release_store = Some(store);
        self
    }

    pub fn token_store(mut self, store: Arc<dyn TokenStore>) -> Self {
        self.token_store = Some(store);
        self
    }

    pub fn credential_resolver(mut self, resolver: Arc<dyn CredentialResolver>) -> Self {
        self.credential_resolver = Some(resolver);
        self
    }

    pub fn telemetry_backend(mut self, backend: Arc<dyn TelemetryBackend>) -> Self {
        self.telemetry_backend = Some(backend);
        self
    }

    pub fn auth_validator(mut self, validator: Arc<dyn AuthValidator>) -> Self {
        self.auth_validator = Some(validator);
        self
    }

    pub fn server_bindings(mut self, bindings: ServerBindings) -> Self {
        self.server_bindings = Some(bindings);
        self
    }

    pub fn extra_routes(mut self, routes: axum::Router<crate::routes::AppState>) -> Self {
        self.extra_routes = Some(routes);
        self
    }

    /// Add platform-specific routes that are merged into the main router.
    pub fn platform_routes(mut self, routes: axum::Router<crate::routes::AppState>) -> Self {
        self.platform_routes = Some(routes);
        self
    }

    pub fn dev_status(mut self, tx: tokio::sync::watch::Sender<()>) -> Self {
        self.dev_status_tx = Some(tx);
        self
    }

    /// Provide a custom log buffer. If not set, a new empty one is created.
    pub fn log_buffer(mut self, buffer: Arc<crate::dev::LogBuffer>) -> Self {
        self.log_buffer = Some(buffer);
        self
    }

    /// Skip the default `/v1/initialize` route so a custom one can be provided via `extra_routes`.
    pub fn skip_initialize(mut self) -> Self {
        self.skip_initialize = true;
        self
    }

    /// Skip the deploy page (`/deploy`) route.
    /// Use this when the platform provides its own deploy page (e.g., a dashboard).
    pub fn skip_deploy_page(mut self) -> Self {
        self.skip_deploy_page = true;
        self
    }

    /// Skip the install script (`/v1/install`) route.
    /// Use this when binaries are distributed through a separate packages service.
    pub fn skip_install(mut self) -> Self {
        self.skip_install = true;
        self
    }

    /// Set up SQLite-backed standalone providers for stores and bindings.
    ///
    /// This is a convenience method that creates SQLite implementations for
    /// `DeploymentStore`, `ReleaseStore`, `TokenStore`, `CommandRegistry`,
    /// and local-filesystem `ServerBindings`. Already-set providers are NOT
    /// overridden — explicit `.deployment_store(...)` etc. calls always win.
    #[cfg(feature = "sqlite")]
    pub async fn with_standalone_defaults(mut self) -> crate::error::Result<Self> {
        use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};

        // --- SQLite database (shared by all default stores) ---
        let db_path = self
            .config
            .db_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "alien-manager.db".to_string());
        let db = Arc::new(
            crate::stores::sqlite::SqliteDatabase::new(&db_path)
                .await
                .map_err(|e| {
                    AlienError::new(ErrorData::ServerInitFailed {
                        reason: format!("Failed to initialize database: {}", e),
                    })
                })?,
        );

        // --- Stores (only set if not already provided) ---
        if self.deployment_store.is_none() {
            self.deployment_store = Some(Arc::new(
                crate::stores::sqlite::SqliteDeploymentStore::new(db.clone()),
            ));
        }

        if self.release_store.is_none() {
            self.release_store = Some(Arc::new(crate::stores::sqlite::SqliteReleaseStore::new(
                db.clone(),
            )));
        }

        if self.token_store.is_none() {
            self.token_store = Some(Arc::new(crate::stores::sqlite::SqliteTokenStore::new(
                db.clone(),
            )));
        }

        // --- Credential resolver ---
        if self.credential_resolver.is_none() {
            let env: std::collections::HashMap<String, String> = std::env::vars().collect();
            let has_management_binding = env.keys().any(|k| k.contains("MANAGEMENT_BINDING"));

            #[cfg(feature = "platform")]
            if has_management_binding {
                // Cross-account mode: use ImpersonationCredentialResolver with
                // SA impersonation for short-lived credentials.
                let primary_platform = self
                    .config
                    .targets
                    .first()
                    .copied()
                    .unwrap_or(alien_core::Platform::Aws);
                let (primary, targets) = build_standalone_providers(primary_platform, &env).await?;

                // If ALIEN_COMMAND_STORAGE_BINDING is configured, resolve it from
                // the primary provider so push-mode runtimes (Lambda, Cloud Run)
                // get real presigned HTTP URLs instead of local filesystem paths.
                // parse_standard_bindings() already mapped it to "command-storage".
                if self.command_storage_override.is_none() {
                    if let Ok(storage) = primary.load_storage("command-storage").await {
                        info!("Resolved command storage from ALIEN_COMMAND_STORAGE_BINDING");
                        self.command_storage_override = Some(storage);
                    }
                }

                self.credential_resolver = Some(Arc::new(
                    crate::providers::impersonation_credentials::ImpersonationCredentialResolver::new(
                        primary, targets,
                    ),
                ));
                info!("Cross-account mode: using ImpersonationCredentialResolver");
            }

            if self.credential_resolver.is_none() {
                // Simple mode: use environment credentials directly.
                let _ = has_management_binding; // suppress unused warning when platform disabled
                self.credential_resolver = Some(Arc::new(
                    crate::providers::environment_credentials::EnvironmentCredentialResolver::new(),
                ));
            }
        }

        // --- Telemetry backend ---
        if self.telemetry_backend.is_none() {
            self.telemetry_backend = Some(if let Some(ref endpoint) = self.config.otlp_endpoint {
                Arc::new(
                    crate::providers::otlp_forwarding::OtlpForwardingBackend::new(endpoint.clone()),
                )
            } else {
                Arc::new(crate::providers::NullTelemetryBackend)
            });
        }

        // --- Auth validator ---
        if self.auth_validator.is_none() {
            self.auth_validator = Some(Arc::new(
                crate::providers::token_db_validator::TokenDbValidator::new(
                    self.token_store.clone().unwrap(),
                ),
            ));
        }

        // --- ServerBindings (command server plumbing) ---
        if self.server_bindings.is_none() {
            let state_dir = self
                .config
                .state_dir
                .as_ref()
                .cloned()
                .unwrap_or_else(|| std::path::PathBuf::from(".alien-manager"));
            let kv_path = state_dir.join("commands_kv");
            let storage_path = state_dir.join("commands_storage");

            let command_kv: Arc<dyn alien_bindings::traits::Kv> =
                Arc::new(LocalKv::new(kv_path).await.map_err(|e| {
                    AlienError::new(ErrorData::ServerInitFailed {
                        reason: format!("Failed to create command KV store: {}", e),
                    })
                })?);

            let command_storage: Arc<dyn alien_bindings::traits::Storage> =
                if let Some(storage) = self.command_storage_override.take() {
                    storage
                } else {
                    Arc::new(
                        LocalStorage::new(
                            storage_path
                                .to_str()
                                .unwrap_or("commands_storage")
                                .to_string(),
                        )
                        .map_err(|e| {
                            AlienError::new(ErrorData::ServerInitFailed {
                                reason: format!("Failed to create command storage: {}", e),
                            })
                        })?,
                    )
                };

            let command_registry: Arc<dyn alien_commands::server::CommandRegistry> =
                Arc::new(crate::stores::sqlite::SqliteCommandRegistry::new(
                    db.clone(),
                    self.deployment_store.clone().unwrap(),
                ));

            let command_dispatcher: Arc<dyn alien_commands::server::CommandDispatcher> =
                Arc::new(crate::commands::DefaultCommandDispatcher::new(
                    self.deployment_store.clone().unwrap(),
                    self.release_store.clone().unwrap(),
                    self.credential_resolver.clone().unwrap(),
                ));

            self.server_bindings = Some(ServerBindings {
                command_kv,
                command_storage,
                command_dispatcher,
                command_registry,
                artifact_registry: None,
                bindings_provider: None,
            });
        }

        Ok(self)
    }

    /// Build the `AlienManager` from explicitly-provided providers.
    ///
    /// All required providers must be set (either directly via builder methods
    /// or via a convenience method like `with_standalone_defaults()`). Missing
    /// providers produce a clear error.
    pub async fn build(self) -> crate::error::Result<AlienManager> {
        macro_rules! require_provider {
            ($field:expr, $name:literal) => {
                $field.ok_or_else(|| {
                    AlienError::new(ErrorData::ServerInitFailed {
                        reason: format!(
                            "{} must be provided (set it explicitly or call with_standalone_defaults())",
                            $name
                        ),
                    })
                })?
            };
        }

        let deployment_store = require_provider!(self.deployment_store, "deployment_store");
        let release_store = require_provider!(self.release_store, "release_store");
        let token_store = require_provider!(self.token_store, "token_store");
        let credential_resolver =
            require_provider!(self.credential_resolver, "credential_resolver");
        let telemetry_backend = require_provider!(self.telemetry_backend, "telemetry_backend");
        let auth_validator = require_provider!(self.auth_validator, "auth_validator");
        let server_bindings = Arc::new(require_provider!(self.server_bindings, "server_bindings"));
        let log_buffer = self
            .log_buffer
            .unwrap_or_else(|| Arc::new(crate::dev::LogBuffer::new()));

        let config = Arc::new(self.config);

        finalize(
            config,
            deployment_store,
            release_store,
            token_store,
            auth_validator,
            telemetry_backend,
            credential_resolver,
            server_bindings,
            log_buffer,
            crate::routes::RouterOptions {
                include_initialize: !self.skip_initialize,
                include_deploy_page: !self.skip_deploy_page,
                include_install: !self.skip_install,
            },
            self.extra_routes,
            self.platform_routes,
            self.dev_status_tx,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Finalization: shared assembly logic
// ---------------------------------------------------------------------------

/// Assembles an `AlienManager` from resolved providers.
async fn finalize(
    config: Arc<ManagerConfig>,
    deployment_store: Arc<dyn DeploymentStore>,
    release_store: Arc<dyn ReleaseStore>,
    token_store: Arc<dyn TokenStore>,
    auth_validator: Arc<dyn AuthValidator>,
    telemetry_backend: Arc<dyn TelemetryBackend>,
    credential_resolver: Arc<dyn CredentialResolver>,
    server_bindings: Arc<ServerBindings>,
    log_buffer: Arc<crate::dev::LogBuffer>,
    router_options: crate::routes::RouterOptions,
    extra_routes: Option<axum::Router<crate::routes::AppState>>,
    platform_routes: Option<axum::Router<crate::routes::AppState>>,
    dev_status_tx: Option<tokio::sync::watch::Sender<()>>,
) -> crate::error::Result<AlienManager> {
    use alien_commands::server::CommandServer;

    // --- CommandServer ---
    let command_server = Arc::new(CommandServer::new(
        server_bindings.command_kv.clone(),
        server_bindings.command_storage.clone(),
        server_bindings.command_dispatcher.clone(),
        server_bindings.command_registry.clone(),
        config.commands_base_url(),
    ));

    // --- AppState ---
    let app_state = crate::routes::AppState {
        deployment_store: deployment_store.clone(),
        release_store: release_store.clone(),
        token_store: token_store.clone(),
        auth_validator: auth_validator.clone(),
        telemetry_backend: telemetry_backend.clone(),
        credential_resolver: credential_resolver.clone(),
        command_server,
        config: config.clone(),
    };

    // --- Router ---
    let mut router = crate::routes::create_router_inner(app_state.clone(), router_options);
    if let Some(platform) = platform_routes {
        router = router.merge(platform.with_state(app_state.clone()));
    }
    if let Some(extra) = extra_routes {
        router = router.merge(extra.with_state(app_state));
    }

    info!(
        port = config.port,
        enable_local_log_ingest = config.enable_local_log_ingest(),
        "AlienManager built"
    );

    Ok(AlienManager {
        config,
        router,
        deployment_store,
        release_store,
        credential_resolver,
        telemetry_backend,
        server_bindings,
        dev_status_tx,
        log_buffer,
    })
}

// ---------------------------------------------------------------------------
// Platform bootstrapping helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "platform")]
pub struct ManagerIdentity {
    pub workspace_name: String,
    pub manager_id: String,
}

#[cfg(feature = "platform")]
pub async fn bootstrap_manager_identity(
    api_base_url: &str,
    manager_api_key: &str,
) -> crate::error::Result<ManagerIdentity> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/whoami", api_base_url);

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", manager_api_key))
        .send()
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ServerInitFailed {
                reason: format!("Failed to call whoami to resolve manager identity: {}", e),
            })
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ServerInitFailed {
            reason: format!(
                "whoami returned {}: {}. Check MANAGER_API_KEY and ALIEN_API_URL.",
                status, body
            ),
        }));
    }

    let body: serde_json::Value = response.json().await.map_err(|e| {
        AlienError::new(ErrorData::ServerInitFailed {
            reason: format!("Failed to parse whoami response: {}", e),
        })
    })?;

    let kind = body
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if kind != "serviceAccount" {
        return Err(AlienError::new(ErrorData::ServerInitFailed {
            reason: format!(
                "MANAGER_API_KEY must authenticate as a service account with manager scope (whoami kind={kind:?}). \
                 Use a manager-scoped API key, not a workspace or project key."
            ),
        }));
    }

    let scope = body.get("scope").ok_or_else(|| {
        AlienError::new(ErrorData::ServerInitFailed {
            reason: "whoami response missing scope for service account.".to_string(),
        })
    })?;

    let scope_type = scope
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if scope_type != "manager" {
        return Err(AlienError::new(ErrorData::ServerInitFailed {
            reason: format!(
                "MANAGER_API_KEY must have manager scope (whoami scope.type={scope_type:?}). \
                 Create a key of type \"manager\" for this manager."
            ),
        }));
    }

    let manager_id = scope
        .get("managerId")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ServerInitFailed {
                reason: "whoami manager scope missing managerId.".to_string(),
            })
        })?;

    let workspace_name = body
        .get("workspaceName")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ServerInitFailed {
                reason: "whoami response missing workspaceName. Is the API up to date?".to_string(),
            })
        })?;

    Ok(ManagerIdentity {
        workspace_name,
        manager_id,
    })
}

pub async fn build_standalone_providers(
    primary_platform: alien_core::Platform,
    env: &std::collections::HashMap<String, String>,
) -> crate::error::Result<(
    std::sync::Arc<dyn alien_bindings::BindingsProviderApi>,
    std::collections::HashMap<
        alien_core::Platform,
        std::sync::Arc<dyn alien_bindings::BindingsProviderApi>,
    >,
)> {
    use alien_bindings::{BindingsProvider, BindingsProviderApi};
    use alien_client_config::ClientConfigExt;
    use alien_core::Platform;
    use std::collections::HashMap;
    use tracing::warn;

    let primary_config = alien_core::ClientConfig::from_std_env(primary_platform)
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ServerInitFailed {
                reason: format!(
                    "Failed to load {} credentials for primary platform: {}",
                    primary_platform, e
                ),
            })
        })?;

    let primary_bindings = parse_standard_bindings(env);
    info!(
        primary_platform = %primary_platform,
        bindings = ?primary_bindings.keys().collect::<Vec<_>>(),
        "Primary provider configured"
    );

    let primary_provider = Arc::new(
        BindingsProvider::new(primary_config, primary_bindings).map_err(|e| {
            AlienError::new(ErrorData::ServerInitFailed {
                reason: format!("Failed to create primary bindings provider: {}", e),
            })
        })?,
    );

    let cloud_platforms = [Platform::Aws, Platform::Gcp, Platform::Azure];
    let mut target_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>> = HashMap::new();

    for platform in &cloud_platforms {
        let target_bindings = parse_target_bindings(env, *platform);
        if target_bindings.is_empty() {
            continue;
        }

        let target_config = match alien_core::ClientConfig::from_std_env(*platform).await {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    platform = %platform,
                    error = %e,
                    "Target bindings found for {} but credentials not available, skipping",
                    platform
                );
                continue;
            }
        };

        info!(
            platform = %platform,
            bindings = ?target_bindings.keys().collect::<Vec<_>>(),
            "Target provider configured"
        );

        let provider = Arc::new(
            BindingsProvider::new(target_config, target_bindings).map_err(|e| {
                AlienError::new(ErrorData::ServerInitFailed {
                    reason: format!("Failed to create {} target provider: {}", platform, e),
                })
            })?,
        );
        target_providers.insert(*platform, provider as Arc<dyn BindingsProviderApi>);
    }

    if target_providers.is_empty() {
        warn!("No target providers configured — SA impersonation and AR will use primary provider");
    } else {
        info!(
            targets = ?target_providers.keys().collect::<Vec<_>>(),
            "Target providers ready"
        );
    }

    Ok((
        primary_provider as Arc<dyn BindingsProviderApi>,
        target_providers,
    ))
}

pub fn parse_standard_bindings(
    env: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, serde_json::Value> {
    let platform_prefixes = ["ALIEN_AWS_", "ALIEN_GCP_", "ALIEN_AZURE_"];
    let mut bindings = std::collections::HashMap::new();

    for (key, value) in env {
        if !key.starts_with("ALIEN_") || !key.ends_with("_BINDING") {
            continue;
        }
        if platform_prefixes.iter().any(|p| key.starts_with(p)) {
            continue;
        }
        let binding_name = key
            .strip_prefix("ALIEN_")
            .unwrap()
            .strip_suffix("_BINDING")
            .unwrap()
            .to_lowercase()
            .replace('_', "-");

        match serde_json::from_str(value) {
            Ok(parsed) => {
                bindings.insert(binding_name, parsed);
            }
            Err(e) => {
                tracing::warn!(key = %key, error = %e, "Failed to parse binding JSON, skipping");
            }
        }
    }

    bindings
}

pub fn parse_target_bindings(
    env: &std::collections::HashMap<String, String>,
    platform: alien_core::Platform,
) -> std::collections::HashMap<String, serde_json::Value> {
    let prefix = format!("ALIEN_{}_", platform.as_str().to_uppercase());
    let mut bindings = std::collections::HashMap::new();

    for (key, value) in env {
        if !key.starts_with(&prefix) || !key.ends_with("_BINDING") {
            continue;
        }
        let binding_name = key
            .strip_prefix(&prefix)
            .unwrap()
            .strip_suffix("_BINDING")
            .unwrap()
            .to_lowercase()
            .replace('_', "-");

        match serde_json::from_str(value) {
            Ok(parsed) => {
                bindings.insert(binding_name, parsed);
            }
            Err(e) => {
                tracing::warn!(key = %key, error = %e, "Failed to parse target binding JSON, skipping");
            }
        }
    }

    bindings
}
