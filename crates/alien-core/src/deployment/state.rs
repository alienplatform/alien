//! Deployment state, step results, and runtime metadata.

use crate::{Platform, StackState};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};

use super::{DeploymentStatus, EnvironmentInfo, ReleaseInfo};

/// Runtime metadata for deployment
///
/// Stores deployment state that needs to persist across step calls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetadata {
    /// Hash of the environment variables snapshot that was last synced to the vault
    /// Used to avoid redundant sync operations during incremental deployment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_synced_env_vars_hash: Option<String>,

    /// The prepared (mutated) stack from the last successful deployment phase
    /// This is the stack AFTER mutations have been applied (with service accounts, vault, etc.)
    /// Used for compatibility checks during updates to compare mutated stacks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepared_stack: Option<crate::Stack>,
}

/// Deployment state
///
/// Represents the current state of deployed infrastructure, including release tracking.
/// This is platform-agnostic - no backend IDs or database relationships.
///
/// The deployment engine manages releases internally: when a deployment succeeds,
/// it promotes `target_release` to `current_release` and clears `target_release`.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentState {
    /// Current lifecycle phase
    pub status: DeploymentStatus,
    /// Target cloud platform (AWS, GCP, Azure, Kubernetes)
    pub platform: Platform,
    /// Currently deployed release (None for first deployment)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_release: Option<ReleaseInfo>,
    /// Target release to deploy (None when synced with current)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_release: Option<ReleaseInfo>,
    /// Infrastructure resource tracking (which resources exist, their status, outputs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_state: Option<StackState>,
    /// Cloud account details (account ID, project number, region)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_info: Option<EnvironmentInfo>,
    /// Deployment-specific data (prepared stacks, phase tracking, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_metadata: Option<RuntimeMetadata>,
    /// Whether a retry has been requested for a failed deployment
    /// When true and status is a failed state, the deployment system will retry failed resources
    #[serde(default, skip_serializing_if = "is_false")]
    pub retry_requested: bool,
}

/// Result of a deployment step
///
/// Contains the complete next deployment state along with hints for the platform.
/// This replaces the old delta-based `DeploymentStateUpdate` approach.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentStepResult {
    /// The complete next deployment state
    pub state: DeploymentState,

    /// Error that occurred during this step (if any)
    /// - `None`: No error, step succeeded
    /// - `Some(error)`: Step failed or encountered an error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AlienError>,

    /// Suggested delay before next step (optimization hint)
    /// - `None`: No suggested delay, can poll immediately
    /// - `Some(ms)`: Wait this many milliseconds before next step
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_delay_ms: Option<u64>,

    /// Whether to update heartbeat timestamp (monitoring signal)
    /// - `false`: Don't update heartbeat (default for most steps)
    /// - `true`: Update lastHeartbeatAt (for successful health checks in Running state)
    #[serde(default, skip_serializing_if = "is_false")]
    pub update_heartbeat: bool,
}

pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}
