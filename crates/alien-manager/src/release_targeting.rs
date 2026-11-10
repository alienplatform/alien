//! Which release a deployment should run, and sending a deployment there when
//! its report shows it settled on another one.
//!
//! A new release fans out once, when it is published, and skips deployments
//! that are still installing or updating. Those deployments settle on the
//! release they were started with, so every report that leaves one Running
//! with nothing queued is checked against the release it should run now.

use std::sync::Arc;

use alien_error::AlienError;

use crate::auth::Subject;
use crate::traits::{
    DeploymentRecord, DeploymentRouting, DeploymentStore, ReleaseChannelStore, ReleaseStore,
    DEFAULT_CHANNEL,
};

/// The stores that decide which release a deployment runs.
#[derive(Clone)]
pub struct ReleaseTargeting {
    pub release_store: Arc<dyn ReleaseStore>,
    /// `None` on a manager without release channels.
    pub channels: Option<Arc<dyn ReleaseChannelStore>>,
}

impl ReleaseTargeting {
    /// Queue an update when `record` settled Running on a release other than
    /// the one it should run. `record` is updated to match what was stored.
    pub async fn retarget_if_settled(
        &self,
        deployment_store: &dyn DeploymentStore,
        subject: &Subject,
        record: &mut DeploymentRecord,
    ) -> Result<(), AlienError> {
        retarget_if_settled(
            deployment_store,
            self.release_store.as_ref(),
            self.channels.as_deref(),
            subject,
            record,
        )
        .await
    }
}

/// The release a deployment should run, and the routing that choice rests on.
struct Selection {
    release_id: Option<String>,
    /// The channel and pin the choice was read from; `None` without channels.
    routing: Option<DeploymentRouting>,
}

async fn select_release(
    channels: Option<&dyn ReleaseChannelStore>,
    release_store: &dyn ReleaseStore,
    subject: &Subject,
    deployment: Option<&DeploymentRecord>,
) -> Result<Selection, AlienError> {
    let mut used_routing = None;
    if let Some(channels) = channels {
        let routing = match deployment {
            Some(deployment) => channels.deployment_routing(&deployment.id).await?,
            None => DeploymentRouting {
                channel: DEFAULT_CHANNEL.to_string(),
                pinned_release_id: None,
            },
        };
        let release_id = match &routing.pinned_release_id {
            Some(pinned) => Some(pinned.clone()),
            None => channels
                .get_channel(&routing.channel)
                .await?
                .and_then(|channel| channel.current_release_id),
        };
        if release_id.is_some() {
            return Ok(Selection {
                release_id,
                routing: Some(routing),
            });
        }
        if routing.channel != DEFAULT_CHANNEL {
            return Ok(Selection {
                release_id: None,
                routing: Some(routing),
            });
        }
        used_routing = Some(routing);
    } else if let Some(release_id) = deployment.and_then(|d| d.desired_release_id.as_deref()) {
        return Ok(Selection {
            release_id: Some(release_id.to_string()),
            routing: None,
        });
    }
    Ok(Selection {
        release_id: release_store.latest_release_id(subject).await?,
        routing: used_routing,
    })
}

/// The id of the release a deployment should run: its pin, else its channel's
/// release. A channel with no release yet sends nothing, so a deployment never
/// runs a release published to another channel; only the default channel falls
/// back to the latest release (a manager that had releases before it had
/// channels). Without channels, the release the manager already chose for it,
/// else the latest release. `deployment` is `None` for one being created,
/// which follows the default channel.
pub async fn release_id_for_deployment(
    channels: Option<&dyn ReleaseChannelStore>,
    release_store: &dyn ReleaseStore,
    subject: &Subject,
    deployment: Option<&DeploymentRecord>,
) -> Result<Option<String>, AlienError> {
    Ok(select_release(channels, release_store, subject, deployment)
        .await?
        .release_id)
}

/// Queue an update when `record` is Running with nothing queued on a release
/// other than the one [`release_id_for_deployment`] picks. Runs on every report
/// that leaves a deployment Running, so it returns before any query unless the
/// deployment is settled.
pub async fn retarget_if_settled(
    deployment_store: &dyn DeploymentStore,
    release_store: &dyn ReleaseStore,
    channels: Option<&dyn ReleaseChannelStore>,
    subject: &Subject,
    record: &mut DeploymentRecord,
) -> Result<(), AlienError> {
    if record.status != "running" || record.desired_release_id.is_some() {
        return Ok(());
    }
    let Some(current) = record.current_release_id.clone() else {
        return Ok(());
    };
    let selection = select_release(channels, release_store, subject, Some(record)).await?;
    let Some(target) = selection.release_id else {
        return Ok(());
    };
    if target == current {
        return Ok(());
    }
    if deployment_store
        .retarget_settled_deployment(subject, &record.id, &current, &target, selection.routing)
        .await?
    {
        record.desired_release_id = Some(target);
        record.status = "update-pending".to_string();
    }
    Ok(())
}
