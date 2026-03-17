use alien_error::{AlienError, AlienErrorData, ContextError};
use serde::{Deserialize, Serialize};

/// Errors related to alien-bindings operations.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Binding provider configuration is invalid or missing.
    #[error(
        code = "BINDING_CONFIG_INVALID",
        message = "Binding configuration invalid for binding '{binding_name}': {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    BindingConfigInvalid {
        /// Name of the binding
        binding_name: String,
        /// Specific reason why the configuration is invalid
        reason: String,
    },

    /// Storage operation failed due to provider issues.
    #[error(
        code = "STORAGE_OPERATION_FAILED",
        message = "Storage operation failed for binding '{binding_name}': {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    StorageOperationFailed {
        /// Name of the storage binding
        binding_name: String,
        /// Description of the operation that failed
        operation: String,
    },

    /// Build operation failed due to provider issues.
    #[error(
        code = "BUILD_OPERATION_FAILED",
        message = "Build operation failed for binding '{binding_name}': {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    BuildOperationFailed {
        /// Name of the build binding
        binding_name: String,
        /// Description of the operation that failed
        operation: String,
    },

    /// Required environment variable is missing or invalid.
    #[error(
        code = "ENVIRONMENT_VARIABLE_MISSING",
        message = "Required environment variable '{variable_name}' is missing",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    EnvironmentVariableMissing {
        /// Name of the missing environment variable
        variable_name: String,
    },

    /// Environment variable has an invalid value.
    #[error(
        code = "INVALID_ENVIRONMENT_VARIABLE",
        message = "Environment variable '{variable_name}' has invalid value '{value}': {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    InvalidEnvironmentVariable {
        /// Name of the environment variable
        variable_name: String,
        /// The invalid value
        value: String,
        /// Reason why the value is invalid
        reason: String,
    },

    /// Configuration URL is malformed or invalid.
    #[error(
        code = "INVALID_CONFIGURATION_URL",
        message = "Invalid configuration URL '{url}': {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    InvalidConfigurationUrl {
        /// The invalid URL
        url: String,
        /// Specific reason why the URL is invalid
        reason: String,
    },

    /// Event processing failed due to malformed or unsupported data.
    #[error(
        code = "EVENT_PROCESSING_FAILED",
        message = "Event processing failed for type '{event_type}': {reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 400
    )]
    EventProcessingFailed {
        /// Type of event that failed to process
        event_type: String,
        /// Specific reason for the processing failure
        reason: String,
    },

    /// gRPC connection failed or became unavailable.
    #[error(
        code = "GRPC_CONNECTION_FAILED",
        message = "gRPC connection failed to endpoint '{endpoint}': {reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    GrpcConnectionFailed {
        /// The gRPC endpoint that failed to connect
        endpoint: String,
        /// Reason for the connection failure
        reason: String,
    },

    /// gRPC service unavailable or returned error.
    #[error(
        code = "GRPC_SERVICE_UNAVAILABLE",
        message = "gRPC service '{service}' unavailable at endpoint '{endpoint}': {reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 503
    )]
    GrpcServiceUnavailable {
        /// Name of the gRPC service
        service: String,
        /// The gRPC endpoint
        endpoint: String,
        /// Reason for service unavailability
        reason: String,
    },

    /// gRPC request failed with an error status.
    #[error(
        code = "GRPC_REQUEST_FAILED",
        message = "gRPC request to service '{service}' method '{method}' failed: {details}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    GrpcRequestFailed {
        /// Name of the gRPC service
        service: String,
        /// Name of the gRPC method
        method: String,
        /// Error details from the gRPC status
        details: String,
    },

    /// Server failed to bind to the specified address.
    #[error(
        code = "SERVER_BIND_FAILED",
        message = "Failed to bind server to address '{address}': {reason}",
        retryable = "true",
        internal = "true",
        http_status_code = 500
    )]
    ServerBindFailed {
        /// The address that failed to bind
        address: String,
        /// Reason for the bind failure
        reason: String,
    },

    /// Authentication failed for the configured provider.
    #[error(
        code = "AUTHENTICATION_FAILED",
        message = "Authentication failed for provider '{provider}' and binding '{binding_name}': {reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 401
    )]
    AuthenticationFailed {
        /// Name of the provider (aws, gcp, azure, etc.)
        provider: String,
        /// Name of the binding
        binding_name: String,
        /// Reason for authentication failure
        reason: String,
    },

    /// Operation not supported by the configured provider.
    #[error(
        code = "OPERATION_NOT_SUPPORTED",
        message = "Operation '{operation}' not supported: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 501
    )]
    OperationNotSupported {
        /// Name of the unsupported operation
        operation: String,
        /// Reason why the operation is not supported
        reason: String,
    },

    /// Feature is not enabled in the compiled binary.
    #[error(
        code = "FEATURE_NOT_ENABLED",
        message = "Feature '{feature}' is not enabled in this build",
        retryable = "false",
        internal = "false",
        http_status_code = 501
    )]
    FeatureNotEnabled {
        /// Name of the feature that is not enabled
        feature: String,
    },

    /// gRPC call failed.
    #[error(
        code = "GRPC_CALL_FAILED",
        message = "gRPC call to service '{service}' method '{method}' failed: {reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    GrpcCallFailed {
        /// Name of the gRPC service
        service: String,
        /// Name of the gRPC method
        method: String,
        /// Reason for the call failure
        reason: String,
    },

    /// Deserialization of data failed.
    #[error(
        code = "DESERIALIZATION_FAILED",
        message = "Failed to deserialize {type_name}: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    DeserializationFailed {
        /// Human-readable error message
        message: String,
        /// Name of the type being deserialized
        type_name: String,
    },

    /// Serialization of data failed.
    #[error(
        code = "SERIALIZATION_FAILED",
        message = "Failed to serialize data: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    SerializationFailed {
        /// Human-readable error message
        message: String,
    },

    /// Operation is not implemented yet.
    #[error(
        code = "NOT_IMPLEMENTED",
        message = "Operation '{operation}' is not implemented: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 501
    )]
    NotImplemented {
        /// Name of the operation that is not implemented
        operation: String,
        /// Reason why the operation is not implemented
        reason: String,
    },

    /// Response format from provider API is unexpected or missing required fields.
    #[error(
        code = "UNEXPECTED_RESPONSE_FORMAT",
        message = "Unexpected response format from '{provider}' for binding '{binding_name}': missing field '{field}'. Response: {response_json}",
        retryable = "false",
        internal = "false",
        http_status_code = 502
    )]
    UnexpectedResponseFormat {
        /// Name of the provider (aws, gcp, azure, etc.)
        provider: String,
        /// Name of the binding
        binding_name: String,
        /// Name of the missing or malformed field
        field: String,
        /// The full response JSON for debugging
        response_json: String,
    },

    /// Cloud platform API error.
    #[error(
        code = "CLOUD_PLATFORM_ERROR",
        message = "Cloud platform error: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    CloudPlatformError {
        /// Human-readable description of the error
        message: String,
        /// Optional resource ID that was involved in the error
        resource_id: Option<String>,
    },

    /// Resource not found in the cloud platform.
    #[error(
        code = "RESOURCE_NOT_FOUND",
        message = "Resource '{resource_id}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    ResourceNotFound {
        /// ID of the resource that was not found
        resource_id: String,
    },

    /// The requested remote resource does not exist.
    #[error(
        code = "REMOTE_RESOURCE_NOT_FOUND",
        message = "{operation_context}: {resource_type} '{resource_name}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    RemoteResourceNotFound {
        /// Context of the operation that failed (e.g., "Failed to get ECR repository details")
        operation_context: String,
        /// Type of the resource that was not found
        resource_type: String,
        /// Name of the resource that was not found
        resource_name: String,
    },

    /// Operation conflicts with current remote resource state.
    #[error(
        code = "REMOTE_RESOURCE_CONFLICT",
        message = "{operation_context}: Conflict with {resource_type} '{resource_name}' - {conflict_reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 409
    )]
    RemoteResourceConflict {
        /// Context of the operation that failed
        operation_context: String,
        /// Type of the resource that has a conflict
        resource_type: String,
        /// Name of the resource that has a conflict
        resource_name: String,
        /// Specific reason for the conflict
        conflict_reason: String,
    },

    /// Access denied due to insufficient permissions.
    #[error(
        code = "REMOTE_ACCESS_DENIED",
        message = "{operation_context}: Access denied to {resource_type} '{resource_name}'",
        retryable = "true",
        internal = "false",
        http_status_code = 403
    )]
    RemoteAccessDenied {
        /// Context of the operation that failed
        operation_context: String,
        /// Type of the resource access was denied to
        resource_type: String,
        /// Name of the resource access was denied to
        resource_name: String,
    },

    /// Request rate limit exceeded.
    #[error(
        code = "RATE_LIMIT_EXCEEDED",
        message = "{operation_context}: Rate limit exceeded - {details}",
        retryable = "true",
        internal = "false",
        http_status_code = 429
    )]
    RateLimitExceeded {
        /// Context of the operation that failed
        operation_context: String,
        /// Additional details about the rate limit
        details: String,
    },

    /// Operation exceeded the allowed timeout.
    #[error(
        code = "TIMEOUT",
        message = "{operation_context}: Operation timed out - {details}",
        retryable = "true",
        internal = "false",
        http_status_code = 408
    )]
    Timeout {
        /// Context of the operation that failed
        operation_context: String,
        /// Additional details about the timeout
        details: String,
    },

    /// Remote service is temporarily unavailable.
    #[error(
        code = "REMOTE_SERVICE_UNAVAILABLE",
        message = "{operation_context}: Service unavailable - {details}",
        retryable = "true",
        internal = "false",
        http_status_code = 503
    )]
    RemoteServiceUnavailable {
        /// Context of the operation that failed
        operation_context: String,
        /// Additional details about the service unavailability
        details: String,
    },

    /// Quota or resource limits have been exceeded.
    #[error(
        code = "QUOTA_EXCEEDED",
        message = "{operation_context}: Quota exceeded - {details}",
        retryable = "true",
        internal = "false",
        http_status_code = 429
    )]
    QuotaExceeded {
        /// Context of the operation that failed
        operation_context: String,
        /// Additional details about the quota violation
        details: String,
    },

    /// Invalid or malformed input parameters provided to the operation.
    #[error(
        code = "INVALID_INPUT",
        message = "{operation_context}: Invalid input - {details}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    InvalidInput {
        /// Context of the operation that failed
        operation_context: String,
        /// Details about what input was invalid
        details: String,
        /// Optional field name that was invalid
        field_name: Option<String>,
    },

    /// Authentication with cloud provider failed.
    #[error(
        code = "AUTHENTICATION_ERROR",
        message = "{operation_context}: Authentication failed - {details}",
        retryable = "true",
        internal = "false",
        http_status_code = 401
    )]
    AuthenticationError {
        /// Context of the operation that failed
        operation_context: String,
        /// Details about the authentication failure
        details: String,
    },

    /// Generic bindings error for uncommon cases.
    #[error(
        code = "BINDINGS_ERROR",
        message = "Bindings error: {message}",
        retryable = "true",
        internal = "true",
        http_status_code = 500
    )]
    Other {
        /// Human-readable description of the error
        message: String,
    },

    /// Presigned request has expired and can no longer be used.
    #[error(
        code = "PRESIGNED_REQUEST_EXPIRED",
        message = "Presigned request for path '{path}' expired at {expired_at}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    PresignedRequestExpired {
        /// Path that the presigned request was for
        path: String,
        /// When the request expired
        expired_at: chrono::DateTime<chrono::Utc>,
    },

    /// HTTP request to external service failed.
    #[error(
        code = "HTTP_REQUEST_FAILED",
        message = "HTTP {method} request to '{url}' failed",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    HttpRequestFailed {
        /// URL that was requested
        url: String,
        /// HTTP method that was used
        method: String,
    },

    /// Local filesystem operation failed.
    #[error(
        code = "LOCAL_FILESYSTEM_ERROR",
        message = "Local filesystem operation '{operation}' failed for path '{path}'",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    LocalFilesystemError {
        /// Path that the operation was performed on
        path: String,
        /// Operation that failed
        operation: String,
    },

    /// Failed to load platform configuration for the provider.
    #[error(
        code = "client_config_LOAD_FAILED",
        message = "Failed to load platform configuration for provider '{provider}'",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    ClientConfigLoadFailed {
        /// Name of the provider (aws, gcp, azure, etc.)
        provider: String,
    },

    /// Binding setup failed during initialization.
    #[error(
        code = "BINDING_SETUP_FAILED",
        message = "Binding setup failed for type '{binding_type}': {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    BindingSetupFailed {
        /// Type of binding being set up
        binding_type: String,
        /// Reason for the setup failure
        reason: String,
    },

    /// KV operation failed.
    #[error(
        code = "KV_OPERATION_FAILED",
        message = "KV operation '{operation}' failed for key '{key}': {reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    KvOperationFailed {
        /// The KV operation that failed
        operation: String,
        /// The key involved in the operation
        key: String,
        /// Reason for the operation failure
        reason: String,
    },

    /// Remote access to deployment resources failed.
    #[error(
        code = "REMOTE_ACCESS_FAILED",
        message = "Remote access failed during operation: {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 502
    )]
    RemoteAccessFailed {
        /// Description of the operation that failed
        operation: String,
    },

    /// Client configuration is invalid or missing for the platform.
    #[error(
        code = "CLIENT_CONFIG_INVALID",
        message = "Client configuration invalid for platform '{platform}': {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    ClientConfigInvalid {
        /// The platform that was expected
        platform: alien_core::Platform,
        /// Description of the configuration issue
        message: String,
    },
}

/// Convenient alias with default error type `ErrorData`.
pub type Result<T, E = ErrorData> = alien_error::Result<T, E>;

/// Convenience alias representing a constructed AlienError with our `ErrorData` payload.
pub type Error = AlienError<ErrorData>;

/// Maps an `alien_client_core::Error` to an appropriate `alien_bindings::Error`.
///
/// Important error types (like resource not found, access denied, etc.) are mapped
/// to their corresponding variants in alien-bindings while preserving the operation context.
/// Less important errors are wrapped in `CloudPlatformError`.
///
/// # Arguments
/// * `cloud_error` - The error from cloud client crates
/// * `operation_context` - Description of the operation that failed (e.g., "Failed to get ECR repository details")
/// * `resource_id` - Optional resource ID for fallback error context
///
/// # Example
/// ```rust
/// use alien_bindings::error::map_cloud_client_error;
///
/// async fn example() {
///     // This would be an actual cloud client operation
///     let result = some_cloud_operation().await
///         .map_err(|e| map_cloud_client_error(e, "Failed to get ECR repository details".to_string(), Some("my-repo".to_string())));
/// }
///
/// async fn some_cloud_operation() -> Result<(), alien_client_core::Error> {
///     // Mock implementation
///     Ok(())
/// }
/// ```
pub fn map_cloud_client_error(
    cloud_error: alien_client_core::Error,
    operation_context: String,
    resource_id: Option<String>,
) -> Error {
    use alien_client_core::ErrorData as CloudErrorData;

    // Check the error type first to determine the right context to add
    let error_data = match cloud_error.error.as_ref() {
        Some(CloudErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
        }) => ErrorData::RemoteResourceNotFound {
            operation_context,
            resource_type: resource_type.clone(),
            resource_name: resource_name.clone(),
        },
        Some(CloudErrorData::RemoteResourceConflict {
            resource_type,
            resource_name,
            message,
        }) => ErrorData::RemoteResourceConflict {
            operation_context,
            resource_type: resource_type.clone(),
            resource_name: resource_name.clone(),
            conflict_reason: message.clone(),
        },
        Some(CloudErrorData::RemoteAccessDenied {
            resource_type,
            resource_name,
        }) => ErrorData::RemoteAccessDenied {
            operation_context,
            resource_type: resource_type.clone(),
            resource_name: resource_name.clone(),
        },
        Some(CloudErrorData::RateLimitExceeded { message }) => ErrorData::RateLimitExceeded {
            operation_context,
            details: message.clone(),
        },
        Some(CloudErrorData::Timeout { message }) => ErrorData::Timeout {
            operation_context,
            details: message.clone(),
        },
        Some(CloudErrorData::RemoteServiceUnavailable { message }) => {
            ErrorData::RemoteServiceUnavailable {
                operation_context,
                details: message.clone(),
            }
        }
        Some(CloudErrorData::QuotaExceeded { message }) => ErrorData::QuotaExceeded {
            operation_context,
            details: message.clone(),
        },
        Some(CloudErrorData::InvalidInput {
            message,
            field_name,
        }) => ErrorData::InvalidInput {
            operation_context,
            details: message.clone(),
            field_name: field_name.clone(),
        },
        Some(CloudErrorData::AuthenticationError { message }) => ErrorData::AuthenticationError {
            operation_context,
            details: message.clone(),
        },
        // For other error types or None, wrap in CloudPlatformError
        _ => ErrorData::CloudPlatformError {
            message: operation_context,
            resource_id,
        },
    };

    // Now add the context to the cloud error
    cloud_error.context(error_data)
}
