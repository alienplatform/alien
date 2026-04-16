use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

pub type Result<T> = alien_error::Result<T, ErrorData>;

#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
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
        code = "AGENT_SERVICE_ERROR",
        message = "Agent service error: {message}",
        retryable = "false",
        internal = "false"
    )]
    AgentServiceError { message: String },

    #[error(
        code = "GENERIC_ERROR",
        message = "{message}",
        retryable = "false",
        internal = "true"
    )]
    GenericError { message: String },
}
