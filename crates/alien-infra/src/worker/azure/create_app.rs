use super::*;
use super::{AzureWorkerHandlerAction as HandlerAction, AzureWorkerState::*};

impl AzureWorkerController {
    pub(super) async fn create_start_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        if self.container_app_name.is_none() {
            self.pre_container_app_rbac_wait_until_epoch_secs = None;
            self.ready_rbac_wait_until_epoch_secs = None;
            self.commands_infrastructure_auth_wait_until_epoch_secs = None;
            self.container_apps_environment_wake_wait_until_epoch_secs = None;
            self.container_apps_environment_wake_retry_after_epoch_secs = None;
            self.update_rbac_wait_required = false;
        }

        // Product limitation: Only allow at most one queue trigger per worker
        let queue_trigger_count = func_cfg
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Worker '{}' has {} queue triggers, but only one queue trigger per worker is currently supported",
                    func_cfg.id, queue_trigger_count
                ),
                resource_id: Some(func_cfg.id.clone()),
            }));
        }

        // Derive deterministic resource names.
        let container_app_name = get_azure_container_app_name(ctx.resource_prefix, &func_cfg.id);

        // Pre-create commands infrastructure BEFORE the Container App so the Dapr
        // sidecar starts with the component already defined AND the RBAC roles already
        // assigned (giving them time to propagate during Container App creation ~30s).
        // This eliminates the race condition where the sidecar starts before permissions exist.
        self.container_app_name = Some(container_app_name.clone());
        if let Some(delay) = self
            .wait_for_container_apps_environment_wake_retry(&func_cfg.id, "commands infrastructure")
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }
        info!(name=%func_cfg.id, "Initiating Azure Container App worker creation");
        if func_cfg.commands_enabled {
            match self
                .setup_commands_infrastructure(ctx, azure_cfg, func_cfg, &container_app_name)
                .await
            {
                Ok(CommandsSetupOperation::Completed) => {
                    self.commands_infrastructure_auth_wait_until_epoch_secs = None;
                    self.container_apps_environment_wake_wait_until_epoch_secs = None;
                    self.container_apps_environment_wake_retry_after_epoch_secs = None;
                }
                Ok(CommandsSetupOperation::Creating(delay)) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForPreCreateCommandsDaprComponentOperation,
                        suggested_delay: Some(delay),
                    });
                }
                Ok(CommandsSetupOperation::Deleting(delay)) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForPreCreateDaprComponentDeletion,
                        suggested_delay: Some(delay),
                    });
                }
                Ok(CommandsSetupOperation::Pending(delay)) => {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
                Err(e) if is_azure_authorization_propagation_error(&e) => {
                    let deadline = ensure_rbac_wait_deadline(
                        &mut self.commands_infrastructure_auth_wait_until_epoch_secs,
                        AZURE_COMMANDS_INFRASTRUCTURE_AUTH_WAIT_SECS,
                    );

                    if let Some(delay) = rbac_wait_delay(deadline) {
                        warn!(
                            name=%func_cfg.id,
                            remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                            error=%e,
                            "Azure authorization is not ready for commands infrastructure; retrying"
                        );
                        return Ok(HandlerAction::Stay {
                            max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                            suggested_delay: Some(delay),
                        });
                    }

                    return Err(e);
                }
                Err(e) if is_azure_container_apps_environment_waking_error(&e) => {
                    let deadline = ensure_rbac_wait_deadline(
                        &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                        AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                    );

                    if let Some(delay) = self.record_container_apps_environment_wake_retry(deadline)
                    {
                        warn!(
                            name=%func_cfg.id,
                            remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                            error=%e,
                            "Azure Container Apps Environment is waking; retrying commands infrastructure"
                        );
                        return Ok(HandlerAction::Stay {
                            max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                            suggested_delay: Some(delay),
                        });
                    }

                    return Err(e);
                }
                Err(e) => return Err(e),
            }
        }

        // Pre-create every trigger input component before the Container App. The
        // Dapr sidecar loads scoped components when the app revision starts; adding
        // queue or cron components only after that revision is running leaves the
        // input binding inactive. Storage triggers additionally create Event Grid ->
        // a dedicated Service Bus queue, and their receiver role can propagate while
        // the app starts.
        let mut cron_index = 0usize;
        for trigger in &func_cfg.triggers {
            let operation = match trigger {
                alien_core::WorkerTrigger::Queue { queue } => {
                    self.create_dapr_service_bus_component(
                        ctx,
                        &container_app_name,
                        func_cfg,
                        queue,
                    )
                    .await?
                }
                alien_core::WorkerTrigger::Storage { storage, events } => {
                    self.create_azure_storage_trigger(
                        ctx,
                        &container_app_name,
                        func_cfg,
                        storage,
                        events,
                    )
                    .await?
                }
                alien_core::WorkerTrigger::Schedule { cron } => {
                    let operation = self
                        .create_dapr_cron_component(
                            ctx,
                            &container_app_name,
                            func_cfg,
                            cron,
                            cron_index,
                        )
                        .await?;
                    cron_index += 1;
                    operation
                }
            };

            match operation {
                DaprComponentOperation::Completed => {}
                DaprComponentOperation::Creating(delay) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForPreCreateCommandsDaprComponentOperation,
                        suggested_delay: Some(delay),
                    });
                }
                DaprComponentOperation::Deleting(delay) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForPreCreateDaprComponentDeletion,
                        suggested_delay: Some(delay),
                    });
                }
                DaprComponentOperation::Pending(delay) => {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
            }
        }

        // Wait in a real controller state for Azure RBAC propagation. A
        // suggested delay alone is only a scheduling hint and can be shortened
        // by other resources in the executor.
        info!(name=%func_cfg.id, "Trigger infrastructure ready, waiting for RBAC propagation before creating Container App");
        Ok(HandlerAction::Continue {
            state: WaitingBeforeContainerAppCreation,
            suggested_delay: None,
        })
    }

    pub(super) async fn waiting_for_pre_create_commands_dapr_component_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let operation_url = self.pending_operation_url.clone().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for pre-created Dapr component"
                    .to_string(),
                operation: Some(
                    "waiting_for_pre_create_commands_dapr_component_operation".to_string(),
                ),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        let azure_cfg = ctx.get_azure_config()?;
        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;
        let lro = LongRunningOperation {
            url: operation_url,
            retry_after: self.pending_operation_retry_after.map(Duration::from_secs),
            location_url: None,
        };

        let status = operation_client
            .check_status(&lro, "CreateOrUpdateDaprComponent", &func_cfg.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure ARM operation failed for pre-created Dapr component".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if status.is_some() {
            self.pending_operation_url = None;
            self.pending_operation_retry_after = None;
            Ok(HandlerAction::Continue {
                state: CreateStart,
                suggested_delay: None,
            })
        } else {
            let delay = self
                .pending_operation_retry_after
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15));
            Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            })
        }
    }

    pub(super) async fn waiting_for_pre_create_dapr_component_deletion_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "waiting_for_pre_create_dapr_component_deletion",
                "Azure ARM operation failed for pre-create Dapr deletion",
            )
            .await?
        {
            None => Ok(HandlerAction::Continue {
                state: CreateStart,
                suggested_delay: None,
            }),
            Some(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn waiting_before_container_app_creation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let deadline = ensure_rbac_wait_deadline(
            &mut self.pre_container_app_rbac_wait_until_epoch_secs,
            AZURE_PRE_CONTAINER_APP_RBAC_WAIT_SECS,
        );

        if let Some(delay) = rbac_wait_delay(deadline) {
            info!(
                name=%func_cfg.id,
                remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                "Waiting for Azure RBAC propagation before creating Container App"
            );
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        self.pre_container_app_rbac_wait_until_epoch_secs = None;
        Ok(HandlerAction::Continue {
            state: CreatingContainerAppResource,
            suggested_delay: None,
        })
    }

    pub(super) async fn creating_container_app_resource_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self
            .container_app_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Container app name not set".to_string(),
                })
            })?
            .clone();
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;

        // Build ARM request body.
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;
        let container_app = self
            .build_container_app(
                func_cfg,
                &environment_name,
                &container_app_name,
                azure_cfg,
                ctx,
            )
            .await?;

        // Fire the CREATE/UPDATE call.
        let op_result = client
            .create_or_update_container_app(
                &resource_group_name,
                &container_app_name,
                &container_app,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to initiate container app creation".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        match op_result {
            OperationResult::Completed(immediate_app) => {
                info!(name=%container_app_name, "Container app creation completed immediately");
                self.handle_creation_completed(ctx, &immediate_app);

                Ok(HandlerAction::Continue {
                    state: ConfiguringDaprComponents,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(lro) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());

                Ok(HandlerAction::Continue {
                    state: WaitingForCreateOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
        }
    }

    pub(super) async fn waiting_for_create_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let operation_url = match &self.pending_operation_url {
            Some(u) => u.clone(),
            None => {
                return Err(AlienError::new(ErrorData::InfrastructureError {
                    message: "No pending operation URL recorded in WaitingForCreateOperation"
                        .to_string(),
                    operation: Some("waiting_for_create_operation".to_string()),
                    resource_id: Some(ctx.desired_resource_config::<Worker>()?.id.clone()),
                }));
            }
        };

        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();

        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        let lro = LongRunningOperation {
            url: operation_url.clone(),
            retry_after: self.pending_operation_retry_after.map(Duration::from_secs),
            location_url: None,
        };

        // Poll ARM operation.
        let op_status = operation_client
            .check_status(&lro, "CreateContainerApp", container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure ARM operation failed for container app creation".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if op_status.is_some() {
            info!(name=%container_app_name, "LRO completed – checking resource status");
            Ok(HandlerAction::Continue {
                state: CreatingContainerApp,
                suggested_delay: None,
            })
        } else {
            // Still running – schedule another poll.
            let delay = self
                .pending_operation_retry_after
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15));
            Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            })
        }
    }

    pub(super) async fn creating_container_app_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();
        let resource_group_name = get_resource_group_name(ctx.state)?;

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .get_container_app(&resource_group_name, container_app_name)
            .await
        {
            Ok(app) => {
                if let Some(props) = &app.properties {
                    match props.provisioning_state.as_ref() {
                        Some(ContainerAppPropertiesProvisioningState::Succeeded) => {
                            info!(name=%container_app_name, "Provisioning succeeded – configuring Dapr components");
                            self.handle_creation_completed(ctx, &app);

                            // Branch based on ingress type
                            // If public, resolve domain and proceed to certificate flow
                            // If private, skip directly to Dapr component configuration
                            if !func_cfg.public_endpoints.is_empty() {
                                match Self::resolve_domain_info(ctx, &func_cfg.id) {
                                    Ok(domain_info) => {
                                        info!(fqdn=%domain_info.fqdn, "Resolved domain for public worker");
                                        self.fqdn = Some(domain_info.fqdn);
                                        self.certificate_id = domain_info.certificate_id;
                                        self.keyvault_cert_id = domain_info.keyvault_cert_id;
                                        self.container_apps_certificate_id =
                                            domain_info.container_apps_certificate_id;
                                        self.uses_custom_domain = domain_info.uses_custom_domain;

                                        // Proceed to certificate flow
                                        return Ok(HandlerAction::Continue {
                                            state: WaitingForCertificate,
                                            suggested_delay: None,
                                        });
                                    }
                                    Err(e) => {
                                        warn!(
                                            "Failed to resolve domain info, skipping custom domain setup: {}",
                                            e
                                        );
                                        // Continue without custom domain
                                    }
                                }
                            }

                            Ok(HandlerAction::Continue {
                                state: ConfiguringDaprComponents,
                                suggested_delay: None,
                            })
                        }
                        Some(ContainerAppPropertiesProvisioningState::InProgress) => {
                            debug!(name=%container_app_name, "Provisioning still in progress");
                            Ok(HandlerAction::Stay {
                                max_times: Some(60),
                                suggested_delay: Some(Duration::from_secs(10)),
                            })
                        }
                        Some(ContainerAppPropertiesProvisioningState::Failed) => {
                            error!(name=%container_app_name, "Container app provisioning failed");
                            Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: "Container app provisioning failed".to_string(),
                                resource_id: Some(func_cfg.id.clone()),
                            }))
                        }
                        _ => Ok(HandlerAction::Stay {
                            max_times: Some(60),
                            suggested_delay: Some(Duration::from_secs(10)),
                        }),
                    }
                } else {
                    debug!(name=%container_app_name, "Properties missing – retry");
                    Ok(HandlerAction::Stay {
                        max_times: Some(60),
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                debug!(name=%container_app_name, "Resource not yet visible – retry");
                Ok(HandlerAction::Stay {
                    max_times: Some(60),
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Error checking container app status".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })),
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
                state: ConfiguringDaprComponents,
                suggested_delay: None,
            });
        }
        if self.uses_custom_domain && self.keyvault_cert_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: ConfiguringCustomDomain,
                suggested_delay: None,
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
        let azure_cfg = ctx.get_azure_config()?;
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

        // Convert PEM to PKCS#12 for Azure Key Vault
        let pkcs12_data = pem_to_pkcs12(private_key, certificate_chain)?;
        let pkcs12_base64 = base64::engine::general_purpose::STANDARD.encode(&pkcs12_data);

        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let location = azure_cfg.region.as_deref().unwrap_or("East US").to_string();
        let certificate = ManagedEnvironmentCertificate {
            location,
            properties: Some(ManagedEnvironmentCertificateProperties {
                value: Some(pkcs12_base64),
                password: Some(String::new()),
                certificate_key_vault_properties: None,
            }),
            tags: HashMap::new(),
        };

        let container_apps_client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;
        let response = container_apps_client
            .create_or_update_managed_environment_certificate(
                &resource_group_name,
                &environment_name,
                &certificate_name,
                &certificate,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import certificate to Azure Container Apps Environment"
                    .to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        self.container_apps_certificate_id = Some(response.id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Azure Container Apps Environment certificate response missing ID"
                    .to_string(),
                resource_id: Some(worker_config.id.clone()),
            })
        })?);

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        info!(
            worker=%worker_config.id,
            cert_id=?self.container_apps_certificate_id,
            "Certificate imported to Azure Container Apps Environment"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForDns,
            suggested_delay: None,
        })
    }

    pub(super) async fn configuring_custom_domain_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Container app name not set".to_string(),
            })
        })?;

        let fqdn = self.fqdn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "FQDN not set".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        let environment_name = get_container_apps_environment_name(ctx.state)?;
        if self.container_apps_certificate_id.is_none() {
            let keyvault_cert_id = self.keyvault_cert_id.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: worker_config.id.clone(),
                    message: "Container Apps certificate ID not set".to_string(),
                })
            })?;
            let rsm_ref = ResourceRef::new(
                RemoteStackManagement::RESOURCE_TYPE,
                "remote-stack-management",
            );
            let rsm_outputs = ctx
                .state
                .get_resource_outputs::<RemoteStackManagementOutputs>(rsm_ref.id())
                .context(ErrorData::DependencyNotReady {
                    resource_id: worker_config.id.clone(),
                    dependency_id: rsm_ref.id().to_string(),
                })?;
            let certificate_name =
                get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);
            let certificate = ManagedEnvironmentCertificate {
                location: azure_cfg.region.as_deref().unwrap_or("East US").to_string(),
                properties: Some(ManagedEnvironmentCertificateProperties {
                    value: None,
                    password: None,
                    certificate_key_vault_properties: Some(
                        ManagedEnvironmentCertificateKeyVaultProperties {
                            identity: rsm_outputs.management_resource_id.clone(),
                            key_vault_url: keyvault_cert_id.clone(),
                        },
                    ),
                }),
                tags: HashMap::new(),
            };
            let response = client
                .create_or_update_managed_environment_certificate(
                    &resource_group_name,
                    &environment_name,
                    &certificate_name,
                    &certificate,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message:
                        "Failed to import Key Vault certificate to Azure Container Apps Environment"
                            .to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })?;
            self.container_apps_certificate_id = Some(response.id.ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Azure Container Apps Environment certificate response missing ID"
                        .to_string(),
                    resource_id: Some(worker_config.id.clone()),
                })
            })?);
        }
        let container_apps_certificate_id =
            self.container_apps_certificate_id.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: worker_config.id.clone(),
                    message: "Container Apps certificate ID not set".to_string(),
                })
            })?;

        let mut app = self
            .build_container_app(
                worker_config,
                &environment_name,
                container_app_name,
                azure_cfg,
                ctx,
            )
            .await?;
        Self::set_custom_domain(
            &mut app,
            fqdn.clone(),
            container_apps_certificate_id.clone(),
        );

        // Update the container app with custom domain
        let _operation = client
            .create_or_update_container_app(&resource_group_name, container_app_name, &app)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to configure custom domain for container app".to_string(),
                resource_id: Some(worker_config.id.clone()),
            })?;

        info!(
            worker=%worker_config.id,
            fqdn=%fqdn,
            "Custom domain configured for container app"
        );

        Ok(HandlerAction::Continue {
            state: ConfiguringDaprComponents,
            suggested_delay: None,
        })
    }
}
