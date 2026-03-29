use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use tracing::{debug, info, warn};

use crate::core::EnvironmentVariableBuilder;

use crate::core::split_certificate_chain;
use crate::core::ResourceController;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::function::readiness_probe::{run_readiness_probe, READINESS_PROBE_MAX_ATTEMPTS};
use alien_aws_clients::apigatewayv2::{
    CreateApiMappingRequest, CreateApiRequest, CreateDomainNameRequest, CreateIntegrationRequest,
    CreateRouteRequest, CreateStageRequest, DomainNameConfiguration,
};
use alien_aws_clients::lambda::{
    AddPermissionRequest, CreateFunctionRequest, Environment,
    FunctionCode, UpdateFunctionCodeRequest, UpdateFunctionConfigurationRequest, VpcConfig,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    CertificateStatus, DnsRecordStatus, Function, FunctionOutputs, Ingress, Network,
    ResourceDefinition, ResourceOutputs, ResourceRef, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

/// Generates the full, prefixed AWS resource name.
fn get_aws_function_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

impl AwsFunctionController {
    fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<DomainInfo> {
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

            return Ok(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                certificate_arn: Some(cert_arn),
                uses_custom_domain: true,
            });
        }

        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for public resource".to_string(),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

        let resource = metadata.resources.get(resource_id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain metadata missing for resource".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        Ok(DomainInfo {
            fqdn: resource.fqdn.clone(),
            certificate_id: Some(resource.certificate_id.clone()),
            certificate_arn: None,
            uses_custom_domain: false,
        })
    }
}

#[controller]
pub struct AwsFunctionController {
    pub(crate) arn: Option<String>,
    pub(crate) url: Option<String>,
    /// The logical AWS Lambda function name (stack prefix + id). Stored to expose in outputs.
    pub(crate) function_name: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerEndpoint {
    pub dns_name: String,
    pub hosted_zone_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerState {
    pub endpoint: Option<LoadBalancerEndpoint>,
}

struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    certificate_arn: Option<String>,
    uses_custom_domain: bool,
}

#[controller]
impl AwsFunctionController {
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
        let cfg = ctx.desired_resource_config::<Function>()?;
        info!(name=%cfg.id, code=?cfg.code, "Initiating creation");

        // Product limitation: Only allow at most one queue trigger per function
        let queue_trigger_count = cfg
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::FunctionTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Function '{}' has {} queue triggers, but only one queue trigger per function is currently supported",
                    cfg.id,
                    queue_trigger_count
                ),
                resource_id: Some(cfg.id.clone()),
            }));
        }

        // Get the ServiceAccount for this function's permission profile
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
            alien_core::FunctionCode::Image { image } => image.clone(),
            alien_core::FunctionCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Function is configured with source code, but only pre-built images are supported".to_string(),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        };

        let code = FunctionCode::builder().image_uri(image_uri).build();
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &cfg.id);

        if cfg.ingress == Ingress::Public {
            match Self::resolve_domain_info(ctx, &cfg.id) {
                Ok(domain_info) => {
                    self.fqdn = Some(domain_info.fqdn.clone());
                    self.certificate_id = domain_info.certificate_id;
                    self.certificate_arn = domain_info.certificate_arn;
                    self.uses_custom_domain = domain_info.uses_custom_domain;
                    self.domain_name = Some(domain_info.fqdn.clone());

                    // Check for URL override in deployment config, otherwise use domain FQDN
                    self.url = ctx
                        .deployment_config
                        .public_urls
                        .as_ref()
                        .and_then(|urls| urls.get(&cfg.id).cloned())
                        .or_else(|| Some(format!("https://{}", domain_info.fqdn)));
                }
                Err(_) => {
                    // Standalone mode: no domain metadata available.
                    // Use API Gateway with its default endpoint URL (no custom domain).
                    // The URL will be set after API Gateway creation.
                    info!(
                        function=%cfg.id,
                        "No domain metadata — will use API Gateway default endpoint (standalone mode)"
                    );
                }
            }
        }

        // Prepare environment variables
        let env_vars = self
            .prepare_environment_variables(&cfg.environment, &cfg.links, ctx, &aws_function_name)
            .await?;

        let environment = if !env_vars.is_empty() {
            Some(Environment::builder().variables(env_vars).build())
        } else {
            None
        };

        // Get VPC configuration if a Network resource exists
        let vpc_config = self.get_vpc_config(ctx)?;
        if vpc_config.is_some() {
            info!(name=%aws_function_name, "Configuring Lambda function to run inside VPC");
        }

        let request = CreateFunctionRequest::builder()
            .function_name(aws_function_name.clone())
            .role(role_arn)
            .code(code)
            .package_type("Image".to_string())
            .description(format!("Alien function: {}", cfg.id))
            .timeout(cfg.timeout_seconds as i32)
            .memory_size(cfg.memory_mb as i32)
            .publish(false)
            .maybe_environment(environment)
            .architectures(vec!["arm64".to_string()])
            .maybe_vpc_config(vpc_config)
            .build();

        let response =
            client
                .create_function(request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to create Lambda function".to_string(),
                    resource_id: Some(cfg.id.clone()),
                })?;

        self.arn = response.function_arn.clone();
        self.function_name = Some(aws_function_name.clone());
        info!(name=%aws_function_name, arn=%self.arn.as_deref().unwrap_or("unknown"), "Function created, waiting for active state");

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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &function_config.id);
        debug!(name=%aws_function_name, "Checking function state");

        let response = client
            .get_function_configuration(&aws_function_name, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Lambda function configuration".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        // Check if function is active based on state and last_update_status
        let is_active = response.state.as_deref() == Some("Active")
            && response.last_update_status.as_deref() == Some("Successful");

        if is_active {
            if function_config.ingress == Ingress::Public {
                let next_state =
                    if self.uses_custom_domain || self.certificate_id.is_some() {
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
                name = %aws_function_name,
                state = %response.state.as_deref().unwrap_or("unknown"),
                last_update_status = %response.last_update_status.as_deref().unwrap_or("unknown"),
                "Function not yet active, retrying"
            );
            Ok(HandlerAction::Stay {
                max_times: 20,
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&function_config.id));

        let status = metadata.map(|m| &m.certificate_status);

        match status {
            Some(CertificateStatus::Issued) => Ok(HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay: None,
            }),
            Some(CertificateStatus::Failed) => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "Certificate issuance failed".to_string(),
                    resource_id: Some(function_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: 60,
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let resource = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&function_config.id))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for certificate import".to_string(),
                    resource_id: Some(function_config.id.clone()),
                })
            })?;

        // Certificate data is included in DeploymentConfig
        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;

        let (leaf, chain) = split_certificate_chain(certificate_chain);

        let aws_cfg = ctx.get_aws_config()?;
        let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
        let response = acm_client
            .import_certificate(
                alien_aws_clients::acm::ImportCertificateRequest::builder()
                    .certificate(leaf)
                    .private_key(private_key.clone())
                    .maybe_certificate_chain(chain)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import certificate to ACM".to_string(),
                resource_id: Some(function_config.id.clone()),
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_apigatewayv2_client(aws_cfg).await?;
        let function_config = ctx.desired_resource_config::<Function>()?;

        let api = client
            .create_api(
                CreateApiRequest::builder()
                    .name(format!(
                        "{}-{}-api",
                        ctx.resource_prefix, function_config.id
                    ))
                    .protocol_type("HTTP".to_string())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API Gateway HTTP API".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        let api_id = api.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "API Gateway ID not returned".to_string(),
                resource_id: Some(function_config.id.clone()),
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_apigatewayv2_client(aws_cfg).await?;
        let function_config = ctx.desired_resource_config::<Function>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for integration".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;

        let function_arn = self.arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Function ARN missing for integration".to_string(),
                resource_id: Some(function_config.id.clone()),
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
                resource_id: Some(function_config.id.clone()),
            })?;

        let integration_id = integration.integration_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Integration ID not returned".to_string(),
                resource_id: Some(function_config.id.clone()),
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_apigatewayv2_client(aws_cfg).await?;
        let function_config = ctx.desired_resource_config::<Function>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for route".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;

        let integration_id = self.integration_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Integration ID missing for route".to_string(),
                resource_id: Some(function_config.id.clone()),
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
                resource_id: Some(function_config.id.clone()),
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_apigatewayv2_client(aws_cfg).await?;
        let function_config = ctx.desired_resource_config::<Function>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for stage".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;

        let stage = client
            .create_stage(
                &api_id,
                CreateStageRequest::builder()
                    .stage_name("$default".to_string())
                    .auto_deploy(true)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API stage".to_string(),
                resource_id: Some(function_config.id.clone()),
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
                function=%function_config.id,
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_apigatewayv2_client(aws_cfg).await?;
        let function_config = ctx.desired_resource_config::<Function>()?;

        let fqdn = self.fqdn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "FQDN missing for API domain".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;

        let cert_arn = self.certificate_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate ARN missing for API domain".to_string(),
                resource_id: Some(function_config.id.clone()),
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
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create API domain name".to_string(),
                resource_id: Some(function_config.id.clone()),
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_apigatewayv2_client(aws_cfg).await?;
        let function_config = ctx.desired_resource_config::<Function>()?;

        let api_id = self.api_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "API ID missing for mapping".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;

        let domain_name = self.domain_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain name missing for mapping".to_string(),
                resource_id: Some(function_config.id.clone()),
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
                resource_id: Some(function_config.id.clone()),
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &function_config.id);

        let request = AddPermissionRequest::builder()
            .statement_id("ApiGatewayInvoke".to_string())
            .action("lambda:InvokeFunction".to_string())
            .principal("apigateway.amazonaws.com".to_string())
            .build();

        client
            .add_permission(&aws_function_name, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to add API Gateway permission".to_string(),
                resource_id: Some(function_config.id.clone()),
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&function_config.id));

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
                    resource_id: Some(function_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: 60,
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
        let function_config = ctx.desired_resource_config::<Function>()?;

        // Only run readiness probe if configured and we have a URL (for public functions)
        if function_config.readiness_probe.is_some() && function_config.ingress == Ingress::Public {
            if let Some(url) = &self.url {
                match run_readiness_probe(ctx, url).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: READINESS_PROBE_MAX_ATTEMPTS,
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
        let config = ctx.desired_resource_config::<Function>()?;

        info!(function=%config.id, "Applying resource-scoped permissions for Lambda function");

        // Apply resource-scoped permissions from the stack using the centralized helper.
        // This handles wildcard ("*") permissions and management SA permissions.
        if let Some(function_name) = &self
            .arn
            .as_ref()
            .and_then(|arn| arn.split(':').last().map(|s| s.to_string()))
        {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                &function_name,
                "function",
            )
            .await?;
        }

        info!(function=%config.id, "Successfully applied resource-scoped permissions");

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
        let config = ctx.desired_resource_config::<Function>()?;

        // Skip this step if the function doesn't have public ingress
        // For private functions, the initial env vars already have complete self-binding
        // (no URL to add later)
        if config.ingress != Ingress::Public {
            info!(function=%config.id, "Skipping env var update - no public URL to add");
            return Ok(HandlerAction::Continue {
                state: CreatingEventSourceMappings,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &config.id);

        // Now that we have the URL, update the environment variables
        // with the complete self-binding information including the URL
        let final_env_vars = self
            .prepare_environment_variables(
                &config.environment,
                &config.links,
                ctx,
                &aws_function_name,
            )
            .await?;

        let lambda_environment = if !final_env_vars.is_empty() {
            Some(Environment::builder().variables(final_env_vars).build())
        } else {
            None
        };

        // Get the ServiceAccount for this function's permission profile
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
                message: "Function ARN not available for env var update".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        client
            .update_function_configuration(arn, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Lambda function configuration with self-binding"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(function=%config.id, "Successfully updated environment variables with complete self-binding");

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
        let config = ctx.desired_resource_config::<Function>()?;
        let aws_cfg = ctx.get_aws_config()?;

        // Validation: Only allow at most one queue trigger per function (non-retriable error)
        let queue_trigger_count = config
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::FunctionTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Function '{}' has {} queue triggers, but only one queue trigger per function is currently supported",
                    config.id,
                    queue_trigger_count
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        // Linear flow principle: Always perform this state. Create mappings for ALL queue triggers
        let mut created_any = false;
        for trigger in &config.triggers {
            if let alien_core::FunctionTrigger::Queue { queue } = trigger {
                info!(function=%config.id, queue=%queue.id, "Creating SQS event source mapping");
                self.create_queue_event_source_mapping(ctx, aws_cfg, &config, queue)
                    .await?;
                created_any = true;
            }
        }
        if !created_any {
            info!(function=%config.id, "No queue triggers found, skipping event source mapping creation");
        }

        // Always continue to concurrency configuration (linear flow)
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
        let config = ctx.desired_resource_config::<Function>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &config.id);

        if let Some(limit) = config.concurrency_limit {
            info!(function=%config.id, limit=%limit, "Setting reserved concurrency on function");
            client
                .put_function_concurrency(&aws_function_name, limit)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to set function reserved concurrency".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
        } else {
            debug!(function=%config.id, "No concurrency limit configured, skipping");
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &function_config.id);

        // Heartbeat check: verify function still exists and is in correct state
        let function_info = client
            .get_function_configuration(&aws_function_name, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get function configuration during heartbeat check".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        // Verify function is in active state - drift is non-retryable
        if function_info.state.as_deref() != Some("Active") {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: function_config.id.clone(),
                message: format!(
                    "Function state is '{}', expected 'Active'",
                    function_info.state.as_deref().unwrap_or("unknown")
                ),
            }));
        }

        // Check if certificate was renewed (for public functions with auto-managed domains)
        if function_config.ingress == Ingress::Public {
            if let Some(domain_metadata) = &ctx.deployment_config.domain_metadata {
                if let Some(resource_info) = domain_metadata.resources.get(&function_config.id) {
                    if let Some(new_issued_at) = &resource_info.issued_at {
                        match &self.certificate_issued_at {
                            Some(stored) if new_issued_at != stored => {
                                // Certificate renewed! Trigger update flow to re-import
                                info!(
                                    name = %function_config.id,
                                    old_issued_at = %stored,
                                    new_issued_at = %new_issued_at,
                                    "Certificate renewed, triggering update"
                                );
                                return Ok(HandlerAction::Continue {
                                    state: UpdateStart,
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

        debug!(name = %function_config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Function>()?;
        let previous_config = ctx.previous_resource_config::<Function>()?;
        let code_changed = current_config.code != previous_config.code;

        // UpdateStart only handles code updates if needed
        if code_changed {
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

            let image_uri = match &current_config.code {
                alien_core::FunctionCode::Image { image } => image,
                alien_core::FunctionCode::Source { .. } => {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Function is configured with source code for update, but only pre-built images are supported".to_string(),
                        resource_id: Some(current_config.id.clone()),
                    }));
                }
            };

            let request = UpdateFunctionCodeRequest::builder()
                .image_uri(image_uri.clone())
                .publish(true)
                .build();

            let arn = self.arn.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Function ARN not available for code update".to_string(),
                    resource_id: Some(current_config.id.clone()),
                })
            })?;

            client.update_function_code(arn, request).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to update Lambda function code".to_string(),
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
        let current_config = ctx.desired_resource_config::<Function>()?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &current_config.id);
        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Function ARN not available for code status check".to_string(),
                resource_id: Some(aws_function_name.clone()),
            })
        })?;
        let result = client.get_function_configuration(arn, None).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to get function configuration for code update".to_string(),
                resource_id: Some(aws_function_name.clone()),
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
                max_times: 20,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Code update failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status,
                ),
                resource_id: Some(aws_function_name),
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
        let current_config = ctx.desired_resource_config::<Function>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &current_config.id);

        // Get the ServiceAccount for this function's permission profile
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
                &aws_function_name,
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
                message: "Function ARN not available for config update".to_string(),
                resource_id: Some(current_config.id.clone()),
            })
        })?;

        client
            .update_function_configuration(arn, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Lambda function configuration".to_string(),
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
        let current_config = ctx.desired_resource_config::<Function>()?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &current_config.id);
        let arn = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Function ARN not available for config status check".to_string(),
                resource_id: Some(aws_function_name.clone()),
            })
        })?;
        let result = client.get_function_configuration(arn, None).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to get function configuration for config update".to_string(),
                resource_id: Some(aws_function_name.clone()),
            },
        )?;

        let is_active = result.state.as_deref() == Some("Active");
        let is_successful = result.last_update_status.as_deref() == Some("Successful");

        if is_active && is_successful {
            // Always proceed to readiness probe phase - linear flow
            Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            })
        } else if result.state.as_deref() == Some("Pending")
            || result.last_update_status.as_deref() == Some("InProgress")
        {
            Ok(HandlerAction::Stay {
                max_times: 20,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Config update failed. State: {:?}, Last Update: {:?}",
                    result.state, result.last_update_status
                ),
                resource_id: Some(aws_function_name),
            }))
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
        let function_config = ctx.desired_resource_config::<Function>()?;

        // Only run readiness probe if configured and we have a URL (for public functions)
        if function_config.readiness_probe.is_some() && function_config.ingress == Ingress::Public {
            if let Some(url) = &self.url {
                match run_readiness_probe(ctx, url).await {
                    Ok(()) => {
                        // Probe succeeded, proceed to Ready
                    }
                    Err(_) => {
                        // Probe failed, let the framework handle retries
                        return Ok(HandlerAction::Stay {
                            max_times: READINESS_PROBE_MAX_ATTEMPTS,
                            suggested_delay: Some(Duration::from_secs(5)),
                        });
                    }
                }
            }
        }

        // Either no readiness probe needed, or probe succeeded - proceed to UpdateEventSourceMappings
        Ok(HandlerAction::Continue {
            state: UpdateEventSourceMappings,
            suggested_delay: None,
        })
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
        let current_config = ctx.desired_resource_config::<Function>()?;
        let previous_config = ctx.previous_resource_config::<Function>()?;

        // Validation: Only allow at most one queue trigger per function (non-retriable error)
        let queue_trigger_count = current_config
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::FunctionTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Function '{}' has {} queue triggers, but only one queue trigger per function is currently supported",
                    current_config.id,
                    queue_trigger_count
                ),
                resource_id: Some(current_config.id.clone()),
            }));
        }

        // Check if triggers have changed
        let triggers_changed = current_config.triggers != previous_config.triggers;

        if triggers_changed {
            info!(function=%current_config.id, "Function triggers changed, updating event source mappings");

            // For simplicity, we'll delete old mappings and create new ones
            // In a production system, you might want to do a more sophisticated diff
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

            // Delete existing mappings
            for uuid in &self.event_source_mappings.clone() {
                match client.delete_event_source_mapping(uuid).await {
                    Ok(_) => {
                        info!(function=%current_config.id, uuid=%uuid, "Deleted existing event source mapping");
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(function=%current_config.id, uuid=%uuid, "Event source mapping was already deleted");
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
                if let alien_core::FunctionTrigger::Queue { queue } = trigger {
                    self.create_queue_event_source_mapping(ctx, aws_cfg, &current_config, queue)
                        .await?;
                }
            }
        } else {
            info!(function=%current_config.id, "No trigger changes detected");
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
        let config = ctx.desired_resource_config::<Function>()?;
        let prev_config = ctx.previous_resource_config::<Function>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &config.id);

        if config.concurrency_limit != prev_config.concurrency_limit {
            match config.concurrency_limit {
                Some(limit) => {
                    info!(function=%config.id, limit=%limit, "Updating reserved concurrency on function");
                    client
                        .put_function_concurrency(&aws_function_name, limit)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to update function reserved concurrency".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
                None => {
                    info!(function=%config.id, "Removing reserved concurrency from function");
                    client
                        .delete_function_concurrency(&aws_function_name)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to remove function reserved concurrency".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;
                }
            }
        } else {
            debug!(function=%config.id, "Concurrency limit unchanged, skipping");
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
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let function_config = ctx.desired_resource_config::<Function>()?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &function_config.id);

        // If function has a URL, delete it first
        if self.url.is_some() {
            info!(name=%aws_function_name, "Deleting function URL in DeleteStart");
            match client
                .delete_function_url_config(&aws_function_name, None)
                .await
            {
                Ok(_) => {
                    info!(name=%aws_function_name, "Function URL deleted");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    warn!(name=%aws_function_name, "Function URL was already deleted (not found)");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete function URL".to_string(),
                        resource_id: Some(function_config.id.clone()),
                    }));
                }
            }
        }

        // Always continue to delete API Gateway resources first - linear flow
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
        let function_config = ctx.desired_resource_config::<Function>()?;

        // Ordering matters: delete API mapping before domain name, domain name before API.
        if let (Some(domain_name), Some(api_mapping_id)) =
            (self.domain_name.as_ref(), self.api_mapping_id.as_ref())
        {
            let client = ctx.service_provider.get_aws_apigatewayv2_client(aws_cfg).await?;
            match client.delete_api_mapping(domain_name, api_mapping_id).await {
                Ok(()) => info!(function=%function_config.id, "API mapping deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(function=%function_config.id, "API mapping already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete API mapping".to_string(),
                        resource_id: Some(function_config.id.clone()),
                    }))
                }
            }
        }
        self.api_mapping_id = None;

        if let Some(domain_name) = self.domain_name.as_ref() {
            let client = ctx.service_provider.get_aws_apigatewayv2_client(aws_cfg).await?;
            match client.delete_domain_name(domain_name).await {
                Ok(()) => {
                    info!(function=%function_config.id, domain=%domain_name, "Custom domain deleted")
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(function=%function_config.id, "Custom domain already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete custom domain".to_string(),
                        resource_id: Some(function_config.id.clone()),
                    }))
                }
            }
        }
        self.domain_name = None;

        // Deleting the API cascades to routes, integrations, and stages.
        if let Some(api_id) = self.api_id.as_ref() {
            let client = ctx.service_provider.get_aws_apigatewayv2_client(aws_cfg).await?;
            match client.delete_api(api_id).await {
                Ok(()) => {
                    info!(function=%function_config.id, api_id=%api_id, "API Gateway deleted")
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(function=%function_config.id, "API Gateway already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete API Gateway".to_string(),
                        resource_id: Some(function_config.id.clone()),
                    }))
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
        let function_config = ctx.desired_resource_config::<Function>()?;

        // Linear flow principle: Always perform this state, even if no event source mappings
        if !self.event_source_mappings.is_empty() {
            info!(function=%function_config.id, mappings=?self.event_source_mappings, "Deleting event source mappings");

            // Delete all event source mappings using best-effort approach (ignore NotFound)
            for uuid in &self.event_source_mappings.clone() {
                match client.delete_event_source_mapping(uuid).await {
                    Ok(_) => {
                        info!(function=%function_config.id, uuid=%uuid, "Event source mapping deleted successfully");
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(function=%function_config.id, uuid=%uuid, "Event source mapping was already deleted (not found)");
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete event source mapping '{}'", uuid),
                            resource_id: Some(function_config.id.clone()),
                        }));
                    }
                }
            }

            // Clear the mapping list after successful deletion
            self.event_source_mappings.clear();
        } else {
            info!(function=%function_config.id, "No event source mappings to delete");
        }

        // Always continue to DeletingFunction state (linear flow)
        Ok(HandlerAction::Continue {
            state: DeletingFunction,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingFunction,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_function(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;
        let function_config = ctx.desired_resource_config::<Function>()?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &function_config.id);
        info!(name=%aws_function_name, "Deleting function itself: {}", aws_function_name);

        match client.delete_function(&aws_function_name, None).await {
            Ok(_) => {
                info!(name=%aws_function_name, "Function deleted successfully, proceeding to DeleteWaitForNotFound state");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                warn!(name=%aws_function_name, "Function was already deleted (not found), proceeding to DeleteWaitForNotFound state");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete Lambda function".to_string(),
                    resource_id: Some(function_config.id.clone()),
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let aws_function_name = get_aws_function_name(ctx.resource_prefix, &function_config.id);
        let lookup_identifier = arn.map(|a| a.as_str()).unwrap_or(&aws_function_name);

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
                self.function_name = None;
                self.event_source_mappings.clear();
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            Ok(_) => Ok(HandlerAction::Stay {
                max_times: 10,
                suggested_delay: Some(Duration::from_secs(10)),
            }),
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to check function deletion status".to_string(),
                resource_id: Some(function_config.id.clone()),
            })),
        }
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
        let function_config = ctx.desired_resource_config::<Function>()?;

        if let Some(certificate_arn) = self.certificate_arn.as_ref() {
            let aws_cfg = ctx.get_aws_config()?;
            let acm_client = ctx.service_provider.get_aws_acm_client(aws_cfg).await?;
            match acm_client.delete_certificate(certificate_arn).await {
                Ok(()) => info!(function=%function_config.id, "ACM certificate deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(function=%function_config.id, "ACM certificate already gone");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete ACM certificate".to_string(),
                        resource_id: Some(function_config.id.clone()),
                    }))
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

            ResourceOutputs::new(FunctionOutputs {
                function_name: self
                    .function_name
                    .clone()
                    .unwrap_or_else(|| "function-name-placeholder".to_string()),
                url: self.url.clone(),
                identifier: Some(arn.clone()),
                load_balancer_endpoint,
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, FunctionBinding, LambdaFunctionBinding};

        if let (Some(function_name), Some(arn)) = (&self.function_name, &self.arn) {
            // Extract region from ARN: arn:aws:lambda:us-east-1:123456789:function:name
            let region = arn.split(':').nth(3).unwrap_or("us-east-1").to_string();

            let binding = FunctionBinding::Lambda(LambdaFunctionBinding {
                function_name: BindingValue::Value(function_name.clone()),
                region: BindingValue::Value(region),
                url: self.url.as_ref().map(|u| BindingValue::Value(u.clone())),
            });
            Ok(Some(serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize binding parameters".to_string(),
                },
            )?))
        } else {
            Ok(None)
        }
    }
}

// Separate impl block for helper methods
impl AwsFunctionController {
    /// Creates an SQS event source mapping for a queue trigger
    async fn create_queue_event_source_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        aws_cfg: &alien_aws_clients::AwsClientConfig,
        function_config: &alien_core::Function,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<()> {
        let lambda_client = ctx.service_provider.get_aws_lambda_client(aws_cfg).await?;

        // Get queue controller to access outputs
        let queue_controller =
            ctx.require_dependency::<crate::queue::aws::AwsQueueController>(queue_ref)?;
        let queue_outputs_wrapper = queue_controller.get_outputs().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: function_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let queue_outputs = queue_outputs_wrapper
            .downcast_ref::<alien_core::QueueOutputs>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Invalid queue outputs type".to_string(),
                    resource_id: Some(function_config.id.clone()),
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
            function=%function_config.id,
            queue_arn=%queue_arn,
            "Creating SQS event source mapping"
        );

        let function_name = self.arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Function ARN not available for event source mapping".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;

        let request = alien_aws_clients::lambda::CreateEventSourceMappingRequest::builder()
            .event_source_arn(queue_arn.clone())
            .function_name(function_name.clone())
            .batch_size(1) // Always 1 message per invocation as per design
            .enabled(true)
            .build();

        let response = lambda_client
            .create_event_source_mapping(request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create event source mapping for queue '{}'",
                    queue_name
                ),
                resource_id: Some(function_config.id.clone()),
            })?;

        if let Some(uuid) = response.uuid {
            self.event_source_mappings.push(uuid.clone());
            info!(
                function=%function_config.id,
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
    /// to configure the Lambda function to run inside the VPC.
    ///
    /// Returns `None` if no Network resource exists in the stack.
    fn get_vpc_config(&self, ctx: &ResourceControllerContext<'_>) -> Result<Option<VpcConfig>> {
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

    async fn prepare_environment_variables(
        &self,
        initial_env: &HashMap<String, String>,
        links: &[ResourceRef],
        ctx: &ResourceControllerContext<'_>,
        function_name_for_error_logging: &str,
    ) -> Result<HashMap<String, String>> {
        let function_config = ctx.desired_resource_config::<Function>()?;

        // Get the function's own binding params (may be None during initial creation)
        let self_binding_params = self.get_binding_params()?;

        let env_vars = EnvironmentVariableBuilder::new(initial_env)
            .add_standard_alien_env_vars(ctx)
            .add_function_transport_env_vars(ctx.platform)
            .add_env_var("ALIEN_RUNTIME_SEND_OTLP".to_string(), "true".to_string())
            .add_linked_resources(links, ctx, function_name_for_error_logging)
            .await?
            .add_self_function_binding(&function_config.id, self_binding_params.as_ref())?
            .build();

        Ok(env_vars)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(function_name: &str) -> Self {
        Self {
            state: AwsFunctionState::Ready,
            arn: Some(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                function_name
            )),
            url: Some(format!("https://abcd1234.lambda-url.us-east-1.on.aws/")),
            function_name: Some(function_name.to_string()),
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
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # AWS Function Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::collections::HashMap;
    use std::sync::Arc;

    use alien_aws_clients::acm::{ImportCertificateResponse, MockAcmApi};
    use alien_aws_clients::apigatewayv2::{
        Api, ApiMapping, DomainName, DomainNameConfiguration, Integration, MockApiGatewayV2Api,
        Route, Stage,
    };
    use alien_aws_clients::iam::MockIamApi;
    use alien_aws_clients::lambda::{AddPermissionResponse, FunctionConfiguration, MockLambdaApi};
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{
        CertificateStatus, DnsRecordStatus, DomainMetadata, Function, FunctionOutputs, Ingress,
        Platform, ResourceDomainInfo, ResourceStatus,
    };
    use alien_error::AlienError;
    use httpmock::prelude::*;
    use rstest::rstest;

    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use crate::function::{
        fixtures::*, readiness_probe::test_utils::create_readiness_probe_mock,
        AwsFunctionController,
    };

    fn create_successful_function_response(function_name: &str) -> FunctionConfiguration {
        FunctionConfiguration {
            function_name: Some(function_name.to_string()),
            function_arn: Some(format!(
                "arn:aws:lambda:us-east-1:123456789012:function:{}",
                function_name
            )),
            state: Some("Active".to_string()),
            last_update_status: Some("Successful".to_string()),
            kms_key_arn: None,
        }
    }

    fn create_test_domain_metadata(resource_id: &str) -> DomainMetadata {
        let mut resources = HashMap::new();
        resources.insert(
            resource_id.to_string(),
            ResourceDomainInfo {
                fqdn: format!("{}.test.example.com", resource_id),
                certificate_id: "test-cert-id".to_string(),
                certificate_status: CertificateStatus::Issued,
                dns_status: DnsRecordStatus::Active,
                dns_error: None,
                certificate_chain: Some(
                    "-----BEGIN CERTIFICATE-----\nMIIBtest\n-----END CERTIFICATE-----\n"
                        .to_string(),
                ),
                private_key: Some(
                    "-----BEGIN RSA PRIVATE KEY-----\nMIIBtest\n-----END RSA PRIVATE KEY-----\n"
                        .to_string(),
                ),
                issued_at: Some("2024-01-01T00:00:00Z".to_string()),
            },
        );
        DomainMetadata {
            base_domain: "test.example.com".to_string(),
            public_subdomain: "test".to_string(),
            hosted_zone_id: "Z1234567890ABC".to_string(),
            resources,
        }
    }

    fn create_acm_mock_for_creation() -> Arc<MockAcmApi> {
        let mut mock_acm = MockAcmApi::new();
        mock_acm.expect_import_certificate().returning(|_| {
            Ok(ImportCertificateResponse {
                certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                    .to_string(),
            })
        });
        Arc::new(mock_acm)
    }

    fn create_acm_mock_for_creation_and_deletion() -> Arc<MockAcmApi> {
        let mut mock_acm = MockAcmApi::new();
        mock_acm.expect_import_certificate().returning(|_| {
            Ok(ImportCertificateResponse {
                certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                    .to_string(),
            })
        });
        mock_acm.expect_delete_certificate().returning(|_| Ok(()));
        Arc::new(mock_acm)
    }

    fn create_apigatewayv2_mock_for_creation() -> Arc<MockApiGatewayV2Api> {
        let mut mock_apigw = MockApiGatewayV2Api::new();
        mock_apigw.expect_create_api().returning(|_| {
            Ok(Api {
                api_id: Some("test-api-id".to_string()),
                api_endpoint: Some(
                    "https://test-api-id.execute-api.us-east-1.amazonaws.com".to_string(),
                ),
                name: None,
                protocol_type: None,
            })
        });
        mock_apigw.expect_create_integration().returning(|_, _| {
            Ok(Integration {
                integration_id: Some("test-integration-id".to_string()),
                integration_type: None,
                integration_uri: None,
            })
        });
        mock_apigw.expect_create_route().returning(|_, _| {
            Ok(Route {
                route_id: Some("test-route-id".to_string()),
                route_key: None,
            })
        });
        mock_apigw.expect_create_stage().returning(|_, _| {
            Ok(Stage {
                stage_name: Some("$default".to_string()),
                auto_deploy: None,
            })
        });
        mock_apigw.expect_create_domain_name().returning(|_| {
            Ok(DomainName {
                domain_name: Some("test.example.com".to_string()),
                domain_name_configurations: Some(vec![DomainNameConfiguration {
                    certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                        .to_string(),
                    endpoint_type: "REGIONAL".to_string(),
                    security_policy: "TLS_1_2".to_string(),
                    api_gateway_domain_name: Some(
                        "test.execute-api.us-east-1.amazonaws.com".to_string(),
                    ),
                    hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
                }]),
            })
        });
        mock_apigw.expect_create_api_mapping().returning(|_, _| {
            Ok(ApiMapping {
                api_mapping_id: Some("test-mapping-id".to_string()),
                api_mapping_key: None,
                stage: None,
            })
        });
        Arc::new(mock_apigw)
    }

    fn create_apigatewayv2_mock_for_creation_and_deletion() -> Arc<MockApiGatewayV2Api> {
        let mut mock_apigw = MockApiGatewayV2Api::new();
        mock_apigw.expect_create_api().returning(|_| {
            Ok(Api {
                api_id: Some("test-api-id".to_string()),
                api_endpoint: Some(
                    "https://test-api-id.execute-api.us-east-1.amazonaws.com".to_string(),
                ),
                name: None,
                protocol_type: None,
            })
        });
        mock_apigw.expect_create_integration().returning(|_, _| {
            Ok(Integration {
                integration_id: Some("test-integration-id".to_string()),
                integration_type: None,
                integration_uri: None,
            })
        });
        mock_apigw.expect_create_route().returning(|_, _| {
            Ok(Route {
                route_id: Some("test-route-id".to_string()),
                route_key: None,
            })
        });
        mock_apigw.expect_create_stage().returning(|_, _| {
            Ok(Stage {
                stage_name: Some("$default".to_string()),
                auto_deploy: None,
            })
        });
        mock_apigw.expect_create_domain_name().returning(|_| {
            Ok(DomainName {
                domain_name: Some("test.example.com".to_string()),
                domain_name_configurations: Some(vec![DomainNameConfiguration {
                    certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                        .to_string(),
                    endpoint_type: "REGIONAL".to_string(),
                    security_policy: "TLS_1_2".to_string(),
                    api_gateway_domain_name: Some(
                        "test.execute-api.us-east-1.amazonaws.com".to_string(),
                    ),
                    hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
                }]),
            })
        });
        mock_apigw.expect_create_api_mapping().returning(|_, _| {
            Ok(ApiMapping {
                api_mapping_id: Some("test-mapping-id".to_string()),
                api_mapping_key: None,
                stage: None,
            })
        });
        mock_apigw
            .expect_delete_api_mapping()
            .returning(|_, _| Ok(()));
        mock_apigw.expect_delete_domain_name().returning(|_| Ok(()));
        mock_apigw.expect_delete_api().returning(|_| Ok(()));
        Arc::new(mock_apigw)
    }

    fn setup_mock_client_for_creation_and_update(
        function_name: &str,
        has_url: bool,
    ) -> Arc<MockLambdaApi> {
        let mut mock_lambda = MockLambdaApi::new();

        // Mock successful function creation
        let function_name = function_name.to_string();
        let function_name_for_create = function_name.clone();
        mock_lambda.expect_create_function().returning(move |_| {
            Ok(create_successful_function_response(
                &function_name_for_create,
            ))
        });

        // Mock function status checks - first pending, then active
        let function_name_for_get = function_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&function_name_for_get)));

        // Mock API Gateway permission and self-binding env var update if public ingress
        if has_url {
            mock_lambda
                .expect_add_permission()
                .returning(|_, _| Ok(AddPermissionResponse { statement: None }));

            let function_name_for_self_binding = function_name.clone();
            mock_lambda
                .expect_update_function_configuration()
                .returning(move |_, _| {
                    Ok(create_successful_function_response(
                        &function_name_for_self_binding,
                    ))
                });
        }

        // Mock concurrency operations (may or may not be called depending on function config)
        mock_lambda
            .expect_put_function_concurrency()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_delete_function_concurrency()
            .returning(|_| Ok(()));

        // Mock successful updates
        let function_name_for_code_update = function_name.clone();
        mock_lambda
            .expect_update_function_code()
            .returning(move |_, _| {
                Ok(create_successful_function_response(
                    &function_name_for_code_update,
                ))
            });

        if !has_url {
            let function_name_for_config_update = function_name.clone();
            mock_lambda
                .expect_update_function_configuration()
                .returning(move |_, _| {
                    Ok(create_successful_function_response(
                        &function_name_for_config_update,
                    ))
                });
        }

        Arc::new(mock_lambda)
    }

    fn setup_mock_client_for_creation_and_deletion(
        function_name: &str,
        has_url: bool,
    ) -> Arc<MockLambdaApi> {
        let mut mock_lambda = MockLambdaApi::new();

        // Mock successful function creation
        let function_name = function_name.to_string();
        let function_name_for_create = function_name.clone();
        mock_lambda.expect_create_function().returning(move |_| {
            Ok(create_successful_function_response(
                &function_name_for_create,
            ))
        });

        // Mock function status checks
        let function_name_for_get = function_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&function_name_for_get)))
            .times(1); // Only for creation flow

        // Mock API Gateway permission and self-binding env var update if public ingress
        if has_url {
            mock_lambda
                .expect_add_permission()
                .returning(|_, _| Ok(AddPermissionResponse { statement: None }));

            // Mock update_function_configuration for self-binding env var update
            let function_name_for_config_update = function_name.clone();
            mock_lambda
                .expect_update_function_configuration()
                .returning(move |_, _| {
                    Ok(create_successful_function_response(
                        &function_name_for_config_update,
                    ))
                });

            // delete_start calls delete_function_url_config when self.url is set;
            // in the new flow this URL is the custom domain FQDN, not a Lambda URL,
            // so gracefully returns Ok (the handler also tolerates NotFound)
            mock_lambda
                .expect_delete_function_url_config()
                .returning(|_, _| Ok(()));
        }

        // Mock concurrency operations (may or may not be called depending on function config)
        mock_lambda
            .expect_put_function_concurrency()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_delete_function_concurrency()
            .returning(|_| Ok(()));

        // Mock successful function deletion
        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));

        // Mock function not found during deletion check
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Function".to_string(),
                        resource_name: "test-function".to_string(),
                    },
                ))
            });

        Arc::new(mock_lambda)
    }

    fn setup_mock_client_for_best_effort_deletion(
        _function_name: &str,
        url_missing: bool,
        function_missing: bool,
    ) -> Arc<MockLambdaApi> {
        let mut mock_lambda = MockLambdaApi::new();

        // Mock URL deletion (might fail if URL missing)
        if url_missing {
            mock_lambda
                .expect_delete_function_url_config()
                .returning(|_, _| {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "Function URL".to_string(),
                            resource_name: "test-url".to_string(),
                        },
                    ))
                });
        } else {
            mock_lambda
                .expect_delete_function_url_config()
                .returning(|_, _| Ok(()));
        }

        // Mock function deletion (might fail if function missing)
        if function_missing {
            mock_lambda.expect_delete_function().returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Function".to_string(),
                        resource_name: "test-function".to_string(),
                    },
                ))
            });
        } else {
            mock_lambda
                .expect_delete_function()
                .returning(|_, _| Ok(()));
        }

        // Always return not found for final status check
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Function".to_string(),
                        resource_name: "test-function".to_string(),
                    },
                ))
            });

        Arc::new(mock_lambda)
    }

    fn create_aws_iam_mock_for_resource_permissions() -> Arc<MockIamApi> {
        let mut mock_iam = MockIamApi::new();
        mock_iam
            .expect_put_role_policy()
            .returning(|_, _, _| Ok(()));
        Arc::new(mock_iam)
    }

    fn setup_mock_service_provider(
        mock_lambda: Arc<MockLambdaApi>,
        mock_acm: Option<Arc<MockAcmApi>>,
        mock_apigw: Option<Arc<MockApiGatewayV2Api>>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_lambda_client()
            .returning(move |_| Ok(mock_lambda.clone()));

        // Mock IAM client for resource-scoped permissions (ApplyingResourcePermissions state)
        let mock_iam = create_aws_iam_mock_for_resource_permissions();
        mock_provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        if let Some(acm) = mock_acm {
            mock_provider
                .expect_get_aws_acm_client()
                .returning(move |_| Ok(acm.clone()));
        }

        if let Some(apigw) = mock_apigw {
            mock_provider
                .expect_get_aws_apigatewayv2_client()
                .returning(move |_| Ok(apigw.clone()));
        }

        Arc::new(mock_provider)
    }

    /// Sets up all mocks for a function test, including Lambda, ACM, and API Gateway.
    ///
    /// Returns `(mock_provider, optional_mock_server, optional_domain_metadata, optional_public_urls)`.
    /// For public functions, `domain_metadata` and `public_urls` must be set on the executor builder.
    /// When a readiness probe is configured, `public_urls` overrides the FQDN URL so the probe
    /// hits the local mock HTTP server instead.
    fn setup_mocks_for_function(
        function: &Function,
        function_name: &str,
        for_deletion: bool,
    ) -> (
        Arc<MockPlatformServiceProvider>,
        Option<MockServer>,
        Option<DomainMetadata>,
        Option<HashMap<String, String>>,
    ) {
        let has_url = function.ingress == Ingress::Public;
        let needs_readiness_probe = has_url && function.readiness_probe.is_some();

        // Set up mock server for readiness probe if needed
        let mock_server = if needs_readiness_probe {
            Some(create_readiness_probe_mock(function))
        } else {
            None
        };

        // Set up Lambda client mock (same for both flows; URL config calls are removed)
        let lambda_mock = if for_deletion {
            setup_mock_client_for_creation_and_deletion(function_name, has_url)
        } else {
            setup_mock_client_for_creation_and_update(function_name, has_url)
        };

        // Set up ACM and API Gateway mocks for public functions
        let (acm_mock, apigw_mock, domain_metadata, public_urls) = if has_url {
            let dm = create_test_domain_metadata(&function.id);
            let acm = if for_deletion {
                create_acm_mock_for_creation_and_deletion()
            } else {
                create_acm_mock_for_creation()
            };
            let apigw = if for_deletion {
                create_apigatewayv2_mock_for_creation_and_deletion()
            } else {
                create_apigatewayv2_mock_for_creation()
            };
            // For readiness probe tests, override the FQDN URL with the mock server URL
            let pub_urls = mock_server.as_ref().map(|server| {
                let mut map = HashMap::new();
                map.insert(function.id.clone(), server.base_url());
                map
            });
            (Some(acm), Some(apigw), Some(dm), pub_urls)
        } else {
            (None, None, None, None)
        };

        let mock_provider = setup_mock_service_provider(lambda_mock, acm_mock, apigw_mock);

        (mock_provider, mock_server, domain_metadata, public_urls)
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_function(), false)]
    #[case::env_vars(function_with_env_vars(), false)]
    #[case::storage_link(function_with_storage_link(), false)]
    #[case::env_and_storage(function_with_env_and_storage(), false)]
    #[case::multiple_storages(function_with_multiple_storages(), false)]
    #[case::public_ingress(function_public_ingress(), true)]
    #[case::private_ingress(function_private_ingress(), false)]
    #[case::concurrency(function_with_concurrency(), false)]
    #[case::custom_config(function_custom_config(), false)]
    #[case::readiness_probe(function_with_readiness_probe(), true)]
    #[case::complete_test(function_complete_test(), true)]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(
        #[case] function: Function,
        #[case] _has_url: bool,
    ) {
        let function_name = format!("test-{}", function.id);
        let (mock_provider, _mock_server, domain_metadata, public_urls) =
            setup_mocks_for_function(&function, &function_name, true);

        let mut builder = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AwsFunctionController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
        }
        if let Some(urls) = public_urls {
            builder = builder.public_urls(urls);
        }

        let mut executor = builder.build().await.unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs are available
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<FunctionOutputs>().unwrap();
        assert!(function_outputs.identifier.is_some());
        assert!(function_outputs.function_name.starts_with("test-"));

        // Delete the function
        executor.delete().unwrap();

        // Run delete flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    #[rstest]
    #[case::basic_to_env(basic_function(), function_with_env_vars())]
    #[case::env_to_storage(function_with_env_vars(), function_with_storage_link())]
    #[case::storage_to_custom(function_with_storage_link(), function_custom_config())]
    #[case::custom_to_public(function_custom_config(), function_public_ingress())]
    #[case::public_to_complete(function_public_ingress(), function_complete_test())]
    #[case::complete_to_basic(function_complete_test(), basic_function())]
    #[tokio::test]
    async fn test_update_flow_succeeds(
        #[case] from_function: Function,
        #[case] to_function: Function,
    ) {
        // Ensure both functions have the same ID for valid updates
        let function_id = "test-update-function".to_string();
        let mut from_function = from_function;
        from_function.id = function_id.clone();

        let mut to_function = to_function;
        to_function.id = function_id.clone();

        let function_name = format!("test-{}", function_id);
        let (mock_provider, mock_server, domain_metadata, public_urls) =
            setup_mocks_for_function(&to_function, &function_name, false);

        // Start with the "from" function in Ready state
        let mut ready_controller = AwsFunctionController::mock_ready(&function_name);

        // If the target function has a readiness probe, update the controller URL to point to mock server
        if to_function.readiness_probe.is_some() && to_function.ingress == Ingress::Public {
            if let Some(ref server) = mock_server {
                ready_controller.url = Some(server.base_url());
            }
        }

        let mut builder = SingleControllerExecutor::builder()
            .resource(from_function)
            .controller(ready_controller)
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
        }
        if let Some(urls) = public_urls {
            builder = builder.public_urls(urls);
        }

        let mut executor = builder.build().await.unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new function
        executor.update(to_function).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    // ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

    #[rstest]
    #[case::basic(basic_function(), false, false)]
    #[case::public_with_missing_url(function_public_ingress(), true, false)]
    #[case::public_with_missing_function(function_public_ingress(), false, true)]
    #[case::public_with_both_missing(function_public_ingress(), true, true)]
    #[case::private_with_missing_function(function_private_ingress(), false, true)]
    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing(
        #[case] function: Function,
        #[case] url_missing: bool,
        #[case] function_missing: bool,
    ) {
        let function_name = format!("test-{}", function.id);
        let has_url = function.ingress == Ingress::Public;
        let mock_lambda = setup_mock_client_for_best_effort_deletion(
            &function_name,
            url_missing,
            function_missing,
        );
        let mock_provider = setup_mock_service_provider(mock_lambda, None, None);

        // Start with a ready controller
        let mut ready_controller = AwsFunctionController::mock_ready(&function_name);
        if has_url {
            ready_controller.url = Some("https://example.lambda-url.us-east-1.on.aws/".to_string());
        }

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(ready_controller)
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the function
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even when resources are missing
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

    /// Test that verifies public functions go through ACM certificate import and API Gateway setup.
    #[tokio::test]
    async fn test_public_function_creates_api_gateway_and_certificate() {
        let function = function_public_ingress();
        let function_name = format!("test-{}", function.id);
        let domain_metadata = create_test_domain_metadata(&function.id);

        let mut mock_lambda = MockLambdaApi::new();

        // Mock function creation
        let function_name_for_create = function_name.clone();
        mock_lambda.expect_create_function().returning(move |_| {
            Ok(create_successful_function_response(
                &function_name_for_create,
            ))
        });

        let function_name_for_get = function_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&function_name_for_get)))
            .times(1);

        // Validate API Gateway permission is added with the correct apigateway principal
        mock_lambda
            .expect_add_permission()
            .withf(|_, request| {
                request.statement_id == "ApiGatewayInvoke"
                    && request.action == "lambda:InvokeFunction"
                    && request.principal == "apigateway.amazonaws.com"
            })
            .returning(|_, _| Ok(AddPermissionResponse { statement: None }));

        // Mock self-binding env var update and delete_start URL cleanup
        let function_name_for_config_update = function_name.clone();
        mock_lambda
            .expect_update_function_configuration()
            .returning(move |_, _| {
                Ok(create_successful_function_response(
                    &function_name_for_config_update,
                ))
            });

        mock_lambda
            .expect_delete_function_url_config()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Function".to_string(),
                        resource_name: "test-function".to_string(),
                    },
                ))
            });

        // Validate ACM certificate import
        let mut mock_acm = MockAcmApi::new();
        mock_acm
            .expect_import_certificate()
            .times(1)
            .returning(|_| {
                Ok(ImportCertificateResponse {
                    certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                        .to_string(),
                })
            });
        mock_acm.expect_delete_certificate().returning(|_| Ok(()));

        // Validate API Gateway is created with the function's name in the API name
        let mut mock_apigw = MockApiGatewayV2Api::new();
        mock_apigw
            .expect_create_api()
            .withf(|request| request.name.contains("public-func"))
            .returning(|_| {
                Ok(Api {
                    api_id: Some("test-api-id".to_string()),
                    api_endpoint: None,
                    name: None,
                    protocol_type: None,
                })
            });
        mock_apigw.expect_create_integration().returning(|_, _| {
            Ok(Integration {
                integration_id: Some("test-integration-id".to_string()),
                integration_type: None,
                integration_uri: None,
            })
        });
        mock_apigw.expect_create_route().returning(|_, _| {
            Ok(Route {
                route_id: Some("test-route-id".to_string()),
                route_key: None,
            })
        });
        mock_apigw.expect_create_stage().returning(|_, _| {
            Ok(Stage {
                stage_name: Some("$default".to_string()),
                auto_deploy: None,
            })
        });
        mock_apigw.expect_create_domain_name().returning(|_| {
            Ok(DomainName {
                domain_name: Some("public-func.test.example.com".to_string()),
                domain_name_configurations: Some(vec![DomainNameConfiguration {
                    certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                        .to_string(),
                    endpoint_type: "REGIONAL".to_string(),
                    security_policy: "TLS_1_2".to_string(),
                    api_gateway_domain_name: Some(
                        "test.execute-api.us-east-1.amazonaws.com".to_string(),
                    ),
                    hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
                }]),
            })
        });
        mock_apigw.expect_create_api_mapping().returning(|_, _| {
            Ok(ApiMapping {
                api_mapping_id: Some("test-mapping-id".to_string()),
                api_mapping_key: None,
                stage: None,
            })
        });
        mock_apigw
            .expect_delete_api_mapping()
            .returning(|_, _| Ok(()));
        mock_apigw.expect_delete_domain_name().returning(|_| Ok(()));
        mock_apigw.expect_delete_api().returning(|_| Ok(()));

        let mock_provider = setup_mock_service_provider(
            Arc::new(mock_lambda),
            Some(Arc::new(mock_acm)),
            Some(Arc::new(mock_apigw)),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AwsFunctionController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .domain_metadata(domain_metadata)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify URL is in outputs (derived from domain_metadata FQDN)
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<FunctionOutputs>().unwrap();
        assert!(function_outputs.url.is_some());
    }

    /// Test that verifies private functions don't get URL creation
    #[tokio::test]
    async fn test_private_function_skips_url_creation() {
        let function = function_private_ingress();
        let function_name = format!("test-{}", function.id);

        let mut mock_lambda = MockLambdaApi::new();

        // Mock function creation
        let function_name_for_create = function_name.clone();
        mock_lambda.expect_create_function().returning(move |_| {
            Ok(create_successful_function_response(
                &function_name_for_create,
            ))
        });

        let function_name_for_get = function_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&function_name_for_get)))
            .times(1);

        // URL creation and permission should NOT be called for private functions
        mock_lambda.expect_create_function_url_config().times(0);
        mock_lambda.expect_add_permission().times(0);

        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Function".to_string(),
                        resource_name: "test-function".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_lambda), None, None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AwsFunctionController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify no URL in outputs
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<FunctionOutputs>().unwrap();
        assert!(function_outputs.url.is_none());
    }

    /// Test that verifies correct function configuration parameters
    #[tokio::test]
    async fn test_function_configuration_validation() {
        let function = function_custom_config();
        let function_name = format!("test-{}", function.id);

        let mut mock_lambda = MockLambdaApi::new();

        // Validate function creation request has correct parameters
        let function_name_for_create = function_name.clone();
        mock_lambda
            .expect_create_function()
            .withf(|request| {
                request.memory_size == Some(512)
                    && request.timeout == Some(120)
                    && request.package_type == "Image"
                    && request
                        .architectures
                        .as_ref()
                        .map(|a| a.contains(&"arm64".to_string()))
                        .unwrap_or(false)
            })
            .returning(move |_| {
                Ok(create_successful_function_response(
                    &function_name_for_create,
                ))
            });

        let function_name_for_get = function_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&function_name_for_get)))
            .times(1);

        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Function".to_string(),
                        resource_name: "test-function".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_lambda), None, None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AwsFunctionController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies environment variables are correctly passed
    #[tokio::test]
    async fn test_environment_variable_handling() {
        let function = function_with_env_vars();
        let function_name = format!("test-{}", function.id);

        let mut mock_lambda = MockLambdaApi::new();

        // Validate function creation request has environment variables
        let function_name_for_create = function_name.clone();
        mock_lambda
            .expect_create_function()
            .withf(|request| {
                if let Some(env) = &request.environment {
                    if let Some(vars) = &env.variables {
                        vars.get("APP_ENV") == Some(&"production".to_string())
                            && vars.get("LOG_LEVEL") == Some(&"debug".to_string())
                            && vars.get("DB_NAME") == Some(&"myapp".to_string())
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .returning(move |_| {
                Ok(create_successful_function_response(
                    &function_name_for_create,
                ))
            });

        let function_name_for_get = function_name.clone();
        mock_lambda
            .expect_get_function_configuration()
            .returning(move |_, _| Ok(create_successful_function_response(&function_name_for_get)))
            .times(1);

        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));
        mock_lambda
            .expect_get_function_configuration()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Function".to_string(),
                        resource_name: "test-function".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_lambda), None, None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AwsFunctionController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}
