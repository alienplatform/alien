use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use alien_core::{Platform, SignedStatement, Stack};
use alien_error::AlienError;
use alien_signing::{Kind, ReleaseBody};

/// Setup template bytes by template name (`cloudformation`, `helm`, ...),
/// base64-encoded.
pub type SetupTemplateBytes = BTreeMap<String, String>;

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
    /// Per-platform signed statements, stored exactly as received.
    #[serde(default)]
    pub signed_stacks: HashMap<Platform, SignedStatement>,
    /// Per-platform setup template bytes the signed statements digest, by
    /// template name (base64), stored exactly as received.
    #[serde(default)]
    pub setup_templates: HashMap<Platform, SetupTemplateBytes>,
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
    /// Per-platform signed statements, stored exactly as received.
    pub signed_stacks: HashMap<Platform, SignedStatement>,
    /// Per-platform setup template bytes, checked against the signed digests.
    pub setup_templates: HashMap<Platform, SetupTemplateBytes>,
    pub git_commit_sha: Option<String>,
    pub git_commit_ref: Option<String>,
    pub git_commit_message: Option<String>,
}

/// Which release a protected setup installs when it registers without a
/// release id: one signed for `platform` on `channel` whose stack has this
/// setup fingerprint. Setup templates are signed before their release exists,
/// so they can't name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedSetupQuery {
    pub platform: Platform,
    /// The channel the setup pins (its signing settings).
    pub channel: String,
    /// The setup target the fingerprint was computed for (`aws`, `gcp`, ...).
    pub setup_target: String,
    pub setup_fingerprint: String,
    pub setup_fingerprint_version: u32,
}

impl ProtectedSetupQuery {
    /// Whether `release` is signed for this setup's platform and channel and
    /// its stack has this setup fingerprint. Signatures aren't checked here:
    /// the deployment verifies the statement against its own settings.
    pub fn matches(&self, release: &ReleaseRecord) -> bool {
        let Some(body) = release
            .signed_stacks
            .get(&self.platform)
            .and_then(|statement| statement.open().ok())
            .filter(|opened| opened.kind == Kind::Release)
            .and_then(|opened| opened.body::<ReleaseBody>().ok())
        else {
            return false;
        };
        if body.channel != self.channel {
            return false;
        }
        let Some(stack) = release
            .stacks
            .get(&self.platform)
            .and_then(|stack| serde_json::to_value(stack).ok())
        else {
            return false;
        };
        alien_cli_common::setup_fingerprint::setup_fingerprint(&stack, &self.setup_target)
            == (
                self.setup_fingerprint.clone(),
                self.setup_fingerprint_version,
            )
    }
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

    /// The newest release a protected setup that registered without a release
    /// id installs (see [`ProtectedSetupQuery`]). None when there is none.
    async fn get_release_for_protected_setup(
        &self,
        caller: &crate::auth::Subject,
        query: &ProtectedSetupQuery,
    ) -> Result<Option<ReleaseRecord>, AlienError> {
        Ok(self
            .list_releases(caller)
            .await?
            .into_iter()
            .filter(|release| query.matches(release))
            .max_by_key(|release| release.created_at))
    }
}
