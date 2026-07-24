use super::*;
use alien_gcp_clients::pubsub::{OidcToken, PushConfig, Subscription};

impl GcpWorkerController {
    pub(super) async fn create_push_subscription(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
        _service_name: &str,
        worker_config: &alien_core::Worker,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<()> {
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;

        // Get queue controller to access the topic name
        let queue_controller =
            ctx.require_dependency::<crate::queue::gcp::GcpQueueController>(queue_ref)?;
        let topic_name = queue_controller.topic_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let topic_full_name = format!("projects/{}/topics/{}", gcp_config.project_id, topic_name);

        // Generate push subscription name: stack-prefix-worker-id-queue-id
        let subscription_name = format!(
            "{}-{}-{}",
            ctx.resource_prefix, worker_config.id, queue_ref.id
        );

        // Get the service URL for push endpoint
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service URL not available for push subscription".to_string(),
            })
        })?;

        // Build push endpoint URL (Cloud Run service URL)
        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Get service account email for OIDC authentication
        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let service_account_state = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &service_account_ref,
            )?;
        let service_account_email = service_account_state
            .service_account_email
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        // Create push config with OIDC authentication
        let oidc_token = OidcToken {
            service_account_email: service_account_email.clone(),
            audience: Some(push_endpoint.clone()),
        };

        let push_config = PushConfig {
            push_endpoint: Some(push_endpoint.clone()),
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
            topic=%topic_full_name,
            subscription=%subscription_name,
            endpoint=%push_endpoint,
            "Creating Pub/Sub push subscription"
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
                    "Pub/Sub push subscription already exists; treating as created"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create push subscription '{}' for queue '{}'",
                        subscription_name, queue_ref.id
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        if !self.push_subscriptions.contains(&subscription_name) {
            self.push_subscriptions.push(subscription_name.clone());
        }

        info!(
            worker=%worker_config.id,
            subscription=%subscription_name,
            "Successfully created Pub/Sub push subscription"
        );

        Ok(())
    }

    /// Deletes all push subscriptions using best-effort approach
    pub(super) async fn delete_all_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
    ) -> Result<()> {
        if self.push_subscriptions.is_empty() {
            return Ok(());
        }

        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        for subscription_name in &self.push_subscriptions.clone() {
            match pubsub_client
                .delete_subscription(subscription_name.clone())
                .await
            {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        subscription=%subscription_name,
                        "Push subscription deleted successfully"
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        worker=%worker_config.id,
                        subscription=%subscription_name,
                        "Push subscription was already deleted (not found)"
                    );
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete push subscription '{}'",
                            subscription_name
                        ),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }

        self.push_subscriptions.clear();
        Ok(())
    }

    /// Gets the service account email for the worker's permission profile.
    pub(super) fn get_service_account_email(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker_config: &alien_core::Worker,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", worker_config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let service_account_state = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &service_account_ref,
            )?;

        service_account_state.service_account_email.ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id().to_string(),
                dependency_id: service_account_id,
            })
        })
    }
}
