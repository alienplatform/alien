use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::worker::{run_readiness_probe, READINESS_PROBE_MAX_ATTEMPTS};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_gcp_clients::cloudscheduler::{HttpTarget, SchedulerJob, SchedulerOidcToken};
use alien_gcp_clients::compute::{
    Address, AddressType, Backend, BackendService, BackendServiceProtocol, BalancingMode,
    ForwardingRule, ForwardingRuleProtocol, LoadBalancingScheme, NetworkEndpointGroup,
    NetworkEndpointGroupCloudRun, NetworkEndpointType, Operation as ComputeOperation,
    SslCertificate, SslCertificateSelfManaged, TargetHttpsProxy, UrlMap,
};
use alien_gcp_clients::longrunning::OperationResult;
use alien_gcp_clients::pubsub::{OidcToken, PushConfig, Subscription, Topic};
// Note: Role controller removed - workers now use ServiceAccount and permission profiles
use alien_core::{
    CertificateStatus, DnsRecordStatus, ResourceDefinition, ResourceOutputs, ResourceRef,
    ResourceStatus, Worker, WorkerOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

mod helpers;
mod support;
#[cfg(test)]
mod tests;

use support::*;

pub use support::GcsNotificationTracker;

#[controller]
pub struct GcpWorkerController {
    /// The Cloud Run service name
    pub(crate) service_name: Option<String>,
    /// The invocation URL of the worker, available after creation.
    pub(crate) url: Option<String>,
    /// The operation name for long-running operations (for create, update, delete)
    pub(crate) operation_name: Option<String>,
    /// Number of targeted retries after GAR IAM propagation denied an image pull.
    #[serde(default)]
    pub(crate) image_pull_permission_retries: u8,
    /// The Compute Engine operation name for load-balancer infrastructure.
    pub(crate) compute_operation_name: Option<String>,
    /// Region for regional Compute Engine operations. `None` means global.
    pub(crate) compute_operation_region: Option<String>,
    /// Push subscription names for queue triggers (one per queue trigger)
    pub(crate) push_subscriptions: Vec<String>,
    /// Pub/Sub topic names created for storage trigger notifications
    pub(crate) storage_notification_topics: Vec<String>,
    /// GCS notification IDs for storage triggers (for cleanup)
    pub(crate) gcs_notification_ids: Vec<GcsNotificationTracker>,
    /// Cloud Scheduler job names for schedule triggers
    pub(crate) scheduler_job_names: Vec<String>,

    // Domain & Certificate
    /// The fully qualified domain name for the worker
    pub(crate) fqdn: Option<String>,
    /// The certificate ID from the TLS controller
    pub(crate) certificate_id: Option<String>,
    /// The GCP SSL certificate name
    pub(crate) ssl_certificate_name: Option<String>,
    /// Whether this worker uses a custom domain
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
    /// The global static IP address value
    pub(crate) global_address_ip: Option<String>,
    /// The forwarding rule name
    pub(crate) forwarding_rule_name: Option<String>,

    // GCP project/region (stored for binding output)
    /// The GCP project ID
    pub(crate) project_id: Option<String>,
    /// The GCP region
    pub(crate) region: Option<String>,

    // Commands infrastructure
    /// Pub/Sub topic short name for commands delivery (without project prefix)
    pub(crate) commands_topic_name: Option<String>,
    /// Pub/Sub subscription name for commands delivery
    pub(crate) commands_subscription_name: Option<String>,
}

impl GcpWorkerController {
    fn record_compute_operation(
        &mut self,
        operation: ComputeOperation,
        region: Option<String>,
        resource_id: &str,
        operation_label: &str,
    ) -> Result<()> {
        if operation.has_error() {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation_label} failed: {error_msg}"),
                resource_id: Some(resource_id.to_string()),
            }));
        }

        if operation.is_done() {
            self.compute_operation_name = None;
            self.compute_operation_region = None;
            return Ok(());
        }

        let operation_name = operation.name.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation_label} returned without operation name"),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        self.compute_operation_name = Some(operation_name);
        self.compute_operation_region = region;
        Ok(())
    }

    async fn compute_operation_done(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        operation_label: &str,
    ) -> Result<bool> {
        let Some(operation_name) = self.compute_operation_name.as_ref() else {
            return Ok(true);
        };

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = if let Some(region) = &self.compute_operation_region {
            compute_client
                .get_region_operation(region.clone(), operation_name.clone())
                .await
        } else {
            compute_client
                .get_global_operation(operation_name.clone())
                .await
        }
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to check {operation_label} status"),
            resource_id: Some(resource_id.to_string()),
        })?;

        if !operation.is_done() {
            debug!(
                operation_name=%operation_name,
                operation=%operation_label,
                "Compute operation still in progress"
            );
            return Ok(false);
        }

        if operation.has_error() {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation_label} failed: {error_msg}"),
                resource_id: Some(resource_id.to_string()),
            }));
        }

        self.compute_operation_name = None;
        self.compute_operation_region = None;
        Ok(true)
    }
}

#[controller]
impl GcpWorkerController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;
        info!(name=%cfg.id, "Initiating Cloud Run service creation");

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

        let gcp_config = ctx.get_gcp_config()?;
        self.project_id = Some(gcp_config.project_id.clone());
        self.region = Some(gcp_config.region.clone());
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
                if is_cross_project_image_pull_permission_error(&error.message)
                    && self.image_pull_permission_retries < MAX_IMAGE_PULL_PERMISSION_RETRIES
                {
                    self.image_pull_permission_retries += 1;
                    self.operation_name = None;
                    let delay =
                        image_pull_permission_retry_delay(self.image_pull_permission_retries);
                    warn!(
                        worker=%ctx.desired_config.id(),
                        attempt=self.image_pull_permission_retries,
                        max_attempts=MAX_IMAGE_PULL_PERMISSION_RETRIES,
                        delay_seconds=delay.as_secs(),
                        "Cloud Run image pull is waiting for the verified GAR reader grant to propagate"
                    );
                    return Ok(HandlerAction::Continue {
                        state: RetryingImagePull,
                        suggested_delay: Some(delay),
                    });
                }

                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Operation failed: {} (code: {})", error.message, error.code),
                    resource_id: Some(ctx.desired_config.id().to_string()),
                }));
            }

            // Operation succeeded, transition to next state
            info!(operation=%operation_name, "Operation completed successfully");
            self.image_pull_permission_retries = 0;

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
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            })
        }
    }

    #[handler(
        state = RetryingImagePull,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn retrying_image_pull(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: cfg.id.clone(),
                message: "Service name not set while retrying a Cloud Run image pull".to_string(),
            })
        })?;
        let service = self.build_cloud_run_service(service_name, cfg, ctx).await?;

        let operation = ctx
            .service_provider
            .get_gcp_cloudrun_client(gcp_config)?
            .patch_service(
                gcp_config.region.clone(),
                service_name.clone(),
                service,
                Some("template".to_string()),
                None,
                Some(false),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to retry Cloud Run service after GAR permission propagation"
                    .to_string(),
                resource_id: Some(cfg.id.clone()),
            })?;
        let operation_name = operation.name.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Cloud Run image-pull retry returned without an operation name"
                    .to_string(),
                resource_id: Some(cfg.id.clone()),
            })
        })?;

        self.operation_name = Some(operation_name);

        Ok(HandlerAction::Continue {
            state: CreatingService,
            suggested_delay: Some(Duration::from_secs(2)),
        })
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
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        let cloud_run_url = service.uri.or_else(|| service.urls.first().cloned());

        // Check for URL override in deployment config, otherwise use Cloud Run URL
        let config = ctx.desired_resource_config::<Worker>()?;
        self.url = ctx
            .deployment_config
            .public_endpoints
            .as_ref()
            .and_then(|resources| resources.get(&config.id))
            .and_then(|endpoints| endpoints.values().next().cloned())
            .or(cloud_run_url);

        info!(name=%service_name, url=?self.url, "Cloud Run service created successfully");

        // Branch based on ingress type
        // If public, resolve domain and proceed to certificate/load balancer flow
        // If private, skip directly to push subscriptions
        if !config.public_endpoints.is_empty() {
            match Self::resolve_domain_info(ctx, &config.id) {
                Ok(domain_info) => {
                    info!(fqdn=%domain_info.fqdn, "Resolved domain for public worker");
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
                        worker=%config.id,
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
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id));

        let status = metadata.map(|m| &m.certificate_status);
        if !self.ensure_domain_info(ctx, &worker_config.id)? {
            return Ok(HandlerAction::Continue {
                state: CreatingPushSubscriptions,
                suggested_delay: None,
            });
        }
        if self.uses_custom_domain && self.ssl_certificate_name.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingServerlessNeg,
                suggested_delay: None,
            });
        }

        match status {
            Some(CertificateStatus::Issued) => Ok(HandlerAction::Continue {
                state: ImportingSslCertificate,
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
        state = ImportingSslCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_ssl_certificate(
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

        // For GCP, we use the full certificate chain
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let ssl_cert_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "cert");

        let ssl_certificate = SslCertificate::builder()
            .name(ssl_cert_name.clone())
            .description(format!("SSL certificate for worker {}", worker_config.id))
            .r#type("SELF_MANAGED".to_string())
            .self_managed(
                SslCertificateSelfManaged::builder()
                    .certificate(certificate_chain.clone())
                    .private_key(private_key.clone())
                    .build(),
            )
            .build();

        let operation = compute_client
            .insert_ssl_certificate(ssl_certificate)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import SSL certificate to GCP".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.ssl_certificate_name = Some(ssl_cert_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "SSL certificate import",
        )?;

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        info!(
            worker=%worker_config.id,
            cert_name=%self.ssl_certificate_name.as_ref().unwrap(),
            "SSL certificate imported to GCP"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForSslCertificate,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForSslCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "SSL certificate import")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

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
        if self.serverless_neg_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForServerlessNeg
            } else {
                CreatingBackendService
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let service_name = self.service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Service name not set".to_string(),
            })
        })?;

        let neg_name = get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "neg");

        // Create serverless NEG pointing to Cloud Run service
        // According to GCP API: https://docs.cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups
        // For serverless NEGs, we must specify cloud_run, app_engine, or cloud_function
        let cloud_run_config = NetworkEndpointGroupCloudRun::builder()
            .service(service_name.clone())
            .build();

        let neg = NetworkEndpointGroup::builder()
            .name(neg_name.clone())
            .description(format!("Serverless NEG for worker {}", worker_config.id))
            .network_endpoint_type(NetworkEndpointType::Serverless)
            .cloud_run(cloud_run_config)
            .build();

        let operation = compute_client
            .insert_region_network_endpoint_group(gcp_config.region.clone(), neg)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create serverless NEG".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.serverless_neg_name = Some(neg_name);
        self.record_compute_operation(
            operation,
            Some(gcp_config.region.clone()),
            &worker_config.id,
            "serverless NEG creation",
        )?;

        info!(
            worker=%worker_config.id,
            neg_name=%self.serverless_neg_name.as_ref().unwrap(),
            "Serverless NEG created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForServerlessNeg,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForServerlessNeg,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "serverless NEG creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingBackendService,
            suggested_delay: None,
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
        if self.backend_service_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForBackendService
            } else {
                CreatingUrlMap
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let neg_name = self.serverless_neg_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Serverless NEG name not set".to_string(),
            })
        })?;

        let backend_service_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "backend");

        let neg_url = format!(
            "projects/{}/regions/{}/networkEndpointGroups/{}",
            gcp_config.project_id, gcp_config.region, neg_name
        );

        // Create backend service with serverless NEG (no health check for serverless)
        let backend_service = BackendService::builder()
            .name(backend_service_name.clone())
            .description(format!("Backend service for worker {}", worker_config.id))
            .protocol(BackendServiceProtocol::Https)
            .load_balancing_scheme(LoadBalancingScheme::External)
            .backends(vec![Backend::builder()
                .group(neg_url)
                .balancing_mode(BalancingMode::Utilization)
                .build()])
            .build();

        let operation = compute_client
            .insert_backend_service(backend_service)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create backend service".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.backend_service_name = Some(backend_service_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "backend service creation",
        )?;

        info!(
            worker=%worker_config.id,
            backend_service_name=%self.backend_service_name.as_ref().unwrap(),
            "Backend service created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForBackendService,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForBackendService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "backend service creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingUrlMap,
            suggested_delay: None,
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
        if self.url_map_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForUrlMap
            } else {
                CreatingTargetHttpsProxy
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let backend_service_name = self.backend_service_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Backend service name not set".to_string(),
            })
        })?;

        let url_map_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "urlmap");

        let backend_service_url = format!(
            "projects/{}/global/backendServices/{}",
            gcp_config.project_id, backend_service_name
        );

        // Create URL map routing to backend service
        let url_map = UrlMap::builder()
            .name(url_map_name.clone())
            .description(format!("URL map for worker {}", worker_config.id))
            .default_service(backend_service_url)
            .build();

        let operation = compute_client.insert_url_map(url_map).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to create URL map".to_string(),
                resource_id: Some(worker_config.id.clone()),
            },
        )?;

        self.url_map_name = Some(url_map_name);
        self.record_compute_operation(operation, None, &worker_config.id, "URL map creation")?;

        info!(
            worker=%worker_config.id,
            url_map_name=%self.url_map_name.as_ref().unwrap(),
            "URL map created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForUrlMap,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForUrlMap,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "URL map creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingTargetHttpsProxy,
            suggested_delay: None,
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
        if self.target_https_proxy_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForTargetHttpsProxy
            } else {
                CreatingGlobalAddress
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let url_map_name = self.url_map_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "URL map name not set".to_string(),
            })
        })?;

        let ssl_cert_name = self.ssl_certificate_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "SSL certificate name not set".to_string(),
            })
        })?;

        let proxy_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "https-proxy");

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
            .description(format!("HTTPS proxy for worker {}", worker_config.id))
            .url_map(url_map_url)
            .ssl_certificates(vec![ssl_cert_url])
            .build();

        let operation = compute_client
            .insert_target_https_proxy(https_proxy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create target HTTPS proxy".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.target_https_proxy_name = Some(proxy_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "target HTTPS proxy creation",
        )?;

        info!(
            worker=%worker_config.id,
            proxy_name=%self.target_https_proxy_name.as_ref().unwrap(),
            "Target HTTPS proxy created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForTargetHttpsProxy,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForTargetHttpsProxy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "target HTTPS proxy creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingGlobalAddress,
            suggested_delay: None,
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
        if self.global_address_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForGlobalAddress
            } else {
                CreatingForwardingRule
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let address_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "ip");

        // Create global static IP address
        let address = Address::builder()
            .name(address_name.clone())
            .description(format!("Global IP for worker {}", worker_config.id))
            .address_type(AddressType::External)
            .build();

        let operation = compute_client
            .insert_global_address(address)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create global address".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.global_address_name = Some(address_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "global address creation",
        )?;

        info!(
            worker=%worker_config.id,
            address_name=%self.global_address_name.as_ref().unwrap(),
            "Global address created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForGlobalAddress,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForGlobalAddress,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "global address creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: CreatingForwardingRule,
            suggested_delay: None,
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
        if self.forwarding_rule_name.is_some() {
            let state = if self.compute_operation_name.is_some() {
                WaitingForForwardingRule
            } else {
                WaitingForDns
            };
            return Ok(HandlerAction::Continue {
                state,
                suggested_delay: Some(Duration::from_secs(2)),
            });
        }

        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let proxy_name = self.target_https_proxy_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Target HTTPS proxy name not set".to_string(),
            })
        })?;

        let address_name = self.global_address_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Global address name not set".to_string(),
            })
        })?;

        let ip_address = self
            .ensure_global_address_ip(ctx, &worker_config.id, &address_name)
            .await?;

        let forwarding_rule_name =
            get_gcp_worker_resource_name(ctx.resource_prefix, &worker_config.id, "https");

        let proxy_url = format!(
            "projects/{}/global/targetHttpsProxies/{}",
            gcp_config.project_id, proxy_name
        );

        // Create forwarding rule exposing HTTPS endpoint
        let forwarding_rule = ForwardingRule::builder()
            .name(forwarding_rule_name.clone())
            .description(format!("Forwarding rule for worker {}", worker_config.id))
            .ip_address(ip_address)
            .ip_protocol(ForwardingRuleProtocol::Tcp)
            .port_range("443-443".to_string())
            .target(proxy_url)
            .load_balancing_scheme(LoadBalancingScheme::External)
            .build();

        let operation = compute_client
            .insert_global_forwarding_rule(forwarding_rule)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create forwarding rule".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.forwarding_rule_name = Some(forwarding_rule_name);
        self.record_compute_operation(
            operation,
            None,
            &worker_config.id,
            "forwarding rule creation",
        )?;

        info!(
            worker=%worker_config.id,
            forwarding_rule_name=%self.forwarding_rule_name.as_ref().unwrap(),
            "Forwarding rule created"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForForwardingRule,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = WaitingForForwardingRule,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if !self
            .compute_operation_done(ctx, &worker_config.id, "forwarding rule creation")
            .await?
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        Ok(HandlerAction::Continue {
            state: WaitingForDns,
            suggested_delay: None,
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
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        if let Some(address_name) = self.global_address_name.clone() {
            self.ensure_global_address_ip(ctx, &worker_config.id, &address_name)
                .await?;
        }

        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&worker_config.id));

        let status = metadata.map(|m| &m.dns_status);

        match status {
            Some(DnsRecordStatus::Active) => {
                info!(
                    worker=%worker_config.id,
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
        state = CreatingPushSubscriptions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_push_subscriptions(
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

    #[handler(
        state = CreatingSchedulerJobs,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_scheduler_jobs(
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

    #[handler(
        state = CreatingCommandsInfrastructure,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_commands_infrastructure(
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

    #[handler(
        state = SettingIamPolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn setting_iam_policy(
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

    #[handler(
        state = RunningReadinessProbe,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn running_readiness_probe(
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
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
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
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateImportingSslCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;

        if cfg.public_endpoints.is_empty() || self.uses_custom_domain {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&cfg.id))
        else {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        };

        if resource.issued_at == self.certificate_issued_at {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }

        let Some(proxy_name) = self.target_https_proxy_name.clone() else {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        };

        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(cfg.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(cfg.id.clone()),
            })
        })?;

        let issued_suffix = resource
            .issued_at
            .as_deref()
            .unwrap_or("renewed")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect::<String>()
            .to_lowercase();
        let ssl_cert_name = get_gcp_worker_resource_name(
            ctx.resource_prefix,
            &cfg.id,
            &format!("cert-{issued_suffix}"),
        );
        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let ssl_certificate = SslCertificate::builder()
            .name(ssl_cert_name.clone())
            .description(format!("Renewed SSL certificate for worker {}", cfg.id))
            .r#type("SELF_MANAGED".to_string())
            .self_managed(
                SslCertificateSelfManaged::builder()
                    .certificate(certificate_chain.clone())
                    .private_key(private_key.clone())
                    .build(),
            )
            .build();

        match compute_client.insert_ssl_certificate(ssl_certificate).await {
            Ok(_) => {}
            Err(e) if is_remote_resource_conflict(&e) => {
                info!(
                    worker=%cfg.id,
                    cert_name=%ssl_cert_name,
                    "Renewed SSL certificate already exists; treating as imported"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to import renewed SSL certificate to GCP".to_string(),
                    resource_id: Some(cfg.id.clone()),
                }));
            }
        }

        let ssl_cert_url = format!(
            "projects/{}/global/sslCertificates/{}",
            gcp_config.project_id, ssl_cert_name
        );
        compute_client
            .set_target_https_proxy_ssl_certificates(proxy_name, vec![ssl_cert_url])
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to bind renewed SSL certificate to target HTTPS proxy".to_string(),
                resource_id: Some(cfg.id.clone()),
            })?;

        let previous_ssl_certificate_name = self.ssl_certificate_name.clone();

        self.ssl_certificate_name = Some(ssl_cert_name);
        self.certificate_issued_at = resource.issued_at.clone();

        if let Some(previous_ssl_certificate_name) = previous_ssl_certificate_name {
            if self.ssl_certificate_name.as_deref() != Some(previous_ssl_certificate_name.as_str())
            {
                match compute_client
                    .delete_ssl_certificate(previous_ssl_certificate_name.clone())
                    .await
                {
                    Ok(_) => {
                        info!(
                            worker=%cfg.id,
                            cert_name=%previous_ssl_certificate_name,
                            "Deleted previous SSL certificate after renewal"
                        );
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(e) => {
                        warn!(
                            worker=%cfg.id,
                            cert_name=%previous_ssl_certificate_name,
                            error=%e,
                            "Failed to delete previous SSL certificate after renewal"
                        );
                    }
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: UpdateStart,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let previous_cfg = ctx.previous_resource_config::<Worker>()?;
        if cfg == previous_cfg {
            return Ok(HandlerAction::Continue {
                state: UpdateEnsuringPublicExposure,
                suggested_delay: None,
            });
        }

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
                max_times: Some(60),
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
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(5)),
            });
        }

        info!(name=%service_name, "Cloud Run service updated successfully");

        Ok(HandlerAction::Continue {
            state: UpdateEnsuringPublicExposure,
            suggested_delay: None,
        })
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

        if current_config.public_endpoints.is_empty() {
            return Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
                suggested_delay: None,
            });
        }

        let has_domain_info = self.ensure_domain_info(ctx, &current_config.id)?;
        if !has_domain_info {
            return Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
                suggested_delay: None,
            });
        }

        if self.forwarding_rule_name.is_some() {
            return Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
                suggested_delay: None,
            });
        }

        Ok(HandlerAction::Continue {
            state: UpdateWaitingForCertificate,
            suggested_delay: None,
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
                state: ImportingSslCertificate,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateImportingInitialSslCertificate,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingServerlessNeg,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingServerlessNeg,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingPushSubscriptions,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
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
        state = UpdateImportingInitialSslCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_initial_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.importing_ssl_certificate(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForSslCertificate,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForSslCertificate,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "importing_ssl_certificate",
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
        state = UpdateWaitingForSslCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_ssl_certificate(ctx).await? {
            HandlerAction::Continue {
                state: CreatingServerlessNeg,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingServerlessNeg,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_ssl_certificate",
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
        state = UpdateCreatingServerlessNeg,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_serverless_neg(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForServerlessNeg,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForServerlessNeg,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingBackendService,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingBackendService,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_serverless_neg",
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
        state = UpdateWaitingForServerlessNeg,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_serverless_neg(ctx).await? {
            HandlerAction::Continue {
                state: CreatingBackendService,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingBackendService,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_serverless_neg",
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
        state = UpdateCreatingBackendService,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_backend_service(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForBackendService,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForBackendService,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingUrlMap,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingUrlMap,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_backend_service",
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
        state = UpdateWaitingForBackendService,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_backend_service(ctx).await? {
            HandlerAction::Continue {
                state: CreatingUrlMap,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingUrlMap,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_backend_service",
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
        state = UpdateCreatingUrlMap,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_url_map(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForUrlMap,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForUrlMap,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingTargetHttpsProxy,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingTargetHttpsProxy,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_url_map",
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
        state = UpdateWaitingForUrlMap,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_url_map(ctx).await? {
            HandlerAction::Continue {
                state: CreatingTargetHttpsProxy,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingTargetHttpsProxy,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_url_map",
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
        state = UpdateCreatingTargetHttpsProxy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_target_https_proxy(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForTargetHttpsProxy,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForTargetHttpsProxy,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingGlobalAddress,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingGlobalAddress,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_target_https_proxy",
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
        state = UpdateWaitingForTargetHttpsProxy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_target_https_proxy(ctx).await? {
            HandlerAction::Continue {
                state: CreatingGlobalAddress,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingGlobalAddress,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_target_https_proxy",
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
        state = UpdateCreatingGlobalAddress,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_global_address(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForGlobalAddress,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForGlobalAddress,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: CreatingForwardingRule,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingForwardingRule,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_global_address",
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
        state = UpdateWaitingForGlobalAddress,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_global_address(ctx).await? {
            HandlerAction::Continue {
                state: CreatingForwardingRule,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateCreatingForwardingRule,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_global_address",
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
        state = UpdateCreatingForwardingRule,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.creating_forwarding_rule(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForForwardingRule,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForForwardingRule,
                suggested_delay,
            }),
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "creating_forwarding_rule",
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
        state = UpdateWaitingForForwardingRule,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        match self.waiting_for_forwarding_rule(ctx).await? {
            HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdateWaitingForDns,
                suggested_delay,
            }),
            HandlerAction::Continue { state, .. } => Err(Self::unexpected_update_wrapper_state(
                &worker_config.id,
                "waiting_for_forwarding_rule",
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
                state: CreatingPushSubscriptions,
                suggested_delay,
            } => Ok(HandlerAction::Continue {
                state: UpdatePushSubscriptions,
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
        state = UpdatePushSubscriptions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_push_subscriptions(
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

    #[handler(
        state = UpdateSettingIamPolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_setting_iam_policy(
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

    #[handler(
        state = UpdateRunningReadinessProbe,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_running_readiness_probe(
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
                self.storage_notification_topics.clear();
                self.gcs_notification_ids.clear();
                self.scheduler_job_names.clear();

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

        let forwarding_rule_already_deleted =
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
                        false
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(name=%forwarding_rule_name, "Forwarding rule was already deleted");
                        true
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete forwarding rule '{}'",
                                forwarding_rule_name
                            ),
                            resource_id: None,
                        }));
                    }
                }
            } else {
                false
            };

        if forwarding_rule_already_deleted {
            self.forwarding_rule_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingTargetHttpsProxy,
            suggested_delay: Some(Duration::from_secs(10)),
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
                Err(e) if is_gcp_resource_in_use(&e) => {
                    info!(
                        name=%proxy_name,
                        "Target HTTPS proxy is still referenced by another GCP resource; retrying deletion"
                    );
                    return Ok(HandlerAction::Stay {
                        max_times: Some(30),
                        suggested_delay: Some(Duration::from_secs(10)),
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete target HTTPS proxy '{}'", proxy_name),
                        resource_id: None,
                    }));
                }
            }
        }

        self.forwarding_rule_name = None;
        self.target_https_proxy_name = None;

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
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete URL map '{}'", url_map_name),
                        resource_id: None,
                    }));
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
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete backend service '{}'",
                            backend_service_name
                        ),
                        resource_id: None,
                    }));
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
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete serverless NEG '{}'", neg_name),
                        resource_id: None,
                    }));
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
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete SSL certificate '{}'", ssl_cert_name),
                        resource_id: None,
                    }));
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
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete global address '{}'", address_name),
                        resource_id: None,
                    }));
                }
            }

            self.global_address_name = None;
            self.global_address_ip = None;
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
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        info!(worker=%worker_config.id, subscriptions=?self.push_subscriptions, "Deleting push subscriptions");

        // Delete all push subscriptions using best-effort approach (ignore NotFound)
        self.delete_all_push_subscriptions(ctx, gcp_config).await?;

        // Delete GCS notifications (best-effort)
        self.delete_all_storage_notifications(ctx, gcp_config)
            .await?;

        // Delete storage notification topics (best-effort)
        self.delete_all_storage_notification_topics(ctx, gcp_config)
            .await?;

        // Continue to scheduler jobs cleanup
        Ok(HandlerAction::Continue {
            state: DeletingSchedulerJobs,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingSchedulerJobs,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_scheduler_jobs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        if self.scheduler_job_names.is_empty() {
            return Ok(HandlerAction::Continue {
                state: DeletingCommandsInfrastructure,
                suggested_delay: None,
            });
        }

        info!(worker=%worker_config.id, jobs=?self.scheduler_job_names, "Deleting Cloud Scheduler jobs");

        let scheduler_client = ctx
            .service_provider
            .get_gcp_cloud_scheduler_client(gcp_config)?;

        for job_name in &self.scheduler_job_names.clone() {
            match scheduler_client.delete_job(job_name.clone()).await {
                Ok(_) => {
                    info!(
                        worker=%worker_config.id,
                        job=%job_name,
                        "Cloud Scheduler job deleted successfully"
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        worker=%worker_config.id,
                        job=%job_name,
                        "Cloud Scheduler job was already deleted (not found)"
                    );
                }
                Err(e) => {
                    warn!(
                        worker=%worker_config.id,
                        job=%job_name,
                        error=%e,
                        "Failed to delete Cloud Scheduler job (best-effort, continuing)"
                    );
                }
            }
        }

        self.scheduler_job_names.clear();

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
        let cfg = ctx.desired_resource_config::<Worker>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let pubsub_client = ctx.service_provider.get_gcp_pubsub_client(gcp_config)?;
        let derived_topic_name = cfg
            .commands_enabled
            .then(|| {
                self.service_name
                    .as_ref()
                    .map(|service_name| format!("{service_name}-rq"))
            })
            .flatten();
        let derived_subscription_name = cfg
            .commands_enabled
            .then(|| {
                self.service_name
                    .as_ref()
                    .map(|service_name| format!("{service_name}-rq-sub"))
            })
            .flatten();

        // Delete commands subscription (best-effort)
        if let Some(subscription_name) = self
            .commands_subscription_name
            .take()
            .or(derived_subscription_name)
        {
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
        if let Some(topic_name) = self.commands_topic_name.take().or(derived_topic_name) {
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
                max_times: Some(20),
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
                    max_times: Some(20),
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
            let public_url = self
                .fqdn
                .as_ref()
                .map(|fqdn| format!("https://{fqdn}"))
                .unwrap_or_else(|| url.clone());

            let load_balancer_endpoint = self.global_address_ip.as_ref().map(|global_address_ip| {
                alien_core::LoadBalancerEndpoint {
                    dns_name: global_address_ip.clone(),
                    hosted_zone_id: None,
                }
            });

            ResourceOutputs::new(WorkerOutputs {
                // Use the service name if available, otherwise fall back to a placeholder
                worker_name: self
                    .service_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                identifier: self.service_name.clone(),
                public_endpoints: std::collections::HashMap::from([(
                    "default".to_string(),
                    alien_core::PublicEndpointOutput {
                        host: alien_core::public_url_host(&public_url).unwrap_or_default(),
                        url: public_url,
                        wildcard_host: None,
                        load_balancer_endpoint,
                    },
                )]),
                commands_push_target: self.commands_topic_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, CloudRunWorkerBinding, WorkerBinding};

        if let (Some(service_name), Some(url)) = (&self.service_name, &self.url) {
            let project_id = self.project_id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "GCP project_id missing when building binding params".to_string(),
                    operation: Some("build_binding_params".to_string()),
                    resource_id: None,
                })
            })?;
            let location = self.region.clone().ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "GCP region missing when building binding params".to_string(),
                    operation: Some("build_binding_params".to_string()),
                    resource_id: None,
                })
            })?;

            let binding = WorkerBinding::CloudRun(CloudRunWorkerBinding {
                project_id: BindingValue::Value(project_id),
                service_name: BindingValue::Value(service_name.clone()),
                location: BindingValue::Value(location),
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
