use super::*;
use alien_gcp_clients::gcs::GcsNotification;
use alien_gcp_clients::iam::Binding;
use alien_gcp_clients::pubsub::{OidcToken, PushConfig, Subscription, Topic};

impl GcpWorkerController {
    /// Creates storage trigger infrastructure: Pub/Sub topic, GCS notification, and push subscription.
    pub(super) async fn create_storage_trigger(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
        _service_name: &str,
        worker_config: &alien_core::Worker,
        storage_ref: &alien_core::ResourceRef,
        events: &[String],
    ) -> Result<()> {
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let gcs_client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Get bucket name from the storage controller dependency
        let storage_controller =
            ctx.require_dependency::<crate::storage::GcpStorageController>(storage_ref)?;
        let bucket_name = storage_controller.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: storage_ref.id.clone(),
            })
        })?;

        // 1. Create a dedicated Pub/Sub topic for this storage notification
        let topic_short_name = format!(
            "{}-{}-{}-notif",
            ctx.resource_prefix, worker_config.id, storage_ref.id
        );
        let topic_full_name = format!(
            "projects/{}/topics/{}",
            gcp_config.project_id, topic_short_name
        );

        info!(
            worker=%worker_config.id,
            storage=%storage_ref.id,
            topic=%topic_full_name,
            "Creating Pub/Sub topic for storage notifications"
        );

        match pubsub_client
            .create_topic(topic_short_name.clone(), Topic::default())
            .await
        {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%worker_config.id,
                    topic=%topic_short_name,
                    "Storage notification topic already exists; treating as created"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create storage notification topic '{}'",
                        topic_short_name
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        if !self.storage_notification_topics.contains(&topic_short_name) {
            self.storage_notification_topics
                .push(topic_short_name.clone());
        }

        // 2. Ask Cloud Storage for its managed service account before granting it
        //    publish permissions. Deriving the email from the project number does
        //    not ensure that the service account has been provisioned yet.
        let gcs_project_service_account = gcs_client.get_project_service_account().await.context(
            ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get the Cloud Storage service account for project '{}'",
                    gcp_config.project_id
                ),
                resource_id: Some(worker_config.id.clone()),
            },
        )?;
        let gcs_service_agent = format!(
            "serviceAccount:{}",
            gcs_project_service_account.email_address
        );

        let iam_policy = alien_gcp_clients::iam::IamPolicy::builder()
            .version(1)
            .bindings(vec![Binding {
                role: "roles/pubsub.publisher".to_string(),
                members: vec![gcs_service_agent],
                condition: None,
            }])
            .build();

        pubsub_client
            .set_topic_iam_policy(topic_short_name.clone(), iam_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to set IAM policy on storage notification topic '{}'",
                    topic_short_name
                ),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // 3. Create GCS notification on the bucket pointing to the topic
        let gcs_event_types: Vec<String> = events
            .iter()
            .map(|event| {
                match event.as_str() {
                    "created" => "OBJECT_FINALIZE".to_string(),
                    "deleted" => "OBJECT_DELETE".to_string(),
                    "archived" => "OBJECT_ARCHIVE".to_string(),
                    "metadataUpdated" => "OBJECT_METADATA_UPDATE".to_string(),
                    other => other.to_string(), // Pass through unknown events as-is
                }
            })
            .collect();

        let notification = GcsNotification {
            id: None,
            topic: Some(topic_full_name.clone()),
            event_types: gcs_event_types,
            payload_format: Some("JSON_API_V1".to_string()),
            object_name_prefix: None,
            custom_attributes: std::collections::HashMap::new(),
        };

        let existing_notification = gcs_client
            .list_notifications(bucket_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to list GCS notifications on bucket '{}' for worker '{}'",
                    bucket_name, worker_config.id
                ),
                resource_id: Some(worker_config.id.clone()),
            })?
            .items
            .into_iter()
            .find(|existing| gcs_notification_matches_existing(existing, &notification));

        let created_notification = if let Some(existing_notification) = existing_notification {
            info!(
                worker=%worker_config.id,
                storage=%storage_ref.id,
                bucket=%bucket_name,
                notification_id=?existing_notification.id,
                "GCS notification already exists; treating as created"
            );
            existing_notification
        } else {
            gcs_client
                .insert_notification(bucket_name.clone(), notification)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create GCS notification on bucket '{}' for worker '{}'",
                        bucket_name, worker_config.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                })?
        };

        if let Some(notification_id) = &created_notification.id {
            if !self.gcs_notification_ids.iter().any(|tracker| {
                tracker.bucket_name == *bucket_name && tracker.notification_id == *notification_id
            }) {
                self.gcs_notification_ids.push(GcsNotificationTracker {
                    bucket_name: bucket_name.clone(),
                    notification_id: notification_id.clone(),
                });
            }
        }

        // 4. Create a push subscription to the Cloud Run URL
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service URL not available for storage trigger push subscription"
                    .to_string(),
            })
        })?;

        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Get service account email for OIDC authentication
        let service_account_email = self.get_service_account_email(ctx, worker_config)?;

        let oidc_token = OidcToken {
            service_account_email,
            audience: Some(push_endpoint.clone()),
        };

        let subscription_name = format!(
            "{}-{}-{}-notif-sub",
            ctx.resource_prefix, worker_config.id, storage_ref.id
        );

        let push_config = PushConfig {
            push_endpoint: Some(push_endpoint),
            attributes: Some(std::collections::HashMap::new()),
            oidc_token: Some(oidc_token),
            pubsub_wrapper: None,
            no_wrapper: None,
        };

        let subscription = Subscription {
            name: Some(subscription_name.clone()),
            topic: Some(topic_full_name.clone()),
            push_config: Some(push_config),
            ack_deadline_seconds: Some(worker_config.timeout_seconds as i32),
            retain_acked_messages: Some(false),
            message_retention_duration: None,
            labels: Some(std::collections::HashMap::from([
                ("worker".to_string(), worker_config.id.clone()),
                ("deployment".to_string(), ctx.resource_prefix.to_string()),
                ("storage".to_string(), storage_ref.id.clone()),
            ])),
            enable_message_ordering: Some(false),
            expiration_policy: None,
            filter: None,
            dead_letter_policy: None,
            retry_policy: None,
            detached: Some(false),
            state: None,
            analytics_hub_subscription_info: None,
            bigquery_config: None,
            cloud_storage_config: None,
        };

        info!(
            worker=%worker_config.id,
            storage=%storage_ref.id,
            subscription=%subscription_name,
            "Creating Pub/Sub push subscription for storage trigger"
        );

        match pubsub_client
            .create_subscription(subscription_name.clone(), subscription)
            .await
        {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%worker_config.id,
                    subscription=%subscription_name,
                    "Storage trigger push subscription already exists; treating as created"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create push subscription '{}' for storage trigger '{}'",
                        subscription_name, storage_ref.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        if !self.push_subscriptions.contains(&subscription_name) {
            self.push_subscriptions.push(subscription_name);
        }

        info!(
            worker=%worker_config.id,
            storage=%storage_ref.id,
            "Successfully created storage trigger infrastructure"
        );

        Ok(())
    }

    /// Deletes all GCS notifications (best-effort)
    pub(super) async fn delete_all_storage_notifications(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
    ) -> Result<()> {
        if self.gcs_notification_ids.is_empty() {
            return Ok(());
        }

        let gcs_client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for tracker in &self.gcs_notification_ids.clone() {
            match gcs_client
                .delete_notification(tracker.bucket_name.clone(), tracker.notification_id.clone())
                .await
            {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        bucket=%tracker.bucket_name,
                        notification_id=%tracker.notification_id,
                        "GCS notification deleted successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        bucket=%tracker.bucket_name,
                        notification_id=%tracker.notification_id,
                        error=%e,
                        "Failed to delete GCS notification (best-effort, continuing)"
                    );
                }
            }
        }

        self.gcs_notification_ids.clear();
        Ok(())
    }

    /// Deletes all storage notification Pub/Sub topics (best-effort)
    pub(super) async fn delete_all_storage_notification_topics(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
    ) -> Result<()> {
        if self.storage_notification_topics.is_empty() {
            return Ok(());
        }

        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for topic_name in &self.storage_notification_topics.clone() {
            match pubsub_client.delete_topic(topic_name.clone()).await {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        topic=%topic_name,
                        "Storage notification topic deleted successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        topic=%topic_name,
                        error=%e,
                        "Failed to delete storage notification topic (best-effort, continuing)"
                    );
                }
            }
        }

        self.storage_notification_topics.clear();
        Ok(())
    }

    /// Deletes all Cloud Scheduler jobs (best-effort)
    pub(super) async fn delete_all_scheduler_jobs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
    ) -> Result<()> {
        if self.scheduler_job_names.is_empty() {
            return Ok(());
        }

        let scheduler_client = ctx
            .service_provider
            .get_gcp_cloud_scheduler_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for job_name in &self.scheduler_job_names.clone() {
            match scheduler_client.delete_job(job_name.clone()).await {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        job=%job_name,
                        "Cloud Scheduler job deleted successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        job=%job_name,
                        error=%e,
                        "Failed to delete Cloud Scheduler job (best-effort, continuing)"
                    );
                }
            }
        }

        self.scheduler_job_names.clear();
        Ok(())
    }
}
