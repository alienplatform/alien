use super::*;
use super::{AwsWorkerHandlerAction as HandlerAction, AwsWorkerState::*};

impl AwsWorkerController {
    pub(super) async fn delete_start_impl(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.url = None;
        Ok(HandlerAction::Continue {
            state: DeletingApiGateway,
            suggested_delay: None,
        })
    }

    pub(super) async fn deleting_api_gateway_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Ordering matters: delete API mapping before domain name, domain name before API.
        if let (Some(domain_name), Some(api_mapping_id)) =
            (self.domain_name.as_ref(), self.api_mapping_id.as_ref())
        {
            let client = ctx
                .service_provider
                .get_aws_apigatewayv2_client(aws_cfg)
                .await?;
            match client.delete_api_mapping(domain_name, api_mapping_id).await {
                Ok(()) => info!(worker=%worker_config.id, "API mapping deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(worker=%worker_config.id, "API mapping already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete API mapping".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }
        self.api_mapping_id = None;

        if let Some(domain_name) = self.domain_name.as_ref() {
            let client = ctx
                .service_provider
                .get_aws_apigatewayv2_client(aws_cfg)
                .await?;
            match client.delete_domain_name(domain_name).await {
                Ok(()) => {
                    info!(worker=%worker_config.id, domain=%domain_name, "Custom domain deleted")
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(worker=%worker_config.id, "Custom domain already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete custom domain".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }
        self.domain_name = None;

        // Deleting the API cascades to routes, integrations, and stages.
        if let Some(api_id) = self.api_id.as_ref() {
            let client = ctx
                .service_provider
                .get_aws_apigatewayv2_client(aws_cfg)
                .await?;
            match client.delete_api(api_id).await {
                Ok(()) => {
                    info!(worker=%worker_config.id, api_id=%api_id, "API Gateway deleted")
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(worker=%worker_config.id, "API Gateway already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete API Gateway".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }
        self.api_id = None;
        self.integration_id = None;
        self.route_id = None;
        self.stage_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingEventSourceMappings,
            suggested_delay: None,
        })
    }

    pub(super) async fn deleting_event_source_mappings_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Linear flow principle: Always perform this state, even if no event source mappings
        if !self.event_source_mappings.is_empty() {
            info!(worker=%worker_config.id, mappings=?self.event_source_mappings, "Deleting event source mappings");

            // Delete all event source mappings using best-effort approach (ignore NotFound)
            for uuid in &self.event_source_mappings.clone() {
                match client.delete_event_source_mapping(uuid).await {
                    Ok(_) => {
                        info!(worker=%worker_config.id, uuid=%uuid, "Event source mapping deleted successfully");
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(worker=%worker_config.id, uuid=%uuid, "Event source mapping was already deleted (not found)");
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete event source mapping '{}'", uuid),
                            resource_id: Some(worker_config.id.clone()),
                        }));
                    }
                }
            }

            // Clear the mapping list after successful deletion
            self.event_source_mappings.clear();
        } else {
            info!(worker=%worker_config.id, "No event source mappings to delete");
        }

        // Clean up S3 storage trigger notifications (best-effort)
        if !self.s3_permission_statement_ids.is_empty() {
            info!(worker=%worker_config.id, "Cleaning up S3 storage trigger notifications");

            // Best-effort: put empty notification configuration on any referenced buckets
            // We don't track which bucket each statement_id maps to, so we attempt to
            // clean up by iterating over storage triggers from the config
            for trigger in &worker_config.triggers {
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
                                    worker=%worker_config.id,
                                    bucket=%bucket_name,
                                    error=%e,
                                    "Failed to clear S3 notification configuration (best-effort)"
                                );
                            } else {
                                info!(
                                    worker=%worker_config.id,
                                    bucket=%bucket_name,
                                    "S3 notification configuration cleared"
                                );
                            }
                        }
                    }
                }
            }
            self.s3_permission_statement_ids.clear();
        }

        // Always continue to DeletingScheduleTriggers state (linear flow)
        Ok(HandlerAction::Continue {
            state: DeletingScheduleTriggers,
            suggested_delay: None,
        })
    }

    pub(super) async fn deleting_schedule_triggers_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_cfg = ctx.get_aws_config()?;

        // Delete EventBridge rules and their targets (best-effort)
        if !self.eventbridge_rule_names.is_empty() {
            info!(
                worker=%worker_config.id,
                rules=?self.eventbridge_rule_names,
                "Deleting EventBridge schedule triggers"
            );

            let eventbridge_client = ctx
                .service_provider
                .get_aws_eventbridge_client(aws_cfg)
                .await?;

            for rule_name in &self.eventbridge_rule_names.clone() {
                // Remove targets first (required before deleting rule)
                if let Err(e) = eventbridge_client
                    .remove_targets(rule_name, vec!["1".to_string()])
                    .await
                {
                    warn!(
                        worker=%worker_config.id,
                        rule=%rule_name,
                        error=%e,
                        "Failed to remove targets from EventBridge rule (best-effort)"
                    );
                } else {
                    info!(worker=%worker_config.id, rule=%rule_name, "EventBridge rule targets removed");
                }

                // Delete the rule
                if let Err(e) = eventbridge_client.delete_rule(rule_name).await {
                    warn!(
                        worker=%worker_config.id,
                        rule=%rule_name,
                        error=%e,
                        "Failed to delete EventBridge rule (best-effort)"
                    );
                } else {
                    info!(worker=%worker_config.id, rule=%rule_name, "EventBridge rule deleted");
                }
            }
            self.eventbridge_rule_names.clear();
        }

        // Clear EventBridge permission statement IDs
        // (Lambda permissions are removed when the worker is deleted)
        self.eventbridge_permission_statement_ids.clear();

        // Detach the Lambda from the VPC before deleting it. AWS otherwise
        // keeps Lambda-managed ENIs around after function deletion, which can
        // block Terraform/CloudFormation from deleting customer-owned subnets.
        Ok(HandlerAction::Continue {
            state: DetachingVpcConfig,
            suggested_delay: None,
        })
    }

    pub(super) async fn detaching_vpc_config_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);

        if self.get_vpc_config(ctx)?.is_none() {
            return Ok(HandlerAction::Continue {
                state: DeletingWorker,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let function_identifier = self.arn.as_deref().unwrap_or(&aws_worker_name);
        let request = UpdateFunctionConfigurationRequest::builder()
            .vpc_config(
                VpcConfig::builder()
                    .subnet_ids(Vec::new())
                    .security_group_ids(Vec::new())
                    .build(),
            )
            .build();

        match client
            .update_function_configuration(function_identifier, request)
            .await
        {
            Ok(_) => {
                info!(worker=%worker_config.id, "Lambda VPC config detach requested");
                Ok(HandlerAction::Continue {
                    state: DetachVpcWaitForActive,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(worker=%worker_config.id, "Lambda already gone while detaching VPC config");
                Ok(HandlerAction::Continue {
                    state: DeletingWorker,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to detach Lambda worker from VPC".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    pub(super) async fn detach_vpc_wait_for_active_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let function_identifier = self.arn.as_deref().unwrap_or(&aws_worker_name);
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

        match client
            .get_function_configuration(function_identifier, None)
            .await
        {
            Ok(result)
                if result.state.as_deref() == Some("Active")
                    && result.last_update_status.as_deref() == Some("Successful") =>
            {
                Ok(HandlerAction::Continue {
                    state: DeletingWorker,
                    suggested_delay: None,
                })
            }
            Ok(result)
                if result.state.as_deref() == Some("Pending")
                    || result.last_update_status.as_deref() == Some("InProgress") =>
            {
                Ok(HandlerAction::Stay {
                    max_times: Some(60),
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
            Ok(result) => Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Lambda VPC detach failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status
                ),
                resource_id: Some(worker_config.id.clone()),
            })),
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(HandlerAction::Continue {
                    state: DeletingWorker,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check Lambda VPC detach status".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    pub(super) async fn deleting_function_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        info!(name=%aws_worker_name, "Deleting worker itself: {}", aws_worker_name);

        match client.delete_function(&aws_worker_name, None).await {
            Ok(_) => {
                info!(name=%aws_worker_name, "Worker deleted successfully, proceeding to DeleteWaitForNotFound state");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                warn!(name=%aws_worker_name, "Worker was already deleted (not found), proceeding to DeleteWaitForNotFound state");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete Lambda worker".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: DeleteWaitForNotFound,
            suggested_delay: None,
        })
    }

    pub(super) async fn delete_wait_for_not_found_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let arn = self.arn.as_ref();
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let lookup_identifier = arn.map(|a| a.as_str()).unwrap_or(&aws_worker_name);

        match client
            .get_function_configuration(lookup_identifier, None)
            .await
        {
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                self.arn = None;
                self.url = None;
                self.worker_name = None;
                self.event_source_mappings.clear();
                if Self::should_wait_for_lambda_vpc_enis(ctx) {
                    Ok(HandlerAction::Continue {
                        state: WaitingForVpcEnisReleased,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                } else {
                    Ok(HandlerAction::Continue {
                        state: DeletingCertificate,
                        suggested_delay: None,
                    })
                }
            }
            Ok(_) => Ok(HandlerAction::Stay {
                max_times: Some(10),
                suggested_delay: Some(Duration::from_secs(10)),
            }),
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check worker deletion status".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    pub(super) async fn waiting_for_vpc_enis_released_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !Self::should_wait_for_lambda_vpc_enis(ctx) {
            info!(
                worker=%worker_config.id,
                "Skipping Lambda VPC network interface wait for externally managed network"
            );
            return Ok(HandlerAction::Continue {
                state: DeletingCertificate,
                suggested_delay: None,
            });
        }

        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        let result = client
            .describe_network_interfaces(
                DescribeNetworkInterfacesRequest::builder()
                    .filters(vec![Filter {
                        name: "description".to_string(),
                        values: vec![format!("AWS Lambda VPC ENI-{}*", aws_worker_name)],
                    }])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to check Lambda VPC network interface cleanup".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let network_interfaces = result
            .network_interface_set
            .map(|set| set.items)
            .unwrap_or_default();

        if network_interfaces.is_empty() {
            return Ok(HandlerAction::Continue {
                state: DeletingCertificate,
                suggested_delay: None,
            });
        }

        let network_interface_ids = network_interfaces
            .iter()
            .filter_map(|eni| eni.network_interface_id.as_deref())
            .collect::<Vec<_>>();

        info!(
            worker=%worker_config.id,
            network_interfaces=?network_interface_ids,
            "Waiting for Lambda VPC network interfaces to be released"
        );

        Ok(HandlerAction::Stay {
            max_times: Some(90),
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    pub(super) async fn deleting_certificate_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if let Some(certificate_arn) = self.certificate_arn.as_ref() {
            let aws_cfg = ctx.get_aws_config()?;
            let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
            match acm_client.delete_certificate(certificate_arn).await {
                Ok(()) => info!(worker=%worker_config.id, "ACM certificate deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(worker=%worker_config.id, "ACM certificate already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete ACM certificate".to_string(),
                        resource_id: Some(worker_config.id.clone()),
                    }));
                }
            }
        }
        self.certificate_arn = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
}
