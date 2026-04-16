use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Convenient type alias for this module's Result type.
pub type Result<T> = alien_error::Result<T, ErrorData>;

#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    #[error(
        code = "CONFIGURATION_ERROR",
        message = "{message}",
        retryable = "false",
        internal = "true",
        http_status_code = 500
    )]
    ConfigurationError { message: String },

    #[error(
        code = "DATABASE_ERROR",
        message = "Database error: {message}",
        retryable = "true",
        internal = "true",
        http_status_code = 500
    )]
    DatabaseError { message: String },

    #[error(
        code = "SYNC_FAILED",
        message = "Failed to sync with Manager: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    SyncFailed { message: String },

    #[error(
        code = "DEPLOYMENT_FAILED",
        message = "{message}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    DeploymentFailed { message: String },

    #[error(
        code = "TELEMETRY_PUSH_FAILED",
        message = "Failed to push telemetry: {message}",
        retryable = "true",
        internal = "true",
        http_status_code = 502
    )]
    TelemetryPushFailed { message: String },

    #[error(
        code = "ENCRYPTION_ERROR",
        message = "Encryption error: {message}",
        retryable = "false",
        internal = "true",
        http_status_code = 500
    )]
    EncryptionError { message: String },
}
