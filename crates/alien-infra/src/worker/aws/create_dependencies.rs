use super::*;
use super::{AwsWorkerHandlerAction as HandlerAction, AwsWorkerState::*};

impl AwsWorkerController {
    pub(super) async fn applying_resource_permissions_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;

        info!(worker=%config.id, "Applying resource-scoped permissions for Lambda worker");

        // Apply resource-scoped permissions from the stack using the centralized helper.
        // This handles wildcard ("*") permissions and management SA permissions.
        if let Some(worker_name) = &self
            .arn
            .as_ref()
            .and_then(|arn| arn.split(':').last().map(|s| s.to_string()))
        {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                &worker_name,
                "worker",
            )
            .await?;
        }

        info!(worker=%config.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: UpdatingEnvVarsWithSelfBinding,
            suggested_delay: None,
        })
    }

    pub(super) async fn updating_env_vars_with_self_binding_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;

        // Skip this step if the worker doesn't have public ingress
        // For private workers, the initial env vars already have complete self-binding
        // (no URL to add later)
        if config.public_endpoints.is_empty() {
            info!(worker=%config.id, "Skipping env var update - no public URL to add");
            return Ok(HandlerAction::Continue {
                state: CreatingEventSourceMappings,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &config.id);

        // Now that we have the URL, update the environment variables
        // with the complete self-binding information including the URL
        let final_env_vars = self
            .prepare_environment_variables(
                &config.environment,
                &config.links,
                ctx,
                &aws_worker_name,
            )
            .await?;

        let lambda_environment = if !final_env_vars.is_empty() {
            Some(Environment::builder().variables(final_env_vars).build())
        } else {
            None
        };

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", config.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &service_account_ref,
            )?;
        let role_arn = service_account_state
            .role_arn
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        // Get VPC configuration if a Network resource exists
        let vpc_config = self.get_vpc_config(ctx)?;

        let request = UpdateFunctionConfigurationRequest::builder()
            .role(role_arn)
            .timeout(config.timeout_seconds as i32)
            .memory_size(config.memory_mb as i32)
            .maybe_environment(lambda_environment)
            .maybe_vpc_config(vpc_config)
            .build();

        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for env var update".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        client
            .update_function_configuration(arn, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Lambda worker configuration with self-binding"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(worker=%config.id, "Successfully updated environment variables with complete self-binding");

        Ok(HandlerAction::Continue {
            state: CreatingEventSourceMappings,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    pub(super) async fn creating_event_source_mappings_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        // Validation: Only allow at most one queue trigger per worker (non-retriable error)
        let queue_trigger_count = config
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    config.id, queue_trigger_count
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        // Linear flow principle: Always perform this state. Create mappings for ALL queue triggers
        let mut created_any = false;
        for trigger in &config.triggers {
            if let alien_core::WorkerTrigger::Queue { queue } = trigger {
                info!(worker=%config.id, queue=%queue.id, "Creating SQS event source mapping");
                self.create_queue_event_source_mapping(ctx, aws_cfg, &config, queue)
                    .await?;
                created_any = true;
            }
        }
        if !created_any {
            info!(worker=%config.id, "No queue triggers found, skipping event source mapping creation");
        }

        // Handle storage triggers: configure S3 bucket notifications to invoke this Lambda
        let worker_name = self.worker_name.as_deref().unwrap_or("unknown");
        let function_arn = self.arn.as_deref().unwrap_or("unknown");

        for trigger in &config.triggers {
            if let alien_core::WorkerTrigger::Storage {
                storage: storage_ref,
                events,
            } = trigger
            {
                info!(worker=%config.id, storage=%storage_ref.id, "Configuring S3 storage trigger");

                // Get storage controller to access bucket name
                let storage_controller =
                    ctx.require_dependency::<crate::storage::AwsStorageController>(storage_ref)?;
                let bucket_name = storage_controller.bucket_name.as_deref().ok_or_else(|| {
                    AlienError::new(ErrorData::DependencyNotReady {
                        resource_id: config.id.clone(),
                        dependency_id: storage_ref.id.clone(),
                    })
                })?;

                // Add Lambda permission for S3 to invoke this worker
                let statement_id = format!("{}-s3-{}", worker_name, storage_ref.id);
                let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                let permission_request = AddPermissionRequest::builder()
                    .statement_id(statement_id.clone())
                    .action("lambda:InvokeFunction".to_string())
                    .principal("s3.amazonaws.com".to_string())
                    .source_arn(format!("arn:aws:s3:::{}", bucket_name))
                    .build();

                match lambda_client
                    .add_permission(worker_name, permission_request)
                    .await
                {
                    Ok(_) => {}
                    Err(e) if is_remote_resource_conflict(&e) => {
                        info!(
                            worker=%config.id,
                            statement_id=%statement_id,
                            "S3 invoke permission already exists; treating as created"
                        );
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to add S3 invoke permission for storage '{}'",
                                storage_ref.id
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }

                // Get current notification config and merge in new Lambda config
                let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                let mut notification_config = s3_client
                    .get_bucket_notification_configuration(bucket_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get notification configuration for bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                // Map alien event types to S3 event types
                let s3_events: Vec<String> = events
                    .iter()
                    .map(|e| match e.as_str() {
                        "created" => "s3:ObjectCreated:*".to_string(),
                        "deleted" => "s3:ObjectRemoved:*".to_string(),
                        other => format!("s3:{}", other),
                    })
                    .collect();

                replace_lambda_notification_config(
                    &mut notification_config,
                    LambdaFunctionConfiguration {
                        id: Some(statement_id.clone()),
                        lambda_function_arn: function_arn.to_string(),
                        events: s3_events,
                        filter: None,
                    },
                );

                s3_client
                    .put_bucket_notification_configuration(bucket_name, &notification_config)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to put notification configuration for bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                if !self.s3_permission_statement_ids.contains(&statement_id) {
                    self.s3_permission_statement_ids.push(statement_id.clone());
                }
                info!(
                    worker=%config.id,
                    storage=%storage_ref.id,
                    bucket=%bucket_name,
                    statement_id=%statement_id,
                    "S3 storage trigger configured"
                );
            }
        }

        // Continue to schedule trigger creation (linear flow)
        Ok(HandlerAction::Continue {
            state: CreatingScheduleTriggers,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_schedule_triggers_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        let worker_name = self.worker_name.as_deref().unwrap_or("unknown");
        let function_arn = self.arn.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for schedule trigger".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        for (index, trigger) in config.triggers.iter().enumerate() {
            if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                info!(worker=%config.id, cron=%cron, index=%index, "Creating EventBridge schedule trigger");

                let rule_name = format!("{}-cron-{}", worker_name, index);

                // Convert standard 5-field cron to EventBridge format
                let schedule_expression =
                    crate::worker::crontab_to_eventbridge::crontab_to_eventbridge(cron).map_err(
                        |e| {
                            AlienError::new(ErrorData::ResourceConfigInvalid {
                                message: format!("Invalid cron expression '{}': {}", cron, e),
                                resource_id: Some(config.id.clone()),
                            })
                        },
                    )?;

                // Create EventBridge rule
                let eventbridge_client = ctx
                    .service_provider
                    .get_aws_eventbridge_client(aws_cfg)
                    .await?;

                let rule_response = eventbridge_client
                    .put_rule(PutRuleRequest {
                        name: rule_name.clone(),
                        schedule_expression,
                        state: Some("ENABLED".to_string()),
                        description: Some(format!(
                            "Alien schedule trigger for worker '{}'",
                            config.id
                        )),
                        tags: Some(eventbridge_tags(ctx.resource_prefix, &config.id)),
                    })
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create EventBridge rule '{}'", rule_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                let rule_arn = rule_response.rule_arn.unwrap_or_default();

                // Add Lambda permission for EventBridge
                let statement_id = format!("{}-eb-{}", worker_name, index);
                let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
                let permission_request = AddPermissionRequest::builder()
                    .statement_id(statement_id.clone())
                    .action("lambda:InvokeFunction".to_string())
                    .principal("events.amazonaws.com".to_string())
                    .source_arn(rule_arn)
                    .build();

                match lambda_client
                    .add_permission(worker_name, permission_request)
                    .await
                {
                    Ok(_) => {}
                    Err(e) if is_remote_resource_conflict(&e) => {
                        info!(
                            worker=%config.id,
                            statement_id=%statement_id,
                            "EventBridge invoke permission already exists; treating as created"
                        );
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to add EventBridge invoke permission for rule '{}'",
                                rule_name
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }

                // Add Lambda as the target of the rule
                eventbridge_client
                    .put_targets(PutTargetsRequest {
                        rule: rule_name.clone(),
                        targets: vec![EventBridgeTarget {
                            id: "1".to_string(),
                            arn: function_arn.to_string(),
                        }],
                    })
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to add target to EventBridge rule '{}'",
                            rule_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                if !self.eventbridge_rule_names.contains(&rule_name) {
                    self.eventbridge_rule_names.push(rule_name.clone());
                }
                if !self
                    .eventbridge_permission_statement_ids
                    .contains(&statement_id)
                {
                    self.eventbridge_permission_statement_ids
                        .push(statement_id.clone());
                }
                info!(
                    worker=%config.id,
                    rule_name=%rule_name,
                    statement_id=%statement_id,
                    "EventBridge schedule trigger created"
                );
            }
        }

        // Continue to concurrency configuration (linear flow)
        Ok(HandlerAction::Continue {
            state: SettingConcurrency,
            suggested_delay: None,
        })
    }

    pub(super) async fn setting_concurrency_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &config.id);

        if let Some(limit) = config.concurrency_limit {
            info!(worker=%config.id, limit=%limit, "Setting reserved concurrency on worker");
            client
                .put_function_concurrency(&aws_worker_name, limit)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to set worker reserved concurrency".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
        } else {
            debug!(worker=%config.id, "No concurrency limit configured, skipping");
        }

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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);

        // Heartbeat check: verify worker still exists and is in correct state
        let function_info = client
            .get_function_configuration(&aws_worker_name, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get worker configuration during heartbeat check".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // Verify worker is in active state - drift is non-retryable
        if function_info.state.as_deref() != Some("Active") {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: worker_config.id.clone(),
                message: format!(
                    "Worker state is '{}', expected 'Active'",
                    function_info.state.as_deref().unwrap_or("unknown")
                ),
            }));
        }

        // Check if certificate was renewed (for public workers with auto-managed domains)
        if !worker_config.public_endpoints.is_empty() {
            if let Some(domain_metadata) = &ctx.deployment_config.domain_metadata {
                if let Some(resource_info) = domain_metadata.resources.get(&worker_config.id) {
                    if let Some(new_issued_at) = &resource_info.issued_at {
                        match &self.certificate_issued_at {
                            Some(stored) if new_issued_at != stored => {
                                // Certificate renewed! Trigger update flow to re-import
                                info!(
                                    name = %worker_config.id,
                                    old_issued_at = %stored,
                                    new_issued_at = %new_issued_at,
                                    "Certificate renewed, triggering update"
                                );
                                return Ok(HandlerAction::Continue {
                                    state: UpdateImportingCertificate,
                                    suggested_delay: None,
                                });
                            }
                            None => {
                                // First heartbeat after deployment, store the timestamp
                                self.certificate_issued_at = Some(new_issued_at.clone());
                            }
                            _ => {} // Same timestamp, no renewal
                        }
                    }
                }
            }
        }

        emit_aws_lambda_worker_heartbeat(ctx, &worker_config, &aws_worker_name, &function_info);

        debug!(name = %worker_config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
}
