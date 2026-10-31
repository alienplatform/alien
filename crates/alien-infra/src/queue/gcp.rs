use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    bindings::{BindingValue, QueueBinding},
    storage_notifications::queue_receives_storage_notifications,
    GcpPubSubQueueHeartbeatData, HeartbeatBackend, InitialSetupAuthority, ObservedHealth, Platform,
    ProviderLifecycleState, Queue, QueueHeartbeatData, QueueHeartbeatStatus, QueueOutputs,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceLifecycle, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::{
    iam::{Binding, IamPolicy},
    pubsub::{Subscription, Topic},
};
use alien_macros::controller;
use alien_permissions::generators::GcpBindingResourceKind;
use chrono::Utc;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::info;

fn get_topic_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct GcpQueueController {
    pub(crate) topic_name: Option<String>,
    pub(crate) subscription_name: Option<String>,
    /// Service-agent membership managed by this queue for storage ingress, recorded before
    /// the grant is written.
    #[serde(default)]
    pub(crate) storage_publisher_member: Option<String>,
    /// Persist intent before a policy write, including recovery after a lost response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) storage_publisher_pending: Option<bool>,
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

        if let Some(topic_name) = &self.topic_name {
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
            state: PreparingStoragePublisher,
            suggested_delay: None,
        })
    }

    #[handler(state = PreparingStoragePublisher, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn preparing_storage_publisher(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.prepare_storage_publisher(ctx).await?;
        Ok(HandlerAction::Continue {
            state: ApplyingStoragePublisher,
            suggested_delay: None,
        })
    }

    #[handler(state = ApplyingStoragePublisher, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn applying_storage_publisher(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.reconcile_storage_publisher(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = UpdatingStoragePublisher, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn updating_storage_publisher(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.reconcile_storage_publisher(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let queue = ctx.desired_resource_config::<Queue>()?;
        Ok(may_manage_storage_publisher(ctx, &queue.id)
            && (self.storage_publisher_pending.is_some()
                || queue_receives_storage_notifications(ctx.desired_stack, &queue.id)
                    != self.storage_publisher_member.is_some()))
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
    // Storage ingress can change even when the Queue declaration is unchanged.
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Queue>()?;
        info!(id=%config.id, "Reconciling GCP Queue storage ingress");
        self.prepare_storage_publisher(ctx).await?;
        Ok(HandlerAction::Continue {
            state: UpdatingStoragePublisher,
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
        self.storage_publisher_member = None;
        self.storage_publisher_pending = None;
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

fn may_manage_storage_publisher(ctx: &ResourceControllerContext<'_>, id: &str) -> bool {
    ctx.initial_setup_authority == InitialSetupAuthority::DirectSetup
        || !ctx
            .desired_stack
            .resources
            .get(id)
            .is_some_and(|entry| entry.lifecycle == ResourceLifecycle::Frozen)
}

/// Preserve unrelated and conditional grants and the optimistic-concurrency etag.
fn set_storage_publisher(policy: &mut IamPolicy, member: &str, enabled: bool) -> bool {
    const ROLE: &str = "roles/pubsub.publisher";
    if enabled {
        if let Some(binding) = policy
            .bindings
            .iter_mut()
            .find(|binding| binding.role == ROLE && binding.condition.is_none())
        {
            if binding.members.iter().any(|existing| existing == member) {
                return false;
            }
            binding.members.push(member.to_string());
        } else {
            policy.bindings.push(Binding {
                role: ROLE.into(),
                members: vec![member.into()],
                condition: None,
            });
        }
        true
    } else {
        let mut changed = false;
        for binding in &mut policy.bindings {
            if binding.role == ROLE && binding.condition.is_none() {
                binding.members.retain(|existing| {
                    if existing == member {
                        changed = true;
                        false
                    } else {
                        true
                    }
                });
            }
        }
        if changed {
            policy
                .bindings
                .retain(|binding| !binding.members.is_empty());
        }
        changed
    }
}

fn gcp_iam_policy_for_kind(
    bindings: &[alien_permissions::generators::GcpIamBinding],
    kind: GcpBindingResourceKind,
) -> IamPolicy {
    IamPolicy {
        version: Some(3),
        bindings: bindings
            .iter()
            .filter(|binding| binding.resource_kind == Some(kind))
            .cloned()
            .map(crate::core::ResourcePermissionsHelper::gcp_policy_binding_from_iam_binding)
            .collect(),
        etag: None,
        kind: None,
        resource_id: None,
    }
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
    let topic_labels = topic.labels.unwrap_or_default().into_iter().collect();
    let (message_storage_allowed_persistence_regions, message_storage_enforce_in_transit) = topic
        .message_storage_policy
        .map(|policy| {
            (
                policy.allowed_persistence_regions,
                policy.enforce_in_transit,
            )
        })
        .unwrap_or_default();
    let (schema_name, schema_encoding, schema_first_revision_id, schema_last_revision_id) = topic
        .schema_settings
        .map(|settings| {
            (
                Some(settings.schema),
                serialize_enum_opt(settings.encoding),
                settings.first_revision_id,
                settings.last_revision_id,
            )
        })
        .unwrap_or_default();

    let subscription_labels: BTreeMap<_, _> = subscription
        .as_ref()
        .and_then(|subscription| subscription.labels.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();
    let push_config = subscription
        .as_ref()
        .and_then(|subscription| subscription.push_config.as_ref());
    let push_attributes = push_config
        .and_then(|push_config| push_config.attributes.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();
    let oidc_token = push_config.and_then(|push_config| push_config.oidc_token.as_ref());
    let pubsub_wrapper = push_config.and_then(|push_config| push_config.pubsub_wrapper.as_ref());
    let no_wrapper = push_config.and_then(|push_config| push_config.no_wrapper.as_ref());
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
                topic_full_name: topic.name,
                subscription_full_name: subscription.as_ref().and_then(|sub| sub.name.clone()),
                endpoint: Some(format!(
                    "https://pubsub.googleapis.com/v1/projects/{}/topics/{}",
                    project_id, topic_name
                )),
                topic_labels,
                subscription_labels,
                message_storage_allowed_persistence_regions,
                message_storage_enforce_in_transit,
                kms_key_name: topic.kms_key_name,
                schema_name,
                schema_encoding,
                schema_first_revision_id,
                schema_last_revision_id,
                topic_message_retention_duration: topic.message_retention_duration,
                topic_state: serialize_enum_opt(topic.state),
                subscription_ack_deadline_seconds: subscription
                    .as_ref()
                    .and_then(|sub| nonnegative_i32_to_u32(sub.ack_deadline_seconds)),
                subscription_message_retention_duration: subscription
                    .as_ref()
                    .and_then(|sub| sub.message_retention_duration.clone()),
                subscription_retain_acked_messages: subscription
                    .as_ref()
                    .and_then(|sub| sub.retain_acked_messages),
                subscription_enable_message_ordering: subscription
                    .as_ref()
                    .and_then(|sub| sub.enable_message_ordering),
                subscription_filter: subscription.as_ref().and_then(|sub| sub.filter.clone()),
                subscription_detached: subscription.as_ref().and_then(|sub| sub.detached),
                subscription_state: subscription
                    .as_ref()
                    .and_then(|sub| serialize_enum_opt(sub.state.clone())),
                subscription_push_config_present: subscription
                    .as_ref()
                    .map(|sub| sub.push_config.is_some()),
                subscription_push_endpoint: push_config.and_then(|push| push.push_endpoint.clone()),
                subscription_push_attributes: push_attributes,
                subscription_push_oidc_service_account_email: oidc_token
                    .map(|token| token.service_account_email.clone()),
                subscription_push_oidc_audience: oidc_token
                    .and_then(|token| token.audience.clone()),
                subscription_push_pubsub_wrapper_write_metadata: pubsub_wrapper
                    .and_then(|wrapper| wrapper.write_metadata),
                subscription_push_no_wrapper_write_metadata: no_wrapper
                    .and_then(|wrapper| wrapper.write_metadata),
                subscription_dead_letter_topic: dead_letter_policy
                    .and_then(|policy| policy.dead_letter_topic.clone()),
                subscription_dead_letter_max_delivery_attempts: dead_letter_policy
                    .and_then(|policy| nonnegative_i32_to_u32(policy.max_delivery_attempts)),
            },
        )),
        raw: vec![],
    });
}

fn serialize_enum<T: serde::Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
}

fn serialize_enum_opt<T: serde::Serialize>(value: Option<T>) -> Option<String> {
    value.and_then(|value| serialize_enum(&value))
}

fn nonnegative_i32_to_u32(value: Option<i32>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
    use alien_core::{Platform, Queue, ResourceStatus};
    use alien_gcp_clients::iam::MockIamApi;
    use alien_gcp_clients::pubsub::{MockPubSubApi, Subscription, Topic};
    use std::sync::Arc;

    #[tokio::test]
    async fn storage_ingress_changes_reconcile_an_unchanged_queue_and_preserve_other_members() {
        let queue = Queue::new("work".into()).build();
        let storage = alien_core::Storage::new("uploads".into())
            .notifications(vec![alien_core::StorageNotification::builder()
                .queue((&queue).into())
                .build()])
            .build();
        let original = IamPolicy {
            version: Some(3),
            etag: Some("preserved-etag".into()),
            bindings: vec![Binding {
                role: "roles/pubsub.publisher".into(),
                members: vec!["serviceAccount:manual@example.com".into()],
                condition: None,
            }],
            ..Default::default()
        };
        let policy = Arc::new(std::sync::Mutex::new(original.clone()));
        let mut pubsub = MockPubSubApi::new();
        pubsub
            .expect_get_topic()
            .times(1)
            .returning(|_| Ok(Topic::default()));
        pubsub
            .expect_get_subscription()
            .times(1)
            .returning(|_| Ok(Subscription::default()));
        let reads = policy.clone();
        pubsub
            .expect_get_topic_iam_policy()
            .times(2)
            .returning(move |topic| {
                assert_eq!(topic, "test-work");
                Ok(reads.lock().unwrap().clone())
            });
        let writes = policy.clone();
        pubsub
            .expect_set_topic_iam_policy()
            .times(2)
            .returning(move |topic, value| {
                assert_eq!(topic, "test-work");
                assert_eq!(value.etag.as_deref(), Some("preserved-etag"));
                *writes.lock().unwrap() = value.clone();
                Ok(value)
            });
        let mut gcs = alien_gcp_clients::gcs::MockGcsApi::new();
        gcs.expect_get_project_service_account()
            .times(1)
            .returning(|| {
                Ok(alien_gcp_clients::gcs::ProjectServiceAccount {
                    email_address: "service-123@gs-project-accounts.iam.gserviceaccount.com".into(),
                    kind: None,
                })
            });
        let gcs = Arc::new(gcs);
        let pubsub = Arc::new(pubsub);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_pubsub_client()
            .returning(move |_| Ok(pubsub.clone()));
        provider
            .expect_get_gcp_gcs_client()
            .returning(move |_| Ok(gcs.clone()));
        let mut run = SingleControllerExecutor::builder()
            .resource(queue.clone())
            .controller(GcpQueueController {
                state: GcpQueueState::Ready,
                topic_name: Some("test-work".into()),
                subscription_name: Some("test-work-sub".into()),
                ..Default::default()
            })
            .platform(Platform::Gcp)
            .service_provider(Arc::new(provider))
            .with_stack_resource(storage, ResourceLifecycle::Live)
            .build()
            .await
            .unwrap();
        assert!(
            run.needs_update().unwrap(),
            "referencing storage schedules queue update"
        );
        run.update(queue.clone()).unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert!(!run.needs_update().unwrap());
        run.step().await.unwrap(); // A refresh must not read or write IAM.
        assert_eq!(
            policy.lock().unwrap().bindings[0].members,
            [
                "serviceAccount:manual@example.com",
                "serviceAccount:service-123@gs-project-accounts.iam.gserviceaccount.com"
            ]
        );
        *run.desired_resource_config_mut("uploads").unwrap() =
            alien_core::Resource::new(alien_core::Storage::new("uploads".into()).build());
        assert!(
            run.needs_update().unwrap(),
            "removing the last rule schedules queue update"
        );
        run.update(queue).unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert!(!run.needs_update().unwrap());
        assert_eq!(
            serde_json::to_value(&*policy.lock().unwrap()).unwrap(),
            serde_json::to_value(original).unwrap()
        );
    }

    /// The grant reaches Google but its response is lost, and the Storage stops notifying
    /// before the retry. The recorded intent still lets removal revoke the grant.
    #[tokio::test]
    async fn grant_with_lost_response_is_revoked_when_notifications_are_removed() {
        let queue = Queue::new("work".into()).build();
        let storage = alien_core::Storage::new("uploads".into())
            .notifications(vec![alien_core::StorageNotification::builder()
                .queue((&queue).into())
                .build()])
            .build();
        let original = IamPolicy {
            version: Some(3),
            etag: Some("etag".into()),
            bindings: Vec::new(),
            ..Default::default()
        };
        let policy = Arc::new(std::sync::Mutex::new(original.clone()));
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut pubsub = MockPubSubApi::new();
        let reads = policy.clone();
        pubsub
            .expect_get_topic_iam_policy()
            .returning(move |_| Ok(reads.lock().unwrap().clone()));
        let (stored, attempts) = (policy.clone(), writes.clone());
        pubsub
            .expect_set_topic_iam_policy()
            .returning(move |_, value| {
                *stored.lock().unwrap() = value.clone();
                if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    return Err(AlienError::new(
                        alien_client_core::ErrorData::RemoteServiceUnavailable {
                            message: "response lost after the write".into(),
                        },
                    ));
                }
                Ok(value)
            });
        let mut gcs = alien_gcp_clients::gcs::MockGcsApi::new();
        gcs.expect_get_project_service_account()
            .times(1)
            .returning(|| {
                Ok(alien_gcp_clients::gcs::ProjectServiceAccount {
                    email_address: "service-123@gs-project-accounts.iam.gserviceaccount.com".into(),
                    kind: None,
                })
            });
        let (gcs, pubsub) = (Arc::new(gcs), Arc::new(pubsub));
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_pubsub_client()
            .returning(move |_| Ok(pubsub.clone()));
        provider
            .expect_get_gcp_gcs_client()
            .returning(move |_| Ok(gcs.clone()));
        let mut run = SingleControllerExecutor::builder()
            .resource(queue.clone())
            .controller(GcpQueueController {
                state: GcpQueueState::Ready,
                topic_name: Some("test-work".into()),
                subscription_name: Some("test-work-sub".into()),
                ..Default::default()
            })
            .platform(Platform::Gcp)
            .service_provider(Arc::new(provider))
            .with_stack_resource(storage, ResourceLifecycle::Live)
            .build()
            .await
            .unwrap();
        run.update(queue.clone()).unwrap();
        while writes.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            let _ = run.step().await;
        }
        assert_eq!(
            policy.lock().unwrap().bindings.len(),
            1,
            "the lost write still granted the service agent"
        );

        *run.desired_resource_config_mut("uploads").unwrap() =
            alien_core::Resource::new(alien_core::Storage::new("uploads".into()).build());
        run.run_until_terminal().await.unwrap();
        if run.needs_update().unwrap() {
            run.update(queue).unwrap();
            run.run_until_terminal().await.unwrap();
        }
        assert_eq!(run.status(), ResourceStatus::Running);
        assert!(!run.needs_update().unwrap());
        assert_eq!(
            serde_json::to_value(&*policy.lock().unwrap()).unwrap(),
            serde_json::to_value(original).unwrap(),
            "no service-agent grant may outlive the last notification"
        );
    }

    #[tokio::test]
    async fn ordinary_queue_and_frozen_import_never_request_ingress_updates() {
        for (notifying, lifecycle) in [
            (false, ResourceLifecycle::Live),
            (true, ResourceLifecycle::Frozen),
        ] {
            let queue = Queue::new("work".into()).build();
            let mut storage = alien_core::Storage::new("uploads".into()).build();
            if notifying {
                storage.notifications.push(
                    alien_core::StorageNotification::builder()
                        .queue((&queue).into())
                        .build(),
                );
            }
            let run = SingleControllerExecutor::builder()
                .resource(queue)
                .controller(GcpQueueController {
                    state: GcpQueueState::Ready,
                    topic_name: Some("test-work".into()),
                    subscription_name: Some("test-work-sub".into()),
                    ..Default::default()
                })
                .platform(Platform::Gcp)
                .resource_lifecycle(lifecycle)
                .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
                .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                .with_stack_resource(storage, lifecycle)
                .build()
                .await
                .unwrap();
            assert!(!run.needs_update().unwrap());
        }
    }

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

    fn create_gcp_iam_mock_for_resource_permissions() -> Arc<MockIamApi> {
        Arc::new(MockIamApi::new())
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

impl GcpQueueController {
    /// Record the intended membership, and for a grant the member itself, before the
    /// policy write. A grant whose response is lost is then still reconciled, including
    /// when notifications are removed before the retry.
    async fn prepare_storage_publisher(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let queue = ctx.desired_resource_config::<Queue>()?;
        if !may_manage_storage_publisher(ctx, &queue.id) {
            return Ok(());
        }
        let desired = queue_receives_storage_notifications(ctx.desired_stack, &queue.id);
        if self.storage_publisher_pending.is_none()
            && desired == self.storage_publisher_member.is_some()
        {
            return Ok(());
        }
        if desired && self.storage_publisher_member.is_none() {
            let account = ctx
                .service_provider
                .get_gcp_gcs_client(ctx.get_gcp_config()?)?
                .get_project_service_account()
                .await
                .context(ErrorData::CloudPlatformError {
                    resource_id: Some(queue.id.clone()),
                    message: "Read the Cloud Storage service agent for queue ingress".into(),
                })?;
            self.storage_publisher_member =
                Some(format!("serviceAccount:{}", account.email_address));
        }
        self.storage_publisher_pending = Some(desired);
        Ok(())
    }

    async fn reconcile_storage_publisher(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let queue = ctx.desired_resource_config::<Queue>()?;
        let Some(desired) = self.storage_publisher_pending else {
            return Ok(());
        };
        let Some(member) = self.storage_publisher_member.clone() else {
            // Nothing was ever recorded, so there is no grant to remove.
            self.storage_publisher_pending = None;
            return Ok(());
        };
        let topic = self.topic_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(queue.id.clone()),
                message: "Topic name missing during storage ingress reconciliation".into(),
            })
        })?;
        let client = ctx
            .service_provider
            .get_gcp_pubsub_client(ctx.get_gcp_config()?)?;
        let mut policy = client.get_topic_iam_policy(topic.clone()).await.context(
            ErrorData::CloudPlatformError {
                resource_id: Some(queue.id.clone()),
                message: format!("Read storage ingress policy for topic '{topic}'"),
            },
        )?;
        if set_storage_publisher(&mut policy, &member, desired) {
            client
                .set_topic_iam_policy(topic.clone(), policy)
                .await
                .context(ErrorData::CloudPlatformError {
                    resource_id: Some(queue.id.clone()),
                    message: format!("Update storage ingress policy for topic '{topic}'"),
                })?;
        }
        self.storage_publisher_member = desired.then_some(member);
        self.storage_publisher_pending = None;
        Ok(())
    }
}
