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

    /// A non-root agent cannot read the saved image command.
    #[error(
        code = "IMAGE_COMMAND_UNREADABLE",
        message = "The agent cannot read {path}: the sandbox image was built by an older release that left this file readable only by root, and this agent runs as a non-root user. Rebuild the sandbox image with this release.",
        retryable = "false",
        internal = "false"
    )]
    ImageCommandUnreadable {
        /// The saved command file
        path: String,
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

    /// A write, close or signal named an interactive command that is not running.
    #[error(
        code = "COMMAND_NOT_RUNNING",
        message = "No interactive command '{id}' is running",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    CommandNotRunning {
        /// The id as the caller supplied it
        id: String,
    },

    /// Every interactive slot holds a running command.
    #[error(
        code = "INTERACTIVE_LIMIT_REACHED",
        message = "The session is running its maximum of {limit} interactive commands; retry once one finishes",
        retryable = "true",
        internal = "false",
        http_status_code = 429
    )]
    InteractiveLimitReached {
        /// The ceiling on concurrent interactive commands
        limit: usize,
    },

    /// A write started past the input the command has taken, so bytes before it never arrived.
    #[error(
        code = "STDIN_OFFSET_MISMATCH",
        message = "The command has taken {accepted} bytes of input; a write at offset {offset} would leave a gap",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    StdinOffsetMismatch {
        /// Bytes the command has taken
        accepted: u64,
        /// Where the refused write started
        offset: u64,
    },

    /// A write from before the caller's last barrier, arriving after it. Applying it would put
    /// bytes the caller has already given up on ahead of the ones it sent since.
    #[error(
        code = "STDIN_STALE_WRITE",
        message = "A write from input generation {generation} arrived after the input moved to generation {current}",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    StdinStaleWrite {
        /// The generation the refused write carried
        generation: u64,
        /// The command's current generation
        current: u64,
    },

    /// The command's stdin is closed, by the caller or because the command stopped reading it.
    #[error(
        code = "STDIN_CLOSED",
        message = "Interactive command '{id}' no longer reads its input",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    StdinClosed {
        /// The command's id
        id: String,
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
