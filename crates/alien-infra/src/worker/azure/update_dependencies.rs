use super::*;
use super::{AzureWorkerHandlerAction as HandlerAction, AzureWorkerState::*};

impl AzureWorkerController {
    pub(super) async fn update_dapr_components_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let current_config = ctx.desired_resource_config::<Worker>()?;
        if let Some(delay) = self.wait_for_container_apps_environment_wake_retry(
            &current_config.id,
            "Dapr component update",
        ) {
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }
        let previous_config = ctx.previous_resource_config::<Worker>()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let permissions_changed =
            current_config.get_permissions() != previous_config.get_permissions();
        let commands_changed = current_config.commands_enabled != previous_config.commands_enabled;
        if current_config.commands_enabled {
            let azure_config = ctx.get_azure_config()?;
            match self
                .setup_commands_infrastructure(
                    ctx,
                    azure_config,
                    current_config,
                    &container_app_name,
                )
                .await?
            {
                CommandsSetupOperation::Completed => {
                    self.commands_update_teardown_candidates_initialized = false;
                }
                CommandsSetupOperation::Creating(delay) => {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForDaprComponentUpdateOperation,
                        suggested_delay: Some(delay),
                    });
                }
                CommandsSetupOperation::Deleting(delay) => {
                    return Ok(HandlerAction::Continue {
                        state: UpdateWaitingForCommandsDaprComponentDeletionForSetup,
                        suggested_delay: Some(delay),
                    });
                }
                CommandsSetupOperation::Pending(delay) => {
                    return Ok(HandlerAction::Stay {
                        max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                        suggested_delay: Some(delay),
                    });
                }
            }
        } else if !self.commands_update_teardown_candidates_initialized
            && (commands_changed
                || self.commands_dapr_component.is_some()
                || self.commands_sender_role_assignment_id.is_some()
                || self.commands_sender_role_assignment_intent.is_some()
                || self.commands_resource_group_name.is_some()
                || self.commands_namespace_name.is_some()
                || self.commands_queue_name.is_some())
        {
            self.initialize_commands_teardown_candidates(ctx, previous_config, &container_app_name)
                .await?;
            self.commands_update_teardown_candidates_initialized = true;
            return Ok(HandlerAction::Continue {
                state: UpdateDeletingCommandsInfrastructure,
                suggested_delay: None,
            });
        }

        let storage_targets_changed = self
            .storage_trigger_targets_changed(ctx, current_config, &container_app_name)
            .await?;
        let triggers_changed = current_config.triggers != previous_config.triggers
            || permissions_changed
            || storage_targets_changed;

        if triggers_changed {
            info!(worker=%current_config.id, "Worker triggers changed, updating Dapr components");

            if !self.update_dapr_components_deleted {
                if !self.trigger_update_teardown_candidates_initialized {
                    self.initialize_storage_trigger_teardown_candidates(
                        ctx,
                        previous_config,
                        &container_app_name,
                    )
                    .await?;
                    self.initialize_trigger_update_teardown_candidates(
                        previous_config,
                        &container_app_name,
                    );
                    self.trigger_update_teardown_candidates_initialized = true;
                    return Ok(HandlerAction::Continue {
                        state: UpdateDaprComponents,
                        suggested_delay: None,
                    });
                }

                // Trigger components are keyed by trigger shape. Delete the previous
                // set once, then recreate desired components across possible ARM LROs.
                if matches!(
                    self.delete_storage_trigger_infrastructure(ctx).await?,
                    StorageTriggerTeardownResult::Mutated
                ) {
                    return Ok(HandlerAction::Continue {
                        state: UpdateDaprComponents,
                        suggested_delay: None,
                    });
                }
                match self.delete_all_dapr_components(ctx).await? {
                    TrackedDaprComponentDeleteStep::Complete => {
                        self.update_dapr_components_deleted = true;
                    }
                    TrackedDaprComponentDeleteStep::Mutated => {
                        return Ok(HandlerAction::Continue {
                            state: UpdateDaprComponents,
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
                            state: WaitingForDaprComponentDeletionForUpdate,
                            suggested_delay: Some(delay),
                        });
                    }
                }
            }
        }

        // Reconcile components for all triggers. This is intentionally
        // idempotent so dependency-only changes update Dapr metadata even
        // when the Worker config itself is unchanged.
        let mut cron_index = 0usize;
        for trigger in &current_config.triggers {
            match trigger {
                alien_core::WorkerTrigger::Queue { queue } => {
                    let operation = match self
                        .create_dapr_service_bus_component(
                            ctx,
                            &container_app_name,
                            &current_config,
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
                                        worker=%current_config.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component update"
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
                                state: WaitingForDaprComponentUpdateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: UpdateWaitingForLegacyDaprComponentDeletion,
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
                }
                alien_core::WorkerTrigger::Storage { storage, events } => {
                    let operation = match self
                        .create_azure_storage_trigger(
                            ctx,
                            &container_app_name,
                            &current_config,
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
                                        worker=%current_config.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component update"
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
                                state: WaitingForDaprComponentUpdateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: UpdateWaitingForLegacyDaprComponentDeletion,
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
                }
                alien_core::WorkerTrigger::Schedule { cron } => {
                    let operation = match self
                        .create_dapr_cron_component(
                            ctx,
                            &container_app_name,
                            &current_config,
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
                                        worker=%current_config.id,
                                        error=%e,
                                        remaining_secs=deadline.saturating_sub(current_unix_timestamp_secs()),
                                        "Azure Container Apps Environment is waking; retrying Dapr component update"
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
                                state: WaitingForDaprComponentUpdateOperation,
                                suggested_delay: Some(delay),
                            });
                        }
                        DaprComponentOperation::Deleting(delay) => {
                            return Ok(HandlerAction::Continue {
                                state: UpdateWaitingForLegacyDaprComponentDeletion,
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
                }
            }
        }
        self.container_apps_environment_wake_wait_until_epoch_secs = None;
        self.container_apps_environment_wake_retry_after_epoch_secs = None;

        if !matches!(
            self.reconcile_commands_sender_role_assignment(ctx, current_config)
                .await?,
            CommandsSenderReconcileResult::Complete
        ) {
            return Ok(HandlerAction::Continue {
                state: UpdateDaprComponents,
                suggested_delay: Some(Duration::from_secs(1)),
            });
        }

        if self.update_rbac_wait_required {
            Ok(HandlerAction::Continue {
                state: UpdateWaitingForRbacPropagation,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: UpdateRunningReadinessProbe,
                suggested_delay: None,
            })
        }
    }

    pub(super) async fn waiting_for_dapr_component_deletion_for_update_impl(
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
                handler_name: "waiting_for_dapr_component_deletion_for_update",
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
                    state: UpdateDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Missing => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: UpdateDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn update_waiting_for_legacy_dapr_component_deletion_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        match self
            .wait_for_reconciled_dapr_component_deletion(
                ctx,
                "update_waiting_for_legacy_dapr_component_deletion",
                "Azure ARM operation failed for legacy Dapr component deletion during update",
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

    pub(super) async fn waiting_for_dapr_component_update_operation_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        match poll_pending_operation(
            ctx,
            self.pending_operation_url.as_deref(),
            self.pending_operation_retry_after,
            AzureOperationPollRequest {
                operation_name: "CreateOrUpdateDaprComponent",
                operation_target: &func_cfg.id,
                resource_id: &func_cfg.id,
                handler_name: "waiting_for_dapr_component_update_operation",
                failure_message: "Azure ARM operation failed for Dapr component update",
            },
        )
        .await?
        {
            AzureStrictOperationPoll::Complete => {
                self.pending_operation_url = None;
                self.pending_operation_retry_after = None;
                Ok(HandlerAction::Continue {
                    state: UpdateDaprComponents,
                    suggested_delay: None,
                })
            }
            AzureStrictOperationPoll::Pending(delay) => Ok(HandlerAction::Stay {
                max_times: Some(100),
                suggested_delay: Some(delay),
            }),
        }
    }

    pub(super) async fn update_running_readiness_probe_impl(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // Re‑use the same readiness‑probe helper.
        let func_cfg = ctx.desired_resource_config::<Worker>()?;
        if func_cfg.readiness_probe.is_none() || func_cfg.public_endpoints.is_empty() {
            self.storage_delivery_update_reconciliation_initialized = false;
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let url = self
            .url
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Readiness probe configured but URL missing after update".to_string(),
                })
            })?
            .clone();

        match run_readiness_probe(ctx, &url).await {
            Ok(()) => {
                self.storage_delivery_update_reconciliation_initialized = false;
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

    pub(super) async fn update_waiting_for_rbac_propagation_impl(
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
                "Waiting for Azure RBAC propagation before completing worker update"
            );
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        self.ready_rbac_wait_until_epoch_secs = None;
        self.update_rbac_wait_required = false;
        Ok(HandlerAction::Continue {
            state: UpdateRunningReadinessProbe,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
}
