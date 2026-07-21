//! BYOC database example: writer/reader containers over durable object
//! storage (see the crate README for the architecture).
//!
//! ## Command receiver gating
//!
//! The reader container also demonstrates the app-owned pull command
//! receiver (`alien_commands::Receiver`): it registers a `stats` handler and
//! leases commands for itself alongside serving its HTTP API when command
//! receiving is enabled for the resource. A deployment with no
//! `ALIEN_COMMANDS_*` variables runs the HTTP API without a receiver. Once any
//! receiver variable is present, the complete configuration is required and
//! receiver termination stops the process rather than leaving a healthy HTTP
//! API that can no longer process commands.

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
use std::{collections::HashMap, future::Future, net::SocketAddr, str::FromStr, sync::Arc};
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
    let (app, command_receiver) = match mode {
        Mode::Writer => {
            let writer = Arc::new(Writer::new(storage));
            let state = WriterState { writer };

            (
                Router::new()
                    .route("/health", get(health))
                    .route("/api/v1/namespaces/{namespace}/upsert", post(upsert))
                    .with_state(state),
                None,
            )
        }
        Mode::Reader => {
            let reader = Arc::new(Reader::new(storage));
            let env = std::env::vars().collect();
            let mut command_receiver = command_receiver_from_env(&env)?;
            if let Some(receiver) = &mut command_receiver {
                register_stats_handler(receiver, reader.clone());
            }

            let state = ReaderState { reader };

            (
                Router::new()
                    .route("/health", get(health))
                    .route("/api/v1/namespaces/{namespace}/query", post(query))
                    .with_state(state),
                command_receiver,
            )
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

    let server = async move {
        axum::serve(listener, app)
            .await
            .map_err(|error| Error::Generic(format!("Server error: {error}")))
    };
    let command_receiver = command_receiver.map(|receiver| async move {
        receiver
            .run()
            .await
            .map_err(|error| Error::CommandReceiver(error.to_string()))
    });

    supervise_services(server, command_receiver).await
}

/// Builds the pull command receiver when command configuration is present.
///
/// No `ALIEN_COMMANDS_*` variables means commands are intentionally disabled.
/// If any such variable is injected, the receiver validates the complete
/// configuration and startup fails on missing or invalid values.
fn command_receiver_from_env(
    env: &HashMap<String, String>,
) -> Result<Option<alien_commands::Receiver>> {
    if !env.keys().any(|key| key.starts_with("ALIEN_COMMANDS_")) {
        tracing::info!("Command receiver not configured; skipping receiver startup");
        return Ok(None);
    }

    let receiver = alien_commands::Receiver::from_env_vars(env).map_err(|error| {
        Error::Configuration(format!("Invalid command receiver configuration: {error}"))
    })?;

    Ok(Some(receiver))
}

fn register_stats_handler(receiver: &mut alien_commands::Receiver, reader: Arc<Reader>) {
    receiver.command("stats", move |request: StatsRequest, _ctx| {
        let reader = reader.clone();
        async move {
            let stats = reader.stats(&request.namespace).await?;
            Ok(stats)
        }
    });
}

/// Runs the HTTP server and optional receiver as one service. If the receiver
/// terminates, the process fails instead of continuing with a command-dead
/// health endpoint.
async fn supervise_services<Server, CommandReceiver>(
    server: Server,
    command_receiver: Option<CommandReceiver>,
) -> Result<()>
where
    Server: Future<Output = Result<()>>,
    CommandReceiver: Future<Output = Result<()>>,
{
    let Some(command_receiver) = command_receiver else {
        return server.await;
    };

    tokio::select! {
        result = server => result,
        result = command_receiver => match result {
            Ok(()) => Err(Error::CommandReceiver(
                "receiver stopped unexpectedly".to_string(),
            )),
            Err(error) => Err(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_command_environment_disables_receiver() {
        let env = HashMap::from([(
            "ALIEN_DEPLOYMENT_ID".to_string(),
            "deployment-without-commands".to_string(),
        )]);

        let receiver = command_receiver_from_env(&env).expect("absent optional config is valid");

        assert!(receiver.is_none());
    }

    #[test]
    fn partial_command_environment_fails_startup() {
        let env = HashMap::from([(
            "ALIEN_COMMANDS_URL".to_string(),
            "https://commands.example.com/v1/".to_string(),
        )]);

        let error =
            command_receiver_from_env(&env).expect_err("partial command config must fail startup");

        assert!(
            matches!(error, Error::Configuration(message) if message.contains("ALIEN_COMMANDS_TOKEN"))
        );
    }

    #[test]
    fn invalid_command_environment_fails_startup() {
        let env = HashMap::from([
            ("ALIEN_COMMANDS_URL".to_string(), "not a URL".to_string()),
            ("ALIEN_COMMANDS_TOKEN".to_string(), "token".to_string()),
            ("ALIEN_DEPLOYMENT_ID".to_string(), "deployment".to_string()),
            (
                "ALIEN_COMMANDS_TARGET_RESOURCE_ID".to_string(),
                "reader".to_string(),
            ),
            (
                "ALIEN_COMMANDS_TARGET_RESOURCE_TYPE".to_string(),
                "container".to_string(),
            ),
        ]);

        let error =
            command_receiver_from_env(&env).expect_err("invalid command config must fail startup");

        assert!(
            matches!(error, Error::Configuration(message) if message.contains("ALIEN_COMMANDS_URL"))
        );
    }

    #[tokio::test]
    async fn terminal_receiver_error_fails_the_service() {
        let server = std::future::pending::<Result<()>>();
        let receiver = async {
            Err(Error::CommandReceiver(
                "terminal receiver failure".to_string(),
            ))
        };

        let error = supervise_services(server, Some(receiver))
            .await
            .expect_err("receiver failure must stop the service");

        assert!(
            matches!(error, Error::CommandReceiver(message) if message == "terminal receiver failure")
        );
    }
}
