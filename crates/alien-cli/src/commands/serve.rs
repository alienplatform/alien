//! `alien serve` — start the OSS alien-manager server.

use crate::error::{ErrorData, Result};
use crate::get_current_dir;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager::traits::{CreateTokenParams, TokenStore, TokenType};
use clap::Parser;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::info;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Start the Alien manager server",
    long_about = "Start a self-hosted alien-manager instance with SQLite storage. \
    On first run, generates and displays an admin token.",
    after_help = "EXAMPLES:
    # Start on default port 8080
    alien serve

    # Start on custom port
    alien serve --port 9090

    # Start with custom data directory
    alien serve --data-dir /var/lib/alien"
)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value = "8080")]
    pub port: u16,

    /// Data directory for SQLite database and state
    #[arg(long)]
    pub data_dir: Option<String>,

    /// Bind address
    #[arg(long, default_value = "0.0.0.0")]
    pub bind: String,
}

pub async fn serve_task(args: ServeArgs) -> Result<()> {
    let data_dir = if let Some(dir) = args.data_dir {
        std::path::PathBuf::from(dir)
    } else {
        get_current_dir()?.join(".alien")
    };

    std::fs::create_dir_all(&data_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: data_dir.display().to_string(),
            reason: "Failed to create data directory".to_string(),
        })?;

    let db_path = data_dir.join("alien-manager.db");
    let token_path = data_dir.join("admin-token");

    // Read or generate admin token
    let token = if !token_path.exists() {
        let token = format!(
            "am_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        );
        std::fs::write(&token_path, &token)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "write".to_string(),
                file_path: token_path.display().to_string(),
                reason: "Failed to write admin token".to_string(),
            })?;

        println!("Generated admin token (save this securely):");
        println!("  {}", token);
        println!();
        println!("Set it as ALIEN_API_KEY when using the CLI:");
        println!("  export ALIEN_SERVER=http://localhost:{}", args.port);
        println!("  export ALIEN_API_KEY={}", token);
        println!();
        token
    } else {
        let token = std::fs::read_to_string(&token_path)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: token_path.display().to_string(),
                reason: "Failed to read admin token".to_string(),
            })?;
        let token = token.trim().to_string();
        info!("Using existing admin token from {}", token_path.display());
        println!("Admin token: {}", token);
        token
    };

    // Create the SQLite database and token store, then bootstrap the admin token
    // before building the manager. This ensures the token is in the DB so that
    // TokenDbValidator can authenticate requests.
    let db = Arc::new(
        alien_manager::stores::sqlite::SqliteDatabase::new(
            &db_path.to_string_lossy(),
        )
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ServerStartFailed {
                reason: format!("Failed to initialize database: {}", e),
            })
        })?,
    );

    let token_store: Arc<dyn TokenStore> = Arc::new(
        alien_manager::stores::sqlite::SqliteTokenStore::new(db.clone()),
    );

    // Bootstrap the admin token: compute hash and insert into DB
    let key_hash = {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    };
    let key_prefix = token[..12.min(token.len())].to_string();

    // Check if this token already exists in the DB
    match token_store.validate_token(&key_hash).await {
        Ok(Some(_)) => {
            info!("Admin token already registered in database");
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
                .map_err(|e| {
                    AlienError::new(ErrorData::ServerStartFailed {
                        reason: format!("Failed to bootstrap admin token: {}", e),
                    })
                })?;
            info!("Admin token bootstrapped into database");
        }
        Err(e) => {
            return Err(AlienError::new(ErrorData::ServerStartFailed {
                reason: format!("Failed to check existing token: {}", e),
            })
            .into());
        }
    }

    let releases_url = std::env::var("ALIEN_RELEASES_URL").ok();

    let config = alien_manager::ManagerConfig {
        port: args.port,
        db_path: Some(db_path),
        state_dir: Some(data_dir),
        dev_mode: false,
        releases_url,
        ..Default::default()
    };

    let addr: std::net::SocketAddr = format!("{}:{}", args.bind, args.port)
        .parse()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Invalid bind address: {}:{}", args.bind, args.port),
        })?;

    println!();
    println!("────────────────────────────────────────────────");
    println!("  Alien Manager running on http://{}", addr);
    println!("────────────────────────────────────────────────");
    println!();
    println!("Quick start:");
    println!("  export ALIEN_SERVER=http://localhost:{}", args.port);
    println!("  export ALIEN_API_KEY={}", token);
    println!();
    println!("  alien build --platform local");
    println!("  alien release --platform local --yes");
    println!("  alien onboard my-fleet");
    println!();

    // Pass the pre-created token store to the builder so it reuses the same DB connection
    let server = alien_manager::AlienManager::builder(config)
        .token_store(token_store)
        .build()
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to initialize manager".to_string(),
        })?;

    server
        .start(addr)
        .await
        .into_alien_error()
        .context(ErrorData::ServerStartFailed {
            reason: "Server stopped unexpectedly".to_string(),
        })
}
