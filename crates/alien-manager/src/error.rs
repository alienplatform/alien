//! Error types for alien-manager.

use alien_core::Platform;
use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

pub(crate) const REMOTE_CREDENTIAL_HANDOFF_FAILED_CODE: &str = "REMOTE_CREDENTIAL_HANDOFF_FAILED";

/// Errors for alien-manager operations.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// Deployment not found
    #[error(
        code = "DEPLOYMENT_NOT_FOUND",
        message = "Deployment '{deployment_id}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    DeploymentNotFound { deployment_id: String },

    /// Deployment is locked by another session
    #[error(
        code = "DEPLOYMENT_LOCKED",
        message = "Deployment '{deployment_id}' is locked",
        retryable = "true",
        internal = "false",
        http_status_code = 409
    )]
    DeploymentLocked {
        deployment_id: String,
        locked_by: Option<String>,
    },

    /// Deployment is already in a deletion state
    #[error(
        code = "DEPLOYMENT_ALREADY_DELETING",
        message = "Deployment '{deployment_id}' is already in status '{status}'",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    DeploymentAlreadyDeleting {
        deployment_id: String,
        status: String,
    },

    /// Deployment group not found
    #[error(
        code = "DEPLOYMENT_GROUP_NOT_FOUND",
        message = "Deployment group '{deployment_group_id}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    DeploymentGroupNotFound { deployment_group_id: String },

    /// Deployment name already exists in deployment group
    #[error(
        code = "DEPLOYMENT_NAME_CONFLICT",
        message = "Deployment name '{name}' already exists in deployment group '{deployment_group_id}'",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    DeploymentNameConflict {
        name: String,
        deployment_group_id: String,
    },

    /// Imported deployment setup fingerprint mismatch or non-idempotent re-import
    #[error(
        code = "IMPORTED_DEPLOYMENT_CONFLICT",
        message = "{reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 409
    )]
    ImportedDeploymentConflict { reason: String },

    /// Deployment group has reached max deployments
    #[error(
        code = "MAX_DEPLOYMENTS_REACHED",
        message = "Deployment group '{deployment_group_id}' has reached maximum deployments ({max_deployments})",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    MaxDeploymentsReached {
        deployment_group_id: String,
        max_deployments: i64,
    },

    /// Release not found
    #[error(
        code = "RELEASE_NOT_FOUND",
        message = "Release '{release_id}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    ReleaseNotFound { release_id: String },

    /// The target-side management identity could not yet be impersonated.
    #[error(
        code = "REMOTE_CREDENTIAL_HANDOFF_FAILED",
        message = "Remote credential handoff failed for deployment '{deployment_id}' on '{platform}'",
        retryable = "true",
        internal = "inherit",
        human = "transparent"
    )]
    RemoteCredentialHandoffFailed {
        deployment_id: String,
        platform: Platform,
    },

    /// A provider failed while turning a refreshable identity into a bearer token.
    #[error(
        code = "CREDENTIAL_MATERIALIZATION_FAILED",
        message = "Failed to materialize {platform} credentials for {purpose}",
        retryable = "inherit",
        internal = "inherit",
        http_status_code = "inherit",
        human = "transparent"
    )]
    CredentialMaterializationFailed { platform: Platform, purpose: String },

    /// Registry permissions could not be removed during deployment cleanup.
    #[error(
        code = "REGISTRY_ACCESS_CLEANUP_FAILED",
        message = "Registry access cleanup failed for deployment '{deployment_id}': {reason}",
        retryable = "true",
        internal = "true"
    )]
    RegistryAccessCleanupFailed {
        deployment_id: String,
        reason: String,
    },

    /// Command not found
    #[error(
        code = "COMMAND_NOT_FOUND",
        message = "Command '{command_id}' not found",
        retryable = "false",
        internal = "false",
        http_status_code = 404
    )]
    CommandNotFound { command_id: String },

    /// Unauthorized - authentication required
    #[error(
        code = "UNAUTHORIZED",
        message = "{reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 401
    )]
    Unauthorized { reason: String },

    /// Forbidden - insufficient permissions
    #[error(
        code = "FORBIDDEN",
        message = "{reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    Forbidden { reason: String },

    /// Invalid request payload
    #[error(
        code = "BAD_REQUEST",
        message = "{reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    BadRequest { reason: String },

    /// Setup import payload version is outside this manager's supported range.
    #[error(
        code = "INCOMPATIBLE_SETUP_IMPORT",
        message = "Setup import format version {found_version} is not supported; this manager supports {min_supported_version} through {current_version}. {repair}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    IncompatibleSetupImport {
        found_version: u32,
        min_supported_version: u32,
        current_version: u32,
        repair: String,
    },

    /// Database operation failed
    #[error(
        code = "DATABASE_ERROR",
        message = "Database operation failed: {message}",
        retryable = "true",
        internal = "true"
    )]
    DatabaseError { message: String },

    /// Server initialization failed
    #[error(
        code = "SERVER_INIT_FAILED",
        message = "Failed to initialize server: {reason}",
        retryable = "false",
        internal = "true"
    )]
    ServerInitFailed { reason: String },

    /// Internal server error
    #[error(
        code = "INTERNAL_ERROR",
        message = "{message}",
        retryable = "false",
        internal = "true"
    )]
    InternalError { message: String },
}

/// Convenience constructors for common errors.
impl ErrorData {
    pub fn unauthorized(reason: impl Into<String>) -> alien_error::AlienError<ErrorData> {
        alien_error::AlienError::new(ErrorData::Unauthorized {
            reason: reason.into(),
        })
    }

    pub fn forbidden(reason: impl Into<String>) -> alien_error::AlienError<ErrorData> {
        alien_error::AlienError::new(ErrorData::Forbidden {
            reason: reason.into(),
        })
    }

    pub fn bad_request(reason: impl Into<String>) -> alien_error::AlienError<ErrorData> {
        alien_error::AlienError::new(ErrorData::BadRequest {
            reason: reason.into(),
        })
    }

    pub fn not_found_deployment(id: impl Into<String>) -> alien_error::AlienError<ErrorData> {
        alien_error::AlienError::new(ErrorData::DeploymentNotFound {
            deployment_id: id.into(),
        })
    }

    pub fn not_found_group(id: impl Into<String>) -> alien_error::AlienError<ErrorData> {
        alien_error::AlienError::new(ErrorData::DeploymentGroupNotFound {
            deployment_group_id: id.into(),
        })
    }

    pub fn not_found_release(id: impl Into<String>) -> alien_error::AlienError<ErrorData> {
        alien_error::AlienError::new(ErrorData::ReleaseNotFound {
            release_id: id.into(),
        })
    }

    pub fn internal(message: impl Into<String>) -> alien_error::AlienError<ErrorData> {
        alien_error::AlienError::new(ErrorData::InternalError {
            message: message.into(),
        })
    }
}

/// Convenient type alias.
pub type Result<T> = alien_error::Result<T, ErrorData>;
