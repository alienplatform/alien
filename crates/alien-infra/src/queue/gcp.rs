// Stub module for future GCP Queue controller implementation.

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    GcpPubSubQueueHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, Queue, QueueHeartbeatData, QueueHeartbeatStatus, QueueOutputs,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError as _, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use alien_permissions::generators::GcpBindingResourceKind;
use chrono::Utc;
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use google_cloud_iam_v1::client::IAMPolicy;
use google_cloud_iam_v1::model::Policy;
use google_cloud_pubsub::{
    client::{SubscriptionAdmin, TopicAdmin},
    model::{Subscription, Topic},
};
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

fn pubsub_topic_resource_name(project_id: &str, topic_id: &str) -> String {
    if topic_id.starts_with("projects/") {
        topic_id.to_string()
    } else {
        format!("projects/{project_id}/topics/{topic_id}")
    }
}

fn pubsub_subscription_resource_name(project_id: &str, subscription_id: &str) -> String {
    if subscription_id.starts_with("projects/") {
        subscription_id.to_string()
    } else {
        format!("projects/{project_id}/subscriptions/{subscription_id}")
    }
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
        let topic_admin = ctx
            .service_provider
            .get_gcp_pubsub_topic_admin_client(cfg)
            .await?;
        let q = ctx.desired_resource_config::<Queue>()?;
        let topic = get_topic_name(ctx.resource_prefix, &q.id);

        // Create topic id without full path; client expects id
        create_pubsub_topic(&topic_admin, &cfg.project_id, &topic, Topic::default())
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
        let subscription_admin = ctx
            .service_provider
            .get_gcp_pubsub_subscription_admin_client(cfg)
            .await?;
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
            Subscription::new().set_topic(pubsub_topic_resource_name(&cfg.project_id, topic));

        create_pubsub_subscription(&subscription_admin, &cfg.project_id, &sub, subscription)
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
                let topic_name_owned = topic_name.clone();
                let iam_policy =
                    gcp_iam_policy_for_kind(&iam_bindings, GcpBindingResourceKind::PubsubTopic);
                if !iam_policy.bindings.is_empty() {
                    let iam_policy_client = ctx
                        .service_provider
                        .get_gcp_pubsub_iam_policy_client(gcp_config)
                        .await?;
                    let config_id_owned = config.id.clone();
                    set_pubsub_iam_policy(
                        &iam_policy_client,
                        pubsub_topic_resource_name(&gcp_config.project_id, &topic_name_owned),
                        iam_policy,
                    )
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
                let sub_name_owned = subscription_name.clone();
                let iam_policy = gcp_iam_policy_for_kind(
                    &iam_bindings,
                    GcpBindingResourceKind::PubsubSubscription,
                );
                if !iam_policy.bindings.is_empty() {
                    let iam_policy_client = ctx
                        .service_provider
                        .get_gcp_pubsub_iam_policy_client(gcp_config)
                        .await?;
                    let config_id_owned = config.id.clone();
                    set_pubsub_iam_policy(
                        &iam_policy_client,
                        pubsub_subscription_resource_name(&gcp_config.project_id, &sub_name_owned),
                        iam_policy,
                    )
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
        let topic_admin = ctx
            .service_provider
            .get_gcp_pubsub_topic_admin_client(cfg)
            .await?;
        let subscription_admin = ctx
            .service_provider
            .get_gcp_pubsub_subscription_admin_client(cfg)
            .await?;
        let q = ctx.desired_resource_config::<Queue>()?;
        let topic = self.topic_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Topic name not set in state".to_string(),
                resource_id: Some(q.id.clone()),
            })
        })?;
        let topic_metadata = get_pubsub_topic(&topic_admin, &cfg.project_id, topic)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get Pub/Sub topic '{}'", topic),
                resource_id: Some(q.id.clone()),
            })?;
        let subscription_metadata = if let Some(subscription) = &self.subscription_name {
            Some(
                get_pubsub_subscription(&subscription_admin, &cfg.project_id, subscription)
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
        let topic_admin = ctx
            .service_provider
            .get_gcp_pubsub_topic_admin_client(cfg)
            .await?;
        let subscription_admin = ctx
            .service_provider
            .get_gcp_pubsub_subscription_admin_client(cfg)
            .await?;
        let _ = ctx.desired_resource_config::<Queue>()?;

        if let Some(sub) = &self.subscription_name {
            match delete_pubsub_subscription(&subscription_admin, &cfg.project_id, sub).await {
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
            match delete_pubsub_topic(&topic_admin, &cfg.project_id, topic).await {
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

async fn create_pubsub_topic(
    client: &TopicAdmin,
    project_id: &str,
    topic_id: &str,
    topic: Topic,
) -> Result<Topic> {
    let resource_name = pubsub_topic_resource_name(project_id, topic_id);
    let mut topic = topic;
    if topic.name.is_empty() {
        topic.name = resource_name.clone();
    }

    match client.create_topic().with_request(topic).send().await {
        Ok(topic) => Ok(topic),
        Err(error) if gax_error_is_conflict(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceConflict {
                resource_type: "Pub/Sub topic".to_string(),
                resource_name,
                message: "create_topic reported the topic already exists".to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Pub/Sub create_topic request failed".to_string(),
                resource_id: Some(topic_id.to_string()),
            })),
    }
}

async fn get_pubsub_topic(client: &TopicAdmin, project_id: &str, topic_id: &str) -> Result<Topic> {
    let resource_name = pubsub_topic_resource_name(project_id, topic_id);
    match client
        .get_topic()
        .set_topic(resource_name.clone())
        .send()
        .await
    {
        Ok(topic) => Ok(topic),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "Pub/Sub topic".to_string(),
                resource_name,
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Pub/Sub get_topic request failed".to_string(),
                resource_id: Some(topic_id.to_string()),
            })),
    }
}

async fn delete_pubsub_topic(client: &TopicAdmin, project_id: &str, topic_id: &str) -> Result<()> {
    let resource_name = pubsub_topic_resource_name(project_id, topic_id);
    match client
        .delete_topic()
        .set_topic(resource_name.clone())
        .send()
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "Pub/Sub topic".to_string(),
                resource_name,
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Pub/Sub delete_topic request failed".to_string(),
                resource_id: Some(topic_id.to_string()),
            })),
    }
}

async fn create_pubsub_subscription(
    client: &SubscriptionAdmin,
    project_id: &str,
    subscription_id: &str,
    subscription: Subscription,
) -> Result<Subscription> {
    let resource_name = pubsub_subscription_resource_name(project_id, subscription_id);
    let mut subscription = subscription;
    if subscription.name.is_empty() {
        subscription.name = resource_name.clone();
    }

    match client
        .create_subscription()
        .with_request(subscription)
        .send()
        .await
    {
        Ok(subscription) => Ok(subscription),
        Err(error) if gax_error_is_conflict(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceConflict {
                resource_type: "Pub/Sub subscription".to_string(),
                resource_name,
                message: "create_subscription reported the subscription already exists".to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Pub/Sub create_subscription request failed".to_string(),
                resource_id: Some(subscription_id.to_string()),
            })),
    }
}

async fn get_pubsub_subscription(
    client: &SubscriptionAdmin,
    project_id: &str,
    subscription_id: &str,
) -> Result<Subscription> {
    let resource_name = pubsub_subscription_resource_name(project_id, subscription_id);
    match client
        .get_subscription()
        .set_subscription(resource_name.clone())
        .send()
        .await
    {
        Ok(subscription) => Ok(subscription),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "Pub/Sub subscription".to_string(),
                resource_name,
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Pub/Sub get_subscription request failed".to_string(),
                resource_id: Some(subscription_id.to_string()),
            })),
    }
}

async fn delete_pubsub_subscription(
    client: &SubscriptionAdmin,
    project_id: &str,
    subscription_id: &str,
) -> Result<()> {
    let resource_name = pubsub_subscription_resource_name(project_id, subscription_id);
    match client
        .delete_subscription()
        .set_subscription(resource_name.clone())
        .send()
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "Pub/Sub subscription".to_string(),
                resource_name,
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Pub/Sub delete_subscription request failed".to_string(),
                resource_id: Some(subscription_id.to_string()),
            })),
    }
}

async fn set_pubsub_iam_policy(
    client: &IAMPolicy,
    resource_name: String,
    policy: Policy,
) -> Result<Policy> {
    client
        .set_iam_policy()
        .set_resource(resource_name.clone())
        .set_policy(policy)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Pub/Sub set_iam_policy request failed".to_string(),
            resource_id: Some(resource_name),
        })
}

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::NOT_FOUND.as_u16())
}

fn gax_error_is_conflict(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::AlreadyExists)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::CONFLICT.as_u16())
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
    };
    use alien_core::{Platform, Queue, ResourceStatus};
    use google_cloud_gax::{options::RequestOptions, response::Response};
    use google_cloud_pubsub::{
        client::{SubscriptionAdmin, TopicAdmin},
        model::{
            DeleteSubscriptionRequest, DeleteTopicRequest, GetSubscriptionRequest, GetTopicRequest,
        },
        stub::{SubscriptionAdmin as SubscriptionAdminStub, TopicAdmin as TopicAdminStub},
    };
    use std::sync::Arc;

    mockall::mock! {
        #[derive(Debug)]
        TopicAdmin {}

        impl TopicAdminStub for TopicAdmin {
            async fn create_topic(
                &self,
                request: Topic,
                options: RequestOptions,
            ) -> google_cloud_pubsub::Result<Response<Topic>>;

            async fn get_topic(
                &self,
                request: GetTopicRequest,
                options: RequestOptions,
            ) -> google_cloud_pubsub::Result<Response<Topic>>;

            async fn delete_topic(
                &self,
                request: DeleteTopicRequest,
                options: RequestOptions,
            ) -> google_cloud_pubsub::Result<Response<()>>;
        }
    }

    mockall::mock! {
        #[derive(Debug)]
        SubscriptionAdmin {}

        impl SubscriptionAdminStub for SubscriptionAdmin {
            async fn create_subscription(
                &self,
                request: Subscription,
                options: RequestOptions,
            ) -> google_cloud_pubsub::Result<Response<Subscription>>;

            async fn get_subscription(
                &self,
                request: GetSubscriptionRequest,
                options: RequestOptions,
            ) -> google_cloud_pubsub::Result<Response<Subscription>>;

            async fn delete_subscription(
                &self,
                request: DeleteSubscriptionRequest,
                options: RequestOptions,
            ) -> google_cloud_pubsub::Result<Response<()>>;
        }
    }

    fn setup_mock_pubsub() -> (TopicAdmin, SubscriptionAdmin) {
        let mut topic_admin = MockTopicAdmin::new();
        topic_admin
            .expect_create_topic()
            .withf(|request, _| request.name == "projects/test-project/topics/test-gcp-queue")
            .once()
            .returning(|request, _| Ok(Response::from(request)));
        topic_admin
            .expect_get_topic()
            .withf(|request, _| request.topic == "projects/test-project/topics/test-gcp-queue")
            .once()
            .returning(|request, _| Ok(Response::from(Topic::new().set_name(request.topic))));
        topic_admin
            .expect_delete_topic()
            .withf(|request, _| request.topic == "projects/test-project/topics/test-gcp-queue")
            .once()
            .returning(|_, _| Ok(Response::from(())));

        let mut subscription_admin = MockSubscriptionAdmin::new();
        subscription_admin
            .expect_create_subscription()
            .withf(|request, _| {
                request.name == "projects/test-project/subscriptions/test-gcp-queue-sub"
                    && request.topic == "projects/test-project/topics/test-gcp-queue"
            })
            .once()
            .returning(|request, _| Ok(Response::from(request)));
        subscription_admin
            .expect_get_subscription()
            .withf(|request, _| {
                request.subscription == "projects/test-project/subscriptions/test-gcp-queue-sub"
            })
            .once()
            .returning(|request, _| {
                Ok(Response::from(
                    Subscription::new().set_name(request.subscription),
                ))
            });
        subscription_admin
            .expect_delete_subscription()
            .withf(|request, _| {
                request.subscription == "projects/test-project/subscriptions/test-gcp-queue-sub"
            })
            .once()
            .returning(|_, _| Ok(Response::from(())));

        (
            TopicAdmin::from_stub(topic_admin),
            SubscriptionAdmin::from_stub(subscription_admin),
        )
    }

    fn create_gcp_iam_mock_for_resource_permissions() -> Arc<MockGcpIamApi> {
        Arc::new(MockGcpIamApi::new())
    }

    fn setup_mock_provider(
        topic_admin: TopicAdmin,
        subscription_admin: SubscriptionAdmin,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_pubsub_topic_admin_client()
            .returning(move |_| Ok(topic_admin.clone()));
        provider
            .expect_get_gcp_pubsub_subscription_admin_client()
            .returning(move |_| Ok(subscription_admin.clone()));

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
        let (topic_admin, subscription_admin) = setup_mock_pubsub();
        let mock_provider = setup_mock_provider(topic_admin, subscription_admin);

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

        executor.step().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }
}
