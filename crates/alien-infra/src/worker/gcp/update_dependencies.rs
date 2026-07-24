use super::*;
use super::{GcpWorkerHandlerAction as HandlerAction, GcpWorkerState::*};

impl GcpWorkerController {
    pub(super) async fn update_push_subscriptions_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Check if triggers have changed
        let triggers_changed = current_config.triggers != previous_config.triggers;

        if triggers_changed {
            info!(worker=%current_config.id, "Worker triggers changed, updating push subscriptions");

            // Delete old subscriptions, storage notifications, topics, and scheduler jobs
            self.delete_all_push_subscriptions(ctx, gcp_config).await?;
            self.delete_all_storage_notifications(ctx, gcp_config)
                .await?;
            self.delete_all_storage_notification_topics(ctx, gcp_config)
                .await?;
            self.delete_all_scheduler_jobs(ctx, gcp_config).await?;

            // Recreate all trigger infrastructure
            for trigger in &current_config.triggers {
                match trigger {
                    alien_core::WorkerTrigger::Queue { queue } => {
                        self.create_push_subscription(
                            ctx,
                            gcp_config,
                            &service_name,
                            &current_config,
                            queue,
                        )
                        .await?;
                    }
                    alien_core::WorkerTrigger::Storage { storage, events } => {
                        self.create_storage_trigger(
                            ctx,
                            gcp_config,
                            &service_name,
                            &current_config,
                            storage,
                            events,
                        )
                        .await?;
                    }
                    alien_core::WorkerTrigger::Schedule { .. } => {
                        // Scheduler jobs are recreated below after all triggers
                    }
                }
            }

            // Recreate scheduler jobs
            let service_url = self.url.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: current_config.id.clone(),
                    message: "Service URL not available for scheduler job".to_string(),
                })
            })?;
            let service_account_email = self.get_service_account_email(ctx, &current_config)?;
            let scheduler_client = ctx
                .service_provider
                .get_gcp_cloud_scheduler_client(gcp_config)?;

            for (index, trigger) in current_config.triggers.iter().enumerate() {
                if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                    let job_id = format!(
                        "{}-{}-cron-{}",
                        ctx.resource_prefix, current_config.id, index
                    );
                    let job_full_name = format!(
                        "projects/{}/locations/{}/jobs/{}",
                        gcp_config.project_id, gcp_config.region, job_id
                    );

                    let job = SchedulerJob {
                        name: None,
                        description: Some(format!(
                            "Schedule trigger for worker '{}' (index {})",
                            current_config.id, index
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
                                worker=%current_config.id,
                                job=%job_id,
                                "Cloud Scheduler job already exists; treating as created"
                            );
                        }
                        Err(e) => {
                            return Err(e.context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to create Cloud Scheduler job '{}' for worker '{}'",
                                    job_id, current_config.id
                                ),
                                resource_id: Some(current_config.id.clone()),
                            }));
                        }
                    }

                    if !self.scheduler_job_names.contains(&job_full_name) {
                        self.scheduler_job_names.push(job_full_name);
                    }
                }
            }
        } else {
            info!(worker=%current_config.id, "No trigger changes detected");
        }

        Ok(HandlerAction::Continue {
            state: UpdateSettingIamPolicy,
            suggested_delay: None,
        })
    }

    pub(super) async fn update_setting_iam_policy_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.setting_iam_policy(ctx).await? {
            HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "setting_iam_policy",
                state,
            )),
            HandlerAction::Stay {
                max_times,
                suggested_delay,
            } => Ok(HandlerAction::Stay {
                max_times,
                suggested_delay,
            }),
        }
    }

    pub(super) async fn update_running_readiness_probe_impl(
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

    // ─────────────── DELETE FLOW ──────────────────────────────
}
