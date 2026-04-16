use alien_core::Platform;
use alien_error::AlienErrorData;
use alien_error::{Context, IntoAlienError};
use serde::{Deserialize, Serialize};

/// Represents application-specific errors for alien-build.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Build configuration is invalid or missing required settings.
    #[error(
        code = "PACKAGE_CONFIG_INVALID",
        message = "Build configuration invalid: {message}",
        retryable = "false",
        internal = "false"
    )]
    BuildConfigInvalid {
        /// Human-readable description of the configuration issue
        message: String,
    },

    /// Resource configuration is invalid for the target platform.
    #[error(
        code = "INVALID_RESOURCE_ON_PLATFORM",
        message = "Resource type '{resource_type}' is not supported on platform '{platform}'",
        retryable = "false",
        internal = "false"
    )]
    InvalidResourceOnPlatform {
        /// The platform that was requested
        platform: Platform,
        /// The resource type that was requested
        resource_type: String,
    },

    /// Resource configuration is invalid or incomplete.
    #[error(
        code = "INVALID_RESOURCE_CONFIG",
        message = "Invalid resource configuration for '{resource_id}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    InvalidResourceConfig {
        /// The resource ID that has invalid configuration
        resource_id: String,
        /// Specific reason why the configuration is invalid
        reason: String,
    },

    /// Specified file or directory path not found.
    #[error(
        code = "PATH_NOT_FOUND",
        message = "Path '{path}' specified in function '{function_name}' does not exist",
        retryable = "false",
        internal = "false"
    )]
    PathNotFound {
        /// The path that was not found
        path: String,
        /// Name of the function that referenced the path
        function_name: String,
    },

    /// Glob pattern is invalid or cannot be processed.
    #[error(
        code = "INVALID_GLOB_PATTERN",
        message = "Invalid glob pattern '{pattern}' for function '{function_name}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    InvalidGlobPattern {
        /// The invalid glob pattern
        pattern: String,
        /// Name of the function that used the pattern
        function_name: String,
        /// Reason why the pattern is invalid
        reason: String,
    },

    /// Failed to download alien-runtime binary.
    #[error(
        code = "ALIEN_RUNTIME_DOWNLOAD_FAILED",
        message = "Failed to download alien-runtime from '{url}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    AlienRuntimeDownloadFailed {
        /// URL that was being downloaded from
        url: String,
        /// Reason for the download failure
        reason: String,
    },

    /// Container image build operation failed.
    #[error(
        code = "IMAGE_BUILD_FAILED",
        message = "Failed to build container image for function '{function_name}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    ImageBuildFailed {
        /// Name of the function being built
        function_name: String,
        /// Reason for the build failure
        reason: String,
        /// Full build output (stdout and stderr combined)
        build_output: Option<String>,
    },

    /// Container image push operation failed.
    #[error(
        code = "IMAGE_PUSH_FAILED",
        message = "Failed to push container image '{image}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    ImagePushFailed {
        /// Image name/URI that failed to push
        image: String,
        /// Reason for the push failure
        reason: String,
    },

    /// Template generation failed for the target platform.
    #[error(
        code = "TEMPLATE_GENERATION_FAILED",
        message = "Failed to generate infrastructure template for platform '{platform}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    TemplateGenerationFailed {
        /// Platform for which template generation failed
        platform: Platform,
        /// Reason for the generation failure
        reason: String,
    },

    /// File system operation failed during packaging.
    #[error(
        code = "FILE_OPERATION_FAILED",
        message = "Failed to {operation} '{file_path}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    FileOperationFailed {
        /// The operation that failed (e.g., "read", "write", "create directory", "remove directory")
        operation: String,
        /// Path to the file or directory that failed
        file_path: String,
        /// Reason for the operation failure
        reason: String,
    },

    /// JSON serialization or deserialization failed.
    #[error(
        code = "JSON_SERIALIZATION_ERROR",
        message = "JSON serialization error: {message}",
        retryable = "false",
        internal = "true"
    )]
    JsonSerializationError {
        /// Description of the serialization error
        message: String,
    },

    /// URL parsing failed.
    #[error(
        code = "URL_PARSE_ERROR",
        message = "Invalid URL format '{url}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    UrlParseError {
        /// The URL that failed to parse
        url: String,
        /// Reason for the parsing failure
        reason: String,
    },

    /// Network or request-level failure when sending HTTP request.
    #[error(
        code = "HTTP_REQUEST_FAILED",
        message = "{message}",
        retryable = "true",
        internal = "false"
    )]
    HttpRequestFailed {
        /// Human-readable description of the HTTP request failure
        message: String,
        /// Optional URL that was being requested
        url: Option<String>,
    },

    /// Docker/container operation failed.
    #[error(
        code = "CONTAINER_OPERATION_FAILED",
        message = "Container operation failed",
        retryable = "inherit",
        internal = "inherit"
    )]
    ContainerOperationFailed,

    /// Event emission failed during packaging.
    #[error(
        code = "EVENT_EMISSION_FAILED",
        message = "Failed to emit packaging event '{event_type}': {reason}",
        retryable = "true",
        internal = "true"
    )]
    EventEmissionFailed {
        /// Type of event that failed to emit
        event_type: String,
        /// Reason for the emission failure
        reason: String,
    },

    /// Service account creation failed during stack processing.
    #[error(
        code = "SERVICE_ACCOUNT_CREATION_FAILED",
        message = "Failed to create service account '{service_account_id}' from permission profile",
        retryable = "false",
        internal = "false"
    )]
    ServiceAccountCreationFailed {
        /// The service account ID that failed to be created
        service_account_id: String,
    },

    /// Stack processing failed during build.
    #[error(
        code = "STACK_PROCESSOR_FAILED",
        message = "Stack processing failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    StackProcessorFailed {
        /// Description of the processing failure
        message: String,
    },

    /// Resource build was canceled due to fail-fast behavior.
    #[error(
        code = "BUILD_CANCELED",
        message = "Build for resource '{resource_name}' was canceled",
        retryable = "false",
        internal = "false"
    )]
    BuildCanceled {
        /// Name of the resource whose build was canceled
        resource_name: String,
    },
}

/// Extension trait to convert dockdash errors to our error type
pub trait DockdashResultExt<T> {
    fn map_dockdash_err(self) -> Result<T>;
}

impl<T> DockdashResultExt<T> for std::result::Result<T, dockdash::Error> {
    fn map_dockdash_err(self) -> Result<T> {
        self.into_alien_error()
            .context(ErrorData::ContainerOperationFailed {})
    }
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
