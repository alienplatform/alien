use crate::CheckResult;
use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Error data types for alien-preflights operations
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// A compile-time check failed
    #[error(
        code = "COMPILE_TIME_CHECK_FAILED",
        message = "Compile-time check '{check_name}' failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    CompileTimeCheckFailed {
        /// Name of the check that failed
        check_name: String,
        /// Detailed error message
        message: String,
        /// Resource ID that caused the failure (if applicable)
        resource_id: Option<String>,
    },

    /// A stack compatibility check failed
    #[error(
        code = "STACK_COMPATIBILITY_CHECK_FAILED",
        message = "Stack compatibility check '{check_name}' failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    StackCompatibilityCheckFailed {
        /// Name of the check that failed
        check_name: String,
        /// Detailed error message
        message: String,
        /// Old resource ID (if applicable)
        old_resource_id: Option<String>,
        /// New resource ID (if applicable)
        new_resource_id: Option<String>,
    },

    /// A runtime check failed
    #[error(
        code = "RUNTIME_CHECK_FAILED",
        message = "Runtime check '{check_name}' failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    RuntimeCheckFailed {
        /// Name of the check that failed
        check_name: String,
        /// Detailed error message
        message: String,
        /// Platform being checked
        platform: Option<String>,
    },

    /// A stack mutation failed
    #[error(
        code = "STACK_MUTATION_FAILED",
        message = "Stack mutation '{mutation_name}' failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    StackMutationFailed {
        /// Name of the mutation that failed
        mutation_name: String,
        /// Detailed error message
        message: String,
        /// Resource ID that caused the failure (if applicable)
        resource_id: Option<String>,
    },

    /// Multiple validation errors occurred
    #[error(
        code = "VALIDATION_FAILED",
        message = "Validation failed with {error_count} errors and {warning_count} warnings",
        retryable = "false",
        internal = "false"
    )]
    ValidationFailed {
        /// Number of errors
        error_count: usize,
        /// Number of warnings
        warning_count: usize,
        /// Detailed validation results
        results: Vec<CheckResult>,
    },

    /// A required permission set was not found
    #[error(
        code = "PERMISSION_SET_NOT_FOUND",
        message = "Permission set '{permission_set_id}' not found in registry",
        retryable = "false",
        internal = "false"
    )]
    PermissionSetNotFound {
        /// Permission set ID that was not found
        permission_set_id: String,
    },

    /// A resource dependency is invalid
    #[error(
        code = "INVALID_RESOURCE_DEPENDENCY",
        message = "Invalid resource dependency: {resource_id} depends on {dependency_id}: {reason}",
        retryable = "false",
        internal = "false"
    )]
    InvalidResourceDependency {
        /// Resource ID with the invalid dependency
        resource_id: String,
        /// Dependency ID that is invalid
        dependency_id: String,
        /// Reason why the dependency is invalid
        reason: String,
    },

    /// Cloud API access failed during runtime checks
    #[error(
        code = "CLOUD_API_ACCESS_FAILED",
        message = "Cloud API access failed for '{operation}' on {provider}: {message}",
        retryable = "true",
        internal = "false"
    )]
    CloudApiAccessFailed {
        /// Operation that failed
        operation: String,
        /// Cloud provider
        provider: String,
        /// Underlying error message
        message: String,
    },

    /// Circular dependency detected
    #[error(
        code = "CIRCULAR_DEPENDENCY_DETECTED",
        message = "Circular dependency detected in resource chain: {resource_chain:?}",
        retryable = "false",
        internal = "false"
    )]
    CircularDependencyDetected {
        /// Resource chain that forms the cycle
        resource_chain: Vec<String>,
    },

    /// Resource validation failed
    #[error(
        code = "RESOURCE_VALIDATION_FAILED",
        message = "Resource validation failed for '{resource_id}': {message}",
        retryable = "false",
        internal = "false"
    )]
    ResourceValidationFailed {
        /// Resource ID that failed validation
        resource_id: String,
        /// Detailed error message
        message: String,
    },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
