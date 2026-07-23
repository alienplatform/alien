use super::{
    get_azure_storage_event_subscription_name, AzureStorageTriggerInfrastructure,
    AzureStorageTriggerTeardownProgress, AzureWorkerController, CommandsSenderReconcileResult,
    CommandsTeardownResult, StorageTriggerTeardownResult,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{ResourceRef, Worker};
use alien_error::{AlienError, ContextError};
use std::time::Duration;
use tracing::info;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils::{
    get_container_apps_environment_outputs, get_resource_group_name,
};
use crate::worker::azure::role_assignments::discover_proven_role_assignments;
use crate::worker::azure::trigger_targets::storage_trigger_receiver_role_definition_id;
use crate::worker::azure_dapr_components::{
    delete_dapr_component_if_owned, DaprComponentDeleteOperation,
};
use crate::worker::azure_dapr_names_migration::commands_component_removal_names;
use crate::worker::azure_names::{
    commands_queue_name, service_bus_queue_scope, storage_trigger_queue_name,
    storage_trigger_receiver_role_assignment_name,
};

fn commands_queue_cleanup_target(
    resource_group_name: Option<String>,
    namespace_name: Option<String>,
    queue_name: Option<String>,
    worker_id: &str,
) -> Result<Option<(String, String, String)>> {
    match (resource_group_name, namespace_name, queue_name) {
        (Some(resource_group_name), Some(namespace_name), Some(queue_name)) => {
            Ok(Some((resource_group_name, namespace_name, queue_name)))
        }
        (None, None, None) => Ok(None),
        (resource_group_name, namespace_name, queue_name) => Err(AlienError::new(
            ErrorData::ResourceControllerConfigError {
                resource_id: worker_id.to_string(),
                message: format!(
                    "Commands cleanup state is incomplete: resource_group={resource_group_name:?}, namespace={namespace_name:?}, queue={queue_name:?}"
                ),
            },
        )),
    }
}

pub(super) struct AzureCommandsQueueTarget {
    pub(super) resource_group_name: String,
    pub(super) namespace_name: String,
    pub(super) queue_name: String,
}

pub(super) enum CommandsQueueTargetPreparation {
    Ready,
    Checkpoint,
    LongRunning(Duration),
}

impl AzureWorkerController {
    pub(super) fn commands_cleanup_target(
        &self,
        worker_id: &str,
    ) -> Result<Option<(String, String, String)>> {
        commands_queue_cleanup_target(
            self.commands_resource_group_name.clone(),
            self.commands_namespace_name.clone(),
            self.commands_queue_name.clone(),
            worker_id,
        )
    }

    pub(super) async fn prepare_commands_target_for_setup(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
        desired: &AzureCommandsQueueTarget,
    ) -> Result<CommandsQueueTargetPreparation> {
        // Controllers written before commands resource-group tracking contain
        // exactly namespace+queue. Preserve that historical target first,
        // even when the namespace dependency has since rotated, so teardown
        // can run against the old namespace instead of deadlocking on a
        // malformed partial triple. Other partial shapes remain fail-closed.
        if self.commands_resource_group_name.is_none()
            && self.commands_namespace_name.is_some()
            && self.commands_queue_name.is_some()
        {
            let namespace_ref = ResourceRef::new(
                alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
                "default-service-bus-namespace",
            );
            let namespace_controller = ctx.require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)?;
            self.commands_resource_group_name =
                Some(namespace_controller.resource_group_name(ctx)?);
            self.commands_queue_applied = false;
            self.commands_sender_role_assignment_discovery_complete = false;
            return Ok(CommandsQueueTargetPreparation::Checkpoint);
        }

        let target_field_count = [
            self.commands_resource_group_name.is_some(),
            self.commands_namespace_name.is_some(),
            self.commands_queue_name.is_some(),
        ]
        .into_iter()
        .filter(|is_some| *is_some)
        .count();
        if target_field_count < 3
            && self
                .commands_resource_group_name
                .as_ref()
                .is_none_or(|tracked| tracked == &desired.resource_group_name)
            && self
                .commands_namespace_name
                .as_ref()
                .is_none_or(|tracked| tracked == &desired.namespace_name)
            && self
                .commands_queue_name
                .as_ref()
                .is_none_or(|tracked| tracked == &desired.queue_name)
        {
            self.commands_resource_group_name
                .get_or_insert_with(|| desired.resource_group_name.clone());
            self.commands_namespace_name
                .get_or_insert_with(|| desired.namespace_name.clone());
            self.commands_queue_name
                .get_or_insert_with(|| desired.queue_name.clone());
            self.commands_queue_applied = false;
            self.commands_sender_role_assignment_discovery_complete = false;
            return Ok(CommandsQueueTargetPreparation::Checkpoint);
        }

        let Some((tracked_resource_group, tracked_namespace, tracked_queue)) =
            self.commands_cleanup_target(&worker.id)?
        else {
            return Ok(CommandsQueueTargetPreparation::Ready);
        };
        if tracked_resource_group == desired.resource_group_name
            && tracked_namespace == desired.namespace_name
            && tracked_queue == desired.queue_name
        {
            return Ok(CommandsQueueTargetPreparation::Ready);
        }

        if !self.commands_update_teardown_candidates_initialized {
            self.initialize_commands_teardown_candidates(ctx, worker, container_app_name)
                .await?;
            self.commands_update_teardown_candidates_initialized = true;
            return Ok(CommandsQueueTargetPreparation::Checkpoint);
        }
        match self.delete_commands_infrastructure_step(ctx).await? {
            CommandsTeardownResult::Complete => {
                self.commands_update_teardown_candidates_initialized = false;
                Ok(CommandsQueueTargetPreparation::Checkpoint)
            }
            CommandsTeardownResult::Mutated => Ok(CommandsQueueTargetPreparation::Checkpoint),
            CommandsTeardownResult::LongRunning(delay) => {
                Ok(CommandsQueueTargetPreparation::LongRunning(delay))
            }
        }
    }

    pub(super) async fn delete_commands_infrastructure_step(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<CommandsTeardownResult> {
        if let Some(component_name) = self.commands_dapr_component.clone() {
            let env_outputs = get_container_apps_environment_outputs(ctx.state)?;
            let worker = ctx.desired_resource_config::<Worker>()?;
            let container_app_name = self.container_app_name.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: worker.id.clone(),
                    message: "Container app name not set in state".to_string(),
                })
            })?;
            let client = ctx
                .service_provider
                .get_azure_container_apps_client(ctx.get_azure_config()?)?;

            match delete_dapr_component_if_owned(
                client.as_ref(),
                &env_outputs.resource_group_name,
                &env_outputs.environment_name,
                container_app_name,
                &component_name,
                &worker.id,
            )
            .await?
            {
                DaprComponentDeleteOperation::NotFound
                | DaprComponentDeleteOperation::Foreign
                | DaprComponentDeleteOperation::Completed => {
                    self.complete_commands_dapr_component_deletion();
                    return Ok(CommandsTeardownResult::Mutated);
                }
                DaprComponentDeleteOperation::LongRunning(lro) => {
                    let delay = lro.retry_after.unwrap_or(Duration::from_secs(15));
                    self.pending_operation_url = Some(lro.url);
                    self.pending_operation_retry_after =
                        lro.retry_after.map(|retry_after| retry_after.as_secs());
                    return Ok(CommandsTeardownResult::LongRunning(delay));
                }
            }
        }

        let worker = ctx.desired_resource_config::<Worker>()?;
        if !matches!(
            self.delete_commands_sender_role_assignment_step(ctx, worker)
                .await?,
            CommandsSenderReconcileResult::Complete
        ) {
            return Ok(CommandsTeardownResult::Mutated);
        }

        if let Some(assignment_id) = self.commands_receiver_role_assignment_id.clone() {
            info!(
                assignment_id,
                "Discarding setup-owned commands receiver cursor without deleting it"
            );
            self.commands_receiver_role_assignment_id = None;
            return Ok(CommandsTeardownResult::Mutated);
        }

        if let Some((resource_group_name, namespace_name, queue_name)) =
            self.commands_cleanup_target(ctx.desired_config.id())?
        {
            info!(namespace=%namespace_name, queue=%queue_name, "Deleting commands Service Bus queue");
            let management_client = ctx
                .service_provider
                .get_azure_service_bus_management_client(ctx.get_azure_config()?)?;
            match management_client
                .delete_queue(
                    resource_group_name,
                    namespace_name.clone(),
                    queue_name.clone(),
                )
                .await
            {
                Ok(_) => info!(queue=%queue_name, "Commands Service Bus queue deleted"),
                Err(error)
                    if matches!(
                        error.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(queue=%queue_name, "Commands Service Bus queue was already deleted");
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete commands Service Bus queue '{queue_name}'"
                        ),
                        resource_id: Some(ctx.desired_config.id().to_string()),
                    }));
                }
            }
            self.commands_resource_group_name = None;
            self.commands_namespace_name = None;
            self.commands_queue_name = None;
            self.commands_queue_applied = false;
            self.commands_sender_role_assignment_discovery_complete = false;
            return Ok(CommandsTeardownResult::Mutated);
        }

        Ok(CommandsTeardownResult::Complete)
    }

    pub(super) fn complete_commands_dapr_component_deletion(&mut self) {
        if let Some(component_name) = self.commands_dapr_component.take() {
            self.commands_dapr_component_deletion_candidates
                .retain(|candidate| candidate != &component_name);
        }
        self.commands_dapr_component = self
            .commands_dapr_component_deletion_candidates
            .first()
            .cloned();
    }

    pub(super) async fn delete_commands_role_assignment(
        ctx: &ResourceControllerContext<'_>,
        assignment_id: &str,
        role: &str,
    ) -> Result<()> {
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(ctx.get_azure_config()?)?;
        match authorization_client
            .delete_role_assignment_by_id(assignment_id.to_string())
            .await
        {
            Ok(_) => {
                info!(assignment_id, role, "Commands role assignment deleted");
                Ok(())
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(
                    assignment_id,
                    role, "Commands role assignment was already deleted"
                );
                Ok(())
            }
            Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete commands {role} role assignment"),
                resource_id: Some(ctx.desired_config.id().to_string()),
            })),
        }
    }

    pub(super) async fn delete_storage_trigger_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<StorageTriggerTeardownResult> {
        let Some(infrastructure) = self.storage_trigger_infrastructure.first().cloned() else {
            self.storage_trigger_teardown_progress = AzureStorageTriggerTeardownProgress::default();
            return Ok(StorageTriggerTeardownResult::Complete);
        };

        let azure_config = ctx.get_azure_config()?;
        match self.storage_trigger_teardown_progress {
            AzureStorageTriggerTeardownProgress::EventSubscription => {
                let event_grid_client = ctx
                    .service_provider
                    .get_azure_event_grid_client(azure_config)?;
                match event_grid_client
                    .delete_event_subscription(
                        infrastructure.source_resource_id.clone(),
                        infrastructure.event_subscription_name.clone(),
                    )
                    .await
                {
                    Ok(()) => info!(
                        subscription=%infrastructure.event_subscription_name,
                        "Deleted Event Grid storage subscription"
                    ),
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(
                            subscription=%infrastructure.event_subscription_name,
                            "Event Grid storage subscription was already deleted"
                        );
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete Event Grid storage subscription '{}'",
                                infrastructure.event_subscription_name
                            ),
                            resource_id: Some(ctx.desired_config.id().to_string()),
                        }));
                    }
                }
                self.storage_trigger_teardown_progress =
                    AzureStorageTriggerTeardownProgress::ReceiverRoleAssignment;
            }
            AzureStorageTriggerTeardownProgress::ReceiverRoleAssignment => {
                let Some(storage_id) = infrastructure.storage_id.as_deref() else {
                    self.storage_trigger_teardown_progress =
                        AzureStorageTriggerTeardownProgress::Queue;
                    return Ok(StorageTriggerTeardownResult::Mutated);
                };
                let worker = ctx.desired_resource_config::<Worker>()?;
                let queue_scope = service_bus_queue_scope(
                    &infrastructure.service_bus_resource_group,
                    &infrastructure.namespace_name,
                    &infrastructure.queue_name,
                );
                let authorization_client = ctx
                    .service_provider
                    .get_azure_authorization_client(azure_config)?;
                let role_definition_id =
                    storage_trigger_receiver_role_definition_id(&azure_config.subscription_id);
                let assignments = discover_proven_role_assignments(
                    ctx,
                    &queue_scope,
                    &role_definition_id,
                    &worker.id,
                    "storage receiver",
                    |principal_id| {
                        storage_trigger_receiver_role_assignment_name(
                            ctx.resource_prefix,
                            &worker.id,
                            storage_id,
                            principal_id,
                        )
                    },
                )
                .await?;
                for assignment in assignments {
                    match authorization_client
                        .delete_role_assignment_by_id(assignment.id.clone())
                        .await
                    {
                        Ok(_) => info!(
                            assignment_id=%assignment.id,
                            "Deleted proven storage-trigger receiver role assignment"
                        ),
                        Err(error)
                            if matches!(
                                error.error,
                                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                            ) =>
                        {
                            info!(
                                assignment_id=%assignment.id,
                                "Proven storage-trigger receiver role assignment was already deleted"
                            );
                        }
                        Err(error) => {
                            return Err(error.context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to delete proven storage-trigger receiver role assignment '{}'",
                                    assignment.id
                                ),
                                resource_id: Some(worker.id.clone()),
                            }));
                        }
                    }
                    return Ok(StorageTriggerTeardownResult::Mutated);
                }
                self.storage_trigger_teardown_progress = AzureStorageTriggerTeardownProgress::Queue;
            }
            AzureStorageTriggerTeardownProgress::Queue => {
                let service_bus_client = ctx
                    .service_provider
                    .get_azure_service_bus_management_client(azure_config)?;
                match service_bus_client
                    .delete_queue(
                        infrastructure.service_bus_resource_group,
                        infrastructure.namespace_name,
                        infrastructure.queue_name.clone(),
                    )
                    .await
                {
                    Ok(()) => info!(
                        queue=%infrastructure.queue_name,
                        "Deleted storage-trigger Service Bus queue"
                    ),
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(
                            queue=%infrastructure.queue_name,
                            "Storage-trigger Service Bus queue was already deleted"
                        );
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete storage-trigger Service Bus queue '{}'",
                                infrastructure.queue_name
                            ),
                            resource_id: Some(ctx.desired_config.id().to_string()),
                        }));
                    }
                }
                self.storage_trigger_infrastructure.remove(0);
                self.storage_trigger_teardown_progress =
                    AzureStorageTriggerTeardownProgress::default();
            }
        }

        Ok(StorageTriggerTeardownResult::Mutated)
    }

    pub(super) async fn initialize_auxiliary_teardown_candidates(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
    ) -> Result<bool> {
        if self.auxiliary_teardown_candidates_initialized {
            return Ok(false);
        }

        let has_storage_triggers = worker
            .triggers
            .iter()
            .any(|trigger| matches!(trigger, alien_core::WorkerTrigger::Storage { .. }));
        let has_commands_candidates = worker.commands_enabled
            || self.commands_resource_group_name.is_some()
            || self.commands_namespace_name.is_some()
            || self.commands_queue_name.is_some()
            || self.commands_dapr_component.is_some()
            || !self.commands_dapr_component_deletion_candidates.is_empty()
            || self.commands_sender_role_assignment_id.is_some()
            || self.commands_sender_role_assignment_intent.is_some()
            || self.commands_receiver_role_assignment_id.is_some();
        if !has_commands_candidates && !has_storage_triggers {
            self.auxiliary_teardown_candidates_initialized = true;
            return Ok(true);
        }

        if has_storage_triggers {
            self.initialize_storage_trigger_teardown_candidates(ctx, worker, container_app_name)
                .await?;
        }

        if has_commands_candidates {
            self.initialize_commands_teardown_candidates(ctx, worker, container_app_name)
                .await?;
        }

        self.auxiliary_teardown_candidates_initialized = true;
        Ok(true)
    }

    pub(super) async fn initialize_storage_trigger_teardown_candidates(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
    ) -> Result<()> {
        if !worker
            .triggers
            .iter()
            .any(|trigger| matches!(trigger, alien_core::WorkerTrigger::Storage { .. }))
        {
            return Ok(());
        }

        let azure_config = ctx.get_azure_config()?;
        let namespace_ref = ResourceRef::new(
            alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
            "default-service-bus-namespace",
        );
        let namespace_controller = ctx.require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)?;
        let namespace_name = namespace_controller.namespace_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker.id.clone(),
                dependency_id: namespace_ref.id.clone(),
            })
        })?;
        let service_bus_resource_group = namespace_controller.resource_group_name(ctx)?;
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            format!("{}-sa", worker.get_permissions()),
        );
        let service_account = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )?;
        let execution_principal_id = service_account
            .identity_principal_id
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker.id.clone(),
                    dependency_id: service_account_ref.id.clone(),
                })
            })?;
        let deployment_resource_group = get_resource_group_name(ctx.state)?;

        for trigger in &worker.triggers {
            let alien_core::WorkerTrigger::Storage { storage, .. } = trigger else {
                continue;
            };
            let storage_controller =
                ctx.require_dependency::<crate::storage::azure::AzureStorageController>(storage)?;
            let storage_account_name = storage_controller
                .storage_account_name
                .as_deref()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::DependencyNotReady {
                        resource_id: worker.id.clone(),
                        dependency_id: storage.id.clone(),
                    })
                })?;
            let queue_name = storage_trigger_queue_name(container_app_name, &storage.id);
            let event_subscription_name =
                get_azure_storage_event_subscription_name(&worker.id, &storage.id);
            if let Some(candidate) = self
                .storage_trigger_infrastructure
                .iter_mut()
                .find(|candidate| candidate.event_subscription_name == event_subscription_name)
            {
                if candidate.storage_id.is_none() {
                    candidate.storage_id = Some(storage.id.clone());
                }
                continue;
            }

            let queue_scope =
                service_bus_queue_scope(&service_bus_resource_group, &namespace_name, &queue_name);
            let role_assignment_id = storage_trigger_receiver_role_assignment_name(
                ctx.resource_prefix,
                &worker.id,
                &storage.id,
                execution_principal_id,
            );
            self.storage_trigger_infrastructure
                .push(AzureStorageTriggerInfrastructure {
                    storage_id: Some(storage.id.clone()),
                    source_resource_id: format!(
                        "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}",
                        azure_config.subscription_id,
                        deployment_resource_group,
                        storage_account_name
                    ),
                    source_container_name: storage_controller.container_name.clone(),
                    event_subscription_name,
                    service_bus_resource_group: service_bus_resource_group.clone(),
                    namespace_name: namespace_name.clone(),
                    queue_name,
                    queue_applied: false,
                    receiver_role_assignment_id: Some(
                        authorization_client.build_role_assignment_id(
                            &queue_scope,
                            role_assignment_id,
                        ),
                    ),
                    delivery_reconciled: false,
                });
        }

        Ok(())
    }

    pub(super) async fn initialize_commands_teardown_candidates(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
    ) -> Result<()> {
        let namespace_ref = ResourceRef::new(
            alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
            "default-service-bus-namespace",
        );
        let namespace_controller = ctx.require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)?;
        let namespace_name = namespace_controller.namespace_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker.id.clone(),
                dependency_id: namespace_ref.id.clone(),
            })
        })?;
        let resource_group_name = namespace_controller.resource_group_name(ctx)?;
        let queue_name = commands_queue_name(container_app_name);
        self.commands_resource_group_name
            .get_or_insert(resource_group_name);
        self.commands_namespace_name
            .get_or_insert_with(|| namespace_name.clone());
        self.commands_queue_name
            .get_or_insert_with(|| queue_name.clone());
        self.commands_queue_applied = false;
        self.commands_dapr_component_deletion_candidates = commands_component_removal_names(
            container_app_name,
            self.commands_dapr_component.as_deref(),
        );
        self.commands_dapr_component = self
            .commands_dapr_component_deletion_candidates
            .first()
            .cloned();

        self.commands_receiver_role_assignment_id = None;
        self.commands_sender_role_assignment_discovery_complete = false;

        Ok(())
    }
}

#[cfg(test)]
#[path = "azure_cleanup_tests.rs"]
mod tests;
