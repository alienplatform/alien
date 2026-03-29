use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Error data types for alien-cli operations
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Configuration is invalid or could not be loaded.
    #[error(
        code = "CONFIGURATION_ERROR",
        message = "Configuration error: {message}",
        retryable = "false",
        internal = "false"
    )]
    ConfigurationError {
        /// Human-readable description of the configuration issue
        message: String,
    },

    /// Input validation failed.
    #[error(
        code = "VALIDATION_ERROR",
        message = "Validation failed for {field}: {message}",
        retryable = "false",
        internal = "false"
    )]
    ValidationError {
        /// The field or parameter that failed validation
        field: String,
        /// Description of what went wrong
        message: String,
    },

    /// File system operation failed.
    #[error(
        code = "FILE_OPERATION_FAILED",
        message = "Failed to {operation} '{file_path}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    FileOperationFailed {
        /// The operation that failed (e.g., "read", "write", "create directory")
        operation: String,
        /// Path to the file or directory that failed
        file_path: String,
        /// Reason for the operation failure
        reason: String,
    },

    /// Authentication failed.
    #[error(
        code = "AUTHENTICATION_FAILED",
        message = "Authentication failed: {reason}",
        retryable = "true",
        internal = "false"
    )]
    AuthenticationFailed {
        /// Reason for authentication failure
        reason: String,
    },

    /// API request failed.
    #[error(
        code = "API_REQUEST_FAILED",
        message = "API request failed: {message}",
        retryable = "inherit",
        internal = "false"
    )]
    ApiRequestFailed {
        /// Human-readable description of the API failure
        message: String,
        /// Optional URL that was requested
        url: Option<String>,
    },

    /// Network or connectivity issue.
    #[error(
        code = "NETWORK_ERROR",
        message = "Network error: {message}",
        retryable = "true",
        internal = "false"
    )]
    NetworkError {
        /// Description of the network error
        message: String,
    },

    /// User cancelled the operation.
    #[error(
        code = "USER_CANCELLED",
        message = "Operation cancelled by user",
        retryable = "false",
        internal = "false"
    )]
    UserCancelled,

    /// Platform mismatch between state and deployment target.
    #[error(
        code = "PLATFORM_MISMATCH",
        message = "Platform mismatch: existing state is for '{existing_platform}', but deployment target is for '{target_platform}'. Please use the correct deployment target or remove the existing state file",
        retryable = "false",
        internal = "false"
    )]
    PlatformMismatch {
        /// The platform in the existing state
        existing_platform: String,
        /// The platform of the deployment target
        target_platform: String,
    },

    /// Maximum steps exceeded during deployment operation.
    #[error(
        code = "MAX_STEPS_EXCEEDED",
        message = "Reached maximum steps ({max_steps}) without reaching a terminal state during {operation}",
        retryable = "false",
        internal = "false"
    )]
    MaxStepsExceeded {
        /// Maximum number of steps that was configured
        max_steps: usize,
        /// The operation that exceeded max steps
        operation: String,
    },

    /// JSON serialization or deserialization failed.
    #[error(
        code = "JSON_ERROR",
        message = "JSON {operation} failed: {reason}",
        retryable = "false",
        internal = "false"
    )]
    JsonError {
        /// The operation that failed
        operation: String,
        /// Reason for the JSON operation failure
        reason: String,
    },

    /// Object store operation failed.
    #[error(
        code = "OBJECT_STORE_ERROR",
        message = "Object store operation failed: {operation} on '{uri}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    ObjectStoreError {
        /// The operation that failed
        operation: String,
        /// URI or path that was accessed
        uri: String,
        /// Reason for the operation failure
        reason: String,
    },

    /// Interactive CLI operation failed.
    #[error(
        code = "CLI_INTERACTION_FAILED",
        message = "CLI interaction failed: {message}",
        retryable = "false",
        internal = "true"
    )]
    CliInteractionFailed {
        /// Description of the interaction failure
        message: String,
    },

    /// Authentication credentials are missing or invalid.
    #[error(
        code = "AUTH_CREDENTIALS_MISSING",
        message = "Missing required credential: {field}",
        retryable = "false",
        internal = "false"
    )]
    AuthCredentialsMissing {
        /// The credential field that is missing
        field: String,
    },

    /// Invalid project name specified.
    #[error(
        code = "INVALID_PROJECT_NAME",
        message = "Invalid project name '{project_name}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    InvalidProjectName {
        /// The invalid project name
        project_name: String,
        /// Reason why the project name is invalid
        reason: String,
    },

    /// Project link file is invalid or corrupted.
    #[error(
        code = "PROJECT_LINK_INVALID",
        message = "Project link invalid: {message}",
        retryable = "false",
        internal = "false"
    )]
    ProjectLinkInvalid {
        /// Description of what is invalid about the project link
        message: String,
    },

    /// Build operation failed.
    #[error(
        code = "BUILD_FAILED",
        message = "Build failed",
        retryable = "inherit",
        internal = "inherit"
    )]
    BuildFailed,

    /// Release operation failed.
    #[error(
        code = "RELEASE_FAILED",
        message = "Release failed: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    ReleaseFailed {
        /// Description of the release failure
        message: String,
    },

    /// Deployment operation failed.
    #[error(
        code = "DEPLOYMENT_FAILED",
        message = "Deployment failed: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    DeploymentFailed {
        /// Description of the deployment failure
        message: String,
    },

    /// Local service operation failed.
    #[error(
        code = "LOCAL_SERVICE_FAILED",
        message = "Local service '{service}' failed: {reason}",
        retryable = "false",
        internal = "false"
    )]
    LocalServiceFailed {
        /// The local service that failed
        service: String,
        /// Reason for the failure
        reason: String,
    },

    /// Server failed to start.
    #[error(
        code = "SERVER_START_FAILED",
        message = "Failed to start server: {reason}",
        retryable = "false",
        internal = "true"
    )]
    ServerStartFailed {
        /// Reason for the server failure
        reason: String,
    },

    /// HTTP request failed.
    #[error(
        code = "HTTP_REQUEST_FAILED",
        message = "HTTP request failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    HttpRequestFailed {
        /// Description of the HTTP failure
        message: String,
        /// Optional URL that was requested
        url: Option<String>,
    },

    /// Generic error for uncommon cases.
    #[error(
        code = "GENERIC_ERROR",
        message = "{message}",
        retryable = "false",
        internal = "true"
    )]
    GenericError {
        /// Human-readable description of the error
        message: String,
    },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
