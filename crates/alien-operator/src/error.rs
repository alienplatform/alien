use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Convenient type alias for this module's Result type.
pub type Result<T> = alien_error::Result<T, ErrorData>;

#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    #[error(
        code = "INCOMPATIBLE_OPERATOR_STATE",
        message = "Operator state schema version {found_version} is not supported; this operator supports {min_supported_version} through {current_version}. {repair}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    IncompatibleOperatorState {
        found_version: u32,
        min_supported_version: u32,
        current_version: u32,
        repair: String,
    },

    #[error(
        code = "CONFIGURATION_ERROR",
        message = "{message}",
        retryable = "false",
        internal = "true",
        http_status_code = 500
    )]
    ConfigurationError { message: String },

    /// The manager rejected `/v1/initialize` because a deployment with the
    /// requested `(deployment_group_id, name)` already exists. Distinct
    /// from `ConfigurationError` so the caller can route this into the
    /// `/v1/rejoin` fall-through (state-wipe recovery) instead of crashing
    /// the agent.
    #[error(
        code = "DEPLOYMENT_NAME_ALREADY_EXISTS",
        message = "Deployment name already exists in this deployment group",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    DeploymentNameAlreadyExists,

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

    /// The os-service self-update actuator failed (download, staging, or
    /// marker I/O). Retryable — the manager keeps advertising the target and
    /// the actuator backs off between attempts.
    #[error(
        code = "SELF_UPDATE_FAILED",
        message = "Self-update failed: {message}",
        retryable = "true",
        internal = "true"
    )]
    SelfUpdateFailed { message: String },

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
        code = "COLLECTOR_AUTHORIZATION_FAILED",
        message = "Collector authorization failed: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 401
    )]
    CollectorAuthorizationFailed { message: String },

    #[error(
        code = "COLLECTOR_PAYLOAD_INVALID",
        message = "Collector payload is invalid: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    CollectorPayloadInvalid { message: String },

    #[error(
        code = "COLLECTOR_TELEMETRY_INVALID",
        message = "Collector telemetry could not be encoded: {message}",
        retryable = "false",
        internal = "true",
        http_status_code = 500
    )]
    CollectorTelemetryInvalid { message: String },

    #[error(
        code = "ENCRYPTION_ERROR",
        message = "Encryption error: {message}",
        retryable = "false",
        internal = "true",
        http_status_code = 500
    )]
    EncryptionError { message: String },
}
