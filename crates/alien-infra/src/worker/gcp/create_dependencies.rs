use super::*;
use super::{GcpWorkerHandlerAction as HandlerAction, GcpWorkerState::*};

impl GcpWorkerController {
    pub(super) async fn creating_push_subscriptions_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        info!(name=%service_name, "Creating Pub/Sub push subscriptions for queue triggers");

        // Create push subscriptions for queue triggers
        let mut created_any = false;
        for trigger in &cfg.triggers {
            if let alien_core::WorkerTrigger::Queue { queue } = trigger {
                info!(worker=%cfg.id, queue=%queue.id, "Creating Pub/Sub push subscription");
                self.create_push_subscription(ctx, gcp_config, &service_name, &cfg, queue)
                    .await?;
                created_any = true;
            }
        }

        if !created_any {
            info!(worker=%cfg.id, "No queue triggers found, skipping push subscription creation");
        }

        // Create push subscriptions for storage triggers
        for trigger in &cfg.triggers {
            if let alien_core::WorkerTrigger::Storage { storage, events } = trigger {
                info!(worker=%cfg.id, storage=%storage.id, "Creating storage trigger infrastructure");
                self.create_storage_trigger(ctx, gcp_config, &service_name, &cfg, storage, events)
                    .await?;
            }
        }

        // Go to scheduler jobs next
        Ok(HandlerAction::Continue {
            state: CreatingSchedulerJobs,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_scheduler_jobs_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;

        let schedule_triggers: Vec<(usize, &str)> = cfg
            .triggers
            .iter()
            .enumerate()
            .filter_map(|(i, trigger)| {
                if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                    Some((i, cron.as_str()))
                } else {
                    None
                }
            })
            .collect();

        if schedule_triggers.is_empty() {
            info!(worker=%cfg.id, "No schedule triggers found, skipping scheduler job creation");
            return Ok(HandlerAction::Continue {
                state: CreatingCommandsInfrastructure,
                suggested_delay: None,
            });
        }

        let scheduler_client = ctx
            .service_provider
            .get_gcp_cloud_scheduler_client(gcp_config)?;

        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: cfg.id.clone(),
                message: "Service URL not available for scheduler job".to_string(),
            })
        })?;

        // Get service account email for OIDC authentication
        let service_account_email = self.get_service_account_email(ctx, &cfg)?;

        for (index, cron) in &schedule_triggers {
            let job_id = format!("{}-{}-cron-{}", ctx.resource_prefix, cfg.id, index);
            let job_full_name = format!(
                "projects/{}/locations/{}/jobs/{}",
                gcp_config.project_id, gcp_config.region, job_id
            );

            info!(
                worker=%cfg.id,
                job=%job_id,
                cron=%cron,
                "Creating Cloud Scheduler job"
            );

            let job = SchedulerJob {
                name: None,
                description: Some(format!(
                    "Schedule trigger for worker '{}' (index {})",
                    cfg.id, index
                )),
                schedule: cron.to_string(),
                time_zone: Some("UTC".to_string()),
                http_target: Some(HttpTarget {
                    uri: service_url.clone(),
                    http_method: Some("POST".to_string()),
                    body: None,
                    headers: None,
                    oidc_token: Some(SchedulerOidcToken {
                        service_account_email: service_account_email.clone(),
                        audience: Some(service_url.clone()),
                    }),
                }),
                state: None,
            };

            match scheduler_client
                .create_job(gcp_config.region.clone(), job_id.clone(), job)
                .await
            {
                Ok(_) => {}
                Err(e) if is_remote_resource_conflict(&e) => {
                    info!(
                        worker=%cfg.id,
                        job=%job_id,
                        "Cloud Scheduler job already exists; treating as created"
                    );
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create Cloud Scheduler job '{}' for worker '{}'",
                            job_id, cfg.id
                        ),
                        resource_id: Some(cfg.id.clone()),
                    }));
                }
            }

            if !self.scheduler_job_names.contains(&job_full_name) {
                self.scheduler_job_names.push(job_full_name);
            }

            info!(
                worker=%cfg.id,
                job=%job_id,
                "Successfully created Cloud Scheduler job"
            );
        }

        Ok(HandlerAction::Continue {
            state: CreatingCommandsInfrastructure,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_commands_infrastructure_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;

        if !cfg.commands_enabled {
            debug!(worker=%cfg.id, "Commands not enabled, skipping commands infrastructure");
            return Ok(HandlerAction::Continue {
                state: SettingIamPolicy,
                suggested_delay: None,
            });
        }

        let gcp_config = ctx.get_gcp_config()?;
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;

        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: cfg.id.clone(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Create commands Pub/Sub topic
        let topic_short_name = format!("{}-rq", service_name);
        let topic_full_name = format!(
            "projects/{}/topics/{}",
            gcp_config.project_id, topic_short_name
        );

        if self.commands_topic_name.is_none() {
            info!(
                worker=%cfg.id,
                topic=%topic_full_name,
                "Creating commands Pub/Sub topic"
            );

            match pubsub_client
                .create_topic(topic_short_name.clone(), Topic::default())
                .await
            {
                Ok(_) => {
                    self.commands_topic_name = Some(topic_short_name.clone());
                }
                Err(e) if is_remote_resource_conflict(&e) => {
                    info!(
                        worker=%cfg.id,
                        topic=%topic_short_name,
                        "Commands Pub/Sub topic already exists, adopting it"
                    );
                    self.commands_topic_name = Some(topic_short_name.clone());
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create commands Pub/Sub topic '{}'",
                            topic_short_name
                        ),
                        resource_id: Some(cfg.id.clone()),
                    }));
                }
            }
        }

        // Create push subscription that delivers to the Cloud Run service
        let subscription_name = format!("{}-rq-sub", service_name);
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: cfg.id.clone(),
                message: "Service URL not available for commands push subscription".to_string(),
            })
        })?;

        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Only use OIDC authentication on the push subscription when the worker
        // is private. Public workers have invoker_iam_disabled=true on the Cloud
        // Run service, so PubSub can deliver without authentication. Using OIDC on
        // public workers would require the PubSub service agent to have
        // roles/iam.serviceAccountTokenCreator on the execution SA, which adds
        // unnecessary complexity.
        let oidc_token = if cfg.public_endpoints.is_empty() {
            let service_account_id = format!("{}-sa", cfg.get_permissions());
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
                        resource_id: cfg.id().to_string(),
                        dependency_id: service_account_id.to_string(),
                    })
                })?
                .to_string();

            Some(OidcToken {
                service_account_email,
                audience: Some(push_endpoint.clone()),
            })
        } else {
            None
        };

        let push_config = PushConfig {
            push_endpoint: Some(push_endpoint.clone()),
            attributes: Some(std::collections::HashMap::new()),
            oidc_token,
            pubsub_wrapper: None,
            no_wrapper: None,
        };

        let subscription = Subscription {
            name: Some(subscription_name.clone()),
            topic: Some(topic_full_name.clone()),
            push_config: Some(push_config),
            ack_deadline_seconds: Some(cfg.timeout_seconds as i32),
            retain_acked_messages: Some(false),
            message_retention_duration: None,
            labels: Some(std::collections::HashMap::from([
                ("commands".to_string(), cfg.id.clone()),
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

        if self.commands_subscription_name.is_none() {
            info!(
                worker=%cfg.id,
                topic=%topic_full_name,
                subscription=%subscription_name,
                endpoint=%push_endpoint,
                "Creating commands Pub/Sub push subscription"
            );

            match pubsub_client
                .create_subscription(subscription_name.clone(), subscription)
                .await
            {
                Ok(_) => {
                    self.commands_subscription_name = Some(subscription_name.clone());
                }
                Err(e) if is_remote_resource_conflict(&e) => {
                    info!(
                        worker=%cfg.id,
                        subscription=%subscription_name,
                        "Commands Pub/Sub push subscription already exists, adopting it"
                    );
                    self.commands_subscription_name = Some(subscription_name.clone());
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create commands push subscription '{}'",
                            subscription_name
                        ),
                        resource_id: Some(cfg.id.clone()),
                    }));
                }
            }
        }

        self.apply_command_topic_management_permissions(ctx, &topic_short_name)
            .await?;

        info!(worker=%cfg.id, "Commands Pub/Sub infrastructure created");

        Ok(HandlerAction::Continue {
            state: SettingIamPolicy,
            suggested_delay: None,
        })
    }

    pub(super) async fn setting_iam_policy_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let _cfg = ctx.desired_resource_config::<Worker>()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        info!(name=%service_name, "Setting IAM policy for Cloud Run service");

        // Apply resource-scoped IAM bindings only. Public access is handled via
        // invoker_iam_disabled on the service (set during creation), not via allUsers
        // IAM binding. This avoids issues with domain-restricted sharing org policies.
        self.apply_consolidated_iam_policy(ctx, service_name, false)
            .await?;

        // Always go to readiness probe next (linear flow - may be no-op)
        Ok(HandlerAction::Continue {
            state: RunningReadinessProbe,
            suggested_delay: None,
        })
    }

    pub(super) async fn running_readiness_probe_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;

        // Only run readiness probe if configured and we have a URL
        if cfg.readiness_probe.is_some() {
            if let Some(url) = self.url.as_ref() {
                match run_readiness_probe(ctx, url).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: Some(READINESS_PROBE_MAX_ATTEMPTS),
                            suggested_delay: Some(Duration::from_secs(5)),
                        });
                    }
                }
            }
        } else {
            debug!(name=%ctx.desired_config.id(), "No readiness probe configured, proceeding to Ready");
        }

        // Either no readiness probe needed, or probe succeeded - proceed to Ready
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    pub(super) async fn ready_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_cloudrun_client(gcp_config)?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Heartbeat check: verify service still exists and is in correct state
        let service = client
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run service during heartbeat check".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // Verify service is ready. A service can be temporarily not-Ready due to
        // scaling events, GCP maintenance, or revision transitions. Use a retryable
        // error to allow recovery instead of immediately failing the deployment.
        // Cloud Run v2 may not return a "Ready" condition, so also accept both
        // sub-conditions as Succeeded.
        let has_condition_succeeded =
            |name: &str| -> bool {
                service.conditions.iter().any(|c| {
                    c.r#type.as_deref() == Some(name)
                        && c.state.as_ref().map(|s| {
                            s == &alien_gcp_clients::cloudrun::ConditionState::ConditionSucceeded
                        }).unwrap_or(false)
                })
            };

        let is_ready = has_condition_succeeded("Ready")
            || (has_condition_succeeded("RoutesReady")
                && has_condition_succeeded("ConfigurationsReady"));

        if !is_ready {
            warn!(name=%worker_config.id, "Cloud Run service is not in Ready state during heartbeat");
            let mut err = AlienError::new(ErrorData::ResourceDrift {
                resource_id: worker_config.id.clone(),
                message: "Cloud Run service is not in Ready state".to_string(),
            });
            err.retryable = true;
            return Err(err);
        }

        // Check for basic configuration drift - compare memory limits
        if let Some(template) = &service.template {
            if let Some(container) = template.containers.first() {
                if let Some(resources) = &container.resources {
                    if let Some(limits) = &resources.limits {
                        if let Some(current_memory) = limits.get("memory") {
                            let expected_memory = format!("{}Mi", worker_config.memory_mb);
                            if current_memory != &expected_memory {
                                return Err(AlienError::new(ErrorData::ResourceDrift {
                                    resource_id: worker_config.id.clone(),
                                    message: format!(
                                        "Service memory configuration has drifted. Expected: {}, but found: {}",
                                        expected_memory, current_memory
                                    ),
                                }));
                            }
                        }
                    }
                }
            }
        }

        // Check for certificate renewal on auto-managed public domains.
        if !worker_config.public_endpoints.is_empty() && !self.uses_custom_domain {
            let metadata = ctx
                .deployment_config
                .domain_metadata
                .as_ref()
                .and_then(|meta| meta.resources.get(&worker_config.id));

            if let Some(resource) = metadata {
                // Check if certificate has been renewed (issued_at timestamp changed)
                if let Some(new_issued_at) = &resource.issued_at {
                    if self.certificate_issued_at.as_ref() != Some(new_issued_at) {
                        info!(
                            worker=%worker_config.id,
                            old_issued_at=?self.certificate_issued_at,
                            new_issued_at=%new_issued_at,
                            "Certificate renewed, triggering update to re-import certificate"
                        );
                        return Ok(HandlerAction::Continue {
                            state: UpdateImportingSslCertificate,
                            suggested_delay: None,
                        });
                    }
                }
            }
        }

        emit_gcp_cloud_run_worker_heartbeat(ctx, &worker_config, service_name, &service);

        debug!(name = %worker_config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)), // Check again in 30 seconds
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
}
