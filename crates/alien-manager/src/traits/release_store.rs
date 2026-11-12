use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use alien_core::{Platform, Stack};
use alien_error::{AlienError, GenericError};

/// A release record. Contains stacks for one or more platforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// The bytes of one served Helm chart version, kept so the version never
/// changes content. Digests are the SHA-256 of these bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredChart {
    pub release_id: String,
    /// Chart version, `1.<release n>.<operator>`.
    pub version: String,
    /// OCI manifest JSON; names the config and archive by digest.
    pub manifest: Vec<u8>,
    pub config: Vec<u8>,
    /// Chart archive (`.tgz`).
    pub archive: Vec<u8>,
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

    /// Read only the release pinned by a verified setup intent. This is an
    /// internal import read, not permission to list/export release contents.
    async fn get_verified_setup_release(
        &self,
        authority: &crate::auth::verified_setup::VerifiedSetupImport,
    ) -> Result<Option<ReleaseRecord>, AlienError> {
        if !authority.is_current() {
            return Err(
                crate::error::ErrorData::forbidden("Verified setup authority expired")
                    .into_generic(),
            );
        }
        let release = self
            .get_release(&authority.subject(), &authority.release_id)
            .await?;
        Ok(release.filter(|release| {
            release.id == authority.release_id
                && release.workspace_id == authority.workspace_id
                && release.project_id == authority.project_id
        }))
    }

    async fn get_latest_release(
        &self,
        caller: &crate::auth::Subject,
    ) -> Result<Option<ReleaseRecord>, AlienError>;

    /// Id of the latest release. Stores override this to avoid loading the
    /// release's stacks.
    async fn latest_release_id(
        &self,
        caller: &crate::auth::Subject,
    ) -> Result<Option<String>, AlienError> {
        Ok(self
            .get_latest_release(caller)
            .await?
            .map(|release| release.id))
    }

    /// Resolve the release used when a project-scoped operation omits an
    /// explicit release. Standalone stores have one release stream, so the
    /// default is the latest release. Hosted implementations may override this
    /// without changing chronological release history.
    async fn get_default_release(
        &self,
        caller: &crate::auth::Subject,
        _project_id: &str,
    ) -> Result<Option<ReleaseRecord>, AlienError> {
        self.get_latest_release(caller).await
    }

    async fn list_releases(
        &self,
        caller: &crate::auth::Subject,
    ) -> Result<Vec<ReleaseRecord>, AlienError>;

    /// Keep `chart` for its release and version unless one is already kept,
    /// and return the kept copy. Insert-if-absent, so two managers serving
    /// the same version at once keep one copy and both return it.
    ///
    /// Stores that don't keep charts return an error: a manager that serves
    /// charts needs durable copies, or a version could change content.
    async fn store_chart_if_absent(
        &self,
        _caller: &crate::auth::Subject,
        _chart: StoredChart,
    ) -> Result<StoredChart, AlienError> {
        Err(AlienError::new(GenericError {
            message: "this release store cannot keep chart versions".to_string(),
        }))
    }

    /// Every kept chart version of the releases in `release_ids`.
    async fn stored_charts(
        &self,
        _caller: &crate::auth::Subject,
        _release_ids: &[String],
    ) -> Result<Vec<StoredChart>, AlienError> {
        Ok(Vec::new())
    }
}
