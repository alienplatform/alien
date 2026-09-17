//! Kubernetes readiness endpoint for initialized Operator processes.

use std::net::{Ipv4Addr, SocketAddr};

use alien_error::{Context, IntoAlienError};
use axum::{http::StatusCode, routing::get, Router};
use tokio_util::sync::CancellationToken;
use tracing::info;

/// Serve readiness after the caller has initialized the durable Operator identity.
pub async fn start_readiness_server(
    port: u16,
    cancel: CancellationToken,
) -> crate::error::Result<()> {
    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::ConfigurationError {
            message: format!("Failed to bind Operator readiness server on {address}"),
        })?;

    info!(%address, "Starting Operator readiness server");
    axum::serve(
        listener,
        Router::new().route("/ready", get(|| async { StatusCode::NO_CONTENT })),
    )
    .with_graceful_shutdown(cancel.cancelled_owned())
    .await
    .into_alien_error()
    .context(crate::error::ErrorData::ConfigurationError {
        message: "Operator readiness server error".to_string(),
    })?;

    Ok(())
}
