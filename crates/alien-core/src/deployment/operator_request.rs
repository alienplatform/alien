//! Operator requests: one-off actions an operator asks a deployment's controllers to run.
//!
//! A request is recorded on the deployment and executed by the controller that owns the
//! resource, even while the deployment is in a failed status. It never plans, never changes
//! the deployment status and is cleared when the controller reports it done or refused.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What an operator request asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum OperatorRequestKind {
    /// Replace one machine of a cloud compute cluster: drain it, terminate it through its
    /// scaling group without lowering the group's desired capacity, then admit its replacement.
    #[serde(rename_all = "camelCase")]
    ReplaceMachine {
        /// The compute cluster resource that owns the machine.
        resource_id: String,
        /// The machine (instance) to replace.
        machine_id: String,
    },
}

/// A request recorded on a deployment, at most one at a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OperatorRequest {
    /// Identifies this request in events and in the resource's state.
    pub id: String,
    /// What is requested.
    pub kind: OperatorRequestKind,
    /// Who asked, for the audit trail.
    pub requested_by: String,
    /// When it was recorded.
    pub requested_at: DateTime<Utc>,
    /// How it ended. Set by the runner when the controller reports it done or refused; the
    /// request is then no longer stepped and the store drops it when the state is reconciled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<OperatorRequestOutcome>,
}

/// Whether a request ended done or refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum OperatorRequestEnd {
    /// The controller carried the request out.
    Done,
    /// The controller refused it; nothing was changed.
    Refused,
}

/// How an operator request ended, with the controller's own words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OperatorRequestOutcome {
    pub status: OperatorRequestEnd,
    /// What was done, or why it was refused.
    pub message: String,
}

impl OperatorRequest {
    /// The resource whose controller executes this request.
    pub fn resource_id(&self) -> &str {
        match &self.kind {
            OperatorRequestKind::ReplaceMachine { resource_id, .. } => resource_id,
        }
    }

    /// Short name for events and messages.
    pub fn kind_name(&self) -> &'static str {
        match &self.kind {
            OperatorRequestKind::ReplaceMachine { .. } => "replace-machine",
        }
    }
}

/// What a controller reports after one step of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatorRequestProgress {
    /// Not finished: step the request again after the delay.
    InProgress {
        /// What it waits for, for the deployment event.
        waiting_for: String,
        /// Delay before the next step.
        delay: std::time::Duration,
    },
    /// Finished. The request is cleared.
    Done {
        /// Result for the deployment event.
        summary: String,
    },
    /// Not carried out. The request is cleared and the reason is reported.
    Refused {
        /// Why, in words an operator can act on.
        reason: String,
    },
}
