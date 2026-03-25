use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Platform-specific error data variants.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    #[error(
        code = "CONFIGURATION_ERROR",
        message = "{message}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    ConfigurationError { message: String },

    #[error(
        code = "BINDING_NOT_FOUND",
        message = "Binding '{binding_name}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    BindingNotFound { binding_name: String },

    #[error(
        code = "SERVICE_ACCOUNT_OPERATION_FAILED",
        message = "Service account operation failed: {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    ServiceAccountOperationFailed { operation: String },

    #[error(
        code = "STACK_OPERATION_FAILED",
        message = "Stack operation failed: {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    StackOperationFailed { operation: String },

    #[error(
        code = "ARTIFACT_REGISTRY_OPERATION_FAILED",
        message = "Artifact registry operation failed: {operation}",
        retryable = "inherit",
        internal = "inherit",
        http_status_code = "inherit"
    )]
    ArtifactRegistryOperationFailed { operation: String },

    #[error(
        code = "CREDENTIAL_RESOLUTION_FAILED",
        message = "Failed to resolve credentials for platform {platform}: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    CredentialResolutionFailed { platform: String, message: String },

    #[error(
        code = "CLIENT_CONFIG_ERROR",
        message = "Client config error: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    ClientConfigError { message: String },

    #[error(
        code = "PLATFORM_MISMATCH",
        message = "Platform mismatch: expected {expected}, got {actual}. {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    PlatformMismatch {
        expected: String,
        actual: String,
        message: String,
    },

    #[error(
        code = "SELF_HEARTBEAT_FAILED",
        message = "Self-heartbeat failed: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    SelfHeartbeatFailed { message: String },

    #[error(
        code = "SYNC_FAILED",
        message = "Sync failed: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    SyncFailed { message: String },

    #[error(
        code = "TELEMETRY_FAILED",
        message = "Telemetry failed: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    TelemetryFailed { message: String },

    #[error(
        code = "FEATURE_NOT_SUPPORTED",
        message = "{feature} is not supported: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 501
    )]
    FeatureNotSupported { feature: String, message: String },

    #[error(
        code = "AUTHENTICATION_FAILED",
        message = "Authentication failed: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 401
    )]
    AuthenticationFailed { message: String },

    #[error(
        code = "UNAUTHORIZED",
        message = "Unauthorized: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 401
    )]
    Unauthorized { message: String },

    #[error(
        code = "STARTUP_FAILED",
        message = "Platform Manager startup failed: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    StartupFailed { message: String },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
