use super::*;
use super::{GcpWorkerHandlerAction as HandlerAction, GcpWorkerState::*};

impl GcpWorkerController {
    pub(super) async fn create_start_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
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

    pub(super) async fn creating_service_impl(
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

    pub(super) async fn retrying_image_pull_impl(
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

    pub(super) async fn waiting_for_service_creation_impl(
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

    pub(super) async fn importing_ssl_certificate_impl(
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

    pub(super) async fn waiting_for_ssl_certificate_impl(
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
}
