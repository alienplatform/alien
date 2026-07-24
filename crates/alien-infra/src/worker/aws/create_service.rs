use super::*;
use super::{AwsWorkerHandlerAction as HandlerAction, AwsWorkerState::*};

impl AwsWorkerController {
    pub(super) async fn create_start_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let cfg = ctx.desired_resource_config::<Worker>()?;
        info!(name=%cfg.id, code=?cfg.code, "Initiating creation");

        // Product limitation: Only allow at most one queue trigger per worker
        let queue_trigger_count = cfg
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    cfg.id, queue_trigger_count
                ),
                resource_id: Some(cfg.id.clone()),
            }));
        }

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", cfg.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        // Get the ServiceAccount's role ARN
        let service_account_state = ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &service_account_ref,
            )?;

        let role_arn = service_account_state
            .role_arn
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: cfg.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        let image_uri = match &cfg.code {
            alien_core::WorkerCode::Image { image } => image.clone(),
            alien_core::WorkerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Worker is configured with source code, but only pre-built images are supported".to_string(),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        };

        // Resolve proxy URIs to native ECR URIs. Lambda can only pull from ECR.
        // The release stores proxy URIs; native_image_host carries the ECR prefix.
        let image_uri = if let Some(ref native_host) = ctx.deployment_config.native_image_host {
            alien_core::image_rewrite::resolve_native_image_uri(&image_uri, native_host)
                .unwrap_or(image_uri)
        } else {
            image_uri
        };

        // Lambda requires container images in the same region as the worker.
        // If the image URI points to ECR in a different region (e.g., the management
        // region), rewrite it to reference the local region where the replicated copy
        // lives. ECR private image replication must be configured separately.
        let image_uri = Self::rewrite_ecr_region_if_needed(&image_uri, &aws_cfg.region);

        let code = FunctionCode::builder().image_uri(image_uri).build();
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &cfg.id);
        let mut function_tags = standard_resource_tags(ctx.resource_prefix, &cfg.id);
        function_tags.insert("Name".to_string(), aws_worker_name.clone());

        if !cfg.public_endpoints.is_empty() {
            match Self::resolve_domain_info(ctx, &cfg.id)? {
                Some(domain_info) => {
                    self.fqdn = Some(domain_info.fqdn.clone());
                    self.certificate_id = domain_info.certificate_id;
                    self.certificate_arn = domain_info.certificate_arn;
                    self.uses_custom_domain = domain_info.uses_custom_domain;
                    self.domain_name = Some(domain_info.fqdn.clone());

                    // Check for URL override in deployment config, otherwise use domain FQDN
                    self.url = ctx
                        .deployment_config
                        .public_endpoints
                        .as_ref()
                        .and_then(|resources| resources.get(&cfg.id))
                        .and_then(|endpoints| endpoints.values().next().cloned())
                        .or_else(|| Some(format!("https://{}", domain_info.fqdn)));
                }
                None => {
                    // Standalone mode: no domain metadata available.
                    // Use API Gateway with its default endpoint URL (no custom domain).
                    // The URL will be set after API Gateway creation.
                    info!(
                        worker=%cfg.id,
                        "No domain metadata — will use API Gateway default endpoint (standalone mode)"
                    );
                }
            }
        }

        // Prepare environment variables
        let env_vars = self
            .prepare_environment_variables(&cfg.environment, &cfg.links, ctx, &aws_worker_name)
            .await?;

        let environment = if !env_vars.is_empty() {
            Some(Environment::builder().variables(env_vars).build())
        } else {
            None
        };

        // Get VPC configuration if a Network resource exists
        let vpc_config = self.get_vpc_config(ctx)?;
        if vpc_config.is_some() {
            info!(name=%aws_worker_name, "Configuring Lambda worker to run inside VPC");
        }

        let request = CreateFunctionRequest::builder()
            .function_name(aws_worker_name.clone())
            .role(role_arn)
            .code(code)
            .package_type("Image".to_string())
            .description(format!("Runtime worker: {}", cfg.id))
            .timeout(cfg.timeout_seconds as i32)
            .memory_size(cfg.memory_mb as i32)
            .publish(false)
            .tags(function_tags)
            .maybe_environment(environment)
            .architectures(vec!["arm64".to_string()])
            .maybe_vpc_config(vpc_config)
            .build();

        let response =
            client
                .create_function(request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to create Lambda worker".to_string(),
                    resource_id: Some(cfg.id.clone()),
                })?;

        self.arn = response.function_arn.clone();
        self.worker_name = Some(aws_worker_name.clone());
        info!(name=%aws_worker_name, arn=%self.arn.as_deref().unwrap_or("unknown"), "Worker created, waiting for active state");

        Ok(HandlerAction::Continue {
            state: CreateWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    pub(super) async fn create_wait_for_active_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);
        debug!(name=%aws_worker_name, "Checking worker state");

        let response = client
            .get_function_configuration(&aws_worker_name, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Lambda worker configuration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        // Check if worker is active based on state and last_update_status
        let is_active = response.state.as_deref() == Some("Active")
            && response.last_update_status.as_deref() == Some("Successful");

        if is_active {
            if !worker_config.public_endpoints.is_empty() {
                let has_domain_info = self.ensure_domain_info(ctx, &worker_config.id)?;
                let next_state = if has_domain_info {
                    // Platform mode: wait for certificate then create API Gateway + custom domain
                    WaitingForCertificate
                } else {
                    // Standalone mode: skip certificate/custom domain, use API Gateway default endpoint
                    CreatingApiGateway
                };
                Ok(HandlerAction::Continue {
                    state: next_state,
                    suggested_delay: Some(Duration::from_secs(2)),
                })
            } else {
                Ok(HandlerAction::Continue {
                    state: ApplyingResourcePermissions,
                    suggested_delay: Some(Duration::from_secs(2)),
                })
            }
        } else {
            debug!(
                name = %aws_worker_name,
                state = %response.state.as_deref().unwrap_or("unknown"),
                last_update_status = %response.last_update_status.as_deref().unwrap_or("unknown"),
                "Worker not yet active, retrying"
            );
            Ok(HandlerAction::Stay {
                max_times: Some(20),
                suggested_delay: Some(Duration::from_secs(3)),
            })
        }
    }

    pub(super) async fn waiting_for_certificate_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id));

        let status = metadata.map(|m| &m.certificate_status);
        if !self.ensure_domain_info(ctx, &worker_config.id)? {
            return Ok(HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }
        if self.uses_custom_domain && self.certificate_arn.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        match status {
            Some(CertificateStatus::Issued) => Ok(HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay: None,
            }),
            Some(CertificateStatus::Failed) => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "Certificate issuance failed".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
    }

    pub(super) async fn importing_certificate_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        self.ensure_domain_info(ctx, &worker_config.id)?;
        let resource = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for certificate import".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;

        // Certificate data is included in DeploymentConfig
        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let (leaf, chain) = split_certificate_chain(certificate_chain);

        let aws_cfg = ctx.get_aws_config()?;
        let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
        let tags = standard_resource_tags(ctx.resource_prefix, &worker_config.id)
            .into_iter()
            .map(|(key, value)| alien_aws_clients::acm::Tag { key, value })
            .collect();
        let response = acm_client
            .import_certificate(
                alien_aws_clients::acm::ImportCertificateRequest::builder()
                    .certificate(leaf)
                    .private_key(private_key.clone())
                    .maybe_certificate_chain(chain)
                    .tags(tags)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import certificate to ACM".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.certificate_arn = Some(response.certificate_arn.clone());

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        Ok(HandlerAction::Continue {
            state: CreatingApiGateway,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_api_gateway_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.api_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiIntegration,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let api_tags = standard_resource_tags(ctx.resource_prefix, &worker_config.id);

        let api = client
            .create_api(
                CreateApiRequest::builder()
                    .name(format!("{}-{}-api", ctx.resource_prefix, worker_config.id))
                    .protocol_type("HTTP".to_string())
                    .tags(api_tags)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API Gateway HTTP API".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let api_id = api.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "API Gateway ID not returned".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        self.api_id = Some(api_id);

        Ok(HandlerAction::Continue {
            state: CreatingApiIntegration,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    pub(super) async fn creating_api_integration_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.integration_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiRoute,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for integration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let function_arn = self.arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN missing for integration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let integration = client
            .create_integration(
                &api_id,
                CreateIntegrationRequest::builder()
                    .integration_type("AWS_PROXY".to_string())
                    .integration_uri(function_arn)
                    .payload_format_version("2.0".to_string())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API integration".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let integration_id = integration.integration_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Integration ID not returned".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        self.integration_id = Some(integration_id.clone());

        Ok(HandlerAction::Continue {
            state: CreatingApiRoute,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    pub(super) async fn creating_api_route_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.route_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiStage,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for route".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let integration_id = self.integration_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Integration ID missing for route".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let route = client
            .create_route(
                &api_id,
                CreateRouteRequest::builder()
                    .route_key("$default".to_string())
                    .target(format!("integrations/{}", integration_id))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API route".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.route_id = route.route_id.clone();

        Ok(HandlerAction::Continue {
            state: CreatingApiStage,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    pub(super) async fn creating_api_stage_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if self.stage_name.is_some() {
            if self.fqdn.is_some() {
                return Ok(HandlerAction::Continue {
                    state: CreatingApiDomain,
                    suggested_delay: Some(Duration::from_secs(1)),
                });
            }

            let aws_cfg = ctx.get_aws_config()?;
            let api_id = self.api_id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "API ID missing for default endpoint".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?;
            self.url = Some(format!(
                "https://{}.execute-api.{}.amazonaws.com",
                api_id, aws_cfg.region
            ));
            return Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for stage".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let stage = client
            .create_stage(
                &api_id,
                CreateStageRequest::builder()
                    .stage_name("$default".to_string())
                    .auto_deploy(true)
                    .tags(standard_resource_tags(
                        ctx.resource_prefix,
                        &worker_config.id,
                    ))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API stage".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.stage_name = stage.stage_name.clone().or(Some("$default".to_string()));

        if self.fqdn.is_some() {
            // Platform mode: proceed to custom domain setup
            Ok(HandlerAction::Continue {
                state: CreatingApiDomain,
                suggested_delay: Some(Duration::from_secs(1)),
            })
        } else {
            // Standalone mode: use the default API Gateway endpoint URL
            let aws_cfg = ctx.get_aws_config()?;
            let region = &aws_cfg.region;
            let default_url = format!("https://{}.execute-api.{}.amazonaws.com", api_id, region);
            info!(
                worker=%worker_config.id,
                url=%default_url,
                "Using API Gateway default endpoint (no custom domain)"
            );
            self.url = Some(default_url);
            Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            })
        }
    }

    pub(super) async fn creating_api_domain_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.load_balancer.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingApiMapping,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let fqdn = self.fqdn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "FQDN missing for API domain".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let cert_arn = self.certificate_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate ARN missing for API domain".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let domain = client
            .create_domain_name(
                CreateDomainNameRequest::builder()
                    .domain_name(fqdn.clone())
                    .domain_name_configurations(vec![DomainNameConfiguration::builder()
                        .certificate_arn(cert_arn)
                        .endpoint_type("REGIONAL".to_string())
                        .security_policy("TLS_1_2".to_string())
                        .build()])
                    .tags(standard_resource_tags(
                        ctx.resource_prefix,
                        &worker_config.id,
                    ))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API domain name".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        let endpoint = domain
            .domain_name_configurations
            .as_ref()
            .and_then(|configs| configs.first())
            .and_then(|config| {
                let dns_name = config.api_gateway_domain_name.clone()?;
                let hosted_zone_id = config.hosted_zone_id.clone()?;
                Some(LoadBalancerEndpoint {
                    dns_name,
                    hosted_zone_id,
                })
            });

        self.load_balancer = Some(LoadBalancerState { endpoint });

        Ok(HandlerAction::Continue {
            state: CreatingApiMapping,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    pub(super) async fn creating_api_mapping_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.api_mapping_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx
            .service_provider
            .get_aws_apigatewayv2_client(aws_cfg)
            .await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let domain_name = self.domain_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain name missing for mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?;

        let stage = self
            .stage_name
            .clone()
            .unwrap_or_else(|| "$default".to_string());

        let mapping = client
            .create_api_mapping(
                &domain_name,
                CreateApiMappingRequest::builder()
                    .api_id(api_id.clone())
                    .stage(stage)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API mapping".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.api_mapping_id = mapping.api_mapping_id.clone();

        Ok(HandlerAction::Continue {
            state: AddingApiGatewayPermission,
            suggested_delay: Some(Duration::from_secs(1)),
        })
    }

    pub(super) async fn adding_api_gateway_permission_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &worker_config.id);

        let request = AddPermissionRequest::builder()
            .statement_id("ApiGatewayInvoke".to_string())
            .action("lambda:InvokeFunction".to_string())
            .principal("apigateway.amazonaws.com".to_string())
            .build();

        client
            .add_permission(&aws_worker_name, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to add API Gateway permission".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        if self.fqdn.is_some() {
            if self.uses_custom_domain {
                // Custom domain: readiness probe then done
                Ok(HandlerAction::Continue {
                    state: RunningReadinessProbe,
                    suggested_delay: None,
                })
            } else {
                // Platform-managed domain: wait for DNS propagation
                Ok(HandlerAction::Continue {
                    state: WaitingForDns,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        } else {
            // Standalone mode: no custom domain, skip DNS and readiness probe
            Ok(HandlerAction::Continue {
                state: ApplyingResourcePermissions,
                suggested_delay: None,
            })
        }
    }

    pub(super) async fn waiting_for_dns_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id));

        let status = metadata.map(|m| &m.dns_status);

        match status {
            Some(DnsRecordStatus::Active) => Ok(HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay: None,
            }),
            Some(DnsRecordStatus::Failed) => {
                let fqdn = metadata.map(|m| m.fqdn.as_str()).unwrap_or("unknown");
                let detail = metadata
                    .and_then(|m| m.dns_error.as_deref())
                    .unwrap_or("unknown error");
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("DNS record creation failed for {fqdn}: {detail}"),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
    }

    pub(super) async fn running_readiness_probe_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        // Only run readiness probe if configured and we have a URL (for public workers)
        if worker_config.readiness_probe.is_some() && !worker_config.public_endpoints.is_empty() {
            if let Some(url) = &self.url {
                let dns_override = readiness_probe_dns_override(
                    url,
                    self.fqdn.as_deref(),
                    self.load_balancer.as_ref(),
                );

                match run_readiness_probe_with_dns_override(ctx, url, dns_override).await {
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
        }

        // Either no readiness probe needed, or probe succeeded - proceed to ApplyingResourcePermissions
        Ok(HandlerAction::Continue {
            state: ApplyingResourcePermissions,
            suggested_delay: None,
        })
    }
}
