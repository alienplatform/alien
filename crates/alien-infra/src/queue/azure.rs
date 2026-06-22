use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::models::queue::SbQueue;
use alien_core::{
    AzureServiceBusQueueHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, Queue, QueueHeartbeatData, QueueHeartbeatStatus, QueueOutputs,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceRef, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;
use std::time::Duration;
use tracing::info;

fn get_queue_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct AzureQueueController {
    pub(crate) namespace_name: Option<String>,
    pub(crate) queue_name: Option<String>,
}

#[controller]
impl AzureQueueController {
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.get_azure_config()?;
        let mgmt = ctx
            .service_provider
            .get_azure_service_bus_management_client(cfg)?;
        let q = ctx.desired_resource_config::<Queue>()?;

        // Get the namespace name from the dependent Azure Service Bus Namespace resource
        let namespace_ref = ResourceRef::new(
            alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
            "default-service-bus-namespace",
        );

        let namespace_controller = ctx.require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)?;
        let namespace_name = namespace_controller
            .namespace_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: q.id.clone(),
                    dependency_id: namespace_ref.id.clone(),
                })
            })?;

        self.namespace_name = Some(namespace_name.clone());
        let queue_name = get_queue_name(ctx.resource_prefix, &q.id);

        // Create the queue in the existing namespace
        let resource_group =
            crate::infra_requirements::azure_utils::get_resource_group_name(&ctx.state)?;
        mgmt.create_or_update_queue(
            resource_group,
            namespace_name.clone(),
            queue_name.clone(),
            alien_azure_clients::models::queue::SbQueueProperties {
                accessed_at: None,
                auto_delete_on_idle: None,
                count_details: None,
                created_at: None,
                dead_lettering_on_message_expiration: None,
                default_message_time_to_live: None,
                duplicate_detection_history_time_window: None,
                enable_batched_operations: None,
                enable_express: None,
                enable_partitioning: None,
                forward_dead_lettered_messages_to: None,
                forward_to: None,
                lock_duration: None,
                max_delivery_count: None,
                max_message_size_in_kilobytes: None,
                max_size_in_megabytes: None,
                message_count: None,
                requires_duplicate_detection: None,
                requires_session: None,
                size_in_bytes: None,
                status: None,
                updated_at: None,
            },
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create Service Bus queue '{}'", queue_name),
            resource_id: Some(q.id.clone()),
        })?;

        self.queue_name = Some(queue_name);
        info!(namespace_name=%namespace_name, queue_name=?self.queue_name, "Azure Service Bus queue created");

        Ok(HandlerAction::Continue {
            state: ApplyingPermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Queue>()?;

        info!(resource_id = %config.id(), "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let (Some(namespace_name), Some(queue_name)) = (&self.namespace_name, &self.queue_name) {
            use crate::core::ResourcePermissionsHelper;
            use alien_azure_clients::authorization::Scope;

            // Build Azure resource scope for the Service Bus queue
            let resource_scope = Scope::Resource {
                resource_group_name:
                    crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?,
                resource_provider: "Microsoft.ServiceBus".to_string(),
                parent_resource_path: Some(format!("namespaces/{}", namespace_name)),
                resource_type: "queues".to_string(),
                resource_name: queue_name.to_string(),
            };

            ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
                ctx,
                &config.id,
                queue_name,
                resource_scope,
                "Queue",
                "queue",
            )
            .await?;
        }

        info!(resource_id = %config.id(), "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.get_azure_config()?;
        let mgmt = ctx
            .service_provider
            .get_azure_service_bus_management_client(cfg)?;
        let q = ctx.desired_resource_config::<Queue>()?;
        let namespace = self.namespace_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Namespace not set in state".to_string(),
                resource_id: Some(q.id.clone()),
            })
        })?;
        let queue = self.queue_name.as_ref().unwrap();

        let resource_group =
            crate::infra_requirements::azure_utils::get_resource_group_name(&ctx.state)?;
        let queue_properties = mgmt
            .get_queue(resource_group, namespace.clone(), queue.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Service Bus queue during heartbeat".to_string(),
                resource_id: Some(q.id.clone()),
            })?;
        emit_azure_service_bus_queue_heartbeat(
            ctx,
            &q.id,
            namespace,
            queue,
            &crate::infra_requirements::azure_utils::get_resource_group_name(&ctx.state)?,
            queue_properties,
        );
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Queue>()?;
        info!(id=%config.id, "Azure Queue update (no-op — no mutable fields)");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let cfg = ctx.get_azure_config()?;
        let mgmt = ctx
            .service_provider
            .get_azure_service_bus_management_client(cfg)?;
        let _ = ctx.desired_resource_config::<Queue>()?;
        if let (Some(ns), Some(qn)) = (&self.namespace_name, &self.queue_name) {
            let resource_group =
                crate::infra_requirements::azure_utils::get_resource_group_name(&ctx.state)?;
            let _ = mgmt
                .delete_queue(resource_group, ns.clone(), qn.clone())
                .await;
        }
        self.namespace_name = None;
        self.queue_name = None;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(ns), Some(qn)) = (&self.namespace_name, &self.queue_name) {
            Some(ResourceOutputs::new(QueueOutputs {
                queue_name: qn.clone(),
                identifier: Some(format!("{}/{}", ns, qn)),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, QueueBinding};
        if let (Some(ns), Some(qn)) = (&self.namespace_name, &self.queue_name) {
            let binding = QueueBinding::service_bus(
                BindingValue::value(ns.clone()),
                BindingValue::value(qn.clone()),
            );
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

fn emit_azure_service_bus_queue_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    namespace_name: &str,
    queue_name: &str,
    resource_group: &str,
    queue: SbQueue,
) {
    let properties = queue.properties.unwrap_or_default();
    let count_details = properties.count_details.clone().unwrap_or_default();
    let active_message_count = nonnegative_i64_to_u64(count_details.active_message_count);
    let dead_letter_message_count = nonnegative_i64_to_u64(count_details.dead_letter_message_count);
    let scheduled_message_count = nonnegative_i64_to_u64(count_details.scheduled_message_count);
    let transfer_message_count = nonnegative_i64_to_u64(count_details.transfer_message_count);
    let transfer_dead_letter_message_count =
        nonnegative_i64_to_u64(count_details.transfer_dead_letter_message_count);
    let name = queue.name.unwrap_or_else(|| queue_name.to_string());

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Queue::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Queue(QueueHeartbeatData::AzureServiceBus(
            AzureServiceBusQueueHeartbeatData {
                status: QueueHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "Azure Service Bus queue '{}' metadata is reachable",
                        queue_name
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name,
                namespace_name: namespace_name.to_string(),
                resource_group: Some(resource_group.to_string()),
                resource_id: queue.id,
                endpoint: Some(format!("{}/{}", namespace_name, queue_name)),
                queue_status: properties.status.map(|status| status.to_string()),
                lock_duration: properties.lock_duration,
                max_delivery_count: nonnegative_i32_to_u32(properties.max_delivery_count),
                requires_duplicate_detection: properties.requires_duplicate_detection,
                duplicate_detection_history_time_window: properties
                    .duplicate_detection_history_time_window,
                requires_session: properties.requires_session,
                dead_lettering_on_message_expiration: properties
                    .dead_lettering_on_message_expiration,
                forward_dead_lettered_messages_to: properties.forward_dead_lettered_messages_to,
                forward_to: properties.forward_to,
                default_message_time_to_live: properties.default_message_time_to_live,
                auto_delete_on_idle: properties.auto_delete_on_idle,
                enable_batched_operations: properties.enable_batched_operations,
                enable_express: properties.enable_express,
                enable_partitioning: properties.enable_partitioning,
                max_message_size_in_kilobytes: nonnegative_i64_to_u64(
                    properties.max_message_size_in_kilobytes,
                ),
                max_size_in_megabytes: nonnegative_i32_to_u32(properties.max_size_in_megabytes),
                message_count: nonnegative_i64_to_u64(properties.message_count),
                active_message_count,
                dead_letter_message_count,
                scheduled_message_count,
                transfer_message_count,
                transfer_dead_letter_message_count,
                size_in_bytes: nonnegative_i64_to_u64(properties.size_in_bytes),
                accessed_at: properties.accessed_at,
                created_at: properties.created_at,
                updated_at: properties.updated_at,
            },
        )),
        raw: vec![],
    });
}

fn nonnegative_i64_to_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn nonnegative_i32_to_u32(value: Option<i32>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use alien_azure_clients::authorization::MockAuthorizationApi;
    use alien_azure_clients::service_bus::MockServiceBusManagementApi;
    use alien_core::{Platform, Queue, ResourceStatus};
    use std::sync::Arc;

    fn setup_mock_mgmt() -> Arc<MockServiceBusManagementApi> {
        let mut mock = MockServiceBusManagementApi::new();
        mock.expect_create_or_update_namespace()
            .returning(|_, _, _| {
                Ok(alien_azure_clients::models::queue_namespace::SbNamespace {
                    location: "eastus".to_string(),
                    tags: std::collections::HashMap::new(),
                    id: None,
                    name: None,
                    type_: None,
                    properties: None,
                    sku: None,
                    system_data: None,
                    identity: None,
                })
            });
        mock.expect_create_or_update_queue()
            .returning(|_, _, _, _| Ok(alien_azure_clients::models::queue::SbQueue::default()));
        mock.expect_get_queue()
            .returning(|_, _, _| Ok(alien_azure_clients::models::queue::SbQueue::default()));
        mock.expect_delete_queue().returning(|_, _, _| Ok(()));
        Arc::new(mock)
    }

    fn setup_mock_provider(
        mock_mgmt: Arc<MockServiceBusManagementApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_service_bus_management_client()
            .returning(move |_| Ok(mock_mgmt.clone()));

        // Mock authorization client for resource-scoped permissions
        provider
            .expect_get_azure_authorization_client()
            .returning(|_| Ok(Arc::new(MockAuthorizationApi::new())));

        Arc::new(provider)
    }

    #[tokio::test]
    async fn test_create_and_delete_servicebus_queue_succeeds() {
        let queue = Queue::new("azure-queue".to_string()).build();
        let mock_mgmt = setup_mock_mgmt();
        let mock_provider = setup_mock_provider(mock_mgmt);

        let mut executor = SingleControllerExecutor::builder()
            .resource(queue)
            .controller(AzureQueueController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }
}
