//! Builder for constructing an `AlienManager` with customizable providers.

use std::sync::Arc;

#[cfg(feature = "sqlite")]
use alien_error::AlienError;
#[cfg(feature = "sqlite")]
use tracing::info;

use crate::config::ManagerConfig;
use crate::traits::*;
#[cfg(feature = "sqlite")]
use crate::{error::ErrorData, server::AlienManager};

pub struct AlienManagerBuilder {
    #[cfg_attr(not(feature = "sqlite"), allow(dead_code))]
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

    /// Build the server, creating default providers for any not explicitly set.
    ///
    /// Default providers (standalone / dev mode):
    /// - **DeploymentStore / ReleaseStore / TokenStore** — SQLite (Turso)
    /// - **CredentialResolver** — `EnvironmentCredentialResolver` (reads AWS_*/GCP_*/AZURE_* from env)
    /// - **TelemetryBackend** — `OtlpForwardingBackend` if `otlp_endpoint` is set, otherwise
    ///   `InMemoryTelemetryBackend` (ring buffer)
    /// - **AuthValidator** — `PermissiveAuthValidator` if `dev_mode`, otherwise `TokenDbValidator`
    /// - **ServerBindings** — local KV + local storage + NullCommandDispatcher + SqliteCommandRegistry
    #[cfg(feature = "sqlite")]
    pub async fn build(self) -> crate::error::Result<AlienManager> {
        use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
        use alien_commands::server::{CommandServer, NullCommandDispatcher};

        let config = Arc::new(self.config);

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
            self.deployment_store.unwrap_or_else(|| {
                Arc::new(crate::stores::sqlite::SqliteDeploymentStore::new(
                    db.clone(),
                ))
            });

        let release_store: Arc<dyn ReleaseStore> = self.release_store.unwrap_or_else(|| {
            Arc::new(crate::stores::sqlite::SqliteReleaseStore::new(db.clone()))
        });

        let token_store: Arc<dyn TokenStore> = self
            .token_store
            .unwrap_or_else(|| Arc::new(crate::stores::sqlite::SqliteTokenStore::new(db.clone())));

        // --- Providers ---
        let credential_resolver: Arc<dyn CredentialResolver> = self
            .credential_resolver
            .unwrap_or_else(|| {
                if config.dev_mode {
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
            self.telemetry_backend.unwrap_or_else(|| {
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

        let auth_validator: Arc<dyn AuthValidator> = self.auth_validator.unwrap_or_else(|| {
            if config.dev_mode {
                Arc::new(crate::providers::permissive_auth::PermissiveAuthValidator::new())
            } else {
                Arc::new(crate::providers::token_db_validator::TokenDbValidator::new(
                    token_store.clone(),
                ))
            }
        });

        // --- ServerBindings (command server plumbing) ---
        let server_bindings = if let Some(bindings) = self.server_bindings {
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
        let mut router = crate::routes::create_router_inner(app_state.clone(), !self.skip_initialize);
        if let Some(extra) = self.extra_routes {
            router = router.merge(extra.with_state(app_state));
        }

        // --- Dev mode: ensure default deployment group exists ---
        if config.dev_mode {
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
                // Use the well-known ID "local-dev" so clients can reference it directly
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
            dev_mode = config.dev_mode,
            db_path = %config.db_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
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
            dev_status_tx: self.dev_status_tx,
            log_buffer,
        })
    }

    /// Build the server with explicitly-provided providers (no SQLite defaults).
    ///
    /// All providers and `server_bindings` must be set before calling this method.
    /// This is the entry point for embedding alien-manager in another process (e.g.
    /// alien-platform-manager) that manages its own storage layer.
    #[cfg(not(feature = "sqlite"))]
    pub async fn build(self) -> crate::error::Result<crate::server::AlienManager> {
        use crate::error::ErrorData;
        use alien_error::AlienError;
        use alien_commands::server::CommandServer;
        use tracing::info;

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

        let config = std::sync::Arc::new(self.config);

        let deployment_store  = require_provider!(self.deployment_store,  "deployment_store");
        let release_store     = require_provider!(self.release_store,     "release_store");
        let token_store       = require_provider!(self.token_store,       "token_store");
        let credential_resolver = require_provider!(self.credential_resolver, "credential_resolver");
        let telemetry_backend = require_provider!(self.telemetry_backend, "telemetry_backend");
        let auth_validator    = require_provider!(self.auth_validator,    "auth_validator");
        let server_bindings   = std::sync::Arc::new(
            require_provider!(self.server_bindings, "server_bindings")
        );

        let command_server = std::sync::Arc::new(CommandServer::new(
            server_bindings.command_kv.clone(),
            server_bindings.command_storage.clone(),
            server_bindings.command_dispatcher.clone(),
            server_bindings.command_registry.clone(),
            config.commands_base_url(),
        ));

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

        let mut router = crate::routes::create_router_inner(app_state.clone(), !self.skip_initialize);
        if let Some(extra) = self.extra_routes {
            router = router.merge(extra.with_state(app_state));
        }

        info!(port = config.port, "AlienManager built (no sqlite defaults)");

        Ok(crate::server::AlienManager {
            config,
            router,
            deployment_store,
            release_store,
            credential_resolver,
            telemetry_backend,
            server_bindings,
            dev_status_tx: self.dev_status_tx,
            log_buffer: std::sync::Arc::new(crate::dev::LogBuffer::new()),
        })
    }
}
