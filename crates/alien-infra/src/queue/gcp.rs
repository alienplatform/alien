// Stub module for future GCP Queue controller implementation.

use crate::core::{Policy, ResourceControllerContext, Subscription, Topic};
use crate::error::{ErrorData, Result};
use alien_core::{
    GcpPubSubQueueHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, Queue, QueueHeartbeatData, QueueHeartbeatStatus, QueueOutputs,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use alien_permissions::generators::GcpBindingResourceKind;
use chrono::Utc;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::info;

fn is_remote_resource_not_found(error: &AlienError<ErrorData>) -> bool {
    matches!(
        error.code.as_str(),
        "REMOTE_RESOURCE_NOT_FOUND" | "CLOUD_RESOURCE_NOT_FOUND"
    )
}

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
        let subscription =
            Subscription::new().set_topic(format!("projects/{}/topics/{}", cfg.project_id, topic));

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

        if let Some(topic_name) = &self.topic_name {
            use crate::core::ResourcePermissionsHelper;

            let gcp_config = ctx.get_gcp_config()?;
            let mut iam_bindings = Vec::new();
            ResourcePermissionsHelper::collect_gcp_resource_scoped_iam_bindings(
                ctx,
                &config.id,
                topic_name,
                "queue",
                &mut iam_bindings,
            )
            .await?;

            // Apply IAM permissions to the topic
            {
                let client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
                let topic_name_owned = topic_name.clone();
                let iam_policy =
                    gcp_iam_policy_for_kind(&iam_bindings, GcpBindingResourceKind::PubsubTopic);
                if !iam_policy.bindings.is_empty() {
                    let config_id_owned = config.id.clone();
                    client
                        .set_topic_iam_policy(topic_name_owned.clone(), iam_policy)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to apply IAM policy to Pub/Sub topic '{}'",
                                topic_name_owned
                            ),
                            resource_id: Some(config_id_owned),
                        })?;
                    info!(topic = %topic_name_owned, "Applied IAM policy to topic");
                }
            }

            // Apply IAM permissions to the subscription
            if let Some(subscription_name) = &self.subscription_name {
                let client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
                let sub_name_owned = subscription_name.clone();
                let iam_policy = gcp_iam_policy_for_kind(
                    &iam_bindings,
                    GcpBindingResourceKind::PubsubSubscription,
                );
                if !iam_policy.bindings.is_empty() {
                    let config_id_owned = config.id.clone();
                    client
                        .set_subscription_iam_policy(sub_name_owned.clone(), iam_policy)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to apply IAM policy to Pub/Sub subscription '{}'",
                                sub_name_owned
                            ),
                            resource_id: Some(config_id_owned),
                        })?;
                    info!(subscription = %sub_name_owned, "Applied IAM policy to subscription");
                }
            }
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
        let topic_metadata =
            client
                .get_topic(topic.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get Pub/Sub topic '{}'", topic),
                    resource_id: Some(q.id.clone()),
                })?;
        let subscription_metadata = if let Some(subscription) = &self.subscription_name {
            Some(
                client
                    .get_subscription(subscription.clone())
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get Pub/Sub subscription '{}'", subscription),
                        resource_id: Some(q.id.clone()),
                    })?,
            )
        } else {
            None
        };
        emit_gcp_pubsub_queue_heartbeat(
            ctx,
            &q.id,
            &cfg.project_id,
            topic,
            self.subscription_name.as_deref(),
            topic_metadata,
            subscription_metadata,
        );
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
                Err(e) if is_remote_resource_not_found(&e) => {
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
                Err(e) if is_remote_resource_not_found(&e) => {
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

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, QueueBinding};
        if let (Some(topic), Some(sub)) = (&self.topic_name, &self.subscription_name) {
            // For runtime binding params, we can't know the project ID at controller level,
            // so we use the simple topic/subscription names. The provider will construct
            // the full resource names using the actual GCP project from the configuration.
            let binding = QueueBinding::pubsub(
                BindingValue::value(topic.clone()),
                BindingValue::value(sub.clone()),
            );
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

fn gcp_iam_policy_for_kind(
    bindings: &[alien_permissions::generators::GcpIamBinding],
    kind: GcpBindingResourceKind,
) -> Policy {
    Policy::new().set_version(3).set_bindings(
        bindings
            .iter()
            .filter(|binding| binding.resource_kind == Some(kind))
            .cloned()
            .map(crate::core::ResourcePermissionsHelper::gcp_policy_binding_from_iam_binding),
    )
}

fn emit_gcp_pubsub_queue_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    project_id: &str,
    topic_name: &str,
    subscription_name: Option<&str>,
    topic: Topic,
    subscription: Option<Subscription>,
) {
    let topic_labels = topic.labels.into_iter().collect();
    let (message_storage_allowed_persistence_regions, message_storage_enforce_in_transit) = topic
        .message_storage_policy
        .map(|policy| {
            (
                policy.allowed_persistence_regions,
                Some(policy.enforce_in_transit),
            )
        })
        .unwrap_or_default();
    let (schema_name, schema_encoding, schema_first_revision_id, schema_last_revision_id) = topic
        .schema_settings
        .map(|settings| {
            (
                Some(settings.schema),
                settings.encoding.name().map(String::from),
                none_if_empty(settings.first_revision_id),
                none_if_empty(settings.last_revision_id),
            )
        })
        .unwrap_or_default();

    let subscription_labels: BTreeMap<_, _> = subscription
        .as_ref()
        .map(|subscription| subscription.labels.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();
    let push_config = subscription
        .as_ref()
        .and_then(|subscription| subscription.push_config.as_ref());
    let push_attributes = push_config
        .map(|push_config| push_config.attributes.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();
    let oidc_token = push_config.and_then(|push_config| push_config.oidc_token());
    let no_wrapper = push_config.and_then(|push_config| push_config.no_wrapper());
    let dead_letter_policy = subscription
        .as_ref()
        .and_then(|subscription| subscription.dead_letter_policy.as_ref());
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Queue::RESOURCE_TYPE,
        controller_platform: Platform::Gcp,
        backend: HeartbeatBackend::Gcp,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Queue(QueueHeartbeatData::GcpPubSub(
            GcpPubSubQueueHeartbeatData {
                status: QueueHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "GCP Pub/Sub topic '{}' metadata is reachable",
                        topic_name
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                topic_name: topic_name.to_string(),
                subscription_name: subscription_name.map(ToString::to_string),
                project_id: Some(project_id.to_string()),
                topic_full_name: none_if_empty(topic.name),
                subscription_full_name: subscription
                    .as_ref()
                    .and_then(|sub| none_if_empty(sub.name.clone())),
                endpoint: Some(format!(
                    "https://pubsub.googleapis.com/v1/projects/{}/topics/{}",
                    project_id, topic_name
                )),
                topic_labels,
                subscription_labels,
                message_storage_allowed_persistence_regions,
                message_storage_enforce_in_transit,
                kms_key_name: none_if_empty(topic.kms_key_name),
                schema_name,
                schema_encoding,
                schema_first_revision_id,
                schema_last_revision_id,
                topic_message_retention_duration: topic
                    .message_retention_duration
                    .map(String::from),
                topic_state: topic.state.name().map(String::from),
                subscription_ack_deadline_seconds: subscription
                    .as_ref()
                    .and_then(|sub| nonnegative_i32_to_u32(sub.ack_deadline_seconds)),
                subscription_message_retention_duration: subscription
                    .as_ref()
                    .and_then(|sub| sub.message_retention_duration.clone().map(String::from)),
                subscription_retain_acked_messages: subscription
                    .as_ref()
                    .map(|sub| sub.retain_acked_messages),
                subscription_enable_message_ordering: subscription
                    .as_ref()
                    .map(|sub| sub.enable_message_ordering),
                subscription_filter: subscription
                    .as_ref()
                    .and_then(|sub| none_if_empty(sub.filter.clone())),
                subscription_detached: subscription.as_ref().map(|sub| sub.detached),
                subscription_state: subscription
                    .as_ref()
                    .and_then(|sub| sub.state.name().map(String::from)),
                subscription_push_config_present: subscription
                    .as_ref()
                    .map(|sub| sub.push_config.is_some()),
                subscription_push_endpoint: push_config
                    .and_then(|push| none_if_empty(push.push_endpoint.clone())),
                subscription_push_attributes: push_attributes,
                subscription_push_oidc_service_account_email: oidc_token
                    .and_then(|token| none_if_empty(token.service_account_email.clone())),
                subscription_push_oidc_audience: oidc_token
                    .and_then(|token| none_if_empty(token.audience.clone())),
                subscription_push_pubsub_wrapper_write_metadata: None,
                subscription_push_no_wrapper_write_metadata: no_wrapper
                    .map(|wrapper| wrapper.write_metadata),
                subscription_dead_letter_topic: dead_letter_policy
                    .and_then(|policy| none_if_empty(policy.dead_letter_topic.clone())),
                subscription_dead_letter_max_delivery_attempts: dead_letter_policy
                    .and_then(|policy| nonnegative_i32_to_u32(policy.max_delivery_attempts)),
            },
        )),
        raw: vec![],
    });
}

fn none_if_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn nonnegative_i32_to_u32(value: i32) -> Option<u32> {
    u32::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        controller_test::SingleControllerExecutor, MockGcpIamApi, MockPlatformServiceProvider,
        MockPubSubApi,
    };
    use alien_core::{Platform, Queue, ResourceStatus};
    use std::sync::Arc;

    fn setup_mock_pubsub() -> Arc<MockPubSubApi> {
        let mut mock = MockPubSubApi::new();
        mock.expect_create_topic()
            .returning(|_, _| Ok(Topic::default()));
        mock.expect_create_subscription()
            .returning(|_, _| Ok(Subscription::default()));
        mock.expect_get_topic().returning(|_| Ok(Topic::default()));
        mock.expect_get_subscription()
            .returning(|_| Ok(Subscription::default()));
        mock.expect_delete_subscription().returning(|_| Ok(()));
        mock.expect_delete_topic().returning(|_| Ok(()));
        Arc::new(mock)
    }

    fn create_gcp_iam_mock_for_resource_permissions() -> Arc<MockGcpIamApi> {
        Arc::new(MockGcpIamApi::new())
    }

    fn setup_mock_provider(mock_pubsub: Arc<MockPubSubApi>) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_pubsub_client()
            .returning(move |_| Ok(mock_pubsub.clone()));

        // Mock IAM client for resource-scoped permissions.
        let mock_iam = create_gcp_iam_mock_for_resource_permissions();
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

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
