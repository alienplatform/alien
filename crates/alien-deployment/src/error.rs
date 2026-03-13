use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors related to agent deployment operations.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Environment information collection failed.
    #[error(
        code = "ENVIRONMENT_INFO_COLLECTION_FAILED",
        message = "Failed to collect environment information for platform '{platform}': {reason}",
        retryable = "inherit",
        internal = "inherit"
    )]
    EnvironmentInfoCollectionFailed {
        /// The platform where environment collection failed
        platform: String,
        /// Reason for the failure
        reason: String,
    },

    /// Preflight checks failed.
    #[error(
        code = "PREFLIGHT_CHECKS_FAILED",
        message = "Preflight checks failed",
        retryable = "false",
        internal = "false"
    )]
    PreflightChecksFailed,

    /// Stack mutation failed.
    #[error(
        code = "STACK_MUTATION_FAILED",
        message = "Failed to apply stack mutations: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    StackMutationFailed {
        /// Human-readable description of the failure
        message: String,
    },

    /// Stack execution step failed.
    #[error(
        code = "STACK_EXECUTION_FAILED",
        message = "Stack execution step failed: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    StackExecutionFailed {
        /// Human-readable description of the failure
        message: String,
    },

    /// Cross-account access setup failed.
    #[error(
        code = "CROSS_ACCOUNT_ACCESS_FAILED",
        message = "Failed to setup cross-account access for platform '{platform}': {reason}",
        retryable = "inherit",
        internal = "inherit"
    )]
    CrossAccountAccessFailed {
        /// The platform where access setup failed
        platform: String,
        /// Reason for the failure
        reason: String,
    },

    /// Invalid agent status for the requested operation.
    #[error(
        code = "INVALID_AGENT_STATUS",
        message = "Agent status '{current_status}' is not valid for operation '{operation}'",
        retryable = "false",
        internal = "false"
    )]
    InvalidAgentStatus {
        /// Current agent status
        current_status: String,
        /// The operation that was attempted
        operation: String,
    },

    /// Required configuration is missing.
    #[error(
        code = "MISSING_CONFIGURATION",
        message = "Missing required configuration: {message}",
        retryable = "false",
        internal = "false"
    )]
    MissingConfiguration {
        /// Description of the missing configuration
        message: String,
    },

    /// Generic deployment error.
    #[error(
        code = "DEPLOYMENT_ERROR",
        message = "Deployment operation failed: {message}",
        retryable = "true",
        internal = "true"
    )]
    DeploymentError {
        /// Human-readable description of the error
        message: String,
    },

    /// Secret sync to vault failed.
    #[error(
        code = "SECRET_SYNC_FAILED",
        message = "Failed to sync secrets to vault '{vault_name}': {reason}",
        retryable = "inherit",
        internal = "inherit"
    )]
    SecretSyncFailed {
        /// Name of the vault
        vault_name: String,
        /// Reason for the failure
        reason: String,
    },

    /// Internal error (unexpected condition).
    #[error(
        code = "INTERNAL_ERROR",
        message = "Internal error: {message}",
        retryable = "false",
        internal = "true"
    )]
    InternalError {
        /// Human-readable description of the error
        message: String,
    },

    /// Resource was not deployed because another resource in the same deployment failed.
    /// The resource's controller state is preserved in `last_failed_state` for retry.
    #[error(
        code = "DEPLOYMENT_INTERRUPTED",
        message = "Resource was not deployed because resource '{failed_resource_id}' failed",
        retryable = "true",
        internal = "false"
    )]
    DeploymentInterrupted {
        /// ID of the resource whose failure caused this resource to be interrupted
        failed_resource_id: String,
        /// Type of the resource that failed
        failed_resource_type: String,
    },

    /// Agent deployment failed with one or more resource errors.
    #[error(
        code = "AGENT_DEPLOYMENT_FAILED",
        message = "Deployment failed: {failed_resources} resource error(s), {interrupted_resources} interrupted, {total_resources} total",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    AgentDeploymentFailed {
        /// Resources that actually failed (excludes interrupted resources).
        resource_errors: Vec<ResourceError>,
        /// Total number of resources in the deployment.
        total_resources: usize,
        /// Number of resources with real failures (excludes interrupted).
        failed_resources: usize,
        /// Number of resources that were stopped because a sibling failed.
        interrupted_resources: usize,
    },
}

/// Information about a failed resource
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceError {
    /// ID of the resource that failed
    pub resource_id: String,
    /// Type of the resource (e.g., "function", "storage")
    pub resource_type: String,
    /// The error that occurred (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<alien_error::AlienError<alien_error::GenericError>>,
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
