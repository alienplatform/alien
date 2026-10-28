use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

pub type Result<T> = alien_error::Result<T, ErrorData>;

#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Download or publication exhausted the destination filesystem.
    #[error(
        code = "DISK_FULL",
        message = "{message}",
        retryable = "false",
        internal = "false"
    )]
    DiskFull { message: String },

    /// The Operator reached a terminal failure while rolling out a release.
    #[error(
        code = "ROLLOUT_FAILED",
        message = "Rollout of {release_id} failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    RolloutFailed { release_id: String, message: String },
    /// Site image repositories cannot be used for this update.
    #[error(
        code = "REGISTRY_PREFLIGHT_FAILED",
        message = "Registry preflight failed: {message}",
        retryable = "inherit",
        internal = "false"
    )]
    RegistryPreflightFailed {
        message: String,
        /// Per-repository failures, including their original error chains.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        failures: Vec<alien_error::AlienError>,
    },

    #[error(
        code = "CONFIGURATION_ERROR",
        message = "Configuration error: {message}",
        retryable = "false",
        internal = "false"
    )]
    ConfigurationError { message: String },

    #[error(
        code = "VALIDATION_ERROR",
        message = "Validation failed for {field}: {message}",
        retryable = "false",
        internal = "false"
    )]
    ValidationError { field: String, message: String },

    #[error(
        code = "MANAGEMENT_UNAVAILABLE",
        message = "Management is unavailable.\nRuntime state was not checked. This does not mean the deployment stopped.",
        retryable = "true",
        internal = "false"
    )]
    ManagementUnavailable,

    #[error(
        code = "FILE_OPERATION_FAILED",
        message = "Failed to {operation} '{file_path}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    FileOperationFailed {
        operation: String,
        file_path: String,
        reason: String,
    },

    #[error(
        code = "DEPLOYMENT_FAILED",
        message = "Deployment {operation} failed",
        retryable = "inherit",
        internal = "inherit"
    )]
    DeploymentFailed { operation: String },

    #[error(
        code = "MAX_STEPS_EXCEEDED",
        message = "Reached maximum steps ({max_steps}) without reaching a terminal state during {operation}",
        retryable = "false",
        internal = "false"
    )]
    MaxStepsExceeded { max_steps: usize, operation: String },

    #[error(
        code = "OPERATOR_SERVICE_ERROR",
        message = "Operator service error: {message}",
        retryable = "false",
        internal = "false"
    )]
    OperatorServiceError { message: String },

    #[error(
        code = "HTTP_ERROR",
        message = "HTTP {operation} to {url} failed: {reason}",
        retryable = "true",
        internal = "false"
    )]
    HttpError {
        operation: String,
        url: String,
        reason: String,
    },

    /// A request to the manager failed or was refused.
    #[error(
        code = "MANAGER_REQUEST_FAILED",
        message = "Manager request failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    ManagerRequestFailed { message: String },

    #[error(
        code = "JSON_ERROR",
        message = "JSON {operation} failed: {reason}",
        retryable = "false",
        internal = "false"
    )]
    JsonError { operation: String, reason: String },

    /// This binary carries an embedded configuration that can't be read.
    #[error(
        code = "EMBEDDED_CONFIG_UNREADABLE",
        message = "Cannot read the configuration embedded in '{file_path}': {reason}. Without it this CLI would run unbranded against the default API. Make the file readable by this user (for example `chmod 0755 '{file_path}'`) or reinstall it.",
        retryable = "false",
        internal = "false"
    )]
    EmbeddedConfigUnreadable { file_path: String, reason: String },

    #[error(
        code = "GENERIC_ERROR",
        message = "{message}",
        retryable = "false",
        internal = "true"
    )]
    GenericError { message: String },
}
