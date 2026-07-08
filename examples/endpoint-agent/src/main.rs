//! Endpoint agent: a command-only daemon that monitors device activity and
//! stores events in an encrypted local database.
//!
//! Its only interface is Alien commands, so it drives the app-owned pull
//! command receiver (`alien_commands::Receiver`) directly:
//! `Receiver::from_env()` → register handlers with `receiver.handle(...)` →
//! `receiver.run().await`. There are no resource bindings to load — the
//! encrypted store is a local Turso database, not a linked binding.

use tokio::spawn;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;
mod db;
mod error;
mod monitor;
mod pii;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,endpoint_agent=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    tracing::info!("Starting endpoint agent");

    // Generate encryption key from device ID + Alien-managed secret
    // In production, this would use a proper key derivation function
    let encryption_key = generate_encryption_key()?;

    // Initialize encrypted database
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./.data".to_string());
    let db = db::EncryptedDb::new(&data_dir, &encryption_key).await?;

    tracing::info!("Encrypted database initialized");

    // Start background monitoring
    let db_clone = db.clone();
    spawn(async move {
        if let Err(e) = monitor::watch_filesystem(&db_clone).await {
            tracing::error!("Filesystem monitoring error: {}", e);
        }
    });

    let db_clone = db.clone();
    spawn(async move {
        if let Err(e) = monitor::watch_clipboard(&db_clone).await {
            tracing::error!("Clipboard monitoring error: {}", e);
        }
    });

    tracing::info!("Started monitoring tasks");

    // Build the app-owned pull command receiver from the injected environment
    // and register the command handlers on it.
    let mut receiver = alien_commands::Receiver::from_env()?;
    commands::register(&mut receiver, db);

    tracing::info!("Registered command handlers");

    // Run the command receiver loop (blocks until shutdown)
    receiver.run().await?;

    Ok(())
}

/// Generate encryption key from device ID + Alien-managed secret
///
/// In production, this would use a proper key derivation function like PBKDF2 or Argon2.
/// For demo purposes, we generate a simple hex key from environment or random.
fn generate_encryption_key() -> std::result::Result<String, Box<dyn std::error::Error>> {
    // Check if we have a pre-generated key (useful for testing)
    if let Ok(key) = std::env::var("DB_ENCRYPTION_KEY") {
        return Ok(key);
    }

    // Generate random 256-bit key
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let key_bytes: [u8; 32] = rng.gen();
    let hex_key = key_bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();

    tracing::warn!(
        "Generated ephemeral encryption key - data will not persist across restarts. \
         Set DB_ENCRYPTION_KEY environment variable for persistence."
    );

    Ok(hex_key)
}
