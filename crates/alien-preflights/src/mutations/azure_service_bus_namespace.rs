//! Azure Service Bus Namespace mutation that adds the required namespace for queue resources.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    AzureServiceBusNamespace, DeploymentConfig, Platform, ResourceEntry, ResourceLifecycle,
    ResourceRef, Stack, StackState,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Mutation that adds AzureServiceBusNamespace resource for queue resources.
///
/// Queue resources on Azure use Service Bus, which requires a
/// Service Bus Namespace to be created first.
pub struct AzureServiceBusNamespaceMutation;

#[async_trait]
impl StackMutation for AzureServiceBusNamespaceMutation {
    fn description(&self) -> &'static str {
        "Add Azure Service Bus Namespace required by Queue resources"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Only add for Azure platform
        if stack_state.platform != Platform::Azure {
            return false;
        }

        // Check if we have queue resources that need Service Bus Namespace
        let has_queue_resources = stack.resources.iter().any(|(_, entry)| {
            let resource_type = entry.config.resource_type();
            matches!(resource_type.as_ref(), "queue")
        });

        if !has_queue_resources {
            return false;
        }

        // Check if AzureServiceBusNamespace already exists
        let namespace_id = "default-service-bus-namespace";
        !stack.resources.iter().any(|(id, _)| id == namespace_id)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding AzureServiceBusNamespace resource for Azure queues");

        let namespace_id = "default-service-bus-namespace";

        // Create the AzureServiceBusNamespace resource
        let service_bus_namespace = AzureServiceBusNamespace::new(namespace_id.to_string()).build();

        // Dependencies: resource group and Microsoft.ServiceBus service activation
        let dependencies = vec![
            ResourceRef::new(
                alien_core::AzureResourceGroup::RESOURCE_TYPE,
                "default-resource-group",
            ),
            ResourceRef::new(
                alien_core::ServiceActivation::RESOURCE_TYPE,
                "enable-servicebus",
            ),
        ];

        // Add it to the stack as a frozen resource
        let namespace_entry = ResourceEntry {
            config: alien_core::Resource::new(service_bus_namespace),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies,
            remote_access: false,
        };

        stack
            .resources
            .insert(namespace_id.to_string(), namespace_entry);

        debug!("Added AzureServiceBusNamespace resource '{}'", namespace_id);

        Ok(stack)
    }
}
