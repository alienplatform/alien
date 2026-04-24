//! Builder for constructing an `AlienManager` with customizable providers.

use std::sync::Arc;

use alien_error::AlienError;
use tracing::{info, warn};

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
    /// When `true`, the install script (`/v1/install`) is omitted from the router.
    skip_install: bool,
    /// Override the bindings provider for cross-account registry access.
    /// When set in `with_standalone_defaults()`, this is stored in `ServerBindings`
    /// so `reconcile_registry_access()` can load the artifact registry and grant
    /// cross-account ECR/GAR pull permissions.
    bindings_provider_override: Option<Arc<dyn alien_bindings::BindingsProviderApi>>,
    /// Per-target bindings providers, keyed by platform. Each target cloud has
    /// its own artifact registry (ECR/GAR/ACR). Stored in `ServerBindings` so
    /// `reconcile_registry_access()` can look up the correct provider.
    target_bindings_providers_override: Option<
        std::collections::HashMap<
            alien_core::Platform,
            Arc<dyn alien_bindings::BindingsProviderApi>,
        >,
    >,
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
            skip_install: false,
            bindings_provider_override: None,
            target_bindings_providers_override: None,
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

    /// Set the primary bindings provider (for registry proxy, credential resolution, etc.)
    pub fn bindings_provider(
        mut self,
        provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    ) -> Self {
        self.bindings_provider_override = Some(provider);
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

    /// Skip the install script (`/v1/install`) route.
    /// Use this when binaries are distributed through a separate packages service.
    pub fn skip_install(mut self) -> Self {
        self.skip_install = true;
        self
    }

    /// Set up SQLite-backed standalone providers from a TOML config.
    ///
    /// This is the primary way to configure a standalone manager. It creates
    /// SQLite stores, wires credential resolution, telemetry, auth, and
    /// server bindings from the typed `ManagerTomlConfig` fields.
    ///
    /// Already-set providers are NOT overridden — explicit `.deployment_store(...)`
    /// etc. calls always win.
    #[cfg(feature = "sqlite")]
    pub async fn with_standalone_defaults(
        mut self,
        toml_config: &crate::standalone_config::ManagerTomlConfig,
    ) -> crate::error::Result<Self> {
        use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
        use alien_bindings::{BindingsProvider, BindingsProviderApi};
        use alien_client_config::ClientConfigExt;
        use alien_core::Platform;
        use alien_error::{Context, IntoAlienError};
        use std::collections::HashMap;

        // --- SQLite database (shared by all default stores) ---
        let state_dir_for_db = self
            .config
            .state_dir
            .as_ref()
            .cloned()
            .unwrap_or_else(|| std::path::PathBuf::from("alien-data"));
        let db_path = self
            .config
            .db_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                state_dir_for_db
                    .join("manager.db")
                    .to_string_lossy()
                    .into_owned()
            });
        let db = Arc::new(
            crate::stores::sqlite::SqliteDatabase::new(&db_path)
                .await
                .context(ErrorData::ServerInitFailed {
                    reason: format!("Failed to initialize database at '{}'", db_path),
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

        // --- Target bindings providers (artifact registry + impersonation) ---
        // Built early because the credential resolver needs to know whether
        // cross-account impersonation bindings exist.
        let mut target_bindings: HashMap<Platform, Arc<dyn BindingsProviderApi>> = self
            .target_bindings_providers_override
            .take()
            .unwrap_or_default();

        {
            // Collect per-platform bindings from TOML config (artifact registry +
            // impersonation). Each platform gets a single BindingsProvider with
            // all its bindings merged into one provider.
            let ar = &toml_config.artifact_registry;
            let imp = &toml_config.impersonation;

            for (platform, ar_binding, imp_binding) in [
                (Platform::Aws, &ar.aws, &imp.aws),
                (Platform::Gcp, &ar.gcp, &imp.gcp),
                (Platform::Azure, &ar.azure, &imp.azure),
            ] {
                if target_bindings.contains_key(&platform) {
                    continue; // explicit override wins
                }

                let mut bindings = HashMap::new();

                if let Some(ref ar_val) = ar_binding {
                    bindings.insert(
                        "artifacts".to_string(),
                        serde_json::to_value(ar_val)
                            .into_alien_error()
                            .context(ErrorData::ServerInitFailed {
                                reason: format!(
                                    "Failed to serialize {} AR binding",
                                    platform
                                ),
                            })?,
                    );
                }

                if let Some(ref imp_val) = imp_binding {
                    bindings.insert(
                        "management".to_string(),
                        serde_json::to_value(imp_val)
                            .into_alien_error()
                            .context(ErrorData::ServerInitFailed {
                                reason: format!(
                                    "Failed to serialize {} impersonation binding",
                                    platform
                                ),
                            })?,
                    );
                }

                if bindings.is_empty() {
                    continue;
                }

                let config = alien_core::ClientConfig::from_std_env(platform)
                    .await
                    .context(ErrorData::ServerInitFailed {
                        reason: format!(
                            "{platform} artifact registry is configured in alien-manager.toml but {platform} credentials are not available. \
                             Either provide valid {platform} credentials or remove the [artifact-registry.{platform_lower}] section from the config.",
                            platform = platform,
                            platform_lower = format!("{}", platform).to_lowercase(),
                        ),
                    })?;
                let provider = BindingsProvider::new(config, bindings).context(
                    ErrorData::ServerInitFailed {
                        reason: format!(
                            "Failed to create {} target bindings provider",
                            platform
                        ),
                    },
                )?;
                info!(platform = %platform, "Target bindings provider configured from TOML");
                target_bindings
                    .insert(platform, Arc::new(provider) as Arc<dyn BindingsProviderApi>);
            }
        }

        // --- Credential resolver ---
        // When impersonation bindings exist, use ImpersonationCredentialResolver
        // for cross-account SA impersonation. Otherwise fall back to
        // EnvironmentCredentialResolver (direct env creds — for single-account setups
        // where only artifact registry is configured, no impersonation).
        if self.credential_resolver.is_none() {
            let has_impersonation = toml_config.impersonation.aws.is_some()
                || toml_config.impersonation.gcp.is_some()
                || toml_config.impersonation.azure.is_some();

            if has_impersonation && !target_bindings.is_empty() {
                let primary_provider = target_bindings.values().next().unwrap().clone();
                self.credential_resolver = Some(Arc::new(
                    crate::providers::impersonation_credentials::ImpersonationCredentialResolver::new(
                        primary_provider,
                        target_bindings.clone(),
                    ),
                ));
                info!("Cross-account mode: using ImpersonationCredentialResolver");
            } else {
                self.credential_resolver = Some(Arc::new(
                    crate::providers::environment_credentials::EnvironmentCredentialResolver::new(),
                ));
                info!("Single-account mode: using EnvironmentCredentialResolver");
            }
        }

        // --- Telemetry backend ---
        if self.telemetry_backend.is_none() {
            self.telemetry_backend = Some(
                if let Some(ref endpoint) = toml_config.telemetry.otlp_endpoint {
                    Arc::new(
                        crate::providers::otlp_forwarding::OtlpForwardingBackend::new(
                            endpoint.clone(),
                            toml_config.telemetry.headers.clone(),
                        ),
                    ) as Arc<dyn TelemetryBackend>
                } else {
                    Arc::new(crate::providers::NullTelemetryBackend) as Arc<dyn TelemetryBackend>
                },
            );
        }

        // --- Auth validator ---
        if self.auth_validator.is_none() {
            self.auth_validator = Some(Arc::new(
                crate::providers::token_db_validator::TokenDbValidator::new(
                    self.token_store.clone().unwrap(),
                ),
            ));
        }

        // --- ServerBindings (command server, registry, bindings providers) ---
        if self.server_bindings.is_none() {
            let state_dir = self
                .config
                .state_dir
                .as_ref()
                .cloned()
                .unwrap_or_else(|| std::path::PathBuf::from("alien-data"));

            // -- Commands KV: from TOML config or local filesystem --
            let kv: Arc<dyn alien_bindings::traits::Kv> =
                if let Some(ref kv_binding) = toml_config.commands.kv {
                    let mut bindings = HashMap::new();
                    bindings.insert(
                        "kv".to_string(),
                        serde_json::to_value(kv_binding)
                            .into_alien_error()
                            .context(ErrorData::ServerInitFailed {
                                reason: "Failed to serialize KV binding".to_string(),
                            })?,
                    );
                    // Need a ClientConfig for the platform the KV binding targets
                    let platform = kv_binding_platform(kv_binding);
                    let config = alien_core::ClientConfig::from_std_env(platform)
                        .await
                        .context(ErrorData::ServerInitFailed {
                            reason: format!(
                                "Failed to load {} credentials for commands KV",
                                platform
                            ),
                        })?;
                    let provider =
                        BindingsProvider::new(config, bindings).context(
                            ErrorData::ServerInitFailed {
                                reason: "Failed to create KV bindings provider".to_string(),
                            },
                        )?;
                    provider.load_kv("kv").await.context(
                        ErrorData::ServerInitFailed {
                            reason: "Failed to load KV binding".to_string(),
                        },
                    )?
                } else {
                    let kv_path = state_dir.join("commands_kv");
                    Arc::new(LocalKv::new(kv_path).await.context(
                        ErrorData::ServerInitFailed {
                            reason: "Failed to create local KV store".to_string(),
                        },
                    )?)
                };

            // -- Commands storage: from TOML config or local filesystem --
            let command_storage: Arc<dyn alien_bindings::traits::Storage> =
                if let Some(ref storage_binding) = toml_config.commands.storage {
                    let mut bindings = HashMap::new();
                    bindings.insert(
                        "storage".to_string(),
                        serde_json::to_value(storage_binding)
                            .into_alien_error()
                            .context(ErrorData::ServerInitFailed {
                                reason: "Failed to serialize storage binding".to_string(),
                            })?,
                    );
                    let platform = storage_binding_platform(storage_binding);
                    let config = alien_core::ClientConfig::from_std_env(platform)
                        .await
                        .context(ErrorData::ServerInitFailed {
                            reason: format!(
                                "Failed to load {} credentials for commands storage",
                                platform
                            ),
                        })?;
                    let provider =
                        BindingsProvider::new(config, bindings).context(
                            ErrorData::ServerInitFailed {
                                reason: "Failed to create storage bindings provider".to_string(),
                            },
                        )?;
                    provider.load_storage("storage").await.context(
                        ErrorData::ServerInitFailed {
                            reason: "Failed to load storage binding".to_string(),
                        },
                    )?
                } else {
                    let storage_path = state_dir.join("commands_storage");
                    Arc::new(
                        LocalStorage::new(
                            storage_path
                                .to_str()
                                .unwrap_or("commands_storage")
                                .to_string(),
                        )
                        .into_alien_error()
                        .context(ErrorData::ServerInitFailed {
                            reason: "Failed to create local command storage".to_string(),
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

            // -- Primary bindings provider (embedded local registry) --
            // Always start the embedded local registry. It serves as the fallback
            // for local platform deployments and pull-mode image delivery.
            // Cloud registries (ECR/GAR/ACR) are in target_bindings for their
            // respective platforms — the routing table picks the right one.
            let bindings_provider = if self.bindings_provider_override.is_some() {
                self.bindings_provider_override.take()
            } else {
                let local_provider =
                    alien_local::LocalBindingsProvider::new(&state_dir).context(
                        ErrorData::ServerInitFailed {
                            reason: "Failed to create local bindings provider".to_string(),
                        },
                    )?;
                let registry_url = local_provider
                    .artifact_registry_manager()
                    .start_registry("artifact-registry")
                    .await
                    .context(ErrorData::ServerInitFailed {
                        reason: "Failed to start embedded artifact registry".to_string(),
                    })?;

                // Create the default "artifacts" repository so pushes work immediately.
                let ar = local_provider
                    .load_artifact_registry("artifact-registry")
                    .await
                    .context(ErrorData::ServerInitFailed {
                        reason: "Failed to load embedded artifact registry".to_string(),
                    })?;
                // Pre-create a default repository so pushes work immediately.
                // The container-registry crate expects two-level names (repo/image).
                let _ = ar.create_repository("artifacts/default").await;
                info!(registry_url = %registry_url, "Embedded local artifact registry started");
                Some(local_provider as Arc<dyn BindingsProviderApi>)
            };

            // Build registry routing table from configured providers.
            let mut routes = Vec::new();

            // Add target platform registries (ECR/GAR/ACR).
            for (platform, provider) in &target_bindings {
                for binding_name in ["artifacts", "artifact-registry"] {
                    if let Ok(ar) = provider.load_artifact_registry(binding_name).await {
                        let prefix = ar.upstream_repository_prefix();
                        if !prefix.is_empty() {
                            info!(
                                platform = %platform,
                                binding_name = %binding_name,
                                prefix = %prefix,
                                "Registered artifact registry route"
                            );
                            routes.push(crate::routes::registry_proxy::RegistryRoute {
                                prefix,
                                platform: *platform,
                                provider: provider.clone(),
                                binding_name: binding_name.to_string(),
                            });
                            break;
                        }
                    }
                }
            }

            // Add local/primary registry as catch-all fallback.
            if let Some(ref primary) = bindings_provider {
                if let Ok(ar) = primary.load_artifact_registry("artifact-registry").await {
                    let prefix = ar.upstream_repository_prefix();
                    info!(prefix = %prefix, "Registered local artifact registry route (fallback)");
                    routes.push(crate::routes::registry_proxy::RegistryRoute {
                        prefix,
                        platform: Platform::Local,
                        provider: primary.clone(),
                        binding_name: "artifact-registry".to_string(),
                    });
                }
            }

            let routing_table = Arc::new(crate::routes::registry_proxy::RegistryRoutingTable::new(
                routes,
            ));
            if let Err(e) = routing_table.validate() {
                return Err(AlienError::new(ErrorData::ServerInitFailed {
                    reason: format!("Artifact registry configuration error: {}", e),
                }));
            }

            self.server_bindings = Some(ServerBindings {
                kv,
                command_storage,
                command_dispatcher,
                command_registry,
                artifact_registry: None,
                bindings_provider,
                target_bindings_providers: target_bindings,
                registry_routing_table: routing_table,
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
// Binding → Platform helpers
// ---------------------------------------------------------------------------

/// Determine which cloud platform a KV binding targets.
fn kv_binding_platform(binding: &alien_core::bindings::KvBinding) -> alien_core::Platform {
    use alien_core::bindings::KvBinding;
    match binding {
        KvBinding::Dynamodb(_) => alien_core::Platform::Aws,
        KvBinding::Firestore(_) => alien_core::Platform::Gcp,
        KvBinding::TableStorage(_) => alien_core::Platform::Azure,
        KvBinding::Redis(_) | KvBinding::Local(_) => alien_core::Platform::Local,
    }
}

/// Determine which cloud platform a storage binding targets.
fn storage_binding_platform(
    binding: &alien_core::bindings::StorageBinding,
) -> alien_core::Platform {
    use alien_core::bindings::StorageBinding;
    match binding {
        StorageBinding::S3(_) => alien_core::Platform::Aws,
        StorageBinding::Gcs(_) => alien_core::Platform::Gcp,
        StorageBinding::Blob(_) => alien_core::Platform::Azure,
        StorageBinding::Local(_) => alien_core::Platform::Local,
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
        server_bindings.kv.clone(),
        server_bindings.command_storage.clone(),
        server_bindings.command_dispatcher.clone(),
        server_bindings.command_registry.clone(),
        config.commands_base_url(),
        config.response_signing_key.clone(),
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
        bindings_provider: server_bindings.bindings_provider.clone(),
        target_bindings_providers: server_bindings.target_bindings_providers.clone(),
        kv: server_bindings.kv.clone(),
        http_client: reqwest::Client::new(),
        credential_cache: Arc::new(crate::routes::registry_proxy::CredentialCache::new()),
        pull_validation_cache: Arc::new(crate::routes::registry_proxy::PullValidationCache::new()),
        registry_routing_table: server_bindings.registry_routing_table.clone(),
    };

    // --- Router ---
    let mut router = crate::routes::create_router_inner(app_state.clone(), router_options);
    if let Some(platform) = platform_routes {
        router = router.merge(platform.with_state(app_state.clone()));
    }
    if let Some(extra) = extra_routes {
        router = router.merge(extra.with_state(app_state));
    }
    // Apply CORS after all routes are merged so it covers platform and extra routes too.
    let router = router.layer(crate::routes::cors_layer(&config));

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
// Standalone provider bootstrapping helpers
// ---------------------------------------------------------------------------

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
    use alien_error::Context;
    use std::collections::HashMap;
    use tracing::warn;

    let primary_config = alien_core::ClientConfig::from_std_env(primary_platform)
        .await
        .context(ErrorData::ServerInitFailed {
            reason: format!(
                "Failed to load {} credentials for primary platform",
                primary_platform
            ),
        })?;

    let primary_bindings = parse_standard_bindings(env);
    info!(
        primary_platform = %primary_platform,
        bindings = ?primary_bindings.keys().collect::<Vec<_>>(),
        "Primary provider configured"
    );

    let primary_provider = Arc::new(
        BindingsProvider::new(primary_config, primary_bindings).context(
            ErrorData::ServerInitFailed {
                reason: "Failed to create primary bindings provider".to_string(),
            },
        )?,
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
            BindingsProvider::new(target_config, target_bindings).context(
                ErrorData::ServerInitFailed {
                    reason: format!("Failed to create {} target provider", platform),
                },
            )?,
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
