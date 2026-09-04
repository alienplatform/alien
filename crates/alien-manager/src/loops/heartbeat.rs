//! Heartbeat loop — periodically refreshes running deployments.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use tracing::{debug, error};

use alien_core::DeploymentModel;

use crate::auth::Subject;
use crate::config::ManagerConfig;
use crate::loops::deployment::{DeploymentLoop, MAX_CONCURRENT_DEPLOYMENTS};
use crate::traits::deployment_store::{AcquiredDeployment, DeploymentFilter};
use crate::traits::DeploymentStore;

/// Maximum deployments to health-check per tick, across all batches.
const HEARTBEAT_DEPLOYMENTS_PER_TICK: usize = 100;

pub struct HeartbeatLoop {
    config: Arc<ManagerConfig>,
    deployment_store: Arc<dyn DeploymentStore>,
    deployment_processor: Arc<dyn HeartbeatDeploymentProcessor>,
}

#[async_trait]
pub(crate) trait HeartbeatDeploymentProcessor: Send + Sync {
    async fn process_heartbeat_deployment(&self, item: AcquiredDeployment, session: &str);
}

#[async_trait]
impl HeartbeatDeploymentProcessor for DeploymentLoop {
    async fn process_heartbeat_deployment(&self, item: AcquiredDeployment, session: &str) {
        self.process_heartbeat_deployment(item.deployment, item.execution_claim, session)
            .await;
    }
}

impl HeartbeatLoop {
    pub(crate) fn new(
        config: Arc<ManagerConfig>,
        deployment_store: Arc<dyn DeploymentStore>,
        deployment_processor: Arc<dyn HeartbeatDeploymentProcessor>,
    ) -> Self {
        Self {
            config,
            deployment_store,
            deployment_processor,
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
            deployment_model: Some(DeploymentModel::Push),
            ..Default::default()
        };

        // Internal loop: no inbound caller. `Subject::system()` carries an
        // empty `bearer_token` — the documented signal to embedders that
        // no caller passthrough is available.
        let caller = Subject::system();

        // Acquire in batches no larger than the processing concurrency. A lease
        // is only renewed once its deployment is being processed, so a larger
        // batch would leave the surplus holding leases that nothing extends.
        let batches = HEARTBEAT_DEPLOYMENTS_PER_TICK.div_ceil(MAX_CONCURRENT_DEPLOYMENTS);
        for _ in 0..batches {
            let session = uuid::Uuid::new_v4().to_string();
            match self
                .deployment_store
                .acquire(
                    &caller,
                    &session,
                    &filter,
                    MAX_CONCURRENT_DEPLOYMENTS as u32,
                )
                .await
            {
                Ok(acquired) => {
                    if acquired.is_empty() {
                        break;
                    }
                    debug!(
                        count = acquired.len(),
                        session = %session,
                        "Heartbeat: acquired running deployments"
                    );
                    stream::iter(acquired)
                        .for_each_concurrent(MAX_CONCURRENT_DEPLOYMENTS, |item| async {
                            self.deployment_processor
                                .process_heartbeat_deployment(item, &session)
                                .await;
                        })
                        .await;
                }
                Err(e) => {
                    error!(error = %e, "Heartbeat: failed to acquire running deployments");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use alien_core::{DeploymentModel, Platform, StackSettings};
    use chrono::Utc;

    use super::*;
    use crate::traits::deployment_store::{DeploymentRecord, ExecutionClaim, MockDeploymentStore};

    struct RecordingProcessor {
        processed: Mutex<Vec<(AcquiredDeployment, String)>>,
    }

    #[async_trait]
    impl HeartbeatDeploymentProcessor for RecordingProcessor {
        async fn process_heartbeat_deployment(&self, item: AcquiredDeployment, session: &str) {
            self.processed
                .lock()
                .expect("recording processor mutex poisoned")
                .push((item, session.to_string()));
        }
    }

    fn running_deployment() -> DeploymentRecord {
        let mut stack_settings = StackSettings::default();
        stack_settings.deployment_model = DeploymentModel::Push;
        DeploymentRecord {
            id: "dep_heartbeat_claim".to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: "heartbeat-claim".to_string(),
            deployment_group_id: "dg_test".to_string(),
            platform: Platform::Machines,
            deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: "running".to_string(),
            stack_settings: Some(stack_settings),
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: Some("rel_test".to_string()),
            desired_release_id: Some("rel_test".to_string()),
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_config: None,
            deployment_token: None,
            input_values: Default::default(),
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn acquired_execution_claim_reaches_heartbeat_processor_unchanged() {
        let expected_claim = ExecutionClaim {
            operation_id: "op_123".to_string(),
            attempt_id: "attempt_456".to_string(),
        };
        let acquired = AcquiredDeployment {
            deployment: running_deployment(),
            execution_claim: Some(expected_claim.clone()),
        };
        let acquire_calls = Arc::new(AtomicUsize::new(0));
        let mut store = MockDeploymentStore::new();
        store.expect_acquire().times(2).returning({
            let acquire_calls = acquire_calls.clone();
            let acquired = acquired.clone();
            move |_, _, filter, limit| {
                assert_eq!(
                    filter.statuses.as_deref(),
                    Some(["running".to_string()].as_slice())
                );
                assert_eq!(filter.deployment_model, Some(DeploymentModel::Push));
                assert_eq!(limit, MAX_CONCURRENT_DEPLOYMENTS as u32);
                let result = if acquire_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    vec![acquired.clone()]
                } else {
                    Vec::new()
                };
                Ok(result)
            }
        });

        let processor = Arc::new(RecordingProcessor {
            processed: Mutex::new(Vec::new()),
        });
        let heartbeat = HeartbeatLoop::new(
            Arc::new(ManagerConfig::default()),
            Arc::new(store),
            processor.clone(),
        );

        heartbeat.tick().await;

        let processed = processor
            .processed
            .lock()
            .expect("recording processor mutex poisoned");
        assert_eq!(processed.len(), 1);
        assert_eq!(processed[0].0.deployment.id, "dep_heartbeat_claim");
        assert_eq!(processed[0].0.execution_claim, Some(expected_claim));
        assert!(!processed[0].1.is_empty());
    }
}
