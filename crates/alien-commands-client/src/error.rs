/// Errors that can occur when invoking commands.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// Command creation failed (HTTP error from manager)
    #[error("command creation failed (HTTP {status}): {body}")]
    CreationFailed { status: u16, body: String },

    /// Command timed out waiting for result
    #[error("command {command_id} timed out (last state: {last_state})")]
    Timeout {
        command_id: String,
        last_state: String,
    },

    /// Deployment returned an error
    #[error("command {command_id} failed: [{code}] {message}")]
    DeploymentError {
        command_id: String,
        code: String,
        message: String,
    },

    /// Command expired (deadline passed)
    #[error("command {command_id} expired")]
    Expired { command_id: String },

    /// Failed to decode response
    #[error("failed to decode response for command {command_id}: {reason}")]
    ResponseDecodingFailed { command_id: String, reason: String },

    /// Storage download failed (for large responses)
    #[error("storage operation failed: {reason}")]
    StorageOperationFailed { reason: String },

    /// HTTP/network error
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

impl CommandError {
    /// Returns true if this error is potentially retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            CommandError::Timeout { .. }
                | CommandError::HttpError(_)
                | CommandError::StorageOperationFailed { .. }
        )
    }
}
