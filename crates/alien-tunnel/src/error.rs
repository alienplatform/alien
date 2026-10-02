use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Result type for tunnel operations.
pub type Result<T> = alien_error::Result<T, ErrorData>;

/// Errors raised by either end of the tunnel.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The deployment has no open tunnel connection to this manager.
    #[error(
        code = "TUNNEL_NOT_CONNECTED",
        message = "Deployment '{deployment_id}' has no open tunnel connection",
        retryable = "true",
        internal = "false",
        http_status_code = 503
    )]
    NotConnected {
        /// Deployment the caller addressed.
        deployment_id: String,
    },

    /// The request could not be carried over the tunnel connection.
    #[error(
        code = "TUNNEL_REQUEST_FAILED",
        message = "Tunnel request to deployment '{deployment_id}' failed: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    RequestFailed {
        /// Deployment the caller addressed.
        deployment_id: String,
        /// What went wrong.
        message: String,
    },

    /// The operator could not open a tunnel connection to the manager.
    #[error(
        code = "TUNNEL_CONNECT_FAILED",
        message = "Failed to open tunnel connection to '{url}': {message}",
        retryable = "true",
        internal = "false"
    )]
    ConnectFailed {
        /// Manager tunnel URL.
        url: String,
        /// What went wrong.
        message: String,
    },

    /// An established tunnel connection ended with an error.
    #[error(
        code = "TUNNEL_CONNECTION_FAILED",
        message = "Tunnel connection failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    ConnectionFailed {
        /// What went wrong.
        message: String,
    },
}
