use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use alien_core::{Platform, Stack};
use alien_error::AlienError;

/// A release record. Contains stacks for one or more platforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseRecord {
    pub id: String,
    /// Workspace this release belongs to. Always `"default"` in OSS.
    #[serde(default = "crate::traits::default_string")]
    pub workspace_id: String,
    /// Project this release belongs to. Always `"default"` in OSS.
    #[serde(default = "crate::traits::default_string")]
    pub project_id: String,
    /// Per-platform stacks. A release can target multiple platforms
    /// (e.g., aws + gcp + azure from a single `alien release` invocation).
    pub stacks: HashMap<Platform, Stack>,
    pub git_commit_sha: Option<String>,
    pub git_commit_ref: Option<String>,
    pub git_commit_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Parameters for creating a release. Data-to-persist only — caller identity
/// (workspace, bearer for passthrough) flows through the separate
/// `caller: &Subject` argument on the trait method.
#[derive(Debug, Clone)]
pub struct CreateReleaseParams {
    /// Project this release belongs to. Always `"default"` in the standalone
    /// binary.
    pub project_id: String,
    /// Per-platform stacks.
    pub stacks: HashMap<Platform, Stack>,
    pub git_commit_sha: Option<String>,
    pub git_commit_ref: Option<String>,
    pub git_commit_message: Option<String>,
}

/// Persistence for releases. `caller` is metadata-about-who; `params` is
/// data-to-persist — never conflate the two on a single struct.
#[async_trait]
pub trait ReleaseStore: Send + Sync {
    async fn create_release(
        &self,
        caller: &crate::auth::Subject,
        params: CreateReleaseParams,
    ) -> Result<ReleaseRecord, AlienError>;

    async fn get_release(
        &self,
        caller: &crate::auth::Subject,
        id: &str,
    ) -> Result<Option<ReleaseRecord>, AlienError>;

    async fn get_latest_release(
        &self,
        caller: &crate::auth::Subject,
    ) -> Result<Option<ReleaseRecord>, AlienError>;
}
