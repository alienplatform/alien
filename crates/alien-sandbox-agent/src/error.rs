use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors raised by the in-sandbox agent.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The sandbox has not proved it can reach the internet only through the gateway.
    #[error(
        code = "EGRESS_PATH_UNEXPECTED",
        message = "EGRESS_PATH_UNEXPECTED: sandbox egress path is not verified",
        retryable = "false",
        internal = "false",
        http_status_code = 503
    )]
    EgressPathUnexpected,
    /// A file path was refused before any filesystem access.
    #[error(
        code = "PATH_REFUSED",
        message = "Path '{path}' refused: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    PathRefused {
        /// The path as the caller supplied it
        path: String,
        /// Why it was refused
        reason: String,
    },

    /// A request was malformed or missing a required field.
    #[error(
        code = "REQUEST_INVALID",
        message = "Request invalid: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    RequestInvalid {
        /// What was wrong with it
        reason: String,
    },

    /// The agent was started with a setting missing or unusable.
    #[error(
        code = "AGENT_CONFIG_INVALID",
        message = "Agent setting {setting} {reason}",
        retryable = "false",
        internal = "true"
    )]
    ConfigInvalid {
        /// The environment variable involved
        setting: String,
        /// What is wrong with it
        reason: String,
    },

    /// The path resolved inside the session but nothing is there.
    #[error(
        code = "PATH_NOT_FOUND",
        message = "No such file in the sandbox: {path}",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    PathNotFound {
        /// The path as the caller wrote it
        path: String,
    },

    /// An operation against the sandbox filesystem or process table failed.
    #[error(
        code = "AGENT_OPERATION_FAILED",
        message = "Agent operation '{operation}' failed: {reason}",
        retryable = "false",
        internal = "false"
    )]
    OperationFailed {
        /// What was being attempted
        operation: String,
        /// The underlying cause
        reason: String,
    },

    /// A poll or cancel named a job the session is not holding.
    #[error(
        code = "JOB_NOT_FOUND",
        message = "No such job: {job_id}",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    JobNotFound {
        /// The job id as the caller supplied it
        job_id: String,
    },

    /// Every job slot holds a still-running job, so a new one cannot be started yet.
    #[error(
        code = "JOB_LIMIT_REACHED",
        message = "The session is running its maximum of {limit} jobs; retry once one finishes",
        retryable = "true",
        internal = "false",
        http_status_code = 429
    )]
    JobLimitReached {
        /// The ceiling on concurrent jobs
        limit: usize,
    },

    /// The image's validate hook found it unable to serve a session.
    ///
    /// 500, never 503: the MicroVM service retries a 503 until the hook's timeout, and nothing
    /// this reports changes by waiting, so a 503 would only delay the same failure and lose the
    /// reason.
    #[error(
        code = "IMAGE_VALIDATION_FAILED",
        message = "Image validation failed at the {check} check: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    ValidationFailed {
        /// Which check failed: `identity` or `file`
        check: String,
        /// What it found
        reason: String,
    },

    /// The caller and the agent do not speak the same protocol version.
    #[error(
        code = "PROTOCOL_VERSION_MISMATCH",
        message = "Caller speaks sandbox agent protocol v{requested}, this agent speaks v{supported}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    ProtocolVersionMismatch {
        /// The version the caller asked for
        requested: u32,
        /// The version this agent implements
        supported: u32,
    },
}

/// This crate's Result type.
pub type Result<T> = alien_error::Result<T, ErrorData>;
