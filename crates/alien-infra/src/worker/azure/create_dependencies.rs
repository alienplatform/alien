use super::*;
use super::{AzureWorkerHandlerAction as HandlerAction, AzureWorkerState::*};

impl AzureWorkerController {
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
            Some(DnsRecordStatus::Active) => {
                info!(
                    worker=%worker_config.id,
                    fqdn=%self.fqdn.as_ref().unwrap_or(&"unknown".to_string()),
                    "DNS record created successfully"
                );
                if self.container_apps_certificate_id.is_some() {
                    return Ok(HandlerAction::Continue {
                        state: ConfiguringCustomDomain,
                        suggested_delay: None,
                    });
                }
                Ok(HandlerAction::Continue {
                    state: ConfiguringDaprComponents,
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

    pub(super) async fn configuring_dapr_components_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        if let Some(delay) = self
            .wait_for_container_apps_environment_wake_retry(&func_cfg.id, "Dapr component creation")
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        info!(name=%container_app_name, "Configuring Dapr components for triggers");

        // Create Dapr components for all trigger types
        let mut created_any = false;
        let mut cron_index = 0usize;
        for trigger in &func_cfg.triggers {
            match trigger {
                alien_core::WorkerTrigger::Queue { queue } => {
                    info!(worker=%func_cfg.id, queue=%queue.id, "Creating Dapr Service Bus component");
                    let operation = match self
                        .create_dapr_service_bus_component(
                            ctx,
                            &container_app_name,
                            &func_cfg,
                            queue,
                        )
                        .await
                    {
                        Ok(operation) => operation,
                        Err(e) => {
                            if is_azure_container_apps_environment_waking_error(&e) {
                                let deadline = ensure_rbac_wait_deadline(
                                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                                );
                                if let Some(delay) =
                                    self.record_container_apps_environment_wake_retry(deadline)
                                {
                                    warn!(
                                        worker=%func_cfg.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component creation"
                                    );
                                    return Ok(HandlerAction::Stay {
                                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    match operation {
                        DaprComponentOperation::Creating(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentCreateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForLegacyDaprComponentDeletionDuringCreate,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Pending(delay) => {
                            return Ok(HandlerAction::Stay {
                                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Completed => {}
                    }
                    created_any = true;
                }
                alien_core::WorkerTrigger::Storage { storage, events } => {
                    info!(worker=%func_cfg.id, storage=%storage.id, "Creating Azure storage trigger delivery");
                    let operation = match self
                        .create_azure_storage_trigger(
                            ctx,
                            &container_app_name,
                            &func_cfg,
                            storage,
                            events,
                        )
                        .await
                    {
                        Ok(operation) => operation,
                        Err(e) => {
                            if is_azure_container_apps_environment_waking_error(&e) {
                                let deadline = ensure_rbac_wait_deadline(
                                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                                );
                                if let Some(delay) =
                                    self.record_container_apps_environment_wake_retry(deadline)
                                {
                                    warn!(
                                        worker=%func_cfg.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component creation"
                                    );
                                    return Ok(HandlerAction::Stay {
                                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    match operation {
                        DaprComponentOperation::Creating(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentCreateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForLegacyDaprComponentDeletionDuringCreate,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Pending(delay) => {
                            return Ok(HandlerAction::Stay {
                                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Completed => {}
                    }
                    created_any = true;
                }
                alien_core::WorkerTrigger::Schedule { cron } => {
                    info!(worker=%func_cfg.id, cron=%cron, "Creating Dapr cron component");
                    let operation = match self
                        .create_dapr_cron_component(
                            ctx,
                            &container_app_name,
                            &func_cfg,
                            cron,
                            cron_index,
                        )
                        .await
                    {
                        Ok(operation) => operation,
                        Err(e) => {
                            if is_azure_container_apps_environment_waking_error(&e) {
                                let deadline = ensure_rbac_wait_deadline(
                                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                                );
                                if let Some(delay) =
                                    self.record_container_apps_environment_wake_retry(deadline)
                                {
                                    warn!(
                                        worker=%func_cfg.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component creation"
                                    );
                                    return Ok(HandlerAction::Stay {
                                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                        suggested_delay: Some(delay),
                                    });
                                }
                            }
                            return Err(e);
                        }
                    };
                    match operation {
                        DaprComponentOperation::Creating(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForDaprComponentCreateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: WaitingForLegacyDaprComponentDeletionDuringCreate,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Pending(delay) => {
                            return Ok(HandlerAction::Stay {
                                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Completed => {}
                    }
                    cron_index += 1;
                    created_any = true;
                }
            }
        }

        if !created_any {
            info!(worker=%func_cfg.id, "No triggers found, skipping Dapr component creation");
        }
        self.container_apps_environment_wake_wait_until_epoch_secs = None;
        self.container_apps_environment_wake_retry_after_epoch_secs = None;

        // Go to commands infrastructure next
        Ok(HandlerAction::Continue {
            state: CreatingCommandsInfrastructure,
            suggested_delay: None,
        })
    }

    pub(super) async fn waiting_for_dapr_component_create_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let operation_url = self.pending_operation_url.clone().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for Dapr component".to_string(),
                operation: Some("waiting_for_dapr_component_create_operation".to_string()),
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
                message: "Azure ARM operation failed for Dapr component creation".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if status.is_some() {
            self.pending_operation_url = None;
            self.pending_operation_retry_after = None;
            Ok(HandlerAction::Continue {
                state: ConfiguringDaprComponents,
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

    pub(super) async fn waiting_for_legacy_dapr_component_deletion_during_create_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "waiting_for_legacy_dapr_component_deletion_during_create",
                "Azure ARM operation failed for legacy Dapr component deletion during create",
            )
            .await?
        {
            None => Ok(HandlerAction::Continue {
                state: ConfiguringDaprComponents,
                suggested_delay: None,
            }),
            Some(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn creating_commands_infrastructure_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        if let Some(delay) = self
            .wait_for_container_apps_environment_wake_retry(&func_cfg.id, "commands Dapr component")
        {
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        if !func_cfg.commands_enabled {
            debug!(worker=%func_cfg.id, "Commands not enabled, skipping commands infrastructure");
            return Ok(HandlerAction::Continue {
                state: ApplyingPermissions,
                suggested_delay: None,
            });
        }

        let azure_config = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: func_cfg.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;
        match self
            .setup_commands_infrastructure(ctx, azure_config, func_cfg, &container_app_name)
            .await
        {
            Ok(CommandsSetupOperation::Completed) => Ok(HandlerAction::Continue {
                state: ApplyingPermissions,
                suggested_delay: None,
            }),
            Ok(CommandsSetupOperation::Creating(delay)) => Ok(HandlerAction::Continue {
                state: WaitingForCommandsDaprComponentOperation,
                suggested_delay: Some(delay),
            }),
            Ok(CommandsSetupOperation::Deleting(delay)) => Ok(HandlerAction::Continue {
                state: WaitingForLegacyCommandsDaprComponentDeletionDuringCreate,
                suggested_delay: Some(delay),
            }),
            Ok(CommandsSetupOperation::Pending(delay)) => Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            }),
            Err(error) if is_azure_authorization_propagation_error(&error) => {
                let deadline = ensure_rbac_wait_deadline(
                    &mut self.commands_infrastructure_auth_wait_until_epoch_secs,
                    AZURE_COMMANDS_INFRASTRUCTURE_AUTH_WAIT_SECS,
                );
                if let Some(delay) = rbac_wait_delay(deadline) {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
                Err(error)
            }
            Err(error) if is_azure_container_apps_environment_waking_error(&error) => {
                let deadline = ensure_rbac_wait_deadline(
                    &mut self.container_apps_environment_wake_wait_until_epoch_secs,
                    AZURE_CONTAINER_APPS_ENVIRONMENT_WAKE_WAIT_SECS,
                );
                if let Some(delay) = self.record_container_apps_environment_wake_retry(deadline) {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) async fn waiting_for_commands_dapr_component_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let operation_url = self.pending_operation_url.clone().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "No pending operation URL recorded for commands Dapr component"
                    .to_string(),
                operation: Some("waiting_for_commands_dapr_component_operation".to_string()),
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
                message: "Azure ARM operation failed for commands Dapr component".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if status.is_some() {
            self.pending_operation_url = None;
            self.pending_operation_retry_after = None;
            Ok(HandlerAction::Continue {
                state: CreatingCommandsInfrastructure,
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

    pub(super) async fn waiting_for_legacy_commands_dapr_component_deletion_during_create_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "waiting_for_legacy_commands_dapr_component_deletion_during_create",
                "Azure ARM operation failed for legacy commands Dapr deletion during create",
            )
            .await?
        {
            None => Ok(HandlerAction::Continue {
                state: CreatingCommandsInfrastructure,
                suggested_delay: None,
            }),
            Some(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn running_readiness_probe_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        // If no readiness probe is configured, the worker is ready.
        if func_cfg.readiness_probe.is_none() {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        // Only run probe for public ingress where we have a URL.
        if func_cfg.public_endpoints.is_empty() {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let url = match &self.url {
            Some(u) => u.clone(),
            None => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Readiness probe configured but URL is missing".to_string(),
                }));
            }
        };

        match run_readiness_probe(ctx, &url).await {
            Ok(()) => {
                info!(name=%func_cfg.id, "Readiness probe succeeded");

                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            Err(_) => {
                // Probe failed, let the framework handle retries
                Ok(HandlerAction::Stay {
                    max_times: Some(READINESS_PROBE_MAX_ATTEMPTS as u32),
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        }
    }

    pub(super) async fn applying_permissions_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        info!(name=%func_cfg.id, "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let Some(container_app_name) = &self.container_app_name {
            use crate::core::ResourcePermissionsHelper;
            use alien_azure_clients::authorization::Scope;

            let config = ctx.desired_resource_config::<Worker>()?;
            let deployment_rg = azure_utils::get_resource_group_name(ctx.state)?;

            // Build Azure resource scope for the container app
            let resource_scope = Scope::Resource {
                resource_group_name: deployment_rg.clone(),
                resource_provider: "Microsoft.App".to_string(),
                parent_resource_path: None,
                resource_type: "containerApps".to_string(),
                resource_name: container_app_name.to_string(),
            };

            ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
                ctx,
                &config.id,
                container_app_name,
                resource_scope,
                "Worker",
                "worker",
            )
            .await?;
        }

        info!(name=%func_cfg.id, "Successfully applied resource-scoped permissions");

        // The worker is the consumer of every upstream Azure role assignment
        // applied during this application install (storage, KV, queue, vault,
        // plus the worker/execute role just applied above). Azure RBAC propagation can
        // take 2-5 minutes for resource-scope assignments — caller code that
        // invokes the worker within seconds otherwise hits 403
        // `AuthorizationPermissionMismatch`. Wait here, in the consumer, before
        // running readiness probes or signalling Ready.
        self.ready_rbac_wait_until_epoch_secs = None;
        Ok(HandlerAction::Continue {
            state: WaitingForRbacPropagation,
            suggested_delay: None,
        })
    }

    pub(super) async fn waiting_for_rbac_propagation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let deadline = ensure_rbac_wait_deadline(
            &mut self.ready_rbac_wait_until_epoch_secs,
            AZURE_READY_RBAC_WAIT_SECS,
        );

        if let Some(delay) = rbac_wait_delay(deadline) {
            info!(
                name=%func_cfg.id,
                remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                "Waiting for Azure RBAC propagation before marking worker Ready"
            );
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        self.ready_rbac_wait_until_epoch_secs = None;
        Ok(HandlerAction::Continue {
            state: RunningReadinessProbe,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────

    pub(super) async fn ready_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        let storage_trigger_count = func_cfg
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::WorkerTrigger::Storage { .. }))
            .count();
        let needs_auxiliary_checkpoint = !self.auxiliary_teardown_candidates_initialized
            && (storage_trigger_count > 0
                || self.dapr_component_naming_version < CURRENT_DAPR_COMPONENT_NAMING_VERSION);
        let needs_storage_delivery_reconciliation = storage_trigger_count
            != self.storage_trigger_infrastructure.len()
            || self
                .storage_trigger_infrastructure
                .iter()
                .any(|target| !target.delivery_reconciled || target.storage_id.is_none());
        if needs_auxiliary_checkpoint || needs_storage_delivery_reconciliation {
            let container_app_name = self.container_app_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Container app name not set in state".to_string(),
                })
            })?;
            if needs_auxiliary_checkpoint {
                self.initialize_auxiliary_teardown_candidates(ctx, func_cfg, &container_app_name)
                    .await?;
                return Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                });
            }
            for trigger in &func_cfg.triggers {
                let alien_core::WorkerTrigger::Storage { storage, events } = trigger else {
                    continue;
                };
                let desired = self
                    .desired_storage_trigger_target(ctx, func_cfg, &container_app_name, storage)
                    .await?;
                if matches!(
                    self.prepare_storage_trigger_target(ctx, &desired.infrastructure)
                        .await?,
                    StorageTargetPreparation::Pending
                ) {
                    return Ok(HandlerAction::Continue {
                        state: Ready,
                        suggested_delay: None,
                    });
                }
                match self
                    .ensure_storage_delivery_infrastructure(
                        ctx, func_cfg, storage, events, &desired,
                    )
                    .await?
                {
                    StorageDeliveryReconcileResult::Complete => {}
                    StorageDeliveryReconcileResult::Pending(delay) => {
                        return Ok(HandlerAction::Continue {
                            state: Ready,
                            suggested_delay: Some(delay),
                        });
                    }
                }
            }
        }
        if self.dapr_component_naming_version < CURRENT_DAPR_COMPONENT_NAMING_VERSION {
            info!(
                worker=%func_cfg.id,
                from_version=self.dapr_component_naming_version,
                to_version=CURRENT_DAPR_COMPONENT_NAMING_VERSION,
                "Migrating Dapr component names"
            );
            return Ok(HandlerAction::Continue {
                state: MigratingDaprComponentNames,
                suggested_delay: None,
            });
        }
        if func_cfg.commands_enabled
            && (self.commands_resource_group_name.is_none()
                || self.commands_namespace_name.is_none()
                || self.commands_queue_name.is_none())
        {
            let container_app_name = self.container_app_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Container app name not set in state".to_string(),
                })
            })?;
            self.initialize_commands_teardown_candidates(ctx, func_cfg, &container_app_name)
                .await?;
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }
        if !matches!(
            self.reconcile_commands_sender_role_assignment(ctx, func_cfg)
                .await?,
            CommandsSenderReconcileResult::Complete
        ) {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }
        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: func_cfg.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        // Heartbeat check: verify Container App still exists and is in correct state
        let container_app = client
            .get_container_app(&resource_group_name, container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Container App during heartbeat check".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        // Verify Container App is in Succeeded state - drift is non-retryable
        if let Some(properties) = &container_app.properties {
            use alien_azure_clients::models::container_apps::ContainerAppPropertiesProvisioningState;
            if properties.provisioning_state
                == Some(ContainerAppPropertiesProvisioningState::Failed)
            {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: func_cfg.id.clone(),
                    message: "Container App is in Failed state".to_string(),
                }));
            }
        }

        // Imported workers skip the create flow, so the heartbeat is where they pick up the ingress
        // host (the DNS CNAME target — see `container_app_url`).
        self.container_app_url = self.extract_url_from_container_app(&container_app);

        // Check for certificate renewal on auto-managed public domains.
        if !func_cfg.public_endpoints.is_empty() && !self.uses_custom_domain {
            let metadata = ctx
                .deployment_config
                .domain_metadata
                .as_ref()
                .and_then(|meta| meta.resources.get(&func_cfg.id));

            if let Some(resource) = metadata {
                // Check if certificate has been renewed (issued_at timestamp changed)
                if let Some(new_issued_at) = &resource.issued_at {
                    if self.certificate_issued_at.as_ref() != Some(new_issued_at) {
                        info!(
                            worker=%func_cfg.id,
                            old_issued_at=?self.certificate_issued_at,
                            new_issued_at=%new_issued_at,
                            "Certificate renewed, triggering update to re-import certificate"
                        );
                        return Ok(HandlerAction::Continue {
                            state: UpdateImportingCertificate,
                            suggested_delay: None,
                        });
                    }
                }
            }
        }

        emit_azure_container_apps_worker_heartbeat(
            ctx,
            &func_cfg,
            container_app_name,
            &container_app,
        );

        debug!(name = %func_cfg.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }
}
