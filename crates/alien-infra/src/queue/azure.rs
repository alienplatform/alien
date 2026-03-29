use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{Queue, QueueOutputs, ResourceOutputs, ResourceRef, ResourceStatus};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
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
        let _ = mgmt
            .get_queue(resource_group, namespace.clone(), queue.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Service Bus queue during heartbeat".to_string(),
                resource_id: Some(q.id.clone()),
            })?;
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
            Ok(Some(serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize binding parameters".to_string(),
                },
            )?))
        } else {
            Ok(None)
        }
    }
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
