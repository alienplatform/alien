use super::*;
use super::{GcpWorkerHandlerAction as HandlerAction, GcpWorkerState::*};

impl GcpWorkerController {
    pub(super) async fn update_importing_ssl_certificate_impl(
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

    pub(super) async fn update_start_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
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

    pub(super) async fn updating_service_impl(
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

    pub(super) async fn waiting_for_service_update_impl(
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

    pub(super) async fn update_ensuring_public_exposure_impl(
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

    pub(super) async fn update_waiting_for_certificate_impl(
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

    pub(super) async fn update_importing_initial_ssl_certificate_impl(
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

    pub(super) async fn update_waiting_for_ssl_certificate_impl(
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
}
