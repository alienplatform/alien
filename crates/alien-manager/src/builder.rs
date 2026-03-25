//! Builder for constructing an `AlienManager` with customizable providers.

use std::sync::Arc;

use alien_error::AlienError;
use tracing::info;

use crate::config::ManagerConfig;
use crate::error::ErrorData;
use crate::server::AlienManager;
use crate::traits::*;

/// Holds all optional provider overrides extracted from the builder.
struct BuilderOverrides {
    deployment_store: Option<Arc<dyn DeploymentStore>>,
    release_store: Option<Arc<dyn ReleaseStore>>,
    token_store: Option<Arc<dyn TokenStore>>,
    credential_resolver: Option<Arc<dyn CredentialResolver>>,
    telemetry_backend: Option<Arc<dyn TelemetryBackend>>,
    auth_validator: Option<Arc<dyn AuthValidator>>,
    server_bindings: Option<ServerBindings>,
    extra_routes: Option<axum::Router<crate::routes::AppState>>,
    dev_status_tx: Option<tokio::sync::watch::Sender<()>>,
    skip_initialize: bool,
    skip_deploy_page: bool,
    skip_install: bool,
}

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
    dev_status_tx: Option<tokio::sync::watch::Sender<()>>,
    /// When `true`, the default `/v1/initialize` route is omitted from the router.
    /// Use this when embedding in a process that overrides initialize via `extra_routes`.
    skip_initialize: bool,
    /// When `true`, the deploy page (`/deploy`) is omitted from the router.
    skip_deploy_page: bool,
    /// When `true`, the install script (`/v1/install`) is omitted from the router.
    skip_install: bool,
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
            dev_status_tx: None,
            skip_initialize: false,
            skip_deploy_page: false,
            skip_install: false,
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

    pub fn dev_status(mut self, tx: tokio::sync::watch::Sender<()>) -> Self {
        self.dev_status_tx = Some(tx);
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

    /// Build the server, creating default providers based on the configured `ManagerMode`.
    ///
    /// - **Platform mode**: bootstraps identity via whoami, builds platform API providers,
    ///   includes platform routes, and spawns the self-heartbeat loop.
    /// - **Standalone / Dev mode**: uses SQLite-backed stores, local KV/storage, and
    ///   environment-based credentials.
    ///
    /// Explicit provider overrides (set via builder methods) always win over defaults.
    pub async fn build(self) -> crate::error::Result<AlienManager> {
        let config = Arc::new(self.config);
        let overrides = BuilderOverrides {
            deployment_store: self.deployment_store,
            release_store: self.release_store,
            token_store: self.token_store,
            credential_resolver: self.credential_resolver,
            telemetry_backend: self.telemetry_backend,
            auth_validator: self.auth_validator,
            server_bindings: self.server_bindings,
            extra_routes: self.extra_routes,
            dev_status_tx: self.dev_status_tx,
            skip_initialize: self.skip_initialize,
            skip_deploy_page: self.skip_deploy_page,
            skip_install: self.skip_install,
        };

        if config.mode.is_platform() {
            #[cfg(feature = "platform")]
            {
                return build_platform(config, overrides).await;
            }
            #[cfg(not(feature = "platform"))]
            {
                return Err(AlienError::new(ErrorData::ServerInitFailed {
                    reason: "Platform mode requires the 'platform' feature".to_string(),
                }));
            }
        }

        #[cfg(feature = "sqlite")]
        {
            build_sqlite(config, overrides).await
        }
        #[cfg(not(feature = "sqlite"))]
        {
            build_explicit(config, overrides).await
        }
    }
}

// ---------------------------------------------------------------------------
// Finalization: shared assembly logic for all build paths
// ---------------------------------------------------------------------------

/// Assembles an `AlienManager` from resolved providers. All three build paths
/// (sqlite, platform, explicit) resolve providers then delegate here.
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

    // --- Dev mode: ensure default deployment group exists ---
    if config.dev_mode() {
        let groups = deployment_store
            .list_deployment_groups()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::ServerInitFailed {
                    reason: format!("Failed to list deployment groups: {}", e),
                })
            })?;
        if groups.is_empty() {
            info!("Dev mode: creating default 'local-dev' deployment group");
            deployment_store
                .create_deployment_group_with_id(
                    "local-dev",
                    CreateDeploymentGroupParams {
                        name: "local-dev".to_string(),
                        max_deployments: 100,
                    },
                )
                .await
                .map_err(|e| {
                    AlienError::new(ErrorData::ServerInitFailed {
                        reason: format!("Failed to create default deployment group: {}", e),
                    })
                })?;
        }
    }

    info!(
        port = config.port,
        dev_mode = config.dev_mode(),
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
// Build paths
// ---------------------------------------------------------------------------

/// Build with SQLite defaults for Standalone/Dev modes.
#[cfg(feature = "sqlite")]
async fn build_sqlite(config: Arc<ManagerConfig>, overrides: BuilderOverrides) -> crate::error::Result<AlienManager> {
    use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
    use alien_commands::server::NullCommandDispatcher;

    // --- SQLite database (shared by all default stores) ---
    let db_path = config
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

    // --- Stores ---
    let deployment_store: Arc<dyn DeploymentStore> =
        overrides.deployment_store.unwrap_or_else(|| {
            Arc::new(crate::stores::sqlite::SqliteDeploymentStore::new(
                db.clone(),
            ))
        });

    let release_store: Arc<dyn ReleaseStore> = overrides.release_store.unwrap_or_else(|| {
        Arc::new(crate::stores::sqlite::SqliteReleaseStore::new(db.clone()))
    });

    let token_store: Arc<dyn TokenStore> = overrides
        .token_store
        .unwrap_or_else(|| Arc::new(crate::stores::sqlite::SqliteTokenStore::new(db.clone())));

    // --- Providers ---
    let credential_resolver: Arc<dyn CredentialResolver> = overrides
        .credential_resolver
        .unwrap_or_else(|| {
            if config.dev_mode() {
                let state_dir = config
                    .state_dir
                    .clone()
                    .unwrap_or_else(|| std::path::PathBuf::from(".alien-manager"));
                Arc::new(crate::providers::composite_credentials::CompositeCredentialResolver::new(
                    state_dir,
                ))
            } else {
                Arc::new(crate::providers::environment_credentials::EnvironmentCredentialResolver::new())
            }
        });

    let log_buffer = Arc::new(crate::dev::LogBuffer::new());

    let telemetry_backend: Arc<dyn TelemetryBackend> =
        overrides.telemetry_backend.unwrap_or_else(|| {
            if let Some(ref endpoint) = config.otlp_endpoint {
                Arc::new(
                    crate::providers::otlp_forwarding::OtlpForwardingBackend::new(
                        endpoint.clone(),
                    ),
                )
            } else {
                Arc::new(
                    crate::providers::in_memory_telemetry::InMemoryTelemetryBackend::new(
                        log_buffer.clone(),
                    ),
                )
            }
        });

    let auth_validator: Arc<dyn AuthValidator> = overrides.auth_validator.unwrap_or_else(|| {
        if config.dev_mode() {
            Arc::new(crate::providers::permissive_auth::PermissiveAuthValidator::new())
        } else {
            Arc::new(crate::providers::token_db_validator::TokenDbValidator::new(
                token_store.clone(),
            ))
        }
    });

    // --- ServerBindings (command server plumbing) ---
    let server_bindings = if let Some(bindings) = overrides.server_bindings {
        Arc::new(bindings)
    } else {
        let state_dir = config
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

        let command_storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
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
        );

        let command_registry: Arc<dyn alien_commands::server::CommandRegistry> = Arc::new(
            crate::stores::sqlite::SqliteCommandRegistry::new(db.clone()),
        );

        Arc::new(ServerBindings {
            command_kv,
            command_storage,
            command_dispatcher: Arc::new(NullCommandDispatcher),
            command_registry,
            artifact_registry: None,
            bindings_provider: None,
        })
    };

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
            include_initialize: !overrides.skip_initialize,
            include_deploy_page: !overrides.skip_deploy_page,
            include_install: !overrides.skip_install,
        },
        overrides.extra_routes,
        None,
        overrides.dev_status_tx,
    )
    .await
}

/// Build with Platform API providers.
#[cfg(feature = "platform")]
async fn build_platform(config: Arc<ManagerConfig>, overrides: BuilderOverrides) -> crate::error::Result<AlienManager> {
    use std::collections::HashMap;
    use alien_bindings::{BindingsProvider, BindingsProviderApi};
    use alien_core::Platform;
    use tracing::warn;

    use crate::providers::platform_api::{
        PlatformApiDeploymentStore,
        PlatformApiReleaseStore,
        NullTokenStore,
        PlatformTokenValidator,
        ImpersonationCredentialResolver,
        DeepStoreTelemetryBackend,
        ManagedCommandDispatcher,
        PlatformCommandRegistry,
        PlatformState,
        extension::{build_platform_client, resolve_base_url},
    };

    let pc = config.mode.platform_config().unwrap();

    // --- Bootstrap manager identity via whoami ---
    let identity = bootstrap_manager_identity(&pc.api_url, &pc.api_key).await?;
    info!(
        manager_id = %identity.manager_id,
        workspace_name = %identity.workspace_name,
        "Identity resolved from MANAGER_API_KEY via whoami"
    );

    // --- Build bindings providers ---
    let env: HashMap<String, String> = std::env::vars().collect();

    // Determine if we're running as an Alien app (has runtime bindings) or standalone
    let is_alien_app = std::env::var("ALIEN_CURRENT_CONTAINER_BINDING_NAME").is_ok();

    let (bindings, target_bindings): (
        Arc<dyn BindingsProviderApi>,
        HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    ) = if is_alien_app {
        info!("Alien App mode: using bindings from Alien runtime environment");
        let provider = Arc::new(
            BindingsProvider::from_env(env.clone())
                .await
                .map_err(|e| {
                    AlienError::new(ErrorData::ServerInitFailed {
                        reason: format!("Failed to initialize bindings provider: {}", e),
                    })
                })?,
        );
        (provider as Arc<dyn BindingsProviderApi>, HashMap::new())
    } else {
        info!(
            primary_platform = %pc.primary_platform,
            "Standalone mode: building multi-cloud providers"
        );
        build_standalone_providers(pc.primary_platform, &env).await?
    };

    // --- Resolve base URL ---
    let base_url = resolve_base_url(&config.base_url, config.port, &bindings)
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ServerInitFailed {
                reason: format!("Failed to resolve base URL: {}", e),
            })
        })?;

    // --- Build Platform API client ---
    let platform_client = build_platform_client(&pc.api_url, &pc.api_key)
        .map_err(|e| {
            AlienError::new(ErrorData::ServerInitFailed {
                reason: format!("Failed to build platform client: {}", e),
            })
        })?;

    // --- Build PlatformState ---
    let ext = Arc::new(PlatformState {
        api_url: pc.api_url.clone(),
        manager_id: identity.manager_id.clone(),
        base_url: base_url.clone(),
        client: platform_client.clone(),
        bindings: bindings.clone(),
        target_bindings: target_bindings.clone(),
        heartbeat_interval_secs: config.self_heartbeat_interval_secs,
        deepstore: pc.deepstore.clone(),
        gcp_oauth: pc.gcp_oauth.clone(),
    });

    // --- Spawn self-heartbeat as a detached task ---
    {
        let ext_clone = ext.clone();
        tokio::spawn(async move {
            if let Err(e) =
                crate::loops::self_heartbeat::run_self_heartbeat_loop(ext_clone).await
            {
                tracing::error!(error = %e, "Self-heartbeat loop failed");
            }
        });
    }

    // --- Build providers (explicit overrides win) ---
    let deployment_store: Arc<dyn DeploymentStore> = overrides.deployment_store.unwrap_or_else(|| {
        Arc::new(PlatformApiDeploymentStore::new(
            platform_client.clone(),
            identity.manager_id.clone(),
        ))
    });

    let release_store: Arc<dyn ReleaseStore> = overrides.release_store.unwrap_or_else(|| {
        Arc::new(PlatformApiReleaseStore::new(platform_client.clone()))
    });

    let token_store: Arc<dyn TokenStore> = overrides.token_store.unwrap_or_else(|| {
        Arc::new(NullTokenStore)
    });

    let credential_resolver: Arc<dyn CredentialResolver> = overrides.credential_resolver.unwrap_or_else(|| {
        Arc::new(ImpersonationCredentialResolver::new(
            bindings.clone(),
            target_bindings.clone(),
        ))
    });

    let telemetry_backend: Arc<dyn TelemetryBackend> = overrides.telemetry_backend.unwrap_or_else(|| {
        if let (Some(otlp_url), Some(database_id)) = (
            pc.deepstore.otlp_url.clone(),
            pc.deepstore.database_id.clone(),
        ) {
            Arc::new(DeepStoreTelemetryBackend::new(
                otlp_url,
                database_id,
                identity.workspace_name.clone(),
                platform_client.clone(),
            ))
        } else {
            warn!("DEEPSTORE_OTLP_URL or DEEPSTORE_DATABASE_ID not set — telemetry will be discarded");
            Arc::new(crate::providers::NullTelemetryBackend)
        }
    });

    let auth_validator: Arc<dyn AuthValidator> = overrides.auth_validator.unwrap_or_else(|| {
        Arc::new(PlatformTokenValidator::new(pc.api_url.clone()))
    });

    // --- ServerBindings ---
    let server_bindings = if let Some(sb) = overrides.server_bindings {
        Arc::new(sb)
    } else {
        let command_kv = bindings
            .load_kv("command-kv")
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::ServerInitFailed {
                    reason: format!("Failed to load command-kv binding: {}", e),
                })
            })?;
        let command_storage = bindings
            .load_storage("command-storage")
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::ServerInitFailed {
                    reason: format!("Failed to load command-storage binding: {}", e),
                })
            })?;

        let command_dispatcher: Arc<dyn alien_commands::server::CommandDispatcher> = Arc::new(
            ManagedCommandDispatcher::new(
                &pc.api_url,
                &pc.api_key,
                bindings.clone(),
                target_bindings.clone(),
            )
            .map_err(|e| {
                AlienError::new(ErrorData::ServerInitFailed {
                    reason: format!("Failed to create command dispatcher: {}", e),
                })
            })?,
        );

        let command_registry: Arc<dyn alien_commands::server::CommandRegistry> = Arc::new(
            PlatformCommandRegistry::new(&pc.api_url, &pc.api_key)
                .map_err(|e| {
                    AlienError::new(ErrorData::ServerInitFailed {
                        reason: format!("Failed to create command registry: {}", e),
                    })
                })?,
        );

        Arc::new(ServerBindings {
            command_kv,
            command_storage,
            command_dispatcher,
            command_registry,
            artifact_registry: None,
            bindings_provider: None,
        })
    };

    let log_buffer = Arc::new(crate::dev::LogBuffer::new());

    // Platform-specific routes
    let platform_routes = Some(crate::routes::platform::build_platform_routes(ext));

    info!(
        port = config.port,
        manager_id = %identity.manager_id,
        base_url = %base_url,
        "AlienManager built (platform mode)"
    );

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
            include_initialize: false,
            include_deploy_page: false,
            include_install: false,
        },
        overrides.extra_routes,
        platform_routes,
        overrides.dev_status_tx,
    )
    .await
}

/// Build with explicitly-provided providers (no defaults).
///
/// All providers and `server_bindings` must be set before calling this method.
#[cfg(not(feature = "sqlite"))]
async fn build_explicit(config: Arc<ManagerConfig>, overrides: BuilderOverrides) -> crate::error::Result<AlienManager> {
    macro_rules! require_provider {
        ($field:expr, $name:literal) => {
            $field.ok_or_else(|| {
                AlienError::new(ErrorData::ServerInitFailed {
                    reason: format!(
                        "{} must be provided when building without sqlite defaults",
                        $name
                    ),
                })
            })?
        };
    }

    let deployment_store = require_provider!(overrides.deployment_store, "deployment_store");
    let release_store = require_provider!(overrides.release_store, "release_store");
    let token_store = require_provider!(overrides.token_store, "token_store");
    let credential_resolver =
        require_provider!(overrides.credential_resolver, "credential_resolver");
    let telemetry_backend = require_provider!(overrides.telemetry_backend, "telemetry_backend");
    let auth_validator = require_provider!(overrides.auth_validator, "auth_validator");
    let server_bindings =
        Arc::new(require_provider!(overrides.server_bindings, "server_bindings"));

    finalize(
        config,
        deployment_store,
        release_store,
        token_store,
        auth_validator,
        telemetry_backend,
        credential_resolver,
        server_bindings,
        Arc::new(crate::dev::LogBuffer::new()),
        crate::routes::RouterOptions {
            include_initialize: !overrides.skip_initialize,
            include_deploy_page: !overrides.skip_deploy_page,
            include_install: !overrides.skip_install,
        },
        overrides.extra_routes,
        None,
        overrides.dev_status_tx,
    )
    .await
}

// --- Platform bootstrapping helpers ---

#[cfg(feature = "platform")]
struct ManagerIdentity {
    workspace_name: String,
    manager_id: String,
}

#[cfg(feature = "platform")]
async fn bootstrap_manager_identity(
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

#[cfg(feature = "platform")]
async fn build_standalone_providers(
    primary_platform: alien_core::Platform,
    env: &std::collections::HashMap<String, String>,
) -> crate::error::Result<(
    std::sync::Arc<dyn alien_bindings::BindingsProviderApi>,
    std::collections::HashMap<alien_core::Platform, std::sync::Arc<dyn alien_bindings::BindingsProviderApi>>,
)> {
    use std::collections::HashMap;
    use alien_bindings::{BindingsProvider, BindingsProviderApi};
    use alien_client_config::ClientConfigExt;
    use alien_core::Platform;
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

    Ok((primary_provider as Arc<dyn BindingsProviderApi>, target_providers))
}

#[cfg(feature = "platform")]
fn parse_standard_bindings(
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

#[cfg(feature = "platform")]
fn parse_target_bindings(
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
