use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Why the gate refused or failed a call.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The request or its statements don't allow the write.
    #[error(
        code = "GATE_REFUSED",
        message = "Refused: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    Refused {
        /// What was wrong.
        reason: String,
    },

    /// The function in the cloud doesn't match what the gate built.
    #[error(
        code = "GATE_READ_BACK_MISMATCH",
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

    /// The gate's own configuration is missing or broken.
    #[error(
        code = "GATE_CONFIGURATION_INVALID",
        message = "Gate configuration is invalid: {message}",
        retryable = "false",
        internal = "false"
    )]
    ConfigurationInvalid {
        /// What is wrong.
        message: String,
    },

    /// A cloud call failed.
    #[error(
        code = "GATE_CLOUD_CALL_FAILED",
        message = "{operation} failed",
        retryable = "inherit",
        internal = "false"
    )]
    CloudCallFailed {
        /// The call.
        operation: String,
    },
}

/// Result type for the gate.
pub type Result<T> = alien_error::Result<T, ErrorData>;
