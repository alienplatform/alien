use super::*;
use super::{AzureWorkerHandlerAction as HandlerAction, AzureWorkerState::*};

impl AzureWorkerController {
    pub(super) async fn delete_start_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;

        if self.pending_operation_url.is_some() {
            return Ok(HandlerAction::Continue {
                state: WaitingForPendingOperationBeforeDelete,
                suggested_delay: self.pending_operation_retry_after.map(Duration::from_secs),
            });
        }

        // Handle case where container_app_name is not set (e.g., creation failed early)
        let _container_app_name = match self.container_app_name.as_ref() {
            Some(name) => name.clone(),
            None => {
                // No container app was created, nothing to delete
                info!(resource_id=%func_cfg.id, "No Container App to delete - creation failed early");

                // Clear any remaining state and mark as deleted
                self.clear_all();

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        // Always go to deleting Dapr components first (linear flow)
        Ok(HandlerAction::Continue {
            state: DeletingDaprComponents,
            suggested_delay: None,
        })
    }

    pub(super) async fn waiting_for_pending_operation_before_delete_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "CompleteOperationBeforeWorkerDelete",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "waiting_for_pending_operation_before_delete",
                failure_message: "Azure ARM operation failed before worker deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_pending_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: DeleteStart,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeleteStart,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn deleting_dapr_components_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;

        info!(worker=%worker_config.id, components=?self.dapr_components, "Deleting Dapr components");

        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker_config.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;
        if self.initialize_dapr_component_deletion_candidates(worker_config, &container_app_name) {
            return Ok(HandlerAction::Continue {
                state: DeletingDaprComponents,
                suggested_delay: None,
            });
        }
        if self
            .initialize_auxiliary_teardown_candidates(ctx, worker_config, &container_app_name)
            .await?
        {
            return Ok(HandlerAction::Continue {
                state: DeletingDaprComponents,
                suggested_delay: None,
            });
        }

        if matches!(
            self.delete_storage_trigger_infrastructure(ctx).await?,
            StorageTriggerTeardownResult::Mutated
        ) {
            return Ok(HandlerAction::Continue {
                state: DeletingDaprComponents,
                suggested_delay: None,
            });
        }

        match self.delete_all_dapr_components(ctx).await? {
            TrackedDaprComponentDeleteStep::Complete => {}
            TrackedDaprComponentDeleteStep::Mutated => {
                return Ok(HandlerAction::Continue {
                    state: DeletingDaprComponents,
                    suggested_delay: None,
                });
            }
            TrackedDaprComponentDeleteStep::LongRunning {
                operation,
                component_name,
            } => {
                let delay = operation.retry_after.unwrap_or(Duration::from_secs(15));
                self.pending_operation_url = Some(operation.url);
                self.pending_operation_retry_after = operation
                    .retry_after
                    .map(|retry_after| retry_after.as_secs());
                self.pending_dapr_component_deletion_name = Some(component_name);
                return Ok(HandlerAction::Continue {
                    state: WaitingForDaprComponentDeletion,
                    suggested_delay: Some(delay),
                });
            }
        }

        // Continue to commands infrastructure cleanup
        Ok(HandlerAction::Continue {
            state: DeletingCommandsInfrastructure,
            suggested_delay: None,
        })
    }

    pub(super) async fn waiting_for_dapr_component_deletion_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "waiting_for_dapr_component_deletion",
                failure_message: "Azure ARM operation failed for Dapr component deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_pending_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: DeletingDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn deleting_commands_infrastructure_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self.delete_commands_infrastructure_step(ctx).await? {
            CommandsTeardownResult::Complete => Ok(HandlerAction::Continue {
                state: DeletingApp,
                suggested_delay: None,
            }),
            CommandsTeardownResult::Mutated => Ok(HandlerAction::Continue {
                state: DeletingCommandsInfrastructure,
                suggested_delay: None,
            }),
            CommandsTeardownResult::LongRunning(delay) => Ok(HandlerAction::Continue {
                state: WaitingForCommandsDaprComponentDeletion,
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn waiting_for_commands_dapr_component_deletion_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "waiting_for_commands_dapr_component_deletion",
                failure_message: "Azure ARM operation failed for commands Dapr deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_commands_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: DeletingCommandsInfrastructure,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingCommandsInfrastructure,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn update_deleting_commands_infrastructure_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self.delete_commands_infrastructure_step(ctx).await? {
            // Keep the checkpoint latched until the next UpdateStart. `commands_changed` remains
            // true for this whole update, so clearing it here would restart teardown forever.
            CommandsTeardownResult::Complete => Ok(HandlerAction::Continue {
                state: UpdateDaprComponents,
                suggested_delay: None,
            }),
            CommandsTeardownResult::Mutated => Ok(HandlerAction::Continue {
                state: UpdateDeletingCommandsInfrastructure,
                suggested_delay: None,
            }),
            CommandsTeardownResult::LongRunning(delay) => Ok(HandlerAction::Continue {
                state: UpdateWaitingForCommandsDaprComponentDeletion,
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn update_waiting_for_commands_dapr_component_deletion_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteDaprComponent",
                operation_target: &worker.id,
                resource_id: &worker.id,
                handler_name: "update_waiting_for_commands_dapr_component_deletion",
                failure_message: "Azure ARM operation failed for commands Dapr update deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.complete_commands_dapr_component_deletion();
                Ok(HandlerAction::Continue {
                    state: UpdateDeletingCommandsInfrastructure,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: UpdateDeletingCommandsInfrastructure,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn update_waiting_for_commands_dapr_component_deletion_for_setup_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "update_waiting_for_commands_dapr_component_deletion_for_setup",
                "Azure ARM operation failed for commands setup Dapr deletion",
            )
            .await?
        {
            None => Ok(HandlerAction::Continue {
                state: UpdateDaprComponents,
                suggested_delay: None,
            }),
            Some(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn deleting_app_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .delete_container_app(&resource_group_name, &container_app_name)
            .await
        {
            Ok(OperationResult::Completed(_)) => {
                info!(name=%container_app_name, "Container app deleted immediately");
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            Ok(OperationResult::LongRunning(lro)) => {
                debug!(name=%container_app_name, "Deletion is long‑running");
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                Ok(HandlerAction::Continue {
                    state: WaitingForDeleteOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(name=%container_app_name, "Container app already deleted");
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                let worker_config = ctx.desired_resource_config::<Worker>()?;
                Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete container app".to_string(),
                    resource_id: Some(worker_config.id.clone()),
                }))
            }
        }
    }

    pub(super) async fn waiting_for_delete_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker = ctx.desired_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker.id.clone(),
                message: "Container app name not set while polling deletion".to_string(),
            })
        })?;
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteContainerApp",
                operation_target: container_app_name,
                resource_id: &worker.id,
                handler_name: "waiting_for_delete_operation",
                failure_message: "Azure ARM operation failed for container app deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingContainerApp,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingApp,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn deleting_container_app_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = match &self.container_app_name {
            Some(n) => n.clone(),
            None => {
                return Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                });
            }
        };
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .get_container_app(&resource_group_name, &container_app_name)
            .await
        {
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(name=%container_app_name, "Container app confirmed deleted");
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            Ok(_) => {
                debug!(name=%container_app_name, "Container app still exists – retry");
                Ok(HandlerAction::Stay {
                    max_times: Some(60),
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Error checking container app deletion status".to_string(),
                resource_id: Some(ctx.desired_resource_config::<Worker>()?.id.clone()),
            })),
        }
    }

    pub(super) async fn deleting_certificate_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let has_resolvable_domain = !worker_config.public_endpoints.is_empty()
            && Self::resolve_domain_info(ctx, &worker_config.id).is_ok();
        if self.container_apps_certificate_id.is_none()
            && !self.uses_custom_domain
            && !has_resolvable_domain
        {
            self.clear_all();
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .delete_managed_environment_certificate(
                &resource_group_name,
                &environment_name,
                &certificate_name,
            )
            .await
        {
            Ok(OperationResult::Completed(())) => {
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Ok(OperationResult::LongRunning(lro)) => {
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                Ok(HandlerAction::Continue {
                    state: WaitingForCertificateDeleteOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Failed to delete Container Apps managed environment certificate"
                    .to_string(),
                resource_id: Some(worker_config.id.clone()),
            })),
        }
    }

    pub(super) async fn waiting_for_certificate_delete_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let worker_config = ctx.desired_resource_config::<Worker>()?;
        let certificate_name =
            get_container_apps_certificate_name(ctx.resource_prefix, &worker_config.id);
        match poll_reconciled_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "DeleteManagedEnvironmentCertificate",
                operation_target: &certificate_name,
                resource_id: &worker_config.id,
                handler_name: "waiting_for_certificate_delete_operation",
                failure_message:
                    "Azure ARM operation failed for managed environment certificate deletion",
            },
        )
        .await?
        {
            AzureOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: DeletingCertificate,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(60),
                suggested_delay: Some(delay),
            }),
        }
    }

    // ─────────────── TERMINALS ────────────────────────────────
}
