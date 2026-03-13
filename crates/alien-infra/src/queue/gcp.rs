// Stub module for future GCP Queue controller implementation.

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Queue, QueueOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context};
use alien_gcp_clients::pubsub::{Subscription, Topic};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use std::time::Duration;
use tracing::info;

fn get_topic_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct GcpQueueController {
    pub(crate) topic_name: Option<String>,
    pub(crate) subscription_name: Option<String>,
}

#[controller]
impl GcpQueueController {
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_pubsub_client(cfg)?;
        let q = ctx.desired_resource_config::<Queue>()?;
        let topic = get_topic_name(ctx.resource_prefix, &q.id);

        // Create topic id without full path; client expects id
        client
            .create_topic(topic.clone(), Topic::default())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create Pub/Sub topic '{}'", topic),
                resource_id: Some(q.id.clone()),
            })?;

        self.topic_name = Some(topic);
        info!(topic=?self.topic_name, "Created Pub/Sub topic");
        Ok(HandlerAction::Continue {
            state: CreateSubscription,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreateSubscription,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_subscription(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_pubsub_client(cfg)?;
        let q = ctx.desired_resource_config::<Queue>()?;
        let topic = self.topic_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Topic name not set in state".to_string(),
                resource_id: Some(q.id.clone()),
            })
        })?;
        let sub = format!("{}-sub", topic);

        // Create pull subscription
        let subscription = Subscription::builder()
            .topic(format!("projects/{}/topics/{}", cfg.project_id, topic))
            .build();

        client
            .create_subscription(sub.clone(), subscription)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create Pub/Sub subscription '{}'", sub),
                resource_id: Some(q.id.clone()),
            })?;

        self.subscription_name = Some(sub);
        info!(topic=?self.topic_name, subscription=?self.subscription_name, "GCP Pub/Sub created");
        Ok(HandlerAction::Continue {
            state: ApplyingPermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Queue>()?;

        info!(resource_id = %config.id(), "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let Some(topic_name) = &self.topic_name {
            use crate::core::ResourcePermissionsHelper;

            // Get the GCP Pub/Sub client
            let gcp_config = ctx.get_gcp_config()?;
            let client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;

            // Apply IAM permissions to the topic
            let topic_name_owned = topic_name.clone();
            let config_id_owned = config.id.clone();
            ResourcePermissionsHelper::apply_gcp_resource_scoped_permissions(
                ctx,
                &config.id,
                topic_name,
                "Queue",
                client,
                |client, iam_policy| async move {
                    // Set IAM policy on the topic
                    client
                        .set_topic_iam_policy(topic_name_owned.clone(), iam_policy)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to apply IAM policy to Pub/Sub topic '{}'", topic_name_owned),
                            resource_id: Some(config_id_owned),
                        })?;

                    info!(topic = %topic_name_owned, "Successfully applied resource-scoped IAM policy");
                    Ok(())
                },
            ).await?;
        }

        info!(resource_id = %config.id(), "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_pubsub_client(cfg)?;
        let q = ctx.desired_resource_config::<Queue>()?;
        let topic = self.topic_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Topic name not set in state".to_string(),
                resource_id: Some(q.id.clone()),
            })
        })?;
        // Heartbeat: get topic
        let _ = client
            .get_topic(topic.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get Pub/Sub topic '{}'", topic),
                resource_id: Some(q.id.clone()),
            })?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    // Queue has no mutable fields — update is a no-op that also recovers RefreshFailed.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Queue>()?;
        info!(id=%config.id, "GCP Queue update (no-op — no mutable fields)");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_pubsub_client(cfg)?;
        let _ = ctx.desired_resource_config::<Queue>()?;

        if let Some(sub) = &self.subscription_name {
            match client.delete_subscription(sub.clone()).await {
                Ok(()) => info!(subscription = %sub, "Pub/Sub subscription deleted"),
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(subscription = %sub, "Pub/Sub subscription already deleted");
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete Pub/Sub subscription '{}'", sub),
                        resource_id: None,
                    });
                }
            }
        }
        if let Some(topic) = &self.topic_name {
            match client.delete_topic(topic.clone()).await {
                Ok(()) => info!(topic = %topic, "Pub/Sub topic deleted"),
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(topic = %topic, "Pub/Sub topic already deleted");
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete Pub/Sub topic '{}'", topic),
                        resource_id: None,
                    });
                }
            }
        }
        self.topic_name = None;
        self.subscription_name = None;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(topic), Some(sub)) = (&self.topic_name, &self.subscription_name) {
            Some(ResourceOutputs::new(QueueOutputs {
                queue_name: topic.clone(),
                identifier: Some(sub.clone()),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, QueueBinding};
        if let (Some(topic), Some(sub)) = (&self.topic_name, &self.subscription_name) {
            // For runtime binding params, we can't know the project ID at controller level,
            // so we use the simple topic/subscription names. The provider will construct
            // the full resource names using the actual GCP project from the configuration.
            let binding = QueueBinding::pubsub(
                BindingValue::value(topic.clone()),
                BindingValue::value(sub.clone()),
            );
            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider, PlatformServiceProvider,
    };
    use alien_core::{Platform, Queue, ResourceStatus};
    use alien_gcp_clients::pubsub::{ListTopicsResponse, MockPubSubApi, Subscription, Topic};
    use std::sync::Arc;

    fn setup_mock_pubsub() -> Arc<MockPubSubApi> {
        let mut mock = MockPubSubApi::new();
        mock.expect_create_topic()
            .returning(|_, _| Ok(Topic::default()));
        mock.expect_create_subscription()
            .returning(|_, _| Ok(Subscription::default()));
        mock.expect_get_topic().returning(|_| Ok(Topic::default()));
        mock.expect_delete_subscription().returning(|_| Ok(()));
        mock.expect_delete_topic().returning(|_| Ok(()));
        Arc::new(mock)
    }

    fn setup_mock_provider(mock_pubsub: Arc<MockPubSubApi>) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_pubsub_client()
            .returning(move |_| Ok(mock_pubsub.clone()));
        Arc::new(provider)
    }

    #[tokio::test]
    async fn test_create_and_delete_pubsub_queue_succeeds() {
        let queue = Queue::new("gcp-queue".to_string()).build();
        let mock_pubsub = setup_mock_pubsub();
        let mock_provider = setup_mock_provider(mock_pubsub);

        let mut executor = SingleControllerExecutor::builder()
            .resource(queue)
            .controller(GcpQueueController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }
}
