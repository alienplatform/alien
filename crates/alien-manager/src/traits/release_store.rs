use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use alien_core::Stack;
use alien_error::AlienError;

/// A release record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseRecord {
    pub id: String,
    pub stack: Stack,
    pub git_commit_sha: Option<String>,
    pub git_commit_ref: Option<String>,
    pub git_commit_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Parameters for creating a release.
#[derive(Debug, Clone)]
pub struct CreateReleaseParams {
    pub stack: Stack,
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
