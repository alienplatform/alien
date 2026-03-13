//! RemoteStackManagement mutation that adds cross-account management resources.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    DeploymentConfig, Platform, RemoteStackManagement, ResourceEntry, ResourceLifecycle, Stack,
    StackState,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Mutation that adds RemoteStackManagement resource for cross-account platforms.
///
/// This enables external management of the stack from another cloud account on
/// AWS, GCP, and Azure platforms. For other platforms, cross-account management
/// is not applicable.
pub struct RemoteStackManagementMutation;

#[async_trait]
impl StackMutation for RemoteStackManagementMutation {
    fn description(&self) -> &'static str {
        "Add RemoteStackManagement resource for cross-account access"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        let platform = stack_state.platform;

        // Only add RemoteStackManagement for cross-account platforms (and test for development)
        if !matches!(
            platform,
            Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Test
        ) {
            return false;
        }

        // Only add if management is configured in deployment config
        if config.management_config.is_none() {
            return false;
        }

        // Check if RemoteStackManagement already exists
        let remote_mgmt_id = "remote-stack-management";
        !stack.resources.iter().any(|(id, _)| id == remote_mgmt_id)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let platform = stack_state.platform;
        info!(
            "Adding RemoteStackManagement resource for platform {:?}",
            platform
        );

        let remote_mgmt_id = "remote-stack-management";

        // Create the RemoteStackManagement resource
        let remote_stack_management =
            RemoteStackManagement::new(remote_mgmt_id.to_string()).build();

        // Add it to the stack as a frozen resource (created once during setup)
        let remote_mgmt_entry = ResourceEntry {
            config: alien_core::Resource::new(remote_stack_management),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(), // No dependencies on other resources
            remote_access: false,
        };

        stack
            .resources
            .insert(remote_mgmt_id.to_string(), remote_mgmt_entry);

        debug!("Added RemoteStackManagement resource '{}'", remote_mgmt_id);

        Ok(stack)
    }
}
