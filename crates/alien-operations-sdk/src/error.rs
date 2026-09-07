//! Error types for the operations plugin SDK.

use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors related to declaring, validating, and running an operations plugin.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// A manifest (`metadata.json`) was missing, unreadable, or malformed.
    #[error(
        code = "PLUGIN_MANIFEST_INVALID",
        message = "Plugin manifest is invalid: {reason}",
        retryable = "false",
        internal = "false"
    )]
    ManifestInvalid {
        /// What was wrong with the manifest.
        reason: String,
    },

    /// A manifest declared two operations with the same name.
    #[error(
        code = "PLUGIN_OPERATION_DUPLICATE",
        message = "Plugin '{plugin}' declares operation '{operation}' more than once",
        retryable = "false",
        internal = "false"
    )]
    OperationDuplicate {
        /// Plugin name.
        plugin: String,
        /// The operation name declared more than once.
        operation: String,
    },

    /// A required identifier field (plugin name, version, operation name, or
    /// a binary entry) was empty or all whitespace. Downstream commands
    /// (`package`, `publish`) derive file paths and bundle entries from
    /// these fields, so an empty one produces a confusing failure far from
    /// the manifest that caused it — reject it here instead, at the point
    /// `check` already validates.
    #[error(
        code = "PLUGIN_MANIFEST_FIELD_EMPTY",
        message = "Plugin manifest field '{field}' must not be empty",
        retryable = "false",
        internal = "false"
    )]
    FieldEmpty {
        /// A description of which field was empty, e.g. "name",
        /// "operations[1].name", "binaries.amd64".
        field: String,
    },

    /// A verification's `pollOperation` does not name a read-only operation
    /// declared by the same plugin.
    #[error(
        code = "PLUGIN_VERIFICATION_POLL_INVALID",
        message = "Plugin '{plugin}' operation '{operation}' verification pollOperation \
                    '{poll_operation}' must name a read-only operation in the same plugin",
        retryable = "false",
        internal = "false"
    )]
    VerificationPollInvalid {
        /// Plugin name.
        plugin: String,
        /// The operation whose verification is invalid.
        operation: String,
        /// The `pollOperation` value that failed validation.
        poll_operation: String,
    },

    /// A requested operation is not exposed by the plugin.
    #[error(
        code = "PLUGIN_OPERATION_UNKNOWN",
        message = "Plugin '{plugin}' does not expose operation '{operation}'",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    OperationUnknown {
        /// Plugin name.
        plugin: String,
        /// The operation that was requested.
        operation: String,
    },

    /// The plugin process could not parse an invocation from stdin.
    #[error(
        code = "PLUGIN_PROTOCOL_VIOLATION",
        message = "Plugin '{plugin}' received an unparseable invocation: {reason}",
        retryable = "false",
        internal = "false"
    )]
    ProtocolViolation {
        /// Plugin name.
        plugin: String,
        /// What made the input unparseable.
        reason: String,
    },

    /// Invocation params did not match the operation's declared params schema.
    #[error(
        code = "PLUGIN_PARAMS_INVALID",
        message = "Plugin '{plugin}' operation '{operation}' received invalid params: {reason}",
        retryable = "false",
        internal = "false"
    )]
    ParamsInvalid {
        /// Plugin name.
        plugin: String,
        /// The operation whose params failed validation.
        operation: String,
        /// Why validation failed.
        reason: String,
    },
}

/// Convenient alias for this crate's `Result` type.
pub type Result<T> = alien_error::Result<T, ErrorData>;
