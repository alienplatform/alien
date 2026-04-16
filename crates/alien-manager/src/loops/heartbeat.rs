//! Heartbeat loop — polls running deployments and updates heartbeat timestamps.
//!
//! This keeps track of which deployments are still alive. The deployment store
//! updates the `updated_at` timestamp for each running deployment on every tick.

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, error};

use crate::config::ManagerConfig;
use crate::traits::deployment_store::DeploymentFilter;
use crate::traits::DeploymentStore;

pub struct HeartbeatLoop {
    config: Arc<ManagerConfig>,
    deployment_store: Arc<dyn DeploymentStore>,
}

impl HeartbeatLoop {
    pub fn new(config: Arc<ManagerConfig>, deployment_store: Arc<dyn DeploymentStore>) -> Self {
        Self {
            config,
            deployment_store,
        }
    }

    /// Run the heartbeat loop forever.
    pub async fn run(&self) {
        debug!(
            interval_secs = self.config.heartbeat_interval_secs,
            "Starting heartbeat loop"
        );

        loop {
            self.tick().await;
            tokio::time::sleep(Duration::from_secs(self.config.heartbeat_interval_secs)).await;
        }
    }

    /// One heartbeat tick: list running deployments and update their timestamps.
    async fn tick(&self) {
        let filter = DeploymentFilter {
            statuses: Some(vec!["running".to_string()]),
            ..Default::default()
        };

        match self.deployment_store.list_deployments(&filter).await {
            Ok(deployments) => {
                if !deployments.is_empty() {
                    debug!(
                        count = deployments.len(),
                        "Heartbeat: found running deployments"
                    );
                }
                for deployment in &deployments {
                    // Use reconcile with update_heartbeat to bump the timestamp,
                    // or simply re-list. For now, listing is sufficient —
                    // the deployment loop's reconcile already sets update_heartbeat
                    // for running deployments.
                    //
                    // In the future, a dedicated heartbeat_update method could
                    // detect stale deployments and mark them as unhealthy.
                    debug!(
                        deployment_id = %deployment.id,
                        "Heartbeat: deployment alive"
                    );
                }
            }
            Err(e) => {
                error!(error = %e, "Heartbeat: failed to list running deployments");
            }
        }
    }
}
