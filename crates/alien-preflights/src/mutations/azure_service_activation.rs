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
/// - storage, kv: Microsoft.Storage  
/// - vault: Microsoft.KeyVault
/// - artifact-registry: Microsoft.ContainerRegistry
/// - queue: Microsoft.ServiceBus
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
