//! Azure Service Bus Namespace mutation that adds the required namespace for queue resources.

use crate::error::Result;
use crate::mutations::runs_on_platform_or_base;
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
        config: &DeploymentConfig,
    ) -> bool {
        if !runs_on_platform_or_base(stack_state, config, Platform::Azure) {
            return false;
        }

        // Queue resources and Worker storage triggers use Service Bus as the
        // durable delivery hop for Azure events.
        let has_queue_resources = stack.resources.iter().any(|(_, entry)| {
            let resource_type = entry.config.resource_type();
            matches!(resource_type.as_ref(), "queue")
                || entry
                    .config
                    .downcast_ref::<alien_core::Worker>()
                    .is_some_and(|worker| {
                        worker.triggers.iter().any(|trigger| {
                            matches!(trigger, alien_core::WorkerTrigger::Storage { .. })
                        })
                    })
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
            enabled_when: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        EnvironmentVariablesSnapshot, ExternalBindings, StackSettings, Storage, Worker, WorkerCode,
        WorkerTrigger,
    };

    fn deployment_config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    #[tokio::test]
    async fn storage_trigger_adds_service_bus_namespace_without_queue_resource() {
        let storage = Storage::new("uploads".to_string()).build();
        let worker = Worker::new("processor".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("worker".to_string())
            .trigger(WorkerTrigger::storage(
                &storage,
                vec!["created".to_string()],
            ))
            .build();
        let stack = Stack::new("test".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .build();
        let state = StackState::new(Platform::Azure);
        let config = deployment_config();
        let mutation = AzureServiceBusNamespaceMutation;

        assert!(mutation.should_run(&stack, &state, &config));
        let mutated = mutation.mutate(stack, &state, &config).await.unwrap();
        let namespace = mutated
            .resources
            .get("default-service-bus-namespace")
            .expect("storage trigger Service Bus namespace");

        assert!(namespace.dependencies.contains(&ResourceRef::new(
            alien_core::ServiceActivation::RESOURCE_TYPE,
            "enable-servicebus",
        )));
    }
}
