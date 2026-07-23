use alien_core::{ResourceRef, Worker, WorkerTrigger};
use alien_error::AlienError;

use super::{
    get_azure_storage_event_subscription_name, AzureStorageTriggerInfrastructure,
    AzureWorkerController,
};
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils::get_resource_group_name;
use crate::worker::azure_names::{
    service_bus_queue_scope, storage_trigger_queue_name,
    storage_trigger_receiver_role_assignment_name,
};

const SERVICE_BUS_DATA_RECEIVER_ROLE_DEFINITION_GUID: &str = "4f6d3b9b-027b-4f4c-9142-0e5a2a2247e0";

pub(super) fn storage_trigger_receiver_role_definition_id(subscription_id: &str) -> String {
    format!(
        "/subscriptions/{subscription_id}/providers/Microsoft.Authorization/roleDefinitions/{SERVICE_BUS_DATA_RECEIVER_ROLE_DEFINITION_GUID}"
    )
}

pub(super) struct AzureStorageTriggerTarget {
    pub(super) infrastructure: AzureStorageTriggerInfrastructure,
    pub(super) execution_client_id: String,
    pub(super) execution_principal_id: String,
    pub(super) receiver_role_assignment_name: String,
}

impl AzureStorageTriggerInfrastructure {
    pub(super) fn matches_target(&self, desired: &Self) -> bool {
        self.storage_id == desired.storage_id
            && self.source_resource_id == desired.source_resource_id
            && self.source_container_name == desired.source_container_name
            && self.event_subscription_name == desired.event_subscription_name
            && self.service_bus_resource_group == desired.service_bus_resource_group
            && self.namespace_name == desired.namespace_name
            && self.queue_name == desired.queue_name
            && self.receiver_role_assignment_id == desired.receiver_role_assignment_id
    }
}

impl AzureWorkerController {
    pub(super) async fn desired_storage_trigger_target(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
        storage_ref: &ResourceRef,
    ) -> Result<AzureStorageTriggerTarget> {
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
        let storage_controller =
            ctx.require_dependency::<crate::storage::azure::AzureStorageController>(storage_ref)?;
        let storage_account_name = storage_controller
            .storage_account_name
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker.id.clone(),
                    dependency_id: storage_ref.id.clone(),
                })
            })?;
        let storage_container_name =
            storage_controller.container_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker.id.clone(),
                    dependency_id: storage_ref.id.clone(),
                })
            })?;
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            format!("{}-sa", worker.get_permissions()),
        );
        let service_account = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )?;
        let execution_client_id = service_account.identity_client_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: worker.id.clone(),
                dependency_id: service_account_ref.id.clone(),
            })
        })?;
        let execution_principal_id =
            service_account
                .identity_principal_id
                .clone()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::DependencyNotReady {
                        resource_id: worker.id.clone(),
                        dependency_id: service_account_ref.id.clone(),
                    })
                })?;
        let queue_name = storage_trigger_queue_name(container_app_name, &storage_ref.id);
        let event_subscription_name =
            get_azure_storage_event_subscription_name(&worker.id, &storage_ref.id);
        let deployment_resource_group = get_resource_group_name(ctx.state)?;
        let source_resource_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}",
            azure_config.subscription_id, deployment_resource_group, storage_account_name
        );
        let queue_scope =
            service_bus_queue_scope(&service_bus_resource_group, &namespace_name, &queue_name);
        let assignment_name = storage_trigger_receiver_role_assignment_name(
            ctx.resource_prefix,
            &worker.id,
            &storage_ref.id,
            &execution_principal_id,
        );
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;
        let receiver_role_assignment_id =
            authorization_client.build_role_assignment_id(&queue_scope, assignment_name.clone());

        Ok(AzureStorageTriggerTarget {
            infrastructure: AzureStorageTriggerInfrastructure {
                storage_id: Some(storage_ref.id.clone()),
                source_resource_id,
                source_container_name: Some(storage_container_name),
                event_subscription_name,
                service_bus_resource_group,
                namespace_name,
                queue_name,
                queue_applied: false,
                receiver_role_assignment_id: Some(receiver_role_assignment_id),
                delivery_reconciled: false,
            },
            execution_client_id,
            execution_principal_id,
            receiver_role_assignment_name: assignment_name,
        })
    }

    pub(super) async fn storage_trigger_targets_changed(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        container_app_name: &str,
    ) -> Result<bool> {
        let storage_refs: Vec<&ResourceRef> = worker
            .triggers
            .iter()
            .filter_map(|trigger| match trigger {
                WorkerTrigger::Storage { storage, .. } => Some(storage),
                _ => None,
            })
            .collect();
        if storage_refs.len() != self.storage_trigger_infrastructure.len() {
            return Ok(true);
        }

        for storage_ref in storage_refs {
            let desired = self
                .desired_storage_trigger_target(ctx, worker, container_app_name, storage_ref)
                .await?
                .infrastructure;
            let Some(tracked) = self
                .storage_trigger_infrastructure
                .iter()
                .find(|tracked| tracked.event_subscription_name == desired.event_subscription_name)
            else {
                return Ok(true);
            };
            if !tracked.matches_target(&desired) {
                return Ok(true);
            }
        }

        Ok(false)
    }
}
