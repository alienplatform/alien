use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors that occur in permission operations.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The generated boundary exceeds the AWS managed-policy size limit.
    #[error(
        code = "PERMISSIONS_BOUNDARY_TOO_LARGE",
        message = "AWS permissions boundary is {size} characters; the limit is {limit}. Largest contributors: {largest_contributors}. Reduce custom runtime permissions, especially grants on resources outside the stack's prefix.",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    PermissionsBoundaryTooLarge {
        /// Minified size of the rendered document
        size: usize,
        /// IAM's managed-policy size limit
        limit: usize,
        /// The grant sources adding the most characters, each with an example resource
        largest_contributors: String,
    },

    /// An operation requests permissions outside its reviewed capability or installer scope.
    #[error(
        code = "OPERATION_PERMISSION_INVALID",
        message = "{message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    OperationPermissionInvalid {
        message: String,
        action: String,
        resource: String,
    },

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

    /// A protected install would receive the grants kept for setups without a boundary.
    #[error(
        code = "PROTECTED_DEPLOYMENT_WITHOUT_BOUNDARY",
        message = "Permission set '{permission_set_id}' grants '{label}' only to setups that recorded no permissions boundary, and a protected install never receives it. Rerun setup so it records the boundary.",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    ProtectedDeploymentWithoutBoundary {
        /// Permission set holding the legacy grant
        permission_set_id: String,
        /// Label of the legacy grant
        label: String,
    },

    /// A grant to a protected install's management identity is not on the allow-list for its
    /// cloud.
    #[error(
        code = "PROTECTED_MANAGEMENT_ACTION_REFUSED",
        message = "Permission set '{permission_set_id}' grants '{action}' to the management identity, which a protected install does not allow: it is not covered by the allow-list at {allow_list}. Remove it from the permission set, or add a line with a reason to that file if it cannot change what runs or reach signing secrets.",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    ProtectedManagementActionRefused {
        /// Permission set holding the grant
        permission_set_id: String,
        /// The refused action pattern, permission or role
        action: String,
        /// The allow-list file that would have to cover it
        allow_list: String,
    },

    /// A permission set grants the management identity an action in a family it never holds on a
    /// protected install.
    #[error(
        code = "PROTECTED_MANAGEMENT_FAMILY_ACTION",
        message = "Permission set '{permission_set_id}' grants '{action}' to the management identity, which a protected install does not allow: it can {family} (run unsigned code, change an identity that could, or open the install to outside access). Only the verifier writes this. Remove it from the permission set, or narrow the pattern to actions outside that family.",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    ProtectedManagementFamilyAction {
        /// Permission set holding the grant
        permission_set_id: String,
        /// The refused action pattern, permission or role
        action: String,
        /// The family the action can belong to: RUN, IDENTITY or REACH
        family: String,
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
