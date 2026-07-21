use std::{collections::HashMap, time::Duration};
use tracing::{debug, info, warn};

use crate::core::EnvironmentVariableBuilder;

use crate::core::split_certificate_chain;
use crate::core::ResourceController;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::worker::readiness_probe::{
    run_readiness_probe_with_dns_override, READINESS_PROBE_MAX_ATTEMPTS,
};
use alien_aws_clients::apigatewayv2::{
    CreateApiMappingRequest, CreateApiRequest, CreateDomainNameRequest, CreateIntegrationRequest,
    CreateRouteRequest, CreateStageRequest, DomainNameConfiguration,
};
use alien_aws_clients::ec2::{DescribeNetworkInterfacesRequest, Filter};
use alien_aws_clients::eventbridge::{EventBridgeTarget, PutRuleRequest, PutTargetsRequest};
use alien_aws_clients::lambda::{
    AddPermissionRequest, CreateFunctionRequest, Environment, FunctionCode,
    ListEventSourceMappingsRequest, UpdateFunctionCodeRequest, UpdateFunctionConfigurationRequest,
    VpcConfig,
};
use alien_aws_clients::s3::{LambdaFunctionConfiguration, NotificationConfiguration};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    standard_resource_tags, CertificateStatus, DnsRecordStatus, Network, NetworkSettings,
    ResourceDefinition, ResourceOutputs, ResourceRef, ResourceStatus, Worker, WorkerOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

mod helpers;
mod support;
#[cfg(test)]
mod tests;

use support::*;

pub use support::{LoadBalancerEndpoint, LoadBalancerState};

#[controller]
pub struct AwsWorkerController {
    pub(crate) arn: Option<String>,
    pub(crate) url: Option<String>,
    /// The logical AWS Lambda worker name (stack prefix + id). Stored to expose in outputs.
    pub(crate) worker_name: Option<String>,
    /// Event source mapping UUIDs for queue triggers
    pub(crate) event_source_mappings: Vec<String>,
    /// Fully qualified domain name for public ingress
    pub(crate) fqdn: Option<String>,
    /// Certificate ID for auto-managed domains
    pub(crate) certificate_id: Option<String>,
    /// ACM certificate ARN (auto-imported or custom)
    pub(crate) certificate_arn: Option<String>,
    /// API Gateway HTTP API ID
    pub(crate) api_id: Option<String>,
    /// API Gateway integration ID
    pub(crate) integration_id: Option<String>,
    /// API Gateway route ID
    pub(crate) route_id: Option<String>,
    /// API Gateway stage name
    pub(crate) stage_name: Option<String>,
    /// API Gateway API mapping ID
    pub(crate) api_mapping_id: Option<String>,
    /// API Gateway domain name
    pub(crate) domain_name: Option<String>,
    /// Endpoint metadata for DNS controller
    pub(crate) load_balancer: Option<LoadBalancerState>,
    /// Timestamp when certificate was imported (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,
    /// Whether this resource uses a customer-managed domain
    pub(crate) uses_custom_domain: bool,
    /// Statement IDs for Lambda permissions granted to S3 for storage triggers
    pub(crate) s3_permission_statement_ids: Vec<String>,
    /// EventBridge rule names for schedule triggers
    pub(crate) eventbridge_rule_names: Vec<String>,
    /// Statement IDs for Lambda permissions granted to EventBridge for schedule triggers
    pub(crate) eventbridge_permission_statement_ids: Vec<String>,
}

#[controller]
impl AwsWorkerController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
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

    #[handler(
        state = CreateWaitForActive,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_wait_for_active(
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

    #[handler(
        state = WaitingForCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_certificate(
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

    #[handler(
        state = ImportingCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_certificate(
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

    #[handler(
        state = CreatingApiGateway,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_gateway(
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

    #[handler(
        state = CreatingApiIntegration,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_integration(
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

    #[handler(
        state = CreatingApiRoute,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_route(
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

    #[handler(
        state = CreatingApiStage,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_stage(
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

    #[handler(
        state = CreatingApiDomain,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_domain(
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

    #[handler(
        state = CreatingApiMapping,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_mapping(
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

    #[handler(
        state = AddingApiGatewayPermission,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn adding_api_gateway_permission(
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

    #[handler(
        state = WaitingForDns,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_dns(
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

    #[handler(
        state = RunningReadinessProbe,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn running_readiness_probe(
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

    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_resource_permissions(
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

    #[handler(
        state = UpdatingEnvVarsWithSelfBinding,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn updating_env_vars_with_self_binding(
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

    #[handler(
        state = CreatingEventSourceMappings,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_event_source_mappings(
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

    #[handler(
        state = CreatingScheduleTriggers,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_schedule_triggers(
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

    #[handler(
        state = SettingConcurrency,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn setting_concurrency(
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
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
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
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateImportingCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if worker_config.public_endpoints.is_empty() || self.uses_custom_domain {
            return Ok(HandlerAction::Continue {
                state: UpdateCodeStart,
                suggested_delay: None,
            });
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id))
        else {
            return Ok(HandlerAction::Continue {
                state: UpdateCodeStart,
                suggested_delay: None,
            });
        };

        if resource.issued_at == self.certificate_issued_at {
            return Ok(HandlerAction::Continue {
                state: UpdateCodeStart,
                suggested_delay: None,
            });
        }

        let Some(certificate_arn) = self.certificate_arn.clone() else {
            return Ok(HandlerAction::Continue {
                state: UpdateCodeStart,
                suggested_delay: None,
            });
        };
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

        acm_client
            .reimport_certificate(
                alien_aws_clients::acm::ReimportCertificateRequest::builder()
                    .certificate_arn(certificate_arn)
                    .certificate(leaf)
                    .private_key(private_key.clone())
                    .maybe_certificate_chain(chain)
                    .tags(tags)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to re-import renewed certificate to ACM".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.certificate_issued_at = resource.issued_at.clone();

        Ok(HandlerAction::Continue {
            state: UpdateCodeStart,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateCodeStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_code_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        let code_changed = current_config.code != previous_config.code;

        // UpdateCodeStart only handles code updates if needed
        if code_changed {
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

            let image_uri = match &current_config.code {
                alien_core::WorkerCode::Image { image } => image.clone(),
                alien_core::WorkerCode::Source { .. } => {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Worker is configured with source code for update, but only pre-built images are supported".to_string(),
                        resource_id: Some(current_config.id.clone()),
                    }));
                }
            };

            // Resolve proxy URIs to native ECR URIs.
            let image_uri = if let Some(ref native_host) = ctx.deployment_config.native_image_host {
                alien_core::image_rewrite::resolve_native_image_uri(&image_uri, native_host)
                    .unwrap_or(image_uri)
            } else {
                image_uri
            };

            let image_uri = Self::rewrite_ecr_region_if_needed(&image_uri, &aws_cfg.region);

            let request = UpdateFunctionCodeRequest::builder()
                .image_uri(image_uri)
                .publish(true)
                .build();

            let arn = self.arn.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Worker ARN not available for code update".to_string(),
                    resource_id: Some(current_config.id.clone()),
                })
            })?;

            client.update_function_code(arn, request).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to update Lambda worker code".to_string(),
                    resource_id: Some(current_config.id.clone()),
                },
            )?;
        }

        // Always transition to wait for code update (even if no code change) - linear flow
        Ok(HandlerAction::Continue {
            state: UpdateCodeWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    #[handler(
        state = UpdateCodeWaitForActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_code_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &current_config.id);
        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for code status check".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            })
        })?;
        let result = client.get_function_configuration(arn, None).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to get worker configuration for code update".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            },
        )?;

        let is_active = result.state.as_deref() == Some("Active");
        let is_successful = result.last_update_status.as_deref() == Some("Successful");

        if is_active && is_successful {
            // Always proceed to config update phase - linear flow
            Ok(HandlerAction::Continue {
                state: UpdateConfigStart,
                suggested_delay: None,
            })
        } else if result.state.as_deref() == Some("Pending")
            || result.last_update_status.as_deref() == Some("InProgress")
        {
            Ok(HandlerAction::Stay {
                max_times: Some(20),
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Code update failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status,
                ),
                resource_id: Some(aws_worker_name),
            }))
        }
    }

    #[handler(
        state = UpdateConfigStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        let config_changed = current_config.permissions != previous_config.permissions
            || current_config.memory_mb != previous_config.memory_mb
            || current_config.timeout_seconds != previous_config.timeout_seconds
            || current_config.environment != previous_config.environment
            || current_config.links != previous_config.links;

        if !config_changed {
            return Ok(HandlerAction::Continue {
                state: UpdateConfigWaitForActive,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &current_config.id);

        // Get the ServiceAccount for this worker's permission profile
        let service_account_id = format!("{}-sa", current_config.get_permissions());
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
                    resource_id: current_config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        let final_env_vars = self
            .prepare_environment_variables(
                &current_config.environment,
                &current_config.links,
                ctx,
                &aws_worker_name,
            )
            .await?;

        let lambda_environment = if !final_env_vars.is_empty() {
            Some(Environment::builder().variables(final_env_vars).build())
        } else {
            None
        };

        // Get VPC configuration if a Network resource exists
        let vpc_config = self.get_vpc_config(ctx)?;

        let request = UpdateFunctionConfigurationRequest::builder()
            .role(role_arn)
            .timeout(current_config.timeout_seconds as i32)
            .memory_size(current_config.memory_mb as i32)
            .maybe_environment(lambda_environment)
            .maybe_vpc_config(vpc_config)
            .build();

        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for config update".to_string(),
                resource_id: Some(current_config.id.clone()),
            })
        })?;

        client
            .update_function_configuration(arn, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Lambda worker configuration".to_string(),
                resource_id: Some(current_config.id.clone()),
            })?;

        // Always transition to wait state - linear flow
        Ok(HandlerAction::Continue {
            state: UpdateConfigWaitForActive,
            suggested_delay: Some(Duration::from_secs(3)),
        })
    }

    #[handler(
        state = UpdateConfigWaitForActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let aws_worker_name = get_aws_worker_name(ctx.resource_prefix, &current_config.id);
        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Worker ARN not available for config status check".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            })
        })?;
        let result = client.get_function_configuration(arn, None).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to get worker configuration for config update".to_string(),
                resource_id: Some(aws_worker_name.clone()),
            },
        )?;

        let is_active = result.state.as_deref() == Some("Active");
        let is_successful = result.last_update_status.as_deref() == Some("Successful");

        if is_active && is_successful {
            Ok(HandlerAction::Continue {
                state: UpdateEnsuringPublicExposure,
                suggested_delay: None,
            })
        } else if result.state.as_deref() == Some("Pending")
            || result.last_update_status.as_deref() == Some("InProgress")
        {
            Ok(HandlerAction::Stay {
                max_times: Some(20),
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Config update failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status
                ),
                resource_id: Some(aws_worker_name),
            }))
        }
    }

    #[handler(
        state = UpdateEnsuringPublicExposure,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_ensuring_public_exposure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        let previous_config = ctx.previous_resource_config::<Worker>()?;

        if current_config.public_endpoints.is_empty() {
            return Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            });
        }

        if previous_config.public_endpoints.is_empty() && self.api_id.is_none() {
            self.url = None;
        }

        let has_domain_info = self.ensure_domain_info(ctx, &current_config.id)?;
        if self.api_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            });
        }

        let next_state = if has_domain_info {
            UpdateWaitingForCertificate
        } else {
            UpdateCreatingApiGateway
        };

        Ok(HandlerAction::Continue {
            state: next_state,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = UpdateWaitingForCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_certificate(ctx).await? {
            HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateImportingInitialCertificate,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiGateway,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_certificate",
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

    #[handler(
        state = UpdateImportingInitialCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_initial_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.importing_certificate(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiGateway,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiGateway,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "importing_certificate",
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

    #[handler(
        state = UpdateCreatingApiGateway,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_gateway(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiIntegration,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiIntegration,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_gateway",
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

    #[handler(
        state = UpdateCreatingApiIntegration,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_integration(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_integration(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiRoute,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiRoute,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_integration",
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

    #[handler(
        state = UpdateCreatingApiRoute,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_route(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_route(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiStage,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiStage,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_route",
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

    #[handler(
        state = UpdateCreatingApiStage,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_stage(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_stage(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiDomain,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiDomain,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_stage",
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

    #[handler(
        state = UpdateCreatingApiDomain,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_domain(ctx).await? {
            HandlerAction::Continue {
                state: CreatingApiMapping,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingApiMapping,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_domain",
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

    #[handler(
        state = UpdateCreatingApiMapping,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_api_mapping(ctx).await? {
            HandlerAction::Continue {
                state: AddingApiGatewayPermission,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateAddingApiGatewayPermission,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_api_mapping",
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

    #[handler(
        state = UpdateAddingApiGatewayPermission,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_adding_api_gateway_permission(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.adding_api_gateway_permission(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: ApplyingResourcePermissions,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateApplyingResourcePermissions,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "adding_api_gateway_permission",
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

    #[handler(
        state = UpdateWaitingForDns,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_dns(ctx).await? {
            HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_dns",
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

    #[handler(
        state = UpdateRunningReadinessProbe,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_running_readiness_probe(
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

        // Either no readiness probe needed, or probe succeeded.
        Ok(HandlerAction::Continue {
            state: UpdateApplyingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateApplyingResourcePermissions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_applying_resource_permissions(
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

    #[handler(
        state = UpdateEnvVarsWithSelfBinding,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_env_vars_with_self_binding(
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

    #[handler(
        state = UpdateEventSourceMappings,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_event_source_mappings(
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

    #[handler(
        state = UpdatingConcurrency,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_concurrency(
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
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.url = None;
        Ok(HandlerAction::Continue {
            state: DeletingApiGateway,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingApiGateway,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_api_gateway(
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

    #[handler(
        state = DeletingEventSourceMappings,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_event_source_mappings(
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

    #[handler(
        state = DeletingScheduleTriggers,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_schedule_triggers(
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

    #[handler(
        state = DetachingVpcConfig,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn detaching_vpc_config(
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

    #[handler(
        state = DetachVpcWaitForActive,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn detach_vpc_wait_for_active(
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

    #[handler(
        state = DeletingWorker,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_function(
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

    #[handler(
        state = DeleteWaitForNotFound,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_wait_for_not_found(
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

    #[handler(
        state = WaitingForVpcEnisReleased,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_vpc_enis_released(
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

    #[handler(
        state = DeletingCertificate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_certificate(
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
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.arn.as_ref().map(|arn| {
            // Map the load balancer endpoint for DNS management
            let load_balancer_endpoint = self
                .load_balancer
                .as_ref()
                .and_then(|lb| lb.endpoint.as_ref())
                .map(|endpoint| alien_core::LoadBalancerEndpoint {
                    dns_name: endpoint.dns_name.clone(),
                    hosted_zone_id: Some(endpoint.hosted_zone_id.clone()),
                });

            ResourceOutputs::new(WorkerOutputs {
                worker_name: self
                    .worker_name
                    .clone()
                    .unwrap_or_else(|| "worker-name-placeholder".to_string()),
                identifier: Some(arn.clone()),
                public_endpoints: self
                    .url
                    .as_ref()
                    .map(|url| {
                        std::collections::HashMap::from([(
                            "default".to_string(),
                            alien_core::PublicEndpointOutput {
                                host: alien_core::public_url_host(url).unwrap_or_default(),
                                protocol: alien_core::ExposeProtocol::Http,
                                port: alien_core::public_url_port(url).unwrap_or(443),
                                url: url.clone(),
                                wildcard_host: None,
                                load_balancer_endpoint,
                            },
                        )])
                    })
                    .unwrap_or_default(),
                commands_push_target: self.worker_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, LambdaWorkerBinding, WorkerBinding};

        if let (Some(worker_name), Some(arn)) = (&self.worker_name, &self.arn) {
            // Extract region from ARN: arn:aws:lambda:us-east-1:123456789:function:name
            let region = arn.split(':').nth(3).unwrap_or("us-east-1").to_string();

            let binding = WorkerBinding::Lambda(LambdaWorkerBinding {
                worker_name: BindingValue::Value(worker_name.clone()),
                region: BindingValue::Value(region),
                url: self.url.as_ref().map(|u| BindingValue::Value(u.clone())),
            });
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}
