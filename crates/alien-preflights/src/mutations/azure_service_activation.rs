//! Azure Service Activation mutation that enables required Azure services.

use crate::error::Result;
use crate::mutations::runs_on_platform_or_base;
use crate::StackMutation;
use alien_core::{
    DeploymentConfig, Platform, ResourceEntry, ResourceLifecycle, ResourceRef, ServiceActivation,
    Stack, StackState,
};
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{debug, info};

/// Mutation that adds ServiceActivation resources for required Azure services.
///
/// Different Azure resource types require different Azure service providers to be enabled:
/// - worker, build: Microsoft.App
/// - worker with a storage trigger: Microsoft.EventGrid + Microsoft.ServiceBus
/// - storage, kv: Microsoft.Storage  
/// - vault: Microsoft.KeyVault
/// - artifact-registry: Microsoft.ContainerRegistry
/// - queue: Microsoft.ServiceBus
/// - postgres: Microsoft.DBforPostgreSQL + Microsoft.Network (private endpoint)
pub struct AzureServiceActivationMutation;

#[async_trait]
impl StackMutation for AzureServiceActivationMutation {
    fn description(&self) -> &'static str {
        "Enable required Azure service providers for resources"
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

        // Check what resource types exist in the stack that need service activation
        let required_services = self.get_required_services(stack, stack_state.platform);

        if required_services.is_empty() {
            return false;
        }

        // Check if all required service activations already exist
        let existing_services: std::collections::HashSet<_> = stack
            .resources
            .iter()
            .filter_map(|(_id, entry)| {
                if let Some(service) = entry.config.downcast_ref::<ServiceActivation>() {
                    Some(service.service_name.clone())
                } else {
                    None
                }
            })
            .collect();

        // Return true if any required service is missing
        required_services
            .values()
            .any(|service_name| !existing_services.contains(service_name))
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding Azure ServiceActivation resources");

        let required_services = self.get_required_services(&stack, stack_state.platform);
        let resource_group_ref = ResourceRef::new(
            alien_core::AzureResourceGroup::RESOURCE_TYPE,
            "default-resource-group",
        );

        for (service_id, service_name) in required_services {
            // Check if this service activation already exists
            if stack
                .resources
                .iter()
                .any(|(existing_id, _)| existing_id == &service_id)
            {
                continue;
            }

            // Create the ServiceActivation resource
            let service_activation = ServiceActivation::new(service_id.clone())
                .service_name(service_name.clone())
                .build();

            // Add it to the stack as a frozen resource
            let service_entry = ResourceEntry {
                config: alien_core::Resource::new(service_activation),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![resource_group_ref.clone()], // Depend on resource group
                remote_access: false,
            };

            stack.resources.insert(service_id.clone(), service_entry);
            debug!(
                "Added ServiceActivation resource '{}' for service '{}'",
                service_id, service_name
            );
        }

        Ok(stack)
    }
}

impl AzureServiceActivationMutation {
    /// Get the mapping of service activation ID to service name based on resources in the stack
    fn get_required_services(&self, stack: &Stack, platform: Platform) -> HashMap<String, String> {
        let mut services = HashMap::new();
        let include_azure_workload_scaffolding = platform == Platform::Azure;

        for (_, entry) in &stack.resources {
            let resource_type = entry.config.resource_type();
            match resource_type.as_ref() {
                "worker" | "build" if include_azure_workload_scaffolding => {
                    services.insert("enable-app".to_string(), "Microsoft.App".to_string());
                    if entry
                        .config
                        .downcast_ref::<alien_core::Worker>()
                        .is_some_and(|worker| {
                            worker.triggers.iter().any(|trigger| {
                                matches!(trigger, alien_core::WorkerTrigger::Storage { .. })
                            })
                        })
                    {
                        services.insert(
                            "enable-eventgrid".to_string(),
                            "Microsoft.EventGrid".to_string(),
                        );
                        services.insert(
                            "enable-servicebus".to_string(),
                            "Microsoft.ServiceBus".to_string(),
                        );
                    }
                }
                "storage" | "kv" => {
                    services.insert(
                        "enable-storage".to_string(),
                        "Microsoft.Storage".to_string(),
                    );
                }
                "vault" => {
                    services.insert(
                        "enable-keyvault".to_string(),
                        "Microsoft.KeyVault".to_string(),
                    );
                }
                "artifact-registry" => {
                    services.insert(
                        "enable-container-registry".to_string(),
                        "Microsoft.ContainerRegistry".to_string(),
                    );
                }
                "queue" => {
                    services.insert(
                        "enable-servicebus".to_string(),
                        "Microsoft.ServiceBus".to_string(),
                    );
                }
                "postgres" => {
                    services.insert(
                        "enable-postgresql".to_string(),
                        "Microsoft.DBforPostgreSQL".to_string(),
                    );
                    // The server is private-only: a Private Endpoint, a dedicated subnet, and a
                    // `privatelink.postgres.database.azure.com` private DNS zone — all
                    // `Microsoft.Network`. Shares the `network` arm's key so they dedupe.
                    services.insert(
                        "enable-network".to_string(),
                        "Microsoft.Network".to_string(),
                    );
                }
                "kubernetes-cluster" => {
                    services.insert(
                        "enable-container-service".to_string(),
                        "Microsoft.ContainerService".to_string(),
                    );
                    services.insert(
                        "enable-network".to_string(),
                        "Microsoft.Network".to_string(),
                    );
                }
                "network" => {
                    services.insert(
                        "enable-network".to_string(),
                        "Microsoft.Network".to_string(),
                    );
                }
                _ => {}
            }
        }

        services
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ResourceLifecycle, Storage, Worker, WorkerCode, WorkerTrigger};

    #[test]
    fn azure_storage_trigger_requires_event_grid_and_service_bus() {
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

        let services =
            AzureServiceActivationMutation.get_required_services(&stack, Platform::Azure);

        assert_eq!(
            services.get("enable-eventgrid").map(String::as_str),
            Some("Microsoft.EventGrid")
        );
        assert_eq!(
            services.get("enable-servicebus").map(String::as_str),
            Some("Microsoft.ServiceBus")
        );
    }
}
