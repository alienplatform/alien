//! Azure Container Apps Environment mutation that adds the required environment for function resources.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    AzureContainerAppsEnvironment, DeploymentConfig, Platform, ResourceEntry, ResourceLifecycle,
    ResourceRef, Stack, StackState,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Mutation that adds AzureContainerAppsEnvironment resource for function/build resources.
///
/// Functions and build resources on Azure run on Container Apps, which require a
/// Container Apps Environment to be created first.
pub struct AzureContainerAppsEnvironmentMutation;

#[async_trait]
impl StackMutation for AzureContainerAppsEnvironmentMutation {
    fn description(&self) -> &'static str {
        "Add Azure Container Apps Environment required by Function resources"
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

        // Check if we have function or build resources that need Container Apps Environment
        let has_container_resources = stack.resources.iter().any(|(_, entry)| {
            let resource_type = entry.config.resource_type();
            matches!(resource_type.as_ref(), "function" | "build")
        });

        if !has_container_resources {
            return false;
        }

        // Check if AzureContainerAppsEnvironment already exists
        let env_id = "default-container-env";
        !stack.resources.iter().any(|(id, _)| id == env_id)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding AzureContainerAppsEnvironment resource for Azure functions/builds");

        let env_id = "default-container-env";

        // Create the AzureContainerAppsEnvironment resource
        let container_env = AzureContainerAppsEnvironment::new(env_id.to_string()).build();

        // Dependencies: resource group and Microsoft.App service activation
        let dependencies = vec![
            ResourceRef::new(
                alien_core::AzureResourceGroup::RESOURCE_TYPE,
                "default-resource-group",
            ),
            ResourceRef::new(alien_core::ServiceActivation::RESOURCE_TYPE, "enable-app"),
        ];

        // Add it to the stack as a frozen resource
        let env_entry = ResourceEntry {
            config: alien_core::Resource::new(container_env),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies,
            remote_access: false,
        };

        stack.resources.insert(env_id.to_string(), env_entry);

        debug!("Added AzureContainerAppsEnvironment resource '{}'", env_id);

        Ok(stack)
    }
}
