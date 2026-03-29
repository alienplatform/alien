//! Release metadata for deployment version tracking.

use crate::Stack;
use serde::{Deserialize, Serialize};

/// Release metadata
///
/// Identifies a specific release version and includes the stack definition.
/// The deployment engine uses this to track which release is currently deployed
/// and which is the target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    /// Release ID (e.g., rel_xyz)
    pub release_id: String,
    /// Version string (e.g., 2.1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Short description of the release
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Stack definition for this release
    pub stack: Stack,
}
