//! Release channels for managers that store deployments themselves.
//!
//! A channel is a named pointer to a release (`production` by default).
//! Every deployment follows one channel, or is pinned to a release. A new
//! release advances one channel and rolls out to the deployments following
//! it; promoting an existing release moves a channel without rebuilding.
//!
//! Optional: embedders with their own channel model leave it unset, and the
//! manager then sends every release to every deployment.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use alien_error::AlienError;

/// The channel deployments follow unless told otherwise.
pub const DEFAULT_CHANNEL: &str = "production";

/// A channel and the release it points at.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseChannelRecord {
    pub name: String,
    /// `None` for a channel created before any release was promoted to it.
    pub current_release_id: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// Which release a deployment should run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentRouting {
    /// Channel the deployment follows.
    pub channel: String,
    /// Release the deployment is pinned to, overriding the channel.
    pub pinned_release_id: Option<String>,
}

#[async_trait]
pub trait ReleaseChannelStore: Send + Sync {
    async fn list_channels(&self) -> Result<Vec<ReleaseChannelRecord>, AlienError>;

    async fn get_channel(&self, name: &str) -> Result<Option<ReleaseChannelRecord>, AlienError>;

    /// Create a channel. Fails if it exists.
    async fn create_channel(
        &self,
        name: &str,
        release_id: Option<&str>,
    ) -> Result<ReleaseChannelRecord, AlienError>;

    /// Delete a channel. Returns whether it existed.
    async fn delete_channel(&self, name: &str) -> Result<bool, AlienError>;

    /// Point `name` at `release_id`, creating the channel if needed.
    async fn set_channel_release(
        &self,
        name: &str,
        release_id: &str,
    ) -> Result<ReleaseChannelRecord, AlienError>;

    /// How a deployment is routed. Deployments never configured follow
    /// [`DEFAULT_CHANNEL`] unpinned.
    async fn deployment_routing(
        &self,
        deployment_id: &str,
    ) -> Result<DeploymentRouting, AlienError>;

    async fn set_deployment_routing(
        &self,
        deployment_id: &str,
        routing: &DeploymentRouting,
    ) -> Result<(), AlienError>;

    /// Number of deployments following `channel` (pinned or not).
    async fn count_following(&self, channel: &str) -> Result<u64, AlienError>;

    /// Send `release_id` to the unpinned deployments following `channel`,
    /// as a new release reaches every deployment when there are no channels.
    async fn roll_out_channel(&self, channel: &str, release_id: &str) -> Result<(), AlienError>;

    /// Send `release_id` to one deployment.
    async fn roll_out_deployment(
        &self,
        deployment_id: &str,
        release_id: &str,
    ) -> Result<(), AlienError>;
}
