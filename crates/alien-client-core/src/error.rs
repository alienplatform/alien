use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Represents common infrastructure errors across multiple cloud platforms.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Invalid or malformed platform configuration.
    #[error(
        code = "INVALID_client_config",
        message = "Invalid platform configuration: {message}",
        retryable = "false",
        internal = "false"
    )]
    InvalidClientConfig {
        /// Human-readable description of the configuration issue
        message: String,
        /// Additional error details if available
        errors: Option<String>,
    },

    /// Authentication with cloud provider failed.
    #[error(
        code = "AUTHENTICATION_ERROR",
        message = "Authentication failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    AuthenticationError {
        /// Human-readable description of the authentication failure
        message: String,
    },

    /// The requested resource does not exist.
    #[error(
        code = "REMOTE_RESOURCE_NOT_FOUND",
        message = "{resource_type} '{resource_name}' not found",
        retryable = "true",
        internal = "false",
        http_status_code = 404
    )]
    RemoteResourceNotFound {
        /// Type of the resource that was not found
        resource_type: String,
        /// Name of the resource that was not found
        resource_name: String,
    },

    /// Operation conflicts with current resource state (e.g., resource already exists, concurrent modifications, etag mismatch).
    #[error(
        code = "REMOTE_RESOURCE_CONFLICT",
        message = "Conflict with {resource_type} '{resource_name}': {message}",
        retryable = "true",
        internal = "false"
    )]
    RemoteResourceConflict {
        /// Type of the resource that has a conflict
        resource_type: String,
        /// Name of the resource that has a conflict
        resource_name: String,
        /// Human-readable description of the specific conflict
        message: String,
    },

    /// Access denied due to insufficient permissions.
    #[error(
        code = "REMOTE_ACCESS_DENIED",
        message = "Access denied to {resource_type} '{resource_name}'",
        retryable = "true",
        internal = "false"
    )]
    RemoteAccessDenied {
        /// Type of the resource access was denied to
        resource_type: String,
        /// Name of the resource access was denied to
        resource_name: String,
    },

    /// Request rate limit exceeded.
    #[error(
        code = "RATE_LIMIT_EXCEEDED",
        message = "Rate limit exceeded: {message}",
        retryable = "true",
        internal = "false"
    )]
    RateLimitExceeded {
        /// Human-readable description of the rate limit context
        message: String,
    },

    /// Operation exceeded the allowed timeout.
    #[error(
        code = "TIMEOUT",
        message = "Operation timed out: {message}",
        retryable = "true",
        internal = "false"
    )]
    Timeout {
        /// Human-readable description of what operation timed out
        message: String,
    },

    /// Service is temporarily unavailable.
    #[error(
        code = "REMOTE_SERVICE_UNAVAILABLE",
        message = "Service unavailable: {message}",
        retryable = "true",
        internal = "false"
    )]
    RemoteServiceUnavailable {
        /// Human-readable description of the service unavailability
        message: String,
    },

    /// Quota or resource limits have been exceeded.
    #[error(
        code = "QUOTA_EXCEEDED",
        message = "Quota exceeded: {message}",
        retryable = "true",
        internal = "false"
    )]
    QuotaExceeded {
        /// Human-readable description of what quota was exceeded
        message: String,
    },

    /// Network or request-level failure when sending HTTP request.
    #[error(
        code = "HTTP_REQUEST_FAILED",
        message = "{message}",
        retryable = "true",
        internal = "false"
    )]
    HttpRequestFailed {
        /// Human-readable description of the HTTP request failure
        message: String,
    },

    /// HTTP request succeeded but returned a non-success status code.
    #[error(
        code = "HTTP_RESPONSE_ERROR",
        message = "{message}",
        retryable = "true",
        internal = "true"
    )]
    HttpResponseError {
        /// Human-readable description of the HTTP response error
        message: String,
        /// The URL that returned the error
        url: String,
        /// HTTP status code returned
        http_status: u16,
        // Request body text
        http_request_text: Option<String>,
        /// Response body text if available
        http_response_text: Option<String>,
    },

    /// Failure during signing of an HTTP request (e.g., AWS SigV4).
    #[error(
        code = "REQUEST_SIGN_ERROR",
        message = "Request signing failed: {message}",
        retryable = "true",
        internal = "true"
    )]
    RequestSignError {
        /// Human-readable description of the signing failure
        message: String,
    },

    /// Catch-all error
    #[error(
        code = "GENERIC_ERROR",
        message = "{message}",
        retryable = "true",
        internal = "true"
    )]
    GenericError {
        /// Human-readable description of the error
        message: String,
    },

    /// Invalid or malformed input parameters provided to the operation.
    #[error(
        code = "INVALID_INPUT",
        message = "Invalid input: {message}",
        retryable = "false",
        internal = "false"
    )]
    InvalidInput {
        /// Human-readable description of what input was invalid
        message: String,
        /// Optional field name that was invalid
        field_name: Option<String>,
    },

    /// Failed to serialize or deserialize data.
    #[error(
        code = "SERIALIZATION_ERROR",
        message = "Serialization failed: {message}",
        retryable = "false",
        internal = "true"
    )]
    SerializationError {
        /// Human-readable description of the serialization failure
        message: String,
    },

    /// Failed to load or parse kubeconfig file.
    #[error(
        code = "KUBECONFIG_ERROR",
        message = "Kubeconfig error: {message}",
        retryable = "false",
        internal = "false"
    )]
    KubeconfigError {
        /// Human-readable description of the kubeconfig error
        message: String,
    },

    /// Failed to load data from base64 or file.
    #[error(
        code = "DATA_LOAD_ERROR",
        message = "Failed to load data: {message}",
        retryable = "false",
        internal = "false"
    )]
    DataLoadError {
        /// Human-readable description of the data loading failure
        message: String,
    },
}

/// Convenient alias with default error type `ErrorData`. Allows callers to override the
/// error type when needed while still accepting the common single-generic form `Result<T>`.
pub type Result<T, E = ErrorData> = alien_error::Result<T, E>;

/// Convenience alias representing a constructed AlienError with our `ErrorData` payload.
pub type Error = alien_error::AlienError<ErrorData>;
