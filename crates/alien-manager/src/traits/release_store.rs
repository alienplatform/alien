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
    /// Per-platform stacks. A release can target multiple platforms
    /// (e.g., aws + gcp + azure from a single `alien release` invocation).
    pub stacks: HashMap<Platform, Stack>,
    pub git_commit_sha: Option<String>,
    pub git_commit_ref: Option<String>,
    pub git_commit_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Parameters for creating a release.
#[derive(Debug, Clone)]
pub struct CreateReleaseParams {
    pub project: Option<String>,
    /// Bearer token from the original caller (for token passthrough to Platform API).
    pub caller_token: Option<String>,
    /// Per-platform stacks.
    pub stacks: HashMap<Platform, Stack>,
    pub git_commit_sha: Option<String>,
    pub git_commit_ref: Option<String>,
    pub git_commit_message: Option<String>,
}

/// Persistence for releases.
#[async_trait]
pub trait ReleaseStore: Send + Sync {
    async fn create_release(
        &self,
        params: CreateReleaseParams,
    ) -> Result<ReleaseRecord, AlienError>;

    async fn get_release(&self, id: &str) -> Result<Option<ReleaseRecord>, AlienError>;

    async fn get_latest_release(&self) -> Result<Option<ReleaseRecord>, AlienError>;
}
