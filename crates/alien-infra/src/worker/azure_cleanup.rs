use super::{
    get_azure_internal_commands_dapr_component_name, get_azure_storage_event_subscription_name,
    management_profile_dispatches_commands, AzureStorageTriggerInfrastructure,
    AzureStorageTriggerTeardownProgress, AzureWorkerController, CommandsTeardownResult,
    StorageTriggerTeardownResult,
};
use alien_azure_clients::authorization::Scope;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{ResourceRef, Worker};
use alien_error::{AlienError, Context, ContextError};
use std::time::Duration;
use tracing::info;

use crate::core::{AzurePermissionsHelper, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils::{
    get_container_apps_environment_outputs, get_resource_group_name,
};
use crate::worker::azure_dapr_components::{
    delete_dapr_component_if_owned, DaprComponentDeleteOperation,
};

pub(super) fn commands_queue_name(container_app_name: &str) -> String {
    format!("{container_app_name}-rq")
}

pub(super) fn storage_trigger_queue_name(container_app_name: &str, storage_id: &str) -> String {
    format!("{container_app_name}-storage-{storage_id}")
}

pub(super) fn service_bus_queue_scope(
    resource_group_name: &str,
    namespace_name: &str,
    queue_name: &str,
) -> Scope {
    Scope::Resource {
        resource_group_name: resource_group_name.to_string(),
        resource_provider: "Microsoft.ServiceBus".to_string(),
        parent_resource_path: Some(format!("namespaces/{namespace_name}")),
        resource_type: "queues".to_string(),
        resource_name: queue_name.to_string(),
    }
}

pub(super) fn commands_sender_role_assignment_name(
    resource_prefix: &str,
    worker_id: &str,
    principal_id: &str,
    namespace_name: &str,
    queue_name: &str,
) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!(
            "deployment:azure:commands-sender:{resource_prefix}:{worker_id}:{principal_id}:{namespace_name}:{queue_name}"
        )
        .as_bytes(),
    )
    .to_string()
}

pub(super) fn storage_trigger_receiver_role_assignment_name(
    resource_prefix: &str,
    worker_id: &str,
    storage_id: &str,
    principal_id: &str,
) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!(
            "deployment:azure:storage-trigger-receiver:{resource_prefix}:{worker_id}:{storage_id}:{principal_id}"
        )
        .as_bytes(),
    )
    .to_string()
}

fn commands_queue_cleanup_target(
    namespace_name: Option<String>,
    queue_name: Option<String>,
    worker_id: &str,
) -> Result<Option<(String, String)>> {
    match (namespace_name, queue_name) {
        (Some(namespace_name), Some(queue_name)) => Ok(Some((namespace_name, queue_name))),
        (None, None) => Ok(None),
        (namespace_name, queue_name) => Err(AlienError::new(
            ErrorData::ResourceControllerConfigError {
                resource_id: worker_id.to_string(),
                message: format!(
                    "Commands cleanup state is incomplete: namespace={namespace_name:?}, queue={queue_name:?}"
                ),
            },
        )),
    }
}

impl AzureWorkerController {
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
                    self.commands_dapr_component = None;
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

        if let Some(assignment_id) = self.commands_sender_role_assignment_id.clone() {
            let worker = ctx.desired_resource_config::<Worker>()?;
            let container_app_name = self.container_app_name.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: worker.id.clone(),
                    message: "Container app name not set in commands cleanup state".to_string(),
                })
            })?;
            let expected_assignment_id =
                Self::provable_commands_sender_role_assignment_id(ctx, worker, container_app_name)
                    .await?;
            if expected_assignment_id.as_deref() == Some(assignment_id.as_str()) {
                Self::delete_commands_role_assignment(ctx, &assignment_id, "sender").await?;
            } else {
                info!(
                    assignment_id,
                    "Discarding unprovable legacy commands sender cursor without deleting it"
                );
            }
            self.commands_sender_role_assignment_id = None;
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

        if let Some((namespace_name, queue_name)) = commands_queue_cleanup_target(
            self.commands_namespace_name.clone(),
            self.commands_queue_name.clone(),
            ctx.desired_config.id(),
        )? {
            let namespace_ref = ResourceRef::new(
                alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
                "default-service-bus-namespace",
            );
            let resource_group_name = match ctx
                .require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)
            {
                Ok(controller) => controller.resource_group_name(ctx)?,
                Err(_) => get_resource_group_name(ctx.state)?,
            };
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
            self.commands_namespace_name = None;
            self.commands_queue_name = None;
            return Ok(CommandsTeardownResult::Mutated);
        }

        Ok(CommandsTeardownResult::Complete)
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
                if let Some(assignment_id) = infrastructure.receiver_role_assignment_id {
                    let authorization_client = ctx
                        .service_provider
                        .get_azure_authorization_client(azure_config)?;
                    match authorization_client
                        .delete_role_assignment_by_id(assignment_id.clone())
                        .await
                    {
                        Ok(_) => info!(
                            assignment_id=%assignment_id,
                            "Deleted storage-trigger receiver role assignment"
                        ),
                        Err(error)
                            if matches!(
                                error.error,
                                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                            ) =>
                        {
                            info!(
                                assignment_id=%assignment_id,
                                "Storage-trigger receiver role assignment was already deleted"
                            );
                        }
                        Err(error) => {
                            return Err(error.context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to delete storage-trigger receiver role assignment '{}'",
                                    assignment_id
                                ),
                                resource_id: Some(ctx.desired_config.id().to_string()),
                            }));
                        }
                    }
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
        if !worker.commands_enabled && !has_storage_triggers {
            self.auxiliary_teardown_candidates_initialized = true;
            return Ok(true);
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

        if has_storage_triggers {
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
                let storage_controller = ctx
                    .require_dependency::<crate::storage::azure::AzureStorageController>(storage)?;
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
                if self
                    .storage_trigger_infrastructure
                    .iter()
                    .any(|candidate| candidate.event_subscription_name == event_subscription_name)
                {
                    continue;
                }

                let queue_scope = service_bus_queue_scope(
                    &service_bus_resource_group,
                    &namespace_name,
                    &queue_name,
                );
                let role_assignment_id = storage_trigger_receiver_role_assignment_name(
                    ctx.resource_prefix,
                    &worker.id,
                    &storage.id,
                    execution_principal_id,
                );
                self.storage_trigger_infrastructure
                    .push(AzureStorageTriggerInfrastructure {
                        source_resource_id: format!(
                            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}",
                            azure_config.subscription_id,
                            deployment_resource_group,
                            storage_account_name
                        ),
                        event_subscription_name,
                        service_bus_resource_group: service_bus_resource_group.clone(),
                        namespace_name: namespace_name.clone(),
                        queue_name,
                        receiver_role_assignment_id: Some(
                            authorization_client
                                .build_role_assignment_id(&queue_scope, role_assignment_id),
                        ),
                    });
            }
        }

        if worker.commands_enabled {
            self.initialize_commands_teardown_candidates(ctx, worker, container_app_name)
                .await?;
        }

        self.auxiliary_teardown_candidates_initialized = true;
        Ok(true)
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
        let queue_name = commands_queue_name(container_app_name);
        self.commands_namespace_name
            .get_or_insert_with(|| namespace_name.clone());
        self.commands_queue_name
            .get_or_insert_with(|| queue_name.clone());
        self.commands_dapr_component.get_or_insert_with(|| {
            get_azure_internal_commands_dapr_component_name(container_app_name)
        });

        self.commands_sender_role_assignment_id =
            Self::provable_commands_sender_role_assignment_id(ctx, worker, container_app_name)
                .await?;
        self.commands_receiver_role_assignment_id = None;

        Ok(())
    }

    async fn provable_commands_sender_role_assignment_id(
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
    ) -> Result<Option<String>> {
        if !management_profile_dispatches_commands(ctx, &worker.id)
            || AzurePermissionsHelper::get_management_uami_principal_id(ctx)?.is_some()
        {
            return Ok(None);
        }

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
        let azure_config = ctx.get_azure_config()?;
        let principal_id = ctx
            .service_provider
            .get_azure_caller_principal_id(azure_config)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to resolve imported Azure command sender principal".to_string(),
                resource_id: Some(worker.id.clone()),
            })?;
        let queue_name = commands_queue_name(container_app_name);
        let queue_scope = service_bus_queue_scope(
            &namespace_controller.resource_group_name(ctx)?,
            &namespace_name,
            &queue_name,
        );
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;
        Ok(Some(authorization_client.build_role_assignment_id(
            &queue_scope,
            commands_sender_role_assignment_name(
                ctx.resource_prefix,
                &worker.id,
                &principal_id,
                &namespace_name,
                &queue_name,
            ),
        )))
    }
}

#[cfg(test)]
#[path = "azure_cleanup_tests.rs"]
mod tests;
