use super::*;
use super::{GcpWorkerHandlerAction as HandlerAction, GcpWorkerState::*};

impl GcpWorkerController {
    pub(super) async fn delete_start_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let _gcp_config = ctx.get_gcp_config()?;

        // Handle case where service_name is not set (e.g., creation failed early)
        let service_name = match self.service_name.as_ref() {
            Some(name) => name,
            None => {
                // No service was created, nothing to delete
                info!(resource_id=%ctx.desired_config.id(), "No Cloud Run service to delete - creation failed early");

                // Clear any remaining state and mark as deleted
                self.service_name = None;
                self.url = None;
                self.operation_name = None;
                self.push_subscriptions.clear();
                self.storage_notification_topics.clear();
                self.gcs_notification_ids.clear();
                self.scheduler_job_names.clear();

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        info!(name=%service_name, "Initiating Cloud Run service deletion");

        // If we have load balancer resources, delete them first
        // Otherwise, skip directly to deleting push subscriptions
        if self.forwarding_rule_name.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingForwardingRule,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: DeletingPushSubscriptions,
                suggested_delay: None,
            })
        }
    }

    // ─────────────── LB DELETION STATES ───────────────────────────

    pub(super) async fn deleting_forwarding_rule_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        let forwarding_rule_already_deleted =
            if let Some(forwarding_rule_name) = &self.forwarding_rule_name {
                info!(name=%forwarding_rule_name, "Deleting forwarding rule");

                match ctx
                    .service_provider
                    .get_gcp_compute_client(gcp_config)?
                    .delete_global_forwarding_rule(forwarding_rule_name.clone())
                    .await
                {
                    Ok(_) => {
                        info!(name=%forwarding_rule_name, "Forwarding rule deletion initiated");
                        false
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(name=%forwarding_rule_name, "Forwarding rule was already deleted");
                        true
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete forwarding rule '{}'",
                                forwarding_rule_name
                            ),
                            resource_id: None,
                        }));
                    }
                }
            } else {
                false
            };

        if forwarding_rule_already_deleted {
            self.forwarding_rule_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingTargetHttpsProxy,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    pub(super) async fn deleting_target_https_proxy_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(proxy_name) = &self.target_https_proxy_name {
            info!(name=%proxy_name, "Deleting target HTTPS proxy");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_target_https_proxy(proxy_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%proxy_name, "Target HTTPS proxy deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%proxy_name, "Target HTTPS proxy was already deleted");
                }
                Err(e) if is_gcp_resource_in_use(&e) => {
                    info!(
                        name=%proxy_name,
                        "Target HTTPS proxy is still referenced by another GCP resource; retrying deletion"
                    );
                    return Ok(HandlerAction::Stay {
                        max_times: Some(30),
                        suggested_delay: Some(Duration::from_secs(10)),
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete target HTTPS proxy '{}'", proxy_name),
                        resource_id: None,
                    }));
                }
            }
        }

        self.forwarding_rule_name = None;
        self.target_https_proxy_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingUrlMap,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    pub(super) async fn deleting_url_map_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(url_map_name) = &self.url_map_name {
            info!(name=%url_map_name, "Deleting URL map");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_url_map(url_map_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%url_map_name, "URL map deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%url_map_name, "URL map was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete URL map '{}'", url_map_name),
                        resource_id: None,
                    }));
                }
            }

            self.url_map_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingBackendService,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    pub(super) async fn deleting_backend_service_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(backend_service_name) = &self.backend_service_name {
            info!(name=%backend_service_name, "Deleting backend service");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_backend_service(backend_service_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%backend_service_name, "Backend service deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%backend_service_name, "Backend service was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete backend service '{}'",
                            backend_service_name
                        ),
                        resource_id: None,
                    }));
                }
            }

            self.backend_service_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingServerlessNeg,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    pub(super) async fn deleting_serverless_neg_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(neg_name) = &self.serverless_neg_name {
            info!(name=%neg_name, "Deleting serverless NEG");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_region_network_endpoint_group(gcp_config.region.clone(), neg_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%neg_name, "Serverless NEG deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%neg_name, "Serverless NEG was already deleted");
                }
                Err(e) if is_gcp_resource_in_use(&e) => {
                    info!(
                        name=%neg_name,
                        "Serverless NEG is still referenced by another GCP resource; retrying deletion"
                    );
                    return Ok(HandlerAction::Stay {
                        max_times: Some(30),
                        suggested_delay: Some(Duration::from_secs(10)),
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete serverless NEG '{}'", neg_name),
                        resource_id: None,
                    }));
                }
            }

            self.serverless_neg_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingSslCertificate,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    pub(super) async fn deleting_ssl_certificate_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(ssl_cert_name) = &self.ssl_certificate_name {
            info!(name=%ssl_cert_name, "Deleting SSL certificate");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_ssl_certificate(ssl_cert_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%ssl_cert_name, "SSL certificate deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%ssl_cert_name, "SSL certificate was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete SSL certificate '{}'", ssl_cert_name),
                        resource_id: None,
                    }));
                }
            }

            self.ssl_certificate_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingGlobalAddress,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    pub(super) async fn deleting_global_address_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

        if let Some(address_name) = &self.global_address_name {
            info!(name=%address_name, "Deleting global address");

            match ctx
                .service_provider
                .get_gcp_compute_client(gcp_config)?
                .delete_global_address(address_name.clone())
                .await
            {
                Ok(_) => {
                    info!(name=%address_name, "Global address deletion initiated");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%address_name, "Global address was already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete global address '{}'", address_name),
                        resource_id: None,
                    }));
                }
            }

            self.global_address_name = None;
            self.global_address_ip = None;
        }

        // Clear domain-related state
        self.fqdn = None;
        self.certificate_id = None;
        self.certificate_issued_at = None;
        self.uses_custom_domain = false;

        Ok(HandlerAction::Continue {
            state: DeletingPushSubscriptions,
            suggested_delay: None,
        })
    }

    // ─────────────── SERVICE DELETION STATES ──────────────────────

    pub(super) async fn deleting_push_subscriptions_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        info!(worker=%worker_config.id, subscriptions=?self.push_subscriptions, "Deleting push subscriptions");

        // Delete all push subscriptions using best-effort approach (ignore NotFound)
        self.delete_all_push_subscriptions(ctx, gcp_config).await?;

        // Delete GCS notifications (best-effort)
        self.delete_all_storage_notifications(ctx, gcp_config)
            .await?;

        // Delete storage notification topics (best-effort)
        self.delete_all_storage_notification_topics(ctx, gcp_config)
            .await?;

        // Continue to scheduler jobs cleanup
        Ok(HandlerAction::Continue {
            state: DeletingSchedulerJobs,
            suggested_delay: None,
        })
    }

    pub(super) async fn deleting_scheduler_jobs_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if self.scheduler_job_names.is_empty() {
            return Ok(HandlerAction::Continue {
                state: DeletingCommandsInfrastructure,
                suggested_delay: None,
            });
        }

        info!(worker=%worker_config.id, jobs=?self.scheduler_job_names, "Deleting Cloud Scheduler jobs");

        let scheduler_client = ctx
            .service_provider
            .get_gcp_cloud_scheduler_client(gcp_config)?;

        for job_name in &self.scheduler_job_names.clone() {
            match scheduler_client.delete_job(job_name.clone()).await {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        job=%job_name,
                        "Cloud Scheduler job deleted successfully"
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
                        job=%job_name,
                        "Cloud Scheduler job was already deleted (not found)"
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

        Ok(HandlerAction::Continue {
            state: DeletingCommandsInfrastructure,
            suggested_delay: None,
        })
    }

    pub(super) async fn deleting_commands_infrastructure_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let derived_topic_name = cfg
            .commands_enabled
            .then(|| {
                self.service_name
                    .as_ref()
                    .map(|service_name| format!("{service_name}-rq"))
            })
            .flatten();
        let derived_subscription_name = cfg
            .commands_enabled
            .then(|| {
                self.service_name
                    .as_ref()
                    .map(|service_name| format!("{service_name}-rq-sub"))
            })
            .flatten();

        // Delete commands subscription (best-effort)
        if let Some(subscription_name) = self
            .commands_subscription_name
            .take()
            .or(derived_subscription_name)
        {
            info!(subscription=%subscription_name, "Deleting commands push subscription");
            match pubsub_client
                .delete_subscription(subscription_name.clone())
                .await
            {
                Ok(_) => {
                    info!(subscription=%subscription_name, "Commands push subscription deleted");
                }
                Err(e) => {
                    warn!(
                        subscription=%subscription_name,
                        error=%e,
                        "Failed to delete commands push subscription (may already be deleted)"
                    );
                }
            }
        }

        // Delete commands topic (best-effort)
        if let Some(topic_name) = self.commands_topic_name.take().or(derived_topic_name) {
            info!(topic=%topic_name, "Deleting commands Pub/Sub topic");
            match pubsub_client.delete_topic(topic_name.clone()).await {
                Ok(_) => {
                    info!(topic=%topic_name, "Commands Pub/Sub topic deleted");
                }
                Err(e) => {
                    warn!(
                        topic=%topic_name,
                        error=%e,
                        "Failed to delete commands Pub/Sub topic (may already be deleted)"
                    );
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingService,
            suggested_delay: None,
        })
    }

    pub(super) async fn deleting_service_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        info!(name=%service_name, "Deleting Cloud Run service");

        // Try to delete the service, handling the case where it's already missing
        match ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .delete_service(gcp_config.region.clone(), service_name.clone(), None, None)
            .await
        {
            Ok(operation) => {
                // Service exists and deletion was initiated
                let operation_name = operation.name.ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceControllerConfigError {
                        resource_id: ctx.desired_config.id().to_string(),
                        message: "Cloud Run delete operation returned without name".to_string(),
                    })
                })?;

                info!(name=%service_name, operation=%operation_name, "Cloud Run service deletion initiated");

                self.operation_name = Some(operation_name);

                Ok(HandlerAction::Continue {
                    state: WaitingForDeleteOperation,
                    suggested_delay: Some(Duration::from_secs(2)),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                // Service is already missing - deletion goal achieved
                info!(name=%service_name, "Cloud Run service was already deleted");

                self.service_name = None;
                self.url = None;
                self.operation_name = None;
                self.push_subscriptions.clear();

                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                // Other error - propagate it
                Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete Cloud Run service".to_string(),
                    resource_id: Some(ctx.desired_config.id().to_string()),
                }))
            }
        }
    }

    pub(super) async fn waiting_for_delete_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let operation_name = self.operation_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Operation name not set in state".to_string(),
            })
        })?;

        let gcp_config = ctx.get_gcp_config()?;

        // Extract operation ID from the full operation name
        let operation_id = operation_name.split('/').last().unwrap_or(operation_name);

        debug!(operation=%operation_name, "Checking delete operation status");

        let operation = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_operation(gcp_config.region.clone(), operation_id.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run operation status".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        if operation.done.unwrap_or(false) {
            // Check if there was an error
            if let Some(OperationResult::Error { error }) = &operation.result {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Delete operation failed: {} (code: {})",
                        error.message, error.code
                    ),
                    resource_id: Some(ctx.desired_config.id().to_string()),
                }));
            }

            // Operation succeeded, now wait for the service to be gone
            info!(operation=%operation_name, "Delete operation completed successfully");

            Ok(HandlerAction::Continue {
                state: WaitingForServiceDeletion,
                suggested_delay: Some(Duration::from_secs(2)),
            })
        } else {
            // Operation still in progress
            debug!(operation=%operation_name, "Delete operation still in progress");
            Ok(HandlerAction::Stay {
                max_times: Some(20),
                suggested_delay: Some(Duration::from_secs(3)),
            })
        }
    }

    pub(super) async fn waiting_for_service_deletion_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Try to get the service - if it's gone, we're done
        match ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
        {
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(name=%service_name, "Cloud Run service successfully deleted");

                self.service_name = None;
                self.url = None;
                self.operation_name = None;
                self.push_subscriptions.clear();

                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check Cloud Run service deletion status".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })),
            Ok(_) => {
                debug!(name=%service_name, "Service still exists, waiting for deletion");
                Ok(HandlerAction::Stay {
                    max_times: Some(20),
                    suggested_delay: Some(Duration::from_secs(3)),
                })
            }
        }
    }

    // ─────────────── TERMINALS ────────────────────────────────
}
