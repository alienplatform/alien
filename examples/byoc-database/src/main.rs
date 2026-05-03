mod error;
mod handlers;
mod models;
mod reader;
mod writer;

use crate::{
    error::{Error, Result},
    handlers::{health, query, upsert, ReaderState, WriterState},
    reader::Reader,
    writer::Writer,
};
use alien_sdk::AlienContext;
use axum::{
    routing::{get, post},
    Router,
};
use clap::Parser;
use std::{net::SocketAddr, str::FromStr, sync::Arc};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Writer,
    Reader,
}

impl FromStr for Mode {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "writer" => Ok(Mode::Writer),
            "reader" => Ok(Mode::Reader),
            _ => Err(Error::Configuration(format!(
                "Invalid mode: {}. Must be 'writer' or 'reader'",
                s
            ))),
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Mode: writer or reader
    #[arg(long)]
    mode: Option<String>,

    /// Port to listen on
    #[arg(long)]
    port: Option<u16>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .json()
                .with_ansi(false)
                .with_target(false)
                .with_current_span(false),
        )
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    tracing::info!("Starting BYOC Database");

    // Parse arguments
    let args = Args::parse();

    // Get mode from args or environment variable
    let mode_str = args
        .mode
        .or_else(|| std::env::var("BYOCDB_MODE").ok())
        .ok_or_else(|| {
            Error::Configuration(
                "Mode must be specified via --mode or BYOCDB_MODE env var".to_string(),
            )
        })?;
    let mode = Mode::from_str(&mode_str)?;

    // Get port from args or environment variable, default to 8080
    let port = args
        .port
        .or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(8080);

    tracing::info!("Running in {:?} mode on port {}", mode, port);

    // Initialize Alien context
    let ctx = AlienContext::from_env()
        .await
        .map_err(|e| Error::Configuration(format!("Failed to create Alien context: {}", e)))?;

    // Load storage binding
    let storage = ctx.get_bindings().load_storage("data").await.map_err(|e| {
        Error::Configuration(format!("Failed to load storage binding 'data': {}", e))
    })?;

    tracing::info!("Storage binding loaded: {}", storage.get_url());

    // Create router based on mode
    let app = match mode {
        Mode::Writer => {
            let writer = Arc::new(Writer::new(storage));
            let state = WriterState { writer };

            Router::new()
                .route("/health", get(health))
                .route("/api/v1/namespaces/{namespace}/upsert", post(upsert))
                .with_state(state)
        }
        Mode::Reader => {
            let reader = Arc::new(Reader::new(storage));
            let state = ReaderState { reader };

            Router::new()
                .route("/health", get(health))
                .route("/api/v1/namespaces/{namespace}/query", post(query))
                .with_state(state)
        }
    };

    // Add common middleware
    let app = app
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| Error::Configuration(format!("Failed to bind to {}: {}", addr, e)))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| Error::Generic(format!("Server error: {}", e)))?;

    Ok(())
}
