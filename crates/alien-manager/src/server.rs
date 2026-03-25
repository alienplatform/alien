//! The assembled alien-manager, ready to start.

use std::net::SocketAddr;
use std::sync::Arc;

use alien_error::{Context, IntoAlienError};
use tokio::net::TcpListener;
use tracing::info;

use crate::config::ManagerConfig;
use crate::dev::LogBuffer;
use crate::error::ErrorData;
use crate::loops::{DeploymentLoop, HeartbeatLoop};
use crate::traits::*;

/// A fully-configured alien-manager instance.
pub struct AlienManager {
    pub(crate) config: Arc<ManagerConfig>,
    pub(crate) router: axum::Router,
    pub(crate) deployment_store: Arc<dyn DeploymentStore>,
    pub(crate) release_store: Arc<dyn ReleaseStore>,
    pub(crate) credential_resolver: Arc<dyn CredentialResolver>,
    pub(crate) telemetry_backend: Arc<dyn TelemetryBackend>,
    pub(crate) server_bindings: Arc<ServerBindings>,
    pub(crate) dev_status_tx: Option<tokio::sync::watch::Sender<()>>,
    pub(crate) log_buffer: Arc<LogBuffer>,
}

impl AlienManager {
    /// Create a builder for configuring the server.
    pub fn builder(config: ManagerConfig) -> crate::builder::AlienManagerBuilder {
        crate::builder::AlienManagerBuilder::new(config)
    }

    /// Access the server config.
    pub fn config(&self) -> &ManagerConfig {
        &self.config
    }

    /// Access the log buffer (useful for dev mode UI).
    pub fn log_buffer(&self) -> &Arc<LogBuffer> {
        &self.log_buffer
    }

    /// Start the HTTP server and background loops.
    ///
    /// This method spawns the deployment loop and heartbeat loop as background
    /// tasks, then runs the axum HTTP server. It blocks until the server shuts down.
    pub async fn start(self, addr: SocketAddr) -> crate::error::Result<()> {
        // Spawn the deployment loop
        if !self.config.disable_deployment_loop {
            let deployment_loop = DeploymentLoop::new(
                self.config.clone(),
                self.deployment_store.clone(),
                self.release_store.clone(),
                self.credential_resolver.clone(),
                self.telemetry_backend.clone(),
                self.server_bindings.clone(),
                self.dev_status_tx,
            );
            tokio::spawn(async move {
                deployment_loop.run().await;
            });
        } else {
            info!("Deployment loop disabled");
        }

        // Spawn the heartbeat loop
        if !self.config.disable_heartbeat_loop {
            let heartbeat_loop = HeartbeatLoop::new(self.config.clone(), self.deployment_store.clone());
            tokio::spawn(async move {
                heartbeat_loop.run().await;
            });
        } else {
            info!("Heartbeat loop disabled");
        }

        // Start the HTTP server
        let listener = TcpListener::bind(addr).await.into_alien_error().context(
            ErrorData::ServerInitFailed {
                reason: format!("Failed to bind to {}", addr),
            },
        )?;

        info!(%addr, "alien-manager listening");

        axum::serve(listener, self.router)
            .await
            .into_alien_error()
            .context(ErrorData::InternalError {
                message: "Server error".to_string(),
            })?;

        Ok(())
    }
}
