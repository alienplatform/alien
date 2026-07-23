use std::time::Duration;

use alien_azure_clients::event_grid::{
    EventSubscriptionFilter, EventSubscriptionRequest, EventSubscriptionRequestProperties,
    ServiceBusQueueDestination, ServiceBusQueueDestinationProperties,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{ResourceRef, Worker, WorkerTrigger};
use alien_error::{AlienError, Context, ContextError};
use tracing::info;

use super::{
    get_azure_storage_event_subscription_name, AzureStorageTriggerInfrastructure,
    AzureStorageTriggerTeardownProgress, AzureWorkerController,
};
use crate::core::{AzurePermissionsHelper, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils::get_resource_group_name;
use crate::worker::azure::role_assignments::discover_proven_role_assignments;
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

pub(super) enum StorageTargetPreparation {
    Ready,
    Pending,
}

pub(super) enum StorageDeliveryReconcileResult {
    Complete,
    Pending(Duration),
}

pub(super) fn azure_storage_event_types(events: &[String], worker_id: &str) -> Result<Vec<String>> {
    events
        .iter()
        .map(|event| {
            let event_type = match event.as_str() {
                "created" => "Microsoft.Storage.BlobCreated",
                "deleted" => "Microsoft.Storage.BlobDeleted",
                "tierChanged" => "Microsoft.Storage.BlobTierChanged",
                _ => {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "Azure storage trigger event '{}' is not supported; expected one of: created, deleted, tierChanged",
                            event
                        ),
                        resource_id: Some(worker_id.to_string()),
                    }));
                }
            };
            Ok(event_type.to_string())
        })
        .collect()
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

    pub(super) async fn prepare_storage_trigger_target(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        desired: &AzureStorageTriggerInfrastructure,
    ) -> Result<StorageTargetPreparation> {
        let tracker_index = self
            .storage_trigger_infrastructure
            .iter()
            .position(|item| item.event_subscription_name == desired.event_subscription_name);
        match tracker_index {
            Some(index) if !self.storage_trigger_infrastructure[index].matches_target(desired) => {
                // A dependency may rotate after the old exact target was
                // checkpointed but before its first remote mutation. Finish
                // teardown from that durable cursor, then checkpoint the
                // newly resolved target on a later controller step.
                if self.storage_trigger_teardown_progress
                    == AzureStorageTriggerTeardownProgress::EventSubscription
                    && index != 0
                {
                    self.storage_trigger_infrastructure.swap(0, index);
                    return Ok(StorageTargetPreparation::Pending);
                }
                let _ = self.delete_storage_trigger_infrastructure(ctx).await?;
                Ok(StorageTargetPreparation::Pending)
            }
            Some(_) => Ok(StorageTargetPreparation::Ready),
            None => {
                self.storage_trigger_infrastructure.push(desired.clone());
                Ok(StorageTargetPreparation::Pending)
            }
        }
    }

    pub(super) async fn ensure_storage_delivery_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        storage_ref: &ResourceRef,
        events: &[String],
        desired: &AzureStorageTriggerTarget,
    ) -> Result<StorageDeliveryReconcileResult> {
        let azure_config = ctx.get_azure_config()?;
        let desired_infrastructure = &desired.infrastructure;
        let tracker_index = self
            .storage_trigger_infrastructure
            .iter()
            .position(|item| {
                item.event_subscription_name == desired_infrastructure.event_subscription_name
            })
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: worker.id.clone(),
                    message: format!(
                        "Storage-trigger target '{}' was not checkpointed",
                        desired_infrastructure.event_subscription_name
                    ),
                })
            })?;
        if !self.storage_trigger_infrastructure[tracker_index]
            .matches_target(desired_infrastructure)
        {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: worker.id.clone(),
                message: format!(
                    "Storage-trigger target '{}' changed during delivery reconciliation",
                    desired_infrastructure.event_subscription_name
                ),
            }));
        }
        if self.storage_trigger_infrastructure[tracker_index].delivery_reconciled {
            return Ok(StorageDeliveryReconcileResult::Complete);
        }

        let resource_group_name = &desired_infrastructure.service_bus_resource_group;
        let namespace_name = &desired_infrastructure.namespace_name;
        let queue_name = &desired_infrastructure.queue_name;
        if !self.storage_trigger_infrastructure[tracker_index].queue_applied {
            let service_bus_client = ctx
                .service_provider
                .get_azure_service_bus_management_client(azure_config)?;
            service_bus_client
                .create_or_update_queue(
                    resource_group_name.clone(),
                    namespace_name.clone(),
                    queue_name.clone(),
                    alien_azure_clients::models::queue::SbQueueProperties::default(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create storage-trigger Service Bus queue '{queue_name}'"
                    ),
                    resource_id: Some(worker.id.clone()),
                })?;
            self.storage_trigger_infrastructure[tracker_index].queue_applied = true;
            return Ok(StorageDeliveryReconcileResult::Pending(
                Duration::from_secs(1),
            ));
        }

        let storage_id = desired_infrastructure
            .storage_id
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: worker.id.clone(),
                    message: format!(
                        "Storage-trigger target '{}' has no storage ID",
                        desired_infrastructure.event_subscription_name
                    ),
                })
            })?;
        let queue_scope = service_bus_queue_scope(resource_group_name, namespace_name, queue_name);
        let role_definition_id =
            storage_trigger_receiver_role_definition_id(&azure_config.subscription_id);
        let proven_assignments = discover_proven_role_assignments(
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
        let desired_assignment_id = desired_infrastructure
            .receiver_role_assignment_id
            .as_deref()
            .expect("desired storage target includes a receiver role assignment");
        let mut desired_assignment_found = false;
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;
        for assignment in proven_assignments {
            if assignment.id.eq_ignore_ascii_case(desired_assignment_id) {
                desired_assignment_found = true;
                continue;
            }
            match authorization_client
                .delete_role_assignment_by_id(assignment.id.clone())
                .await
            {
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete stale storage receiver role assignment '{}'",
                            assignment.id
                        ),
                        resource_id: Some(worker.id.clone()),
                    }));
                }
            }
            return Ok(StorageDeliveryReconcileResult::Pending(
                Duration::from_secs(1),
            ));
        }
        if !desired_assignment_found {
            AzurePermissionsHelper::create_role_assignment(
                &authorization_client,
                azure_config,
                &queue_scope,
                &desired.receiver_role_assignment_name,
                &desired.execution_principal_id,
                &role_definition_id,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to grant the worker access to storage-trigger queue '{queue_name}'"
                ),
                resource_id: Some(worker.id.clone()),
            })?;
            return Ok(StorageDeliveryReconcileResult::Pending(
                Duration::from_secs(1),
            ));
        }

        let container_name = desired_infrastructure
            .source_container_name
            .as_deref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: worker.id.clone(),
                    message: format!(
                        "Storage-trigger target '{}' has no source container",
                        desired_infrastructure.event_subscription_name
                    ),
                })
            })?;
        let queue_resource_id = format!(
            "/subscriptions/{}/resourceGroups/{resource_group_name}/providers/Microsoft.ServiceBus/namespaces/{namespace_name}/queues/{queue_name}",
            azure_config.subscription_id
        );
        let included_event_types = azure_storage_event_types(events, &worker.id)?;
        let event_grid_client = ctx
            .service_provider
            .get_azure_event_grid_client(azure_config)?;
        let event_subscription = event_grid_client
            .create_or_update_event_subscription(
                desired_infrastructure.source_resource_id.clone(),
                desired_infrastructure.event_subscription_name.clone(),
                EventSubscriptionRequest {
                    properties: EventSubscriptionRequestProperties {
                        destination: ServiceBusQueueDestination {
                            endpoint_type: "ServiceBusQueue".to_string(),
                            properties: ServiceBusQueueDestinationProperties {
                                resource_id: queue_resource_id,
                            },
                        },
                        filter: EventSubscriptionFilter {
                            included_event_types,
                            subject_begins_with: format!(
                                "/blobServices/default/containers/{container_name}/blobs/"
                            ),
                            is_subject_case_sensitive: false,
                        },
                        event_delivery_schema: "CloudEventSchemaV1_0".to_string(),
                    },
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to reconcile Event Grid subscription '{}' for storage '{}'",
                    desired_infrastructure.event_subscription_name, storage_ref.id
                ),
                resource_id: Some(worker.id.clone()),
            })?;
        let provisioning_state = event_subscription
            .properties
            .and_then(|properties| properties.provisioning_state);
        if provisioning_state
            .as_deref()
            .is_none_or(|state| !state.eq_ignore_ascii_case("Succeeded"))
        {
            info!(
                worker=%worker.id,
                subscription=%desired_infrastructure.event_subscription_name,
                state=?provisioning_state,
                "Waiting for exact Event Grid storage subscription"
            );
            return Ok(StorageDeliveryReconcileResult::Pending(
                Duration::from_secs(5),
            ));
        }

        self.storage_trigger_infrastructure[tracker_index].delivery_reconciled = true;
        Ok(StorageDeliveryReconcileResult::Pending(
            Duration::from_secs(1),
        ))
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
