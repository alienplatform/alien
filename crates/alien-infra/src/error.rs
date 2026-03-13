use alien_core::{Platform, ResourceType};
use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Represents application-specific errors for alien-infra.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Resource configuration validation failed.
    #[error(
        code = "RESOURCE_CONFIG_INVALID",
        message = "Resource configuration validation failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    ResourceConfigInvalid {
        /// Human-readable description of the validation failure
        message: String,
        /// ID of the resource that failed validation
        resource_id: Option<String>,
    },

    /// Resource state serialization or deserialization failed.
    #[error(
        code = "RESOURCE_STATE_SERIALIZATION_FAILED",
        message = "Resource state serialization failed for '{resource_id}': {message}",
        retryable = "false",
        internal = "true"
    )]
    ResourceStateSerializationFailed {
        /// ID of the resource whose state failed to serialize/deserialize
        resource_id: String,
        /// Human-readable description of the serialization failure
        message: String,
    },

    /// Circular dependency detected in resource dependency graph.
    #[error(
        code = "CIRCULAR_DEPENDENCY_DETECTED",
        message = "Circular dependency detected in resource graph involving: {resource_ids:?}",
        retryable = "false",
        internal = "false"
    )]
    CircularDependencyDetected {
        /// List of resource IDs involved in the circular dependency
        resource_ids: Vec<String>,
    },

    /// Required dependency resource not found in stack configuration.
    #[error(
        code = "DEPENDENCY_NOT_FOUND",
        message = "Resource '{resource_id}' depends on '{dependency_id}' which is not defined in the stack",
        retryable = "false",
        internal = "false"
    )]
    DependencyNotFound {
        /// ID of the resource that has the dependency
        resource_id: String,
        /// ID of the missing dependency resource
        dependency_id: String,
    },

    /// Resource depends on another resource that was filtered out by lifecycle filter.
    #[error(
        code = "FILTERED_DEPENDENCY_CONFLICT",
        message = "Resource '{resource_id}' depends on '{dependency_id}' which is excluded by the lifecycle filter",
        retryable = "false",
        internal = "false"
    )]
    FilteredDependencyConflict {
        /// ID of the resource that has the dependency
        resource_id: String,
        /// ID of the dependency that was filtered out
        dependency_id: String,
    },

    /// Dependency resource is not ready for consumption.
    #[error(
        code = "DEPENDENCY_NOT_READY",
        message = "Resource '{resource_id}' depends on '{dependency_id}' which is not yet ready",
        retryable = "true",
        internal = "false"
    )]
    DependencyNotReady {
        /// ID of the resource waiting for the dependency
        resource_id: String,
        /// ID of the dependency that's not ready
        dependency_id: String,
    },

    /// Duplicate resource ID detected in stack configuration.
    #[error(
        code = "DUPLICATE_RESOURCE_ID",
        message = "Duplicate resource ID '{resource_id}' found in stack configuration",
        retryable = "false",
        internal = "false"
    )]
    DuplicateResourceId {
        /// The duplicate resource ID
        resource_id: String,
    },

    /// Controller received unexpected resource type.
    #[error(
        code = "CONTROLLER_RESOURCE_TYPE_MISMATCH",
        message = "Controller expected resource type '{expected}' but received '{actual}' for resource '{resource_id}'",
        retryable = "false",
        internal = "true"
    )]
    ControllerResourceTypeMismatch {
        /// The expected resource type
        expected: ResourceType,
        /// The actual resource type received
        actual: ResourceType,
        /// ID of the resource with the type mismatch
        resource_id: String,
    },

    /// Controller received unexpected internal state type.
    #[error(
        code = "CONTROLLER_STATE_TYPE_MISMATCH",
        message = "Controller expected state type '{expected}' but received different type for resource '{resource_id}'",
        retryable = "false",
        internal = "true"
    )]
    ControllerStateTypeMismatch {
        /// The expected state type name
        expected: String,
        /// ID of the resource with the state type mismatch
        resource_id: String,
    },

    /// No controller available for the specified resource type and platform combination.
    #[error(
        code = "CONTROLLER_NOT_AVAILABLE",
        message = "No controller available for resource type '{resource_type}' on platform '{platform}'",
        retryable = "false",
        internal = "false"
    )]
    ControllerNotAvailable {
        /// The resource type that needs a controller
        resource_type: ResourceType,
        /// The platform where the controller is needed
        platform: Platform,
    },

    /// Stack execution reached maximum steps without completing.
    #[error(
        code = "EXECUTION_MAX_STEPS_REACHED",
        message = "Stack execution reached maximum steps ({max_steps}) without completion. Pending resources: {pending_resources:?}",
        retryable = "false",
        internal = "false"
    )]
    ExecutionMaxStepsReached {
        /// Maximum number of steps that were allowed
        max_steps: u64,
        /// List of resource IDs that were still pending
        pending_resources: Vec<String>,
    },

    /// Stack execution step failed unexpectedly.
    #[error(
        code = "EXECUTION_STEP_FAILED",
        message = "Stack execution step failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    ExecutionStepFailed {
        /// Human-readable description of the execution failure
        message: String,
        /// ID of the resource that caused the failure, if applicable
        resource_id: Option<String>,
    },

    /// A polling handler exhausted its Stay budget without the condition being met.
    #[error(
        code = "RESOURCE_POLLING_TIMEOUT",
        message = "Polling timed out after {max_times} attempts in state '{state}'",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    PollingTimeout {
        /// The controller state that was being polled when the timeout occurred
        state: String,
        /// The maximum number of Stay attempts that were allowed
        max_times: u32,
    },

    /// Failed to import existing infrastructure state.
    #[error(
        code = "INFRASTRUCTURE_IMPORT_FAILED",
        message = "Failed to import infrastructure state: {message}",
        retryable = "false",
        internal = "false"
    )]
    InfrastructureImportFailed {
        /// Human-readable description of the import failure
        message: String,
        /// The source/platform being imported from
        import_source: Option<String>,
        /// ID of the resource that failed to import, if applicable
        resource_id: Option<String>,
    },

    /// CloudFormation stack has resources in non-successful states.
    #[error(
        code = "CLOUDFORMATION_STACK_UNHEALTHY",
        message = "CloudFormation stack '{stack_name}' has resources in non-successful states: {failed_resources:?}",
        retryable = "true",
        internal = "false"
    )]
    CloudFormationStackUnhealthy {
        /// Name of the CloudFormation stack
        stack_name: String,
        /// List of failed resources with their status
        failed_resources: Vec<String>,
    },

    /// Expected CloudFormation resource not found during import.
    #[error(
        code = "CLOUDFORMATION_RESOURCE_MISSING",
        message = "Expected CloudFormation resource '{logical_id}' not found in stack '{stack_name}'",
        retryable = "false",
        internal = "false"
    )]
    CloudFormationResourceMissing {
        /// CloudFormation logical ID that was expected
        logical_id: String,
        /// Name of the CloudFormation stack
        stack_name: String,
        /// Alien resource ID that was being imported
        resource_id: Option<String>,
    },

    /// Platform configuration is missing or invalid.
    #[error(
        code = "client_config_INVALID",
        message = "Platform configuration invalid for '{platform}': {message}",
        retryable = "false",
        internal = "false"
    )]
    ClientConfigInvalid {
        /// The platform that has invalid configuration
        platform: Platform,
        /// Human-readable description of the configuration issue
        message: String,
    },

    /// Platform configuration is invalid (legacy variant for backwards compatibility).
    #[error(
        code = "INVALID_client_config",
        message = "Platform configuration invalid: {message}",
        retryable = "false",
        internal = "false"
    )]
    InvalidClientConfig {
        /// Human-readable description of the configuration issue
        message: String,
        /// Optional nested errors
        errors: Option<Vec<String>>,
    },

    /// Input validation failed.
    #[error(
        code = "INVALID_INPUT",
        message = "Invalid input: {message}",
        retryable = "false",
        internal = "false"
    )]
    InvalidInput {
        /// Human-readable description of the validation failure
        message: String,
        /// The field that was invalid
        field_name: Option<String>,
    },

    /// Deployment target configuration is invalid.
    #[error(
        code = "DEPLOYMENT_TARGET_INVALID",
        message = "Deployment target configuration invalid: {message}",
        retryable = "false",
        internal = "false"
    )]
    DeploymentTargetInvalid {
        /// Human-readable description of the validation failure
        message: String,
        /// The deployment target field that was invalid
        field_name: Option<String>,
    },

    /// Remote access configuration is invalid.
    #[error(
        code = "REMOTE_ACCESS_INVALID",
        message = "Remote access configuration invalid: {message}",
        retryable = "false",
        internal = "false"
    )]
    RemoteAccessInvalid {
        /// Human-readable description of the validation failure
        message: String,
        /// The remote access field that was invalid
        field_name: Option<String>,
    },

    /// Authentication or impersonation failed.
    #[error(
        code = "AUTHENTICATION_FAILED",
        message = "Authentication failed: {message}",
        retryable = "false",
        internal = "false"
    )]
    AuthenticationFailed {
        /// Human-readable description of the authentication failure
        message: String,
        /// The authentication method that failed
        method: Option<String>,
    },

    /// Wrong platform configuration provided for the operation.
    #[error(
        code = "client_config_MISMATCH",
        message = "Operation requires '{required_platform:?}' platform configuration, but '{found_platform:?}' configuration was provided",
        retryable = "false",
        internal = "false"
    )]
    ClientConfigMismatch {
        /// The platform configuration that was required
        required_platform: Platform,
        /// The platform configuration that was actually found
        found_platform: Platform,
    },

    /// Resource controller configuration error.
    #[error(
        code = "RESOURCE_CONTROLLER_CONFIG_ERROR",
        message = "Resource controller configuration error for '{resource_id}': {message}",
        retryable = "false",
        internal = "false"
    )]
    ResourceControllerConfigError {
        /// ID of the resource with configuration error
        resource_id: String,
        /// Human-readable description of the configuration error
        message: String,
    },

    /// Resource readiness probe failed.
    #[error(
        code = "READINESS_PROBE_FAILED",
        message = "Readiness probe failed for resource '{resource_id}': {reason}",
        retryable = "true",
        internal = "false"
    )]
    ReadinessProbeFailure {
        /// ID of the resource whose readiness probe failed
        resource_id: String,
        /// Reason for the probe failure
        reason: String,
        /// The probe URL that was tested
        probe_url: String,
    },

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

    /// Resource configuration has drifted from expected state.
    #[error(
        code = "RESOURCE_DRIFT",
        message = "Resource '{resource_id}' has drifted from expected configuration: {message}",
        retryable = "false",
        internal = "false"
    )]
    ResourceDrift {
        /// ID of the resource that has drifted
        resource_id: String,
        /// Human-readable description of the drift
        message: String,
    },

    /// Errors originating from cloud platform operations.
    #[error(
        code = "CLOUD_PLATFORM_ERROR",
        message = "Cloud platform operation failed: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    CloudPlatformError {
        /// Human-readable description of the platform error
        message: String,
        /// The resource ID affected by the error, if applicable
        resource_id: Option<String>,
    },

    /// Local platform service not available.
    #[error(
        code = "LOCAL_SERVICES_NOT_AVAILABLE",
        message = "Local platform service '{service_name}' is not available",
        retryable = "false",
        internal = "false"
    )]
    LocalServicesNotAvailable {
        /// Name of the service that is not available
        service_name: String,
    },

    /// Generic catch-all error for uncommon cases.
    #[error(
        code = "INFRASTRUCTURE_ERROR",
        message = "Infrastructure operation failed: {message}",
        retryable = "true",
        internal = "true"
    )]
    InfrastructureError {
        /// Human-readable description of the error
        message: String,
        /// Context about what operation was being performed
        operation: Option<String>,
        /// The resource ID affected by the error, if applicable
        resource_id: Option<String>,
    },

    /// Horizon API call failed.
    #[error(
        code = "HORIZON_API_ERROR",
        message = "Horizon API call failed: {message}",
        retryable = "true",
        internal = "false"
    )]
    HorizonApiError {
        /// Human-readable description of the error
        message: String,
        /// The cluster ID involved, if applicable
        cluster_id: Option<String>,
    },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
