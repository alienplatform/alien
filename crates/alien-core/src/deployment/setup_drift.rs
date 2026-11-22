//! Setup drift: what a setup run would still change on an installed deployment.
//!
//! Drift is **recommended**, never required: it never pauses a release. It is a notice that the
//! installer can run a setup update to bring grants, alarms and flow logs up to date.

use serde::{Deserialize, Serialize};

/// Why an installed deployment's setup is behind. Codes are stable: clients render and match
/// them, so a new reason gets a new code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SetupDriftCode {
    /// A permission set the management identity holds has a body this version would change,
    /// compared by content, not by name. `resource` is the permission set id when known.
    ManagementPermissionsChanged,
    /// No record of the management permissions that were installed (an older install): setup
    /// runs once to record them.
    ManagementPermissionsUnrecorded,
    /// The Alien-created VPC has no complete flow log. `resource` is the network id.
    FlowLogMissing,
    /// The management identity has no recorded permissions boundary (an install from before
    /// the boundary existed). Setup installs it. `resource` is the management resource id.
    PermissionsBoundaryMissing,
    /// A grant was denied while creating something setup's grant covers (CloudWatch alarms,
    /// point-in-time recovery). `resource` is the resource id; `detail` names what was denied.
    GrantDenied,
}

/// One reason an installed deployment's setup is behind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SetupDriftReason {
    /// What kind of reason this is.
    pub code: SetupDriftCode,
    /// The resource or permission set the reason is about, when one is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// Plain-language detail for a person.
    pub detail: String,
}

impl SetupDriftReason {
    /// One line for a terminal.
    pub fn summary(&self) -> String {
        match &self.resource {
            Some(resource) => format!("{} ({resource})", self.detail),
            None => self.detail.clone(),
        }
    }
}
