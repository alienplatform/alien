use super::*;
use super::{AwsWorkerHandlerAction as HandlerAction, AwsWorkerState::*};

impl AwsWorkerController {
    pub(super) async fn update_applying_resource_permissions_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.applying_resource_permissions(ctx).await? {
            HandlerAction::Continue {
                state: UpdatingEnvVarsWithSelfBinding,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateEnvVarsWithSelfBinding,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "applying_resource_permissions",
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

    pub(super) async fn update_env_vars_with_self_binding_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.updating_env_vars_with_self_binding(ctx).await? {
            HandlerAction::Continue {
                state: CreatingEventSourceMappings,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateEventSourceMappings,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "updating_env_vars_with_self_binding",
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

    pub(super) async fn update_event_source_mappings_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;

        // Validation: Only allow at most one queue trigger per worker (non-retriable error)
        let queue_trigger_count = current_config
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    current_config.id, queue_trigger_count
                ),
                resource_id: Some(current_config.id.clone()),
            }));
        }

        // Check if triggers have changed
        let triggers_changed = current_config.triggers != previous_config.triggers;

        if triggers_changed {
            info!(worker=%current_config.id, "Worker triggers changed, updating event source mappings");

            // For simplicity, we'll delete old mappings and create new ones
            // In a production system, you might want to do a more sophisticated diff
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

            // Delete existing mappings
            for uuid in &self.event_source_mappings.clone() {
                match client.delete_event_source_mapping(uuid).await {
                    Ok(_) => {
                        info!(worker=%current_config.id, uuid=%uuid, "Deleted existing event source mapping");
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(worker=%current_config.id, uuid=%uuid, "Event source mapping was already deleted");
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete existing event source mapping '{}'",
                                uuid
                            ),
                            resource_id: Some(current_config.id.clone()),
                        }));
                    }
                }
            }
            self.event_source_mappings.clear();

            // Create new mappings for ALL queue triggers
            for trigger in &current_config.triggers {
                if let alien_core::WorkerTrigger::Queue { queue } = trigger {
                    self.create_queue_event_source_mapping(ctx, aws_cfg, &current_config, queue)
                        .await?;
                }
            }

            // Clean up old S3 storage trigger notifications
            for trigger in &previous_config.triggers {
                if let alien_core::WorkerTrigger::Storage {
                    storage: storage_ref,
                    ..
                } = trigger
                {
                    if let Ok(storage_controller) =
                        ctx.require_dependency::<crate::storage::AwsStorageController>(storage_ref)
                    {
                        if let Some(bucket_name) = storage_controller.bucket_name.as_deref() {
                            let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                            let empty_config = NotificationConfiguration::default();
                            if let Err(e) = s3_client
                                .put_bucket_notification_configuration(bucket_name, &empty_config)
                                .await
                            {
                                warn!(
                                    worker=%current_config.id,
                                    bucket=%bucket_name,
                                    error=%e,
                                    "Failed to clear old S3 notification configuration (best-effort)"
                                );
                            }
                        }
                    }
                }
            }
            self.s3_permission_statement_ids.clear();

            // Clean up old EventBridge schedule triggers
            if !self.eventbridge_rule_names.is_empty() {
                let eventbridge_client = ctx
                    .service_provider
                    .get_aws_eventbridge_client(aws_cfg)
                    .await?;

                for rule_name in &self.eventbridge_rule_names.clone() {
                    if let Err(e) = eventbridge_client
                        .remove_targets(rule_name, vec!["1".to_string()])
                        .await
                    {
                        warn!(
                            worker=%current_config.id,
                            rule=%rule_name,
                            error=%e,
                            "Failed to remove targets from old EventBridge rule (best-effort)"
                        );
                    }
                    if let Err(e) = eventbridge_client.delete_rule(rule_name).await {
                        warn!(
                            worker=%current_config.id,
                            rule=%rule_name,
                            error=%e,
                            "Failed to delete old EventBridge rule (best-effort)"
                        );
                    }
                }
                self.eventbridge_rule_names.clear();
                self.eventbridge_permission_statement_ids.clear();
            }

            // Recreate storage triggers
            let worker_name = self.worker_name.as_deref().unwrap_or("unknown");
            let function_arn = self.arn.as_deref().unwrap_or("unknown");

            for trigger in &current_config.triggers {
                if let alien_core::WorkerTrigger::Storage {
                    storage: storage_ref,
                    events,
                } = trigger
                {
                    let storage_controller = ctx
                        .require_dependency::<crate::storage::AwsStorageController>(storage_ref)?;
                    let bucket_name =
                        storage_controller.bucket_name.as_deref().ok_or_else(|| {
                            AlienError::new(ErrorData::DependencyNotReady {
                                resource_id: current_config.id.clone(),
                                dependency_id: storage_ref.id.clone(),
                            })
                        })?;

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
                                worker=%current_config.id,
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
                                resource_id: Some(current_config.id.clone()),
                            }));
                        }
                    }

                    let s3_client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
                    let mut notification_config = s3_client
                        .get_bucket_notification_configuration(bucket_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to get notification configuration for bucket '{}'",
                                bucket_name
                            ),
                            resource_id: Some(current_config.id.clone()),
                        })?;

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
                            resource_id: Some(current_config.id.clone()),
                        })?;

                    if !self.s3_permission_statement_ids.contains(&statement_id) {
                        self.s3_permission_statement_ids.push(statement_id);
                    }
                }
            }

            // Recreate schedule triggers
            for (index, trigger) in current_config.triggers.iter().enumerate() {
                if let alien_core::WorkerTrigger::Schedule { cron } = trigger {
                    let rule_name = format!("{}-cron-{}", worker_name, index);
                    let schedule_expression =
                        crate::worker::crontab_to_eventbridge::crontab_to_eventbridge(cron)
                            .map_err(|e| {
                                AlienError::new(ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid cron expression '{}': {}", cron, e),
                                    resource_id: Some(current_config.id.clone()),
                                })
                            })?;

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
                                current_config.id
                            )),
                            tags: Some(eventbridge_tags(ctx.resource_prefix, &current_config.id)),
                        })
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to create EventBridge rule '{}'", rule_name),
                            resource_id: Some(current_config.id.clone()),
                        })?;

                    let rule_arn = rule_response.rule_arn.unwrap_or_default();
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
                                worker=%current_config.id,
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
                                resource_id: Some(current_config.id.clone()),
                            }));
                        }
                    }

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
                            resource_id: Some(current_config.id.clone()),
                        })?;

                    if !self.eventbridge_rule_names.contains(&rule_name) {
                        self.eventbridge_rule_names.push(rule_name);
                    }
                    if !self
                        .eventbridge_permission_statement_ids
                        .contains(&statement_id)
                    {
                        self.eventbridge_permission_statement_ids.push(statement_id);
                    }
                }
            }
        } else {
            info!(worker=%current_config.id, "No trigger changes detected");
        }

        Ok(HandlerAction::Continue {
            state: UpdatingConcurrency,
            suggested_delay: None,
        })
    }

    pub(super) async fn updating_concurrency_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Worker>()?;
        let prev_config = ctx.previous_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &config.id);

        if config.concurrency_limit != prev_config.concurrency_limit {
            match config.concurrency_limit {
                Some(limit) => {
                    info!(worker=%config.id, limit=%limit, "Updating reserved concurrency on worker");
                    client
                        .put_function_concurrency(&aws_worker_name, limit)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to update worker reserved concurrency".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
                None => {
                    info!(worker=%config.id, "Removing reserved concurrency from worker");
                    client
                        .delete_function_concurrency(&aws_worker_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to remove worker reserved concurrency".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
            }
        } else {
            debug!(worker=%config.id, "Concurrency limit unchanged, skipping");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
}
