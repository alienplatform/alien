use alien_error::{AlienError, AlienErrorData};
use serde::{Deserialize, Serialize};

/// Errors that occur in the Alien Remote Call (ARC) protocol.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Command validation failed or contains invalid data.
    #[error(
        code = "INVALID_COMMAND",
        message = "Invalid command: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    InvalidCommand {
        /// Human-readable description of what makes the command invalid
        message: String,
    },

    /// Requested ARC command ID was not found.
    #[error(
        code = "COMMAND_NOT_FOUND",
        message = "Command '{command_id}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    CommandNotFound {
        /// ID of the command that was not found
        command_id: String,
    },

    /// Invalid state transition attempted on command.
    #[error(
        code = "INVALID_STATE_TRANSITION",
        message = "Invalid state transition from '{from}' to '{to}'",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    InvalidStateTransition {
        /// Current state of the command
        from: String,
        /// Attempted new state
        to: String,
    },

    /// Command has expired and can no longer be processed.
    #[error(
        code = "COMMAND_EXPIRED",
        message = "Command '{command_id}' has expired",
        retryable = "false",
        internal = "false",
        http_status_code = 410
    )]
    CommandExpired {
        /// ID of the expired command
        command_id: String,
    },

    /// Storage backend operation failed.
    #[error(
        code = "STORAGE_OPERATION_FAILED",
        message = "Storage operation failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    StorageOperationFailed {
        /// Human-readable description of the storage failure
        message: String,
        /// Storage operation type (upload, download, etc.)
        operation: Option<String>,
        /// Storage path or URL that failed
        path: Option<String>,
    },

    /// Key-value store operation failed.
    #[error(
        code = "KV_OPERATION_FAILED",
        message = "KV operation '{operation}' failed on key '{key}': {message}",
        retryable = "true",
        internal = "false"
    )]
    KvOperationFailed {
        /// Type of KV operation that failed
        operation: String,
        /// Key that was being operated on
        key: String,
        /// Human-readable description of the failure
        message: String,
    },

    /// Transport dispatch to agent failed.
    #[error(
        code = "TRANSPORT_DISPATCH_FAILED",
        message = "Transport dispatch failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    TransportDispatchFailed {
        /// Human-readable description of the dispatch failure
        message: String,
        /// Transport type that failed
        transport_type: Option<String>,
        /// Target endpoint or agent identifier
        target: Option<String>,
    },

    /// ARC envelope validation or parsing failed.
    #[error(
        code = "INVALID_ENVELOPE",
        message = "Invalid ARC envelope: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    InvalidEnvelope {
        /// Human-readable description of the envelope issue
        message: String,
        /// Envelope field that caused the validation failure
        field: Option<String>,
    },

    /// Agent reported an error during command processing.
    #[error(
        code = "AGENT_ERROR",
        message = "Agent error: {message}",
        retryable = "true",
        internal = "false"
    )]
    AgentError {
        /// Human-readable description of the agent error
        message: String,
        /// Agent identifier if available
        deployment_id: Option<String>,
    },

    /// Serialization or deserialization operation failed.
    #[error(
        code = "SERIALIZATION_FAILED",
        message = "Serialization failed: {message}",
        retryable = "false",
        internal = "true"
    )]
    SerializationFailed {
        /// Human-readable description of the serialization failure
        message: String,
        /// Data type that failed to serialize/deserialize
        data_type: Option<String>,
    },

    /// HTTP operation failed during command processing.
    #[error(
        code = "HTTP_OPERATION_FAILED",
        message = "HTTP operation failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    HttpOperationFailed {
        /// Human-readable description of the HTTP failure
        message: String,
        /// HTTP method if available
        method: Option<String>,
        /// URL that failed if available
        url: Option<String>,
    },

    /// Operation is not supported by the current configuration.
    #[error(
        code = "OPERATION_NOT_SUPPORTED",
        message = "Operation not supported: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 501
    )]
    OperationNotSupported {
        /// Human-readable description of what operation is not supported
        message: String,
        /// Operation type that was attempted
        operation: Option<String>,
    },

    /// Resource conflict detected (e.g., concurrent modification).
    #[error(
        code = "CONFLICT",
        message = "Conflict: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 409
    )]
    Conflict {
        /// Human-readable description of the conflict
        message: String,
        /// Resource identifier that has the conflict
        resource_id: Option<String>,
    },

    /// Requested lease ID was not found.
    #[error(
        code = "LEASE_NOT_FOUND",
        message = "Lease '{lease_id}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    LeaseNotFound {
        /// ID of the lease that was not found
        lease_id: String,
    },

    /// Generic ARC error for uncommon cases.
    #[error(
        code = "ARC_ERROR",
        message = "ARC protocol error: {message}",
        retryable = "true",
        internal = "true"
    )]
    Other {
        /// Human-readable description of the error
        message: String,
    },
}

pub type Error = AlienError<ErrorData>;
pub type Result<T> = alien_error::Result<T, ErrorData>;
