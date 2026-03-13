use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors that occur in permission operations.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Platform is not supported by the permission set.
    #[error(
        code = "PLATFORM_NOT_SUPPORTED",
        message = "Platform '{platform}' is not supported by permission set '{permission_set_id}'",
        retryable = "false",
        internal = "false"
    )]
    PlatformNotSupported {
        /// The unsupported platform name
        platform: String,
        /// ID of the permission set that doesn't support this platform
        permission_set_id: String,
    },

    /// Binding target is not supported by the platform.
    #[error(
        code = "BINDING_TARGET_NOT_SUPPORTED",
        message = "Binding target '{binding_target}' is not supported by platform '{platform}' in permission set '{permission_set_id}'",
        retryable = "false",
        internal = "false"
    )]
    BindingTargetNotSupported {
        /// The platform that doesn't support this binding target
        platform: String,
        /// The unsupported binding target type
        binding_target: String,
        /// ID of the permission set
        permission_set_id: String,
    },

    /// Required variable not found in permission context.
    #[error(
        code = "VARIABLE_NOT_FOUND",
        message = "Variable '{variable}' is not found in permission context",
        retryable = "false",
        internal = "false"
    )]
    VariableNotFound {
        /// Name of the missing variable
        variable: String,
    },

    /// Permission set format is invalid.
    #[error(
        code = "INVALID_PERMISSION_SET",
        message = "Invalid permission set format: {message}",
        retryable = "false",
        internal = "true"
    )]
    InvalidPermissionSet {
        /// Human-readable description of the format issue
        message: String,
    },

    /// Permission generator failed for the specified platform.
    #[error(
        code = "GENERATOR_ERROR",
        message = "Generator error for platform '{platform}': {message}",
        retryable = "false",
        internal = "true"
    )]
    GeneratorError {
        /// The platform where generation failed
        platform: String,
        /// Human-readable description of the generation failure
        message: String,
    },

    /// Serialization or deserialization of permission data failed.
    #[error(
        code = "SERIALIZATION_ERROR",
        message = "Serialization error: {message}",
        retryable = "false",
        internal = "true"
    )]
    SerializationError {
        /// Human-readable description of the serialization failure
        message: String,
    },

    /// Permission file I/O operation failed.
    #[error(
        code = "PERMISSION_FILE_IO_ERROR",
        message = "Permission file I/O operation failed on '{path}': {message}",
        retryable = "true",
        internal = "false"
    )]
    PermissionFileIoError {
        /// Path to the file that failed
        path: String,
        /// Human-readable description of the I/O failure
        message: String,
    },

    /// Generic permission error for uncommon cases.
    #[error(
        code = "PERMISSION_ERROR",
        message = "Permission operation failed: {message}",
        retryable = "false",
        internal = "true"
    )]
    Other {
        /// Human-readable description of the error
        message: String,
    },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
