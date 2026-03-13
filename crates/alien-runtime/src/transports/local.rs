//! Local Platform Transport
//!
//! A minimal HTTP proxy transport for the Local Platform. Forwards all HTTP
//! requests to the application's registered HTTP server without any
//! platform-specific middleware (no CloudEvents parsing, no scheduler handling).
//!
//! Used for:
//! - Local development (`acme run`)
//! - Production deployments on VMs, bare metal, edge devices
//! - Any environment where the runtime manages HTTP routing

use std::net::SocketAddr;
use std::sync::Arc;

use alien_bindings::grpc::control_service::ControlGrpcServer;
use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use super::shared::forward_http_request;
use crate::error::{ErrorData, Result};
use alien_error::AlienError;

/// Local platform transport.
///
/// Simple HTTP proxy that forwards all requests to the application.
/// No CloudEvents parsing, no platform-specific middleware.
pub struct LocalTransport {
    port: u16,
    #[allow(dead_code)]
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    shutdown_rx: broadcast::Receiver<()>,
}

impl LocalTransport {
    pub fn new(
        port: u16,
        control_server: Arc<ControlGrpcServer>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            port,
            control_server,
            app_http_port: None,
            shutdown_rx,
        }
    }

    pub fn with_app_port(mut self, port: u16) -> Self {
        self.app_http_port = Some(port);
        self
    }

    /// Run the transport.
    pub async fn run(mut self) -> Result<()> {
        let addr = SocketAddr::from(([0, 0, 0, 0], self.port));

        info!(port = self.port, "Starting Local transport");

        let state = TransportState {
            app_http_port: self.app_http_port,
        };

        let app = Router::new()
            .route("/{*path}", any(handle_request))
            .route("/", any(handle_request))
            .with_state(state);

        let listener = TcpListener::bind(addr).await.map_err(|e| {
            AlienError::new(ErrorData::TransportStartupFailed {
                transport_name: "local".to_string(),
                message: format!("Failed to bind to {}: {}", addr, e),
                address: Some(addr.to_string()),
            })
        })?;

        info!(addr = %addr, "Local transport listening");

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                self.shutdown_rx.recv().await.ok();
                info!("Local transport received shutdown signal");
            })
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::TransportStartupFailed {
                    transport_name: "local".to_string(),
                    message: format!("Server error: {}", e),
                    address: Some(addr.to_string()),
                })
            })?;

        info!("Local transport shutdown complete");
        Ok(())
    }
}

#[derive(Clone)]
struct TransportState {
    app_http_port: Option<u16>,
}

async fn handle_request(
    State(state): State<TransportState>,
    request: Request<Body>,
) -> Response<Body> {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    debug!(path = %path, method = %method, "Received request");

    // Forward HTTP request to app
    if let Some(app_port) = state.app_http_port {
        return forward_http_request(request, app_port).await;
    }

    error!("No app HTTP port registered");
    (StatusCode::SERVICE_UNAVAILABLE, "No application registered").into_response()
}
