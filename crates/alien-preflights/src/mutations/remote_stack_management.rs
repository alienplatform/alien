//! RemoteStackManagement mutation that adds cross-account management resources.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    DeploymentConfig, DeploymentModel, Platform, RemoteStackManagement, ResourceEntry,
    ResourceLifecycle, Stack, StackState,
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

        // Only add RemoteStackManagement for cross-account platforms (and test
        // for development). Kubernetes setup can also need a cloud management
        // identity for Helm pull mode; in that case the cloud is carried as the
        // deployment config's base platform.
        let base_platform = config.base_platform.unwrap_or(platform);
        if !matches!(
            platform,
            Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Test
        ) && !(platform == Platform::Kubernetes
            && matches!(
                base_platform,
                Platform::Aws | Platform::Gcp | Platform::Azure
            ))
        {
            return false;
        }

        let cloud_backed_kubernetes_pull = platform == Platform::Kubernetes
            && matches!(
                base_platform,
                Platform::Aws | Platform::Gcp | Platform::Azure
            )
            && config.stack_settings.deployment_model == DeploymentModel::Pull;

        // Push-mode cloud stacks need an external manager management config.
        // Cloud-backed Kubernetes pull stacks still need this setup-owned
        // identity, but the agent uses it from inside the cluster.
        if config.management_config.is_none() && !cloud_backed_kubernetes_pull {
            return false;
        }

        !stack
            .resources
            .values()
            .any(|entry| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
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

        let remote_mgmt_id = "management";

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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{EnvironmentVariablesSnapshot, ExternalBindings, StackSettings};

    fn empty_stack() -> Stack {
        Stack::new("test".to_string()).build()
    }

    fn config(
        base_platform: Option<Platform>,
        deployment_model: DeploymentModel,
    ) -> DeploymentConfig {
        DeploymentConfig {
            deployment_name: None,
            stack_settings: StackSettings {
                deployment_model,
                ..StackSettings::default()
            },
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
            },
            allow_frozen_changes: false,
            compute_backend: None,
            external_bindings: ExternalBindings::default(),
            base_platform,
            label_domain: None,
            public_endpoints: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
        }
    }

    #[test]
    fn cloud_backed_kubernetes_pull_runs_without_external_management_config() {
        let mutation = RemoteStackManagementMutation;
        let stack = empty_stack();
        let config = config(Some(Platform::Aws), DeploymentModel::Pull);

        assert!(mutation.should_run(&stack, &StackState::new(Platform::Kubernetes), &config));
    }

    #[test]
    fn cloud_backed_kubernetes_push_still_requires_external_management_config() {
        let mutation = RemoteStackManagementMutation;
        let stack = empty_stack();
        let config = config(Some(Platform::Aws), DeploymentModel::Push);

        assert!(!mutation.should_run(&stack, &StackState::new(Platform::Kubernetes), &config));
    }
}
