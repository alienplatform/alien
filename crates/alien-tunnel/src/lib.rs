//! Reverse HTTP tunnel between a manager and the deployments it manages.
//!
//! Deployments often run where nothing can connect in: a customer's
//! Kubernetes cluster that only allows outbound HTTPS. The operator inside the
//! deployment opens a WebSocket to the manager and runs an HTTP/2 **server**
//! over it; the manager runs the HTTP/2 **client**. Requests sent to the
//! manager for that deployment travel over the operator's outbound connection
//! and are forwarded to an in-cluster service declared in the stack.
//!
//! HTTP/2 supplies what a tunnel needs without a custom protocol: streaming
//! bodies in both directions, per-stream and per-connection flow control
//! (backpressure end to end), many concurrent requests on one connection,
//! cancellation, keepalive pings and graceful shutdown. The WebSocket only
//! carries bytes, so the tunnel works through proxies and load balancers that
//! allow WebSockets.
//!
//! ```text
//! caller ──HTTPS──▶ manager ══ WebSocket (HTTP/2 inside) ══ operator ──HTTP──▶ service
//!                   h2 client        opened by the operator      h2 server
//! ```

mod error;
mod ws_io;

#[cfg(feature = "manager")]
pub mod manager;
#[cfg(feature = "operator")]
pub mod operator;

use std::time::Duration;

pub use error::{ErrorData, Result};
pub use ws_io::websocket_io;

/// Path on the manager where operators open tunnel connections.
pub const CONNECT_PATH: &str = "/v1/tunnel/connect";

/// WebSocket subprotocol spoken over tunnel connections.
pub const SUBPROTOCOL: &str = "alien-tunnel.v1";

/// Response header set on errors produced by the tunnel itself (not by the
/// service behind it), carrying the error code.
pub const ERROR_HEADER: &str = "alien-tunnel-error";

/// HTTP/2 settings shared by both ends.
///
/// Windows bound how much data is in flight per request and per connection;
/// large uploads and downloads stream at the pace of the slowest hop instead
/// of being buffered in the manager or operator.
pub(crate) mod h2_settings {
    use super::Duration;

    pub const INITIAL_STREAM_WINDOW: u32 = 2 * 1024 * 1024;
    pub const INITIAL_CONNECTION_WINDOW: u32 = 16 * 1024 * 1024;
    pub const MAX_CONCURRENT_STREAMS: u32 = 512;
    /// Below common 60s idle timeouts of load balancers and proxies.
    pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);
    pub const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(20);
}
