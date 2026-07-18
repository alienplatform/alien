use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

use crate::{Platform, ResourceType};

/// Core error data exposed by the `alien-core` crate.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// A fallback error when nothing more specific matches.
    #[error(
        code = "GENERIC_ERROR",
        message = "{message}",
        retryable = "true",
        internal = "false"
    )]
    GenericError {
        /// Human-readable description of the error.
        message: String,
    },

    /// Resource type mismatch found during stack operations.
    #[error(
        code = "UNEXPECTED_RESOURCE_TYPE",
        message = "Unexpected resource type for resource '{resource_id}': expected {expected}, but got {actual}",
        retryable = "false",
        internal = "false"
    )]
    UnexpectedResourceType {
        resource_id: String,
        expected: ResourceType,
        actual: ResourceType,
    },

    /// Attempt to update a resource that does not support updates.
    #[error(
        code = "INVALID_RESOURCE_UPDATE",
        message = "Resource '{resource_id}' cannot be updated: {reason}",
        retryable = "false",
        internal = "false"
    )]
    InvalidResourceUpdate { resource_id: String, reason: String },

    /// Worker execution timeout is outside the supported range.
    #[error(
        code = "WORKER_TIMEOUT_INVALID",
        message = "Worker timeoutSeconds must be between 1 and {max_timeout_seconds} seconds, got {timeout_seconds}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    WorkerTimeoutInvalid {
        timeout_seconds: u32,
        max_timeout_seconds: u32,
    },

    /// The resource exists but has not produced any outputs yet.
    #[error(
        code = "RESOURCE_HAS_NO_OUTPUTS",
        message = "Resource '{resource_id}' has no outputs",
        retryable = "true",
        internal = "false"
    )]
    ResourceHasNoOutputs { resource_id: String },

    /// Requested resource absent from the stack state.
    #[error(
        code = "RESOURCE_NOT_FOUND",
        message = "Resource '{resource_id}' not found in stack state. Available resources: {available_resources:?}",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    ResourceNotFound {
        resource_id: String,
        available_resources: Vec<String>,
    },

    /// Binding configuration is invalid or missing required fields.
    #[error(
        code = "BINDING_CONFIG_INVALID",
        message = "Invalid binding configuration for '{binding_name}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    BindingConfigInvalid {
        binding_name: String,
        reason: String,
    },

    /// Environment variable for binding is missing.
    #[error(
        code = "BINDING_ENV_VAR_MISSING",
        message = "Missing environment variable '{env_var}' for binding '{binding_name}'",
        retryable = "false",
        internal = "false"
    )]
    BindingEnvVarMissing {
        binding_name: String,
        env_var: String,
    },

    /// Failed to parse binding JSON from environment variable.
    #[error(
        code = "BINDING_JSON_PARSE_FAILED",
        message = "Failed to parse binding JSON for '{binding_name}': {reason}",
        retryable = "false",
        internal = "false"
    )]
    BindingJsonParseFailed {
        binding_name: String,
        reason: String,
    },

    /// Unexpected combination of resource statuses when computing stack status.
    #[error(
        code = "UNEXPECTED_RESOURCE_STATUS_COMBINATION",
        message = "Unexpected resource status combination during {operation}: {resource_statuses:?}",
        retryable = "false",
        internal = "true"
    )]
    UnexpectedResourceStatusCombination {
        resource_statuses: Vec<String>,
        operation: String,
    },

    /// External binding type does not match the resource type.
    #[error(
        code = "EXTERNAL_BINDING_TYPE_MISMATCH",
        message = "External binding type mismatch for resource '{resource_id}': expected {expected}, got {actual}",
        retryable = "false",
        internal = "false"
    )]
    ExternalBindingTypeMismatch {
        resource_id: String,
        expected: String,
        actual: String,
    },

    // Presigned request errors
    /// Presigned request has expired.
    #[error(
        code = "PRESIGNED_REQUEST_EXPIRED",
        message = "Presigned request for '{path}' expired at {expired_at}",
        retryable = "false",
        internal = "false"
    )]
    PresignedRequestExpired {
        path: String,
        expired_at: chrono::DateTime<chrono::Utc>,
    },

    /// HTTP request failed.
    #[error(
        code = "HTTP_REQUEST_FAILED",
        message = "HTTP {method} request to '{url}' failed",
        retryable = "true",
        internal = "false"
    )]
    HttpRequestFailed { url: String, method: String },

    /// Operation not supported.
    #[error(
        code = "OPERATION_NOT_SUPPORTED",
        message = "Operation '{operation}' not supported: {reason}",
        retryable = "false",
        internal = "false"
    )]
    OperationNotSupported { operation: String, reason: String },

    /// Local filesystem error.
    #[error(
        code = "LOCAL_FILESYSTEM_ERROR",
        message = "Local filesystem error for '{path}' during {operation}",
        retryable = "false",
        internal = "false"
    )]
    LocalFilesystemError { path: String, operation: String },

    /// Feature not enabled.
    #[error(
        code = "FEATURE_NOT_ENABLED",
        message = "Feature '{feature}' is not enabled",
        retryable = "false",
        internal = "false"
    )]
    FeatureNotEnabled { feature: String },

    // Commands protocol errors
    /// Invalid Commands envelope.
    #[error(
        code = "INVALID_ENVELOPE",
        message = "Invalid Commands envelope: {message}",
        retryable = "false",
        internal = "false"
    )]
    InvalidEnvelope {
        message: String,
        field: Option<String>,
    },

    /// A command-enabled resource exists but the deployment has no token to
    /// authenticate its command transport.
    #[error(
        code = "COMMAND_TOKEN_MISSING",
        message = "Deployment has command-enabled resource '{resource_id}' but no deployment token to authenticate it: {reason}",
        retryable = "false",
        internal = "false"
    )]
    CommandTokenMissing { resource_id: String, reason: String },

    /// Public URL configuration is invalid.
    #[error(
        code = "PUBLIC_URL_INVALID",
        message = "Invalid public URL for resource '{resource_id}': {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    PublicUrlInvalid { resource_id: String, reason: String },

    /// JSON serialization failed.
    #[error(
        code = "JSON_SERIALIZATION_FAILED",
        message = "Failed to serialize JSON: {reason}",
        retryable = "false",
        internal = "false"
    )]
    JsonSerializationFailed { reason: String },

    /// JSON deserialization failed.
    #[error(
        code = "JSON_DESERIALIZATION_FAILED",
        message = "Failed to deserialize JSON: {reason}",
        retryable = "false",
        internal = "false"
    )]
    JsonDeserializationFailed { reason: String },

    /// Distribution template serialization failed.
    #[error(
        code = "TEMPLATE_SERIALIZATION_FAILED",
        message = "Failed to serialize {format} template: {reason}",
        retryable = "false",
        internal = "true"
    )]
    TemplateSerializationFailed { format: String, reason: String },

    /// ImportData did not match the registered typed payload.
    #[error(
        code = "IMPORT_DATA_INVALID",
        message = "Invalid ImportData for resource '{resource_id}' ({resource_type}) on {platform}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    ImportDataInvalid {
        resource_id: String,
        resource_type: ResourceType,
        platform: Platform,
    },

    /// No registry implementation exists for a resource/platform pair.
    #[error(
        code = "IMPORT_REGISTRATION_MISSING",
        message = "Missing {registration_kind} registration for resource type '{resource_type}' on {platform}",
        retryable = "false",
        internal = "true"
    )]
    ImportRegistrationMissing {
        resource_type: ResourceType,
        platform: Platform,
        registration_kind: String,
    },

    /// ImportData schema serialization failed.
    #[error(
        code = "IMPORT_SCHEMA_SERIALIZATION_FAILED",
        message = "Failed to serialize ImportData schema",
        retryable = "false",
        internal = "true"
    )]
    ImportSchemaSerializationFailed,
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
