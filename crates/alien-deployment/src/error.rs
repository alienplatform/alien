use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors related to deployment operations.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Deployment protocol version is outside this binary's supported range.
    #[error(
        code = "INCOMPATIBLE_DEPLOYMENT_PROTOCOL",
        message = "Deployment protocol version {found_version} is not supported; this binary supports {min_supported_version} through {current_version}. {repair}",
        retryable = "false",
        internal = "false"
    )]
    IncompatibleDeploymentProtocol {
        found_version: u32,
        min_supported_version: u32,
        current_version: u32,
        repair: String,
    },

    /// An update supplied a value for an input whose frozen-gate answer is
    /// already fixed for the deployment's lifetime.
    #[error(
        code = "FROZEN_GATE_ANSWER_CHANGED",
        message = "Input '{input_id}' gates a setup-created resource and its answer was fixed at \
                   {persisted} when this deployment was created; the update supplies {requested}. \
                   A frozen gate cannot be re-answered — create a new deployment for a different \
                   answer",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    FrozenGateAnswerChanged {
        /// The input whose answer the update tried to change
        input_id: String,
        /// The answer recorded when the deployment was created
        persisted: bool,
        /// The conflicting answer the update supplied
        requested: bool,
    },

    /// A frozen gate's answer could not be resolved from its sources — the
    /// import's state presence, the provided input values, or the declared
    /// default — wherever answers are recorded: deployment creation and
    /// setup import.
    #[error(
        code = "FROZEN_GATE_ANSWER_UNDERIVABLE",
        message = "Cannot derive the recorded answer for input '{input_id}': {reason}. Refusing \
                   the update rather than guessing a frozen resource's existence",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    FrozenGateAnswerUnderivable {
        /// The input whose answer could not be derived
        input_id: String,
        /// Why derivation failed
        reason: String,
    },

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

    /// Invalid deployment status for the requested operation.
    #[error(
        code = "INVALID_DEPLOYMENT_STATUS",
        message = "Deployment status '{current_status}' is not valid for operation '{operation}'",
        retryable = "false",
        internal = "false"
    )]
    InvalidDeploymentStatus {
        /// Current deployment status
        current_status: String,
        /// The operation that was attempted
        operation: String,
    },

    /// The manager rejected an explicit deployment acquisition request.
    #[error(
        code = "DEPLOYMENT_ACQUIRE_UNAVAILABLE",
        message = "Deployment '{deployment_id}' cannot be acquired for this operation: {reason}",
        retryable = "false",
        internal = "false"
    )]
    DeploymentAcquireUnavailable {
        /// Deployment requested by the caller.
        deployment_id: String,
        /// Bounded reason returned by the manager.
        reason: String,
    },

    /// A setup run found no operation to apply because the deployment's last operation failed
    /// and is not waiting for setup.
    #[error(
        code = "SETUP_RUN_AFTER_FAILED_OPERATION",
        message = "Setup has nothing to apply to deployment '{deployment_id}': its last operation failed ({status}) and is not waiting for setup. Retry the deployment first (`alien deployments retry {deployment_id}`, or Retry in the dashboard). If the retry fails with an error that asks for setup, rerun setup then",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    SetupRunAfterFailedOperation {
        /// Deployment the setup run targeted.
        deployment_id: String,
        /// Its failed status.
        status: String,
    },

    /// Required deployer secrets are not in the customer's secret store yet;
    /// workloads wait for them.
    #[error(
        code = "DEPLOYER_SECRETS_MISSING",
        message = "Waiting for deployer secrets: {summary}",
        retryable = "true",
        internal = "false"
    )]
    DeployerSecretsMissing {
        /// One `missing: <label>` / `invalid: <label> (...)` entry per slot
        summary: String,
    },

    /// A retry cannot resume these failed resources from where they stopped.
    #[error(
        code = "RETRY_CANNOT_RESUME",
        message = "Retry cannot resume {resources}",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    RetryCannotResume {
        /// Each resource with what it needs instead
        resources: String,
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

    /// The deployment lease could not be renewed or was lost.
    #[error(
        code = "DEPLOYMENT_LEASE_LOST",
        message = "Deployment lease lost: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    DeploymentLeaseLost { message: String },

    /// A deployment state checkpoint could not be persisted.
    #[error(
        code = "DEPLOYMENT_CHECKPOINT_FAILED",
        message = "Deployment checkpoint failed: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    DeploymentCheckpointFailed { message: String },

    /// A request to the manager API failed. Keeps the source's retryable flag and status: a
    /// network error or a manager that says to retry is retried by the caller, a rejection is not.
    #[error(
        code = "MANAGER_REQUEST_FAILED",
        message = "Manager request failed: {message}",
        retryable = "inherit",
        internal = "inherit",
        http_status_code = "inherit"
    )]
    ManagerRequestFailed { message: String },

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

    /// Deployment failed with one or more resource errors.
    #[error(
        code = "DEPLOYMENT_FAILED",
        message = "Deployment failed: {failed_resources} resource error(s), {interrupted_resources} interrupted, {total_resources} total",
        retryable = "false",
        internal = "false",
        http_status_code = 500
    )]
    DeploymentFailed {
        /// Resources that actually failed (excludes interrupted resources).
        resource_errors: Vec<ResourceError>,
        /// Total number of resources in the deployment.
        total_resources: usize,
        /// Number of resources with real failures (excludes interrupted).
        failed_resources: usize,
        /// Number of resources that were stopped because a sibling failed.
        interrupted_resources: usize,
    },

    /// Every resource that failed stopped at a step that only rerunning the installation's
    /// setup unblocks, such as a grant the management role lacks.
    ///
    /// The platform waits for a setup run instead of retrying, and the deployment resumes the
    /// failed steps when setup hands it back. Updating a CloudFormation stack or Terraform
    /// configuration does not hand the deployment back, so those installs retry after it.
    #[error(
        code = "DEPLOYMENT_RESOURCE_SETUP_REQUIRED",
        message = "Waiting for the installation's setup to run again: {summary}. A setup CLI run continues the deployment by itself; after updating a CloudFormation stack or Terraform configuration instead, retry the deployment",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    ResourceSetupRequired {
        /// The resources waiting for setup.
        resource_ids: Vec<String>,
        /// Each waiting resource with its error.
        summary: String,
    },
}

/// Information about a failed resource
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceError {
    /// ID of the resource that failed
    pub resource_id: String,
    /// Type of the resource (e.g., "worker", "storage")
    pub resource_type: String,
    /// The error that occurred (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<alien_error::AlienError<alien_error::GenericError>>,
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
