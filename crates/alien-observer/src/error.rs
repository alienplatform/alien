use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

pub type Result<T> = alien_client_core::Result<T>;

/// Errors from pulling provider metrics.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MetricsErrorData {
    /// The deployment's identity may not read the provider's metrics.
    #[error(
        code = "METRICS_SETUP_UPDATE_REQUIRED",
        message = "Reading {metrics_source} metrics needs a setup update: the deployment identity lacks '{missing_action}' (permission set '{permission_set}')",
        retryable = "false",
        internal = "false",
        http_status_code = 403
    )]
    SetupUpdateRequired {
        /// The metrics source that was denied, for example `CloudWatch`
        metrics_source: String,
        /// The provider action that setup has to grant
        missing_action: String,
        /// The permission set that carries the action
        permission_set: String,
    },

    /// The provider call failed.
    #[error(
        code = "METRICS_PULL_FAILED",
        message = "Pulling {metrics_source} metrics failed: {message}",
        retryable = "inherit",
        internal = "inherit"
    )]
    PullFailed {
        /// The metrics source that was queried
        metrics_source: String,
        /// What the pull was doing when it failed
        message: String,
    },

    /// The provider answered, but the answer cannot be mapped faithfully.
    #[error(
        code = "METRICS_RESPONSE_INVALID",
        message = "{metrics_source} returned a metrics response that cannot be used: {message}",
        retryable = "true",
        internal = "true"
    )]
    ResponseInvalid {
        /// The metrics source that answered
        metrics_source: String,
        /// What is wrong with the response
        message: String,
    },
}

pub type MetricsResult<T> = alien_error::Result<T, MetricsErrorData>;
