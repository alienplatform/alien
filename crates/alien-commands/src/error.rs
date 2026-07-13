use alien_error::{AlienError, AlienErrorData};
use serde::{Deserialize, Serialize};

/// Errors that occur in the commands protocol.
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

    /// Requested command ID was not found.
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

    /// Explicitly requested command target does not exist in the deployment
    /// (or is not command-capable). Also returned for an empty resource id.
    ///
    /// 404 mirrors the crate's other lookup failures (COMMAND_NOT_FOUND,
    /// LEASE_NOT_FOUND): the named resource does not exist.
    #[error(
        code = "COMMAND_TARGET_NOT_FOUND",
        message = "Command target '{resource_id}' not found in deployment '{deployment_id}'",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    CommandTargetNotFound {
        /// Resource ID that was requested but not found
        resource_id: String,
        /// Deployment the target was looked up in
        deployment_id: String,
    },

    /// Single-target shorthand was used but the deployment has multiple
    /// command-capable targets.
    ///
    /// 409 mirrors CONFLICT: the request conflicts with the deployment's
    /// current state and the client resolves it by naming a target.
    #[error(
        code = "COMMAND_TARGET_AMBIGUOUS",
        message = "Deployment '{deployment_id}' has multiple command-capable targets; specify targetResourceId",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    CommandTargetAmbiguous {
        /// Deployment with more than one command-capable target
        deployment_id: String,
    },

    /// The deployment has no command-capable targets at all.
    ///
    /// 422: the request is well-formed but unsatisfiable for this deployment
    /// (no resource has commands enabled) — unlike 400 (malformed) or 404
    /// (a specific named thing missing).
    #[error(
        code = "NO_COMMAND_TARGETS",
        message = "Deployment '{deployment_id}' has no command-capable targets",
        retryable = "false",
        internal = "false",
        http_status_code = 422
    )]
    NoCommandTargets {
        /// Deployment without any command-capable targets
        deployment_id: String,
    },

    /// A command target resource id contains a character that would break the
    /// `:`-delimited pending-index / idempotency key grammar.
    ///
    /// 400: the request (or a stored target) is malformed. Resource ids must
    /// match the documented `[A-Za-z0-9-_]` charset — in particular they may
    /// never contain `:`, which delimits key segments.
    #[error(
        code = "COMMAND_TARGET_ID_INVALID",
        message = "Command target id '{resource_id}' is invalid: ids must match [A-Za-z0-9-_] and cannot contain ':'",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    CommandTargetIdInvalid {
        /// The offending resource id
        resource_id: String,
    },

    /// Pull command receiver configuration from the environment is missing
    /// or invalid (e.g. a required `ALIEN_COMMANDS_*` variable is absent).
    #[error(
        code = "COMMAND_RECEIVER_CONFIG_INVALID",
        message = "Command receiver configuration invalid: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    CommandReceiverConfigInvalid {
        /// Human-readable description of what is missing or invalid
        message: String,
        /// Environment variable that is missing or invalid
        env_var: String,
    },

    /// The command receiver bearer token was rejected by the commands API.
    #[error(
        code = "COMMAND_RECEIVER_UNAUTHORIZED",
        message = "Command receiver authorization failed during {operation}",
        retryable = "false",
        internal = "false",
        http_status_code = 401
    )]
    CommandReceiverUnauthorized {
        /// Command API operation that was rejected
        operation: String,
        /// Commands API URL that rejected the token
        url: String,
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

    /// Command envelope validation or parsing failed.
    #[error(
        code = "INVALID_ENVELOPE",
        message = "Invalid command envelope: {message}",
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

    /// Generic commands error for uncommon cases.
    #[error(
        code = "COMMANDS_ERROR",
        message = "Commands protocol error: {message}",
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
