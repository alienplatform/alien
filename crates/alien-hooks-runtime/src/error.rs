use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors from loading a hooks module or calling one of its hooks.
///
/// None is retryable, a hook stopped at its deadline included: a hook gives the same answer for
/// the same input, so a retry fails the same way. The one exception is a hook's own error that
/// says another try may pass. An interceptor's host decides for itself whether a deadline before
/// the call is sent is worth another try.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The module's sha256 is not the one the release pins.
    #[error(
        code = "HOOKS_MODULE_DIGEST_MISMATCH",
        message = "The hooks module's sha256 is {actual_sha256}, but the release pins {expected_sha256}",
        retryable = "false",
        internal = "false"
    )]
    ModuleDigestMismatch {
        /// The sha256 the release pins, in lowercase hex
        expected_sha256: String,
        /// The sha256 of the bytes given, in lowercase hex
        actual_sha256: String,
    },

    /// The module is not a hooks module this runtime can run.
    #[error(
        code = "HOOKS_MODULE_REFUSED",
        message = "The hooks module {sha256} was refused: {reason}",
        retryable = "false",
        internal = "false"
    )]
    ModuleRefused {
        /// The module's sha256, in lowercase hex
        sha256: String,
        /// Why it was refused
        reason: String,
    },

    /// The hook returned an error of its own, without a code.
    #[error(
        code = "HOOK_FAILED",
        message = "Hook '{hook}' of extension '{extension}' failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    HookFailed {
        /// The extension that declares the hook
        extension: String,
        /// The hook's name in the module's manifest
        hook: String,
        /// The error the hook returned
        message: String,
    },

    /// The hook returned an error of its own, with a code.
    #[error(
        code = "HOOK_FAILED",
        message = "Hook '{hook}' of extension '{extension}' failed ({code}): {message}",
        retryable = "false",
        internal = "false"
    )]
    HookFailedWithCode {
        /// The extension that declares the hook
        extension: String,
        /// The hook's name in the module's manifest
        hook: String,
        /// The code of the error the hook returned
        code: String,
        /// The error the hook returned
        message: String,
    },

    /// The hook ran past its deadline and was stopped.
    #[error(
        code = "HOOK_DEADLINE_EXCEEDED",
        message = "Hook '{hook}' of extension '{extension}' was stopped at its {deadline_ms} ms deadline",
        retryable = "false",
        internal = "false"
    )]
    HookDeadlineExceeded {
        /// The extension that declares the hook
        extension: String,
        /// The hook's name in the module's manifest
        hook: String,
        /// The deadline, in milliseconds
        deadline_ms: u64,
    },

    /// The hook needed more memory than its limit.
    #[error(
        code = "HOOK_MEMORY_LIMIT_EXCEEDED",
        message = "Hook '{hook}' of extension '{extension}' needed more than its {limit_bytes} bytes of memory: {message}",
        retryable = "false",
        internal = "false"
    )]
    HookMemoryLimitExceeded {
        /// The extension that declares the hook
        extension: String,
        /// The hook's name in the module's manifest
        hook: String,
        /// The memory limit, in bytes
        limit_bytes: usize,
        /// How the hook failed, with what it wrote to stderr
        message: String,
    },

    /// The hook returned more output than its limit.
    #[error(
        code = "HOOK_OUTPUT_LIMIT_EXCEEDED",
        message = "Hook '{hook}' of extension '{extension}' returned {output_bytes} bytes, over its {limit_bytes} byte limit",
        retryable = "false",
        internal = "false"
    )]
    HookOutputLimitExceeded {
        /// The extension that declares the hook
        extension: String,
        /// The hook's name in the module's manifest
        hook: String,
        /// The size of the output the hook returned
        output_bytes: usize,
        /// The output limit, in bytes
        limit_bytes: usize,
    },

    /// The hook trapped: it panicked, overflowed its stack, exited, or broke the module ABI.
    #[error(
        code = "HOOK_TRAPPED",
        message = "Hook '{hook}' of extension '{extension}' stopped: {message}",
        retryable = "false",
        internal = "false"
    )]
    HookTrapped {
        /// The extension that declares the hook
        extension: String,
        /// The hook's name in the module's manifest
        hook: String,
        /// How the hook stopped, with what it wrote to stderr
        message: String,
    },

    /// The WebAssembly engine could not start.
    #[error(
        code = "HOOKS_RUNTIME_START_FAILED",
        message = "The hooks runtime could not start: {message}",
        retryable = "false",
        internal = "true"
    )]
    RuntimeStartFailed {
        /// What failed
        message: String,
    },
}

/// Convenient type alias for this crate's Result type.
pub type Result<T> = alien_error::Result<T, ErrorData>;
