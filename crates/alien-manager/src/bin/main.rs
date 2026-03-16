//! Standalone alien-manager binary.
//!
//! Reads configuration from environment variables, initializes default providers,
//! and starts the HTTP server with deployment and heartbeat loops.

use std::net::SocketAddr;
use std::path::PathBuf;

use alien_manager::{AlienManager, ManagerConfig};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "alien_manager=info".into()),
        )
        .init();

    let config = config_from_env();

    let addr: SocketAddr = SocketAddr::from(([0, 0, 0, 0], config.port));

    let server = AlienManager::builder(config)
        .build()
        .await
        .expect("Failed to build alien-manager");

    // If not dev mode and no tokens exist yet, create an initial admin token
    // and print it to stdout so the operator can save it.
    if !server.config().dev_mode {
        print_initial_admin_token(&server).await;
    }

    server.start(addr).await.expect("Server exited with error");
}

fn config_from_env() -> ManagerConfig {
    let port = std::env::var("PORT")
        .or_else(|_| std::env::var("ALIEN_SERVER_PORT"))
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let db_path = Some(
        std::env::var("ALIEN_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("alien-manager.db")),
    );

    let state_dir = Some(
        std::env::var("ALIEN_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".alien-manager")),
    );

    let deployment_interval_secs = std::env::var("ALIEN_DEPLOYMENT_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    let heartbeat_interval_secs = std::env::var("ALIEN_HEARTBEAT_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    let otlp_endpoint = std::env::var("OTLP_ENDPOINT").ok();

    let dev_mode = std::env::var("ALIEN_DEV_MODE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let base_url = std::env::var("BASE_URL").ok();

    ManagerConfig {
        port,
        db_path,
        state_dir,
        deployment_interval_secs,
        heartbeat_interval_secs,
        otlp_endpoint,
        dev_mode,
        base_url,
    }
}

async fn print_initial_admin_token(server: &AlienManager) {
    // This is a best-effort helper. If listing tokens fails, we skip it silently.
    // The operator can always create tokens via the API later.
    let _ = async {
        // We don't have direct access to token_store from the public API,
        // but the server is not started yet. For the standalone binary,
        // we rely on the user creating tokens via the API after startup.
        //
        // TODO: Consider exposing a helper to bootstrap an admin token
        // on first run when no tokens exist.
        tracing::info!(
            "alien-manager started in production mode. \
             Use the API to create admin tokens for authentication."
        );
    }
    .await;
}
