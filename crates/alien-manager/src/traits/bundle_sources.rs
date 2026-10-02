//! Where an air-gapped bundle's chart comes from.
//!
//! A manager that serves charts itself (see
//! [`crate::routes::charts::ChartSettings`]) answers from its own registry.
//! Managers whose charts are published elsewhere plug in a
//! [`BundleSourceResolver`]. The images the chart runs (the Operator, its
//! cleanup hooks) are read from the chart's values by the bundler, so the
//! chart is the only source of truth for them.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use alien_error::AlienError;

use crate::auth::Subject;
use crate::traits::{DeploymentRecord, ReleaseRecord};

/// The artifacts a bundle carries besides the release's own images.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BundleSources {
    /// The Helm chart, as an OCI reference (`registry/repository:version`).
    pub chart: BundleSource,
    /// `alien-deploy` builds for the site, matching this manager.
    #[serde(default)]
    pub deploy_cli: Vec<ToolDownload>,
}

/// A downloadable build of a tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ToolDownload {
    /// `<os>-<arch>`, e.g. `linux-x86_64`.
    pub platform: String,
    pub url: String,
}

/// `alien-deploy` builds for Linux sites from a releases server, the version
/// this manager was built from.
pub fn deploy_cli_downloads(releases_url: &str) -> Vec<ToolDownload> {
    let version = env!("CARGO_PKG_VERSION");
    ["x86_64", "aarch64"]
        .into_iter()
        .map(|arch| ToolDownload {
            platform: format!("linux-{arch}"),
            url: format!(
                "{}/alien-deploy/v{version}/linux-{arch}/alien-deploy",
                releases_url.trim_end_matches('/')
            ),
        })
        .collect()
}

/// One OCI artifact and how to authenticate when pulling it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BundleSource {
    pub reference: String,
    pub credentials: SourceCredentials,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum SourceCredentials {
    /// Pull with the caller's own token, from this manager's registry.
    Caller,
    /// A public artifact.
    Anonymous,
}

#[async_trait]
pub trait BundleSourceResolver: Send + Sync {
    async fn resolve(
        &self,
        subject: &Subject,
        deployment: &DeploymentRecord,
        release: &ReleaseRecord,
    ) -> Result<BundleSources, AlienError>;
}
