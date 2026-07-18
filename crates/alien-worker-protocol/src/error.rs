use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors raised while running the worker app protocol gRPC server.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// gRPC service unavailable or returned error.
    #[error(
        code = "GRPC_SERVICE_UNAVAILABLE",
        message = "gRPC service '{service}' unavailable at endpoint '{endpoint}': {reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 503
    )]
    GrpcServiceUnavailable {
        /// Name of the gRPC service
        service: String,
        /// The gRPC endpoint
        endpoint: String,
        /// Reason for service unavailability
        reason: String,
    },

    /// Server failed to bind to the specified address.
    #[error(
        code = "SERVER_BIND_FAILED",
        message = "Failed to bind server to address '{address}': {reason}",
        retryable = "true",
        internal = "true",
        http_status_code = 500
    )]
    ServerBindFailed {
        /// The address that failed to bind
        address: String,
        /// Reason for the bind failure
        reason: String,
    },
}

/// Convenient type alias for this crate's Result type.
pub type Result<T> = alien_error::Result<T, ErrorData>;
