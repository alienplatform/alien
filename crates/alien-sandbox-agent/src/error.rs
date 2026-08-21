use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors raised by the in-sandbox agent.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// A file path was refused before any filesystem access.
    #[error(
        code = "PATH_REFUSED",
        message = "Path '{path}' refused: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    PathRefused {
        /// The path as the caller supplied it
        path: String,
        /// Why it was refused
        reason: String,
    },

    /// A request was malformed or missing a required field.
    #[error(
        code = "REQUEST_INVALID",
        message = "Request invalid: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    RequestInvalid {
        /// What was wrong with it
        reason: String,
    },

    /// The agent was started with a setting missing or unusable.
    #[error(
        code = "AGENT_CONFIG_INVALID",
        message = "Agent setting {setting} {reason}",
        retryable = "false",
        internal = "true"
    )]
    ConfigInvalid {
        /// The environment variable involved
        setting: String,
        /// What is wrong with it
        reason: String,
    },

    /// The path resolved inside the session but nothing is there.
    #[error(
        code = "PATH_NOT_FOUND",
        message = "No such file in the sandbox: {path}",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    PathNotFound {
        /// The path as the caller wrote it
        path: String,
    },

    /// An operation against the sandbox filesystem or process table failed.
    #[error(
        code = "AGENT_OPERATION_FAILED",
        message = "Agent operation '{operation}' failed: {reason}",
        retryable = "false",
        internal = "false"
    )]
    OperationFailed {
        /// What was being attempted
        operation: String,
        /// The underlying cause
        reason: String,
    },

    /// The caller and the agent do not speak the same protocol version.
    #[error(
        code = "PROTOCOL_VERSION_MISMATCH",
        message = "Caller speaks sandbox agent protocol v{requested}, this agent speaks v{supported}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    ProtocolVersionMismatch {
        /// The version the caller asked for
        requested: u32,
        /// The version this agent implements
        supported: u32,
    },
}

/// This crate's Result type.
pub type Result<T> = alien_error::Result<T, ErrorData>;
