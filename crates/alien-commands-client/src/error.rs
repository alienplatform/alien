/// Errors that can occur when invoking commands.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// Command creation failed (HTTP error from manager)
    #[error("command creation failed (HTTP {status}): {}", api_error_summary(.body))]
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

/// Show a manager error body as `[CODE] message` so its guidance reads as
/// text; any other body is shown unchanged.
fn api_error_summary(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.to_string();
    };
    match (
        value.get("code").and_then(serde_json::Value::as_str),
        value.get("message").and_then(serde_json::Value::as_str),
    ) {
        (Some(code), Some(message)) => format!("[{code}] {message}"),
        _ => body.to_string(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_failure_shows_the_manager_error_message() {
        let error = CommandError::CreationFailed {
            status: 422,
            body: r#"{"code":"COMMAND_IS_OPERATION","message":"Run: alien operations invoke --deployment d --operation p/o","retryable":false}"#
                .to_string(),
        };

        assert_eq!(
            error.to_string(),
            "command creation failed (HTTP 422): [COMMAND_IS_OPERATION] Run: alien operations invoke --deployment d --operation p/o"
        );
    }

    #[test]
    fn creation_failure_keeps_a_body_that_is_not_an_api_error() {
        let error = CommandError::CreationFailed {
            status: 502,
            body: "bad gateway".to_string(),
        };

        assert_eq!(
            error.to_string(),
            "command creation failed (HTTP 502): bad gateway"
        );
    }
}
