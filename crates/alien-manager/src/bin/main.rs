//! Standalone alien-manager binary.
//!
//! Configuration is driven by clap (CLI flags + env vars). Mode detection:
//! - `--api-key` / `MANAGER_API_KEY` → Platform mode (requires `platform` feature)
//! - `--dev-mode` / `ALIEN_DEV_MODE` → Dev mode
//! - Otherwise → Standalone mode

use std::net::SocketAddr;
use std::path::PathBuf;

use alien_core::Platform;
use alien_manager::{
    AlienManager, DeepStoreConfig, GcpOAuthConfig, ManagerConfig, PlatformConfig,
    config::ManagerMode,
};
use clap::Parser;

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

    /// Enable dev mode (SQLite + permissive auth + local credentials).
    #[arg(long, env = "ALIEN_DEV_MODE")]
    dev_mode: bool,

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
    targets: Vec<Platform>,

    /// Primary platform for bindings infrastructure.
    #[arg(long, env = "ALIEN_PRIMARY_PLATFORM", default_value = "aws")]
    primary_platform: Platform,

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
    let config = build_config(cli);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Invalid bind address");

    let server = AlienManager::builder(config)
        .build()
        .await
        .expect("Failed to build alien-manager");

    // If not dev mode and not platform mode, print admin token hint
    if !server.config().dev_mode() && !server.config().mode.is_platform() {
        print_initial_admin_token(&server).await;
    }

    server.start(addr).await.expect("Server exited with error");
}

fn build_config(cli: Cli) -> ManagerConfig {
    // Mode detection: api_key present → Platform, dev_mode → Dev, else → Standalone
    let mode = if let Some(api_key) = cli.api_key {
        ManagerMode::Platform(PlatformConfig {
            api_url: cli.api_url,
            api_key,
            primary_platform: cli.primary_platform,
            deepstore: DeepStoreConfig {
                otlp_url: cli.deepstore_otlp_url,
                query_url: cli.deepstore_query_url,
                jwt_public_key: cli.deepstore_jwt_public_key,
                database_id: cli.deepstore_database_id,
            },
            gcp_oauth: GcpOAuthConfig {
                client_id: cli.gcp_oauth_client_id,
                client_secret: cli.gcp_oauth_client_secret,
            },
        })
    } else if cli.dev_mode {
        ManagerMode::Dev
    } else {
        ManagerMode::Standalone
    };

    let db_path = if mode.is_platform() {
        None
    } else {
        Some(cli.db_path.unwrap_or_else(|| PathBuf::from("alien-manager.db")))
    };

    let state_dir = if mode.is_platform() {
        None
    } else {
        Some(cli.state_dir.unwrap_or_else(|| PathBuf::from(".alien-manager")))
    };

    ManagerConfig {
        port: cli.port,
        host: cli.host,
        db_path,
        state_dir,
        deployment_interval_secs: cli.deployment_interval,
        heartbeat_interval_secs: cli.heartbeat_interval,
        self_heartbeat_interval_secs: cli.self_heartbeat_interval,
        otlp_endpoint: cli.otlp_endpoint,
        base_url: cli.base_url,
        releases_url: cli.releases_url,
        targets: cli.targets,
        disable_deployment_loop: cli.disable_deployment_loop,
        disable_heartbeat_loop: cli.disable_heartbeat_loop,
        mode,
    }
}

async fn print_initial_admin_token(server: &AlienManager) {
    let _ = async {
        tracing::info!(
            "alien-manager started in production mode. \
             Use the API to create admin tokens for authentication."
        );
    }
    .await;
}
