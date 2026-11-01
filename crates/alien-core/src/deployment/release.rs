//! Release metadata for deployment version tracking.

use crate::Stack;
use serde::{Deserialize, Serialize};

/// A signed release statement as it is stored and forwarded: the exact bytes
/// that were signed and the signature over them. Carriers pass both through
/// unchanged; only a verifier reads them (see `release_signature`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedRelease {
    /// Base64 of the statement's JSON bytes.
    pub payload: String,
    /// Base64 DER ECDSA P-256 signature over SHA-256 of the payload bytes.
    pub signature: String,
}

/// Release metadata
///
/// Identifies a specific release version and includes the stack definition.
/// The deployment engine uses this to track which release is currently deployed
/// and which is the target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    /// Release ID (e.g., rel_xyz). `None` for an observe deployment, which has no
    /// Alien-assigned release — the platform resolves a release from `version`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
    /// Version string (e.g., 2.1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Short description of the release
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Stack definition for this release
    pub stack: Stack,
    /// The developer's signed statement for this platform, when the release
    /// was signed. A deployment in protected mode deploys only the stack inside
    /// it and ignores `stack`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed: Option<SignedRelease>,
}
