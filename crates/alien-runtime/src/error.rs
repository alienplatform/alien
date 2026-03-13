use alien_error::{AlienError, AlienErrorData};
use serde::{Deserialize, Serialize};

/// Errors that occur in the Alien runtime system.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Configuration is invalid or missing required fields.
    #[error(
        code = "CONFIGURATION_INVALID",
        message = "Configuration error: {message}",
        retryable = "false",
        internal = "false"
    )]
    ConfigurationInvalid {
        /// Human-readable description of the configuration issue
        message: String,
        /// Specific field or section that caused the issue
        field: Option<String>,
    },

    /// Failed to start or initialize a transport.
    #[error(
        code = "TRANSPORT_STARTUP_FAILED",
        message = "Transport '{transport_name}' failed to start: {message}",
        retryable = "true",
        internal = "false"
    )]
    TransportStartupFailed {
        /// Name of the transport that failed to start
        transport_name: String,
        /// Human-readable description of the failure
        message: String,
        /// Address or endpoint the transport was trying to bind to
        address: Option<String>,
    },

    /// Failed to start or initialize a request handler.
    #[error(
        code = "HANDLER_STARTUP_FAILED",
        message = "Request handler failed to start: {message}",
        retryable = "true",
        internal = "false"
    )]
    HandlerStartupFailed {
        /// Human-readable description of the failure
        message: String,
        /// Handler type or configuration details
        handler_type: Option<String>,
    },

    /// Failed to spawn or communicate with a child process.
    #[error(
        code = "PROCESS_SPAWN_FAILED",
        message = "Failed to spawn process '{command}': {message}",
        retryable = "true",
        internal = "false"
    )]
    ProcessSpawnFailed {
        /// Command that failed to spawn
        command: String,
        /// Arguments passed to the command
        args: Vec<String>,
        /// Human-readable description of the failure
        message: String,
        /// Working directory if specified
        working_dir: Option<String>,
    },

    /// Child process terminated unexpectedly.
    #[error(
        code = "CHILD_PROCESS_DIED",
        message = "Child process {pid} terminated with {exit_status}: {message}",
        retryable = "false",
        internal = "false"
    )]
    ChildProcessDied {
        /// Process ID of the child that died
        pid: u32,
        /// Exit status or signal information
        exit_status: String,
        /// Human-readable description of why process died
        message: String,
        /// Recent stdout output if available
        recent_stdout: Option<String>,
        /// Recent stderr output if available  
        recent_stderr: Option<String>,
    },

    /// Network request failed due to connectivity or transport issues.
    #[error(
        code = "NETWORK_REQUEST_FAILED",
        message = "Network request to '{url}' failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    NetworkRequestFailed {
        /// URL or endpoint that failed
        url: String,
        /// HTTP method used
        method: Option<String>,
        /// Human-readable description of the failure
        message: String,
    },

    /// Request processing failed at the application level.
    #[error(
        code = "REQUEST_PROCESSING_FAILED",
        message = "Failed to process request {request_id}: {message}",
        retryable = "true",
        internal = "false"
    )]
    RequestProcessingFailed {
        /// ID of the request that failed to process
        request_id: String,
        /// Human-readable description of the failure
        message: String,
        /// HTTP method of the failed request
        method: Option<String>,
        /// URI path of the failed request
        path: Option<String>,
    },

    /// Serialization or deserialization operation failed.
    #[error(
        code = "SERIALIZATION_FAILED",
        message = "Serialization failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    SerializationFailed {
        /// Human-readable description of the failure
        message: String,
    },

    /// File or I/O operation failed.
    #[error(
        code = "IO_OPERATION_FAILED",
        message = "I/O operation failed on '{path}': {message}",
        retryable = "true",
        internal = "false"
    )]
    IoOperationFailed {
        /// File path or resource identifier
        path: String,
        /// Type of operation (read, write, create, etc.)
        operation: String,
        /// Human-readable description of the failure
        message: String,
    },

    /// Alien bindings gRPC server operation failed.
    #[error(
        code = "BINDINGS_OPERATION_FAILED",
        message = "Alien bindings operation failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    BindingsOperationFailed {
        /// gRPC server address
        address: String,
        /// Provider type (local, aws, gcp, etc.)
        provider: Option<String>,
        /// Human-readable description of the failure
        message: String,
    },

    /// Invalid HTTP request received by a transport.
    #[error(
        code = "INVALID_HTTP_REQUEST",
        message = "Invalid HTTP request: {message}",
        retryable = "true",
        internal = "false"
    )]
    InvalidHttpRequest {
        /// Human-readable description of the validation issue
        message: String,
        /// HTTP method if available
        method: Option<String>,
        /// URI path if available
        path: Option<String>,
    },

    /// Response routing or delivery failed.
    #[error(
        code = "RESPONSE_DELIVERY_FAILED",
        message = "Failed to deliver response for request {request_id}: {message}",
        retryable = "true",
        internal = "false"
    )]
    ResponseDeliveryFailed {
        /// ID of the request whose response failed to deliver
        request_id: String,
        /// Human-readable description of the failure
        message: String,
        /// Destination endpoint or transport
        destination: Option<String>,
    },

    /// Middleware processing failed.
    #[error(
        code = "MIDDLEWARE_PROCESSING_FAILED",
        message = "Middleware '{middleware_name}' failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    MiddlewareProcessingFailed {
        /// Name or type of the middleware that failed
        middleware_name: String,
        /// Human-readable description of the failure
        message: String,
        /// Request ID being processed
        request_id: Option<String>,
    },

    /// Event processing failed during parsing or transformation.
    #[error(
        code = "EVENT_PROCESSING_FAILED",
        message = "Event processing failed for '{event_type}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    EventProcessingFailed {
        /// Type of event being processed
        event_type: String,
        /// Reason for the processing failure
        reason: String,
    },

    /// Failed to load a secret from the vault at startup.
    #[error(
        code = "SECRET_LOAD_FAILED",
        message = "Failed to load secret '{secret_name}': {message}",
        retryable = "true",
        internal = "false"
    )]
    SecretLoadFailed {
        /// Name of the secret that failed to load
        secret_name: String,
        /// Human-readable description of the failure
        message: String,
    },

    /// Application process failed or exited with error.
    #[error(
        code = "PROCESS_FAILED",
        message = "Process failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    ProcessFailed {
        /// Exit code if available
        exit_code: Option<i32>,
        /// Human-readable description of the failure
        message: String,
    },

    /// Generic runtime error for uncommon cases.
    #[error(
        code = "RUNTIME_ERROR",
        message = "Runtime error: {message}",
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
