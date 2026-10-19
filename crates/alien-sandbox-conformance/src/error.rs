use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The fixture configuration or command result cannot be interpreted.
    #[error(
        code = "CONFORMANCE_INVALID",
        message = "Invalid conformance run: {message}",
        retryable = "false",
        internal = "false"
    )]
    Invalid { message: String },
    /// A required run operation failed; retain its original source.
    #[error(
        code = "CONFORMANCE_FAILED",
        message = "Conformance operation failed: {operation}",
        retryable = "inherit",
        internal = "inherit"
    )]
    Operation { operation: String },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;

pub fn operation(description: impl Into<String>) -> ErrorData {
    ErrorData::Operation {
        operation: description.into(),
    }
}
