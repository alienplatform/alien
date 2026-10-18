use alien_error::{AlienError, AlienErrorData};
use serde::{Deserialize, Serialize};

/// Gateway failures. Reasons are fixed categories, never request contents or secrets.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Invalid configuration or protocol input.
    #[error(
        code = "EGRESS_INVALID",
        message = "Invalid egress input: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    Invalid { reason: String },
    /// An authenticated request is outside the effective destination policy.
    #[error(
        code = "EGRESS_POLICY_DENIED",
        message = "Egress target outside stored policy",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    PolicyDenied,
    /// The canonical policy exceeds the shared limit.
    #[error(
        code = "SANDBOX_EGRESS_POLICY_TOO_LARGE",
        message = "Egress policy exceeds 4096 targets or 256 KiB",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    PolicyTooLarge,
    /// The authenticated request does not satisfy policy.
    #[error(
        code = "EGRESS_DENIED",
        message = "Egress denied: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    Denied { reason: String },
    /// A required network or storage operation failed.
    #[error(
        code = "EGRESS_UNAVAILABLE",
        message = "Egress dependency unavailable: {operation}",
        retryable = "true",
        internal = "true",
        http_status_code = 503
    )]
    Unavailable { operation: String },
}
/// Gateway result with structured error metadata.
pub type Result<T> = alien_error::Result<T, ErrorData>;

pub(crate) fn invalid(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Invalid {
        reason: reason.into(),
    })
}
pub(crate) fn denied(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Denied {
        reason: reason.into(),
    })
}
pub(crate) fn unavailable(operation: &str) -> ErrorData {
    ErrorData::Unavailable {
        operation: operation.into(),
    }
}
