//! Azure Resource Group mutation that adds the required resource group for Azure resources.

use crate::error::Result;
use crate::mutations::runs_on_platform_or_base;
use crate::StackMutation;
use alien_core::{
    AzureResourceGroup, DeploymentConfig, Platform, ResourceEntry, ResourceLifecycle, Stack,
    StackState,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Mutation that adds AzureResourceGroup resource for Azure platform.
///
/// All Azure resources need a resource group, so this adds a default
/// resource group that other resources can depend on.
pub struct AzureResourceGroupMutation;

#[async_trait]
impl StackMutation for AzureResourceGroupMutation {
    fn description(&self) -> &'static str {
        "Add Azure Resource Group required by all Azure resources"
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

        let include_azure_workload_scaffolding = stack_state.platform == Platform::Azure;

        // Check if we have any user-defined resources that require Azure setup.
        let has_user_resources = stack.resources.iter().any(|(_, entry)| {
            let resource_type = entry.config.resource_type();
            match resource_type.as_ref() {
                "worker" | "build" => include_azure_workload_scaffolding,
                "storage" | "vault" | "kv" | "artifact-registry" | "queue" => true,
                _ => false,
            }
        });

        if !has_user_resources {
            return false;
        }

        // Check if AzureResourceGroup already exists
        let resource_group_id = "default-resource-group";
        !stack
            .resources
            .iter()
            .any(|(id, _)| id == resource_group_id)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding AzureResourceGroup resource for Azure platform");

        let resource_group_id = "default-resource-group";

        // Create the AzureResourceGroup resource
        let resource_group = AzureResourceGroup::new(resource_group_id.to_string()).build();

        // Add it to the stack as a frozen resource (created once during setup)
        let resource_group_entry = ResourceEntry {
            config: alien_core::Resource::new(resource_group),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(), // No dependencies on other resources
            remote_access: false,
        };

        stack
            .resources
            .insert(resource_group_id.to_string(), resource_group_entry);

        debug!("Added AzureResourceGroup resource '{}'", resource_group_id);

        Ok(stack)
    }
}
