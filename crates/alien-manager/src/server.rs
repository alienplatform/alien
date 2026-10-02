//! The assembled alien-manager, ready to start.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use alien_error::{Context, IntoAlienError};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tracing::{debug, info, warn};

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

        serve(listener, self.router, INBOUND_IDLE_TIMEOUT)
            .await
            .into_alien_error()
            .context(ErrorData::InternalError {
                message: "Server error".to_string(),
            })?;

        Ok(())
    }
}

/// How long an inbound keep-alive connection may sit without sending a request before the
/// server closes it. Without this, every connection a client opens and forgets stays open
/// for the life of the process, along with its read buffer. Kept above the load balancer's
/// 60s idle timeout so the balancer never reuses a connection the server has just closed.
const INBOUND_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Serve `router` on `listener`, closing connections that stay idle longer than
/// `idle_timeout`. `axum::serve` sets no timer, so it never closes an idle connection.
pub(crate) async fn serve(
    listener: TcpListener,
    router: axum::Router,
    idle_timeout: Duration,
) -> std::io::Result<()> {
    let mut builder = auto::Builder::new(TokioExecutor::new());
    // hyper only enforces the header read timeout once it has a timer; it also runs between
    // requests on a keep-alive connection, which is what makes it an idle timeout.
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(idle_timeout);
    let builder = Arc::new(builder);

    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(error) => {
                // Typically fd exhaustion; the listener is still valid, so keep serving.
                warn!(%error, "Failed to accept inbound connection");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let service = TowerToHyperService::new(router.clone());
        let builder = builder.clone();
        tokio::spawn(async move {
            if let Err(error) = builder
                .serve_connection_with_upgrades(TokioIo::new(stream), service)
                .await
            {
                debug!(%error, "Inbound connection ended with an error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const REQUEST: &[u8] = b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n";

    async fn read_response(stream: &mut TcpStream) -> String {
        let mut buf = vec![0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf))
            .await
            .expect("response should arrive before the deadline")
            .expect("read should succeed");
        assert!(n > 0, "server closed the connection instead of responding");
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[tokio::test]
    async fn idle_keep_alive_connection_is_closed_after_the_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let router = Router::new().route("/health", get(|| async { "ok" }));
        let idle_timeout = Duration::from_millis(300);
        tokio::spawn(serve(listener, router, idle_timeout));

        let mut stream = TcpStream::connect(addr).await.expect("connect");

        // Two requests inside the idle window reuse the connection.
        stream.write_all(REQUEST).await.expect("write");
        assert!(read_response(&mut stream).await.starts_with("HTTP/1.1 200"));
        tokio::time::sleep(idle_timeout / 2).await;
        stream.write_all(REQUEST).await.expect("write");
        assert!(read_response(&mut stream).await.starts_with("HTTP/1.1 200"));

        // Silence for longer than the window: the server closes the connection (EOF).
        let mut buf = [0u8; 16];
        let closed = tokio::time::timeout(idle_timeout * 5, stream.read(&mut buf))
            .await
            .expect("server should close the idle connection within the timeout")
            .expect("read should return EOF, not an error");
        assert_eq!(
            closed, 0,
            "expected EOF from the server closing the idle connection"
        );
    }
}
