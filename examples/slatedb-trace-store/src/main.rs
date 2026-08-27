#![allow(clippy::result_large_err)]

mod error;
mod keys;
mod models;
mod service;
mod store;

use crate::{
    error::{ErrorData, Result},
    service::{
        get_trace, health, ingest, list_traces, open_writer, run_writer, writer_health, ApiState,
        WriterHealth,
    },
};
use alien_error::{Context, IntoAlienError};
use alien_sdk::Bindings;
use axum::{routing::get, Router};
use std::{net::SocketAddr, str::FromStr, sync::Arc};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Debug, Clone, Copy)]
enum Mode {
    Api,
    Writer,
}

impl FromStr for Mode {
    type Err = crate::error::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "api" => Ok(Self::Api),
            "writer" => Ok(Self::Writer),
            other => Err(alien_error::AlienError::new(
                ErrorData::ConfigurationInvalid {
                    message: format!("TRACE_STORE_MODE must be 'api' or 'writer', got '{other}'"),
                },
            )),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
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

    let mode = std::env::var("TRACE_STORE_MODE")
        .into_alien_error()
        .context(ErrorData::ConfigurationInvalid {
            message: "TRACE_STORE_MODE is required".to_string(),
        })?
        .parse()?;
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .into_alien_error()
        .context(ErrorData::ConfigurationInvalid {
            message: "PORT must be a valid TCP port".to_string(),
        })?;
    let bindings = Bindings::from_env().context(ErrorData::BindingOperationFailed {
        operation: "load bindings".to_string(),
    })?;
    let storage = bindings
        .storage("data")
        .await
        .context(ErrorData::BindingOperationFailed {
            operation: "load data storage".to_string(),
        })?;
    let queue = bindings
        .queue("ingestion")
        .await
        .context(ErrorData::BindingOperationFailed {
            operation: "load ingestion queue".to_string(),
        })?;

    let (app, writer) = match mode {
        Mode::Api => {
            let state = Arc::new(ApiState::new(storage, queue));
            (
                Router::new()
                    .route("/health", get(health))
                    .route("/v1/traces", get(list_traces).post(ingest))
                    .route("/v1/traces/{trace_id}", get(get_trace))
                    .with_state(state),
                None,
            )
        }
        Mode::Writer => {
            let (writer, storage, queue) = open_writer(storage, queue).await?;
            let health = Arc::new(WriterHealth::new());
            (
                Router::new()
                    .route("/health", get(writer_health))
                    .with_state(Arc::clone(&health)),
                Some(run_writer(writer, storage, queue, health)),
            )
        }
    };
    let app = app.layer(TraceLayer::new_for_http());
    let address = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationInvalid {
            message: format!("could not bind HTTP server to {address}"),
        })?;
    let server = axum::serve(listener, app);
    tracing::info!(%address, ?mode, "trace-history service started");

    if let Some(writer) = writer {
        tokio::select! {
            result = server => result.into_alien_error().context(ErrorData::ConfigurationInvalid {
                message: "HTTP server stopped".to_string(),
            }),
            result = writer => result,
        }
    } else {
        server
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationInvalid {
                message: "HTTP server stopped".to_string(),
            })
    }
}
