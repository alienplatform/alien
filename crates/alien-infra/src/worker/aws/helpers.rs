use super::*;

impl AwsWorkerController {
    pub(super) fn should_wait_for_lambda_vpc_enis(ctx: &ResourceControllerContext<'_>) -> bool {
        matches!(
            ctx.deployment_config.stack_settings.network,
            Some(NetworkSettings::Create { .. })
        )
    }

    pub(super) fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<Option<DomainInfo>> {
        let stack_settings = &ctx.deployment_config.stack_settings;
        if let Some(custom) = stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id))
        {
            let cert_arn = custom
                .certificate
                .aws
                .as_ref()
                .map(|cert| cert.certificate_arn.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires an AWS certificate ARN".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(Some(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                certificate_arn: Some(cert_arn),
                uses_custom_domain: true,
            }));
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|metadata| metadata.resources.get(resource_id))
        else {
            return Ok(None);
        };

        Ok(Some(DomainInfo {
            fqdn: resource.fqdn.clone(),
            certificate_id: Some(resource.certificate_id.clone()),
            certificate_arn: None,
            uses_custom_domain: false,
        }))
    }

    pub(super) fn ensure_domain_info(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<bool> {
        if self.fqdn.is_some()
            && self.domain_name.is_some()
            && (self.certificate_id.is_some()
                || self.certificate_arn.is_some()
                || self.uses_custom_domain)
        {
            return Ok(true);
        }

        match Self::resolve_domain_info(ctx, resource_id)? {
            Some(domain_info) => {
                self.fqdn = Some(domain_info.fqdn.clone());
                self.domain_name = Some(domain_info.fqdn.clone());
                self.certificate_id = domain_info.certificate_id;
                self.certificate_arn = domain_info.certificate_arn;
                self.uses_custom_domain = domain_info.uses_custom_domain;
                if self.url.is_none() {
                    self.url = ctx
                        .deployment_config
                        .public_endpoints
                        .as_ref()
                        .and_then(|resources| resources.get(resource_id))
                        .and_then(|endpoints| endpoints.values().next().cloned())
                        .or_else(|| Some(format!("https://{}", domain_info.fqdn)));
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub(super) fn unexpected_update_wrapper_state(
        resource_id: &str,
        handler: &str,
        state: AwsWorkerState,
    ) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: resource_id.to_string(),
            message: format!("{handler} returned unexpected state during update: {state:?}"),
        })
    }
}

// Separate impl block for helper methods
impl AwsWorkerController {
    /// Rewrite an ECR image URI to use the given region if it points to a different one.
    ///
    /// Lambda requires container images in the same region as the worker.
    /// When the management account's ECR is in a different region and private
    /// image replication copies images to the target region, the image URI must
    /// reference the replicated copy.
    ///
    /// Only rewrites URIs matching the ECR format: `{account}.dkr.ecr.{region}.amazonaws.com/...`
    pub(super) fn rewrite_ecr_region_if_needed(image_uri: &str, target_region: &str) -> String {
        // ECR URI format: {account_id}.dkr.ecr.{region}.amazonaws.com/{repo}:{tag}
        let Some(host_end) = image_uri.find('/') else {
            return image_uri.to_string();
        };
        let host = &image_uri[..host_end];
        let parts: Vec<&str> = host.split('.').collect();
        // parts: [account_id, "dkr", "ecr", region, "amazonaws", "com"]
        if parts.len() >= 6
            && parts[1] == "dkr"
            && parts[2] == "ecr"
            && parts[4] == "amazonaws"
            && parts[3] != target_region
        {
            let new_host = format!("{}.dkr.ecr.{}.amazonaws.com", parts[0], target_region);
            format!("{}{}", new_host, &image_uri[host_end..])
        } else {
            image_uri.to_string()
        }
    }

    /// Creates an SQS event source mapping for a queue trigger
    pub(super) async fn create_queue_event_source_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        aws_cfg: &alien_aws_clients::AwsClientConfig,
        worker_config: &alien_core::Worker,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<()> {
        let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

        // Get queue controller to access outputs
        let queue_controller =
            ctx.require_dependency::<crate::queue::aws::AwsQueueController>(queue_ref)?;
        let queue_outputs_wrapper = queue_controller.get_outputs().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let queue_outputs = queue_outputs_wrapper
            .downcast_ref::<alien_core::QueueOutputs>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Invalid queue outputs type".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;

        // Extract queue name from the queue URL
        let queue_name = if let Some(url) = &queue_outputs.identifier {
            // SQS URL format: https://sqs.region.amazonaws.com/account-id/queue-name
            url.split('/')
                .last()
                .unwrap_or(&queue_outputs.queue_name)
                .to_string()
        } else {
            queue_outputs.queue_name.clone()
        };

        // Construct SQS queue ARN: arn:aws:sqs:region:account-id:queue-name
        let queue_arn = format!(
            "arn:aws:sqs:{}:{}:{}",
            aws_cfg.region, aws_cfg.account_id, queue_name
        );

        info!(
            worker=%worker_config.id,
            queue_arn=%queue_arn,
            "Creating SQS event source mapping"
        );

        let worker_name = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for event source mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let existing_mappings = lambda_client
            .list_event_source_mappings(ListEventSourceMappingsRequest {
                event_source_arn: Some(queue_arn.clone()),
                function_name: Some(worker_name.clone()),
                marker: None,
                max_items: None,
            })
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to list event source mappings for queue '{}'",
                    queue_name
                ),
                resource_id: Some(worker_config.id.clone()),
            })?;

        if let Some(existing_mapping) = existing_mappings
            .event_source_mappings
            .unwrap_or_default()
            .into_iter()
            .find(|mapping| {
                mapping.event_source_arn.as_deref() == Some(queue_arn.as_str())
                    && mapping.function_arn.as_deref() == Some(worker_name.as_str())
            })
        {
            if let Some(uuid) = existing_mapping.uuid {
                if !self.event_source_mappings.contains(&uuid) {
                    self.event_source_mappings.push(uuid.clone());
                }
                info!(
                    worker=%worker_config.id,
                    queue_arn=%queue_arn,
                    uuid=%uuid,
                    "SQS event source mapping already exists; treating as created"
                );
                return Ok(());
            }
        }

        let request = alien_aws_clients::lambda::CreateEventSourceMappingRequest::builder()
            .event_source_arn(queue_arn.clone())
            .function_name(worker_name.clone())
            .batch_size(1) // Always 1 message per invocation as per design
            .enabled(true)
            .build();

        let response = match lambda_client.create_event_source_mapping(request).await {
            Ok(response) => response,
            Err(e) if is_remote_resource_conflict(&e) => {
                let existing_mappings = lambda_client
                    .list_event_source_mappings(ListEventSourceMappingsRequest {
                        event_source_arn: Some(queue_arn.clone()),
                        function_name: Some(worker_name.clone()),
                        marker: None,
                        max_items: None,
                    })
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to list event source mappings for queue '{}' after conflict",
                            queue_name
                        ),
                        resource_id: Some(worker_config.id.clone()),
                    })?;

                existing_mappings
                    .event_source_mappings
                    .unwrap_or_default()
                    .into_iter()
                    .find(|mapping| {
                        mapping.event_source_arn.as_deref() == Some(queue_arn.as_str())
                            && mapping.function_arn.as_deref() == Some(worker_name.as_str())
                    })
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "Event source mapping for queue '{}' already exists but could not be found",
                                queue_name
                            ),
                            resource_id: Some(worker_config.id.clone()),
                        })
                    })?
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create event source mapping for queue '{}'",
                        queue_name
                    ),
                    resource_id: Some(worker_config.id.clone()),
                }));
            }
        };

        if let Some(uuid) = response.uuid {
            if !self.event_source_mappings.contains(&uuid) {
                self.event_source_mappings.push(uuid.clone());
            }
            info!(
                worker=%worker_config.id,
                queue_arn=%queue_arn,
                uuid=%uuid,
                "Successfully created SQS event source mapping"
            );
        }

        Ok(())
    }

    // ─────────────── HELPER METHODS ────────────────────────────

    /// Gets VPC configuration from the Network resource if one exists in the stack.
    ///
    /// If a Network resource exists (ID: "default-network"), this method retrieves
    /// the VPC ID, subnet IDs, and security group ID from the Network controller
    /// to configure the Lambda worker to run inside the VPC.
    ///
    /// Returns `None` if no Network resource exists in the stack.
    pub(super) fn get_vpc_config(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<VpcConfig>> {
        // Check if the stack has a Network resource
        let network_id = "default-network";
        if !ctx.desired_stack.resources.contains_key(network_id) {
            return Ok(None);
        }

        // Get the Network controller state via require_dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, network_id.to_string());
        let network_state =
            ctx.require_dependency::<crate::network::AwsNetworkController>(&network_ref)?;

        // Only configure VPC if we have subnet IDs and a security group
        // For Lambda, we use private subnets (no public IP assignment)
        if network_state.private_subnet_ids.is_empty() {
            return Ok(None);
        }

        let security_group_ids = match &network_state.security_group_id {
            Some(sg) => vec![sg.clone()],
            None => vec![],
        };

        if security_group_ids.is_empty() {
            return Ok(None);
        }

        Ok(Some(
            VpcConfig::builder()
                .subnet_ids(network_state.private_subnet_ids.clone())
                .security_group_ids(security_group_ids)
                .build(),
        ))
    }

    pub(super) async fn prepare_environment_variables(
        &self,
        initial_env: &HashMap<String, String>,
        links: &[ResourceRef],
        ctx: &ResourceControllerContext<'_>,
        worker_name_for_error_logging: &str,
    ) -> Result<HashMap<String, String>> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Get the worker's own binding params (may be None during initial creation)
        let self_binding_params = self.get_binding_params()?;

        let env_vars = EnvironmentVariableBuilder::try_new(initial_env)?
            .add_worker_runtime_env_vars(ctx, &worker_config.id, worker_config.timeout_seconds)?
            .add_linked_resources(links, ctx, worker_name_for_error_logging)
            .await?
            .add_self_worker_binding(&worker_config.id, self_binding_params.as_ref())?
            .build();

        Ok(env_vars)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(worker_name: &str) -> Self {
        Self {
            state: AwsWorkerState::Ready,
            arn: Some(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                worker_name
            )),
            url: Some(format!("https://abcd1234.lambda-url.us-east-1.on.aws/")),
            worker_name: Some(worker_name.to_string()),
            event_source_mappings: Vec::new(),
            fqdn: None,
            certificate_id: None,
            certificate_arn: None,
            api_id: None,
            integration_id: None,
            route_id: None,
            stage_name: None,
            api_mapping_id: None,
            domain_name: None,
            load_balancer: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            s3_permission_statement_ids: Vec::new(),
            eventbridge_rule_names: Vec::new(),
            eventbridge_permission_statement_ids: Vec::new(),
            _internal_stay_count: None,
        }
    }
}
