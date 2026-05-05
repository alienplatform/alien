use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors returned by shared test helpers.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Test fixture file operation failed.
    #[error(
        code = "TEST_FILE_OPERATION_FAILED",
        message = "Test file operation '{operation}' failed for '{path}'",
        retryable = "false",
        internal = "true"
    )]
    FileOperationFailed { path: String, operation: String },

    /// Linter command failed to start.
    #[error(
        code = "TEST_LINTER_COMMAND_FAILED",
        message = "Failed to run linter command '{command}'",
        retryable = "false",
        internal = "true"
    )]
    LinterCommandFailed { command: String },

    /// Import request capture failed.
    #[error(
        code = "TEST_IMPORT_CAPTURE_FAILED",
        message = "Import request capture failed: {message}",
        retryable = "false",
        internal = "true"
    )]
    ImportCaptureFailed { message: String },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
