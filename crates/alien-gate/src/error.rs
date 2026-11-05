use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Why the verifier refused or failed a call.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The request or its statements don't allow the write.
    #[error(
        code = "VERIFIER_REFUSED",
        message = "Refused: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    Refused {
        /// What was wrong.
        reason: String,
    },

    /// What runs in the cloud doesn't match what the verifier built.
    #[error(
        code = "VERIFIER_READ_BACK_MISMATCH",
        message = "Function '{function}' does not match its signed release: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    ReadBackMismatch {
        /// Function name.
        function: String,
        /// What differs.
        reason: String,
    },

    /// The verifier's own configuration is missing or broken.
    #[error(
        code = "VERIFIER_CONFIGURATION_INVALID",
        message = "Verifier configuration is invalid: {message}",
        retryable = "false",
        internal = "false"
    )]
    ConfigurationInvalid {
        /// What is wrong.
        message: String,
    },

    /// A cloud call failed.
    #[error(
        code = "VERIFIER_CLOUD_CALL_FAILED",
        message = "{operation} failed",
        retryable = "inherit",
        internal = "false"
    )]
    CloudCallFailed {
        /// The call.
        operation: String,
    },
}

/// Result type for the verifier.
pub type Result<T> = alien_error::Result<T, ErrorData>;
