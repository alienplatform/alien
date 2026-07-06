//! BYOC database example: writer/reader containers over durable object
//! storage (see the crate README for the architecture).
//!
//! ## Command receiver gating (ALIEN-221)
//!
//! The reader container also demonstrates the app-owned pull command
//! receiver (`alien_commands::Receiver`): it registers a `stats` handler and
//! leases commands for itself alongside serving its HTTP API. Because the
//! receiver's environment (`ALIEN_COMMANDS_URL` and friends) isn't injected
//! by the platform until a later task, [`spawn_command_receiver`] treats a
//! missing/invalid receiver environment as "not configured" rather than a
//! fatal error: it logs and returns, leaving the HTTP API fully functional.
//! This keeps the example runnable today and automatically picks up real
//! command leasing once the platform wires injection — no code change
//! needed here.

mod error;
mod handlers;
mod models;
mod reader;
mod writer;

use crate::{
    error::{Error, Result},
    handlers::{health, query, upsert, ReaderState, WriterState},
    models::StatsRequest,
    reader::Reader,
    writer::Writer,
};
use alien_sdk::Bindings;
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

    // Load bindings configured via `ALIEN_*_BINDING` environment variables. This is a
    // long-running, resident process (no Worker event handlers), so it talks to bindings
    // directly rather than through `AlienContext`'s event loop.
    let bindings = Bindings::from_env()
        .map_err(|e| Error::Configuration(format!("Failed to load bindings: {}", e)))?;

    // Load storage binding
    let storage = bindings.storage("data").await.map_err(|e| {
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

            // Spawns as a background task and returns immediately; see the
            // module doc note above for why a missing receiver environment
            // is not fatal here.
            spawn_command_receiver(reader.clone());

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

/// Registers the `stats` command handler and starts the pull command
/// receiver as a background task, alongside the axum server started by
/// `main`.
///
/// Gated (see the module doc note): if the receiver's environment
/// (`ALIEN_COMMANDS_URL` and friends) is absent or invalid,
/// `Receiver::from_env()` returns an error, which is logged and swallowed
/// here rather than propagated — the container keeps running its HTTP API
/// either way. This keeps the example runnable before the platform wires
/// receiver-env injection for this resource (a later ALIEN-221 task); once
/// injection lands, the same container starts leasing commands with no
/// code change.
fn spawn_command_receiver(reader: Arc<Reader>) {
    let mut receiver = match alien_commands::Receiver::from_env() {
        Ok(receiver) => receiver,
        Err(error) => {
            tracing::info!(
                %error,
                "Command receiver environment not configured; skipping receiver startup"
            );
            return;
        }
    };

    receiver.handle("stats", move |ctx| {
        let reader = reader.clone();
        async move {
            let request: StatsRequest = ctx.input_json()?;
            let stats = reader.stats(&request.namespace).await?;
            Ok(stats)
        }
    });

    tokio::spawn(async move {
        if let Err(error) = receiver.run().await {
            tracing::error!(%error, "Command receiver stopped with an error");
        }
    });
}
