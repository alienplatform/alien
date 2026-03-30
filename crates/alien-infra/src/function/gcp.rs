use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::{split_certificate_chain, EnvironmentVariableBuilder};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::function::{run_readiness_probe, READINESS_PROBE_MAX_ATTEMPTS};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_gcp_clients::cloudrun::{
    Ingress as CloudRunIngress, NetworkInterface, RevisionTemplate, Service, TrafficTarget,
    TrafficTargetAllocationType, VpcAccess, VpcEgress,
};
use alien_gcp_clients::compute::{
    Address, AddressType, Backend, BackendService, BackendServiceProtocol, BalancingMode,
    ForwardingRule, ForwardingRuleProtocol, LoadBalancingScheme, NetworkEndpointGroup,
    NetworkEndpointGroupCloudRun, NetworkEndpointType, SslCertificate, SslCertificateSelfManaged,
    TargetHttpsProxy, UrlMap,
};
use alien_gcp_clients::longrunning::OperationResult;
use alien_gcp_clients::pubsub::{OidcToken, PushConfig, Subscription, Topic};
// Note: Role controller removed - functions now use ServiceAccount and permission profiles
use alien_core::{
    CertificateStatus, DnsRecordStatus, Function, FunctionOutputs, Ingress, Network,
    ResourceDefinition, ResourceOutputs, ResourceRef, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

/// Generates the Cloud Run service name from stack prefix and function ID
fn get_cloudrun_service_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

/// Domain information for a function.
struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    ssl_certificate_name: Option<String>,
    uses_custom_domain: bool,
}

#[controller]
pub struct GcpFunctionController {
    /// The Cloud Run service name
    pub(crate) service_name: Option<String>,
    /// The invocation URL of the function, available after creation.
    pub(crate) url: Option<String>,
    /// The operation name for long-running operations (for create, update, delete)
    pub(crate) operation_name: Option<String>,
    /// Push subscription names for queue triggers (one per queue trigger)
    pub(crate) push_subscriptions: Vec<String>,

    // Domain & Certificate
    /// The fully qualified domain name for the function
    pub(crate) fqdn: Option<String>,
    /// The certificate ID from the TLS controller
    pub(crate) certificate_id: Option<String>,
    /// The GCP SSL certificate name
    pub(crate) ssl_certificate_name: Option<String>,
    /// Whether this function uses a custom domain
    pub(crate) uses_custom_domain: bool,
    /// Timestamp when certificate was issued (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,

    // HTTPS Load Balancer components
    /// The serverless NEG name pointing to Cloud Run
    pub(crate) serverless_neg_name: Option<String>,
    /// The backend service name
    pub(crate) backend_service_name: Option<String>,
    /// The URL map name
    pub(crate) url_map_name: Option<String>,
    /// The target HTTPS proxy name
    pub(crate) target_https_proxy_name: Option<String>,
    /// The global static IP address name
    pub(crate) global_address_name: Option<String>,
    /// The forwarding rule name
    pub(crate) forwarding_rule_name: Option<String>,

    // Commands infrastructure
    /// Pub/Sub topic short name for commands delivery (without project prefix)
    pub(crate) commands_topic_name: Option<String>,
    /// Pub/Sub subscription name for commands delivery
    pub(crate) commands_subscription_name: Option<String>,
}

#[controller]
impl GcpFunctionController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Function>()?;
        info!(name=%cfg.id, "Initiating Cloud Run service creation");

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

        let gcp_config = ctx.get_gcp_config()?;
        let service_name = get_cloudrun_service_name(ctx.resource_prefix, &cfg.id);

        // Build the Cloud Run service
        let service = self
            .build_cloud_run_service(&service_name, cfg, ctx)
            .await?;

        // Create the service
        let operation = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .create_service(
                gcp_config.region.clone(),
                service_name.to_string(),
                service,
                None,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Cloud Run service".to_string(),
                resource_id: Some(cfg.id.clone()),
            })?;

        let operation_name = operation.name.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Cloud Run create operation returned without name".to_string(),
                resource_id: Some(cfg.id.clone()),
            })
        })?;

        info!(name=%service_name, operation=%operation_name, "Cloud Run service creation initiated");

        self.service_name = Some(service_name);
        self.operation_name = Some(operation_name);

        Ok(HandlerAction::Continue {
            state: CreatingService,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreatingService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_service(
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

        debug!(operation=%operation_name, "Checking operation status");

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
                    message: format!("Operation failed: {} (code: {})", error.message, error.code),
                    resource_id: Some(ctx.desired_config.id().to_string()),
                }));
            }

            // Operation succeeded, transition to next state
            info!(operation=%operation_name, "Operation completed successfully");

            Ok(HandlerAction::Continue {
                state: WaitingForServiceCreation,
                suggested_delay: None,
            })
        } else {
            // Operation still in progress.
            // Cloud Run service creation can take 2-5 minutes, especially for
            // first-time deployments that need to pull and start a container image.
            debug!(operation=%operation_name, "Operation still in progress");
            Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        }
    }

    #[handler(
        state = WaitingForServiceCreation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_service_creation(
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

        // Get the created service to extract the URL and verify readiness
        let service = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run service after creation".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        // Wait for the service to be Ready before proceeding. The create operation
        // may complete before the first revision is fully serving traffic, so the
        // Ready condition can still be false at this point.
        //
        // Cloud Run v2 API may not return a top-level "Ready" condition. When both
        // "RoutesReady" and "ConfigurationsReady" are Succeeded, the service is
        // effectively ready for traffic.
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
            // Log condition details at info level to aid debugging slow deployments
            let condition_summary: Vec<String> = service
                .conditions
                .iter()
                .map(|c| {
                    format!(
                        "{}={:?} (reason={:?}, message={})",
                        c.r#type.as_deref().unwrap_or("?"),
                        c.state,
                        c.reason,
                        c.message.as_deref().unwrap_or("")
                    )
                })
                .collect();
            info!(
                name=%service_name,
                conditions=?condition_summary,
                "Service not yet ready after creation, waiting"
            );
            // 240 attempts × ~9s (5s suggested + API latency) ≈ 36 minutes.
            // Cloud Run services that pull from cross-project Artifact Registry
            // may take 10-20 minutes while freshly-granted IAM bindings propagate.
            return Ok(HandlerAction::Stay {
                max_times: 240,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        let cloud_run_url = service.uri.or_else(|| service.urls.first().cloned());

        // Check for URL override in deployment config, otherwise use Cloud Run URL
        let config = ctx.desired_resource_config::<Function>()?;
        self.url = ctx
            .deployment_config
            .public_urls
            .as_ref()
            .and_then(|urls| urls.get(&config.id).cloned())
            .or(cloud_run_url);

        info!(name=%service_name, url=?self.url, "Cloud Run service created successfully");

        // Branch based on ingress type
        // If public, resolve domain and proceed to certificate/load balancer flow
        // If private, skip directly to push subscriptions
        if config.ingress == Ingress::Public {
            match Self::resolve_domain_info(ctx, &config.id) {
                Ok(domain_info) => {
                    info!(fqdn=%domain_info.fqdn, "Resolved domain for public function");
                    self.fqdn = Some(domain_info.fqdn);
                    self.certificate_id = domain_info.certificate_id;
                    self.ssl_certificate_name = domain_info.ssl_certificate_name;
                    self.uses_custom_domain = domain_info.uses_custom_domain;

                    // Proceed to certificate flow
                    return Ok(HandlerAction::Continue {
                        state: WaitingForCertificate,
                        suggested_delay: None,
                    });
                }
                Err(_) => {
                    // Standalone mode: no domain metadata available.
                    // The Cloud Run service URL is already set from the service
                    // creation response and is publicly accessible. Skip the
                    // custom domain / certificate / load balancer flow.
                    info!(
                        function=%config.id,
                        url=?self.url,
                        "No domain metadata — skipping custom domain setup (standalone mode)"
                    );
                }
            }
        }

        // Always go to CreatingPushSubscriptions next (linear flow)
        Ok(HandlerAction::Continue {
            state: CreatingPushSubscriptions,
            suggested_delay: None,
        })
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
                state: ImportingSslCertificate,
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
        state = ImportingSslCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_ssl_certificate(
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

        // For GCP, we use the full certificate chain
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let ssl_cert_name = format!("{}-{}-cert", ctx.resource_prefix, function_config.id);

        let ssl_certificate = SslCertificate::builder()
            .name(ssl_cert_name.clone())
            .description(format!(
                "SSL certificate for function {}",
                function_config.id
            ))
            .r#type("SELF_MANAGED".to_string())
            .self_managed(
                SslCertificateSelfManaged::builder()
                    .certificate(certificate_chain.clone())
                    .private_key(private_key.clone())
                    .build(),
            )
            .build();

        compute_client
            .insert_ssl_certificate(ssl_certificate)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import SSL certificate to GCP".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.ssl_certificate_name = Some(ssl_cert_name);

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        info!(
            function=%function_config.id,
            cert_name=%self.ssl_certificate_name.as_ref().unwrap(),
            "SSL certificate imported to GCP"
        );

        Ok(HandlerAction::Continue {
            state: CreatingServerlessNeg,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingServerlessNeg,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let function_config = ctx.desired_resource_config::<Function>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "Service name not set".to_string(),
            })
        })?;

        let neg_name = format!("{}-{}-neg", ctx.resource_prefix, function_config.id);

        // Create serverless NEG pointing to Cloud Run service
        // According to GCP API: https://docs.cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups
        // For serverless NEGs, we must specify cloud_run, app_engine, or cloud_function
        let cloud_run_config = NetworkEndpointGroupCloudRun::builder()
            .service(service_name.clone())
            .build();

        let neg = NetworkEndpointGroup::builder()
            .name(neg_name.clone())
            .description(format!(
                "Serverless NEG for function {}",
                function_config.id
            ))
            .network_endpoint_type(NetworkEndpointType::Serverless)
            .cloud_run(cloud_run_config)
            .build();

        compute_client
            .insert_region_network_endpoint_group(gcp_config.region.clone(), neg)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create serverless NEG".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.serverless_neg_name = Some(neg_name);

        info!(
            function=%function_config.id,
            neg_name=%self.serverless_neg_name.as_ref().unwrap(),
            "Serverless NEG created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingBackendService,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreatingBackendService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let function_config = ctx.desired_resource_config::<Function>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let neg_name = self.serverless_neg_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "Serverless NEG name not set".to_string(),
            })
        })?;

        let backend_service_name =
            format!("{}-{}-backend", ctx.resource_prefix, function_config.id);

        let neg_url = format!(
            "projects/{}/regions/{}/networkEndpointGroups/{}",
            gcp_config.project_id, gcp_config.region, neg_name
        );

        // Create backend service with serverless NEG (no health check for serverless)
        let backend_service = BackendService::builder()
            .name(backend_service_name.clone())
            .description(format!(
                "Backend service for function {}",
                function_config.id
            ))
            .protocol(BackendServiceProtocol::Https)
            .load_balancing_scheme(LoadBalancingScheme::External)
            .backends(vec![Backend::builder()
                .group(neg_url)
                .balancing_mode(BalancingMode::Utilization)
                .build()])
            .build();

        compute_client
            .insert_backend_service(backend_service)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create backend service".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.backend_service_name = Some(backend_service_name);

        info!(
            function=%function_config.id,
            backend_service_name=%self.backend_service_name.as_ref().unwrap(),
            "Backend service created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingUrlMap,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreatingUrlMap,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let function_config = ctx.desired_resource_config::<Function>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let backend_service_name = self.backend_service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "Backend service name not set".to_string(),
            })
        })?;

        let url_map_name = format!("{}-{}-urlmap", ctx.resource_prefix, function_config.id);

        let backend_service_url = format!(
            "projects/{}/global/backendServices/{}",
            gcp_config.project_id, backend_service_name
        );

        // Create URL map routing to backend service
        let url_map = UrlMap::builder()
            .name(url_map_name.clone())
            .description(format!("URL map for function {}", function_config.id))
            .default_service(backend_service_url)
            .build();

        compute_client
            .insert_url_map(url_map)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create URL map".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.url_map_name = Some(url_map_name);

        info!(
            function=%function_config.id,
            url_map_name=%self.url_map_name.as_ref().unwrap(),
            "URL map created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingTargetHttpsProxy,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreatingTargetHttpsProxy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let function_config = ctx.desired_resource_config::<Function>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let url_map_name = self.url_map_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "URL map name not set".to_string(),
            })
        })?;

        let ssl_cert_name = self.ssl_certificate_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "SSL certificate name not set".to_string(),
            })
        })?;

        let proxy_name = format!("{}-{}-https-proxy", ctx.resource_prefix, function_config.id);

        let url_map_url = format!(
            "projects/{}/global/urlMaps/{}",
            gcp_config.project_id, url_map_name
        );

        let ssl_cert_url = format!(
            "projects/{}/global/sslCertificates/{}",
            gcp_config.project_id, ssl_cert_name
        );

        // Create HTTPS proxy with SSL certificate
        let https_proxy = TargetHttpsProxy::builder()
            .name(proxy_name.clone())
            .description(format!("HTTPS proxy for function {}", function_config.id))
            .url_map(url_map_url)
            .ssl_certificates(vec![ssl_cert_url])
            .build();

        compute_client
            .insert_target_https_proxy(https_proxy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create target HTTPS proxy".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.target_https_proxy_name = Some(proxy_name);

        info!(
            function=%function_config.id,
            proxy_name=%self.target_https_proxy_name.as_ref().unwrap(),
            "Target HTTPS proxy created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingGlobalAddress,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreatingGlobalAddress,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let function_config = ctx.desired_resource_config::<Function>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let address_name = format!("{}-{}-ip", ctx.resource_prefix, function_config.id);

        // Create global static IP address
        let address = Address::builder()
            .name(address_name.clone())
            .description(format!("Global IP for function {}", function_config.id))
            .address_type(AddressType::External)
            .build();

        compute_client
            .insert_global_address(address)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create global address".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.global_address_name = Some(address_name);

        info!(
            function=%function_config.id,
            address_name=%self.global_address_name.as_ref().unwrap(),
            "Global address created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingForwardingRule,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreatingForwardingRule,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let function_config = ctx.desired_resource_config::<Function>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let proxy_name = self.target_https_proxy_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "Target HTTPS proxy name not set".to_string(),
            })
        })?;

        let address_name = self.global_address_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "Global address name not set".to_string(),
            })
        })?;

        // Get the IP address
        let address = compute_client
            .get_global_address(address_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get global address".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        let ip_address = address.address.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Global address has no IP".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;

        let forwarding_rule_name = format!("{}-{}-https", ctx.resource_prefix, function_config.id);

        let proxy_url = format!(
            "projects/{}/global/targetHttpsProxies/{}",
            gcp_config.project_id, proxy_name
        );

        // Create forwarding rule exposing HTTPS endpoint
        let forwarding_rule = ForwardingRule::builder()
            .name(forwarding_rule_name.clone())
            .description(format!(
                "Forwarding rule for function {}",
                function_config.id
            ))
            .ip_address(ip_address)
            .ip_protocol(ForwardingRuleProtocol::Tcp)
            .port_range("443-443".to_string())
            .target(proxy_url)
            .load_balancing_scheme(LoadBalancingScheme::External)
            .build();

        compute_client
            .insert_global_forwarding_rule(forwarding_rule)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create forwarding rule".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.forwarding_rule_name = Some(forwarding_rule_name);

        info!(
            function=%function_config.id,
            forwarding_rule_name=%self.forwarding_rule_name.as_ref().unwrap(),
            "Forwarding rule created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForDns,
            suggested_delay: Some(Duration::from_secs(2)),
        })
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
            Some(DnsRecordStatus::Active) => {
                info!(
                    function=%function_config.id,
                    fqdn=%self.fqdn.as_ref().unwrap_or(&"unknown".to_string()),
                    "DNS record created successfully"
                );
                Ok(HandlerAction::Continue {
                    state: CreatingPushSubscriptions,
                    suggested_delay: None,
                })
            }
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
        state = CreatingPushSubscriptions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Function>()?;
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
            if let alien_core::FunctionTrigger::Queue { queue } = trigger {
                info!(function=%cfg.id, queue=%queue.id, "Creating Pub/Sub push subscription");
                self.create_push_subscription(ctx, gcp_config, &service_name, &cfg, queue)
                    .await?;
                created_any = true;
            }
        }

        if !created_any {
            info!(function=%cfg.id, "No queue triggers found, skipping push subscription creation");
        }

        // Go to commands infrastructure next
        Ok(HandlerAction::Continue {
            state: CreatingCommandsInfrastructure,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingCommandsInfrastructure,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Function>()?;

        if !cfg.commands_enabled {
            debug!(function=%cfg.id, "Commands not enabled, skipping commands infrastructure");
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

        info!(
            function=%cfg.id,
            topic=%topic_full_name,
            "Creating commands Pub/Sub topic"
        );

        pubsub_client
            .create_topic(topic_short_name.clone(), Topic::default())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create commands Pub/Sub topic '{}'",
                    topic_short_name
                ),
                resource_id: Some(cfg.id.clone()),
            })?;

        // Create push subscription that delivers to the Cloud Run service
        let subscription_name = format!("{}-rq-sub", service_name);
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: cfg.id.clone(),
                message: "Service URL not available for commands push subscription".to_string(),
            })
        })?;

        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Only use OIDC authentication on the push subscription when the function
        // is private. Public functions have invoker_iam_disabled=true on the Cloud
        // Run service, so PubSub can deliver without authentication. Using OIDC on
        // public functions would require the PubSub service agent to have
        // roles/iam.serviceAccountTokenCreator on the execution SA, which adds
        // unnecessary complexity.
        let oidc_token = if cfg.ingress != Ingress::Public {
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
                ("alien-commands".to_string(), cfg.id.clone()),
                ("alien-stack".to_string(), ctx.resource_prefix.to_string()),
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

        info!(
            function=%cfg.id,
            topic=%topic_full_name,
            subscription=%subscription_name,
            endpoint=%push_endpoint,
            "Creating commands Pub/Sub push subscription"
        );

        pubsub_client
            .create_subscription(subscription_name.clone(), subscription)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create commands push subscription '{}'",
                    subscription_name
                ),
                resource_id: Some(cfg.id.clone()),
            })?;

        self.commands_topic_name = Some(topic_short_name);
        self.commands_subscription_name = Some(subscription_name);

        info!(function=%cfg.id, "Commands Pub/Sub infrastructure created");

        Ok(HandlerAction::Continue {
            state: SettingIamPolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = SettingIamPolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn setting_iam_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Function>()?;
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

    #[handler(
        state = RunningReadinessProbe,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn running_readiness_probe(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Function>()?;

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
                            max_times: READINESS_PROBE_MAX_ATTEMPTS,
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
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_cloudrun_client(gcp_config)?;
        let function_config = ctx.desired_resource_config::<Function>()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        // Heartbeat check: verify service still exists and is in correct state
        let service = client
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run service during heartbeat check".to_string(),
                resource_id: Some(function_config.id.clone()),
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
            warn!(name=%function_config.id, "Cloud Run service is not in Ready state during heartbeat");
            let mut err = AlienError::new(ErrorData::ResourceDrift {
                resource_id: function_config.id.clone(),
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
                            let expected_memory = format!("{}Mi", function_config.memory_mb);
                            if current_memory != &expected_memory {
                                return Err(AlienError::new(ErrorData::ResourceDrift {
                                    resource_id: function_config.id.clone(),
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

        // Check for certificate renewal (if using custom domain)
        if self.uses_custom_domain && self.certificate_id.is_some() {
            let metadata = ctx
                .deployment_config
                .domain_metadata
                .as_ref()
                .and_then(|meta| meta.resources.get(&function_config.id));

            if let Some(resource) = metadata {
                // Check if certificate has been renewed (issued_at timestamp changed)
                if let Some(new_issued_at) = &resource.issued_at {
                    if self.certificate_issued_at.as_ref() != Some(new_issued_at) {
                        info!(
                            function=%function_config.id,
                            old_issued_at=?self.certificate_issued_at,
                            new_issued_at=%new_issued_at,
                            "Certificate renewed, triggering update to re-import certificate"
                        );
                        return Ok(HandlerAction::Continue {
                            state: UpdateStart,
                            suggested_delay: None,
                        });
                    }
                }
            }
        }

        debug!(name = %function_config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)), // Check again in 30 seconds
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
        let cfg = ctx.desired_resource_config::<Function>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Service name not set in state".to_string(),
            })
        })?;

        info!(name=%service_name, "Starting Cloud Run service update");

        // Get current service to preserve etag
        let current_service = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run service for update".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        // Build updated service configuration
        let mut updated_service = self.build_cloud_run_service(service_name, cfg, ctx).await?;

        // Preserve important fields from current service
        updated_service.name = current_service.name;
        updated_service.etag = current_service.etag;

        // Patch the service
        let operation = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .patch_service(
                gcp_config.region.clone(),
                service_name.clone(),
                updated_service,
                None, // update_mask - let the API figure it out
                None, // validate_only
                None, // allow_missing
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to patch Cloud Run service".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        let operation_name = operation.name.ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Cloud Run update operation returned without name".to_string(),
            })
        })?;

        info!(name=%service_name, operation=%operation_name, "Cloud Run service update initiated");

        self.operation_name = Some(operation_name);

        Ok(HandlerAction::Continue {
            state: UpdatingService,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = UpdatingService,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_service(
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

        debug!(operation=%operation_name, "Checking update operation status");

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
                        "Update operation failed: {} (code: {})",
                        error.message, error.code
                    ),
                    resource_id: Some(ctx.desired_config.id().to_string()),
                }));
            }

            // Operation succeeded, transition to next state
            info!(operation=%operation_name, "Update operation completed successfully");

            Ok(HandlerAction::Continue {
                state: WaitingForServiceUpdate,
                suggested_delay: None,
            })
        } else {
            // Operation still in progress
            debug!(operation=%operation_name, "Update operation still in progress");
            Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        }
    }

    #[handler(
        state = WaitingForServiceUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_service_update(
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

        // Get the updated service
        let service = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .get_service(gcp_config.region.clone(), service_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Cloud Run service after update".to_string(),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })?;

        // Check if the service is ready. Cloud Run v2 may not return a "Ready"
        // condition, so also accept both sub-conditions as Succeeded.
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
            debug!(name=%service_name, "Service not yet ready after update");
            return Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        info!(name=%service_name, "Cloud Run service updated successfully");

        // Always go to updating push subscriptions next (linear flow)
        Ok(HandlerAction::Continue {
            state: UpdatePushSubscriptions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatePushSubscriptions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Function>()?;
        let previous_config = ctx.previous_resource_config::<Function>()?;
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
            info!(function=%current_config.id, "Function triggers changed, updating push subscriptions");

            // For simplicity, we'll delete old subscriptions and create new ones
            // In a production system, you might want to do a more sophisticated diff
            self.delete_all_push_subscriptions(ctx, gcp_config).await?;

            // Create new subscriptions for ALL queue triggers
            for trigger in &current_config.triggers {
                if let alien_core::FunctionTrigger::Queue { queue } = trigger {
                    self.create_push_subscription(
                        ctx,
                        gcp_config,
                        &service_name,
                        &current_config,
                        queue,
                    )
                    .await?;
                }
            }
        } else {
            info!(function=%current_config.id, "No trigger changes detected");
        }

        // Always go to readiness probe next (linear flow)
        Ok(HandlerAction::Continue {
            state: UpdateRunningReadinessProbe,
            suggested_delay: None,
        })
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
        let cfg = ctx.desired_resource_config::<Function>()?;

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
                            max_times: READINESS_PROBE_MAX_ATTEMPTS,
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
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
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

    #[handler(
        state = DeletingForwardingRule,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;

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
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(name=%forwarding_rule_name, "Forwarding rule was already deleted");
                }
                Err(e) => {
                    warn!(name=%forwarding_rule_name, error=%e, "Failed to delete forwarding rule (continuing)");
                }
            }

            self.forwarding_rule_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingTargetHttpsProxy,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingTargetHttpsProxy,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_target_https_proxy(
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
                Err(e) => {
                    warn!(name=%proxy_name, error=%e, "Failed to delete target HTTPS proxy (continuing)");
                }
            }

            self.target_https_proxy_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingUrlMap,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingUrlMap,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_url_map(
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
                    warn!(name=%url_map_name, error=%e, "Failed to delete URL map (continuing)");
                }
            }

            self.url_map_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingBackendService,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingBackendService,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_backend_service(
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
                    warn!(name=%backend_service_name, error=%e, "Failed to delete backend service (continuing)");
                }
            }

            self.backend_service_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingServerlessNeg,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingServerlessNeg,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_serverless_neg(
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
                Err(e) => {
                    warn!(name=%neg_name, error=%e, "Failed to delete serverless NEG (continuing)");
                }
            }

            self.serverless_neg_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingSslCertificate,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingSslCertificate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_ssl_certificate(
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
                    warn!(name=%ssl_cert_name, error=%e, "Failed to delete SSL certificate (continuing)");
                }
            }

            self.ssl_certificate_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingGlobalAddress,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = DeletingGlobalAddress,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_global_address(
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
                    warn!(name=%address_name, error=%e, "Failed to delete global address (continuing)");
                }
            }

            self.global_address_name = None;
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

    #[handler(
        state = DeletingPushSubscriptions,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let function_config = ctx.desired_resource_config::<Function>()?;

        info!(function=%function_config.id, subscriptions=?self.push_subscriptions, "Deleting push subscriptions");

        // Delete all push subscriptions using best-effort approach (ignore NotFound)
        self.delete_all_push_subscriptions(ctx, gcp_config).await?;

        // Continue to commands infrastructure cleanup
        Ok(HandlerAction::Continue {
            state: DeletingCommandsInfrastructure,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingCommandsInfrastructure,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;

        // Delete commands subscription (best-effort)
        if let Some(subscription_name) = self.commands_subscription_name.take() {
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
        if let Some(topic_name) = self.commands_topic_name.take() {
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

    #[handler(
        state = DeletingService,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_service(
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

    #[handler(
        state = WaitingForDeleteOperation,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_delete_operation(
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
                max_times: 20,
                suggested_delay: Some(Duration::from_secs(3)),
            })
        }
    }

    #[handler(
        state = WaitingForServiceDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_service_deletion(
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
                    max_times: 20,
                    suggested_delay: Some(Duration::from_secs(3)),
                })
            }
        }
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
        self.url.as_ref().map(|url| {
            // If we have a custom domain with HTTPS load balancer, use the FQDN
            // Otherwise, use the Cloud Run URL
            let load_balancer_endpoint = if let Some(fqdn) = &self.fqdn {
                Some(alien_core::LoadBalancerEndpoint {
                    dns_name: fqdn.clone(),
                    hosted_zone_id: None, // GCP doesn't use hosted zones like AWS
                })
            } else {
                Some(alien_core::LoadBalancerEndpoint {
                    dns_name: url.clone(),
                    hosted_zone_id: None,
                })
            };

            ResourceOutputs::new(FunctionOutputs {
                // Use the service name if available, otherwise fall back to a placeholder
                function_name: self
                    .service_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                url: Some(url.clone()),
                identifier: self.service_name.clone(),
                load_balancer_endpoint,
                commands_push_target: self.commands_topic_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, CloudRunFunctionBinding, FunctionBinding};

        if let (Some(service_name), Some(url)) = (&self.service_name, &self.url) {
            // Extract project ID and location from service name or URL
            // For now, we'll use defaults since we don't store these separately
            let binding = FunctionBinding::CloudRun(CloudRunFunctionBinding {
                project_id: BindingValue::Value("unknown-project".to_string()), // TODO: Store in controller
                service_name: BindingValue::Value(service_name.clone()),
                location: BindingValue::Value("unknown-region".to_string()), // TODO: Store in controller
                private_url: BindingValue::Value(url.clone()),
                public_url: Some(BindingValue::Value(url.clone())),
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

// Separate impl block for helper methods
impl GcpFunctionController {
    // ─────────────── HELPER METHODS ────────────────────────────

    /// Resolve domain information for a public function.
    /// Returns either custom domain config or auto-generated domain from metadata.
    fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<DomainInfo> {
        let stack_settings = &ctx.deployment_config.stack_settings;

        // Check for custom domain configuration
        if let Some(custom) = stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id))
        {
            let ssl_cert_name = custom
                .certificate
                .gcp
                .as_ref()
                .map(|cert| cert.certificate_name.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires a GCP SSL certificate name".to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                ssl_certificate_name: Some(ssl_cert_name),
                uses_custom_domain: true,
            });
        }

        // Use auto-generated domain from domain metadata
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
            ssl_certificate_name: None,
            uses_custom_domain: false,
        })
    }

    async fn build_cloud_run_service(
        &self,
        service_name: &str,
        cfg: &Function,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Service> {
        use alien_gcp_clients::cloudrun::{
            Container, ContainerPort, EnvVar, ResourceRequirements, Service,
        };

        // Get the ServiceAccount for this function's permission profile
        let service_account_id = format!("{}-sa", cfg.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        // Get the ServiceAccount's email
        let service_account_state = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &service_account_ref,
            )?;

        let service_account = service_account_state
            .service_account_email
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: cfg.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();
        let service_account = Some(service_account);

        // Extract container image
        let image = match &cfg.code {
            alien_core::FunctionCode::Image { image } => image.clone(),
            alien_core::FunctionCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Function '{}' is configured with source code, but only pre-built images are supported in alien-infra.", cfg.id),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        };

        // Prepare environment variables
        let env_vars = self
            .prepare_environment_variables(&cfg.environment, &cfg.links, ctx, service_name)
            .await?;

        let env: Vec<EnvVar> = env_vars
            .into_iter()
            .map(|(name, value)| EnvVar {
                name,
                value: Some(value),
                value_source: None,
            })
            .collect();

        // Build resource requirements
        let mut limits = HashMap::new();
        limits.insert("memory".to_string(), format!("{}Mi", cfg.memory_mb));
        // Cloud Run automatically allocates CPU based on memory

        let resources = ResourceRequirements {
            limits: Some(limits),
            cpu_idle: Some(true),          // Allow CPU throttling when idle
            startup_cpu_boost: Some(true), // Boost CPU during startup
        };

        // Build container port
        let ports = vec![ContainerPort {
            name: Some("http1".to_string()),
            // NOTE: This must match the alien-runtime port on alien-build/src/lib.rs
            container_port: Some(8080),
        }];

        // Build container
        let container = Container::builder()
            .name("function".to_string())
            .image(image)
            .env(env)
            .resources(resources)
            .ports(ports)
            .build();

        // Map ingress settings
        let ingress = match cfg.ingress {
            Ingress::Public => CloudRunIngress::IngressTrafficAll,
            Ingress::Private => CloudRunIngress::IngressTrafficInternal,
        };

        // Get VPC access configuration if a Network resource exists
        let vpc_access = self.get_vpc_access(ctx)?;
        if vpc_access.is_some() {
            info!(name=%service_name, "Configuring Cloud Run service with Direct VPC Egress");
        }

        // Build revision template
        let template = RevisionTemplate::builder()
            .labels(HashMap::from([(
                "alien-function".to_string(),
                cfg.id.clone(),
            )]))
            .scaling(
                alien_gcp_clients::cloudrun::RevisionScaling::builder()
                    .min_instance_count(0) // Scale to zero
                    .maybe_max_instance_count(cfg.concurrency_limit.map(|c| c as i32))
                    .build(),
            )
            .timeout(format!("{}s", cfg.timeout_seconds))
            .maybe_service_account(service_account)
            .containers(vec![container])
            .execution_environment(
                alien_gcp_clients::cloudrun::ExecutionEnvironment::ExecutionEnvironmentGen2,
            )
            .max_instance_request_concurrency(1000) // Cloud Run default
            .maybe_vpc_access(vpc_access)
            .build();

        // Build traffic target
        let traffic = vec![TrafficTarget::builder()
            .r#type(TrafficTargetAllocationType::TrafficTargetAllocationTypeLatest)
            .percent(100)
            .build()];

        // Build service
        // When ingress is public, disable the IAM invoker check instead of adding
        // allUsers to IAM policy. This works even when the GCP organization has
        // domain-restricted sharing enabled (which blocks allUsers in IAM).
        let is_public = cfg.ingress == Ingress::Public;
        let service = Service::builder()
            .description(format!("Alien function: {}", cfg.id))
            .labels(HashMap::from([
                ("alien-resource".to_string(), "function".to_string()),
                ("alien-function-id".to_string(), cfg.id.clone()),
                ("alien-stack".to_string(), ctx.resource_prefix.to_string()),
            ]))
            .ingress(ingress)
            .template(template)
            .traffic(traffic)
            .invoker_iam_disabled(is_public)
            .build();

        Ok(service)
    }

    /// Gets VPC access configuration from the Network resource if one exists in the stack.
    ///
    /// If a Network resource exists (ID: "default-network"), this method retrieves
    /// the network name and subnetwork name from the Network controller to configure
    /// the Cloud Run service with Direct VPC Egress.
    ///
    /// Returns `None` if no Network resource exists in the stack.
    fn get_vpc_access(&self, ctx: &ResourceControllerContext<'_>) -> Result<Option<VpcAccess>> {
        // Check if the stack has a Network resource
        let network_id = "default-network";
        if !ctx.desired_stack.resources.contains_key(network_id) {
            return Ok(None);
        }

        // Get the Network controller state via require_dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, network_id.to_string());
        let network_state =
            ctx.require_dependency::<crate::network::GcpNetworkController>(&network_ref)?;

        // Only configure VPC access if we have network and subnetwork names
        let network_name = match &network_state.network_name {
            Some(name) => name.clone(),
            None => return Ok(None),
        };

        let subnetwork_name = match &network_state.subnetwork_name {
            Some(name) => name.clone(),
            None => return Ok(None),
        };

        // Build Direct VPC Egress configuration using network interfaces
        let network_interface = NetworkInterface::builder()
            .network(network_name)
            .subnetwork(subnetwork_name)
            .build();

        Ok(Some(
            VpcAccess::builder()
                .egress(VpcEgress::AllTraffic)
                .network_interfaces(vec![network_interface])
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
        use crate::core::ResourceController;
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

    /// Applies consolidated IAM policy (resource-scoped permissions + public access) in a single operation
    async fn apply_consolidated_iam_policy(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_name: &str,
        enable_public_access: bool,
    ) -> Result<()> {
        use alien_gcp_clients::iam::Binding;

        let config = ctx.desired_resource_config::<Function>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_cloudrun_client(gcp_config)?;

        // Get existing IAM policy to preserve any existing bindings
        let mut policy = client
            .get_service_iam_policy(gcp_config.region.clone(), service_name.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get IAM policy for Cloud Run service '{}' before applying bindings. Refusing to proceed to avoid overwriting existing bindings.", service_name),
                resource_id: Some(config.id.clone()),
            })?;

        // Step 1: Apply resource-scoped permissions from the stack
        let mut resource_bindings = Vec::new();
        self.collect_resource_scoped_bindings(ctx, service_name, &mut resource_bindings)
            .await?;

        // Step 2: Add public access binding if needed
        if enable_public_access {
            info!(service_name = %service_name, "Adding public access to IAM policy");
            let invoker_role = "roles/run.invoker".to_string();
            let all_users_member = "allUsers".to_string();

            // Check if binding already exists
            let binding_exists = policy
                .bindings
                .iter()
                .any(|b| b.role == invoker_role && b.members.contains(&all_users_member));

            if !binding_exists {
                // Find existing binding or create new one
                if let Some(binding) = policy.bindings.iter_mut().find(|b| b.role == invoker_role) {
                    if !binding.members.contains(&all_users_member) {
                        binding.members.push(all_users_member);
                    }
                } else {
                    policy.bindings.push(
                        Binding::builder()
                            .role(invoker_role)
                            .members(vec![all_users_member])
                            .build(),
                    );
                }
            }
        }

        // Step 3: Add resource-scoped bindings
        if !resource_bindings.is_empty() {
            info!(
                service_name = %service_name,
                bindings_count = resource_bindings.len(),
                "Adding resource-scoped permissions to IAM policy"
            );
            policy.bindings.extend(resource_bindings);
        }

        // Step 4: Apply the consolidated policy in one operation
        client
            .set_service_iam_policy(gcp_config.region.clone(), service_name.to_string(), policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to apply consolidated IAM policy to Cloud Run service '{}'",
                    service_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(service_name = %service_name, "Consolidated IAM policy applied successfully");
        Ok(())
    }

    /// Collect resource-scoped bindings without applying them
    async fn collect_resource_scoped_bindings(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_name: &str,
        all_bindings: &mut Vec<alien_gcp_clients::iam::Binding>,
    ) -> Result<()> {
        use alien_permissions::{generators::GcpRuntimePermissionsGenerator, PermissionContext};

        let config = ctx.desired_resource_config::<Function>()?;
        let gcp_config = ctx.get_gcp_config()?;

        // Build permission context for this specific function resource
        let mut permission_context = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(service_name.to_string());
        if let Some(ref project_number) = gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }

        let generator = GcpRuntimePermissionsGenerator::new();
        let type_prefix = "function/";

        // Process each permission profile in the stack
        for (profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            // Combine resource-specific permissions with matching wildcard permissions
            let mut combined_refs: Vec<alien_core::permissions::PermissionSetReference> =
                Vec::new();

            if let Some(permission_set_refs) = profile.0.get(&config.id) {
                combined_refs.extend(permission_set_refs.iter().cloned());
            }

            if let Some(wildcard_refs) = profile.0.get("*") {
                combined_refs.extend(
                    wildcard_refs
                        .iter()
                        .filter(|r| r.id().starts_with(type_prefix))
                        .cloned(),
                );
            }

            if !combined_refs.is_empty() {
                info!(
                    service_name = %service_name,
                    profile = %profile_name,
                    permission_sets = ?combined_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                    "Processing resource-scoped permissions for function"
                );

                self.process_profile_permissions(
                    ctx,
                    profile_name,
                    &combined_refs,
                    &generator,
                    &permission_context,
                    all_bindings,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to process permissions for profile '{}' on function '{}'",
                        profile_name, service_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            }
        }

        // Process management SA permissions matching the function resource type
        if let Some(management_profile) = ctx.desired_stack.management().profile() {
            let mut management_refs: Vec<alien_core::permissions::PermissionSetReference> =
                Vec::new();

            if let Some(permission_set_refs) = management_profile.0.get(&config.id) {
                management_refs.extend(permission_set_refs.iter().cloned());
            }

            if let Some(wildcard_refs) = management_profile.0.get("*") {
                management_refs.extend(
                    wildcard_refs
                        .iter()
                        .filter(|r| r.id().starts_with(type_prefix))
                        .cloned(),
                );
            }

            if !management_refs.is_empty() {
                use crate::core::ResourcePermissionsHelper;
                ResourcePermissionsHelper::collect_gcp_management_bindings_for(
                    ctx,
                    &config.id,
                    service_name,
                    &management_refs,
                    &generator,
                    &permission_context,
                    all_bindings,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Process permissions for a specific profile
    async fn process_profile_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
        permission_set_refs: &[alien_core::permissions::PermissionSetReference],
        generator: &alien_permissions::generators::GcpRuntimePermissionsGenerator,
        permission_context: &alien_permissions::PermissionContext,
        all_bindings: &mut Vec<alien_gcp_clients::iam::Binding>,
    ) -> Result<()> {
        use alien_gcp_clients::iam::{Binding, Expr};
        use alien_permissions::BindingTarget;

        // Get the service account email for this profile
        let service_account_email =
            self.get_service_account_email_for_profile(ctx, profile_name)?;

        // Process each permission set for this resource
        for permission_set_ref in permission_set_refs {
            let permission_set = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Permission set '{}' not found", permission_set_ref.id()),
                        resource_id: Some(profile_name.to_string()),
                    })
                })?;

            // Generate IAM bindings for resource-scoped permissions
            let bindings_result = generator
                .generate_bindings(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate bindings for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;

            // Convert and add bindings
            let member = format!("serviceAccount:{}", service_account_email);
            for binding in bindings_result.bindings {
                all_bindings.push(Binding {
                    role: binding.role,
                    members: vec![member.clone()],
                    condition: binding.condition.map(|cond| Expr {
                        title: Some(cond.title),
                        description: Some(cond.description),
                        expression: cond.expression,
                        location: None,
                    }),
                });
            }
        }

        Ok(())
    }

    /// Get the service account email for a permission profile
    fn get_service_account_email_for_profile(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", profile_name);
        let service_account_resource = ctx
            .desired_stack
            .resources
            .get(&service_account_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Service account resource '{}' not found for profile '{}'",
                        service_account_id, profile_name
                    ),
                    resource_id: Some(profile_name.to_string()),
                })
            })?;

        let service_account_controller = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &(&service_account_resource.config).into(),
            )?;

        service_account_controller
            .service_account_email
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: "function".to_string(),
                    dependency_id: profile_name.to_string(),
                })
            })
    }

    /// Creates a Pub/Sub push subscription for a queue trigger
    async fn create_push_subscription(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
        _service_name: &str,
        function_config: &alien_core::Function,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<()> {
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;

        // Get queue controller to access the topic name
        let queue_controller =
            ctx.require_dependency::<crate::queue::gcp::GcpQueueController>(queue_ref)?;
        let topic_name = queue_controller.topic_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: function_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;

        // Generate push subscription name: stack-prefix-function-id-queue-id
        let subscription_name = format!(
            "{}-{}-{}",
            ctx.resource_prefix, function_config.id, queue_ref.id
        );

        // Get the service URL for push endpoint
        let service_url = self.url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "Service URL not available for push subscription".to_string(),
            })
        })?;

        // Build push endpoint URL (Cloud Run service URL)
        let push_endpoint = format!("{}/", service_url.trim_end_matches('/'));

        // Get service account email for OIDC authentication
        let service_account_id = format!("{}-sa", function_config.get_permissions());
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
                    resource_id: function_config.id().to_string(),
                    dependency_id: service_account_id.to_string(),
                })
            })?
            .to_string();

        // Create push config with OIDC authentication
        let oidc_token = OidcToken {
            service_account_email: service_account_email.clone(),
            audience: Some(push_endpoint.clone()),
        };

        let push_config = PushConfig {
            push_endpoint: Some(push_endpoint.clone()),
            attributes: Some(std::collections::HashMap::new()),
            oidc_token: Some(oidc_token),
            pubsub_wrapper: None,
            no_wrapper: None,
        };

        let subscription = Subscription {
            name: Some(subscription_name.clone()),
            topic: Some(topic_name.clone()),
            push_config: Some(push_config),
            ack_deadline_seconds: Some(function_config.timeout_seconds as i32),
            retain_acked_messages: Some(false),
            message_retention_duration: None,
            labels: Some(std::collections::HashMap::from([
                ("alien-function".to_string(), function_config.id.clone()),
                ("alien-stack".to_string(), ctx.resource_prefix.to_string()),
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

        info!(
            function=%function_config.id,
            topic=%topic_name,
            subscription=%subscription_name,
            endpoint=%push_endpoint,
            "Creating Pub/Sub push subscription"
        );

        pubsub_client
            .create_subscription(subscription_name.clone(), subscription)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create push subscription '{}' for queue '{}'",
                    subscription_name, queue_ref.id
                ),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.push_subscriptions.push(subscription_name.clone());

        info!(
            function=%function_config.id,
            subscription=%subscription_name,
            "Successfully created Pub/Sub push subscription"
        );

        Ok(())
    }

    /// Deletes all push subscriptions using best-effort approach
    async fn delete_all_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        gcp_config: &alien_gcp_clients::GcpClientConfig,
    ) -> Result<()> {
        if self.push_subscriptions.is_empty() {
            return Ok(());
        }

        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let function_config = ctx.desired_resource_config::<Function>()?;

        for subscription_name in &self.push_subscriptions.clone() {
            match pubsub_client
                .delete_subscription(subscription_name.clone())
                .await
            {
                Ok(_) => {
                    info!(
                        function=%function_config.id,
                        subscription=%subscription_name,
                        "Push subscription deleted successfully"
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        function=%function_config.id,
                        subscription=%subscription_name,
                        "Push subscription was already deleted (not found)"
                    );
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete push subscription '{}'",
                            subscription_name
                        ),
                        resource_id: Some(function_config.id.clone()),
                    }));
                }
            }
        }

        self.push_subscriptions.clear();
        Ok(())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(function_name: &str) -> Self {
        Self {
            state: GcpFunctionState::Ready,
            service_name: Some(function_name.to_string()),
            url: Some(format!("https://{}-abcd1234-uc.a.run.app", function_name)),
            operation_name: None,
            push_subscriptions: Vec::new(),
            fqdn: None,
            certificate_id: None,
            ssl_certificate_name: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            serverless_neg_name: None,
            backend_service_name: None,
            url_map_name: None,
            target_https_proxy_name: None,
            global_address_name: None,
            forwarding_rule_name: None,
            commands_topic_name: None,
            commands_subscription_name: None,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # GCP Function Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::collections::HashMap;
    use std::sync::Arc;

    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{
        CertificateStatus, DnsRecordStatus, DomainMetadata, Function, FunctionOutputs, HttpMethod,
        Ingress, Platform, ResourceDomainInfo, ResourceStatus,
    };
    use alien_error::AlienError;
    use alien_gcp_clients::cloudrun::{Condition, ConditionState, MockCloudRunApi, Service};
    use alien_gcp_clients::gcp::compute::{Address, MockComputeApi, Operation};
    use alien_gcp_clients::iam::{IamPolicy, MockIamApi, Role};
    use alien_gcp_clients::pubsub::MockPubSubApi;
    use alien_gcp_clients::longrunning::Operation as LongRunningOperation;
    use alien_gcp_clients::longrunning::{OperationResult, Status};
    use httpmock::{prelude::*, Mock};
    use rstest::rstest;

    use crate::core::MockPlatformServiceProvider;
    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        PlatformServiceProvider,
    };
    use crate::function::readiness_probe::test_utils::create_readiness_probe_mock;
    use crate::function::{fixtures::*, GcpFunctionController};
    use crate::GcpFunctionState;

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

    fn create_ssl_compute_mock_for_creation_and_deletion() -> Arc<MockComputeApi> {
        let mut mock = MockComputeApi::new();
        mock.expect_insert_ssl_certificate()
            .returning(|_| Ok(Operation::default()));
        mock.expect_insert_region_network_endpoint_group()
            .returning(|_, _| Ok(Operation::default()));
        mock.expect_insert_backend_service()
            .returning(|_| Ok(Operation::default()));
        mock.expect_insert_url_map()
            .returning(|_| Ok(Operation::default()));
        mock.expect_insert_target_https_proxy()
            .returning(|_| Ok(Operation::default()));
        mock.expect_insert_global_address()
            .returning(|_| Ok(Operation::default()));
        mock.expect_get_global_address().returning(|_| {
            Ok(Address {
                address: Some("203.0.113.1".to_string()),
                ..Default::default()
            })
        });
        mock.expect_insert_global_forwarding_rule()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_global_forwarding_rule()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_target_https_proxy()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_url_map()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_backend_service()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_region_network_endpoint_group()
            .returning(|_, _| Ok(Operation::default()));
        mock.expect_delete_ssl_certificate()
            .returning(|_| Ok(Operation::default()));
        mock.expect_delete_global_address()
            .returning(|_| Ok(Operation::default()));
        Arc::new(mock)
    }

    fn create_successful_service_response(service_name: &str) -> Service {
        use alien_gcp_clients::cloudrun::Service;

        Service::builder()
            .name(format!(
                "projects/test-project/locations/us-central1/services/{}",
                service_name
            ))
            .uri(format!("https://{}-abcd1234-uc.a.run.app", service_name))
            .urls(vec![format!(
                "https://{}-abcd1234-uc.a.run.app",
                service_name
            )])
            .conditions(vec![Condition::builder()
                .r#type("Ready".to_string())
                .state(ConditionState::ConditionSucceeded)
                .build()])
            .build()
    }

    fn create_successful_operation_response(operation_name: &str) -> LongRunningOperation {
        LongRunningOperation::builder()
            .name(format!(
                "projects/test-project/locations/us-central1/operations/{}",
                operation_name
            ))
            .done(false)
            .build()
    }

    fn create_completed_operation_response(operation_name: &str) -> LongRunningOperation {
        LongRunningOperation::builder()
            .name(format!("projects/test-project/locations/us-central1/operations/{}", operation_name))
            .done(true)
            .result(OperationResult::Response { 
                response: serde_json::json!({
                    "name": format!("projects/test-project/locations/us-central1/services/test-{}", operation_name)
                })
            })
            .build()
    }

    fn create_empty_iam_policy() -> IamPolicy {
        IamPolicy::builder().version(1).bindings(vec![]).build()
    }

    fn setup_mock_client_for_creation_and_update(
        function_name: &str,
        _has_public_access: bool,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock successful service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        // Mock operation status checks - first pending, then completed
        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(&operation_name_for_get))
        });

        // Mock service retrieval after creation
        let function_name_for_get = function_name.to_string();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)));

        // Mock IAM policy operations for all functions (resource-scoped permissions + optional public access)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock successful updates
        let function_name_for_update = function_name.to_string();
        let update_operation_name = format!("update-{}", function_name_for_update);
        mock_cloudrun
            .expect_patch_service()
            .returning(move |_, _, _, _, _, _| {
                Ok(create_successful_operation_response(&update_operation_name))
            });

        Arc::new(mock_cloudrun)
    }

    fn setup_mock_client_for_creation_and_deletion(
        function_name: &str,
        _has_public_access: bool,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock successful service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        // Mock operation status checks for creation
        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1); // Only for creation flow

        // Mock service retrieval after creation
        let function_name_for_get = function_name.to_string();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1); // Only for creation flow

        // Mock IAM policy operations for all functions (resource-scoped permissions + optional public access)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock successful service deletion
        let function_name_for_delete = function_name.to_string();
        let delete_operation_name = format!("delete-{}", function_name_for_delete);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        // Mock operation status checks for deletion
        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        // Mock service not found during deletion check
        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        Arc::new(mock_cloudrun)
    }

    fn setup_mock_client_for_best_effort_deletion(
        _function_name: &str,
        service_missing: bool,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock service deletion (might fail if service missing)
        if service_missing {
            mock_cloudrun
                .expect_delete_service()
                .returning(|_, _, _, _| {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "Service".to_string(),
                            resource_name: "test-service".to_string(),
                        },
                    ))
                });
        } else {
            let delete_operation_name = "delete-test".to_string();
            let delete_operation_name_for_get = delete_operation_name.clone();
            mock_cloudrun
                .expect_delete_service()
                .returning(move |_, _, _, _| {
                    Ok(create_successful_operation_response(&delete_operation_name))
                });

            mock_cloudrun.expect_get_operation().returning(move |_, _| {
                Ok(create_completed_operation_response(
                    &delete_operation_name_for_get,
                ))
            });
        }

        // Always return not found for final status check
        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        Arc::new(mock_cloudrun)
    }

    fn create_gcp_iam_mock_for_resource_permissions() -> Arc<MockIamApi> {
        let mut mock_iam = MockIamApi::new();
        mock_iam
            .expect_get_role()
            .returning(|_| Ok(Role::default()));
        mock_iam
            .expect_patch_role()
            .returning(|_, _, _| Ok(Role::default()));
        Arc::new(mock_iam)
    }

    fn setup_mock_service_provider(
        mock_cloudrun: Arc<MockCloudRunApi>,
        mock_compute: Option<Arc<MockComputeApi>>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_gcp_cloudrun_client()
            .returning(move |_| Ok(mock_cloudrun.clone()));

        if let Some(compute) = mock_compute {
            mock_provider
                .expect_get_gcp_compute_client()
                .returning(move |_| Ok(compute.clone()));
        }

        // Mock IAM client for resource-scoped permissions (custom role management)
        let mock_iam = create_gcp_iam_mock_for_resource_permissions();
        mock_provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        // Mock PubSub client for commands infrastructure cleanup
        let mock_pubsub = Arc::new(MockPubSubApi::new());
        mock_provider
            .expect_get_gcp_pubsub_client()
            .returning(move |_| Ok(mock_pubsub.clone()));

        Arc::new(mock_provider)
    }

    /// Sets up mock CloudRun client and optional readiness probe mock server
    /// Returns (cloudrun_mock_provider, optional_mock_server, optional_domain_metadata)
    fn setup_mocks_for_function(
        function: &Function,
        function_name: &str,
        for_deletion: bool,
    ) -> (
        Arc<MockPlatformServiceProvider>,
        Option<MockServer>,
        Option<DomainMetadata>,
    ) {
        let has_public_access = function.ingress == Ingress::Public;
        let needs_readiness_probe = has_public_access && function.readiness_probe.is_some();

        // Set up mock server for readiness probe if needed
        let mock_server = if needs_readiness_probe {
            Some(create_readiness_probe_mock(function))
        } else {
            None
        };

        // Set up CloudRun client mock
        let cloudrun_mock = if for_deletion {
            if let Some(ref _server) = mock_server {
                setup_mock_client_for_creation_and_deletion_with_mock_url(
                    function_name,
                    has_public_access,
                    &_server.base_url(),
                )
            } else {
                setup_mock_client_for_creation_and_deletion(function_name, has_public_access)
            }
        } else {
            if let Some(ref _server) = mock_server {
                setup_mock_client_for_creation_and_update_with_mock_url(
                    function_name,
                    has_public_access,
                    &_server.base_url(),
                )
            } else {
                setup_mock_client_for_creation_and_update(function_name, has_public_access)
            }
        };

        // For public functions, also set up compute mock and domain metadata
        let (compute_mock, domain_metadata) = if has_public_access {
            let dm = create_test_domain_metadata(&function.id);
            let compute = create_ssl_compute_mock_for_creation_and_deletion();
            (Some(compute), Some(dm))
        } else {
            (None, None)
        };

        let mock_provider = setup_mock_service_provider(cloudrun_mock, compute_mock);

        (mock_provider, mock_server, domain_metadata)
    }

    fn setup_mock_client_for_creation_and_update_with_mock_url(
        function_name: &str,
        has_public_access: bool,
        mock_url: &str,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock successful service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        // Mock operation status checks
        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(&operation_name_for_get))
        });

        // Mock service retrieval after creation - use mock URL
        let mock_url = mock_url.to_string();
        let function_name_for_get = function_name.to_string();
        mock_cloudrun.expect_get_service().returning(move |_, _| {
            let mut service = create_successful_service_response(&function_name_for_get);
            service.uri = Some(mock_url.clone());
            service.urls = vec![mock_url.clone()];
            Ok(service)
        });

        // Mock IAM policy operations for all functions (resource-scoped permissions + optional public access)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock successful updates
        let function_name_for_update = function_name.to_string();
        let update_operation_name = format!("update-{}", function_name_for_update);
        mock_cloudrun
            .expect_patch_service()
            .returning(move |_, _, _, _, _, _| {
                Ok(create_successful_operation_response(&update_operation_name))
            });

        Arc::new(mock_cloudrun)
    }

    fn setup_mock_client_for_creation_and_deletion_with_mock_url(
        function_name: &str,
        has_public_access: bool,
        mock_url: &str,
    ) -> Arc<MockCloudRunApi> {
        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock successful service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        // Mock operation status checks for creation
        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1); // Only for creation flow

        // Mock service retrieval after creation - use mock URL
        let mock_url = mock_url.to_string();
        let function_name_for_get = function_name.to_string();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| {
                let mut service = create_successful_service_response(&function_name_for_get);
                service.uri = Some(mock_url.clone());
                service.urls = vec![mock_url.clone()];
                Ok(service)
            })
            .times(1); // Only for creation flow

        // Mock IAM policy operations for all functions (resource-scoped permissions + optional public access)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock successful service deletion
        let function_name_for_delete = function_name.to_string();
        let delete_operation_name = format!("delete-{}", function_name_for_delete);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        // Mock operation status checks for deletion
        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        // Mock service not found during deletion check
        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        Arc::new(mock_cloudrun)
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_function())]
    #[case::env_vars(function_with_env_vars())]
    #[case::storage_link(function_with_storage_link())]
    #[case::env_and_storage(function_with_env_and_storage())]
    #[case::multiple_storages(function_with_multiple_storages())]
    #[case::public_ingress(function_public_ingress())]
    #[case::private_ingress(function_private_ingress())]
    #[case::concurrency(function_with_concurrency())]
    #[case::custom_config(function_custom_config())]
    #[case::readiness_probe(function_with_readiness_probe())]
    #[case::complete_test(function_complete_test())]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] function: Function) {
        let function_name = format!("test-{}", function.id);
        let (mock_provider, _mock_server, domain_metadata) =
            setup_mocks_for_function(&function, &function_name, true);

        let mut builder = SingleControllerExecutor::builder()
            .resource(function)
            .controller(GcpFunctionController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
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
        let (mock_provider, mock_server, domain_metadata) =
            setup_mocks_for_function(&to_function, &function_name, false);

        // Start with the "from" function in Ready state
        let mut ready_controller = GcpFunctionController::mock_ready(&function_name);

        // If the target function has a readiness probe, update the controller URL to point to mock server
        if to_function.readiness_probe.is_some() && to_function.ingress == Ingress::Public {
            if let Some(ref server) = mock_server {
                ready_controller.url = Some(server.base_url());
            }
        }

        let mut builder = SingleControllerExecutor::builder()
            .resource(from_function)
            .controller(ready_controller)
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies();

        if let Some(dm) = domain_metadata {
            builder = builder.domain_metadata(dm);
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
    #[case::basic(basic_function(), false)]
    #[case::public_with_missing_service(function_public_ingress(), true)]
    #[case::private_with_missing_service(function_private_ingress(), true)]
    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing(
        #[case] function: Function,
        #[case] service_missing: bool,
    ) {
        let function_name = format!("test-{}", function.id);
        let has_public_access = function.ingress == Ingress::Public;
        let mock_cloudrun =
            setup_mock_client_for_best_effort_deletion(&function_name, service_missing);
        let mock_provider = setup_mock_service_provider(mock_cloudrun, None);

        // Start with a ready controller
        let mut ready_controller = GcpFunctionController::mock_ready(&function_name);
        if has_public_access {
            ready_controller.url = Some("https://example-abcd1234-uc.a.run.app".to_string());
        }

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(ready_controller)
            .platform(Platform::Gcp)
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

    /// Test that verifies public functions get IAM policy update
    #[tokio::test]
    async fn test_public_function_sets_iam_policy() {
        let function = function_public_ingress();
        let function_name = format!("test-{}", function.id);

        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1);

        let function_name_for_get = function_name.clone();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1);

        // Validate IAM policy operations are called for resource-scoped permissions
        mock_cloudrun
            .expect_get_service_iam_policy()
            .withf(|location, service_name| {
                location == "us-central1" && service_name.starts_with("test-")
            })
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .withf(|location, service_name, _policy| {
                location == "us-central1" && service_name.starts_with("test-")
            })
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock deletion
        let delete_operation_name = format!("delete-{}", function_name);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        let compute_mock = create_ssl_compute_mock_for_creation_and_deletion();
        let mock_provider =
            setup_mock_service_provider(Arc::new(mock_cloudrun), Some(compute_mock));
        let domain_metadata = create_test_domain_metadata(&function.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(GcpFunctionController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .domain_metadata(domain_metadata)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify URL is in outputs
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<FunctionOutputs>().unwrap();
        assert!(function_outputs.url.is_some());
    }

    /// Test that verifies private functions handle resource-scoped permissions correctly
    #[tokio::test]
    async fn test_private_function_skips_iam_policy() {
        let function = function_private_ingress();
        let function_name = format!("test-{}", function.id);

        let mut mock_cloudrun = MockCloudRunApi::new();

        // Mock service creation
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1);

        let function_name_for_get = function_name.clone();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1);

        // IAM policy operations are now called for all functions (for resource-scoped permissions)
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock deletion
        let delete_operation_name = format!("delete-{}", function_name);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(GcpFunctionController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify URL is still available for private functions (internal access)
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<FunctionOutputs>().unwrap();
        assert!(function_outputs.url.is_some());
    }

    /// Test that verifies correct service configuration parameters
    #[tokio::test]
    async fn test_service_configuration_validation() {
        let function = function_custom_config();
        let function_name = format!("test-{}", function.id);

        let mut mock_cloudrun = MockCloudRunApi::new();

        // Validate service creation request has correct parameters
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .withf(|_, _, service, _| {
                // Check if the service has the expected configuration
                if let Some(template) = &service.template {
                    let containers = &template.containers;
                    if let Some(container) = containers.first() {
                        // Check memory configuration
                        if let Some(resources) = &container.resources {
                            if let Some(limits) = &resources.limits {
                                if let Some(memory) = limits.get("memory") {
                                    return memory == "512Mi";
                                }
                            }
                        }
                    }
                }
                false
            })
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1);

        let function_name_for_get = function_name.clone();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1);

        // Mock IAM policy operations for resource-scoped permissions
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock deletion
        let delete_operation_name = format!("delete-{}", function_name);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(GcpFunctionController::default())
            .platform(Platform::Gcp)
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

        let mut mock_cloudrun = MockCloudRunApi::new();

        // Validate service creation request has environment variables
        let operation_name = format!("create-{}", function_name);
        let operation_name_for_get = operation_name.clone();
        mock_cloudrun
            .expect_create_service()
            .withf(|_, _, service, _| {
                if let Some(template) = &service.template {
                    let containers = &template.containers;
                    if let Some(container) = containers.first() {
                        // Check environment variables
                        let env_vars: HashMap<String, String> = container
                            .env
                            .iter()
                            .filter_map(|env| {
                                env.value.as_ref().map(|v| (env.name.clone(), v.clone()))
                            })
                            .collect();

                        return env_vars.get("APP_ENV") == Some(&"production".to_string())
                            && env_vars.get("LOG_LEVEL") == Some(&"debug".to_string())
                            && env_vars.get("DB_NAME") == Some(&"myapp".to_string());
                    }
                }
                false
            })
            .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

        mock_cloudrun
            .expect_get_operation()
            .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
            .times(1);

        let function_name_for_get = function_name.clone();
        mock_cloudrun
            .expect_get_service()
            .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
            .times(1);

        // Mock IAM policy operations for resource-scoped permissions
        mock_cloudrun
            .expect_get_service_iam_policy()
            .returning(|_, _| Ok(create_empty_iam_policy()));

        mock_cloudrun
            .expect_set_service_iam_policy()
            .returning(|_, _, _| Ok(create_empty_iam_policy()));

        // Mock deletion
        let delete_operation_name = format!("delete-{}", function_name);
        let delete_operation_name_for_get = delete_operation_name.clone();
        mock_cloudrun
            .expect_delete_service()
            .returning(move |_, _, _, _| {
                Ok(create_successful_operation_response(&delete_operation_name))
            });

        mock_cloudrun.expect_get_operation().returning(move |_, _| {
            Ok(create_completed_operation_response(
                &delete_operation_name_for_get,
            ))
        });

        mock_cloudrun.expect_get_service().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Service".to_string(),
                    resource_name: "test-service".to_string(),
                },
            ))
        });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(GcpFunctionController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies deletion works when service_name is not set (early creation failure)
    #[tokio::test]
    async fn test_delete_with_no_service_name_succeeds() {
        let function = basic_function();

        // Create a controller with no service name set (simulating early creation failure)
        let controller = GcpFunctionController {
            state: GcpFunctionState::CreateFailed,
            service_name: None, // This is the key - no service name set
            url: None,
            operation_name: None,
            push_subscriptions: Vec::new(),
            fqdn: None,
            certificate_id: None,
            ssl_certificate_name: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            serverless_neg_name: None,
            backend_service_name: None,
            url_map_name: None,
            target_https_proxy_name: None,
            global_address_name: None,
            forwarding_rule_name: None,
            commands_topic_name: None,
            commands_subscription_name: None,
            _internal_stay_count: None,
        };

        // Mock provider - no expectations since no API calls should be made
        let mock_provider = Arc::new(MockPlatformServiceProvider::new());

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(controller)
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Start in CreateFailed state
        assert_eq!(executor.status(), ResourceStatus::ProvisionFailed);

        // Delete the function
        executor.delete().unwrap();

        // Run the delete flow - should succeed without making any API calls
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }
}
