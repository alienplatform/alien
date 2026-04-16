//! Standalone alien-manager binary.
//!
//! Configuration is driven by TOML (`alien-manager.toml`) with CLI overrides.
//! For Platform mode, use the `alien-managerx` binary instead.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use alien_manager::{
    standalone_config::ManagerTomlConfig,
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
    let (token_store, admin_token) = bootstrap_standalone_admin_token(&config).await;

    // Derive command response signing key from the admin token.
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(admin_token.as_bytes());
    hasher.update(b":commands-response-signing");
    config.response_signing_key = hasher.finalize().to_vec();

    // Build the server with standalone defaults
    let server = AlienManager::builder(config)
        .token_store(token_store)
        .with_standalone_defaults(toml_config)
        .await
        .expect("Failed to set up standalone defaults")
        .build()
        .await
        .expect("Failed to build alien-manager");

    // Clean up stale deployment locks from previous runs
    let deployment_store = server.deployment_store();
    match deployment_store.cleanup_stale_locks().await {
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

/// Bootstrap admin token for standalone mode.
///
/// On first run: generates an `ax_admin_<uuid>` token, writes it to `{state_dir}/admin-token`,
/// hashes it with SHA-256, and stores it in SQLite via TokenStore.
/// On subsequent runs: reads the existing token from the file and verifies it exists in the DB.
/// Returns the pre-created TokenStore so the builder reuses the same DB connection.
async fn bootstrap_standalone_admin_token(
    config: &ManagerConfig,
) -> (Arc<dyn TokenStore>, String) {
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
        alien_core::file_utils::write_secret_file(&token_path, token.as_bytes())
            .unwrap_or_else(|e| {
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

    (token_store, token)
}
