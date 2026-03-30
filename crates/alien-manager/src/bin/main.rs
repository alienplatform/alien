//! Standalone alien-manager binary.
//!
//! Configuration is driven by clap (CLI flags + env vars). Mode detection:
//! - `--api-key` / `MANAGER_API_KEY` → Platform mode (requires `platform` feature)
//! - Otherwise → Standalone mode (SQLite + admin token bootstrap)

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use alien_manager::{
    stores::sqlite::{SqliteDatabase, SqliteTokenStore},
    traits::{CreateTokenParams, TokenStore, TokenType},
    AlienManager, ManagerConfig,
};
use clap::Parser;
use sha2::{Digest, Sha256};

#[derive(Parser, Debug)]
#[command(
    name = "alien-manager",
    about = "Control plane for Alien applications",
    version
)]
struct Cli {
    /// HTTP server port.
    #[arg(long, env = "PORT", default_value = "8080")]
    port: u16,

    /// HTTP server bind address.
    #[arg(long, env = "HOST", default_value = "0.0.0.0")]
    host: String,

    /// Path to SQLite database file.
    #[arg(long, env = "ALIEN_DB_PATH")]
    db_path: Option<PathBuf>,

    /// Directory for local state (KV, storage, etc.).
    #[arg(long, env = "ALIEN_STATE_DIR")]
    state_dir: Option<PathBuf>,

    /// Deployment loop interval in seconds.
    #[arg(long, env = "DEPLOYMENT_INTERVAL", default_value = "10")]
    deployment_interval: u64,

    /// Heartbeat interval in seconds.
    #[arg(long, env = "HEARTBEAT_INTERVAL", default_value = "60")]
    heartbeat_interval: u64,

    /// Self-heartbeat interval in seconds (platform mode).
    #[arg(long, env = "SELF_HEARTBEAT_INTERVAL", default_value = "60")]
    self_heartbeat_interval: u64,

    /// OTLP endpoint for telemetry forwarding.
    #[arg(long, env = "OTLP_ENDPOINT")]
    otlp_endpoint: Option<String>,

    /// Public base URL for this manager instance.
    #[arg(long, env = "BASE_URL")]
    base_url: Option<String>,

    /// Base URL for release binary downloads.
    #[arg(long, env = "ALIEN_RELEASES_URL")]
    releases_url: Option<String>,

    /// Disable the deployment loop.
    #[arg(long, env = "DISABLE_DEPLOYMENT_LOOP")]
    disable_deployment_loop: bool,

    /// Disable the heartbeat loop.
    #[arg(long, env = "DISABLE_HEARTBEAT_LOOP")]
    disable_heartbeat_loop: bool,

    // --- Platform mode options ---
    /// Manager API key (triggers Platform mode when set).
    #[arg(long, env = "MANAGER_API_KEY")]
    api_key: Option<String>,

    /// Alien Platform API URL.
    #[arg(long, env = "ALIEN_API_URL", default_value = "https://api.alien.dev")]
    api_url: String,

    /// Target platforms (comma-separated: aws,gcp,azure).
    #[arg(long, env = "TARGETS", value_delimiter = ',')]
    targets: Vec<alien_core::Platform>,

    /// Primary platform for bindings infrastructure.
    #[arg(long, env = "ALIEN_PRIMARY_PLATFORM", default_value = "aws")]
    primary_platform: alien_core::Platform,

    /// DeepStore OTLP endpoint URL.
    #[arg(long, env = "DEEPSTORE_OTLP_URL")]
    deepstore_otlp_url: Option<String>,

    /// DeepStore query endpoint URL.
    #[arg(long, env = "DEEPSTORE_QUERY_URL")]
    deepstore_query_url: Option<String>,

    /// DeepStore JWT public key (PEM).
    #[arg(long, env = "DEEPSTORE_JWT_PUBLIC_KEY")]
    deepstore_jwt_public_key: Option<String>,

    /// DeepStore database ID.
    #[arg(long, env = "DEEPSTORE_DATABASE_ID")]
    deepstore_database_id: Option<String>,

    /// GCP OAuth Client ID.
    #[arg(long, env = "GCP_OAUTH_CLIENT_ID")]
    gcp_oauth_client_id: Option<String>,

    /// GCP OAuth Client Secret.
    #[arg(long, env = "GCP_OAUTH_CLIENT_SECRET")]
    gcp_oauth_client_secret: Option<String>,
}

/// Detected operating mode from CLI args/env.
enum Mode {
    Platform {
        platform_config: alien_manager::PlatformConfig,
    },
    Standalone,
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "alien_manager=info".into()),
        )
        .init();

    let cli = Cli::parse();

    // --- Mode detection ---
    let mode = detect_mode(&cli);

    // --- Build ManagerConfig (mode-agnostic) ---
    let config = build_config(&cli, &mode);
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Invalid bind address");

    // --- Wire providers based on detected mode ---
    let server = match mode {
        Mode::Platform { platform_config } => build_platform_server(config, platform_config).await,
        Mode::Standalone => build_standalone_server(config).await,
    };

    server.start(addr).await.expect("Server exited with error");
}

fn detect_mode(cli: &Cli) -> Mode {
    if let Some(ref api_key) = cli.api_key {
        Mode::Platform {
            platform_config: alien_manager::PlatformConfig {
                api_url: cli.api_url.clone(),
                api_key: api_key.clone(),
                primary_platform: cli.primary_platform,
                deepstore: alien_manager::DeepStoreConfig {
                    otlp_url: cli.deepstore_otlp_url.clone(),
                    query_url: cli.deepstore_query_url.clone(),
                    jwt_public_key: cli.deepstore_jwt_public_key.clone(),
                    database_id: cli.deepstore_database_id.clone(),
                },
                gcp_oauth: alien_manager::GcpOAuthConfig {
                    client_id: cli.gcp_oauth_client_id.clone(),
                    client_secret: cli.gcp_oauth_client_secret.clone(),
                },
            },
        }
    } else {
        Mode::Standalone
    }
}

fn build_config(cli: &Cli, mode: &Mode) -> ManagerConfig {
    let is_platform = matches!(mode, Mode::Platform { .. });

    let db_path = if is_platform {
        None
    } else {
        Some(
            cli.db_path
                .clone()
                .unwrap_or_else(|| PathBuf::from("alien-manager.db")),
        )
    };

    let state_dir = if is_platform {
        None
    } else {
        Some(
            cli.state_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from(".alien-manager")),
        )
    };

    ManagerConfig {
        port: cli.port,
        host: cli.host.clone(),
        db_path,
        state_dir,
        deployment_interval_secs: cli.deployment_interval,
        heartbeat_interval_secs: cli.heartbeat_interval,
        self_heartbeat_interval_secs: cli.self_heartbeat_interval,
        otlp_endpoint: cli.otlp_endpoint.clone(),
        base_url: cli.base_url.clone(),
        releases_url: cli.releases_url.clone(),
        targets: cli.targets.clone(),
        disable_deployment_loop: cli.disable_deployment_loop,
        disable_heartbeat_loop: cli.disable_heartbeat_loop,
        enable_local_log_ingest: false,
    }
}

/// Build standalone server: SQLite stores + admin token bootstrap.
async fn build_standalone_server(config: ManagerConfig) -> AlienManager {
    let addr_display = format!("{}:{}", config.host, config.port);
    let token_store = bootstrap_standalone_admin_token(&config).await;

    let server = AlienManager::builder(config)
        .token_store(token_store)
        .with_standalone_defaults()
        .await
        .expect("Failed to set up SQLite defaults")
        .build()
        .await
        .expect("Failed to build alien-manager");

    println!();
    println!("────────────────────────────────────────────────");
    println!("  Alien Manager running on http://{}", addr_display);
    println!("────────────────────────────────────────────────");
    println!();

    server
}
/// Build platform server: API-backed providers, multi-tenant.
#[cfg(feature = "platform")]
async fn build_platform_server(
    config: ManagerConfig,
    pc: alien_manager::PlatformConfig,
) -> AlienManager {
    use alien_bindings::{BindingsProvider, BindingsProviderApi};
    use alien_manager::builder::{bootstrap_manager_identity, build_standalone_providers};
    use alien_manager::providers::platform_api::{
        extension::{build_platform_client, resolve_base_url},
        DeepStoreTelemetryBackend, ImpersonationCredentialResolver, NullTokenStore,
        PlatformApiDeploymentStore, PlatformApiReleaseStore, PlatformCommandRegistry,
        PlatformState, PlatformTokenValidator,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use tracing::{info, warn};

    // --- Bootstrap manager identity via whoami ---
    let identity = bootstrap_manager_identity(&pc.api_url, &pc.api_key)
        .await
        .expect("Failed to resolve manager identity");
    info!(
        manager_id = %identity.manager_id,
        workspace_name = %identity.workspace_name,
        "Identity resolved from MANAGER_API_KEY via whoami"
    );

    // --- Build bindings providers ---
    let env: HashMap<String, String> = std::env::vars().collect();
    let is_alien_app = std::env::var("ALIEN_CURRENT_CONTAINER_BINDING_NAME").is_ok();

    let (bindings, target_bindings): (
        Arc<dyn BindingsProviderApi>,
        HashMap<alien_core::Platform, Arc<dyn BindingsProviderApi>>,
    ) = if is_alien_app {
        info!("Alien App mode: using bindings from Alien runtime environment");
        let provider = Arc::new(
            BindingsProvider::from_env(env.clone())
                .await
                .expect("Failed to initialize bindings provider"),
        );
        (provider as Arc<dyn BindingsProviderApi>, HashMap::new())
    } else {
        info!(
            primary_platform = %pc.primary_platform,
            "Standalone mode: building multi-cloud providers"
        );
        build_standalone_providers(pc.primary_platform, &env)
            .await
            .expect("Failed to build standalone providers")
    };

    // --- Resolve base URL ---
    let base_url = resolve_base_url(&config.base_url, config.port, &bindings)
        .await
        .expect("Failed to resolve base URL");

    // --- Build Platform API client ---
    let platform_client =
        build_platform_client(&pc.api_url, &pc.api_key).expect("Failed to build platform client");

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
                alien_manager::loops::self_heartbeat::run_self_heartbeat_loop(ext_clone).await
            {
                tracing::error!(error = %e, "Self-heartbeat loop failed");
            }
        });
    }

    // --- Build providers ---
    let deployment_store: Arc<dyn alien_manager::traits::DeploymentStore> = Arc::new(
        PlatformApiDeploymentStore::new(platform_client.clone(), identity.manager_id.clone()),
    );

    let release_store: Arc<dyn alien_manager::traits::ReleaseStore> =
        Arc::new(PlatformApiReleaseStore::new(platform_client.clone()));

    let token_store: Arc<dyn alien_manager::traits::TokenStore> = Arc::new(NullTokenStore);

    let credential_resolver: Arc<dyn alien_manager::traits::CredentialResolver> = Arc::new(
        ImpersonationCredentialResolver::new(bindings.clone(), target_bindings.clone()),
    );

    let telemetry_backend: Arc<dyn alien_manager::traits::TelemetryBackend> = if let (
        Some(otlp_url),
        Some(database_id),
    ) = (
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
        Arc::new(alien_manager::providers::NullTelemetryBackend)
    };

    let auth_validator: Arc<dyn alien_manager::traits::AuthValidator> =
        Arc::new(PlatformTokenValidator::new(pc.api_url.clone()));

    // --- ServerBindings ---
    let command_kv = bindings
        .load_kv("command-kv")
        .await
        .expect("Failed to load command-kv binding");
    let command_storage = bindings
        .load_storage("command-storage")
        .await
        .expect("Failed to load command-storage binding");

    let command_dispatcher: Arc<dyn alien_commands::server::CommandDispatcher> =
        Arc::new(alien_manager::commands::DefaultCommandDispatcher::new(
            deployment_store.clone(),
            release_store.clone(),
            credential_resolver.clone(),
        ));

    let command_registry: Arc<dyn alien_commands::server::CommandRegistry> = Arc::new(
        PlatformCommandRegistry::new(&pc.api_url, &pc.api_key)
            .expect("Failed to create command registry"),
    );

    let server_bindings = alien_manager::traits::ServerBindings {
        command_kv,
        command_storage,
        command_dispatcher,
        command_registry,
        artifact_registry: None,
        bindings_provider: None,
    };

    // Platform-specific routes
    let platform_routes = alien_manager::routes::platform::build_platform_routes(ext);

    info!(
        port = config.port,
        manager_id = %identity.manager_id,
        base_url = %base_url,
        "Building AlienManager (platform mode)"
    );

    AlienManager::builder(config)
        .deployment_store(deployment_store)
        .release_store(release_store)
        .token_store(token_store)
        .credential_resolver(credential_resolver)
        .telemetry_backend(telemetry_backend)
        .auth_validator(auth_validator)
        .server_bindings(server_bindings)
        .platform_routes(platform_routes)
        .skip_initialize()
        .skip_deploy_page()
        .skip_install()
        .build()
        .await
        .expect("Failed to build alien-manager (platform mode)")
}

#[cfg(not(feature = "platform"))]
async fn build_platform_server(
    _config: ManagerConfig,
    _pc: alien_manager::PlatformConfig,
) -> AlienManager {
    panic!("Platform mode requires the 'platform' feature to be enabled");
}

/// Bootstrap admin token for standalone mode.
///
/// On first run: generates an `ax_admin_<uuid>` token, writes it to `{state_dir}/admin-token`,
/// hashes it with SHA-256, and stores it in SQLite via TokenStore.
/// On subsequent runs: reads the existing token from the file and verifies it exists in the DB.
/// Returns the pre-created TokenStore so the builder reuses the same DB connection.
async fn bootstrap_standalone_admin_token(config: &ManagerConfig) -> Arc<dyn TokenStore> {
    let state_dir = config
        .state_dir
        .as_ref()
        .expect("state_dir is required for standalone mode");

    std::fs::create_dir_all(state_dir).unwrap_or_else(|e| {
        panic!(
            "Failed to create state directory {}: {}",
            state_dir.display(),
            e
        )
    });

    let db_path = config
        .db_path
        .as_ref()
        .expect("db_path is required for standalone mode");

    let token_path = state_dir.join("admin-token");

    // Read or generate admin token
    let token = if !token_path.exists() {
        let token = format!(
            "ax_admin_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        );
        std::fs::write(&token_path, &token).unwrap_or_else(|e| {
            panic!(
                "Failed to write admin token to {}: {}",
                token_path.display(),
                e
            )
        });

        println!("Generated admin token (save this securely):");
        println!("  {}", token);
        println!();
        println!("Set it as ALIEN_API_KEY when using the CLI:");
        println!(
            "  export ALIEN_MANAGER_URL=http://localhost:{}",
            config.port
        );
        println!("  export ALIEN_API_KEY={}", token);
        println!();
        token
    } else {
        let token = std::fs::read_to_string(&token_path).unwrap_or_else(|e| {
            panic!(
                "Failed to read admin token from {}: {}",
                token_path.display(),
                e
            )
        });
        let token = token.trim().to_string();
        tracing::info!("Using existing admin token from {}", token_path.display());
        token
    };

    // Create SQLite database and token store
    let db = Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap_or_else(|e| panic!("Failed to initialize database: {}", e)),
    );

    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));

    // Compute SHA-256 hash and bootstrap the token into the DB
    let key_hash = {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    };
    let key_prefix = token[..12.min(token.len())].to_string();

    match token_store.validate_token(&key_hash).await {
        Ok(Some(_)) => {
            tracing::info!("Admin token already registered in database");
        }
        Ok(None) => {
            token_store
                .create_token(CreateTokenParams {
                    token_type: TokenType::Admin,
                    key_prefix,
                    key_hash,
                    deployment_group_id: None,
                    deployment_id: None,
                })
                .await
                .expect("Failed to bootstrap admin token");
            tracing::info!("Admin token bootstrapped into database");
        }
        Err(e) => {
            panic!("Failed to check existing token: {}", e);
        }
    }

    println!("Quick start:");
    println!(
        "  export ALIEN_MANAGER_URL=http://localhost:{}",
        config.port
    );
    println!("  export ALIEN_API_KEY={}", token);
    println!();
    println!("  alien build --platform local");
    println!("  alien release --platform local --yes");
    println!("  alien onboard my-fleet");
    println!();

    token_store
}
