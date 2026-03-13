use alien_error::{AlienError, AlienErrorData};
use serde::{Deserialize, Serialize};

/// Errors that can occur in the alien-test-server.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Binding configuration is invalid or binding not found.
    #[error(
        code = "BINDING_NOT_FOUND",
        message = "Binding '{binding_name}' not found or invalid",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    BindingNotFound {
        /// Name of the binding that was not found
        binding_name: String,
    },

    /// Build operation failed.
    #[error(
        code = "BUILD_OPERATION_FAILED",
        message = "Build operation failed: {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    BuildOperationFailed {
        /// Description of the build operation that failed
        operation: String,
    },

    /// Artifact registry operation failed.
    #[error(
        code = "ARTIFACT_REGISTRY_OPERATION_FAILED",
        message = "Artifact registry operation failed: {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = "inherit"
    )]
    ArtifactRegistryOperationFailed {
        /// Description of the artifact registry operation that failed
        operation: String,
    },

    /// Storage operation failed.
    #[error(
        code = "STORAGE_OPERATION_FAILED",
        message = "Storage operation failed: {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    StorageOperationFailed {
        /// Description of the storage operation that failed
        operation: String,
    },

    /// Environment variable operation failed.
    #[error(
        code = "ENV_VAR_NOT_FOUND",
        message = "Environment variable '{var_name}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    EnvVarNotFound {
        /// Name of the environment variable that was not found
        var_name: String,
    },

    /// Docker operation failed.
    #[error(
        code = "DOCKER_OPERATION_FAILED",
        message = "Docker operation failed: {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    DockerOperationFailed {
        /// Description of the Docker operation that failed
        operation: String,
    },

    /// Test validation failed.
    #[error(
        code = "TEST_VALIDATION_FAILED",
        message = "Test validation failed: {reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    TestValidationFailed {
        /// Reason why the test validation failed
        reason: String,
    },

    /// WaitUntil test operation failed.
    #[error(
        code = "WAIT_UNTIL_TEST_FAILED",
        message = "WaitUntil test failed: {message}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    WaitUntilTestFailed {
        /// Description of what failed during the wait_until test
        message: String,
    },

    /// Vault operation failed.
    #[error(
        code = "VAULT_OPERATION_FAILED",
        message = "Vault operation failed: {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    VaultOperationFailed {
        /// Description of the vault operation that failed
        operation: String,
    },

    /// KV operation failed.
    #[error(
        code = "KV_OPERATION_FAILED",
        message = "KV operation failed: {operation} on key '{key}' - {reason}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    KvOperationFailed {
        /// Description of the KV operation that failed
        operation: String,
        /// The key being operated on
        key: String,
        /// Reason for the failure
        reason: String,
    },

    /// Queue operation failed.
    #[error(
        code = "QUEUE_OPERATION_FAILED",
        message = "Queue operation failed: {operation}",
        retryable = "true",
        internal = "false",
        http_status_code = 500
    )]
    QueueOperationFailed {
        /// Description of the queue operation that failed
        operation: String,
    },

    /// Serialization operation failed.
    #[error(
        code = "SERIALIZATION_FAILED",
        message = "Serialization failed: {message}",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    SerializationFailed {
        /// Description of what failed to serialize
        message: String,
    },

    /// Generic test server error for uncommon cases.
    #[error(
        code = "TEST_SERVER_ERROR",
        message = "Test server error: {message}",
        retryable = "true",
        internal = "true",
        http_status_code = 500
    )]
    Other {
        /// Human-readable description of the error
        message: String,
    },
}

/// Result type for alien-test-server operations.
pub type Result<T> = alien_error::Result<T, ErrorData>;

/// Error type for alien-test-server operations.
pub type Error = AlienError<ErrorData>;
