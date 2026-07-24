use super::*;
use super::{AzureWorkerHandlerAction as HandlerAction, AzureWorkerState::*};

impl AzureWorkerController {
    pub(super) async fn migrating_dapr_component_names_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self.migrate_dapr_component_names(ctx).await? {
            DaprComponentMigrationStep::Complete => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            }),
            DaprComponentMigrationStep::Mutated => Ok(HandlerAction::Continue {
                state: MigratingDaprComponentNames,
                suggested_delay: None,
            }),
            DaprComponentMigrationStep::LongRunning {
                operation,
                deleted_component,
            } => {
                let delay = operation.retry_after.unwrap_or(Duration::from_secs(15));
                self.pending_operation_url = Some(operation.url);
                self.pending_operation_retry_after = operation
                    .retry_after
                    .map(|retry_after| retry_after.as_secs());
                self.pending_dapr_component_deletion_name = deleted_component;
                Ok(HandlerAction::Continue {
                    state: WaitingForDaprComponentNameMigrationOperation,
                    suggested_delay: Some(delay),
                })
            }
        }
    }

    pub(super) async fn waiting_for_dapr_component_name_migration_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "MigrateDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "waiting_for_dapr_component_name_migration_operation",
                failure_message: "Azure ARM operation failed during Dapr name migration",
                // A missing operation URL is not proof that either a create or
                // delete finished. Re-enter migration and let ownership GETs
                // plus idempotent ensure/delete determine the remote state.
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_pending_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: MigratingDaprComponentNames,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: MigratingDaprComponentNames,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    pub(super) async fn update_importing_certificate_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        if func_cfg.public_endpoints.is_empty() || self.uses_custom_domain {
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }

        let Some(resource) = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&func_cfg.id))
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

        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        let pkcs12_data = pem_to_pkcs12(private_key, certificate_chain)?;
        let pkcs12_base64 = base64::engine::general_purpose::STANDARD.encode(&pkcs12_data);
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &func_cfg.id);
        let certificate = ManagedEnvironmentCertificate {
            location: azure_cfg.region.as_deref().unwrap_or("East US").to_string(),
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
                message: "Failed to re-import certificate to Azure Container Apps Environment"
                    .to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        let container_apps_certificate_id = response.id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Azure Container Apps Environment certificate response missing ID"
                    .to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;

        if self.fqdn.is_some() {
            let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Container app name not set in state".to_string(),
                })
            })?;
            let fqdn = self.fqdn.clone().unwrap();
            let environment_name = get_container_apps_environment_name(ctx.state)?;
            let mut app = self
                .build_container_app(
                    func_cfg,
                    &environment_name,
                    container_app_name,
                    azure_cfg,
                    ctx,
                )
                .await?;
            Self::set_custom_domain(&mut app, fqdn, container_apps_certificate_id.clone());

            container_apps_client
                .create_or_update_container_app(&resource_group_name, container_app_name, &app)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to bind renewed certificate to custom domain".to_string(),
                    resource_id: Some(func_cfg.id.clone()),
                })?;
        }

        self.container_apps_certificate_id = Some(container_apps_certificate_id);
        self.certificate_issued_at = resource.issued_at.clone();

        Ok(HandlerAction::Continue {
            state: UpdateStart,
            suggested_delay: None,
        })
    }

    pub(super) async fn update_start_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        self.ready_rbac_wait_until_epoch_secs = None;
        self.update_dapr_components_deleted = false;
        self.commands_update_teardown_candidates_initialized = false;
        self.trigger_update_teardown_candidates_initialized = false;
        self.commands_sender_role_assignment_discovery_complete = false;
        self.commands_queue_applied = false;

        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "container_app_name missing prior to update_start".to_string(),
                operation: Some("update_start".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;
        if !self.storage_delivery_update_reconciliation_initialized {
            let mut desired_storage_targets = Vec::new();
            for trigger in &func_cfg.triggers {
                let alien_core::WorkerTrigger::Storage { storage, .. } = trigger else {
                    continue;
                };
                desired_storage_targets.push(
                    self.desired_storage_trigger_target(
                        ctx,
                        func_cfg,
                        &container_app_name,
                        storage,
                    )
                    .await?
                    .infrastructure,
                );
            }
            for desired in desired_storage_targets {
                if let Some(tracked) = self
                    .storage_trigger_infrastructure
                    .iter_mut()
                    .find(|tracked| tracked.matches_target(&desired))
                {
                    tracked.queue_applied = false;
                    tracked.delivery_reconciled = false;
                }
            }
            self.storage_delivery_update_reconciliation_initialized = true;
            return Ok(HandlerAction::Continue {
                state: UpdateStart,
                suggested_delay: None,
            });
        }
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;
        self.update_rbac_wait_required = true;

        // Build desired spec
        let desired_app = self
            .build_container_app(
                func_cfg,
                &environment_name,
                &container_app_name,
                azure_cfg,
                ctx,
            )
            .await?;
        let mut desired_app = desired_app;
        if let (Some(fqdn), Some(keyvault_cert_id)) = (&self.fqdn, &self.keyvault_cert_id) {
            Self::set_custom_domain(&mut desired_app, fqdn.clone(), keyvault_cert_id.clone());
        }

        // Issue UPDATE
        let op_result = client
            .update_container_app(&resource_group_name, &container_app_name, &desired_app)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to initiate container app update".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        match op_result {
            OperationResult::Completed(_) => {
                info!(name=%container_app_name, "Update completed immediately – polling app status");
                Ok(HandlerAction::Continue {
                    state: UpdatingContainerApp,
                    suggested_delay: Some(Duration::from_secs(3)),
                })
            }
            OperationResult::LongRunning(lro) => {
                debug!(name=%container_app_name, "Update is long‑running");
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());

                Ok(HandlerAction::Continue {
                    state: WaitingForUpdateOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
        }
    }

    pub(super) async fn waiting_for_update_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker.id.clone(),
                message: "Container app name not set while polling update".to_string(),
            })
        })?;
        match poll_pending_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "UpdateContainerApp",
                operation_target: container_app_name,
                resource_id: &worker.id,
                handler_name: "waiting_for_update_operation",
                failure_message: "Azure ARM operation failed for container app update",
            },
        )
        .await?
        {
            AzureStrictOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: UpdatingContainerApp,
                    suggested_delay: None,
                })
            }
            AzureStrictOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn updating_container_app_impl(
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

        let app = client
            .get_container_app(&resource_group_name, container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Error checking container app update status".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if let Some(props) = &app.properties {
            match props.provisioning_state.as_ref() {
                Some(ContainerAppPropertiesProvisioningState::Succeeded) => {
                    info!(name=%container_app_name, "Update provisioning succeeded – updating Dapr components");

                    let container_app_url = self.extract_url_from_container_app(&app);
                    // Capture the ingress host (DNS CNAME target) before `url` is overridden below.
                    self.container_app_url = container_app_url.clone();

                    // Check for URL override in deployment config, otherwise use Container App URL
                    self.url = ctx
                        .deployment_config
                        .public_endpoints
                        .as_ref()
                        .and_then(|resources| resources.get(&func_cfg.id))
                        .and_then(|endpoints| endpoints.values().next().cloned())
                        .or(container_app_url);

                    Ok(HandlerAction::Continue {
                        state: UpdateDaprComponents,
                        suggested_delay: None,
                    })
                }
                Some(ContainerAppPropertiesProvisioningState::InProgress) => {
                    Ok(HandlerAction::Stay {
                        max_times: Some(60),
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
                Some(ContainerAppPropertiesProvisioningState::Failed) => {
                    Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: "Container app update failed".to_string(),
                        resource_id: Some(func_cfg.id.clone()),
                    }))
                }
                _ => Ok(HandlerAction::Stay {
                    max_times: Some(60),
                    suggested_delay: Some(Duration::from_secs(10)),
                }),
            }
        } else {
            Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(Duration::from_secs(10)),
            })
        }
    }
}
