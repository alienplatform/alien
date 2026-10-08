//! The assembled alien-manager, ready to start.

use std::future::{pending, Future};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use alien_error::{Context, IntoAlienError};
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::{net::TcpListener, task::JoinSet};
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
        let listener = TcpListener::bind(addr).await.into_alien_error().context(
            ErrorData::ServerInitFailed {
                reason: format!("Failed to bind to {}", addr),
            },
        )?;
        self.start_with_listener(listener).await
    }

    /// Start with caller-owned shutdown, finishing reconciliation and draining native runtimes.
    pub async fn start_with_shutdown(
        self,
        addr: SocketAddr,
        shutdown: impl Future<Output = ()> + Send,
    ) -> crate::error::Result<()> {
        let listener = TcpListener::bind(addr).await.into_alien_error().context(
            ErrorData::ServerInitFailed {
                reason: format!("Failed to bind to {addr}"),
            },
        )?;
        self.start_with_listener_and_shutdown(listener, shutdown)
            .await
    }

    /// Start using an already-bound listener, retaining ownership of its reserved port.
    pub async fn start_with_listener(self, listener: TcpListener) -> crate::error::Result<()> {
        self.start_with_listener_and_shutdown(listener, pending())
            .await
    }

    async fn start_with_listener_and_shutdown(
        self,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> crate::error::Result<()> {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let mut tasks = JoinSet::new();
        let addr =
            listener
                .local_addr()
                .into_alien_error()
                .context(ErrorData::ServerInitFailed {
                    reason: "Failed to read the manager listener address".to_string(),
                })?;
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
            let shutdown = shutdown_rx.clone();
            tasks.spawn(async move {
                deployment_loop.run_until_shutdown(shutdown).await;
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
                    .as_ref()
                    .expect("deployment loop processor is constructed when heartbeat is enabled")
                    .clone(),
            );
            let shutdown = shutdown_rx.clone();
            tasks.spawn(async move {
                heartbeat_loop.run_until_shutdown(shutdown).await;
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
            let mut shutdown = shutdown_rx;
            tasks.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tokio::select! {
                        _ = shutdown.changed() => break,
                        _ = interval.tick() => {}
                    }
                    if let Err(e) = command_server.reap_expired_commands().await {
                        tracing::warn!(error = %e, "Command deadline reap failed");
                    }
                }
            });
        }

        info!(%addr, "alien-manager listening");

        // Keep HTTP available while native runtimes drain their accepted work.
        let mut server = JoinSet::new();
        server.spawn(serve(listener, self.router, INBOUND_IDLE_TIMEOUT));
        let server_result = tokio::select! {
            result = server.join_next() => Some(result.expect("HTTP task is owned until completion")),
            _ = shutdown => None,
        };
        let _ = shutdown_tx.send(true);
        let mut task_error = None;
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result {
                task_error = Some(error);
            }
        }
        if let Some(processor) = deployment_loop {
            processor.shutdown_local_runtimes().await;
        }
        if let Some(result) = server_result {
            result
                .into_alien_error()
                .context(ErrorData::InternalError {
                    message: "Server task failed".to_string(),
                })?
                .into_alien_error()
                .context(ErrorData::InternalError {
                    message: "Server error".to_string(),
                })?;
        } else {
            server.shutdown().await;
        }
        if let Some(error) = task_error {
            return Err(error)
                .into_alien_error()
                .context(ErrorData::InternalError {
                    message: "Manager background task failed".to_string(),
                });
        }

        Ok(())
    }
}

/// How long an inbound keep-alive connection may sit without sending a request before the
/// server closes it. Without this, every connection a client opens and forgets stays open
/// for the life of the process, along with its read buffer. Kept above the load balancer's
/// 60s idle timeout so the balancer never reuses a connection the server has just closed.
const INBOUND_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Serve `router` on `listener` over HTTP/1, closing connections that stay idle longer than
/// `idle_timeout`. `axum::serve` sets no timer, so it never closes an idle connection.
///
/// HTTP/1 only: every client of the manager (the load balancer, browsers, fetch, reqwest)
/// speaks HTTP/1.1, and hyper's HTTP/1 header timeout covers a connection from its first byte,
/// so a client that connects and sends nothing is closed too. Protocol auto-detection would
/// wait for that first byte without any timeout.
pub(crate) async fn serve(
    listener: TcpListener,
    router: axum::Router,
    idle_timeout: Duration,
) -> std::io::Result<()> {
    let mut builder = http1::Builder::new();
    // hyper only enforces the header read timeout once it has a timer. The timeout also runs
    // between requests on a keep-alive connection, which is what makes it an idle timeout.
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(idle_timeout);
    let builder = Arc::new(builder);

    let mut connections = JoinSet::new();
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            completed = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = completed {
                    return Err(std::io::Error::other(format!("Inbound connection task failed: {error}")));
                }
                continue;
            }
        };
        let stream = match accepted {
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
        connections.spawn(async move {
            // `with_upgrades` keeps WebSocket upgrades (debug sessions) working.
            if let Err(error) = builder
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
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
    use crate::standalone_config::ManagerTomlConfig;
    use axum::{routing::get, Router};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const REQUEST: &[u8] = b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n";

    /// Reads one complete HTTP/1.1 response (headers plus `content-length` body) so no bytes
    /// of it are left behind to be mistaken for the next response or for a closed socket.
    async fn read_response(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut chunk))
                .await
                .expect("response should arrive before the deadline")
                .expect("read should succeed");
            assert!(n > 0, "server closed the connection instead of responding");
            bytes.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&bytes);
            let Some(header_end) = text.find("\r\n\r\n") else {
                continue;
            };
            let content_length: usize = text[..header_end]
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .expect("response should carry content-length")
                .parse()
                .expect("content-length should be a number");
            if bytes.len() >= header_end + 4 + content_length {
                return text.into_owned();
            }
        }
    }

    #[tokio::test]
    async fn caller_shutdown_finishes_loops_and_closes_listener_and_connections() {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let config = ManagerConfig {
            db_path: Some(directory.path().join("manager.db")),
            state_dir: Some(directory.path().to_path_buf()),
            deployment_interval_secs: 3600,
            heartbeat_interval_secs: 3600,
            response_signing_key: b"test-response-signing-key".to_vec(),
            ..Default::default()
        };
        let server = AlienManager::builder(config)
            .with_standalone_defaults(&ManagerTomlConfig::default())
            .await
            .unwrap()
            .build()
            .await
            .unwrap();
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(server.start_with_listener_and_shutdown(listener, async {
            let _ = receiver.await;
        }));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(REQUEST).await.unwrap();
        assert!(read_response(&mut stream).await.starts_with("HTTP/1.1 200"));
        shutdown.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("shutdown must interrupt long loop intervals")
            .unwrap()
            .unwrap();
        assert!(
            TcpStream::connect(addr).await.is_err(),
            "listener must close"
        );
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0,
            "existing keep-alive connection must close"
        );
    }

    #[tokio::test]
    async fn aborting_manager_closes_listener_and_existing_connections() {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let config = ManagerConfig {
            db_path: Some(directory.path().join("manager.db")),
            state_dir: Some(directory.path().to_path_buf()),
            response_signing_key: b"test-response-signing-key".to_vec(),
            ..Default::default()
        };
        let server = AlienManager::builder(config)
            .with_standalone_defaults(&ManagerTomlConfig::default())
            .await
            .unwrap()
            .build()
            .await
            .unwrap();
        let task = tokio::spawn(server.start_with_listener_and_shutdown(listener, pending()));
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(REQUEST).await.unwrap();
        assert!(read_response(&mut stream).await.starts_with("HTTP/1.1 200"));
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
                .await
                .expect("aborting the manager must close its connections")
                .unwrap(),
            0,
        );
        assert!(
            TcpStream::connect(addr).await.is_err(),
            "listener must close"
        );
        TcpListener::bind(addr)
            .await
            .expect("manager port must be released");
    }

    #[tokio::test]
    async fn idle_keep_alive_connection_is_closed_after_the_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let router = Router::new().route("/health", get(|| async { "ok" }));
        let idle_timeout = Duration::from_millis(300);
        tokio::spawn(serve(listener, router, idle_timeout));

        // A connection that never sends a byte is closed too: the timeout covers the first
        // request, not only the gaps between requests.
        let mut silent = TcpStream::connect(addr).await.expect("connect");
        let mut buf = [0u8; 16];
        let closed = tokio::time::timeout(idle_timeout * 5, silent.read(&mut buf))
            .await
            .expect("server should close the silent connection within the timeout")
            .expect("read should return EOF, not an error");
        assert_eq!(
            closed, 0,
            "expected EOF from the server closing the silent connection"
        );

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
