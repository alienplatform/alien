use super::*;
use super::{AwsWorkerHandlerAction as HandlerAction, AwsWorkerState::*};

impl AwsWorkerController {
    pub(super) async fn update_importing_certificate_impl(
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

    pub(super) async fn update_code_start_impl(
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

    pub(super) async fn update_code_wait_for_active_impl(
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

    pub(super) async fn update_config_start_impl(
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

    pub(super) async fn update_config_wait_for_active_impl(
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

    pub(super) async fn update_ensuring_public_exposure_impl(
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

    pub(super) async fn update_waiting_for_certificate_impl(
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
}
