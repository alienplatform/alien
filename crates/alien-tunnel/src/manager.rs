//! Manager end: tracks open operator connections and sends requests into them.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use alien_error::{AlienError, Context, IntoAlienError};
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::combinators::UnsyncBoxBody;
use hyper::{body::Incoming, client::conn::http2};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, info};

use crate::{h2_settings, ErrorData, Result};

/// Boxed error used by tunnel request bodies.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Request body sent into a tunnel.
pub type TunnelBody = UnsyncBoxBody<Bytes, BoxError>;

struct Connection {
    id: u64,
    sender: http2::SendRequest<TunnelBody>,
}

/// Open tunnel connections, keyed by deployment ID.
///
/// Held in memory by the manager process that accepted the connections. A
/// manager running several replicas needs requests for a deployment to reach
/// the replica holding its connection.
#[derive(Default)]
pub struct TunnelRegistry {
    connections: Mutex<HashMap<String, Vec<Connection>>>,
    next_id: AtomicU64,
}

impl TunnelRegistry {
    /// Create an empty registry.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Serve one operator connection until it closes.
    ///
    /// `io` is the byte stream of an accepted tunnel WebSocket (see
    /// [`crate::websocket_io`]). The connection is usable for requests as soon
    /// as the HTTP/2 handshake completes and is removed when it ends.
    pub async fn serve<IO>(&self, deployment_id: String, io: IO) -> Result<()>
    where
        IO: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let (sender, connection) = http2::Builder::new(TokioExecutor::new())
            .timer(TokioTimer::new())
            .initial_stream_window_size(h2_settings::INITIAL_STREAM_WINDOW)
            .initial_connection_window_size(h2_settings::INITIAL_CONNECTION_WINDOW)
            .keep_alive_interval(h2_settings::KEEPALIVE_INTERVAL)
            .keep_alive_timeout(h2_settings::KEEPALIVE_TIMEOUT)
            .keep_alive_while_idle(true)
            .handshake(TokioIo::new(io))
            .await
            .into_alien_error()
            .context(ErrorData::ConnectionFailed {
                message: format!("HTTP/2 handshake with deployment '{deployment_id}'"),
            })?;

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let open = {
            let mut connections = self.lock();
            let entry = connections.entry(deployment_id.clone()).or_default();
            entry.push(Connection { id, sender });
            entry.len()
        };
        info!(deployment_id = %deployment_id, connection_id = id, open, "Tunnel connection opened");
        // Deregisters when the connection ends or this future is dropped.
        let _registration = Registration {
            registry: self,
            deployment_id: &deployment_id,
            id,
        };

        let result = connection.await;

        result
            .into_alien_error()
            .context(ErrorData::ConnectionFailed {
                message: format!("connection from deployment '{deployment_id}'"),
            })
    }

    /// Number of open connections for a deployment.
    pub fn connection_count(&self, deployment_id: &str) -> usize {
        self.lock().get(deployment_id).map_or(0, Vec::len)
    }

    /// Send a request to a deployment and return the streaming response.
    ///
    /// The request URI's authority names the tunnel target inside the
    /// deployment; the operator resolves it against the stack.
    pub async fn send(
        &self,
        deployment_id: &str,
        request: Request<TunnelBody>,
    ) -> Result<Response<Incoming>> {
        let mut sender = self.pick(deployment_id).ok_or_else(|| {
            AlienError::new(ErrorData::NotConnected {
                deployment_id: deployment_id.to_string(),
            })
        })?;
        let request_failed = |message: &str| ErrorData::RequestFailed {
            deployment_id: deployment_id.to_string(),
            message: message.to_string(),
        };
        sender
            .ready()
            .await
            .into_alien_error()
            .context(request_failed("connection is not usable"))?;
        debug!(deployment_id, uri = %request.uri(), "Sending request over tunnel");
        sender
            .send_request(request)
            .await
            .into_alien_error()
            .context(request_failed("request failed in transit"))
    }

    /// Spread requests over a deployment's open connections.
    fn pick(&self, deployment_id: &str) -> Option<http2::SendRequest<TunnelBody>> {
        let connections = self.lock();
        let open: Vec<_> = connections
            .get(deployment_id)?
            .iter()
            .filter(|c| !c.sender.is_closed())
            .collect();
        if open.is_empty() {
            return None;
        }
        let index = self.next_id.fetch_add(1, Ordering::Relaxed) as usize % open.len();
        Some(open[index].sender.clone())
    }

    fn deregister(&self, deployment_id: &str, id: u64) {
        let mut connections = self.lock();
        let open = match connections.get_mut(deployment_id) {
            Some(entry) => {
                entry.retain(|c| c.id != id);
                entry.len()
            }
            None => 0,
        };
        if open == 0 {
            connections.remove(deployment_id);
        }
        info!(
            deployment_id,
            connection_id = id,
            open,
            "Tunnel connection closed"
        );
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<Connection>>> {
        self.connections
            .lock()
            .expect("tunnel registry mutex is never held across a panic")
    }
}

struct Registration<'a> {
    registry: &'a TunnelRegistry,
    deployment_id: &'a str,
    id: u64,
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.registry.deregister(self.deployment_id, self.id);
    }
}
