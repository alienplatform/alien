//! Heartbeat loop — periodically refreshes running deployments.

use std::sync::Arc;
use std::time::Duration;

use futures::{stream, StreamExt};
use tracing::{debug, error};

use crate::auth::Subject;
use crate::config::ManagerConfig;
use crate::loops::deployment::{DeploymentLoop, MAX_CONCURRENT_DEPLOYMENTS};
use crate::traits::deployment_store::DeploymentFilter;
use crate::traits::DeploymentStore;

const HEARTBEAT_ACQUIRE_LIMIT: u32 = 100;

pub struct HeartbeatLoop {
    config: Arc<ManagerConfig>,
    deployment_store: Arc<dyn DeploymentStore>,
    deployment_loop: Arc<DeploymentLoop>,
}

impl HeartbeatLoop {
    pub fn new(
        config: Arc<ManagerConfig>,
        deployment_store: Arc<dyn DeploymentStore>,
        deployment_loop: Arc<DeploymentLoop>,
    ) -> Self {
        Self {
            config,
            deployment_store,
            deployment_loop,
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

    /// One heartbeat tick: acquire running deployments and run one health-check step.
    async fn tick(&self) {
        let filter = DeploymentFilter {
            statuses: Some(vec!["running".to_string()]),
            platforms: if self.config.targets.is_empty() {
                None
            } else {
                Some(self.config.targets.clone())
            },
            ..Default::default()
        };

        // Internal loop: no inbound caller. `Subject::system()` carries an
        // empty `bearer_token` — the documented signal to embedders that
        // no caller passthrough is available.
        let caller = Subject::system();
        let session = uuid::Uuid::new_v4().to_string();
        match self
            .deployment_store
            .acquire(&caller, &session, &filter, HEARTBEAT_ACQUIRE_LIMIT)
            .await
        {
            Ok(acquired) => {
                if !acquired.is_empty() {
                    debug!(
                        count = acquired.len(),
                        session = %session,
                        "Heartbeat: acquired running deployments"
                    );
                }
                stream::iter(acquired)
                    .for_each_concurrent(MAX_CONCURRENT_DEPLOYMENTS, |item| async {
                        self.deployment_loop
                            .process_heartbeat_deployment(item.deployment, &session)
                            .await;
                    })
                    .await;
            }
            Err(e) => {
                error!(error = %e, "Heartbeat: failed to acquire running deployments");
            }
        }
    }
}
