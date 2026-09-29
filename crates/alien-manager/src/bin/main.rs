//! Standalone alien-manager binary.
//!
//! Configuration is driven by TOML (`alien-manager.toml`) with CLI overrides.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use alien_manager::{
    standalone_config::ManagerTomlConfig,
    stores::sqlite::{SqliteDatabase, SqliteTokenStore},
    traits::TokenStore,
    AlienManager, ManagerConfig,
};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "alien-manager",
    about = "Control plane for Alien applications",
    version
)]
struct Cli {
    /// Path to TOML configuration file (default: alien-manager.toml in CWD).
    #[arg(long, short = 'c')]
    config: Option<PathBuf>,

    /// Generate a template alien-manager.toml and exit.
    #[arg(long)]
    init: bool,

    /// Override the HTTP server port.
    #[arg(long, env = "PORT")]
    port: Option<u16>,

    /// Override the HTTP server bind address.
    #[arg(long, env = "HOST")]
    host: Option<String>,

    /// Disable the deployment loop.
    #[arg(long, env = "DISABLE_DEPLOYMENT_LOOP")]
    disable_deployment_loop: bool,

    /// Disable the heartbeat loop.
    #[arg(long, env = "DISABLE_HEARTBEAT_LOOP")]
    disable_heartbeat_loop: bool,
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
        .build()
        .expect("Failed to build Tokio runtime");
    runtime.block_on(async_main());
}

async fn async_main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "alien_manager=info".into()),
        )
        .init();

    let cli = Cli::parse();

    // Handle --init: generate template and exit
    if cli.init {
        print!("{}", ManagerTomlConfig::generate_template());
        return;
    }

    let toml_config = ManagerTomlConfig::load(cli.config.as_deref()).unwrap_or_else(|e| {
        eprintln!("Error loading config: {}", e);
        std::process::exit(1);
    });

    let config = toml_config.to_manager_config();

    // Apply CLI overrides
    let config = apply_cli_overrides(config, &cli);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Invalid bind address");

    let server = build_standalone_server(config, &toml_config).await;
    server.start(addr).await.expect("Server exited with error");
}

/// Apply CLI-level overrides (--port, --host) on top of TOML-derived config.
fn apply_cli_overrides(mut config: ManagerConfig, cli: &Cli) -> ManagerConfig {
    if let Some(port) = cli.port {
        config.port = port;
    }
    if let Some(ref host) = cli.host {
        config.host = host.clone();
    }
    if cli.disable_deployment_loop {
        config.disable_deployment_loop = true;
    }
    if cli.disable_heartbeat_loop {
        config.disable_heartbeat_loop = true;
    }
    config
}

/// Build standalone server: SQLite stores + admin token bootstrap + stale lock cleanup.
async fn build_standalone_server(
    mut config: ManagerConfig,
    toml_config: &ManagerTomlConfig,
) -> AlienManager {
    let addr_display = format!("{}:{}", config.host, config.port);
    let state_dir = config
        .state_dir
        .clone()
        .expect("the manager config always resolves a state directory");
    std::fs::create_dir_all(&state_dir).unwrap_or_else(|e| {
        panic!(
            "Failed to create state directory {}: {}",
            state_dir.display(),
            e
        )
    });
    let db_path = config
        .db_path
        .clone()
        .expect("the manager config always resolves a database path");
    let db = Arc::new(
        SqliteDatabase::new_with_key(
            &db_path.to_string_lossy(),
            toml_config.database.encryption_key.as_deref(),
        )
        .await
        .unwrap_or_else(|e| panic!("Failed to initialize database: {}", e)),
    );
    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db));

    let admin_token = alien_manager::bootstrap::ensure_admin_token(
        token_store.as_ref(),
        &state_dir,
        std::env::var(alien_manager::bootstrap::ADMIN_TOKEN_ENV).ok(),
    )
    .await
    .unwrap_or_else(|e| panic!("Failed to set up the admin token: {e}"));
    if let alien_manager::bootstrap::AdminToken::Generated(token) = &admin_token {
        println!("Generated admin token (shown once; save it securely):");
        println!("  {token}");
        println!();
        println!("Connect the CLI:");
        println!(
            "  alien login --manager {} --token {token}",
            config.base_url()
        );
        println!();
    }
    config.response_signing_key = alien_manager::bootstrap::response_signing_key(&state_dir)
        .unwrap_or_else(|e| panic!("Failed to set up the response signing key: {e}"));

    // Build the server with standalone defaults
    let server = AlienManager::builder(config)
        .token_store(token_store)
        .tunnels()
        .charts(alien_manager::routes::charts::ChartSettings::new(
            toml_config.operator.image.clone(),
            toml_config.operator.insecure_registry,
        ))
        .with_standalone_defaults(toml_config)
        .await
        .expect("Failed to set up standalone defaults")
        .build()
        .await
        .expect("Failed to build alien-manager");

    // Clean up stale deployment locks from previous runs. Startup hook —
    // `Subject::system()` is the synthetic operator (single-tenant OSS mode).
    let deployment_store = server.deployment_store();
    let startup_caller = alien_manager::auth::Subject::system();
    match deployment_store.cleanup_stale_locks(&startup_caller).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(count = n, "Cleaned up stale deployment locks"),
        Err(e) => tracing::warn!(error = %e, "Failed to clean up stale deployment locks"),
    }

    println!();
    println!("────────────────────────────────────────────────");
    println!("  Alien Manager running on http://{}", addr_display);
    println!("────────────────────────────────────────────────");
    println!();

    server
}
