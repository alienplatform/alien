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
    pub(crate) server_bindings: Arc<ServerBindings>,
    pub(crate) dev_status_tx: Option<tokio::sync::watch::Sender<()>>,
    pub(crate) log_buffer: Arc<LogBuffer>,
    pub(crate) command_server: Arc<alien_commands::server::CommandServer>,
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

    /// Access the deployment store.
    pub fn deployment_store(&self) -> &Arc<dyn DeploymentStore> {
        &self.deployment_store
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
        let deployment_loop =
            if !self.config.disable_deployment_loop || !self.config.disable_heartbeat_loop {
                Some(Arc::new(DeploymentLoop::new(
                    self.config.clone(),
                    self.deployment_store.clone(),
                    self.release_store.clone(),
                    self.credential_resolver.clone(),
                    self.server_bindings.clone(),
                    self.dev_status_tx,
                )))
            } else {
                None
            };

        // Spawn the deployment loop
        if !self.config.disable_deployment_loop {
            let deployment_loop = deployment_loop
                .as_ref()
                .expect("deployment loop is constructed when enabled")
                .clone();
            tokio::spawn(async move {
                deployment_loop.run().await;
            });
        } else {
            info!("Deployment loop disabled");
        }

        // Spawn the heartbeat loop
        if !self.config.disable_heartbeat_loop {
            let heartbeat_loop = HeartbeatLoop::new(
                self.config.clone(),
                self.deployment_store.clone(),
                deployment_loop
                    .expect("deployment loop processor is constructed when heartbeat is enabled"),
            );
            tokio::spawn(async move {
                heartbeat_loop.run().await;
            });
        } else {
            info!("Heartbeat loop disabled");
        }

        // Spawn the command deadline reaper: expires overdue non-terminal
        // commands (including PendingUpload ones no lease scan or status
        // poll would ever touch). Lazy enforcement still exists on the
        // status/lease paths; this loop is the backstop that guarantees
        // termination without a poller.
        {
            let command_server = self.command_server.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    interval.tick().await;
                    if let Err(e) = command_server.reap_expired_commands().await {
                        tracing::warn!(error = %e, "Command deadline reap failed");
                    }
                }
            });
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
